//! Persistence: crash recovery and history in the user's data directory,
//! collections/environments in the project's `.simpleeditor/` folder.

pub mod history;
pub mod project;
pub mod recovery;

use std::path::{Path, PathBuf};

/// SimpleEditor's local data directory, or `$SE_DATA_DIR` when set (tests,
/// portable use).
///
/// - macOS: `~/Library/Application Support/SimpleEditor`
/// - Windows: `%LOCALAPPDATA%\SimpleEditor`
/// - other: `$XDG_DATA_HOME/SimpleEditor` or `~/.local/share/SimpleEditor`
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = non_empty_env("SE_DATA_DIR") {
        return Some(dir);
    }
    platform_data_dir().map(|d| d.join("SimpleEditor"))
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    non_empty_env("HOME").map(|h| h.join("Library").join("Application Support"))
}

#[cfg(windows)]
fn platform_data_dir() -> Option<PathBuf> {
    non_empty_env("LOCALAPPDATA")
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_data_dir() -> Option<PathBuf> {
    non_empty_env("XDG_DATA_HOME")
        .or_else(|| non_empty_env("HOME").map(|h| h.join(".local").join("share")))
}

/// Writes pretty JSON (with a trailing newline) atomically.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let mut json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    json.push(b'\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::editor::save::atomic_write(path, &json)
}
