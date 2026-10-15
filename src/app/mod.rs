//! egui front end. Translates input into commands on the editor/HTTP models
//! and presents their state. All slow work runs on background threads.

mod docs;
pub mod editor_view;
mod env_view;
mod find;
mod http_app;
mod http_view;
mod jobs;
mod recovery;
mod session;
pub mod strings;
pub mod theme;
mod view;

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

pub use editor_view::EditorViewState;
pub use env_view::EnvTab;
pub use find::FindState;
pub use http_app::{HttpState, HttpTab, ProjectState, RequestPane, ResponsePane, ResponseView};
use jobs::Jobs;
use recovery::{RecoveryEvent, RecoveryWorker};

use crate::cli::LaunchArgs;
use crate::editor::Document;
use crate::http::secrets::{SecretBackend, default_backend};
use crate::http::{ClientConfig, HttpService, Notify};
use crate::storage::history::{HistoryEntry, UserState};
use crate::storage::recovery::OrphanSession;
use crate::syntax::{HighlightSpans, Lang, SyntaxService};
use crate::workspace::Workspace;

pub type TabKey = u64;

/// Snapshot unsaved buffers for crash recovery after this much idle time.
const RECOVERY_DELAY: Duration = Duration::from_millis(1500);
/// Large documents are rewritten in full on every snapshot, so less often.
const RECOVERY_DELAY_LARGE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskNotice {
    /// Modified by another program while this buffer has unsaved changes.
    Changed,
    /// Deleted or moved by another program.
    Deleted,
}

pub struct DocTab {
    pub key: TabKey,
    pub doc: Document,
    pub view: EditorViewState,
    pub lang: Option<Lang>,
    /// `None` when highlighting is off (unknown language or large file).
    pub highlight: Option<HighlightSpans>,
    requested_highlight: Option<(u64, Range<usize>)>,
    pub find: Option<FindState>,
    pub saving: bool,
    pub disk_notice: Option<DiskNotice>,
    /// Revision last written to the recovery store (None = no entry).
    recovery_written: Option<u64>,
    last_edit: Instant,
}

pub enum Tab {
    Doc(Box<DocTab>),
    Http(Box<HttpTab>),
    Env(Box<EnvTab>),
}

impl Tab {
    pub fn key(&self) -> TabKey {
        match self {
            Tab::Doc(t) => t.key,
            Tab::Http(t) => t.key,
            Tab::Env(t) => t.key,
        }
    }

