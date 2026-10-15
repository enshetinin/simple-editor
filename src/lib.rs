//! SimpleEditor: a desktop code editor with a built-in HTTP client.

pub mod app;
pub mod cli;
pub mod editor;
pub mod http;
pub mod storage;
pub mod syntax;
pub mod workspace;

use std::process::ExitCode;
use std::time::Instant;

use cli::CliAction;

/// Entry point shared by the `se` and `simpleeditor` binaries.
pub fn run(bin_name: &str) -> ExitCode {
    let started = Instant::now();
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("{bin_name}: cannot read the current directory: {e}");
            return ExitCode::from(2);
        }
    };
    let launch = match cli::parse(std::env::args_os().skip(1), &cwd) {
        CliAction::Launch(launch) => launch,
        CliAction::Help => {
            print!("{}", cli::help_text(bin_name));
            return ExitCode::SUCCESS;
        }
        CliAction::Version => {
            println!("SimpleEditor {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        CliAction::Error(message) => {
            eprintln!("{bin_name}: {message}");
            return ExitCode::from(2);
        }
    };
    let startup_probe = std::env::var_os("SE_STARTUP_PROBE").map(|_| started);

    let title = match &launch.root {
        Some(root) => format!(
            "{} — {}",
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string()),
            app::strings::APP_NAME
        ),
        None => app::strings::APP_NAME.to_owned(),
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id("simpleeditor")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        app::strings::APP_NAME,
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::SimpleEditorApp::new(
                &cc.egui_ctx,
                launch,
                app::AppOptions {
                    recovery_root: None,
                    startup_probe,
                },
            )))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{bin_name}: could not start the window: {e}");
            ExitCode::FAILURE
        }
    }
}
