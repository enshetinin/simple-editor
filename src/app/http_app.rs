//! HTTP side of the app: request tabs, sending, history, collections,
//! environments and cURL import. Disk and network work happen in workers.

use std::path::PathBuf;
use std::time::Instant;

use super::jobs::JobResult;
use super::{AfterSave, Dialog, SimpleEditorApp, Tab, TabKey};
use crate::http::curl;
use crate::http::model::{RequestDefinition, Variable, new_id};
use crate::http::{BodySink, FieldError, HttpError, HttpResponse, RequestId, resolve};
use crate::storage::history::{self, HistoryEntry};
use crate::storage::project::{Collection, ProjectData, collections_dir, unique_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestPane {
    Params,
    Headers,
    Body,
    Auth,
    Variables,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsePane {
    Body,
    Headers,
}

pub struct ResponseView {
    pub response: HttpResponse,
    pub pane: ResponsePane,
    /// Show pretty JSON (when available) instead of the raw body.
    pub pretty: bool,
}

pub enum HttpState {
    Idle,
    Invalid(Vec<FieldError>),
    Sending {
        id: RequestId,
        started: Instant,
        download: Option<PathBuf>,
    },
    Cancelled,
    Failed(HttpError),
    Done(Box<ResponseView>),
}

/// Where a request tab's definition is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSource {
    pub collection: String,
    pub request: String,
}

pub struct HttpTab {
    pub key: TabKey,
    pub def: RequestDefinition,
    pub source: Option<RequestSource>,
    /// Definition as last saved (for the dirty indicator).
    saved: Option<RequestDefinition>,
    pub state: HttpState,
    pub pane: RequestPane,
    pub saving: bool,
    /// Definition being sent (for history when it finishes).
    sent: Option<RequestDefinition>,
}

impl HttpTab {
    fn new(key: TabKey, def: RequestDefinition, source: Option<RequestSource>) -> Self {
        let saved = source.as_ref().map(|_| def.clone());
        Self {
            key,
            def,
            source,
            saved,
            state: HttpState::Idle,
            pane: RequestPane::Params,
            saving: false,
            sent: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        match &self.saved {
            Some(saved) => *saved != self.def,
            // A new request is work once it has content.
            None => {
                let blank = RequestDefinition {
                    id: self.def.id.clone(),
                    name: self.def.name.clone(),
                    ..RequestDefinition::default()
                };
                self.def != blank
            }
        }
    }

    pub fn request_id(&self) -> Option<RequestId> {
        match self.state {
            HttpState::Sending { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Collections, environments and variables of the open folder.
pub struct ProjectState {
    pub root: PathBuf,
    pub loaded: bool,
    pub data: ProjectData,
    /// Id of the active environment.
    pub active_env: Option<String>,
}

impl ProjectState {
    pub fn loading(root: PathBuf) -> Self {
        Self {
            root,
            loaded: false,
            data: ProjectData::default(),
            active_env: None,
        }
    }

    pub fn environment_vars(&self) -> &[Variable] {
        self.active_env
            .as_ref()
            .and_then(|id| self.data.environments.iter().find(|e| &e.id == id))
            .map_or(&[], |e| &e.variables)
    }

    pub fn active_env_name(&self) -> Option<&str> {
        let id = self.active_env.as_ref()?;
        self.data
            .environments
            .iter()
            .find(|e| &e.id == id)
            .map(|e| e.name.as_str())
    }

    pub fn collection(&self, id: &str) -> Option<&Collection> {
        self.data.collections.iter().find(|c| c.id == id)
    }

    fn root_key(&self) -> String {
        self.root.display().to_string()
    }
}

impl SimpleEditorApp {
    fn open_request_tab(&mut self, def: RequestDefinition, source: Option<RequestSource>) {
        let key = self.alloc_key();
        self.tabs
            .push(Tab::Http(Box::new(HttpTab::new(key, def, source))));
        self.active = Some(key);
    }

    pub fn new_request(&mut self) {
        let def = RequestDefinition {
            name: format!("Request {}", self.next_request_number),
            ..RequestDefinition::default()
        };
        self.next_request_number += 1;
        self.open_request_tab(def, None);
    }

    /// Opens a saved request, or focuses its tab if already open.
    pub fn open_saved_request(&mut self, collection: &str, request: &str) {
        let source = RequestSource {
            collection: collection.to_owned(),
            request: request.to_owned(),
        };
        if let Some(t) = self
            .tabs
            .iter()
            .find(|t| matches!(t, Tab::Http(h) if h.source.as_ref() == Some(&source)))
        {
            self.active = Some(t.key());
            return;
        }
        let def = self
            .project
            .as_ref()
            .and_then(|p| p.collection(collection))
            .and_then(|c| c.requests.iter().find(|r| r.id == request))
            .cloned();
        if let Some(def) = def {
            self.open_request_tab(def, Some(source));
        }
    }

    /// Opens a history entry as a new, unsaved request.
    pub fn open_history_entry(&mut self, index: usize) {
        if let Some(entry) = self.history.get(index) {
            let mut def = entry.request.clone();
            def.id = String::new();
            self.open_request_tab(def, None);
        }
    }

    fn scope_vars(&self) -> (Vec<Variable>, Vec<Variable>) {
        match &self.project {
            Some(p) => (p.environment_vars().to_vec(), p.data.variables.clone()),
            None => (Vec::new(), Vec::new()),
        }
    }

    /// Validates and sends. Unresolved variables or invalid fields block the
    /// request and are listed by field. `download` streams the body to a file.
    pub fn send_request_to(&mut self, key: TabKey, download: Option<PathBuf>) {
        let (env, project) = self.scope_vars();
        let limit = self.download_limit_mib.max(1) * 1024 * 1024;
        let Some(tab) = self.tabs.iter_mut().find_map(|t| match t {
            Tab::Http(h) if h.key == key => Some(h.as_mut()),
            _ => None,
        }) else {
            return;
        };
        let Some(http) = self.http.as_mut() else {
            return;
        };
        if let HttpState::Sending { id, .. } = tab.state {
            http.cancel(id);
        }
        tab.state = match resolve(&tab.def, &env, &project) {
            Ok(resolved) => {
                let sink = match &download {
                    Some(path) => BodySink::File {
                        path: path.clone(),
                        max_bytes: limit,
                    },
                    None => BodySink::Preview,
                };
                tab.sent = Some(tab.def.clone());
                HttpState::Sending {
                    id: http.send_to(resolved, sink),
                    started: Instant::now(),
                    download,
                }
            }
            Err(errors) => HttpState::Invalid(errors),
        };
    }

    pub fn send_request(&mut self, key: TabKey) {
        self.send_request_to(key, None);
    }

    pub fn cancel_request(&mut self, key: TabKey) {
        let Some(tab) = self.tabs.iter_mut().find_map(|t| match t {
            Tab::Http(h) if h.key == key => Some(h.as_mut()),
            _ => None,
        }) else {
            return;
        };
        if let (HttpState::Sending { id, .. }, Some(http)) = (&tab.state, self.http.as_mut()) {
            http.cancel(*id);
            tab.state = HttpState::Cancelled;
        }
    }

    pub(super) fn poll_http(&mut self) {
        let Some(http) = self.http.as_mut() else {
            return;
        };
        let mut finished = Vec::new();
        for event in http.poll() {
            // Only the latest request of a tab may update it.
            let tab = self.tabs.iter_mut().find_map(|t| match t {
                Tab::Http(h)
                    if matches!(h.state, HttpState::Sending { id, .. } if id == event.id) =>
                {
                    Some(h)
                }
                _ => None,
            });
            let Some(tab) = tab else { continue };
            let started = match tab.state {
                HttpState::Sending { started, .. } => started,
                _ => Instant::now(),
            };
            let (status, error) = match &event.result {
                Ok(r) => (Some(r.status), None),
                Err(e) => (None, Some(e.title().to_owned())),
            };
            if let Some(def) = tab.sent.take() {
                let ms = started.elapsed().as_millis() as u64;
                finished.push(HistoryEntry::new(&def, status, error, ms));
            }
            tab.state = match event.result {
                Ok(response) => HttpState::Done(Box::new(ResponseView {
                    pretty: matches!(response.json, crate::http::JsonView::Pretty(_)),
                    response,
                    pane: ResponsePane::Body,
                })),
                Err(e) => HttpState::Failed(e),
            };
        }
        if !finished.is_empty() {
            for entry in finished {
                history::push(&mut self.history, entry);
            }
            self.save_user_data();
        }
    }

    pub(super) fn open_request_definition(&mut self, def: RequestDefinition) {
        self.open_request_tab(def, None);
    }

    pub(super) fn save_user_data(&mut self) {
        if !self.user_data_loaded {
            // Writing now would replace history we have not read yet.
            self.user_data_dirty = true;
            return;
        }
        if let Some(dir) = &self.data_dir {
            self.jobs
                .save_user_data(dir.clone(), self.history.clone(), self.user_state.clone());
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.save_user_data();
    }

    pub fn set_active_environment(&mut self, id: Option<String>) {
        let Some(project) = self.project.as_mut() else {
            return;
        };
        project.active_env = id.clone();
        let root = project.root_key();
        match id {
            Some(id) => self.user_state.active_environment.insert(root, id),
            None => self.user_state.active_environment.remove(&root),
        };
        self.save_user_data();
    }

    // ---- saving requests ---------------------------------------------------

    /// Saves a request tab to its collection, or asks where to save it.
    pub fn save_request(&mut self, key: TabKey, then: AfterSave) -> bool {
        let Some(project) = self.project.as_ref().filter(|p| p.loaded) else {
            self.notice_error(
                "Open a folder to save requests in a collection (.simpleeditor/collections).",
            );
            return false;
        };
        let Some(tab) = self.http_tab(key) else {
            return false;
        };
        if tab.saving {
            return true;
        }
        match tab.source.clone() {
            Some(source) => {
                let def = tab.def.clone();
                let root = project.root.clone();
                let Some(collection) = self.update_collection(&source.collection, |c| {
                    if let Some(r) = c.requests.iter_mut().find(|r| r.id == source.request) {
                        *r = def.clone();
                    } else {
                        c.requests.push(def.clone());
                    }
                }) else {
                    self.notice_error(
                        "The collection of this request no longer exists; use Save as.",
                    );
                    return false;
                };
                if let Some(tab) = self.http_tab_mut(key) {
                    tab.saving = true;
                    tab.saved = Some(tab.def.clone());
                }
                self.jobs.save_collection(root, collection, Some(key), then);
                true
            }
            None => {
                let first = project.data.collections.first().map(|c| c.id.clone());
                self.dialog = Some(Dialog::SaveRequest {
                    tab: key,
                    collection: first,
                    new_collection: if project.data.collections.is_empty() {
                        "My collection".to_owned()
                    } else {
                        String::new()
                    },
                    name: tab.def.name.clone(),
                    then,
                });
                // The dialog decides; nothing started yet.
                false
            }
        }
    }

    /// Completes the "Save to collection" dialog.
    pub fn save_request_as(
        &mut self,
        key: TabKey,
        collection: Option<String>,
        new_collection: &str,
        name: &str,
        then: AfterSave,
    ) {
        let Some(project) = self.project.as_mut() else {
            return;
        };
        let root = project.root.clone();
        let collection_id = match collection {
            Some(id) if new_collection.trim().is_empty() => id,
            _ => {
                let name = new_collection.trim();
                let name = if name.is_empty() {
                    "My collection"
                } else {
                    name
                };
                let id = unique_id(&collections_dir(&root), name);
                project.data.collections.push(Collection {
                    id: id.clone(),
                    name: name.to_owned(),
                    requests: Vec::new(),
                });
                id
            }
        };
        let Some(tab) = self.http_tab_mut(key) else {
            return;
        };
        tab.def.name = if name.trim().is_empty() {
            tab.def.name.clone()
        } else {
            name.trim().to_owned()
        };
        if tab.def.id.is_empty() {
            tab.def.id = new_id();
        }
        tab.source = Some(RequestSource {
            collection: collection_id,
            request: tab.def.id.clone(),
        });
        tab.saved = None;
        self.save_request(key, then);
    }

    /// Applies `edit` to a collection in memory and returns a copy to save.
    fn update_collection(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut Collection),
    ) -> Option<Collection> {
        let project = self.project.as_mut()?;
        let c = project.data.collections.iter_mut().find(|c| c.id == id)?;
        edit(c);
        Some(c.clone())
    }

    pub fn delete_collection(&mut self, id: &str) {
        let Some(project) = self.project.as_mut() else {
            return;
        };
        let Some(pos) = project.data.collections.iter().position(|c| c.id == id) else {
            return;
        };
        let c = project.data.collections.remove(pos);
        let root = project.root.clone();
        for t in &mut self.tabs {
            if let Tab::Http(h) = t
                && h.source.as_ref().is_some_and(|s| s.collection == c.id)
            {
                h.source = None;
                h.saved = None;
            }
        }
        self.jobs.delete_collection(root, c.id, c.name);
    }

    pub fn delete_request(&mut self, collection: &str, request: &str) {
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            return;
        };
        let Some(saved) = self.update_collection(collection, |c| {
            c.requests.retain(|r| r.id != request);
        }) else {
            return;
        };
        for t in &mut self.tabs {
            if let Tab::Http(h) = t
                && h.source.as_ref().is_some_and(|s| s.request == request)
            {
                h.source = None;
                h.saved = None;
            }
        }
        self.jobs
            .save_collection(root, saved, None, AfterSave::Nothing);
    }

    // ---- import / export ---------------------------------------------------

    /// Parses pasted cURL text; on success opens a new request tab.
    pub fn import_curl(&mut self, text: &str) -> Result<(), String> {
        let imported = curl::parse(text)?;
        if !imported.warnings.is_empty() {
            self.notice_info(format!(
                "cURL imported with notes: {}",
                imported.warnings.join(" ")
            ));
        }
        self.open_request_tab(imported.request, None);
        Ok(())
    }

    pub fn copy_as_curl(&mut self, key: TabKey) {
        if let Some(tab) = self.http_tab(key) {
            let cmd = curl::export_posix(&tab.def, false);
            self.ctx.copy_text(cmd);
            self.notice_info(
                "Copied as cURL for POSIX shells (bash, zsh). Literal credentials were replaced with <omitted>; {{variables}} are kept.",
            );
        }
    }

    pub fn import_collection_dialog(&mut self) {
        let Some(project) = self.project.as_ref().filter(|p| p.loaded) else {
            self.notice_error("Open a folder before importing a collection.");
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SimpleEditor collection", &["json"])
            .pick_file()
        {
            let names = project
                .data
                .collections
                .iter()
                .map(|c| c.name.clone())
                .collect();
            self.jobs
                .import_collection(project.root.clone(), path, names);
        }
    }

    pub fn export_collection_dialog(&mut self, id: &str, include_secrets: bool) {
        let Some(c) = self
            .project
            .as_ref()
            .and_then(|p| p.collection(id))
            .cloned()
        else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}.json", c.id))
            .save_file()
        {
            self.jobs.export_collection(c, path, include_secrets);
        }
    }

    // ---- job results ---------------------------------------------------------

    pub(super) fn on_http_job(&mut self, result: JobResult) {
        match result {
            JobResult::ProjectLoaded {
                root,
                data,
                secret_errors,
            } => {
                let active = self
                    .user_state
                    .active_environment
                    .get(&root.display().to_string())
                    .cloned()
                    .filter(|id| data.environments.iter().any(|e| &e.id == id));
                for e in &data.errors {
                    self.notices.push(super::Notice {
                        is_error: true,
                        text: format!("Not loaded: {e}"),
                    });
                }
                if !secret_errors.is_empty() {
                    self.notice_error(format!(
                        "Some remembered secrets could not be read from {}; enter them for this session: {}",
                        self.secrets.name(),
                        secret_errors.join(", ")
                    ));
                }
                if let Some(p) = self.project.as_mut().filter(|p| p.root == root) {
                    p.data = data;
                    p.loaded = true;
                    if p.active_env.is_none() {
                        p.active_env = active;
                    }
                }
            }
            JobResult::UserDataLoaded { history, state } => {
                match history {
                    Ok(mut entries) => {
                        // Keep requests sent before the file finished loading.
                        let mut merged = std::mem::take(&mut self.history);
                        merged.append(&mut entries);
                        merged.truncate(history::MAX_ENTRIES);
                        self.history = merged;
                    }
                    Err(e) => self.notice_error(format!(
                        "Request history could not be read ({e}); it will start empty."
                    )),
                }
                // Keep values chosen before the file finished loading.
                let mut state = state;
                for (k, v) in std::mem::take(&mut self.user_state.active_environment) {
                    state.active_environment.insert(k, v);
                }
                self.user_state = state;
                self.user_data_loaded = true;
                self.apply_saved_theme();
                if std::mem::take(&mut self.user_data_dirty) {
                    self.save_user_data();
                }
                if let Some(p) = self.project.as_mut()
                    && p.active_env.is_none()
                    && let Some(id) = self.user_state.active_environment.get(&p.root_key())
                    && p.data.environments.iter().any(|e| &e.id == id)
                {
                    p.active_env = Some(id.clone());
                }
            }
            JobResult::CollectionSaved {
                tab,
                name,
                result,
                then,
            } => {
                if let Some(key) = tab
                    && let Some(t) = self.http_tab_mut(key)
                {
                    t.saving = false;
                    if result.is_err() {
                        t.saved = None;
                    }
                }
                match result {
                    Ok(omitted) => {
                        if !omitted.is_empty() {
                            self.notice_info(format!(
                                "Saved \"{name}\" without these credentials (not written to project files): {}. Use secret {{{{variables}}}} to keep them.",
                                omitted.join(", ")
                            ));
                        }
                        if let Some(key) = tab {
                            self.after_save(key, then);
                        }
                    }
                    Err(e) => {
                        self.quit_after_saves = false;
                        self.notice_error(format!(
                            "Could not save collection \"{name}\": {e}. Your changes are still open."
                        ));
                    }
                }
            }
            JobResult::CollectionImported(result) => match result {
                Ok(c) => {
                    let msg = format!(
                        "Imported collection \"{}\" ({} requests).",
                        c.name,
                        c.requests.len()
                    );
                    if let Some(p) = self.project.as_mut() {
                        p.data.collections.push(c);
                    }
                    self.notice_info(msg);
                }
                Err(e) => self.notice_error(format!("Import failed: {e}")),
            },
            JobResult::Done { message, is_error } if !message.is_empty() => {
                self.notices.push(super::Notice {
                    is_error,
                    text: message,
                });
            }
            _ => {}
        }
    }
}
