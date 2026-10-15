use std::collections::HashMap;
use std::ops::Range;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use ropey::Rope;
use tree_sitter::{InputEdit, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use super::{Hl, Lang, Span, classify};
use crate::editor::DocumentId;
use crate::http::Notify;

/// Largest byte range highlighted per request (a screenful is far smaller).
const MAX_RANGE: usize = 512 * 1024;

enum Msg {
    Open {
        doc: DocumentId,
        lang: Lang,
        rope: Rope,
        revision: u64,
    },
    Edit {
        doc: DocumentId,
        revision: u64,
        rope: Rope,
        edits: Vec<InputEdit>,
    },
    Request {
        doc: DocumentId,
        range: Range<usize>,
    },
    Close {
        doc: DocumentId,
    },
}

#[derive(Debug, Clone)]
pub struct HighlightResult {
    pub doc: DocumentId,
    pub revision: u64,
    pub range: Range<usize>,
    pub spans: Vec<Span>,
    /// Time of the last (incremental) parse for this document.
    pub parse_time: Duration,
    pub query_time: Duration,
}

/// Handle to the highlighting worker thread.
pub struct SyntaxService {
    tx: Sender<Msg>,
    rx: Receiver<HighlightResult>,
}

impl SyntaxService {
    pub fn new(notify: Notify) -> Self {
        let (tx, worker_rx) = channel();
        let (worker_tx, rx) = channel();
        std::thread::Builder::new()
            .name("se-syntax".to_owned())
            .spawn(move || run(worker_rx, worker_tx, notify))
            .expect("spawning the syntax thread");
        Self { tx, rx }
    }

    pub fn open(&self, doc: DocumentId, lang: Lang, rope: Rope, revision: u64) {
        let _ = self.tx.send(Msg::Open {
            doc,
            lang,
            rope,
            revision,
        });
    }

    pub fn edit(&self, doc: DocumentId, revision: u64, rope: Rope, edits: Vec<InputEdit>) {
        let _ = self.tx.send(Msg::Edit {
            doc,
            revision,
            rope,
            edits,
        });
    }

    pub fn request(&self, doc: DocumentId, range: Range<usize>) {
        let _ = self.tx.send(Msg::Request { doc, range });
    }

    pub fn close(&self, doc: DocumentId) {
        let _ = self.tx.send(Msg::Close { doc });
    }

    pub fn poll(&self) -> Vec<HighlightResult> {
        self.rx.try_iter().collect()
    }

    /// Blocks for one result (tests and tools).
    pub fn wait(&self, timeout: Duration) -> Option<HighlightResult> {
        self.rx.recv_timeout(timeout).ok()
    }
}

struct DocState {
    lang: Lang,
    parser: Parser,
    tree: Option<Tree>,
    rope: Rope,
    revision: u64,
    needs_parse: bool,
    parse_time: Duration,
}

struct CompiledQuery {
    query: Query,
    classes: Vec<Hl>,
}

fn run(rx: Receiver<Msg>, tx: Sender<HighlightResult>, notify: Notify) {
    let mut docs: HashMap<DocumentId, DocState> = HashMap::new();
    let mut queries: HashMap<Lang, Option<CompiledQuery>> = HashMap::new();
    let mut cursor = QueryCursor::new();
    while let Ok(first) = rx.recv() {
        // Coalesce everything queued so a burst of keystrokes costs one parse.
        let mut requests: HashMap<DocumentId, Range<usize>> = HashMap::new();
        for msg in std::iter::once(first).chain(rx.try_iter()) {
            match msg {
                Msg::Open {
                    doc,
                    lang,
                    rope,
                    revision,
                } => {
                    let mut parser = Parser::new();
                    if parser.set_language(&lang.language()).is_err() {
                        continue;
                    }
                    docs.insert(
                        doc,
                        DocState {
                            lang,
                            parser,
                            tree: None,
                            rope,
                            revision,
                            needs_parse: true,
                            parse_time: Duration::ZERO,
                        },
                    );
                }
                Msg::Edit {
                    doc,
                    revision,
                    rope,
                    edits,
                } => {
                    if let Some(state) = docs.get_mut(&doc) {
                        if let Some(tree) = state.tree.as_mut() {
                            for edit in &edits {
                                tree.edit(edit);
                            }
                        }
                        state.rope = rope;
                        state.revision = revision;
                        state.needs_parse = true;
                    }
                }
                Msg::Request { doc, range } => {
                    requests.insert(doc, range);
                }
                Msg::Close { doc } => {
                    docs.remove(&doc);
                    requests.remove(&doc);
                }
            }
        }
        for state in docs.values_mut().filter(|s| s.needs_parse) {
            parse(state);
        }
        let mut sent = false;
        for (doc, range) in requests {
            let Some(state) = docs.get(&doc) else {
                continue;
            };
            let compiled = queries
                .entry(state.lang)
                .or_insert_with(|| compile(state.lang));
            let Some(compiled) = compiled.as_ref() else {
                continue;
            };
            let started = Instant::now();
            let (range, spans) = highlight(state, compiled, &mut cursor, range);
            let _ = tx.send(HighlightResult {
                doc,
                revision: state.revision,
                range,
                spans,
                parse_time: state.parse_time,
                query_time: started.elapsed(),
            });
            sent = true;
        }
        if sent {
            notify();
        }
    }
}

fn compile(lang: Lang) -> Option<CompiledQuery> {
    let query = Query::new(&lang.language(), &lang.highlight_query()).ok()?;
    let classes = query.capture_names().iter().map(|n| classify(n)).collect();
    Some(CompiledQuery { query, classes })
}

fn parse(state: &mut DocState) {
    let started = Instant::now();
    let rope = &state.rope;
    let len = rope.len_bytes();
    let mut read = |byte: usize, _: tree_sitter::Point| -> &[u8] {
        if byte >= len {
            return &[];
        }
        let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte);
        &chunk.as_bytes()[byte - chunk_start..]
    };
    state.tree = state
        .parser
        .parse_with_options(&mut read, state.tree.as_ref(), None);
    state.needs_parse = false;
    state.parse_time = started.elapsed();
}

