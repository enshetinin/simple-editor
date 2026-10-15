//! Editing model, independent of the UI.

pub mod document;
pub mod history;
pub mod indent;
pub mod movement;
pub mod save;
pub mod search;
pub mod selection;
pub mod text_format;

pub use document::{Document, DocumentId, LoadedText, OpenError, SaveError, SaveJob};
pub use selection::Selection;
