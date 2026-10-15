use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ropey::Rope;
use tree_sitter::{InputEdit, Point};

use super::history::{EditKind, History, Op};
use super::indent::{self, IndentStyle};
use super::movement;
use super::save::{DiskStamp, atomic_write_with};
use super::selection::Selection;
use super::text_format::{DecodeError, TextFormat, decode, encode_to, normalize_input};

/// Largest file that can be opened. Files are held in memory as a rope.
pub const MAX_OPEN_BYTES: u64 = 256 * 1024 * 1024;

/// Above this size the document opens in large-file mode: syntax
/// highlighting is off (shown in the status bar); editing stays available.
pub const LARGE_FILE_BYTES: usize = 8 * 1024 * 1024;

/// Lines longer than this are drawn with a fixed-width approximation and
/// without highlighting, so a single huge line cannot stall a frame.
pub const LONG_LINE_CHARS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(u64);

impl DocumentId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub enum OpenError {
    Io(io::Error),
    Decode(DecodeError),
    TooLarge { len: u64 },
    NotAFile,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Io(e) => write!(f, "{e}"),
            OpenError::Decode(e) => write!(f, "{e}"),
            OpenError::TooLarge { len } => write!(
                f,
                "the file is {:.1} MiB; SimpleEditor opens files up to {} MiB",
                *len as f64 / (1024.0 * 1024.0),
                MAX_OPEN_BYTES / (1024 * 1024)
            ),
            OpenError::NotAFile => write!(f, "the path is not a regular file"),
        }
    }
}

#[derive(Debug)]
pub enum SaveError {
    /// The document has never been saved; a path must be chosen first.
    NoPath,
    /// The file changed on disk since it was opened or last saved.
    ModifiedOnDisk,
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NoPath => write!(f, "choose where to save the file"),
            SaveError::ModifiedOnDisk => write!(
                f,
                "the file was changed by another program since it was opened"
            ),
            SaveError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Text loaded from disk, ready to become (or replace) a document.
#[derive(Debug)]
pub struct LoadedText {
    pub rope: Rope,
    pub format: TextFormat,
    pub stamp: DiskStamp,
    pub longest_line: usize,
}

/// Reads and decodes a file. Meant to run off the UI thread.
pub fn load(path: &Path) -> Result<LoadedText, OpenError> {
    let meta = std::fs::metadata(path).map_err(OpenError::Io)?;
    if !meta.is_file() {
        return Err(OpenError::NotAFile);
    }
    if meta.len() > MAX_OPEN_BYTES {
        return Err(OpenError::TooLarge { len: meta.len() });
    }
    let stamp = DiskStamp::of(path).map_err(OpenError::Io)?;
    let bytes = std::fs::read(path).map_err(OpenError::Io)?;
    let (text, format) = decode(&bytes).map_err(OpenError::Decode)?;
    drop(bytes);
    let longest_line = text
        .split('\n')
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    Ok(LoadedText {
        rope: Rope::from_str(&text),
        format,
        stamp,
        longest_line,
    })
}

/// A snapshot to write to disk on a worker thread. Cloning a rope is O(1).
#[derive(Debug, Clone)]
pub struct SaveJob {
    pub doc: DocumentId,
    pub path: PathBuf,
    pub rope: Rope,
    pub format: TextFormat,
    /// Refuse to write if the file on disk no longer matches this.
    pub expected: Option<DiskStamp>,
    pub state_id: u64,
}

impl SaveJob {
    /// Writes the snapshot atomically. Returns the new disk stamp.
    pub fn run(&self) -> Result<DiskStamp, SaveError> {
        if let Some(expected) = self.expected
            && let Ok(current) = DiskStamp::of(&self.path)
            && current != expected
        {
            return Err(SaveError::ModifiedOnDisk);
        }
        atomic_write_with(&self.path, |w| {
            encode_to(self.rope.chunks(), &self.format, w)
        })
        .map_err(SaveError::Io)?;
        DiskStamp::of(&self.path).map_err(SaveError::Io)
    }
}

/// An open text document. Independent of any UI.
#[derive(Debug)]
pub struct Document {
    id: DocumentId,
    path: Option<PathBuf>,
    rope: Rope,
    format: TextFormat,
    indent: IndentStyle,
    selection: Selection,
    history: History,
    /// Increments on every content change (for workers and caches).
    revision: u64,
    /// History state that matches the file on disk.
    saved_state: u64,
    /// Disk content no longer matches any history state (e.g. after reload
    /// failed or the file was deleted); forces dirty until saved.
    force_dirty: bool,
    disk_stamp: Option<DiskStamp>,
    longest_line: usize,
    syntax_edits: Vec<InputEdit>,
    /// Preferred x position (in display columns) for vertical movement.
    pub preferred_column: Option<f32>,
}

impl Document {
    pub fn untitled() -> Self {
        Self::from_parts(None, Rope::new(), TextFormat::default(), None, 0)
    }

