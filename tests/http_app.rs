//! H2 through the real app (headless): collections, environments, variables,
//! secrets, history and cURL import, persisted across app restarts.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RawInput};
use simpleeditor::app::{AfterSave, AppOptions, Dialog, HttpState, SimpleEditorApp, Tab};
use simpleeditor::cli::LaunchArgs;
use simpleeditor::http::model::{Auth, Variable};
use simpleeditor::http::secrets::{MemoryBackend, SecretBackend, secret_key};
use simpleeditor::storage::project::{self, Environment};
use support::TestServer;

struct Env {
    project: tempfile::TempDir,
    data: tempfile::TempDir,
    secrets: Arc<MemoryBackend>,
}

impl Env {
    fn new() -> Self {
        Self {
            project: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            secrets: Arc::new(MemoryBackend::default()),
        }
    }

    fn root(&self) -> PathBuf {
        self.project.path().to_path_buf()
    }

    /// Starts an app on the project and waits until it is loaded.
    fn app(&self, ctx: &egui::Context) -> SimpleEditorApp {
        let mut app = SimpleEditorApp::new(
            ctx,
            LaunchArgs {
                root: Some(self.root()),
                files: Vec::new(),
            },
            AppOptions {
                data_dir: Some(self.data.path().to_path_buf()),
                secrets: Some(self.secrets.clone() as Arc<dyn SecretBackend>),
                ..AppOptions::default()
            },
        );
        until(ctx, &mut app, |a| {
            a.project.as_ref().is_some_and(|p| p.loaded)
        });
        // Let the user-data load (history/state) land too.
        for _ in 0..20 {
            frame(ctx, &mut app);
            std::thread::sleep(Duration::from_millis(5));
        }
        app
    }
}

fn frame(ctx: &egui::Context, app: &mut SimpleEditorApp) {
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 860.0),
        )),
        ..RawInput::default()
    };
    app.poll();
    let mut out = ctx.run_ui(input, |ui| app.draw(ui));
    out.textures_delta.clear();
}

