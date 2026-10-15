//! Crash recovery for unsaved buffers.
//!
//! Each running instance owns a session directory under the recovery root and
//! holds an exclusive lock on `session.lock` while alive. Snapshots of
//! modified buffers are written there (never next to the user's files). On
//! startup, session directories whose lock can be acquired belong to
//! instances that ended without cleaning up (crash, kill, power loss); their
//! entries are offered to the user. Restoring never overwrites the original
//! file: it opens the recovered text as an unsaved change.

use std::fs::{self, File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::editor::save::atomic_write_with;
use crate::editor::text_format::{LineEnding, TextFormat};

pub const SCHEMA_VERSION: u32 = 1;
const LOCK_FILE: &str = "session.lock";
const SESSION_PREFIX: &str = "session-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryMeta {
    pub schema_version: u32,
    /// File the buffer belongs to, if it was ever saved/opened from disk.
    pub original_path: Option<PathBuf>,
    pub display_name: String,
    pub written_at_unix: u64,
    pub bom: bool,
    pub line_ending: String,
}

impl RecoveryMeta {
    pub fn new(original_path: Option<PathBuf>, display_name: String, format: TextFormat) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            original_path,
            display_name,
            written_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            bom: format.bom,
            line_ending: format.line_ending.label().to_owned(),
        }
    }

    pub fn format(&self) -> TextFormat {
        let line_ending = match self.line_ending.as_str() {
            "CRLF" => LineEnding::Crlf,
            "LF" => LineEnding::Lf,
            _ => LineEnding::Mixed,
        };
        TextFormat {
            bom: self.bom,
            line_ending,
        }
    }
}

/// A recovered buffer from a previous session.
#[derive(Debug, Clone)]
pub struct RecoveredEntry {
    pub meta: RecoveryMeta,
    pub content_path: PathBuf,
    meta_path: PathBuf,
}

impl RecoveredEntry {
    pub fn read_text(&self) -> io::Result<String> {
        fs::read_to_string(&self.content_path)
    }

    /// Deletes this entry after it was restored or discarded.
    pub fn remove(&self) -> io::Result<()> {
        remove_if_exists(&self.content_path)?;
        remove_if_exists(&self.meta_path)
    }
}

/// A previous session that ended without cleaning up. Holding this keeps its
/// lock, so another instance cannot offer the same entries at the same time.
#[derive(Debug)]
pub struct OrphanSession {
    pub dir: PathBuf,
    pub entries: Vec<RecoveredEntry>,
    lock: Option<File>,
}

