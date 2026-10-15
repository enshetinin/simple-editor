//! Command line parsing for `se` / `simpleeditor`. No external parser: the
//! surface is small and must treat every argument as a path, never a command.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchArgs {
    /// Project folder shown in the sidebar.
    pub root: Option<PathBuf>,
    /// Files to open as tabs. Paths that do not exist yet open as new files.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Launch(LaunchArgs),
    Help,
    Version,
    /// Opt-in: put a command on the PATH (`--install-cli`).
    InstallCli {
        name: String,
        dir: Option<PathBuf>,
    },
    UninstallCli {
        name: String,
        dir: Option<PathBuf>,
    },
    Error(String),
}

pub fn help_text(bin: &str) -> String {
    format!(
        "SimpleEditor {version} — Escribe código. Prueba APIs. Así de simple.\n\
         \n\
         Usage:\n  {bin} [OPTIONS] [PATH]...\n\
         \n\
         Arguments:\n  [PATH]...  A folder to open as the project and/or files to open.\n\
         \x20            Relative paths are resolved from the current directory.\n\
         \n\
         Options:\n  -h, --help          Print help\n  -V, --version       Print version\n\
         \x20     --install-cli   Install the `se` command (opt-in; never replaces another `se`)\n\
         \x20     --uninstall-cli Remove the command installed by --install-cli\n\
         \x20     --name NAME     Command name for --install-cli/--uninstall-cli (default: se;\n\
         \x20                     use `simpleeditor` if another `se` exists)\n\
         \x20     --dir DIR       Folder for the command (default: ~/.local/bin on macOS,\n\
         \x20                     %LOCALAPPDATA%\\SimpleEditor\\bin on Windows)\n\
         \n\
         Examples:\n  {bin} .\n  {bin} src/main.rs README.md\n  {bin} --install-cli --name simpleeditor\n",
        version = env!("CARGO_PKG_VERSION"),
    )
}

pub fn parse<I>(args: I, cwd: &Path) -> CliAction
where
    I: IntoIterator<Item = OsString>,
{
    let mut launch = LaunchArgs::default();
    let mut options_done = false;
    let mut install: Option<bool> = None;
    let mut name = "se".to_owned();
    let mut dir: Option<PathBuf> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if !options_done {
            match arg.to_str() {
                Some("-h" | "--help") => return CliAction::Help,
                Some("-V" | "--version") => return CliAction::Version,
                Some("--install-cli") => {
                    install = Some(true);
                    continue;
                }
                Some("--uninstall-cli") => {
                    install = Some(false);
                    continue;
                }
                Some(flag @ ("--name" | "--dir")) => {
                    let Some(value) = args.next() else {
                        return CliAction::Error(format!("{flag} needs a value"));
                    };
                    if flag == "--name" {
                        name = value.to_string_lossy().into_owned();
                    } else {
                        dir = Some(normalize(&cwd.join(PathBuf::from(value))));
                    }
                    continue;
                }
                Some("--") => {
                    options_done = true;
                    continue;
                }
                // Finder on older macOS passes a process serial number.
                Some(s) if s.starts_with("-psn_") => continue,
                Some(s) if s.starts_with('-') && s.len() > 1 => {
                    return CliAction::Error(format!("unknown option '{s}' (see --help)"));
                }
                _ => {}
            }
        }
        let path = normalize(&cwd.join(PathBuf::from(&arg)));
        if path.is_dir() {
            if launch.root.is_some() {
                return CliAction::Error("only one folder can be opened per window".to_owned());
            }
            launch.root = Some(path);
        } else if path.exists() || path.parent().is_some_and(Path::is_dir) {
            launch.files.push(path);
        } else {
            return CliAction::Error(format!(
                "'{}' does not exist and its folder does not exist either",
                path.display()
            ));
        }
    }
    match install {
        Some(_) if launch.root.is_some() || !launch.files.is_empty() => {
            CliAction::Error("--install-cli/--uninstall-cli do not take paths".to_owned())
        }
        Some(true) => CliAction::InstallCli { name, dir },
        Some(false) => CliAction::UninstallCli { name, dir },
        None if name != "se" || dir.is_some() => CliAction::Error(
            "--name and --dir are only used with --install-cli/--uninstall-cli".to_owned(),
        ),
        None => CliAction::Launch(launch),
    }
}

/// Lexically removes `.` and `..` components without touching the file system
/// (symlinks are kept as written).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(component);
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn dot_resolves_to_cwd_and_files_are_relative_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("proj ñ 👋");
        std::fs::create_dir_all(cwd.join("src")).unwrap();
        std::fs::write(cwd.join("src/main.rs"), "fn main() {}").unwrap();

        let CliAction::Launch(l) = parse(args(&["."]), &cwd) else {
            panic!("expected launch")
        };
        assert_eq!(l.root.as_deref(), Some(cwd.as_path()));
        assert!(l.files.is_empty());

        let CliAction::Launch(l) = parse(args(&["src/./main.rs", "new file.txt"]), &cwd) else {
            panic!("expected launch")
        };
        assert_eq!(l.root, None);
        assert_eq!(
            l.files,
            vec![cwd.join("src/main.rs"), cwd.join("new file.txt")]
        );
    }

    #[test]
    fn options_and_errors() {
        let cwd = std::env::temp_dir();
        assert_eq!(parse(args(&["--help"]), &cwd), CliAction::Help);
        assert_eq!(parse(args(&["-V"]), &cwd), CliAction::Version);
        assert!(matches!(
            parse(args(&["--frobnicate"]), &cwd),
            CliAction::Error(_)
        ));
        assert!(matches!(
            parse(args(&["no/such/dir/file.rs"]), &cwd),
            CliAction::Error(_)
        ));
        assert_eq!(
            parse(args(&[]), &cwd),
            CliAction::Launch(LaunchArgs::default())
        );
        assert_eq!(
            parse(args(&["--install-cli"]), &cwd),
            CliAction::InstallCli {
                name: "se".into(),
                dir: None
            }
        );
        assert_eq!(
            parse(
                args(&["--uninstall-cli", "--name", "simpleeditor", "--dir", "bin"]),
                &cwd
            ),
            CliAction::UninstallCli {
                name: "simpleeditor".into(),
                dir: Some(cwd.join("bin"))
            }
        );
        assert!(matches!(
            parse(args(&["--name", "x"]), &cwd),
            CliAction::Error(_)
        ));
        assert!(matches!(
            parse(args(&["--install-cli", "."]), &cwd),
            CliAction::Error(_)
        ));
        assert!(matches!(parse(args(&["--dir"]), &cwd), CliAction::Error(_)));
        assert_eq!(
            parse(args(&["-psn_0_12345"]), &cwd),
            CliAction::Launch(LaunchArgs::default())
        );
    }
}
