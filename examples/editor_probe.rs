//! H1 performance probe for the editor engine and view.
//!
//! Run: `cargo run --release --example editor_probe [-- --100mib]`
//!
//! Measures, with a deterministic corpus:
//! 1. Engine operations on `Document` (rope + undo history + syntax edits).
//! 2. Tree-sitter: initial parse, incremental re-parse after a keystroke and
//!    the highlight query for a screenful.
//! 3. The SimpleEditor editor widget driven headless (layout + tessellation
//!    on the CPU, no GPU): idle, typing and page-down frames. This is a proxy,
//!    NOT key-to-photon latency.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Id, RawInput};
use simpleeditor::app::editor_view::{self, EditorInput, EditorViewState};
use simpleeditor::app::theme;
use simpleeditor::editor::{Document, Selection};
use simpleeditor::syntax::{HighlightSpans, Lang, SyntaxService};

const MIB: usize = 1024 * 1024;

fn corpus_code(target: usize) -> String {
    let lines = [
        "fn handle(req: &Request) -> Result<Response, Error> {",
        "    let user = db.find_user(req.id).await?; // lookup",
        "    if user.active && user.roles.contains(&Role::Admin) {",
        "        return Ok(Response::json(&user));",
        "    }",
        "    Err(Error::Forbidden(format!(\"user {} denied\", user.id)))",
        "}",
        "",
    ];
    fill(target, &lines)
}

fn corpus_unicode(target: usize) -> String {
    let lines = [
        "Árbol, café, niño — ¿qué tal? ¡Olé!",
        "Emoji: 👩‍💻 🚀 👋🏽 🇪🇸 ❤️",
        "Combinados: e\u{301} a\u{308} n\u{303} — Ελληνικά — Русский — 中文 — 日本語 — العربية",
        "let s = \"ascii plain text line for mixing\";\r",
    ];
    fill(target, &lines)
}

fn fill(target: usize, lines: &[&str]) -> String {
    let mut s = String::with_capacity(target + 256);
    let mut i = 0;
    while s.len() < target {
        s.push_str(lines[i % lines.len()]);
        s.push('\n');
        i += 1;
    }
    s
}

fn single_line(target: usize) -> String {
    let mut s = String::with_capacity(target);
    while s.len() < target {
        s.push_str("{\"key\":\"value\",\"n\":12345},");
    }
    s
}

struct Stats {
    p50: Duration,
    p95: Duration,
    max: Duration,
}

fn stats(mut v: Vec<Duration>) -> Stats {
    v.sort();
    let pick = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
    Stats {
        p50: pick(0.5),
        p95: pick(0.95),
        max: *v.last().unwrap(),
    }
}

fn ms(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1000.0)
}

fn us(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() * 1e6)
}

// ---- 1. engine ------------------------------------------------------------

fn probe_engine(name: &str, text: &str) {
    const OPS: usize = 2_000;
    let t = Instant::now();
    let mut doc = Document::from_text(text);
    let load = t.elapsed();
    let mid = doc.len_chars() / 2;
    doc.move_to(mid, false);
    let mut insert = Vec::with_capacity(OPS);
    for i in 0..OPS {
        let t = Instant::now();
        doc.insert_text(if i % 2 == 0 { "a" } else { "é" });
        insert.push(t.elapsed());
    }
    let mut backspace = Vec::with_capacity(OPS);
    for _ in 0..OPS {
        let t = Instant::now();
        doc.backspace();
        backspace.push(t.elapsed());
    }
    // Paste a multi-line block, then undo/redo it.
    let block = "line one\nline two 👋\nline three\n".repeat(50);
    let mut paste = Vec::new();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for _ in 0..200 {
        let t = Instant::now();
        doc.insert_text(&block);
        paste.push(t.elapsed());
        let t = Instant::now();
        doc.undo();
        undo.push(t.elapsed());
        let t = Instant::now();
        doc.redo();
        redo.push(t.elapsed());
        doc.undo();
    }
    doc.take_syntax_edits();
    let (i, b, p, u, r) = (
        stats(insert),
        stats(backspace),
        stats(paste),
        stats(undo),
        stats(redo),
    );
    println!(
        "| {name} | {} | {} / {} | {} / {} | {} / {} | {} / {} | {} / {} |",
        ms(load),
        us(i.p50),
        us(i.p95),
        us(b.p50),
        us(b.p95),
        us(p.p50),
        us(p.p95),
        us(u.p50),
        us(u.p95),
        us(r.p50),
        us(r.p95),
    );
}