    /// A document bound to a path that does not exist yet (created on save).
    pub fn new_at(path: PathBuf) -> Self {
        Self::from_parts(Some(path), Rope::new(), TextFormat::default(), None, 0)
    }

    pub fn from_loaded(path: PathBuf, loaded: LoadedText) -> Self {
        Self::from_parts(
            Some(path),
            loaded.rope,
            loaded.format,
            Some(loaded.stamp),
            loaded.longest_line,
        )
    }

    pub fn from_text(text: &str) -> Self {
        let longest = text
            .split('\n')
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        Self::from_parts(
            None,
            Rope::from_str(text),
            TextFormat::default(),
            None,
            longest,
        )
    }

    fn from_parts(
        path: Option<PathBuf>,
        rope: Rope,
        format: TextFormat,
        disk_stamp: Option<DiskStamp>,
        longest_line: usize,
    ) -> Self {
        let indent = indent::detect(&rope);
        Self {
            id: DocumentId::next(),
            path,
            rope,
            format,
            indent,
            selection: Selection::default(),
            history: History::default(),
            revision: 0,
            saved_state: 0,
            force_dirty: false,
            disk_stamp,
            longest_line,
            syntax_edits: Vec::new(),
            preferred_column: None,
        }
    }

    pub fn open(path: &Path) -> Result<Self, OpenError> {
        load(path).map(|loaded| Self::from_loaded(path.to_path_buf(), loaded))
    }

    // ---- accessors --------------------------------------------------------

