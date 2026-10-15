//! Request history and small per-user state, in the user's data directory
//! (never in the project).
//!
//! History keeps at most [`MAX_ENTRIES`] requests, sanitized with
//! `sanitize::for_history`: no bodies, no auth values, no literal secrets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::write_json;
use crate::http::model::RequestDefinition;
use crate::http::sanitize::for_history;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_ENTRIES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at_unix: u64,
    pub request: RequestDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl HistoryEntry {
    /// Builds a sanitized entry for a finished request.
    pub fn new(
        def: &RequestDefinition,
        status: Option<u16>,
        error: Option<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            request: for_history(def),
            status,
            error,
            duration_ms,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryFile {
    schema_version: u32,
    entries: Vec<HistoryEntry>,
}

pub fn history_path(data_dir: &Path) -> PathBuf {
    data_dir.join("history.json")
}

/// Loads history; a missing, unreadable or newer-version file yields an
/// empty list (the file is not modified until the next write).
pub fn load_history(path: &Path) -> Result<Vec<HistoryEntry>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let file: HistoryFile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if file.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "history schema_version {} not supported",
            file.schema_version
        ));
    }
    let mut entries = file.entries;
    entries.truncate(MAX_ENTRIES);
    Ok(entries)
}

pub fn save_history(path: &Path, entries: &[HistoryEntry]) -> std::io::Result<()> {
    write_json(
        path,
        &HistoryFile {
            schema_version: SCHEMA_VERSION,
            entries: entries.iter().take(MAX_ENTRIES).cloned().collect(),
        },
    )
}

/// Adds an entry at the front, keeping the list bounded.
pub fn push(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);
}

/// Small per-user state (e.g. the active environment of each project).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserState {
    pub schema_version: u32,
    /// Project root (as displayed) → active environment id.
    #[serde(default)]
    pub active_environment: BTreeMap<String, String>,
}

pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state.json")
}

pub fn load_state(path: &Path) -> UserState {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<UserState>(&b).ok())
        .filter(|s| s.schema_version == SCHEMA_VERSION)
        .unwrap_or(UserState {
            schema_version: SCHEMA_VERSION,
            ..UserState::default()
        })
}

pub fn save_state(path: &Path, state: &UserState) -> std::io::Result<()> {
    write_json(path, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::model::{Auth, Body, BodyMode};

    #[test]
    fn bounded_sanitized_and_persistent() {
        let dir = tempfile::tempdir().unwrap();
        let path = history_path(dir.path());
        assert!(load_history(&path).unwrap().is_empty());
        let def = RequestDefinition {
            url: "https://h/login?password=hunter2".into(),
            body: Body {
                mode: BodyMode::Json,
                text: "{\"secret\":1}".into(),
            },
            auth: Auth::Bearer {
                token: "tok-123".into(),
            },
            ..RequestDefinition::default()
        };
        let mut entries = Vec::new();
        for i in 0..150 {
            push(&mut entries, HistoryEntry::new(&def, Some(200), None, i));
        }
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].duration_ms, 149);
        save_history(&path, &entries).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in ["hunter2", "tok-123", "\\\"secret\\\":1"] {
            assert!(!text.contains(secret), "{secret} leaked");
        }
        assert_eq!(load_history(&path).unwrap().len(), MAX_ENTRIES);

        let sp = state_path(dir.path());
        let mut st = load_state(&sp);
        st.active_environment.insert("/p".into(), "local".into());
        save_state(&sp, &st).unwrap();
        assert_eq!(load_state(&sp), st);
    }
}
