//! Session restore: open tabs per project and the theme preference.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use eframe::egui::ThemePreference;

use super::{SimpleEditorApp, Tab, TabKey};
use crate::http::sanitize::strip_secrets;
use crate::storage::history::{MAX_SESSIONS, ProjectSession, SessionTab};

/// What is still pending while a session is being restored.
#[derive(Debug, Default)]
pub struct RestoreState {
    /// Files being reopened → cursor to place.
    pub cursors: HashMap<PathBuf, usize>,
    /// Files being reopened (they must not steal the active tab).
    pub files: HashSet<PathBuf>,
    /// File that was active, if the active tab was a file.
    pub active_file: Option<PathBuf>,
}

pub fn theme_name(t: ThemePreference) -> &'static str {
    match t {
        ThemePreference::System => "system",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

fn theme_from(name: &str) -> Option<ThemePreference> {
    match name {
        "system" => Some(ThemePreference::System),
        "light" => Some(ThemePreference::Light),
        "dark" => Some(ThemePreference::Dark),
        _ => None,
    }
}

impl SimpleEditorApp {
    pub fn set_theme_preference(&mut self, theme: ThemePreference) {
        self.theme_preference = theme;
        self.ctx.set_theme(theme);
        self.user_state.theme = Some(theme_name(theme).to_owned());
        self.save_user_data();
    }

    pub(super) fn apply_saved_theme(&mut self) {
        if let Some(theme) = self.user_state.theme.as_deref().and_then(theme_from) {
            self.theme_preference = theme;
            self.ctx.set_theme(theme);
        }
    }

    /// The tabs of the open project, as they would be restored.
    pub fn capture_session(&self) -> Option<(String, ProjectSession)> {
        let project = self.project.as_ref()?;
        let mut tabs = Vec::new();
        let mut active = None;
        for t in &self.tabs {
            let entry = match t {
                Tab::Doc(d) => d.doc.path().map(|p| SessionTab::File {
                    path: p.to_path_buf(),
                    cursor: d.doc.selection().head,
                }),
                Tab::Http(h) => Some(match &h.source {
                    Some(s) => SessionTab::Saved {
                        collection: s.collection.clone(),
                        request: s.request.clone(),
                    },
                    None => SessionTab::Draft {
                        request: strip_secrets(&h.def).0,
                    },
                }),
                Tab::Env(_) => None,
            };
            if let Some(entry) = entry {
                if self.active == Some(t.key()) {
                    active = Some(tabs.len());
                }
                tabs.push(entry);
            }
        }
        Some((
            project.root.display().to_string(),
            ProjectSession { tabs, active },
        ))
    }

    fn remember_session(&mut self) {
        if let Some((root, session)) = self.capture_session() {
            self.user_state.sessions.insert(root, session);
            while self.user_state.sessions.len() > MAX_SESSIONS {
                // BTreeMap order is alphabetical; drop any one other entry.
                let Some(key) = self.user_state.sessions.keys().next().cloned() else {
                    break;
                };
                self.user_state.sessions.remove(&key);
            }
        }
    }

    /// Writes history, state and the session now (used at exit).
    pub(super) fn save_session_blocking(&mut self) {
        if !self.user_data_loaded {
            // Never overwrite files we could not read yet.
            return;
        }
        self.remember_session();
        if let Some(dir) = &self.data_dir
            && let Err(e) = self.jobs.write_user_data_blocking(
                dir.clone(),
                self.history.clone(),
                self.user_state.clone(),
            )
        {
            eprintln!("could not save the session: {e}");
        }
    }

    /// Restores the project's last session once everything it needs is loaded.
    pub(super) fn maybe_restore_session(&mut self) {
        let Some(root) = self.pending_restore.clone() else {
            return;
        };
        let loaded = self
            .project
            .as_ref()
            .is_some_and(|p| p.loaded && p.root == root);
        if !loaded || !self.user_data_loaded {
            return;
        }
        self.pending_restore = None;
        let Some(session) = self
            .user_state
            .sessions
            .get(&root.display().to_string())
            .cloned()
        else {
            return;
        };
        let mut restore = RestoreState::default();
        let mut active_key: Option<TabKey> = None;
        for (i, tab) in session.tabs.iter().enumerate() {
            let is_active = session.active == Some(i);
            match tab {
                SessionTab::File { path, cursor } => {
                    if !path.is_file() {
                        continue;
                    }
                    restore.cursors.insert(path.clone(), *cursor);
                    restore.files.insert(path.clone());
                    if is_active {
                        restore.active_file = Some(path.clone());
                    }
                    self.open_path(path.clone());
                }
                SessionTab::Saved {
                    collection,
                    request,
                } => {
                    self.open_saved_request(collection, request);
                    if is_active {
                        active_key = self.active;
                    }
                }
                SessionTab::Draft { request } => {
                    self.open_request_draft(request.clone());
                    if is_active {
                        active_key = self.active;
                    }
                }
            }
        }
        if active_key.is_some() {
            self.active = active_key;
        }
        self.restore = Some(restore);
    }

    /// Called when a reopened file arrives: place its cursor and keep the
    /// restored active tab.
    pub(super) fn on_restored_doc(
        &mut self,
        key: TabKey,
        path: &std::path::Path,
        previous_active: Option<TabKey>,
    ) {
        let Some(restore) = self.restore.as_mut() else {
            return;
        };
        if !restore.files.remove(path) {
            return;
        }
        let cursor = restore.cursors.remove(path);
        let is_active = restore.active_file.as_deref() == Some(path);
        if restore.files.is_empty() {
            self.restore = None;
        }
        if let Some(tab) = self.doc_tab_mut(key)
            && let Some(cursor) = cursor
        {
            let pos = cursor.min(tab.doc.len_chars());
            tab.doc.move_to(pos, false);
            tab.view.scroll_to_cursor = true;
        }
        if !is_active {
            self.active = previous_active.or(self.active);
        }
    }
}

impl SimpleEditorApp {
    /// Opens an unsaved request with a given definition.
    pub fn open_request_draft(&mut self, def: crate::http::RequestDefinition) {
        self.open_request_definition(def);
    }
}
