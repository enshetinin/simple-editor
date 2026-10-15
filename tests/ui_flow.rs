//! Drives the real UI code headless (no window): typed text goes through the
//! egui `TextEdit`, the save command writes the file, and an HTTP tab sends a
//! request to a local server.

mod support;

use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Id, Key, Modifiers, RawInput};
use simpleeditor::app::{Dialog, HttpState, SimpleEditorApp, Tab};
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
        None,
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
    assert!(tab.doc.text.contains("// café 👋 "), "{:?}", tab.doc.text);

    // Cmd/Ctrl+S.
    let save = Event::Key {
        key: Key::S,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    };
    frame(&ctx, &mut app, vec![save]);
    let Tab::Doc(tab) = &app.tabs[0] else {
        panic!()
    };
    assert!(!tab.doc.is_dirty());
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(saved.contains("// café 👋 "));
    assert_eq!(saved, "fn main() {}\r\n// café 👋 ", "CRLF preserved");

    // HTTP request from a new tab.
    let server = TestServer::start();
    app.new_request();
    let http_key = app.active.unwrap();
    if let Some(Tab::Http(t)) = app.tabs.iter_mut().find(|t| t.key() == http_key) {
        t.draft.url = server.url("/json");
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
        tab.doc.text.push('x');
        tab.doc.mark_edited();
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
    let mut app = SimpleEditorApp::new(&ctx, LaunchArgs::default(), None);
    app.new_request();
    let key = app.active.unwrap();
    if let Some(Tab::Http(t)) = app.tabs.iter_mut().find(|t| t.key() == key) {
        t.draft.url = server.url("/delay");
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
    let mut app = SimpleEditorApp::new(&ctx, LaunchArgs::default(), None);
    app.new_request();
    let key = app.active.unwrap();
    if let Some(Tab::Http(t)) = app.tabs.iter_mut().find(|t| t.key() == key) {
        t.draft.url = "ftp://example.com".to_owned();
    }
    app.send_request(key);
    let Some(Tab::Http(h)) = app.tabs.iter().find(|t| t.key() == key) else {
        panic!()
    };
    let HttpState::Invalid(e) = &h.state else {
        panic!("expected validation error")
    };
    assert!(e.to_string().starts_with("URL:"));
    frame(&ctx, &mut app, vec![]);
}
