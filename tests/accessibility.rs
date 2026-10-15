//! Accessibility and layout checks (headless app, AccessKit enabled).

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Id, Key, Modifiers, RawInput};
use simpleeditor::app::{AppOptions, SimpleEditorApp};
use simpleeditor::cli::LaunchArgs;
use simpleeditor::http::secrets::MemoryBackend;

fn options(data: &std::path::Path) -> AppOptions {
    AppOptions {
        data_dir: Some(data.to_path_buf()),
        secrets: Some(Arc::new(MemoryBackend::default())),
        ..AppOptions::default()
    }
}

fn run(
    ctx: &egui::Context,
    app: &mut SimpleEditorApp,
    size: egui::Vec2,
    ppp: f32,
    events: Vec<Event>,
) -> egui::FullOutput {
    let mut input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..RawInput::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(ppp);
    app.poll();
    let mut out = ctx.run_ui(input, |ui| app.draw(ui));
    out.textures_delta.clear();
    out
}

fn wait(
    ctx: &egui::Context,
    app: &mut SimpleEditorApp,
    mut done: impl FnMut(&SimpleEditorApp) -> bool,
) {
    let start = Instant::now();
    while !done(app) {
        assert!(start.elapsed() < Duration::from_secs(10));
        run(ctx, app, egui::vec2(1200.0, 800.0), 1.0, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn screen_readers_get_the_editor_line_and_position() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() {\n    println!(\"hola\");\n}\n").unwrap();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = SimpleEditorApp::new(
        &ctx,
        LaunchArgs {
            root: None,
            files: vec![file],
        },
        options(dir.path()),
    );
    wait(&ctx, &mut app, |a| !a.tabs.is_empty());
    let key = app.active.unwrap();
    ctx.memory_mut(|m| m.request_focus(Id::new(("doc_edit", key))));
    run(&ctx, &mut app, egui::vec2(1200.0, 800.0), 1.0, vec![]);
    let down = Event::Key {
        key: Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let out = run(&ctx, &mut app, egui::vec2(1200.0, 800.0), 1.0, vec![down]);
    let update = out
        .platform_output
        .accesskit_update
        .expect("accesskit update");
    let editor = update
        .nodes
        .iter()
        .map(|(_, n)| n)
        .find(|n| n.label().is_some_and(|l| l.starts_with("Code editor")))
        .expect("editor node");
    assert_eq!(
        editor.label().unwrap(),
        "Code editor: main.rs, line 2 of 4, column 1"
    );
    assert_eq!(editor.value(), Some("    println!(\"hola\");"));
}

#[test]
fn layout_fits_960x640_at_100_150_and_200_percent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    for ppp in [1.0, 1.5, 2.0] {
        let ctx = egui::Context::default();
        let mut app = SimpleEditorApp::new(
            &ctx,
            LaunchArgs {
                root: Some(dir.path().to_path_buf()),
                files: Vec::new(),
            },
            options(dir.path()),
        );
        wait(&ctx, &mut app, |a| {
            a.project.as_ref().is_some_and(|p| p.loaded)
        });
        app.open_path(dir.path().join("a.rs"));
        wait(&ctx, &mut app, |a| !a.tabs.is_empty());
        let size = egui::vec2(960.0, 640.0);
        let check = |ctx: &egui::Context, what: &str| {
            // Everything drawn stays inside the window (nothing overflows).
            let used = ctx.globally_used_rect();
            assert!(
                used.max.x <= size.x + 0.5,
                "ppp {ppp} {what}: width {}",
                used.max.x
            );
            assert!(
                used.max.y <= size.y + 0.5,
                "ppp {ppp} {what}: height {}",
                used.max.y
            );
        };
        for _ in 0..5 {
            run(&ctx, &mut app, size, ppp, vec![]);
        }
        check(&ctx, "editor");
        app.open_environments();
        for _ in 0..5 {
            run(&ctx, &mut app, size, ppp, vec![]);
        }
        check(&ctx, "environments");
        app.new_request();
        use simpleeditor::app::RequestPane::*;
        for pane in [Params, Headers, Body, Auth, Variables, Settings] {
            let key = app.active.unwrap();
            app.http_tab_mut(key).unwrap().pane = pane;
            for _ in 0..3 {
                run(&ctx, &mut app, size, ppp, vec![]);
            }
            check(&ctx, &format!("{pane:?}"));
        }
        // Ctrl+Tab cycles tabs.
        let before = app.active;
        let tab = Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        };
        run(&ctx, &mut app, size, ppp, vec![tab]);
        assert_ne!(app.active, before, "Ctrl+Tab switches tab");
    }
}