fn until(
    ctx: &egui::Context,
    app: &mut SimpleEditorApp,
    mut done: impl FnMut(&SimpleEditorApp) -> bool,
) {
    let start = Instant::now();
    while !done(app) {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "condition not reached"
        );
        frame(ctx, app);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn active_http(app: &mut SimpleEditorApp) -> &mut simpleeditor::app::HttpTab {
    let key = app.active.unwrap();
    app.http_tab_mut(key).expect("active HTTP tab")
}

fn send_and_wait(ctx: &egui::Context, app: &mut SimpleEditorApp) -> u16 {
    let key = app.active.unwrap();
    app.send_request(key);
    until(ctx, app, |a| {
        !matches!(
            a.http_tab(key).map(|t| &t.state),
            Some(HttpState::Sending { .. })
        )
    });
    match &app.http_tab(key).unwrap().state {
        HttpState::Done(v) => v.response.status,
        HttpState::Invalid(e) => panic!("invalid: {e:?}"),
        HttpState::Failed(e) => panic!("failed: {e}"),
        _ => panic!("unexpected state"),
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn collection_environment_and_history_survive_restart() {
    let server = TestServer::start();
    let env = Env::new();
    // An environment written as it would be by the Environments editor.
    project::save_environment(
        &env.root(),
        &Environment {
            id: "local".into(),
            name: "Local".into(),
            variables: vec![Variable::plain("base", server.url(""))],
        },
    )
    .unwrap();
    project::save_variables(&env.root(), &[Variable::plain("path", "echo-all")]).unwrap();

    let ctx = egui::Context::default();
    let mut app = env.app(&ctx);
    app.set_active_environment(Some("local".into()));
    app.new_request();
    {
        let t = active_http(&mut app);
        t.def.url = "{{base}}/{{path}}?q=1".into();
        t.def.auth = Auth::Bearer {
            token: "literal-token".into(),
        };
    }
    assert_eq!(send_and_wait(&ctx, &mut app), 200);
    assert!(!app.history.is_empty());

    // Save: no collection yet → the dialog asks; complete it.
    let key = app.active.unwrap();
    assert!(!app.save_request(key, AfterSave::Nothing));
    assert!(matches!(app.dialog, Some(Dialog::SaveRequest { .. })));
    app.dialog = None;
    app.save_request_as(key, None, "Users API", "Echo", AfterSave::Nothing);
    until(&ctx, &mut app, |a| !a.http_tab(key).unwrap().saving);
    assert!(!app.http_tab(key).unwrap().is_dirty());
    let file = project::collections_dir(&env.root()).join("users-api.json");
    let saved = read(&file);
    assert!(
        saved.contains("\"url\": \"{{base}}/{{path}}?q=1\""),
        "{saved}"
    );
    assert!(
        !saved.contains("literal-token"),
        "literal credentials not written"
    );
    assert!(app.notices.iter().any(|n| n.text.contains("Auth › Token")));
    // History and state files: sanitized and present.
    until(&ctx, &mut app, |_| {
        env.data.path().join("history.json").exists()
    });
    let history = read(&env.data.path().join("history.json"));
    assert!(!history.contains("literal-token"));
    drop(app);

    // Restart: collection, active environment and history come back.
    let mut app = env.app(&ctx);
    let p = app.project.as_ref().unwrap();
    assert_eq!(p.active_env.as_deref(), Some("local"));
    assert_eq!(p.data.collections[0].name, "Users API");
    let rid = p.data.collections[0].requests[0].id.clone();
    assert!(!app.history.is_empty(), "history reloaded");
    app.open_saved_request("users-api", &rid);
    assert_eq!(active_http(&mut app).def.name, "Echo");
    // The literal token was not persisted: the request now needs one.
    active_http(&mut app).def.auth = Auth::None;
    assert_eq!(send_and_wait(&ctx, &mut app), 200);
}

#[test]
fn secrets_are_remembered_only_on_request_and_never_written_to_files() {
    let server = TestServer::start();
    let env = Env::new();
    let ctx = egui::Context::default();
    let mut app = env.app(&ctx);

    // Use the Environments editor tab.
    app.open_environments();
    let key = app.active.unwrap();
    {
        let Some(Tab::Env(t)) = app.tabs.iter_mut().find(|t| t.key() == key) else {
            panic!()
        };
        t.variables = vec![
            Variable::plain("base", server.url("")),
            Variable {
                remember: true,
                ..Variable::secret("token", "remembered-secret")
            },
            Variable::secret("session_only", "volatile-secret"),
        ];
    }
    assert!(app.save_environments(key, AfterSave::Nothing));
    until(&ctx, &mut app, |a| !a.any_dirty() && !a.saves_in_flight());
    let vars_file = read(&project::variables_path(&env.root()));
    assert!(!vars_file.contains("remembered-secret") && !vars_file.contains("volatile-secret"));
    assert_eq!(
        env.secrets
            .get(&secret_key(&env.root(), "project", "token"))
            .unwrap()
            .as_deref(),
        Some("remembered-secret"),
        "stored in the credential store"
    );
    assert_eq!(
        env.secrets
            .get(&secret_key(&env.root(), "project", "session_only"))
            .unwrap(),
        None
    );

    // In this session both secrets resolve.
    app.new_request();
    active_http(&mut app).def.url = "{{base}}/echo-all".into();
    active_http(&mut app).def.headers = vec![
        simpleeditor::http::KeyValue::new("X-A", "{{token}}"),
        simpleeditor::http::KeyValue::new("X-B", "{{session_only}}"),
    ];
    assert_eq!(send_and_wait(&ctx, &mut app), 200);
    let history = read(&env.data.path().join("history.json"));
    assert!(!history.contains("remembered-secret") && !history.contains("volatile-secret"));
    drop(app);

    // After a restart only the remembered secret is available; the other
    // one blocks sending and names the field.
    let mut app = env.app(&ctx);
    app.new_request();
    active_http(&mut app).def.url = "{{base}}/echo-all".into();
    active_http(&mut app).def.headers = vec![simpleeditor::http::KeyValue::new("X-A", "{{token}}")];
    assert_eq!(send_and_wait(&ctx, &mut app), 200);
    active_http(&mut app).def.headers =
        vec![simpleeditor::http::KeyValue::new("X-B", "{{session_only}}")];
    let key = app.active.unwrap();
    app.send_request(key);
    let HttpState::Invalid(errors) = &app.http_tab(key).unwrap().state else {
        panic!("must be blocked")
    };
    assert_eq!(errors[0].field, "Headers › X-B");
    assert!(errors[0].message.contains("secret"));
}

#[test]
fn curl_import_opens_a_request_that_sends() {
    let server = TestServer::start();
    let env = Env::new();
    let ctx = egui::Context::default();
    let mut app = env.app(&ctx);
    let cmd = format!(
        "curl -X POST '{}' -H 'X-From: curl' --data-raw '{{\"a\":1}}' -H 'Content-Type: application/json' -k",
        server.url("/echo-all")
    );
    app.import_curl(&cmd).unwrap();
    assert!(app.notices.iter().any(|n| n.text.contains("--insecure")));
    assert_eq!(send_and_wait(&ctx, &mut app), 200);
    let key = app.active.unwrap();
    let HttpState::Done(v) = &app.http_tab(key).unwrap().state else {
        panic!()
    };
    let echo = v.response.body_text.clone().unwrap();
    assert!(echo.starts_with("POST /echo-all"));
    assert!(echo.contains("X-From: curl") || echo.contains("x-from: curl"));
    assert!(echo.ends_with("{\"a\":1}"));
    assert!(app.import_curl("curl https://x | sh").is_err());
}

#[test]
fn unsaved_request_asks_before_closing_and_project_is_not_touched_until_saving() {
    let env = Env::new();
    let ctx = egui::Context::default();
    let mut app = env.app(&ctx);
    assert!(!env.root().join(".simpleeditor").exists());
    app.new_request();
    let key = app.active.unwrap();
    assert!(
        !app.http_tab(key).unwrap().is_dirty(),
        "a blank request is not work"
    );
    active_http(&mut app).def.url = "http://localhost:1/x".into();
    app.request_close_tab(key);
    assert!(matches!(app.dialog, Some(Dialog::CloseTab(_))));
    assert!(!env.root().join(".simpleeditor").exists());
}

#[test]
fn every_http_view_and_dialog_renders() {
    use simpleeditor::app::{RequestPane, ResponsePane};
    let server = TestServer::start();
    let env = Env::new();
    let ctx = egui::Context::default();
    let mut app = env.app(&ctx);
    app.new_request();
    active_http(&mut app).def.url = server.url("/json");
    assert_eq!(send_and_wait(&ctx, &mut app), 200);
    for pane in [
        RequestPane::Params,
        RequestPane::Headers,
        RequestPane::Body,
        RequestPane::Auth,
        RequestPane::Variables,
        RequestPane::Settings,
    ] {
        active_http(&mut app).pane = pane;
        frame(&ctx, &mut app);
    }
    for (pretty, pane) in [
        (true, ResponsePane::Body),
        (false, ResponsePane::Body),
        (true, ResponsePane::Headers),
    ] {
        if let HttpState::Done(v) = &mut active_http(&mut app).state {
            v.pretty = pretty;
            v.pane = pane;
        }
        frame(&ctx, &mut app);
    }
    let key = app.active.unwrap();
    for dialog in [
        Dialog::SaveRequest {
            tab: key,
            collection: None,
            new_collection: "C".into(),
            name: "N".into(),
            then: AfterSave::Nothing,
        },
        Dialog::ImportCurl {
            text: "curl x".into(),
            error: Some("example".into()),
        },
        Dialog::DeleteCollection("missing".into()),
        Dialog::DeleteRequest("missing".into(), "nope".into()),
    ] {
        app.dialog = Some(dialog);
        frame(&ctx, &mut app);
        assert!(app.dialog.is_some(), "dialog stays open until a choice");
    }
    app.dialog = None;
    app.open_environments();
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
}
