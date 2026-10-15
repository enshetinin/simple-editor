//! Drives the real UI code headless (no window): typed text goes through the
//! egui `TextEdit`, the save command writes the file, and an HTTP tab sends a
//! request to a local server.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use simpleeditor::http::secrets::MemoryBackend;

use eframe::egui::{self, Event, Id, Key, Modifiers, RawInput};
use simpleeditor::app::{AppOptions, Dialog, HttpState, SimpleEditorApp, Tab};
use simpleeditor::cli::LaunchArgs;
use support::TestServer;

fn frame(ctx: &egui::Context, app: &mut SimpleEditorApp, events: Vec<Event>) {
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 800.0),
        )),
        events,
        ..RawInput::default()
    };
    app.poll();
    let mut output = ctx.run_ui(input, |ui| app.draw(ui));
    // No renderer here: textures are intentionally not uploaded.
    output.textures_delta.clear();
}

/// Recovery snapshots of tests go to a throwaway directory, never the
/// user's data directory.
fn options() -> AppOptions {
    let root = std::env::temp_dir().join(format!(
        "se-test-data-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    AppOptions {
        data_dir: Some(root),
        // Never touch the real Keychain / Credential Manager from tests.
        secrets: Some(Arc::new(MemoryBackend::default())),
        ..AppOptions::default()
    }
}

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn key_event(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn doc_text(app: &SimpleEditorApp) -> String {
    match app.active_tab() {
        Some(Tab::Doc(t)) => t.doc.text(),
        _ => panic!("no active document"),
    }
}

fn open_doc(
    ctx: &egui::Context,
    text: &str,
    name: &str,
    opts: AppOptions,
) -> (SimpleEditorApp, tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(name);
    std::fs::write(&file, text).unwrap();
    let mut app = SimpleEditorApp::new(
        ctx,
        LaunchArgs {
            root: None,
            files: vec![file.clone()],
        },
        opts,
    );
    until(ctx, &mut app, |a| !a.tabs.is_empty());
    let key = app.active.unwrap();
    ctx.memory_mut(|m| m.request_focus(Id::new(("doc_edit", key))));
    frame(ctx, &mut app, vec![]);
    (app, dir, file)
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
        frame(ctx, app, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn edit_save_and_send_request_through_the_ui() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() {}\r\n").unwrap();

    let ctx = egui::Context::default();
    let mut app = SimpleEditorApp::new(
        &ctx,
        LaunchArgs {
            root: Some(dir.path().to_path_buf()),
            files: vec![file.clone()],
        },
        options(),
    );
    until(&ctx, &mut app, |a| !a.tabs.is_empty());
    let key = app.active.expect("file tab is active");

    // Focus the editor and type through egui input events.
    ctx.memory_mut(|m| m.request_focus(Id::new(("doc_edit", key))));
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![Event::Text("// café 👋 ".to_owned())]);
    let Tab::Doc(tab) = &app.tabs[0] else {
        panic!()
    };
    assert!(tab.doc.is_dirty());
    assert!(
        tab.doc.text().contains("// café 👋 "),
        "{:?}",
        tab.doc.text()
    );

    // Cmd/Ctrl+S.
    let save = Event::Key {
        key: Key::S,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    };
    frame(&ctx, &mut app, vec![save]);
    until(&ctx, &mut app, |a| !a.any_dirty());
    let saved = std::fs::read_to_string(&file).unwrap();
    assert_eq!(saved, "// café 👋 fn main() {}\r\n", "CRLF preserved");

    // HTTP request from a new tab.
    let server = TestServer::start();
    app.new_request();
    let http_key = app.active.unwrap();
    if let Some(Tab::Http(t)) = app.tabs.iter_mut().find(|t| t.key() == http_key) {
        t.def.url = server.url("/json");
    }
    app.send_request(http_key);
    frame(&ctx, &mut app, vec![]);
    until(&ctx, &mut app, |a| {
        a.tabs
            .iter()
            .any(|t| matches!(t, Tab::Http(h) if matches!(h.state, HttpState::Done(_))))
    });
    let Some(Tab::Http(h)) = app.tabs.iter().find(|t| t.key() == http_key) else {
        panic!()
    };
    let HttpState::Done(view) = &h.state else {
        panic!()
    };
    assert_eq!(view.response.status, 200);
    assert_eq!(
        view.response.body_text.as_deref(),
        Some(r#"{"ok":true,"n":1}"#)
    );

    // Closing a modified document asks first; cancel keeps it open.
    if let Tab::Doc(tab) = &mut app.tabs[0] {
        tab.doc.insert_text("x");
    }
    app.request_close_tab(key);
    assert!(matches!(app.dialog, Some(Dialog::CloseTab(k)) if k == key));
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.tabs.len(), 2);

    // Window close with unsaved changes is intercepted.
    app.dialog = None;
    assert!(app.on_close_requested());
    assert!(matches!(app.dialog, Some(Dialog::Quit)));
}

#[test]
fn cancel_from_ui_state_ignores_late_results() {
    let server = TestServer::start();
    let ctx = egui::Context::default();
    let mut app = SimpleEditorApp::new(&ctx, LaunchArgs::default(), options());
    app.new_request();
    let key = app.active.unwrap();
    if let Some(Tab::Http(t)) = app.tabs.iter_mut().find(|t| t.key() == key) {
        t.def.url = server.url("/delay");
    }
    app.send_request(key);
    frame(&ctx, &mut app, vec![]);
    app.cancel_request(key);
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    let Some(Tab::Http(h)) = app.tabs.iter().find(|t| t.key() == key) else {
        panic!()
    };
    assert!(matches!(h.state, HttpState::Cancelled));
}

#[test]
fn invalid_url_blocks_sending_and_names_the_field() {
    let ctx = egui::Context::default();
    let mut app = SimpleEditorApp::new(&ctx, LaunchArgs::default(), options());
    app.new_request();
    let key = app.active.unwrap();
    if let Some(Tab::Http(t)) = app.tabs.iter_mut().find(|t| t.key() == key) {
        t.def.url = "ftp://example.com".to_owned();
    }
    app.send_request(key);
    let Some(Tab::Http(h)) = app.tabs.iter().find(|t| t.key() == key) else {
        panic!()
    };
    let HttpState::Invalid(e) = &h.state else {
        panic!("expected validation error")
    };
    assert_eq!(e[0].field, "URL");
    assert!(e[0].message.contains("ftp"));
    frame(&ctx, &mut app, vec![]);
}

#[test]
fn ime_preedit_is_not_in_the_document_until_commit() {
    let ctx = egui::Context::default();
    let (mut app, _dir, _file) = open_doc(&ctx, "ab", "a.txt", options());
    frame(
        &ctx,
        &mut app,
        vec![key_event(Key::ArrowRight, Modifiers::NONE)],
    );
    let preedit = |t: &str| {
        Event::Ime(egui::ImeEvent::Preedit {
            text: t.to_owned(),
            active_range_chars: None,
        })
    };
    frame(&ctx, &mut app, vec![preedit("に")]);
    frame(&ctx, &mut app, vec![preedit("にほ")]);
    assert_eq!(doc_text(&app), "ab", "composition is not inserted yet");
    // Keys during composition belong to the IME.
    frame(
        &ctx,
        &mut app,
        vec![key_event(Key::Backspace, Modifiers::NONE)],
    );
    assert_eq!(doc_text(&app), "ab");
    frame(
        &ctx,
        &mut app,
        vec![Event::Ime(egui::ImeEvent::Commit("日本".to_owned()))],
    );
    assert_eq!(doc_text(&app), "a日本b");
    // Dead-key style accent: preedit "´" then commit "é".
    frame(&ctx, &mut app, vec![preedit("´")]);
    frame(
        &ctx,
        &mut app,
        vec![Event::Ime(egui::ImeEvent::Commit("é".to_owned()))],
    );
    assert_eq!(doc_text(&app), "a日本éb");
    // One undo step per commit.
    frame(&ctx, &mut app, vec![key_event(Key::Z, Modifiers::COMMAND)]);
    assert_eq!(doc_text(&app), "a日本b");
}

#[test]
fn keyboard_editing_selection_clipboard_and_undo() {
    let ctx = egui::Context::default();
    let (mut app, _dir, _file) = open_doc(&ctx, "fn main() {\n}\n", "m.rs", options());
    let cmd = Modifiers::COMMAND;
    frame(&ctx, &mut app, vec![key_event(Key::End, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![key_event(Key::Enter, Modifiers::NONE)]);
    frame(&ctx, &mut app, vec![key_event(Key::Tab, Modifiers::NONE)]);
    frame(
        &ctx,
        &mut app,
        vec![Event::Text("x".into()), Event::Text("y".into())],
    );
    assert_eq!(doc_text(&app), "fn main() {\n    xy\n}\n");
    // Select the word, cut, paste twice.
    frame(
        &ctx,
        &mut app,
        vec![
            key_event(Key::ArrowLeft, Modifiers::SHIFT),
            key_event(Key::ArrowLeft, Modifiers::SHIFT),
        ],
    );
    frame(&ctx, &mut app, vec![Event::Cut]);
    assert_eq!(doc_text(&app), "fn main() {\n    \n}\n");
    frame(
        &ctx,
        &mut app,
        vec![Event::Paste("xy".into()), Event::Paste("👋🏽".into())],
    );
    assert_eq!(doc_text(&app), "fn main() {\n    xy👋🏽\n}\n");
    // Backspace removes the whole emoji grapheme.
    frame(
        &ctx,
        &mut app,
        vec![key_event(Key::Backspace, Modifiers::NONE)],
    );
    assert_eq!(doc_text(&app), "fn main() {\n    xy\n}\n");
    // Undo everything back to the file content; redo again.
    for _ in 0..10 {
        frame(&ctx, &mut app, vec![key_event(Key::Z, cmd)]);
    }
    assert_eq!(doc_text(&app), "fn main() {\n}\n");
    assert!(!app.any_dirty());
    frame(
        &ctx,
        &mut app,
        vec![key_event(Key::Z, cmd.plus(Modifiers::SHIFT))],
    );
    assert!(app.any_dirty());
}

#[test]
fn find_and_replace_through_the_ui() {
    let ctx = egui::Context::default();
    let (mut app, _dir, _file) = open_doc(&ctx, "café x café\nCAFÉ y\n", "f.txt", options());
    frame(&ctx, &mut app, vec![key_event(Key::F, Modifiers::COMMAND)]);
    let key = app.active.unwrap();
    {
        let Some(Tab::Doc(t)) = app.tabs.iter_mut().find(|t| t.key() == key) else {
            panic!()
        };
        let find = t.find.as_mut().expect("find bar open");
        find.query = "café".into();
        find.replacement = "té".into();
    }
    until(&ctx, &mut app, |a| match a.active_tab() {
        Some(Tab::Doc(t)) => t.find.as_ref().is_some_and(|f| f.matches.len() == 3),
        _ => false,
    });
    app.find_next(key, false);
    let Some(Tab::Doc(t)) = app.active_tab() else {
        panic!()
    };
    assert_eq!(t.doc.selected_text(), "café");
    app.replace_all(key);
    assert_eq!(doc_text(&app), "té x té\nté y\n");
    // As after clicking "Replace all": nothing focused, Cmd+Z undoes the doc.
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key_event(Key::Z, Modifiers::COMMAND)]);
    assert_eq!(
        doc_text(&app),
        "café x café\nCAFÉ y\n",
        "replace all is one undo step"
    );
}

#[test]
fn syntax_highlighting_arrives_for_visible_lines() {
    let ctx = egui::Context::default();
    let (mut app, _dir, _file) = open_doc(
        &ctx,
        "fn main() { let s = \"hola\"; }\n",
        "main.rs",
        options(),
    );
    until(&ctx, &mut app, |a| match a.active_tab() {
        Some(Tab::Doc(t)) => t.highlight.as_ref().is_some_and(|h| !h.spans.is_empty()),
        _ => false,
    });
    let Some(Tab::Doc(t)) = app.active_tab() else {
        panic!()
    };
    let spans = &t.highlight.as_ref().unwrap().spans;
    assert!(
        spans
            .iter()
            .any(|s| s.start == 0 && s.hl == simpleeditor::syntax::Hl::Keyword),
        "{spans:?}"
    );
}

#[test]
fn unsaved_changes_survive_a_crash_and_restore_without_touching_the_file() {
    let ctx = egui::Context::default();
    let opts = options();
    let (mut app, _dir, file) = open_doc(&ctx, "original\n", "notes.txt", opts.clone());
    frame(&ctx, &mut app, vec![Event::Text("recovered ".into())]);
    // Wait until the snapshot is written (after the idle delay).
    let root = opts.data_dir.clone().unwrap().join("recovery");
    let has_snapshot = |root: &std::path::Path| {
        std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .any(|d| {
                std::fs::read_dir(d.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|f| f.path().extension().is_some_and(|e| e == "json"))
            })
    };
    let start = Instant::now();
    while !has_snapshot(&root) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no recovery snapshot"
        );
        frame(&ctx, &mut app, vec![]);
        std::thread::sleep(Duration::from_millis(50));
    }
    // "Crash": drop the app without a clean shutdown.
    drop(app);
    std::thread::sleep(Duration::from_millis(100));

    let ctx2 = egui::Context::default();
    let mut app2 = SimpleEditorApp::new(&ctx2, LaunchArgs::default(), opts);
    until(&ctx2, &mut app2, |a| !a.orphans.is_empty());
    app2.restore_orphans();
    until(&ctx2, &mut app2, |a| !a.tabs.is_empty());
    assert_eq!(doc_text(&app2), "recovered original\n");
    assert!(app2.any_dirty(), "restored text is an unsaved change");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "original\n",
        "file untouched"
    );
    // Undo returns to the file's content.
    let key = app2.active.unwrap();
    ctx2.memory_mut(|m| m.request_focus(Id::new(("doc_edit", key))));
    frame(&ctx2, &mut app2, vec![]);
    frame(
        &ctx2,
        &mut app2,
        vec![key_event(Key::Z, Modifiers::COMMAND)],
    );
    assert_eq!(doc_text(&app2), "original\n");
    app2.shutdown();
}

