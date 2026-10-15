//! Regression: the sidebar must keep a stable width across frames.

use eframe::egui::{self, RawInput};
use simpleeditor::app::{AppOptions, SimpleEditorApp};
use simpleeditor::cli::LaunchArgs;
use simpleeditor::http::RequestDefinition;
use simpleeditor::http::secrets::MemoryBackend;
use simpleeditor::storage::history::HistoryEntry;
use simpleeditor::storage::project::{Collection, Environment};
use std::sync::Arc;

fn sidebar_width(ctx: &egui::Context) -> f32 {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new("sidebar"))
        .map(|s| s.size().x)
        .unwrap_or(0.0)
}

fn check(fill: &[&str]) {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // A long file name and a long folder name in the file tree.
    std::fs::write(project.path().join(format!("{}.rs", "f".repeat(200))), "").unwrap();
    std::fs::create_dir(project.path().join("d".repeat(200))).unwrap();
    let ctx = egui::Context::default();
    let mut app = SimpleEditorApp::new(
        &ctx,
        LaunchArgs {
            root: Some(project.path().to_path_buf()),
            files: Vec::new(),
        },
        AppOptions {
            data_dir: Some(data.path().to_path_buf()),
            secrets: Some(Arc::new(MemoryBackend::default())),
            ..AppOptions::default()
        },
    );
    // Wait for the project to load, then fill every sidebar section.
    let start = std::time::Instant::now();
    while !app.project.as_ref().is_some_and(|p| p.loaded) {
        assert!(start.elapsed().as_secs() < 10);
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let long = "x".repeat(300);
    let def = RequestDefinition {
        id: "r1".into(),
        name: format!("A very long request name {long}"),
        url: format!("https://example.com/{long}"),
        ..RequestDefinition::default()
    };
    let p = app.project.as_mut().unwrap();
    if fill.contains(&"collection") {
        p.data.collections.push(Collection {
            id: "c".into(),
            name: format!("Collection {long}"),
            requests: vec![def.clone()],
        });
    }
    if fill.contains(&"env") {
        p.data.environments.push(Environment {
            id: "e".into(),
            name: format!("Environment {long}"),
            variables: Vec::new(),
        });
        p.active_env = Some("e".into());
    }
    if fill.contains(&"history") {
        app.history
            .push(HistoryEntry::new(&def, Some(200), None, 1));
    }
    if fill.contains(&"unsaved") {
        app.new_request();
        let key = app.active.unwrap();
        app.http_tab_mut(key).unwrap().def.name = format!("Unsaved {long}");
    }

    let mut widths = Vec::new();
    for i in 0..120 {
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
        if i % 10 == 9 {
            widths.push(sidebar_width(&ctx));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    eprintln!("{fill:?} widths: {widths:?}");
    let first = widths[1];
    assert!(
        widths.iter().skip(1).all(|w| (w - first).abs() < 0.5),
        "{widths:?}"
    );
    assert!(first <= 400.0, "{widths:?}");
}

#[test]
fn sidebar_width_is_stable() {
    for f in [
        &[][..],
        &["collection"],
        &["env"],
        &["history"],
        &["unsaved"],
    ] {
        check(f);
    }
    check(&["collection", "env", "history", "unsaved"]);
}
