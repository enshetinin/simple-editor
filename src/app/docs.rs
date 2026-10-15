//! Document lifecycle in the app: open, save (on a worker), external
//! changes, syntax plumbing and crash recovery.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::jobs::JobResult;
use super::{
    AfterSave, Dialog, DiskNotice, DocTab, EditorViewState, RECOVERY_DELAY, RECOVERY_DELAY_LARGE,
    SimpleEditorApp, Tab, TabKey,
};
use crate::editor::{Document, SaveError};
use crate::storage::recovery::RecoveryMeta;
use crate::syntax::{HighlightSpans, Lang};

/// Lines above and below the viewport included in highlight requests.
const HIGHLIGHT_MARGIN_LINES: usize = 200;

impl SimpleEditorApp {
    pub fn open_path(&mut self, path: PathBuf) {
        if let Some(key) = self.find_doc_by_path(&path) {
            self.active = Some(key);
            return;
        }
        self.workspace.request_open(path);
    }

    fn find_doc_by_path(&self, path: &Path) -> Option<TabKey> {
        self.tabs.iter().find_map(|t| match t {
            Tab::Doc(d) if d.doc.path() == Some(path) => Some(d.key),
            _ => None,
        })
    }

    pub(super) fn push_doc(&mut self, doc: Document) {
        if let Some(path) = doc.path()
            && let Some(key) = self.find_doc_by_path(path)
        {
            self.active = Some(key);
            return;
        }
        let key = self.alloc_key();
        let mut tab = DocTab {
            key,
            doc,
            view: EditorViewState::default(),
            lang: None,
            highlight: None,
            requested_highlight: None,
            find: None,
            saving: false,
            disk_notice: None,
            recovery_written: None,
            last_edit: Instant::now(),
        };
        setup_syntax(&self.syntax, &mut tab);
        if tab.doc.longest_line() > crate::editor::document::LONG_LINE_CHARS {
            self.notice_info(format!(
                "{} has lines longer than {} characters; they are shown without highlighting.",
                tab.doc.display_name(),
                crate::editor::document::LONG_LINE_CHARS
            ));
        }
        let path = tab.doc.path().map(Path::to_path_buf);
        self.tabs.push(Tab::Doc(Box::new(tab)));
        let previous_active = self.active;
        self.active = Some(key);
        self.ctx
            .memory_mut(|m| m.request_focus(super::view::editor_id(key)));
        if let Some(path) = path {
            self.on_restored_doc(key, &path, previous_active);
        }
    }