impl OrphanSession {
    /// Removes the session directory if no entries are left.
    pub fn cleanup_if_empty(&mut self) {
        self.entries.retain(|e| e.content_path.exists());
        if self.entries.is_empty() {
            // Release the lock first: Windows may refuse to delete a
            // directory while a handle inside it is open.
            drop(self.lock.take());
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

/// The current instance's recovery session.
#[derive(Debug)]
pub struct RecoverySession {
    root: PathBuf,
    dir: PathBuf,
    _lock: File,
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Default recovery root: `<data dir>/recovery` (see [`super::data_dir`]).
pub fn default_root() -> Option<PathBuf> {
    super::data_dir().map(|d| d.join("recovery"))
}

impl RecoverySession {
    /// Creates and locks a new session directory under `root`.
    pub fn start(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = root.join(format!("{SESSION_PREFIX}{}-{nanos}", std::process::id()));
        fs::create_dir(&dir)?;
        let lock = File::create(dir.join(LOCK_FILE))?;
        lock.try_lock().map_err(|e| match e {
            TryLockError::Error(e) => e,
            TryLockError::WouldBlock => io::Error::other("recovery session already locked"),
        })?;
        Ok(Self {
            root: root.to_path_buf(),
            dir,
            _lock: lock,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn paths(&self, key: u64) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{key}.json")),
            self.dir.join(format!("{key}.txt")),
        )
    }

    /// Writes (or replaces) the snapshot for buffer `key`. Content first, then
    /// metadata, both atomically; an entry without metadata is ignored.
    pub fn write(&self, key: u64, meta: &RecoveryMeta, rope: &Rope) -> io::Result<()> {
        let (meta_path, content_path) = self.paths(key);
        atomic_write_with(&content_path, |w| {
            for chunk in rope.chunks() {
                w.write_all(chunk.as_bytes())?;
            }
            Ok(())
        })?;
        let json = serde_json::to_vec_pretty(meta).map_err(io::Error::other)?;
        atomic_write_with(&meta_path, |w| w.write_all(&json))
    }

    pub fn remove(&self, key: u64) -> io::Result<()> {
        let (meta_path, content_path) = self.paths(key);
        remove_if_exists(&meta_path)?;
        remove_if_exists(&content_path)
    }

    /// Clean shutdown: nothing left to recover.
    pub fn finish(self) {
        let Self { dir, _lock, .. } = self;
        drop(_lock);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Finds sessions of instances that are no longer running.
    pub fn find_orphans(&self) -> Vec<OrphanSession> {
        find_orphans(&self.root, Some(&self.dir))
    }
}

pub fn find_orphans(root: &Path, own: Option<&Path>) -> Vec<OrphanSession> {
    let Ok(read) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let dir = entry.path();
        let is_session = entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(SESSION_PREFIX));
        if !is_session || !dir.is_dir() || Some(dir.as_path()) == own {
            continue;
        }
        let Ok(lock) = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))
        else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue; // owned by a running instance
        }
        let mut session = OrphanSession {
            entries: read_entries(&dir),
            dir,
            lock: Some(lock),
        };
        if session.entries.is_empty() {
            session.cleanup_if_empty();
            continue;
        }
        out.push(session);
    }
    out
}

fn read_entries(dir: &Path) -> Vec<RecoveredEntry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let meta_path = entry.path();
        if meta_path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content_path = meta_path.with_extension("txt");
        let Ok(bytes) = fs::read(&meta_path) else {
            continue;
        };
        // Unknown schema versions are left on disk untouched.
        let Ok(meta) = serde_json::from_slice::<RecoveryMeta>(&bytes) else {
            continue;
        };
        if meta.schema_version != SCHEMA_VERSION || !content_path.exists() {
            continue;
        }
        entries.push(RecoveredEntry {
            meta,
            content_path,
            meta_path,
        });
    }
    entries.sort_by(|a, b| a.meta.display_name.cmp(&b.meta.display_name));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: &str) -> RecoveryMeta {
        RecoveryMeta::new(
            Some(PathBuf::from(name)),
            name.to_owned(),
            TextFormat::default(),
        )
    }

    #[test]
    fn live_sessions_are_not_orphans_and_dead_ones_are() {
        let root = tempfile::tempdir().unwrap();
        let live = RecoverySession::start(root.path()).unwrap();
        live.write(1, &meta("a.rs"), &Rope::from_str("unsaved 👋"))
            .unwrap();

        let other = RecoverySession::start(root.path()).unwrap();
        // `live` still holds its lock.
        assert!(other.find_orphans().is_empty());

        // Simulate a crash: drop the lock without cleaning up.
        drop(live);
        let orphans = other.find_orphans();
        assert_eq!(orphans.len(), 1);
        let entry = &orphans[0].entries[0];
        assert_eq!(entry.meta.display_name, "a.rs");
        assert_eq!(entry.read_text().unwrap(), "unsaved 👋");

        // While held, a third instance cannot claim the same session.
        let third = RecoverySession::start(root.path()).unwrap();
        assert!(third.find_orphans().iter().all(|o| o.entries.is_empty()));

        entry.remove().unwrap();
        let mut orphans = orphans;
        orphans[0].cleanup_if_empty();
        assert!(!orphans[0].dir.exists());
    }

    #[test]
    fn clean_finish_leaves_nothing_and_removed_entries_disappear() {
        let root = tempfile::tempdir().unwrap();
        let s = RecoverySession::start(root.path()).unwrap();
        s.write(7, &meta("b.rs"), &Rope::from_str("x")).unwrap();
        s.remove(7).unwrap();
        let dir = s.dir().to_path_buf();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1); // only the lock
        s.finish();
        assert!(!dir.exists());
    }
}