fn highlight(
    state: &DocState,
    compiled: &CompiledQuery,
    cursor: &mut QueryCursor,
    range: Range<usize>,
) -> (Range<usize>, Vec<Span>) {
    let len = state.rope.len_bytes();
    let start = range.start.min(len);
    let end = range.end.min(len).min(start + MAX_RANGE);
    let range = start..end;
    let Some(tree) = state.tree.as_ref() else {
        return (range, Vec::new());
    };
    if range.is_empty() {
        return (range, Vec::new());
    }
    let rope = &state.rope;
    cursor.set_byte_range(range.clone());
    let text = |node: Node| {
        rope.byte_slice(node.byte_range())
            .chunks()
            .map(str::as_bytes)
    };
    let mut raw: Vec<(usize, usize, usize, Hl)> = Vec::new();
    let mut captures = cursor.captures(&compiled.query, tree.root_node(), text);
    while let Some((m, index)) = captures.next() {
        let capture = m.captures()[*index];
        let hl = compiled.classes[capture.index as usize];
        if hl == Hl::None {
            continue;
        }
        let r = capture.node.byte_range();
        let (s, e) = (r.start.max(range.start), r.end.min(range.end));
        if s < e {
            raw.push((s, e, m.pattern_index, hl));
        }
    }
    // Larger nodes first, smaller ones override; for the same node the
    // earlier pattern wins (painted last), as in tree-sitter-highlight.
    raw.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(b.2.cmp(&a.2)));
    let mut paint = vec![Hl::None; range.len()];
    for (s, e, _, hl) in raw {
        paint[s - range.start..e - range.start].fill(hl);
    }
    let mut spans = Vec::new();
    let mut i = 0;
    while i < paint.len() {
        let hl = paint[i];
        let mut j = i + 1;
        while j < paint.len() && paint[j] == hl {
            j += 1;
        }
        if hl != Hl::None {
            spans.push(Span {
                start: range.start + i,
                end: range.start + j,
                hl,
            });
        }
        i = j;
    }
    (range, spans)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::editor::Document;

    fn hl_at(result: &HighlightResult, text: &str, needle: &str) -> Hl {
        let pos = text.find(needle).unwrap();
        result
            .spans
            .iter()
            .find(|s| s.start <= pos && pos < s.end)
            .map_or(Hl::None, |s| s.hl)
    }

    #[test]
    fn highlights_and_reparses_incrementally() {
        let svc = SyntaxService::new(Arc::new(|| {}));
        let mut doc = Document::from_text("fn main() {\n    let s = \"hola\"; // c\n}\n");
        svc.open(doc.id(), Lang::Rust, doc.rope().clone(), doc.revision());
        svc.request(doc.id(), 0..doc.rope().len_bytes());
        let r = svc.wait(Duration::from_secs(5)).expect("result");
        let text = doc.text();
        assert_eq!(hl_at(&r, &text, "fn"), Hl::Keyword);
        assert_eq!(hl_at(&r, &text, "\"hola\""), Hl::String);
        assert_eq!(hl_at(&r, &text, "// c"), Hl::Comment);

        // Edit: turn the string into a number; only the edit is sent.
        let start = text.find("\"hola\"").unwrap();
        let start_char = doc.rope().byte_to_char(start);
        doc.set_selection(crate::editor::Selection::new(start_char, start_char + 6));
        doc.insert_text("42");
        let edits = doc.take_syntax_edits();
        svc.edit(doc.id(), doc.revision(), doc.rope().clone(), edits);
        svc.request(doc.id(), 0..doc.rope().len_bytes());
        let r = svc.wait(Duration::from_secs(5)).expect("result");
        assert_eq!(r.revision, doc.revision());
        // tree-sitter-rust tags integer literals as `constant.builtin`.
        assert!(matches!(
            hl_at(&r, &doc.text(), "42"),
            Hl::Number | Hl::Constant
        ));
    }

    #[test]
    fn every_language_produces_highlights() {
        let samples = [
            (Lang::Rust, "fn f() -> u8 { 1 }", "fn"),
            (Lang::JavaScript, "const x = 'a'; // c", "const"),
            (Lang::TypeScript, "let x: number = 1;", "let"),
            (Lang::Tsx, "const a = <div>hi</div>;", "const"),
            (Lang::Json, "{\"k\": true}", "true"),
            (Lang::Html, "<p class=\"a\">x</p>", "p"),
            (Lang::Css, "a { color: red; }", "color"),
        ];
        let svc = SyntaxService::new(Arc::new(|| {}));
        for (lang, text, needle) in samples {
            let doc = Document::from_text(text);
            svc.open(doc.id(), lang, doc.rope().clone(), 0);
            svc.request(doc.id(), 0..text.len());
            let r = svc.wait(Duration::from_secs(5)).expect("result");
            assert_ne!(hl_at(&r, text, needle), Hl::None, "{lang:?}: {:?}", r.spans);
        }
    }
}
