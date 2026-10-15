//! egui front end. Translates input into commands on the editor/HTTP models
//! and presents their state. All slow work runs on background threads.

mod http_view;
pub mod strings;
pub mod theme;
mod view;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;

use crate::cli::LaunchArgs;
use crate::editor::document::{LONG_LINE_WARNING_BYTES, line_col};
use crate::editor::{Document, SaveError};
use crate::http::{
    ClientConfig, HttpError, HttpResponse, HttpService, Notify, RequestDraft, RequestId,
    ValidationError,
};
use crate::workspace::Workspace;

pub type TabKey = u64;

pub struct DocTab {
    pub key: TabKey,
    pub doc: Document,
    cursor_char: usize,
    cursor_revision: u64,
    pub line_col: (usize, usize),
}

impl DocTab {
    fn update_cursor(&mut self, char_index: usize) {
        if char_index != self.cursor_char || self.doc.revision() != self.cursor_revision {
            self.cursor_char = char_index;
            self.cursor_revision = self.doc.revision();
            self.line_col = line_col(&self.doc.text, char_index);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsePane {
    Body,
    Headers,
}

pub struct ResponseView {
    pub response: HttpResponse,
    /// Byte ranges of each line of `response.body_text`.
    pub lines: Vec<std::ops::Range<usize>>,
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
    pub notices: Vec<Notice>,
    pub dialog: Option<Dialog>,
    allow_close: bool,
    quit_requested: bool,
    next_key: TabKey,
    next_request_number: u32,
    pub theme_preference: egui::ThemePreference,
    startup_probe: Option<Instant>,
    frames: u64,
}

impl SimpleEditorApp {
    pub fn new(ctx: &egui::Context, launch: LaunchArgs, startup_probe: Option<Instant>) -> Self {
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
        let mut app = Self {
            workspace: Workspace::new(notify),
            tabs: Vec::new(),
            active: None,
            http,
            notices,
            dialog: None,
            allow_close: false,
            quit_requested: false,
            next_key: 1,
            next_request_number: 1,
            theme_preference: egui::ThemePreference::System,
            startup_probe,
            frames: 0,
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

    pub fn tab_index(&self, key: TabKey) -> Option<usize> {
        self.tabs.iter().position(|t| t.key() == key)
    }

    fn doc_tab_mut(&mut self, key: TabKey) -> Option<&mut DocTab> {
        self.tabs.iter_mut().find_map(|t| match t {
            Tab::Doc(d) if d.key == key => Some(d.as_mut()),
            _ => None,
        })
    }

    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(Tab::is_dirty)
    }

    /// Background work results; called every frame before drawing.
    pub fn poll(&mut self) {
        for (path, result) in self.workspace.poll() {
            match result {
                Ok(doc) => self.push_doc(doc),
                Err(e) => self.notice_error(format!("Could not open {}: {e}.", path.display())),
            }
        }
        let Some(http) = self.http.as_mut() else {
            return;
        };
        for event in http.poll() {
            // Only the latest request of a tab may update it.
            let tab = self.tabs.iter_mut().find_map(|t| match t {
                Tab::Http(h) if matches!(h.state, HttpState::Sending { id, .. } if id == event.id) => {
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

    // ---- commands -------------------------------------------------------

    pub fn open_path(&mut self, path: PathBuf) {
        if let Some(key) = self.tabs.iter().find_map(|t| match t {
            Tab::Doc(d) if d.doc.path() == Some(path.as_path()) => Some(d.key),
            _ => None,
        }) {
            self.active = Some(key);
            return;
        }
        self.workspace.request_open(path);
    }

    fn push_doc(&mut self, doc: Document) {
        if let Some(path) = doc.path()
            && let Some(key) = self.tabs.iter().find_map(|t| match t {
                Tab::Doc(d) if d.doc.path() == Some(path) => Some(d.key),
                _ => None,
            })
        {
            self.active = Some(key);
            return;
        }
        if doc.longest_line_at_open() > LONG_LINE_WARNING_BYTES {
            self.notices.push(Notice {
                is_error: false,
                text: format!(
                    "{} has a line of {} KiB; editing it may be slow in this preview build.",
                    doc.display_name(),
                    doc.longest_line_at_open() / 1024
                ),
            });
        }
        let key = self.alloc_key();
        self.tabs.push(Tab::Doc(Box::new(DocTab {
            key,
            doc,
            cursor_char: 0,
            cursor_revision: 0,
            line_col: (1, 1),
        })));
        self.active = Some(key);
    }

    pub fn new_file(&mut self) {
        self.push_doc(Document::untitled());
    }

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

    pub fn open_file_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(root) = &self.workspace.root {
            dialog = dialog.set_directory(root);
        }
        if let Some(paths) = dialog.pick_files() {
            for path in paths {
                self.open_path(path);
            }
        }
    }

    pub fn open_folder_dialog(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.workspace.set_root(dir);
        }
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
        if let Tab::Http(t) = &self.tabs[index]
            && let (HttpState::Sending { id, .. }, Some(http)) = (&t.state, self.http.as_mut())
        {
            http.cancel(*id);
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

    pub fn save_active(&mut self, choose_path: bool) {
        if let Some(key) = self.active
            && matches!(
                self.tabs.get(self.tab_index(key).unwrap_or(usize::MAX)),
                Some(Tab::Doc(_))
            )
        {
            self.save_tab(key, choose_path, false, AfterSave::Nothing);
        }
    }

    /// Saves a document tab and then runs `then`. Returns true if saved.
    pub fn save_tab(
        &mut self,
        key: TabKey,
        choose_path: bool,
        overwrite: bool,
        then: AfterSave,
    ) -> bool {
        let default_dir = self.workspace.root.clone();
        let Some(tab) = self.doc_tab_mut(key) else {
            return false;
        };
        let result = if choose_path || tab.doc.path().is_none() {
            match pick_save_path(&tab.doc, default_dir.as_deref()) {
                Some(path) => tab.doc.save_as(path),
                None => return false,
            }
        } else if overwrite {
            tab.doc.save_overwriting()
        } else {
            tab.doc.save()
        };
        let name = tab.doc.display_name();
        match result {
            Ok(()) => {
                self.after_save(then);
                true
            }
            Err(SaveError::ModifiedOnDisk) => {
                self.active = Some(key);
                self.dialog = Some(Dialog::Conflict { tab: key, then });
                false
            }
            Err(SaveError::NoPath) => false,
            Err(SaveError::Io(e)) => {
                self.notice_error(format!(
                    "Could not save {name}: {e}. The original file was not changed and your edits are still open."
                ));
                false
            }
        }
    }

    fn after_save(&mut self, then: AfterSave) {
        match then {
            AfterSave::Nothing => {}
            AfterSave::CloseTab(key) => self.close_tab_now(key),
            AfterSave::Quit => self.save_all_and_quit(),
        }
    }

    /// Saves every modified document; quits only if all of them were saved.
    pub fn save_all_and_quit(&mut self) {
        let dirty: Vec<TabKey> = self
            .tabs
            .iter()
            .filter(|t| t.is_dirty())
            .map(Tab::key)
            .collect();
        for key in dirty {
            if !self.save_tab(key, false, false, AfterSave::Nothing) {
                return;
            }
        }
        self.quit_now();
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
}

fn pick_save_path(doc: &Document, default_dir: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_file_name(doc.display_name());
    if let Some(dir) = doc.path().and_then(Path::parent).or(default_dir) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
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

fn line_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
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
}