#[test]
fn external_changes_reload_clean_docs_and_warn_dirty_ones() {
    let ctx = egui::Context::default();
    let (mut app, _dir, file) = open_doc(&ctx, "v1\n", "a.txt", options());
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&file, "v2 from another program\n").unwrap();
    app.check_disk_changes();
    until(&ctx, &mut app, |a| {
        doc_text(a) == "v2 from another program\n"
    });
    assert!(!app.any_dirty());

    frame(&ctx, &mut app, vec![Event::Text("mine ".into())]);
    std::fs::write(&file, "v3 longer content from elsewhere\n").unwrap();
    app.check_disk_changes();
    until(&ctx, &mut app, |a| match a.active_tab() {
        Some(Tab::Doc(t)) => t.disk_notice.is_some(),
        _ => false,
    });
    assert_eq!(
        doc_text(&app),
        "mine v2 from another program\n",
        "dirty buffer kept"
    );
    // Saving now asks before overwriting.
    frame(&ctx, &mut app, vec![key_event(Key::S, Modifiers::COMMAND)]);
    until(&ctx, &mut app, |a| a.dialog.is_some());
    assert!(matches!(app.dialog, Some(Dialog::Conflict { .. })));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "v3 longer content from elsewhere\n"
    );
}

#[test]
fn close_with_save_or_discard() {
    let ctx = egui::Context::default();
    let (mut app, _dir, file) = open_doc(&ctx, "a\n", "c.txt", options());
    frame(&ctx, &mut app, vec![Event::Text("b".into())]);
    let key = app.active.unwrap();
    // Save, then close (what the "Save" button of the close dialog does).
    assert!(app.save_tab(
        key,
        false,
        false,
        simpleeditor::app::AfterSave::CloseTab(key)
    ));
    until(&ctx, &mut app, |a| a.tabs.is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ba\n");

    // Discard: the file keeps its content.
    app.open_path(file.clone());
    until(&ctx, &mut app, |a| !a.tabs.is_empty());
    let key = app.active.unwrap();
    ctx.memory_mut(|m| m.request_focus(Id::new(("doc_edit", key))));
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![Event::Text("zzz".into())]);
    app.request_close_tab(key);
    assert!(matches!(app.dialog, Some(Dialog::CloseTab(_))));
    app.dialog = None;
    app.close_tab_now(key);
    assert!(app.tabs.is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ba\n");
}
