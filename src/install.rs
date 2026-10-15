//! Opt-in installation of the `se` command on the user's PATH.
//!
//! - Never overwrites an existing file that is not ours: another `se` keeps
//!   working and the user can choose the alternative name `simpleeditor`.
//! - macOS/Linux: a symbolic link to the running executable.
//! - Windows: a `.cmd` shim that forwards arguments to the executable.
//! - The PATH itself is never modified; we print what to add if needed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed(PathBuf),
    AlreadyInstalled(PathBuf),
}

/// Default folder for the command: `~/.local/bin` on macOS/Linux,
/// `%LOCALAPPDATA%\SimpleEditor\bin` on Windows (no admin rights needed).
pub fn default_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|d| PathBuf::from(d).join("SimpleEditor").join("bin"))
    } else {
        std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join(".local").join("bin"))
    }
}

/// File that represents the command `name` in `dir`.
pub fn command_path(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{name}.cmd"))
    } else {
        dir.join(name)
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(format!("'{name}' is not a valid command name"))
    }
}

#[cfg(windows)]
fn shim_text(exe: &Path) -> String {
    format!("@echo off\r\n\"{}\" %*\r\n", exe.display())
}

/// Whether `path` is a command we created for `exe`.
fn is_ours(path: &Path, exe: &Path) -> bool {
    #[cfg(windows)]
    {
        fs::read_to_string(path).is_ok_and(|t| t == shim_text(exe))
    }
    #[cfg(not(windows))]
    {
        fs::read_link(path)
            .is_ok_and(|target| target == exe || fs::canonicalize(&target).is_ok_and(|t| t == exe))
    }
}

/// Installs command `name` in `dir` pointing to `exe`.
pub fn install(dir: &Path, name: &str, exe: &Path) -> Result<InstallOutcome, String> {
    validate_name(name)?;
    let exe =
        fs::canonicalize(exe).map_err(|e| format!("cannot resolve {}: {e}", exe.display()))?;
    let path = command_path(dir, name);
    if fs::symlink_metadata(&path).is_ok() {
        if is_ours(&path, &exe) {
            return Ok(InstallOutcome::AlreadyInstalled(path));
        }
        return Err(format!(
            "{} already exists and is not SimpleEditor; it was left untouched. Use another name (e.g. --name simpleeditor) or another --dir.",
            path.display()
        ));
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    create(&path, &exe).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    Ok(InstallOutcome::Installed(path))
}

#[cfg(not(windows))]
fn create(path: &Path, exe: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(exe, path)
}

#[cfg(windows)]
fn create(path: &Path, exe: &Path) -> io::Result<()> {
    use std::io::Write;
    // `create_new` never replaces a file created in the meantime.
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(shim_text(exe).as_bytes())
}

/// Removes command `name` from `dir` only if it is ours.
pub fn uninstall(dir: &Path, name: &str, exe: &Path) -> Result<PathBuf, String> {
    validate_name(name)?;
    let exe =
        fs::canonicalize(exe).map_err(|e| format!("cannot resolve {}: {e}", exe.display()))?;
    let path = command_path(dir, name);
    if fs::symlink_metadata(&path).is_err() {
        return Err(format!("{} is not installed", path.display()));
    }
    if !is_ours(&path, &exe) {
        return Err(format!(
            "{} was not created by this SimpleEditor; it was left untouched.",
            path.display()
        ));
    }
    fs::remove_file(&path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    Ok(path)
}

/// True if `dir` is listed in the PATH environment variable.
pub fn dir_in_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir))
}

/// Instructions to add `dir` to PATH (printed, never applied).
pub fn path_hint(dir: &Path) -> String {
    if cfg!(windows) {
        format!(
            "Add {0} to your user PATH, for example in PowerShell:\n  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';{0}', 'User')\nthen open a new terminal.",
            dir.display()
        )
    } else {
        format!(
            "{0} is not in your PATH. Add this line to your shell profile (~/.zshrc or ~/.bashrc):\n  export PATH=\"{0}:$PATH\"\nthen open a new terminal.",
            dir.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_without_overwriting_and_uninstalls_only_ours() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let exe = dir.path().join("SimpleEditor ñ").join("se-binary");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "binary").unwrap();

        let out = install(&bin, "se", &exe).unwrap();
        let path = command_path(&bin, "se");
        assert_eq!(out, InstallOutcome::Installed(path.clone()));
        assert_eq!(
            install(&bin, "se", &exe).unwrap(),
            InstallOutcome::AlreadyInstalled(path.clone())
        );

        // Someone else's `se` is never touched.
        let other = command_path(&bin, "se2");
        fs::write(&other, "another tool").unwrap();
        let err = install(&bin, "se2", &exe).unwrap_err();
        assert!(err.contains("not SimpleEditor"), "{err}");
        assert!(uninstall(&bin, "se2", &exe).is_err());
        assert_eq!(fs::read_to_string(&other).unwrap(), "another tool");

        // Alternative name works side by side.
        install(&bin, "simpleeditor", &exe).unwrap();
        assert_eq!(uninstall(&bin, "se", &exe).unwrap(), path);
        assert!(fs::symlink_metadata(&path).is_err());
        assert!(uninstall(&bin, "se", &exe).is_err());
        assert!(install(&bin, "../evil", &exe).is_err());
        assert!(install(&bin, "", &exe).is_err());
    }
}
