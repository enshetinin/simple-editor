//! Crash-safe file replacement: write a temporary file next to the target,
//! flush it to disk and atomically rename it over the original.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Size and modification time observed on disk, used to detect external changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl DiskStamp {
    pub fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// Resolves a symlinked target so that the link itself stays a link and the
/// file it points to is the one replaced.
fn resolve_target(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path),
        _ => Ok(path.to_path_buf()),
    }
}

/// Atomically replaces (or creates) `path` with `bytes`.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |w| w.write_all(bytes))
}

/// Atomically replaces (or creates) `path` with what `write` produces.
///
/// If any step fails the original file is left untouched and the temporary
/// file is removed.
pub fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
    let target = resolve_target(path)?;
    let dir = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let existing_permissions = fs::metadata(&target).ok().map(|m| m.permissions());

    let tmp = tempfile::Builder::new()
        .prefix(".se-save-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    let mut writer = io::BufWriter::with_capacity(256 * 1024, tmp);
    write(&mut writer)?;
    let tmp = writer.into_inner().map_err(|e| e.into_error())?;
    if let Some(perms) = existing_permissions {
        fs::set_permissions(tmp.path(), perms)?;
    }
    tmp.as_file().sync_all()?;
    // On Windows `persist` uses MoveFileExW with MOVEFILE_REPLACE_EXISTING.
    tmp.persist(&target).map_err(|e| e.error)?;
    sync_dir(dir);
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) {
    // Best effort: makes the rename durable on power loss.
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}