    pub fn id(&self) -> DocumentId {
        self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Full text as a String (O(n); for tests and small documents).
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn format(&self) -> TextFormat {
        self.format
    }

    pub fn indent(&self) -> IndentStyle {
        self.indent
    }

    pub fn set_indent(&mut self, indent: IndentStyle) {
        self.indent = indent;
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn disk_stamp(&self) -> Option<DiskStamp> {
        self.disk_stamp
    }

    pub fn is_dirty(&self) -> bool {
        self.force_dirty || self.history.state_id() != self.saved_state
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Longest line in chars (grows with edits; recomputed on reload).
    pub fn longest_line(&self) -> usize {
        self.longest_line
    }

    pub fn is_large(&self) -> bool {
        self.rope.len_bytes() > LARGE_FILE_BYTES
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    /// Tree-sitter edits produced since the last call, in application order.
    pub fn take_syntax_edits(&mut self) -> Vec<InputEdit> {
        std::mem::take(&mut self.syntax_edits)
    }

    pub fn selected_text(&self) -> String {
        self.rope.slice(self.selection.range()).to_string()
    }

    /// 0-based line and char column of a char index.
    pub fn line_col(&self, char_idx: usize) -> (usize, usize) {
        let char_idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(char_idx);
        (line, char_idx - self.rope.line_to_char(line))
    }

    /// Char index of the end of a line, before its `\n` (and `\r` if any).
    pub fn line_end(&self, line: usize) -> usize {
        let start = self.rope.line_to_char(line);
        let slice = self.rope.line(line);
        let mut len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len -= 1;
            if len > 0 && slice.char(len - 1) == '\r' {
                len -= 1;
            }
        }
        start + len
    }

    // ---- selection --------------------------------------------------------

    pub fn set_selection(&mut self, selection: Selection) {
        let selection = selection.clamped(self.rope.len_chars());
        if selection != self.selection {
            self.history.break_group();
        }
        self.selection = selection;
    }

    /// Moves the cursor (or the selection head when `extend`).
    pub fn move_to(&mut self, pos: usize, extend: bool) {
        let anchor = if extend { self.selection.anchor } else { pos };
        self.set_selection(Selection::new(anchor, pos));
    }

    pub fn select_all(&mut self) {
        self.set_selection(Selection::new(0, self.rope.len_chars()));
    }

    // ---- core mutation ----------------------------------------------------

    fn point_at(&self, byte: usize) -> Point {
        let line = self.rope.byte_to_line(byte);
        Point::new(line, byte - self.rope.line_to_byte(line))
    }

    /// Replaces `start..end` (chars) with `text`, recording the syntax edit.
    fn raw_replace(&mut self, start: usize, end: usize, text: &str) -> Op {
        let start_byte = self.rope.char_to_byte(start);
        let old_end_byte = self.rope.char_to_byte(end);
        let start_position = self.point_at(start_byte);
        let old_end_position = self.point_at(old_end_byte);
        let removed = self.rope.slice(start..end).to_string();
        if end > start {
            self.rope.remove(start..end);
        }
        if !text.is_empty() {
            self.rope.insert(start, text);
        }
        let new_end_byte = start_byte + text.len();
        let new_end_position = match text.rfind('\n') {
            Some(last_nl) => Point::new(
                start_position.row + text.matches('\n').count(),
                text.len() - last_nl - 1,
            ),
            None => Point::new(start_position.row, start_position.column + text.len()),
        };
        self.syntax_edits.push(InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position,
            old_end_position,
            new_end_position,
        });
        // Track the longest line cheaply: only lines touched by this edit.
        let first = self.rope.char_to_line(start);
        let last = self.rope.char_to_line(start + text.chars().count());
        for line in first..=last.min(first + 1000) {
            self.longest_line = self.longest_line.max(self.rope.line(line).len_chars());
        }
        self.revision += 1;
        Op {
            at: start,
            removed,
            inserted: text.to_owned(),
        }
    }

    /// Applies non-overlapping replacements given in pre-edit coordinates,
    /// as one undo step.
    pub fn apply(
        &mut self,
        mut changes: Vec<(usize, usize, String)>,
        after: Selection,
        kind: EditKind,
    ) {
        changes.sort_by_key(|c| c.0);
        let before = self.selection;
        let mut ops = Vec::with_capacity(changes.len());
        // Apply back to front so earlier coordinates stay valid.
        for (start, end, text) in changes.into_iter().rev() {
            if start == end && text.is_empty() {
                continue;
            }
            ops.push(self.raw_replace(start, end, &text));
        }
        self.selection = after.clamped(self.rope.len_chars());
        self.preferred_column = None;
        self.history.record(ops, before, self.selection, kind);
    }

    // ---- editing commands -------------------------------------------------

    /// Types or pastes text, replacing the selection.
    pub fn insert_text(&mut self, text: &str) {
        let kind = if text.chars().count() == 1 {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.insert_text_as(text, kind);
    }

    pub fn insert_text_as(&mut self, text: &str, kind: EditKind) {
        let text = normalize_input(text, self.format.line_ending);
        let sel = self.selection;
        let caret = sel.start() + text.chars().count();
        self.apply(
            vec![(sel.start(), sel.end(), text.into_owned())],
            Selection::caret(caret),
            kind,
        );
    }

    /// Enter: new line keeping the current line's leading whitespace.
    pub fn newline(&mut self) {
        let (line, _) = self.line_col(self.selection.start());
        let line_start = self.rope.line_to_char(line);
        let leading: String = self
            .rope
            .slice(line_start..self.selection.start())
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .collect();
        let newline = if self.format.line_ending == super::text_format::LineEnding::Mixed
            && self.line_uses_crlf(line)
        {
            "\r\n"
        } else {
            "\n"
        };
        self.insert_text_as(&format!("{newline}{leading}"), EditKind::Other);
    }

    fn line_uses_crlf(&self, line: usize) -> bool {
        let slice = self.rope.line(line);
        let len = slice.len_chars();
        len >= 2 && slice.char(len - 1) == '\n' && slice.char(len - 2) == '\r'
    }

    /// Backspace: deletes the selection, a grapheme, or back to the previous
    /// indent stop inside leading spaces.
    pub fn backspace(&mut self) {
        let sel = self.selection;
        if !sel.is_empty() {
            self.delete_range(sel.start(), sel.end(), EditKind::Other);
            return;
        }
        let pos = sel.head;
        if pos == 0 {
            return;
        }
        let (line, col) = self.line_col(pos);
        let line_start = self.rope.line_to_char(line);
        if let IndentStyle::Spaces(n) = self.indent
            && col > 0
            && self.rope.slice(line_start..pos).chars().all(|c| c == ' ')
        {
            let n = n as usize;
            let target = ((col - 1) / n) * n;
            self.delete_range(line_start + target, pos, EditKind::DeleteBackward);
            return;
        }
        let start = movement::prev_grapheme(&self.rope, pos);
        self.delete_range(start, pos, EditKind::DeleteBackward);
    }

    /// Delete key: the selection or the next grapheme.
    pub fn delete_forward(&mut self) {
        let sel = self.selection;
        if !sel.is_empty() {
            self.delete_range(sel.start(), sel.end(), EditKind::Other);
            return;
        }
        let end = movement::next_grapheme(&self.rope, sel.head);
        self.delete_range(sel.head, end, EditKind::Other);
    }

    pub fn delete_word_backward(&mut self) {
        let sel = self.selection;
        if !sel.is_empty() {
            self.delete_range(sel.start(), sel.end(), EditKind::Other);
            return;
        }
        let start = movement::prev_word(&self.rope, sel.head);
        self.delete_range(start, sel.head, EditKind::Other);
    }

    pub fn delete_range(&mut self, start: usize, end: usize, kind: EditKind) {
        if start >= end {
            return;
        }
        self.apply(
            vec![(start, end, String::new())],
            Selection::caret(start),
            kind,
        );
    }

    /// Lines covered by the selection (a selection ending at column 0 of a
    /// line does not include that line).
    fn selected_lines(&self) -> std::ops::RangeInclusive<usize> {
        let sel = self.selection;
        let first = self.rope.char_to_line(sel.start());
        let mut last = self.rope.char_to_line(sel.end());
        if last > first && sel.end() == self.rope.line_to_char(last) {
            last -= 1;
        }
        first..=last
    }

    /// Tab: indents selected lines, or inserts one indent unit.
    pub fn indent_or_insert_tab(&mut self) {
        let sel = self.selection;
        let lines = self.selected_lines();
        if sel.is_empty()
            || lines.start() == lines.end() && !self.selection_covers_line(*lines.start())
        {
            let unit = match self.indent {
                IndentStyle::Tabs => "\t".to_owned(),
                IndentStyle::Spaces(n) => {
                    let (_, col) = self.line_col(sel.start());
                    let n = n as usize;
                    " ".repeat(n - col % n)
                }
            };
            self.insert_text_as(&unit, EditKind::Other);
            return;
        }
        let unit = self.indent.unit();
        let unit_len = unit.chars().count();
        let changes: Vec<_> = lines
            .clone()
            .filter(|&l| !self.line_is_blank(l))
            .map(|l| {
                let at = self.rope.line_to_char(l);
                (at, at, unit.clone())
            })
            .collect();
        let shift_anchor = self.shift_for(sel.anchor, &changes, unit_len);
        let shift_head = self.shift_for(sel.head, &changes, unit_len);
        self.apply(
            changes,
            Selection::new(sel.anchor + shift_anchor, sel.head + shift_head),
            EditKind::Other,
        );
    }

    fn selection_covers_line(&self, line: usize) -> bool {
        let start = self.rope.line_to_char(line);
        self.selection.start() <= start && self.selection.end() >= self.line_end(line)
    }

    fn line_is_blank(&self, line: usize) -> bool {
        self.rope.line(line).chars().all(|c| c == '\n' || c == '\r')
    }

    fn shift_for(&self, pos: usize, inserts: &[(usize, usize, String)], unit_len: usize) -> usize {
        inserts.iter().filter(|c| c.0 <= pos).count() * unit_len
    }

    /// Shift+Tab: removes up to one indent unit from each selected line.
    pub fn outdent(&mut self) {
        let sel = self.selection;
        let width = self.indent.width(4);
        let mut changes = Vec::new();
        for line in self.selected_lines() {
            let start = self.rope.line_to_char(line);
            let mut remove = 0;
            for c in self.rope.line(line).chars() {
                match c {
                    '\t' if remove == 0 => {
                        remove = 1;
                        break;
                    }
                    ' ' if remove < width => remove += 1,
                    _ => break,
                }
            }
            if remove > 0 {
                changes.push((start, start + remove, String::new()));
            }
        }
        if changes.is_empty() {
            return;
        }
        let map = |pos: usize| {
            let mut out = pos;
            for (s, e, _) in &changes {
                if pos >= *e {
                    out -= e - s;
                } else if pos > *s {
                    out -= pos - s;
                }
            }
            out
        };
        let after = Selection::new(map(sel.anchor), map(sel.head));
        self.apply(changes, after, EditKind::Other);
    }

    /// Replaces each char range with `replacement` as a single undo step.
    pub fn replace_ranges(&mut self, ranges: &[std::ops::Range<usize>], replacement: &str) {
        if ranges.is_empty() {
            return;
        }
        let replacement = normalize_input(replacement, self.format.line_ending).into_owned();
        let rep_len = replacement.chars().count() as isize;
        let last = ranges
            .iter()
            .max_by_key(|r| r.start)
            .cloned()
            .unwrap_or(0..0);
        let delta: isize = ranges
            .iter()
            .filter(|r| r.start < last.start)
            .map(|r| rep_len - (r.end - r.start) as isize)
            .sum();
        let caret = (last.start as isize + delta + rep_len).max(0) as usize;
        let changes = ranges
            .iter()
            .map(|r| (r.start, r.end, replacement.clone()))
            .collect();
        self.apply(changes, Selection::caret(caret), EditKind::Other);
    }

    // ---- undo / redo ------------------------------------------------------

    pub fn undo(&mut self) -> bool {
        let Some(t) = self.history.pop_undo() else {
            return false;
        };
        for op in t.ops.iter().rev() {
            let end = op.at + op.inserted.chars().count();
            self.raw_replace(op.at, end, &op.removed);
        }
        self.selection = t.before.clamped(self.rope.len_chars());
        self.preferred_column = None;
        self.history.push_redo(t);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(t) = self.history.pop_redo() else {
            return false;
        };
        for op in &t.ops {
            let end = op.at + op.removed.chars().count();
            self.raw_replace(op.at, end, &op.inserted);
        }
        self.selection = t.after.clamped(self.rope.len_chars());
        self.preferred_column = None;
        self.history.push_undo(t);
        true
    }

    // ---- saving and reloading ----------------------------------------------

    /// Prepares a save to the current (or a new) path.
    pub fn save_job(&self, path: Option<PathBuf>, overwrite: bool) -> Result<SaveJob, SaveError> {
        let same_path = path.is_none();
        let path = path
            .or_else(|| self.path.clone())
            .ok_or(SaveError::NoPath)?;
        let expected = if overwrite || !same_path {
            None
        } else {
            self.disk_stamp
        };
        Ok(SaveJob {
            doc: self.id,
            path,
            rope: self.rope.clone(),
            format: self.format,
            expected,
            state_id: self.history.state_id(),
        })
    }

    /// Records a finished save.
    pub fn finish_save(&mut self, job: &SaveJob, stamp: DiskStamp) {
        self.path = Some(job.path.clone());
        self.disk_stamp = Some(stamp);
        self.saved_state = job.state_id;
        self.force_dirty = false;
    }

    /// Synchronous save (tests, small tools). The app saves on a worker.
    pub fn save(&mut self) -> Result<(), SaveError> {
        self.run_save(None, false)
    }

    pub fn save_overwriting(&mut self) -> Result<(), SaveError> {
        self.run_save(None, true)
    }

    pub fn save_as(&mut self, path: PathBuf) -> Result<(), SaveError> {
        self.run_save(Some(path), false)
    }

    fn run_save(&mut self, path: Option<PathBuf>, overwrite: bool) -> Result<(), SaveError> {
        let job = self.save_job(path, overwrite)?;
        let stamp = job.run()?;
        self.finish_save(&job, stamp);
        Ok(())
    }

    /// Replaces the content with a fresh load of the file (user chose Reload).
    pub fn reload(&mut self, loaded: LoadedText) {
        let len = loaded.rope.len_chars();
        let old_len_bytes = self.rope.len_bytes();
        let old_end = self.point_at(old_len_bytes);
        self.syntax_edits.push(InputEdit {
            start_byte: 0,
            old_end_byte: old_len_bytes,
            new_end_byte: loaded.rope.len_bytes(),
            start_position: Point::new(0, 0),
            old_end_position: old_end,
            new_end_position: Point::new(loaded.rope.len_lines().saturating_sub(1), 0),
        });
        self.rope = loaded.rope;
        self.format = loaded.format;
        self.disk_stamp = Some(loaded.stamp);
        self.longest_line = loaded.longest_line;
        self.indent = indent::detect(&self.rope);
        self.history.clear();
        self.saved_state = 0;
        self.force_dirty = false;
        self.revision += 1;
        self.selection = self.selection.clamped(len);
    }

    /// The file disappeared or diverged: keep the buffer marked as modified.
    pub fn mark_disk_diverged(&mut self) {
        self.force_dirty = true;
    }

    /// Recovered content replaces the loaded text as an undoable edit.
    pub fn restore_text(&mut self, text: &str) {
        let len = self.rope.len_chars();
        self.apply(
            vec![(0, len, text.to_owned())],
            Selection::caret(0),
            EditKind::Other,
        );
        self.longest_line = text
            .split('\n')
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
    }

    pub fn set_format(&mut self, format: TextFormat) {
        self.format = format;
    }
}

/// Human-readable language name derived from a path's extension.
pub fn language_name(path: Option<&Path>) -> &'static str {
    let ext = path
        .and_then(Path::extension)
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "Rust",
        "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "json" => "JSON",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "md" => "Markdown",
        "toml" => "TOML",
        "yml" | "yaml" => "YAML",
        _ => "Plain text",
    }
}

impl Document {
    pub fn language(&self) -> &'static str {
        language_name(self.path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::from_text(text)
    }

