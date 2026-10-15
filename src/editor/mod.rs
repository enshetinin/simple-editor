//! Editing model, independent of the UI.

pub mod document;
pub mod save;
pub mod text_format;

pub use document::{Document, DocumentId, OpenError, SaveError};
