//! Session restore across app restarts (headless app).

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RawInput, ThemePreference};
use simpleeditor::app::{AppOptions, SimpleEditorApp, Tab};
use simpleeditor::cli::LaunchArgs;
use simpleeditor::http::Auth;
use simpleeditor::http::secrets::MemoryBackend;

fn frame(ctx: &egui::Context, app: &mut SimpleEditorApp) {
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 800.0),
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
            start.elapsed() < Duration::from_secs(10),
            "condition not reached"
        );
        frame(ctx, app);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn start(
    project: &std::path::Path,
    data: &std::path::Path,
    files: Vec<std::path::PathBuf>,
) -> (egui::Context, SimpleEditorApp) {
    let ctx = egui::Context::default();
    let app = SimpleEditorApp::new(
        &ctx,
        LaunchArgs {
            root: Some(project.to_path_buf()),
            files,
        },
        AppOptions {
            data_dir: Some(data.to_path_buf()),
            secrets: Some(Arc::new(MemoryBackend::default())),
            ..AppOptions::default()
        },
    );
    (ctx, app)
}

#[test]
fn tabs_cursor_draft_and_theme_are_restored_without_secrets() {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let a = project.path().join("a.rs");
    let b = project.path().join("b.rs");
    std::fs::write(&a, "fn a() {}\n").unwrap();
    std::fs::write(&b, "fn b() {}\nlet x = 1;\n").unwrap();

    let (ctx, mut app) = start(project.path(), data.path(), Vec::new());
    until(&ctx, &mut app, |a| {
        a.project.as_ref().is_some_and(|p| p.loaded)
    });
    app.open_path(a.clone());
    app.open_path(b.clone());
    until(&ctx, &mut app, |a| a.tabs.len() == 2);
    // Cursor in b.rs at line 2.
    let b_key = app
        .tabs
        .iter()
        .find_map(|t| match t {
            Tab::Doc(d) if d.doc.path() == Some(b.as_path()) => Some(d.key),
            _ => None,
        })
        .unwrap();
    app.doc_tab_mut(b_key).unwrap().doc.move_to(13, false);
    // A draft request with a literal token, and it is the active tab.
    app.new_request();
    let key = app.active.unwrap();
    let t = app.http_tab_mut(key).unwrap();
    t.def.url = "https://example.com/items".into();
    t.def.auth = Auth::Bearer {
        token: "literal-secret".into(),
    };
    app.set_theme_preference(ThemePreference::Dark);
    for _ in 0..5 {
        frame(&ctx, &mut app);
    }
    app.shutdown();
    let state = std::fs::read_to_string(data.path().join("state.json")).unwrap();
    assert!(!state.contains("literal-secret"), "{state}");
    assert!(state.contains("\"theme\": \"dark\""));
    drop(app);

    // `se .` again: everything comes back.
    let (ctx, mut app) = start(project.path(), data.path(), Vec::new());
    until(&ctx, &mut app, |a| a.tabs.len() == 3);
    for _ in 0..5 {
        frame(&ctx, &mut app);
    }
    assert_eq!(app.theme_preference, ThemePreference::Dark);
    let active = app.active.unwrap();
    let Some(Tab::Http(h)) = app.tabs.iter().find(|t| t.key() == active) else {
        panic!("the draft request was the active tab")
    };
    assert_eq!(h.def.url, "https://example.com/items");
    assert_eq!(
        h.def.auth,
        Auth::Bearer {
            token: String::new()
        },
        "secret not restored"
    );
    let b_doc = app
        .tabs
        .iter()
        .find_map(|t| match t {
            Tab::Doc(d) if d.doc.path() == Some(b.as_path()) => Some(&d.doc),
            _ => None,
        })
        .unwrap();
    assert_eq!(b_doc.selection().head, 13);

    // `se file`: no session restore, only the file.
    drop(app);
    let (ctx, mut app) = start(project.path(), data.path(), vec![a.clone()]);
    until(&ctx, &mut app, |a| !a.tabs.is_empty());
    for _ in 0..20 {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.tabs.len(), 1);
}