    #[test]
    fn typing_groups_into_words_and_undo_restores_selection() {
        let mut d = doc("");
        for c in "hello world".chars() {
            d.insert_text(&c.to_string());
        }
        assert_eq!(d.text(), "hello world");
        assert!(d.undo());
        assert_eq!(d.text(), "hello ");
        assert!(d.undo());
        assert_eq!(d.text(), "");
        assert!(!d.is_dirty(), "back at the loaded state");
        assert!(d.redo());
        assert!(d.redo());
        assert_eq!(d.text(), "hello world");
        assert_eq!(d.selection(), Selection::caret(11));
    }

    #[test]
    fn cursor_movement_breaks_typing_group() {
        let mut d = doc("");
        d.insert_text("a");
        d.insert_text("b");
        d.move_to(0, false);
        d.move_to(2, false);
        d.insert_text("c");
        d.undo();
        assert_eq!(d.text(), "ab");
    }

    #[test]
    fn backspace_deletes_whole_graphemes_and_groups() {
        let mut d = doc("a👨‍👩‍👧e\u{301}");
        d.move_to(d.len_chars(), false);
        d.backspace();
        assert_eq!(d.text(), "a👨‍👩‍👧");
        d.backspace();
        assert_eq!(d.text(), "a");
        d.undo();
        assert_eq!(
            d.text(),
            "a👨‍👩‍👧e\u{301}",
            "consecutive backspaces are one step"
        );
    }

