//! egui front end. Translates input into commands on the editor/HTTP models
//! and presents their state. All slow work runs on background threads.

mod docs;
pub mod editor_view;
mod find;
mod http_view;
mod jobs;
mod recovery;
pub mod strings;
pub mod theme;
mod view;

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

pub use editor_view::EditorViewState;
pub use find::FindState;
use jobs::Jobs;
use recovery::{RecoveryEvent, RecoveryWorker};

use crate::cli::LaunchArgs;
use crate::editor::Document;
use crate::http::{
    ClientConfig, HttpError, HttpResponse, HttpService, Notify, RequestDraft, RequestId,
    ValidationError,
};
use crate::storage::recovery::{OrphanSession, default_root};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsePane {
    Body,
    Headers,
}

pub struct ResponseView {
    pub response: HttpResponse,
    /// Byte ranges of each line of `response.body_text`.
    pub lines: Vec<Range<usize>>,
    /// Some line is longer than the view shows.
    pub has_long_lines: bool,
    pub pane: ResponsePane,
}

pub enum HttpState {
    Idle,
    Invalid(ValidationError),
    Sending { id: RequestId, started: Instant },
    Cancelled,
    Failed(HttpError),
    Done(Box<ResponseView>),
}

pub struct HttpTab {
    pub key: TabKey,
    pub title: String,
    pub draft: RequestDraft,
    pub state: HttpState,
}

pub enum Tab {
    Doc(Box<DocTab>),
    Http(Box<HttpTab>),
}

impl Tab {
    pub fn key(&self) -> TabKey {
        match self {
            Tab::Doc(t) => t.key,
            Tab::Http(t) => t.key,
        }
    }

    pub fn title(&self) -> String {
        match self {
            Tab::Doc(t) => t.doc.display_name(),
            Tab::Http(t) => t.title.clone(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        matches!(self, Tab::Doc(t) if t.doc.is_dirty())
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
    Conflict { tab: TabKey, then: AfterSave },
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
#[derive(Debug, Clone, Default)]
pub struct AppOptions {
    /// Where crash-recovery snapshots go; `None` uses the platform default.
    pub recovery_root: Option<PathBuf>,
    pub startup_probe: Option<Instant>,
}

impl SimpleEditorApp {
    pub fn new(ctx: &egui::Context, launch: LaunchArgs, options: AppOptions) -> Self {
        theme::apply(ctx);
        let repaint_ctx = ctx.clone();
        let notify: Notify = Arc::new(move || repaint_ctx.request_repaint());

        let mut notices = Vec::new();
        let http = match HttpService::new(ClientConfig::default(), notify.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                notices.push(Notice {
                    is_error: true,
                    text: format!("The HTTP client is unavailable: {e}"),
                });
                None
            }
        };
        let recovery = match options.recovery_root.or_else(default_root) {
            Some(root) => Some(RecoveryWorker::start(&root, notify.clone())),
            None => {
                notices.push(Notice {
                    is_error: true,
                    text: "Crash recovery is unavailable: no data directory was found.".to_owned(),
                });
                None
            }
        };
        let mut app = Self {
            workspace: Workspace::new(notify.clone()),
            tabs: Vec::new(),
            active: None,
            http,
            syntax: SyntaxService::new(notify.clone()),
            jobs: Jobs::new(notify),
            recovery,
            orphans: Vec::new(),
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
        if let Some(root) = launch.root {
            app.workspace.set_root(root);
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

    pub fn active_tab(&self) -> Option<&Tab> {
        let key = self.active?;
        self.tabs.iter().find(|t| t.key() == key)
    }

    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(Tab::is_dirty)
    }

    pub fn saves_in_flight(&self) -> bool {
        self.tabs
            .iter()
            .any(|t| matches!(t, Tab::Doc(d) if d.saving))
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
    }

    fn poll_http(&mut self) {
        let Some(http) = self.http.as_mut() else {
            return;
        };
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
            if let Some(tab) = tab {
                tab.state = match event.result {
                    Ok(response) => HttpState::Done(Box::new(ResponseView::new(response))),
                    Err(e) => HttpState::Failed(e),
                };
            }
        }
    }

    // ---- HTTP -------------------------------------------------------------

    pub fn new_request(&mut self) {
        let key = self.alloc_key();
        let title = format!("Request {}", self.next_request_number);
        self.next_request_number += 1;
        self.tabs.push(Tab::Http(Box::new(HttpTab {
            key,
            title,
            draft: RequestDraft::default(),
            state: HttpState::Idle,
        })));
        self.active = Some(key);
    }

    pub fn send_request(&mut self, key: TabKey) {
        let Some(Tab::Http(tab)) = self.tabs.iter_mut().find(|t| t.key() == key) else {
            return;
        };
        let Some(http) = self.http.as_mut() else {
            return;
        };
        if let HttpState::Sending { id, .. } = tab.state {
            http.cancel(id);
        }
        tab.state = match tab.draft.prepare() {
            Ok(prepared) => HttpState::Sending {
                id: http.send(prepared),
                started: Instant::now(),
            },
            Err(e) => HttpState::Invalid(e),
        };
    }

    pub fn cancel_request(&mut self, key: TabKey) {
        let Some(Tab::Http(tab)) = self.tabs.iter_mut().find(|t| t.key() == key) else {
            return;
        };
        if let (HttpState::Sending { id, .. }, Some(http)) = (&tab.state, self.http.as_mut()) {
            http.cancel(*id);
            tab.state = HttpState::Cancelled;
        }
    }

    // ---- tabs and quitting -------------------------------------------------

    /// Asks to close a tab; unsaved documents ask for confirmation first.
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
                if let (HttpState::Sending { id, .. }, Some(http)) = (&t.state, self.http.as_mut())
                {
                    http.cancel(*id);
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

    /// Saves every modified document; quits once all of them are saved.
    pub fn save_all_and_quit(&mut self) {
        self.quit_after_saves = true;
        let dirty: Vec<TabKey> = self
            .tabs
            .iter()
            .filter(|t| t.is_dirty())
            .map(Tab::key)
            .collect();
        for key in dirty {
            if !self.save_tab(key, false, false, AfterSave::Quit) {
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
        if let Some(r) = self.recovery.take() {
            r.finish();
        }
    }
}

impl ResponseView {
    fn new(response: HttpResponse) -> Self {
        let lines = response
            .body_text
            .as_deref()
            .map(line_ranges)
            .unwrap_or_default();
        let has_long_lines = lines.iter().any(|r| r.len() > http_view::MAX_LINE_CHARS);
        Self {
            response,
            lines,
            has_long_lines,
            pane: ResponsePane::Body,
        }
    }
}

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            ranges.push(start..i);
            start = i + 1;
        }
    }
    if start < text.len() || ranges.is_empty() {
        ranges.push(start..text.len());
    }
    ranges
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