// ---- 2. syntax -------------------------------------------------------------

fn probe_syntax(text: &str) -> HighlightSpans {
    let svc = SyntaxService::new(Arc::new(|| {}));
    let mut doc = Document::from_text(text);
    let screen = 0..doc.rope().line_to_byte(60.min(doc.rope().len_lines()));
    let t = Instant::now();
    svc.open(doc.id(), Lang::Rust, doc.rope().clone(), doc.revision());
    svc.request(doc.id(), screen.clone());
    let first = svc.wait(Duration::from_secs(120)).expect("result");
    let wall_first = t.elapsed();
    let mut parse = Vec::new();
    let mut query = Vec::new();
    let mut roundtrip = Vec::new();
    for i in 0..50 {
        let at = doc.len_chars() / 2 + i;
        doc.move_to(at, false);
        doc.insert_text("x");
        let edits = doc.take_syntax_edits();
        let t = Instant::now();
        svc.edit(doc.id(), doc.revision(), doc.rope().clone(), edits);
        svc.request(doc.id(), screen.clone());
        let r = svc.wait(Duration::from_secs(120)).expect("result");
        roundtrip.push(t.elapsed());
        parse.push(r.parse_time);
        query.push(r.query_time);
    }
    let (p, q, r) = (stats(parse), stats(query), stats(roundtrip));
    println!(
        "| {:.1} MiB Rust | {} (wall {}) | {} / {} | {} / {} | {} / {} / {} |",
        text.len() as f64 / MIB as f64,
        ms(first.parse_time),
        ms(wall_first),
        ms(p.p50),
        ms(p.p95),
        ms(q.p50),
        ms(q.p95),
        ms(r.p50),
        ms(r.p95),
        ms(r.max),
    );
    let mut spans = HighlightSpans::at_revision(0);
    spans.accept(0, first.range, first.spans);
    spans
}

// ---- 3. view ---------------------------------------------------------------

struct Harness {
    ctx: egui::Context,
    doc: Document,
    state: EditorViewState,
}

impl Harness {
    fn new(text: &str) -> Self {
        let ctx = egui::Context::default();
        ctx.memory_mut(|m| m.request_focus(Id::new("probe")));
        Self {
            ctx,
            doc: Document::from_text(text),
            state: EditorViewState::default(),
        }
    }

    fn frame(&mut self, events: Vec<Event>, spans: Option<&HighlightSpans>) -> Duration {
        let input = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            events,
            ..RawInput::default()
        };
        let start = Instant::now();
        let (doc, state) = (&mut self.doc, &mut self.state);
        let mut out = self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                editor_view::show(
                    ui,
                    doc,
                    state,
                    EditorInput {
                        id: Id::new("probe"),
                        palette: &theme::LIGHT,
                        dark: false,
                        highlight: spans,
                        matches: &[],
                        current_match: None,
                    },
                );
            });
        });
        let _prims = self
            .ctx
            .tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        out.textures_delta.clear();
        start.elapsed()
    }
}

fn probe_view(name: &str, text: &str, spans: Option<HighlightSpans>) {
    const FRAMES: usize = 60;
    let mut h = Harness::new(text);
    let first = h.frame(vec![], spans.as_ref());
    h.frame(vec![], spans.as_ref());
    // Cursor in the middle (scrolls there).
    let mid = h.doc.len_chars() / 2;
    h.doc.set_selection(Selection::caret(mid));
    h.state.scroll_to_cursor = true;
    h.frame(vec![], spans.as_ref());
    h.frame(vec![], spans.as_ref());
    let idle: Vec<_> = (0..FRAMES)
        .map(|_| h.frame(vec![], spans.as_ref()))
        .collect();
    let before = h.doc.len_chars();
    let typing: Vec<_> = (0..FRAMES)
        .map(|i| {
            let c = if i % 3 == 0 { "é" } else { "a" };
            h.frame(vec![Event::Text(c.into())], spans.as_ref())
        })
        .collect();
    let typed = h.doc.len_chars() - before;
    let page = Event::Key {
        key: egui::Key::PageDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let scroll: Vec<_> = (0..FRAMES)
        .map(|_| h.frame(vec![page.clone()], spans.as_ref()))
        .collect();
    let (i, t, s) = (stats(idle), stats(typing), stats(scroll));
    println!(
        "| {name} | {} | {} / {} | {} / {} / {} | {} / {} / {} | {typed}/{FRAMES} |",
        ms(first),
        ms(i.p50),
        ms(i.p95),
        ms(t.p50),
        ms(t.p95),
        ms(t.max),
        ms(s.p50),
        ms(s.p95),
        ms(s.max),
    );
}

fn probe_disk(name: &str, text: &str) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("probe.rs");
    std::fs::write(&path, text).expect("write corpus");
    let t = Instant::now();
    let mut doc = Document::open(&path).expect("open");
    let open = t.elapsed();
    doc.move_to(doc.len_chars() / 2, false);
    doc.insert_text("x");
    let t = Instant::now();
    doc.save().expect("save");
    let save = t.elapsed();
    println!("| {name} | {} | {} |", ms(open), ms(save));
}