    pub fn title(&self) -> String {
        match self {
            Tab::Doc(t) => t.doc.display_name(),
            Tab::Http(t) => t.def.name.clone(),
            Tab::Env(_) => strings::ENVIRONMENTS.to_owned(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        match self {
            Tab::Doc(t) => t.doc.is_dirty(),
            Tab::Http(t) => t.is_dirty(),
            Tab::Env(t) => t.is_dirty(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterSave {
    Nothing,
    CloseTab(TabKey),
    Quit,
}

pub enum Dialog {
    CloseTab(TabKey),
    Quit,
    Conflict {
        tab: TabKey,
        then: AfterSave,
    },
    /// Choose a collection and name for an unsaved request.
    SaveRequest {
        tab: TabKey,
        collection: Option<String>,
        new_collection: String,
        name: String,
        then: AfterSave,
    },
    ImportCurl {
        text: String,
        error: Option<String>,
    },
    DeleteCollection(String),
    /// (collection id, request id)
    DeleteRequest(String, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub is_error: bool,
    pub text: String,
}

pub struct SimpleEditorApp {
    pub workspace: Workspace,
    pub tabs: Vec<Tab>,
    pub active: Option<TabKey>,
    http: Option<HttpService>,
    syntax: SyntaxService,
    jobs: Jobs,
    recovery: Option<RecoveryWorker>,
    /// Sessions from previous runs with unsaved buffers, awaiting a decision.
    pub orphans: Vec<OrphanSession>,
    /// Collections, environments and variables of the open folder.
    pub project: Option<ProjectState>,
    pub history: Vec<HistoryEntry>,
    user_state: UserState,
    /// History/state were read (or found missing); writes wait for this.
    user_data_loaded: bool,
    /// A write was requested before the user data finished loading.
    user_data_dirty: bool,
    /// Project whose last session should be restored once loaded.
    pending_restore: Option<PathBuf>,
    restore: Option<session::RestoreState>,
    data_dir: Option<PathBuf>,
    secrets: Arc<dyn SecretBackend>,
    /// Limit for "Send and save to file", in MiB (session setting).
    pub download_limit_mib: u64,
    pub notices: Vec<Notice>,
    pub dialog: Option<Dialog>,
    allow_close: bool,
    quit_requested: bool,
    quit_after_saves: bool,
    next_key: TabKey,
    next_request_number: u32,
    pub theme_preference: egui::ThemePreference,
    startup_probe: Option<Instant>,
    frames: u64,
    ctx: egui::Context,
}

/// Options that tests and tools may change.
#[derive(Clone, Default)]
pub struct AppOptions {
    /// Where user data (recovery, history, state) goes; `None` uses the
    /// platform default (or `$SE_DATA_DIR`).
    pub data_dir: Option<PathBuf>,
    pub startup_probe: Option<Instant>,
    /// Secret store; `None` uses the OS credential store.
    pub secrets: Option<Arc<dyn SecretBackend>>,
    /// HTTP client settings (e.g. extra trusted certificates for tests).
    pub client: Option<ClientConfig>,
}

impl SimpleEditorApp {
    pub fn new(ctx: &egui::Context, launch: LaunchArgs, options: AppOptions) -> Self {
        theme::apply(ctx);
        let repaint_ctx = ctx.clone();
        let notify: Notify = Arc::new(move || repaint_ctx.request_repaint());

        let mut notices = Vec::new();
        let http = match HttpService::new(options.client.unwrap_or_default(), notify.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                notices.push(Notice {
                    is_error: true,
                    text: format!("The HTTP client is unavailable: {e}"),
                });
                None
            }
        };
        let data_dir = options.data_dir.or_else(crate::storage::data_dir);
        let recovery_root = data_dir.as_ref().map(|d| d.join("recovery"));
        let recovery = match recovery_root {
            Some(root) => Some(RecoveryWorker::start(&root, notify.clone())),
            None => {
                notices.push(Notice {
                    is_error: true,
                    text:
                        "Crash recovery and history are unavailable: no data directory was found."
                            .to_owned(),
                });
                None
            }
        };
        let jobs = Jobs::new(notify.clone());
        if let Some(dir) = &data_dir {
            jobs.load_user_data(dir.clone());
        }
        let mut app = Self {
            workspace: Workspace::new(notify.clone()),
            tabs: Vec::new(),
            active: None,
            http,
            syntax: SyntaxService::new(notify),
            jobs,
            recovery,
            orphans: Vec::new(),
            project: None,
            history: Vec::new(),
            user_state: UserState::default(),
            user_data_loaded: false,
            user_data_dirty: false,
            pending_restore: None,
            restore: None,
            data_dir,
            secrets: options.secrets.unwrap_or_else(default_backend),
            download_limit_mib: crate::http::client::DEFAULT_DOWNLOAD_LIMIT / (1024 * 1024),
            notices,
            dialog: None,
            allow_close: false,
            quit_requested: false,
            quit_after_saves: false,
            next_key: 1,
            next_request_number: 1,
            theme_preference: egui::ThemePreference::System,
            startup_probe: options.startup_probe,
            frames: 0,
            ctx: ctx.clone(),
        };
        if app.data_dir.is_none() {
            app.user_data_loaded = true;
        }
        if let Some(root) = launch.root {
            // `se .` restores the folder's last session; `se file` does not.
            if launch.files.is_empty() {
                app.pending_restore = Some(root.clone());
            }
            app.open_folder(root);
        }
        for file in launch.files {
            app.open_path(file);
        }
        app
    }

    fn alloc_key(&mut self) -> TabKey {
        let key = self.next_key;
        self.next_key += 1;
        key
    }

    pub fn notice_error(&mut self, text: impl Into<String>) {
        self.notices.push(Notice {
            is_error: true,
            text: text.into(),
        });
    }

    pub fn notice_info(&mut self, text: impl Into<String>) {
        self.notices.push(Notice {
            is_error: false,
            text: text.into(),
        });
    }

    pub fn tab_index(&self, key: TabKey) -> Option<usize> {
        self.tabs.iter().position(|t| t.key() == key)
    }

    pub fn doc_tab(&self, key: TabKey) -> Option<&DocTab> {
        self.tabs.iter().find_map(|t| match t {
            Tab::Doc(d) if d.key == key => Some(d.as_ref()),
            _ => None,
        })
    }

    pub fn doc_tab_mut(&mut self, key: TabKey) -> Option<&mut DocTab> {
        self.tabs.iter_mut().find_map(|t| match t {
            Tab::Doc(d) if d.key == key => Some(d.as_mut()),
            _ => None,
        })
    }

    pub fn http_tab(&self, key: TabKey) -> Option<&HttpTab> {
        self.tabs.iter().find_map(|t| match t {
            Tab::Http(h) if h.key == key => Some(h.as_ref()),
            _ => None,
        })
    }

    pub fn http_tab_mut(&mut self, key: TabKey) -> Option<&mut HttpTab> {
        self.tabs.iter_mut().find_map(|t| match t {
            Tab::Http(h) if h.key == key => Some(h.as_mut()),
            _ => None,
        })
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let key = self.active?;
        self.tabs.iter().find(|t| t.key() == key)
    }

    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(Tab::is_dirty)
    }

    pub fn saves_in_flight(&self) -> bool {
        self.tabs.iter().any(|t| match t {
            Tab::Doc(d) => d.saving,
            Tab::Http(h) => h.saving,
            Tab::Env(e) => e.saving,
        })
    }

    /// Opens a project folder: file tree and `.simpleeditor/` data.
    pub fn open_folder(&mut self, root: PathBuf) {
        self.workspace.set_root(root.clone());
        self.project = Some(ProjectState::loading(root.clone()));
        self.jobs.load_project(root, self.secrets.clone());
    }

    /// Background work results; called every frame before drawing.
    pub fn poll(&mut self) {
        for (path, result) in self.workspace.poll() {
            match result {
                Ok(doc) => self.push_doc(doc),
                Err(e) => self.notice_error(format!("Could not open {}: {e}.", path.display())),
            }
        }
        for result in self.jobs.poll() {
            self.on_job(result);
        }
        if let Some(recovery) = &self.recovery {
            for event in recovery.poll() {
                match event {
                    RecoveryEvent::Orphans(found) => self.orphans.extend(found),
                    RecoveryEvent::Failed(text) => self.notices.push(Notice {
                        is_error: true,
                        text,
                    }),
                }
            }
        }
        for result in self.syntax.poll() {
            for tab in &mut self.tabs {
                if let Tab::Doc(d) = tab
                    && d.doc.id() == result.doc
                    && let Some(h) = d.highlight.as_mut()
                {
                    h.accept(result.revision, result.range.clone(), result.spans.clone());
                }
            }
        }
        self.poll_http();
        self.sync_documents();
        self.maybe_restore_session();
    }

    // ---- tabs and quitting -------------------------------------------------

    /// Asks to close a tab; unsaved work asks for confirmation first.
    pub fn request_close_tab(&mut self, key: TabKey) {
        let Some(index) = self.tab_index(key) else {
            return;
        };
        if self.tabs[index].is_dirty() {
            self.active = Some(key);
            self.dialog = Some(Dialog::CloseTab(key));
        } else {
            self.close_tab_now(key);
        }
    }

    pub fn close_tab_now(&mut self, key: TabKey) {
        let Some(index) = self.tab_index(key) else {
            return;
        };
        match &self.tabs[index] {
            Tab::Http(t) => {
                if let (Some(id), Some(http)) = (t.request_id(), self.http.as_mut()) {
                    http.cancel(id);
                }
            }
            Tab::Doc(d) => {
                self.syntax.close(d.doc.id());
                if d.recovery_written.is_some()
                    && let Some(r) = &self.recovery
                {
                    r.remove(d.key);
                }
            }
            Tab::Env(_) => {}
        }
        self.tabs.remove(index);
        if self.active == Some(key) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(Tab::key);
        }
    }

    /// Saves any tab kind; returns false if nothing was started.
    pub fn save_any(&mut self, key: TabKey, then: AfterSave) -> bool {
        match self.tabs.iter().find(|t| t.key() == key) {
            Some(Tab::Doc(_)) => self.save_tab(key, false, false, then),
            Some(Tab::Http(_)) => self.save_request(key, then),
            Some(Tab::Env(_)) => self.save_environments(key, then),
            None => false,
        }
    }

    /// Saves everything modified; quits once all saves have finished.
    pub fn save_all_and_quit(&mut self) {
        self.quit_after_saves = true;
        let dirty: Vec<TabKey> = self
            .tabs
            .iter()
            .filter(|t| t.is_dirty())
            .map(Tab::key)
            .collect();
        for key in dirty {
            if !self.save_any(key, AfterSave::Quit) {
                self.quit_after_saves = false;
                return;
            }
        }
        self.maybe_quit_after_saves();
    }

    fn maybe_quit_after_saves(&mut self) {
        if self.quit_after_saves && !self.any_dirty() && !self.saves_in_flight() {
            self.quit_now();
        }
    }

    fn after_save(&mut self, key: TabKey, then: AfterSave) {
        let still_dirty = self
            .tabs
            .iter()
            .find(|t| t.key() == key)
            .is_some_and(Tab::is_dirty);
        match then {
            AfterSave::Nothing => {}
            AfterSave::CloseTab(k) => {
                if !still_dirty {
                    self.close_tab_now(k);
                }
            }
            AfterSave::Quit => self.maybe_quit_after_saves(),
        }
    }

    pub fn quit_now(&mut self) {
        self.allow_close = true;
        self.quit_requested = true;
        self.dialog = None;
    }

    /// True once after the user confirmed quitting from a dialog.
    pub fn take_quit_request(&mut self) -> bool {
        std::mem::take(&mut self.quit_requested)
    }

    /// Called when the window manager asks to close. Returns true if the
    /// close must be cancelled to ask the user first.
    pub fn on_close_requested(&mut self) -> bool {
        if self.allow_close || !self.any_dirty() {
            self.allow_close = true;
            return false;
        }
        self.dialog = Some(Dialog::Quit);
        true
    }

    /// Clean shutdown: the user saved or discarded everything.
    pub fn shutdown(&mut self) {
        self.save_session_blocking();
        if let Some(r) = self.recovery.take() {
            r.finish();
        }
    }
}

impl eframe::App for SimpleEditorApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(true)))
        }) {
            self.check_disk_changes();
        }
        self.poll();
        if ctx.input(|i| i.viewport().close_requested()) && self.on_close_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
        self.frames += 1;
        if let Some(start) = self.startup_probe
            && self.frames == 2
        {
            eprintln!(
                "startup_probe: second frame built {:.1} ms after main()",
                start.elapsed().as_secs_f64() * 1000.0
            );
            self.allow_close = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self) {
        // Reached only after the close was allowed: everything was saved or
        // explicitly discarded, so the recovery copies are no longer needed.
        self.shutdown();
    }
}