    #[test]
    fn backspace_in_leading_spaces_goes_to_indent_stop() {
        let mut d = doc("fn x() {\n        y\n}");
        d.set_indent(IndentStyle::Spaces(4));
        d.move_to(9 + 8, false);
        d.backspace();
        assert_eq!(d.text(), "fn x() {\n    y\n}");
    }

    #[test]
    fn newline_keeps_indentation_and_crlf_docs_stay_lf_internally() {
        let mut d = doc("    let a = 1;");
        d.move_to(d.len_chars(), false);
        d.newline();
        d.insert_text("x");
        assert_eq!(d.text(), "    let a = 1;\n    x");
    }

    #[test]
    fn paste_normalizes_line_endings_and_is_one_step() {
        let mut d = doc("ab");
        d.move_to(1, false);
        d.insert_text("1\r\n2\r3");
        assert_eq!(d.text(), "a1\n2\n3b");
        d.undo();
        assert_eq!(d.text(), "ab");
        assert_eq!(d.selection(), Selection::caret(1));
    }

    #[test]
    fn indent_and_outdent_selected_lines() {
        let mut d = doc("a\nb\n\nc");
        d.set_indent(IndentStyle::Spaces(2));
        d.set_selection(Selection::new(0, d.len_chars()));
        d.indent_or_insert_tab();
        assert_eq!(d.text(), "  a\n  b\n\n  c");
        d.outdent();
        assert_eq!(d.text(), "a\nb\n\nc");
        d.undo();
        d.undo();
        assert_eq!(d.text(), "a\nb\n\nc");
        // Tab with a caret inserts spaces to the next stop.
        d.move_to(1, false);
        d.indent_or_insert_tab();
        assert_eq!(d.text(), "a \nb\n\nc");
    }

