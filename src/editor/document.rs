use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::save::{DiskStamp, atomic_write};
use super::text_format::{DecodeError, TextFormat, decode, encode};

/// Largest file the H0 editor opens for editing. The prototype view is the
/// standard egui `TextEdit`, whose cost grows with document size (see
/// docs/STATUS.md for measurements); larger files are refused explicitly
/// instead of freezing the window.
pub const MAX_EDITABLE_BYTES: u64 = 2 * 1024 * 1024;

/// Lines longer than this make the H0 view noticeably slow; the UI warns.
pub const LONG_LINE_WARNING_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(u64);

impl DocumentId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
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
                "the file is {:.1} MiB; this preview build edits files up to {} MiB",
                *len as f64 / (1024.0 * 1024.0),
                MAX_EDITABLE_BYTES / (1024 * 1024)
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
            SaveError::ModifiedOnDisk => {
                write!(
                    f,
                    "the file was changed by another program since it was opened"
                )
            }
            SaveError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// An open text document. Independent of any UI.
///
/// H0 prototype: the text lives in a `String` because the standard egui
/// `TextEdit` requires contiguous text. See docs/STATUS.md for the buffer
/// decision for H1.
#[derive(Debug)]
pub struct Document {
    id: DocumentId,
    path: Option<PathBuf>,
    pub text: String,
    format: TextFormat,
    revision: u64,
    saved_revision: u64,
    disk_stamp: Option<DiskStamp>,
    longest_line_at_open: usize,
}

impl Document {
    pub fn untitled() -> Self {
        Self {
            id: DocumentId::next(),
            path: None,
            text: String::new(),
            format: TextFormat::default(),
            revision: 0,
            saved_revision: 0,
            disk_stamp: None,
            longest_line_at_open: 0,
        }
    }

    /// A document bound to a path that does not exist yet (created on save).
    pub fn new_at(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Self::untitled()
        }
    }

    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let meta = std::fs::metadata(path).map_err(OpenError::Io)?;
        if !meta.is_file() {
            return Err(OpenError::NotAFile);
        }
        if meta.len() > MAX_EDITABLE_BYTES {
            return Err(OpenError::TooLarge { len: meta.len() });
        }
        let stamp = DiskStamp::of(path).map_err(OpenError::Io)?;
        let bytes = std::fs::read(path).map_err(OpenError::Io)?;
        let (text, format) = decode(&bytes).map_err(OpenError::Decode)?;
        let longest_line_at_open = text.split('\n').map(str::len).max().unwrap_or(0);
        Ok(Self {
            id: DocumentId::next(),
            path: Some(path.to_path_buf()),
            text,
            format,
            revision: 0,
            saved_revision: 0,
            disk_stamp: Some(stamp),
            longest_line_at_open,
        })
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn format(&self) -> TextFormat {
        self.format
    }

    /// Length in bytes of the longest line when the file was opened.
    pub fn longest_line_at_open(&self) -> usize {
        self.longest_line_at_open
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Must be called after every change to `text`.
    pub fn mark_edited(&mut self) {
        self.revision += 1;
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    /// Saves to the current path, refusing to overwrite external changes.
    pub fn save(&mut self) -> Result<(), SaveError> {
        self.save_inner(false)
    }

    /// Saves to the current path even if it changed on disk.
    pub fn save_overwriting(&mut self) -> Result<(), SaveError> {
        self.save_inner(true)
    }

    /// Saves to a new path and binds the document to it.
    pub fn save_as(&mut self, path: PathBuf) -> Result<(), SaveError> {
        self.write_to(&path)?;
        self.path = Some(path);
        Ok(())
    }

    fn save_inner(&mut self, overwrite: bool) -> Result<(), SaveError> {
        let path = self.path.clone().ok_or(SaveError::NoPath)?;
        if !overwrite && let Some(expected) = self.disk_stamp {
            match DiskStamp::of(&path) {
                Ok(current) if current != expected => return Err(SaveError::ModifiedOnDisk),
                // Deleted externally: saving recreates it.
                _ => {}
            }
        }
        self.write_to(&path)
    }

    fn write_to(&mut self, path: &Path) -> Result<(), SaveError> {
        let bytes = encode(&self.text, &self.format);
        atomic_write(path, &bytes).map_err(SaveError::Io)?;
        self.saved_revision = self.revision;
        self.disk_stamp = DiskStamp::of(path).ok();
        Ok(())
    }

    /// Human-readable language name derived from the file extension.
    pub fn language(&self) -> &'static str {
        let ext = self
            .path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => "Rust",
            "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
            "ts" | "tsx" | "mts" | "cts" => "TypeScript",
            "json" => "JSON",
            "html" | "htm" => "HTML",
            "css" => "CSS",
            "md" => "Markdown",
            "toml" => "TOML",
            "yml" | "yaml" => "YAML",
            _ => "Plain text",
        }
    }
}

/// 1-based line and column (in characters) for a character index.
pub fn line_col(text: &str, char_index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for c in text.chars().take(char_index) {
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_chars_not_bytes() {
        let text = "héllo\n👋🏽x";
        assert_eq!(line_col(text, 0), (1, 1));
        assert_eq!(line_col(text, 5), (1, 6));
        assert_eq!(line_col(text, 6), (2, 1));
        // 👋🏽 is two chars (emoji + skin tone modifier).
        assert_eq!(line_col(text, 8), (2, 3));
    }
}
