//! SimpleEditor: a desktop code editor with a built-in HTTP client.

pub mod app;
pub mod cli;
pub mod editor;
pub mod http;
pub mod install;
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
        CliAction::InstallCli { name, dir } => return install_cli(bin_name, &name, dir, true),
        CliAction::UninstallCli { name, dir } => {
            return install_cli(bin_name, &name, dir, false);
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
            .with_min_inner_size([960.0, 640.0])
            .with_icon(std::sync::Arc::new(eframe::egui::IconData {
                rgba: include_bytes!("../assets/icon/icon-64.rgba").to_vec(),
                width: 64,
                height: 64,
            })),
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
                    startup_probe,
                    ..app::AppOptions::default()
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

fn install_cli(bin_name: &str, name: &str, dir: Option<std::path::PathBuf>, add: bool) -> ExitCode {
    let Some(dir) = dir.or_else(install::default_dir) else {
        eprintln!("{bin_name}: no default folder found; pass --dir");
        return ExitCode::from(2);
    };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{bin_name}: cannot locate this executable: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = if add {
        install::install(&dir, name, &exe).map(|outcome| match outcome {
            install::InstallOutcome::Installed(p) => format!("Installed {}", p.display()),
            install::InstallOutcome::AlreadyInstalled(p) => {
                format!("Already installed: {}", p.display())
            }
        })
    } else {
        install::uninstall(&dir, name, &exe).map(|p| format!("Removed {}", p.display()))
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            if add && !install::dir_in_path(&dir) {
                println!("{}", install::path_hint(&dir));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{bin_name}: {e}");
            ExitCode::FAILURE
        }
    }
}