    #[test]
    fn replace_ranges_is_single_step_and_utf8_safe() {
        let mut d = doc("café café café");
        d.replace_ranges(&[0..4, 5..9, 10..14], "té 🍵");
        assert_eq!(d.text(), "té 🍵 té 🍵 té 🍵");
        d.undo();
        assert_eq!(d.text(), "café café café");
    }

    #[test]
    fn syntax_edits_use_bytes_and_points() {
        let mut d = doc("é\nab");
        d.take_syntax_edits();
        d.move_to(3, false); // after "é\na"
        d.insert_text("🚀");
        let edits = d.take_syntax_edits();
        assert_eq!(edits.len(), 1);
        let e = edits[0];
        assert_eq!(e.start_byte, 4); // é is 2 bytes + \n
        assert_eq!(e.new_end_byte, 8); // 🚀 is 4 bytes
        assert_eq!(e.start_position, Point::new(1, 1));
        assert_eq!(e.new_end_position, Point::new(1, 5));
    }

    #[test]
    fn dirty_tracks_saved_state_through_undo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "x").unwrap();
        let mut d = Document::open(&path).unwrap();
        d.move_to(1, false);
        d.insert_text("y");
        d.save().unwrap();
        assert!(!d.is_dirty());
        d.undo();
        assert!(d.is_dirty(), "undo past the save point is a change");
        d.redo();
        assert!(!d.is_dirty());
    }

    #[test]
    fn line_end_skips_crlf_in_mixed_docs() {
        let mut d = doc("");
        d.set_format(TextFormat {
            bom: false,
            line_ending: super::super::text_format::LineEnding::Mixed,
        });
        d.insert_text("ab\r\ncd");
        assert_eq!(d.line_end(0), 2);
        assert_eq!(d.line_end(1), 6);
    }
}