fn probe_json_response() {
    // ~5 MiB JSON array of objects (the preview limit).
    let mut json = String::from("[");
    let mut i = 0;
    while json.len() < 5 * MIB - 200 {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"id":{i},"name":"user {i} café","tags":["a","b"],"active":true,"score":{}.5}}"#,
            i % 97
        ));
        i += 1;
    }
    json.push(']');
    let t = Instant::now();
    simpleeditor::http::json::validate(&json).expect("valid");
    let validate = t.elapsed();
    let t = Instant::now();
    let pretty = simpleeditor::http::json::pretty(&json);
    let format = t.elapsed();
    let t = Instant::now();
    let lines = simpleeditor::http::TextLines::new(pretty);
    let split = t.elapsed();
    println!(
        "| {:.1} MiB → {:.1} MiB pretty, {} lines | {} | {} | {} |",
        json.len() as f64 / MIB as f64,
        lines.text.len() as f64 / MIB as f64,
        lines.lines.len(),
        ms(validate),
        ms(format),
        ms(split)
    );
}

fn main() {
    let big = std::env::args().any(|a| a == "--100mib");
    println!("## HTTP response processing on the worker (ms)");
    println!("| body | validate | pretty | split lines |");
    println!("|---|---|---|---|");
    probe_json_response();
    println!();
    let mut corpora: Vec<(&str, String)> = vec![
        ("code 1 KiB", corpus_code(1024)),
        ("code 1 MiB", corpus_code(MIB)),
        ("unicode+CR mix 1 MiB", corpus_unicode(MIB)),
        ("single line 1 MiB", single_line(MIB)),
        ("code 10 MiB", corpus_code(10 * MIB)),
    ];
    if big {
        corpora.push(("code 100 MiB", corpus_code(100 * MIB)));
    }

    println!("## Engine (Document: rope + history + syntax edits), µs p50 / p95");
    println!(
        "| corpus | load ms | insert char | backspace | paste 150 lines | undo paste | redo paste |"
    );
    println!("|---|---|---|---|---|---|---|");
    for (name, text) in &corpora {
        probe_engine(name, text);
    }

    println!();
    println!(
        "## Disk (open = read + UTF-8 check + line endings + rope + indent detection; save = atomic write + fsync), ms"
    );
    println!("| corpus | open | save |");
    println!("|---|---|---|");
    for (name, text) in &corpora {
        probe_disk(name, text);
    }

    println!();
    println!("## Tree-sitter worker, ms");
    println!(
        "| corpus | initial parse | incremental parse p50 / p95 | screen query p50 / p95 | edit→result p50 / p95 / max |"
    );
    println!("|---|---|---|---|---|");
    let spans_1mib = probe_syntax(&corpus_code(MIB));
    probe_syntax(&corpus_code(8 * MIB - 4096));

    println!();
    println!("## Editor widget, headless frame ms (layout + tessellation, 60 frames)");
    println!(
        "| corpus | first | idle p50 / p95 | typing p50 / p95 / max | page down p50 / p95 / max | chars typed |"
    );
    println!("|---|---|---|---|---|---|");
    for (name, text) in &corpora {
        probe_view(name, text, None);
    }
    probe_view(
        "code 1 MiB + highlight spans",
        &corpus_code(MIB),
        Some(spans_1mib),
    );
}
