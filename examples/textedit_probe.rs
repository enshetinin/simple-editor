//! H0 feasibility probe for the editor view and buffer.
//!
//! Run: `cargo run --release --example textedit_probe`
//!
//! 1. Drives the standard egui `TextEdit` headless (layout + tessellation, no
//!    GPU) with deterministic corpora and real typed-text events, reporting
//!    per-frame CPU cost. This is a proxy, NOT key-to-photon latency.
//! 2. Compares a `String` buffer with a `ropey::Rope` for a single insertion
//!    in the middle of the document.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Event, Id, RawInput, TextEdit, text::CCursor, text_selection::CCursorRange,
};

const FRAMES: usize = 30;

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
    let mut s = String::with_capacity(target + 128);
    let mut i = 0;
    while s.len() < target {
        s.push_str(lines[i % lines.len()]);
        s.push('\n');
        i += 1;
    }
    s
}

fn corpus_unicode(target: usize) -> String {
    let lines = [
        "Árbol, café, niño — ¿qué tal? ¡Olé!",
        "Emoji: 👩‍💻 🚀 👋🏽 🇪🇸 ❤️",
        "Combinados: e\u{301} a\u{308} n\u{303} — Ελληνικά — Русский — 中文 — 日本語 — العربية",
        "let s = \"ascii plain text line for mixing\";",
    ];
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

fn run_frame(ctx: &egui::Context, text: &mut String, events: Vec<Event>) -> Duration {
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 800.0),
        )),
        events,
        ..RawInput::default()
    };
    let start = Instant::now();
    let mut out = ctx.run_ui(input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                TextEdit::multiline(text)
                    .id(Id::new("probe"))
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .show(ui);
            });
        });
    });
    let _prims = ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
    let elapsed = start.elapsed();
    out.textures_delta.clear();
    elapsed
}

fn probe_textedit(name: &str, mut text: String) {
    let ctx = egui::Context::default();
    let bytes = text.len();
    let first = run_frame(&ctx, &mut text, vec![]);

    // Focus and place the cursor in the middle of the document.
    ctx.memory_mut(|m| m.request_focus(Id::new("probe")));
    let mid_char = text.chars().count() / 2;
    let mut state = TextEdit::load_state(&ctx, Id::new("probe")).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(mid_char))));
    TextEdit::store_state(&ctx, Id::new("probe"), state);
    run_frame(&ctx, &mut text, vec![]);

    let idle: Vec<_> = (0..FRAMES)
        .map(|_| run_frame(&ctx, &mut text, vec![]))
        .collect();
    let typing: Vec<_> = (0..FRAMES)
        .map(|_| run_frame(&ctx, &mut text, vec![Event::Text("a".into())]))
        .collect();
    let inserted = text.len() - bytes;
    let (idle, typing) = (stats(idle), stats(typing));
    println!(
        "| {name} | {:.2} MiB | {} | {} / {} | {} / {} / {} | {inserted} |",
        bytes as f64 / (1024.0 * 1024.0),
        ms(first),
        ms(idle.p50),
        ms(idle.p95),
        ms(typing.p50),
        ms(typing.p95),
        ms(typing.max),
    );
}

fn probe_buffers(size: usize) {
    const OPS: usize = 2_000;
    let base = corpus_code(size);

    let mut s = base.clone();
    let mut v = Vec::with_capacity(OPS);
    for i in 0..OPS {
        let mut at = s.len() / 2 + i;
        while !s.is_char_boundary(at) {
            at += 1;
        }
        let t = Instant::now();
        s.insert(at, 'a');
        v.push(t.elapsed());
    }
    let string = stats(v);

    let mut rope = ropey::Rope::from_str(&base);
    let mut v = Vec::with_capacity(OPS);
    for i in 0..OPS {
        let at = rope.len_chars() / 2 + i;
        let t = Instant::now();
        rope.insert_char(at, 'a');
        v.push(t.elapsed());
    }
    let rope = stats(v);

    println!(
        "| {:.0} MiB | {} µs / {} µs | {} µs / {} µs |",
        size as f64 / (1024.0 * 1024.0),
        string.p50.as_micros(),
        string.p95.as_micros(),
        rope.p50.as_nanos() as f64 / 1000.0,
        rope.p95.as_nanos() as f64 / 1000.0,
    );
}

fn main() {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * 1024;
    println!("egui TextEdit headless frame cost (layout + tessellation, ms), {FRAMES} frames each");
    println!(
        "| corpus | size | first frame | idle p50 / p95 | typing p50 / p95 / max | bytes typed |"
    );
    println!("|---|---|---|---|---|---|");
    probe_textedit("code 1 KiB", corpus_code(KIB));
    probe_textedit("code 100 KiB", corpus_code(100 * KIB));
    probe_textedit("code 1 MiB", corpus_code(MIB));
    probe_textedit("unicode mix 1 MiB", corpus_unicode(MIB));
    probe_textedit("single line 1 MiB", single_line(MIB));
    probe_textedit("code 4 MiB", corpus_code(4 * MIB));
    probe_textedit("code 8 MiB", corpus_code(8 * MIB));
    if std::env::args().any(|a| a == "--10mib") {
        probe_textedit("code 10 MiB", corpus_code(10 * MIB));
    }
    println!();
    println!("Buffer: insert one char in the middle, 2000 ops (p50 / p95)");
    println!("| size | String | ropey::Rope |");
    println!("|---|---|---|");
    probe_buffers(MIB);
    probe_buffers(10 * MIB);
    probe_buffers(100 * MIB);
}
