//! Syntax highlighting with Tree-sitter.
//!
//! A worker thread owns one parser and tree per document. The UI sends edits
//! (`InputEdit`) plus an O(1) rope snapshot; the worker re-parses
//! incrementally and answers highlight requests for the visible byte range
//! only. Results carry the document revision so stale ones can be adjusted or
//! dropped.

mod spans;
mod worker;

use std::path::Path;

pub use spans::{HighlightSpans, Span, shift_spans};
pub use worker::{HighlightResult, SyntaxService};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Json,
    Html,
    Css,
}

impl Lang {
    pub const ALL: [Lang; 7] = [
        Lang::Rust,
        Lang::JavaScript,
        Lang::TypeScript,
        Lang::Tsx,
        Lang::Json,
        Lang::Html,
        Lang::Css,
    ];

    pub fn from_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => Lang::Rust,
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "json" => Lang::Json,
            "html" | "htm" => Lang::Html,
            "css" => Lang::Css,
            _ => return None,
        })
    }

    pub fn language(self) -> tree_sitter::Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Json => tree_sitter_json::LANGUAGE.into(),
            Lang::Html => tree_sitter_html::LANGUAGE.into(),
            Lang::Css => tree_sitter_css::LANGUAGE.into(),
        }
    }

    /// Highlight query source. TypeScript extends the JavaScript query (its
    /// own patterns first, so they take precedence).
    pub fn highlight_query(self) -> String {
        match self {
            Lang::Rust => tree_sitter_rust::HIGHLIGHTS_QUERY.to_owned(),
            Lang::JavaScript => format!(
                "{}\n{}",
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY
            ),
            Lang::TypeScript => format!(
                "{}\n{}",
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY
            ),
            Lang::Tsx => format!(
                "{}\n{}\n{}",
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY
            ),
            Lang::Json => tree_sitter_json::HIGHLIGHTS_QUERY.to_owned(),
            Lang::Html => tree_sitter_html::HIGHLIGHTS_QUERY.to_owned(),
            Lang::Css => tree_sitter_css::HIGHLIGHTS_QUERY.to_owned(),
        }
    }
}

/// Highlight categories the theme knows how to color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Hl {
    #[default]
    None,
    Keyword,
    String,
    Escape,
    Comment,
    Number,
    Constant,
    Function,
    Type,
    Property,
    VariableBuiltin,
    Operator,
    Punctuation,
    Tag,
    Attribute,
    Label,
}

/// Maps a Tree-sitter capture name (e.g. `function.method`) to a category.
pub fn classify(capture: &str) -> Hl {
    let root = capture.split('.').next().unwrap_or(capture);
    match (root, capture) {
        (_, "string.escape" | "escape") => Hl::Escape,
        (_, "variable.builtin") => Hl::VariableBuiltin,
        (_, "punctuation.special") => Hl::Operator,
        ("keyword" | "conditional" | "repeat" | "include" | "storageclass", _) => Hl::Keyword,
        ("string" | "character", _) => Hl::String,
        ("comment", _) => Hl::Comment,
        ("number" | "float", _) => Hl::Number,
        ("constant" | "boolean", _) => Hl::Constant,
        ("function" | "method" | "constructor", _) => Hl::Function,
        ("type", _) => Hl::Type,
        ("property" | "field", _) => Hl::Property,
        ("operator", _) => Hl::Operator,
        ("punctuation", _) => Hl::Punctuation,
        ("tag", _) => Hl::Tag,
        ("attribute", _) => Hl::Attribute,
        ("label", _) => Hl::Label,
        _ => Hl::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_grammars_load_and_queries_compile() {
        for lang in Lang::ALL {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&lang.language())
                .unwrap_or_else(|e| panic!("{lang:?}: {e}"));
            tree_sitter::Query::new(&lang.language(), &lang.highlight_query())
                .unwrap_or_else(|e| panic!("{lang:?} query: {e}"));
        }
    }

    #[test]
    fn classify_capture_names() {
        assert_eq!(classify("keyword"), Hl::Keyword);
        assert_eq!(classify("function.method"), Hl::Function);
        assert_eq!(classify("string.escape"), Hl::Escape);
        assert_eq!(classify("variable"), Hl::None);
        assert_eq!(classify("variable.builtin"), Hl::VariableBuiltin);
    }
}