    pub fn new_file(&mut self) {
        self.push_doc(Document::untitled());
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
            self.open_folder(dir);
        }
    }

    // ---- saving --------------------------------------------------------------

    pub fn save_active(&mut self, choose_path: bool) {
        if let Some(key) = self.active
            && self.doc_tab(key).is_some()
        {
            self.save_tab(key, choose_path, false, AfterSave::Nothing);
        }
    }

    /// Starts saving a document on a worker. Returns false if nothing was
    /// started (cancelled path dialog, save already running).
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
        if tab.saving {
            return true; // the running save will report back
        }
        let path = if choose_path || tab.doc.path().is_none() {
            match pick_save_path(&tab.doc, default_dir.as_deref()) {
                Some(path) => Some(path),
                None => return false,
            }
        } else {
            None
        };
        match tab.doc.save_job(path, overwrite) {
            Ok(job) => {
                tab.saving = true;
                self.jobs.save(key, job, then);
                true
            }
            Err(e) => {
                let name = tab.doc.display_name();
                self.notice_error(format!("Could not save {name}: {e}."));
                false
            }
        }
    }

    fn on_saved(
        &mut self,
        key: TabKey,
        job: crate::editor::SaveJob,
        result: Result<crate::editor::save::DiskStamp, SaveError>,
        then: AfterSave,
    ) {
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        tab.saving = false;
        let name = tab.doc.display_name();
        match result {
            Ok(stamp) => {
                let path_changed = tab.doc.path() != Some(job.path.as_path());
                tab.doc.finish_save(&job, stamp);
                tab.disk_notice = None;
                let still_dirty = tab.doc.is_dirty();
                if path_changed && let Some(tab) = find_doc_mut(&mut self.tabs, key) {
                    setup_syntax(&self.syntax, tab);
                }
                if !still_dirty {
                    self.drop_recovery(key);
                }
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
            Err(SaveError::ModifiedOnDisk) => {
                self.quit_after_saves = false;
                self.active = Some(key);
                self.dialog = Some(Dialog::Conflict { tab: key, then });
            }
            Err(e) => {
                self.quit_after_saves = false;
                self.notice_error(format!(
                    "Could not save {name}: {e}. The original file was not changed and your edits are still open."
                ));
            }
        }
    }

    pub(super) fn on_job(&mut self, result: JobResult) {
        match result {
            JobResult::Saved {
                tab,
                job,
                result,
                then,
            } => self.on_saved(tab, job, result, then),
            JobResult::Reloaded { tab, result } => {
                let Some(t) = self.doc_tab_mut(tab) else {
                    return;
                };
                match result {
                    Ok(loaded) => {
                        t.doc.reload(loaded);
                        t.disk_notice = None;
                        let mut owned_highlight = t.highlight.take();
                        if let Some(h) = owned_highlight.as_mut() {
                            h.clear();
                        }
                        t.highlight = owned_highlight;
                        t.requested_highlight = None;
                        let edits = t.doc.take_syntax_edits();
                        let (id, rev, rope) = (t.doc.id(), t.doc.revision(), t.doc.rope().clone());
                        if let Some(h) = t.highlight.as_mut() {
                            h.revision = rev;
                            self.syntax.edit(id, rev, rope, edits);
                        }
                        self.drop_recovery(tab);
                    }
                    Err(e) => {
                        let name = t.doc.display_name();
                        self.notice_error(format!("Could not reload {name}: {e}."));
                    }
                }
            }
            JobResult::DiskChecked { tab, stamp } => self.on_disk_checked(tab, stamp),
            JobResult::Searched {
                tab,
                generation,
                revision,
                query,
                result,
            } => {
                if let Some(t) = self.doc_tab_mut(tab)
                    && let Some(find) = t.find.as_mut()
                {
                    find.on_result(generation, revision, query, result);
                }
            }
            JobResult::Restored {
                entry,
                loaded,
                text,
            } => self.on_restored(entry, loaded, text),
            env @ JobResult::EnvironmentsSaved { .. } => self.on_env_saved(env),
            other => self.on_http_job(other),
        }
    }

    // ---- external changes ------------------------------------------------

    /// Checks open files for changes made by other programs (on focus).
    pub fn check_disk_changes(&mut self) {
        let tabs: Vec<(TabKey, PathBuf)> = self
            .tabs
            .iter()
            .filter_map(|t| match t {
                Tab::Doc(d) if !d.saving && d.doc.disk_stamp().is_some() => {
                    d.doc.path().map(|p| (d.key, p.to_path_buf()))
                }
                _ => None,
            })
            .collect();
        self.jobs.check_disk(tabs);
    }

    fn on_disk_checked(&mut self, key: TabKey, stamp: Option<crate::editor::save::DiskStamp>) {
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        if tab.saving || stamp == tab.doc.disk_stamp() {
            return;
        }
        let name = tab.doc.display_name();
        match stamp {
            None => {
                if tab.disk_notice != Some(DiskNotice::Deleted) {
                    tab.disk_notice = Some(DiskNotice::Deleted);
                    tab.doc.mark_disk_diverged();
                }
            }
            Some(_) if !tab.doc.is_dirty() => {
                // Nothing to lose: follow the file on disk.
                if let Some(path) = tab.doc.path().map(Path::to_path_buf) {
                    self.jobs.reload(key, path);
                    self.notice_info(format!("{name} changed on disk and was reloaded."));
                }
            }
            Some(_) => tab.disk_notice = Some(DiskNotice::Changed),
        }
    }

    /// User chose to discard their changes and load the file from disk.
    pub fn reload_tab(&mut self, key: TabKey) {
        if let Some(path) = self
            .doc_tab(key)
            .and_then(|t| t.doc.path().map(Path::to_path_buf))
        {
            self.jobs.reload(key, path);
        }
    }

    /// User chose to keep their version; the next save will ask to overwrite.
    pub fn keep_mine(&mut self, key: TabKey) {
        if let Some(tab) = self.doc_tab_mut(key) {
            tab.disk_notice = None;
            tab.doc.mark_disk_diverged();
        }
    }

    // ---- per-frame document sync ---------------------------------------------

    /// Sends edits and visible ranges to the syntax worker and schedules
    /// recovery snapshots.
    pub(super) fn sync_documents(&mut self) {
        let now = Instant::now();
        let mut snapshot_due_in = None;
        for tab in &mut self.tabs {
            let Tab::Doc(t) = tab else { continue };
            let edits = t.doc.take_syntax_edits();
            if !edits.is_empty() {
                t.last_edit = now;
            }
            if let Some(h) = t.highlight.as_mut() {
                if !edits.is_empty() {
                    let rev = t.doc.revision();
                    let base = rev - edits.len() as u64;
                    for (i, e) in edits.iter().enumerate() {
                        h.on_edit(base + i as u64 + 1, *e);
                    }
                    self.syntax
                        .edit(t.doc.id(), rev, t.doc.rope().clone(), edits);
                }
                let rope = t.doc.rope();
                let lines = &t.view.visible_lines;
                let first = lines.start.saturating_sub(HIGHLIGHT_MARGIN_LINES);
                let last = (lines.end + HIGHLIGHT_MARGIN_LINES).min(rope.len_lines());
                let range = rope.line_to_byte(first)..rope.line_to_byte(last.max(first));
                let want = (t.doc.revision(), range.clone());
                if t.requested_highlight.as_ref() != Some(&want) && !range.is_empty() {
                    self.syntax.request(t.doc.id(), range);
                    t.requested_highlight = Some(want);
                }
            }

            // Crash recovery snapshots.
            let Some(recovery) = &self.recovery else {
                continue;
            };
            if t.doc.is_dirty() {
                if t.recovery_written != Some(t.doc.revision()) {
                    let idle = now.duration_since(t.last_edit);
                    let delay = if t.doc.is_large() {
                        RECOVERY_DELAY_LARGE
                    } else {
                        RECOVERY_DELAY
                    };
                    if idle >= delay {
                        let meta = RecoveryMeta::new(
                            t.doc.path().map(Path::to_path_buf),
                            t.doc.display_name(),
                            t.doc.format(),
                        );
                        recovery.write(t.key, meta, t.doc.rope().clone());
                        t.recovery_written = Some(t.doc.revision());
                    } else {
                        let left = delay - idle;
                        snapshot_due_in = Some(
                            snapshot_due_in.map_or(left, |d: std::time::Duration| d.min(left)),
                        );
                    }
                }
            } else if t.recovery_written.take().is_some() {
                recovery.remove(t.key);
            }
        }
        if let Some(delay) = snapshot_due_in {
            self.ctx.request_repaint_after(delay);
        }
    }

    fn drop_recovery(&mut self, key: TabKey) {
        if let Some(tab) = self.doc_tab_mut(key)
            && tab.recovery_written.take().is_some()
            && let Some(r) = &self.recovery
        {
            r.remove(key);
        }
    }

    // ---- restoring from a previous session -------------------------------

    pub fn restore_orphans(&mut self) {
        for session in &self.orphans {
            for entry in &session.entries {
                self.jobs.restore(entry.clone());
            }
        }
    }

    pub fn discard_orphans(&mut self) {
        for mut session in std::mem::take(&mut self.orphans) {
            for entry in &session.entries {
                let _ = entry.remove();
            }
            session.cleanup_if_empty();
        }
    }

    fn on_restored(
        &mut self,
        entry: crate::storage::recovery::RecoveredEntry,
        loaded: Option<crate::editor::LoadedText>,
        text: std::io::Result<String>,
    ) {
        let text = match text {
            Ok(t) => t,
            Err(e) => {
                self.notice_error(format!(
                    "Could not read the recovered copy of {}: {e}. It was kept in {}.",
                    entry.meta.display_name,
                    entry.content_path.display()
                ));
                return;
            }
        };
        let path = entry.meta.original_path.clone();
        let mut doc = match (loaded, path) {
            // The file still exists: open it and apply the recovered text as
            // an unsaved (undoable) change. Nothing is written to it.
            (Some(loaded), Some(path)) => Document::from_loaded(path, loaded),
            (_, Some(path)) => {
                let mut d = Document::new_at(path);
                d.set_format(entry.meta.format());
                d
            }
            (_, None) => {
                let mut d = Document::untitled();
                d.set_format(entry.meta.format());
                d
            }
        };
        doc.restore_text(&text);
        if let Some(existing) = doc.path().and_then(|p| self.find_doc_by_path(p)) {
            self.notice_info(format!(
                "A recovered copy of {} was not applied because the file is already open; it was kept in {}.",
                entry.meta.display_name,
                entry.content_path.display()
            ));
            self.active = Some(existing);
            return;
        }
        if !doc.is_dirty() {
            doc.mark_disk_diverged();
        }
        self.push_doc(doc);
        let _ = entry.remove();
        for session in &mut self.orphans {
            session.cleanup_if_empty();
        }
        self.orphans.retain(|s| !s.entries.is_empty());
    }
}

fn pick_save_path(doc: &Document, default_dir: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_file_name(doc.display_name());
    if let Some(dir) = doc.path().and_then(Path::parent).or(default_dir) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

fn find_doc_mut(tabs: &mut [Tab], key: TabKey) -> Option<&mut DocTab> {
    tabs.iter_mut().find_map(|t| match t {
        Tab::Doc(d) if d.key == key => Some(d.as_mut()),
        _ => None,
    })
}

/// (Re)starts highlighting for a tab according to its path and size.
fn setup_syntax(syntax: &crate::syntax::SyntaxService, tab: &mut DocTab) {
    let lang = tab.doc.path().and_then(Lang::from_path);
    if tab.lang.is_some() {
        syntax.close(tab.doc.id());
    }
    tab.lang = lang;
    tab.requested_highlight = None;
    tab.doc.take_syntax_edits();
    tab.highlight = match lang {
        Some(lang) if !tab.doc.is_large() => {
            syntax.open(
                tab.doc.id(),
                lang,
                tab.doc.rope().clone(),
                tab.doc.revision(),
            );
            Some(HighlightSpans::at_revision(tab.doc.revision()))
        }
        _ => None,
    };
}
