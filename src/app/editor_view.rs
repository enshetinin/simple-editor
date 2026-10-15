//! Custom code editor widget over a rope `Document`.
//!
//! Only the visible lines are laid out each frame (egui caches identical
//! galleys between frames). Lines longer than `LONG_LINE_CHARS` are drawn
//! with a fixed-width approximation, showing only the horizontally visible
//! slice. Cursor movement and deletion use grapheme clusters.

use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CursorIcon, Event, EventFilter, FontId, Galley, Id, ImeEvent, Key, Modifiers,
    Pos2, Rect, Sense, Stroke, Vec2, text::CCursor, text::LayoutJob, text::TextFormat,
};

use super::theme::{Palette, syntax_color};
use crate::editor::document::LONG_LINE_CHARS;
use crate::editor::{Document, Selection, movement};
use crate::syntax::{HighlightSpans, Hl};

pub const TAB_WIDTH: usize = 4;
const FONT_SIZE: f32 = 13.5;
const GUTTER_PAD: f32 = 16.0;
const TEXT_PAD: f32 = 8.0;

/// Per-tab view state that is not part of the document.
#[derive(Debug, Default)]
pub struct EditorViewState {
    /// Scroll so the cursor is visible on the next frame.
    pub scroll_to_cursor: bool,
    /// IME composition in progress (shown inline, not yet in the document).
    pub preedit: Option<String>,
    /// Byte range of the lines drawn in the last frame (for highlighting).
    pub visible_bytes: Range<usize>,
    pub visible_lines: Range<usize>,
    /// Widest galley seen, to grow the horizontal scroll extent.
    max_width_seen: f32,
    /// Whether the cursor was inside the visible area in the last frame.
    pub cursor_on_screen: bool,
    drag_anchor: Option<Selection>,
}

pub struct EditorInput<'a> {
    pub id: Id,
    pub palette: &'static Palette,
    pub dark: bool,
    pub highlight: Option<&'a HighlightSpans>,
    /// Char ranges of search matches (sorted) and the current one.
    pub matches: &'a [Range<usize>],
    pub current_match: Option<usize>,
    /// Name announced by screen readers (e.g. the file name).
    pub name: &'a str,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EditorOutput {
    pub changed: bool,
    pub selection_changed: bool,
    pub focused: bool,
}

/// One line prepared for drawing.
struct LineView {
    galley: Arc<Galley>,
    /// Char index (in the document) of the first char of the line.
    start_char: usize,
    /// Chars in the line, excluding the line break.
    len_chars: usize,
    /// For long lines: first char drawn and fixed column width.
    long: Option<(usize, f32)>,
    /// Display column of each char when the line contains tabs or preedit.
    display_index: Option<Vec<usize>>,
    /// Display column where the IME preedit text starts, if shown here.
    preedit_at: Option<usize>,
}

impl LineView {
    fn display_of(&self, col: usize) -> usize {
        match &self.display_index {
            Some(map) => map
                .get(col)
                .copied()
                .unwrap_or_else(|| *map.last().unwrap_or(&0)),
            None => col,
        }
    }

    /// X offset (relative to the text origin) of a column within the line.
    fn x_of(&self, col: usize) -> f32 {
        let col = col.min(self.len_chars);
        match self.long {
            Some((_, w)) => col as f32 * w,
            None => {
                self.galley
                    .pos_from_cursor(CCursor::new(self.display_of(col)))
                    .min
                    .x
            }
        }
    }

    /// Column nearest to an x offset (relative to the text origin).
    fn col_at(&self, x: f32) -> usize {
        match self.long {
            Some((_, w)) => ((x / w).round().max(0.0) as usize).min(self.len_chars),
            None => {
                let display = self.galley.cursor_from_pos(Vec2::new(x, 0.0)).index.0;
                match &self.display_index {
                    Some(map) => map.partition_point(|&d| d < display).min(self.len_chars),
                    None => display.min(self.len_chars),
                }
            }
        }
    }
}

struct Metrics {
    font: FontId,
    row_h: f32,
    char_w: f32,
    gutter_w: f32,
}

fn metrics(ctx: &egui::Context, line_count: usize) -> Metrics {
    let font = FontId::monospace(FONT_SIZE);
    let (row_h, char_w) = ctx.fonts_mut(|f| (f.row_height(&font), f.glyph_width(&font, 'M')));
    let digits = line_count.max(1).to_string().len().max(3);
    Metrics {
        font,
        row_h: row_h.ceil(),
        char_w,
        gutter_w: digits as f32 * char_w + GUTTER_PAD * 2.0,
    }
}

/// Draws the editor and applies input. Returns what happened.
pub fn show(
    ui: &mut egui::Ui,
    doc: &mut Document,
    state: &mut EditorViewState,
    input: EditorInput<'_>,
) -> EditorOutput {
    let ctx = ui.ctx().clone();
    let m = metrics(&ctx, doc.rope().len_lines());
    let id = input.id;
    let mut out = EditorOutput::default();

    let focused = ctx.memory(|mem| mem.has_focus(id));
    if focused {
        ctx.memory_mut(|mem| {
            mem.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        let before = (doc.revision(), doc.selection());
        let page_lines = state.visible_lines.len().max(2) - 1;
        handle_keyboard(&ctx, doc, state, page_lines);
        out.changed = doc.revision() != before.0;
        out.selection_changed = doc.selection() != before.1;
        if out.changed || out.selection_changed {
            state.scroll_to_cursor = true;
        }
    }
    out.focused = focused;
    if !focused && ctx.memory(|mem| mem.focused().is_none()) {
        // Undo/redo still apply to the active document after clicking a
        // button (e.g. Replace all) that left nothing focused.
        let (undo, redo) = ctx.input_mut(|i| {
            let redo = i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::Z,
            ));
            let undo = i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));
            (undo, redo)
        });
        if undo {
            out.changed |= doc.undo();
        }
        if redo {
            out.changed |= doc.redo();
        }
    }

    let line_count = doc.rope().len_lines();
    // Long lines are drawn at a fixed column width, so their extent is exact
    // enough; f32 stays precise to the pixel up to ~2 million columns.
    let text_width = (doc.longest_line() as f32 + 4.0) * m.char_w;
    let content = Vec2::new(
        m.gutter_w + TEXT_PAD + text_width.max(state.max_width_seen) + TEXT_PAD,
        line_count as f32 * m.row_h + ui.available_height() * 0.5,
    );

    egui::ScrollArea::both()
        .id_salt(id.with("scroll"))
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            let size = content.max(ui.available_size());
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let response = ui.interact(rect, id, Sense::click_and_drag());
            let origin = rect.min;
            let text_x = origin.x + m.gutter_w + TEXT_PAD;

            let first = ((viewport.min.y / m.row_h).floor().max(0.0) as usize).min(line_count - 1);
            let last = ((viewport.max.y / m.row_h).ceil() as usize).min(line_count);
            state.visible_lines = first..last.max(first + 1);
            let rope = doc.rope();
            state.visible_bytes = rope.line_to_byte(first)..rope.line_to_byte(last.max(first + 1));

            // Build visible lines.
            let col_first = ((viewport.min.x - m.gutter_w) / m.char_w).floor().max(0.0) as usize;
            let col_count = (viewport.width() / m.char_w).ceil() as usize + 2;
            let cursor_line = doc.rope().char_to_line(doc.selection().head);
            let mut lines = Vec::with_capacity(state.visible_lines.len());
            for line in state.visible_lines.clone() {
                let preedit = if focused && line == cursor_line {
                    state.preedit.as_deref().filter(|p| !p.is_empty())
                } else {
                    None
                };
                let lv = build_line(ui, doc, line, &m, &input, (col_first, col_count), preedit);
                state.max_width_seen = state.max_width_seen.max(lv.galley.size().x);
                lines.push(lv);
            }

            // Mouse.
            if let Some(pointer) = response.interact_pointer_pos() {
                let pos = position_at(
                    doc,
                    &lines,
                    &state.visible_lines,
                    &m,
                    origin,
                    text_x,
                    pointer,
                );
                handle_mouse(ui, &response, doc, state, pos);
                out.selection_changed = true;
            }
            // Only a real pointer press focuses the editor. (`clicked()` is
            // also true for Enter/Space on a focused widget, and re-requesting
            // focus would reset the Tab/arrow focus-lock filter.)
            let pressed_here =
                ui.input(|i| i.pointer.primary_pressed()) && response.contains_pointer();
            if pressed_here && !focused {
                response.request_focus();
            }
            if response.hovered() {
                ctx.set_cursor_icon(CursorIcon::Text);
            }

            // Paint.
            let painter = ui.painter_at(ui.clip_rect());
            let p = input.palette;
            let sel = doc.selection();
            let gutter_rect = Rect::from_min_size(
                Pos2::new(origin.x + viewport.min.x, origin.y + viewport.min.y),
                Vec2::new(m.gutter_w, viewport.height()),
            );
            for (i, lv) in lines.iter().enumerate() {
                let line = state.visible_lines.start + i;
                let y = origin.y + line as f32 * m.row_h;
                let row = Rect::from_min_size(Pos2::new(origin.x, y), Vec2::new(size.x, m.row_h));
                if line == cursor_line && sel.is_empty() {
                    painter.rect_filled(row, 0.0, p.background);
                }
                // Search matches and selection.
                let line_range = lv.start_char..lv.start_char + lv.len_chars;
                for (mi, mr) in matches_in(input.matches, &line_range) {
                    let color = if Some(mi) == input.current_match {
                        p.warning.gamma_multiply(0.55)
                    } else {
                        p.warning.gamma_multiply(0.25)
                    };
                    paint_range(
                        &painter,
                        lv,
                        mr,
                        &line_range,
                        text_x,
                        y,
                        m.row_h,
                        color,
                        0.0,
                    );
                }
                if !sel.is_empty() {
                    let past_end = sel.end() > line_range.end && sel.start() <= line_range.end;
                    let extra = if past_end { m.char_w * 0.6 } else { 0.0 };
                    let sel_color = p
                        .action
                        .gamma_multiply(if input.dark { 0.45 } else { 0.25 });
                    paint_range(
                        &painter,
                        lv,
                        sel.range(),
                        &line_range,
                        text_x,
                        y,
                        m.row_h,
                        sel_color,
                        extra,
                    );
                }
                let text_origin = match lv.long {
                    Some((first_col, w)) => Pos2::new(text_x + first_col as f32 * w, y),
                    None => Pos2::new(text_x, y),
                };
                painter.galley(text_origin, lv.galley.clone(), p.text);
                // IME preedit underline.
                if let (Some(at), Some(pre)) = (lv.preedit_at, state.preedit.as_ref()) {
                    let x0 = text_x + lv.galley.pos_from_cursor(CCursor::new(at)).min.x;
                    let end = at + pre.chars().count();
                    let x1 = text_x + lv.galley.pos_from_cursor(CCursor::new(end)).min.x;
                    painter.line_segment(
                        [
                            Pos2::new(x0, y + m.row_h - 2.0),
                            Pos2::new(x1, y + m.row_h - 2.0),
                        ],
                        Stroke::new(1.0, p.text),
                    );
                }
            }

            // Cursor.
            let mut cursor_rect = None;
            if let Some(i) = cursor_line.checked_sub(state.visible_lines.start)
                && let Some(lv) = lines.get(i)
            {
                let col = sel.head - lv.start_char;
                let mut x = text_x + lv.x_of(col);
                if let (Some(at), Some(pre)) = (lv.preedit_at, state.preedit.as_ref()) {
                    let d = at + pre.chars().count();
                    x = text_x + lv.galley.pos_from_cursor(CCursor::new(d)).min.x;
                }
                let y = origin.y + cursor_line as f32 * m.row_h;
                let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(2.0, m.row_h));
                if focused {
                    painter.rect_filled(r, 0.0, p.action);
                }
                state.cursor_on_screen = ui.clip_rect().intersects(r);
                cursor_rect = Some(r);
            }
            if state.scroll_to_cursor {
                state.scroll_to_cursor = false;
                let r = cursor_rect.unwrap_or_else(|| {
                    let y = origin.y + cursor_line as f32 * m.row_h;
                    let col = doc.selection().head - doc.rope().line_to_char(cursor_line);
                    let x = text_x + col as f32 * m.char_w;
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(2.0, m.row_h))
                });
                // Following the caret must be immediate, not animated.
                ui.scroll_to_rect_animation(
                    r.expand2(Vec2::new(m.char_w * 4.0, m.row_h)),
                    None,
                    egui::style::ScrollAnimation::none(),
                );
            }
            if focused && let Some(r) = cursor_rect {
                let to_global = ctx
                    .layer_transform_to_global(ui.layer_id())
                    .unwrap_or_default();
                ctx.output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        purpose: egui::IMEPurpose::Normal,
                        rect: to_global * viewport.translate(origin.to_vec2()),
                        cursor_rect: to_global * r,
                        should_interrupt_composition: false,
                    });
                });
            }

            // Gutter (drawn last so horizontally scrolled text goes under it).
            painter.rect_filled(gutter_rect, 0.0, p.surface);
            painter.line_segment(
                [gutter_rect.right_top(), gutter_rect.right_bottom()],
                Stroke::new(1.0, p.border),
            );
            for (i, _) in lines.iter().enumerate() {
                let line = state.visible_lines.start + i;
                let y = origin.y + line as f32 * m.row_h;
                let color = if line == cursor_line {
                    p.text
                } else {
                    p.text_secondary
                };
                painter.text(
                    Pos2::new(gutter_rect.right() - GUTTER_PAD, y),
                    egui::Align2::RIGHT_TOP,
                    (line + 1).to_string(),
                    m.font.clone(),
                    color,
                );
            }

            // Screen readers get the current line as the value and a label
            // with the position. This closure only runs when accessibility
            // is active, so it costs nothing otherwise.
            let doc_ref: &Document = doc;
            let name = input.name;
            response.widget_info(|| accessibility_info(doc_ref, name));
        });
    out
}

/// Accessibility description: position in the label, current line as text.
fn accessibility_info(doc: &Document, name: &str) -> egui::WidgetInfo {
    const MAX_CHARS: usize = 10_000;
    let sel = doc.selection();
    let rope = doc.rope();
    let line = rope.char_to_line(sel.head);
    let start = rope.line_to_char(line);
    let end = doc.line_end(line);
    let text: String = rope.slice(start..end).chars().take(MAX_CHARS).collect();
    let col = sel.head - start;
    let mut info = egui::WidgetInfo::labeled(
        egui::WidgetType::TextEdit,
        true,
        format!(
            "Code editor: {name}, line {} of {}, column {}",
            line + 1,
            rope.len_lines(),
            col + 1
        ),
    );
    let len = text.chars().count();
    let clamp = |c: usize| c.saturating_sub(start).min(len);
    info.text_selection =
        Some(egui::text::CharIndex(clamp(sel.start()))..egui::text::CharIndex(clamp(sel.end())));
    info.current_text_value = Some(text);
    info
}

#[expect(clippy::too_many_arguments)]
fn paint_range(
    painter: &egui::Painter,
    lv: &LineView,
    range: Range<usize>,
    line_range: &Range<usize>,
    text_x: f32,
    y: f32,
    row_h: f32,
    color: Color32,
    extra: f32,
) {
    let s = range.start.max(line_range.start);
    let e = range.end.min(line_range.end);
    if s > e || (s == e && extra == 0.0) {
        return;
    }
    let x0 = text_x + lv.x_of(s - line_range.start);
    let x1 = text_x + lv.x_of(e - line_range.start) + extra;
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + row_h)),
        0.0,
        color,
    );
}

fn matches_in<'a>(
    matches: &'a [Range<usize>],
    line: &Range<usize>,
) -> impl Iterator<Item = (usize, Range<usize>)> + 'a {
    let first = matches.partition_point(|m| m.end < line.start);
    let line = line.clone();
    matches[first..]
        .iter()
        .enumerate()
        .take_while(move |(_, m)| m.start <= line.end)
        .map(move |(i, m)| (first + i, m.clone()))
}

fn build_line(
    ui: &egui::Ui,
    doc: &Document,
    line: usize,
    m: &Metrics,
    input: &EditorInput<'_>,
    (col_first, col_count): (usize, usize),
    preedit: Option<&str>,
) -> LineView {
    let rope = doc.rope();
    let start_char = rope.line_to_char(line);
    let len_chars = doc.line_end(line) - start_char;
    let p = input.palette;
    let plain = TextFormat::simple(m.font.clone(), p.text);

    if len_chars > LONG_LINE_CHARS {
        let first = col_first.min(len_chars);
        let end = (first + col_count).min(len_chars);
        let text: String = rope
            .slice(start_char + first..start_char + end)
            .chars()
            .map(|c| if c == '\t' { ' ' } else { c })
            .collect();
        let mut job = LayoutJob::single_section(text, plain);
        job.wrap.max_width = f32::INFINITY;
        return LineView {
            galley: ui.painter().layout_job(job),
            start_char,
            len_chars,
            long: Some((first, m.char_w)),
            display_index: None,
            preedit_at: None,
        };
    }

    let line_start_byte = rope.line_to_byte(line);
    let spans: &[crate::syntax::Span] = input.highlight.map_or(&[], |h| &h.spans);
    let mut span_i = spans.partition_point(|s| s.end <= line_start_byte);
    let cursor_col = preedit.map(|_| doc.selection().head - start_char);

    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut run = String::new();
    let mut run_hl = Hl::None;
    // `map[col]` = display column of char `col`; only built when it differs
    // from the identity (tabs or preedit present).
    let mut map: Vec<usize> = Vec::new();
    let mut identity = true;
    let mut display = 0usize;
    let mut preedit_at = None;
    let mut byte = line_start_byte;
    let flush = |job: &mut LayoutJob, run: &mut String, hl: Hl| {
        if !run.is_empty() {
            let color = syntax_color(p, hl, input.dark);
            job.append(run, 0.0, TextFormat::simple(m.font.clone(), color));
            run.clear();
        }
    };
    let mut insert_preedit =
        |job: &mut LayoutJob, run: &mut String, run_hl: Hl, display: &mut usize| {
            if let Some(pre) = preedit {
                flush(job, run, run_hl);
                job.append(pre, 0.0, plain.clone());
                preedit_at = Some(*display);
                *display += pre.chars().count();
            }
        };
    let chars = rope.slice(start_char..start_char + len_chars).chars();
    for (col, c) in chars.enumerate() {
        if cursor_col == Some(col) {
            insert_preedit(&mut job, &mut run, run_hl, &mut display);
            identity = false;
        }
        while span_i < spans.len() && spans[span_i].end <= byte {
            span_i += 1;
        }
        let hl = spans
            .get(span_i)
            .filter(|s| s.start <= byte)
            .map_or(Hl::None, |s| s.hl);
        if hl != run_hl {
            flush(&mut job, &mut run, run_hl);
            run_hl = hl;
        }
        map.push(display);
        if c == '\t' {
            let n = TAB_WIDTH - display % TAB_WIDTH;
            run.extend(std::iter::repeat_n(' ', n));
            display += n;
            identity = false;
        } else {
            run.push(c);
            display += 1;
        }
        byte += c.len_utf8();
    }
    flush(&mut job, &mut run, run_hl);
    map.push(display);
    if cursor_col == Some(len_chars) {
        insert_preedit(&mut job, &mut run, run_hl, &mut display);
    }
    LineView {
        galley: ui.painter().layout_job(job),
        start_char,
        len_chars,
        long: None,
        display_index: (!identity).then_some(map),
        preedit_at,
    }
}

fn position_at(
    doc: &Document,
    lines: &[LineView],
    visible: &Range<usize>,
    m: &Metrics,
    origin: Pos2,
    text_x: f32,
    pointer: Pos2,
) -> usize {
    let line_count = doc.rope().len_lines();
    let line = (((pointer.y - origin.y) / m.row_h).floor().max(0.0) as usize).min(line_count - 1);
    let x = pointer.x - text_x;
    let col = match line.checked_sub(visible.start).and_then(|i| lines.get(i)) {
        Some(lv) => lv.col_at(x),
        None => {
            let len = doc.line_end(line) - doc.rope().line_to_char(line);
            ((x / m.char_w).round().max(0.0) as usize).min(len)
        }
    };
    movement::snap_to_grapheme(doc.rope(), doc.rope().line_to_char(line) + col)
}

fn handle_mouse(
    ui: &egui::Ui,
    response: &egui::Response,
    doc: &mut Document,
    state: &mut EditorViewState,
    pos: usize,
) {
    let (shift, pressed) = ui.input(|i| (i.modifiers.shift, i.pointer.primary_pressed()));
    if response.triple_clicked() {
        let line = doc.rope().char_to_line(pos);
        let start = doc.rope().line_to_char(line);
        let end = if line + 1 >= doc.rope().len_lines() {
            doc.len_chars()
        } else {
            doc.rope().line_to_char(line + 1)
        };
        doc.set_selection(Selection::new(start, end));
        state.drag_anchor = Some(doc.selection());
    } else if response.double_clicked() {
        let (s, e) = movement::word_at(doc.rope(), pos);
        doc.set_selection(Selection::new(s, e));
        state.drag_anchor = Some(doc.selection());
    } else if pressed && response.contains_pointer() {
        doc.move_to(pos, shift);
        state.drag_anchor = None;
    } else if response.dragged() {
        match state.drag_anchor {
            // Dragging after a double/triple click extends by whole units.
            Some(unit) => {
                let (anchor, head) = if pos < unit.start() {
                    (unit.end(), pos)
                } else {
                    (unit.start(), pos.max(unit.end()))
                };
                doc.set_selection(Selection::new(anchor, head));
            }
            None => doc.move_to(pos, true),
        }
        state.scroll_to_cursor = true;
    }
    doc.preferred_column = None;
}

fn is_mac(ctx: &egui::Context) -> bool {
    ctx.os() == egui::os::OperatingSystem::Mac
}

fn handle_keyboard(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorViewState,
    page_lines: usize,
) {
    let mac = is_mac(ctx);
    let events = ctx.input(|i| i.events.clone());
    for event in events {
        let composing = state.preedit.as_ref().is_some_and(|p| !p.is_empty());
        match event {
            Event::Text(text) => {
                if !composing && !text.is_empty() && text != "\n" && text != "\r" {
                    doc.insert_text(&text);
                }
            }
            Event::Paste(text) => {
                if !text.is_empty() {
                    doc.insert_text_as(&text, crate::editor::history::EditKind::Other);
                }
            }
            Event::Copy => {
                if !doc.selection().is_empty() {
                    ctx.copy_text(doc.selected_text());
                }
            }
            Event::Cut => {
                if !doc.selection().is_empty() {
                    ctx.copy_text(doc.selected_text());
                    let sel = doc.selection();
                    doc.delete_range(
                        sel.start(),
                        sel.end(),
                        crate::editor::history::EditKind::Other,
                    );
                }
            }
            Event::Ime(ime) => match ime {
                ImeEvent::Preedit { text, .. } => {
                    state.preedit = (!text.is_empty()).then_some(text);
                }
                ImeEvent::Commit(text) => {
                    state.preedit = None;
                    if !text.is_empty() && text != "\n" && text != "\r" {
                        doc.insert_text_as(
                            &text,
                            if text.chars().count() == 1 {
                                crate::editor::history::EditKind::Typing
                            } else {
                                crate::editor::history::EditKind::Other
                            },
                        );
                    }
                }
                ImeEvent::DeleteSurrounding {
                    before_chars,
                    after_chars,
                } => {
                    let head = doc.selection().head;
                    let s = head.saturating_sub(before_chars);
                    let e = (head + after_chars).min(doc.len_chars());
                    doc.delete_range(s, e, crate::editor::history::EditKind::Other);
                }
                #[expect(deprecated)]
                ImeEvent::Enabled | ImeEvent::Disabled => {}
            },
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if !composing => handle_key(doc, key, modifiers, mac, page_lines),
            _ => {}
        }
    }
}

fn handle_key(doc: &mut Document, key: Key, mods: Modifiers, mac: bool, page_lines: usize) {
    let extend = mods.shift;
    // Word jumps: Alt on macOS, Ctrl elsewhere. Line/document jumps: Cmd on macOS.
    let word = if mac { mods.alt } else { mods.ctrl };
    let line_jump = mac && mods.mac_cmd;
    let sel = doc.selection();
    let rope_len = doc.len_chars();
    match key {
        Key::ArrowLeft => {
            let target = if !extend && !sel.is_empty() && !word && !line_jump {
                sel.start()
            } else if line_jump {
                smart_home(doc, sel.head)
            } else if word {
                movement::prev_word(doc.rope(), sel.head)
            } else {
                movement::prev_grapheme(doc.rope(), sel.head)
            };
            doc.move_to(target, extend);
        }
        Key::ArrowRight => {
            let target = if !extend && !sel.is_empty() && !word && !line_jump {
                sel.end()
            } else if line_jump {
                doc.line_end(doc.rope().char_to_line(sel.head))
            } else if word {
                movement::next_word(doc.rope(), sel.head)
            } else {
                movement::next_grapheme(doc.rope(), sel.head)
            };
            doc.move_to(target, extend);
        }
        Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown => {
            let up = matches!(key, Key::ArrowUp | Key::PageUp);
            if line_jump && matches!(key, Key::ArrowUp | Key::ArrowDown) {
                doc.move_to(if up { 0 } else { rope_len }, extend);
                return;
            }
            let lines = if matches!(key, Key::PageUp | Key::PageDown) {
                page_lines
            } else {
                1
            };
            vertical(doc, up, lines, extend);
        }
        Key::Home => {
            let target = if mods.command {
                0
            } else {
                smart_home(doc, sel.head)
            };
            doc.move_to(target, extend);
        }
        Key::End => {
            let target = if mods.command {
                rope_len
            } else {
                doc.line_end(doc.rope().char_to_line(sel.head))
            };
            doc.move_to(target, extend);
        }
        Key::Backspace => {
            if line_jump && sel.is_empty() {
                let start = doc.rope().line_to_char(doc.rope().char_to_line(sel.head));
                doc.delete_range(start, sel.head, crate::editor::history::EditKind::Other);
            } else if word {
                doc.delete_word_backward();
            } else {
                doc.backspace();
            }
        }
        Key::Delete => doc.delete_forward(),
        Key::Enter if !mods.command => doc.newline(),
        Key::Tab if !mods.command => {
            if mods.shift {
                doc.outdent();
            } else {
                doc.indent_or_insert_tab();
            }
        }
        Key::A if mods.command => doc.select_all(),
        Key::Z if mods.command && mods.shift => {
            doc.redo();
        }
        Key::Z if mods.command => {
            doc.undo();
        }
        Key::Y if mods.command && !mac => {
            doc.redo();
        }
        _ => {}
    }
}

/// Home: first non-whitespace char, or the line start if already there.
fn smart_home(doc: &Document, pos: usize) -> usize {
    let line = doc.rope().char_to_line(pos);
    let start = doc.rope().line_to_char(line);
    let indent = doc
        .rope()
        .line(line)
        .chars()
        .take_while(|&c| c == ' ' || c == '\t')
        .count();
    if pos == start + indent {
        start
    } else {
        start + indent
    }
}

/// Up/down keeping the preferred display column.
fn vertical(doc: &mut Document, up: bool, lines: usize, extend: bool) {
    let head = doc.selection().head;
    let rope = doc.rope();
    let line = rope.char_to_line(head);
    let col_now = display_col(doc, line, head - rope.line_to_char(line));
    let want = doc.preferred_column.unwrap_or(col_now as f32) as usize;
    let last_line = rope.len_lines() - 1;
    let target_line = if up {
        line.saturating_sub(lines)
    } else {
        (line + lines).min(last_line)
    };
    let target = if up && line == 0 {
        0
    } else if !up && line == last_line {
        doc.len_chars()
    } else {
        let start = rope.line_to_char(target_line);
        let len = doc.line_end(target_line) - start;
        let col = char_col_for_display(doc, target_line, want).min(len);
        movement::snap_to_grapheme(doc.rope(), start + col)
    };
    doc.move_to(target, extend);
    doc.preferred_column = Some(want as f32);
}

fn display_col(doc: &Document, line: usize, col: usize) -> usize {
    let start = doc.rope().line_to_char(line);
    // Long lines are drawn at a fixed width (tab = 1 column): no scan needed.
    if doc.line_end(line) - start > LONG_LINE_CHARS {
        return col;
    }
    let mut display = 0;
    for c in doc.rope().slice(start..start + col).chars() {
        display += if c == '\t' {
            TAB_WIDTH - display % TAB_WIDTH
        } else {
            1
        };
    }
    display
}

fn char_col_for_display(doc: &Document, line: usize, want: usize) -> usize {
    let start = doc.rope().line_to_char(line);
    let len = doc.line_end(line) - start;
    if len > LONG_LINE_CHARS {
        return want.min(len);
    }
    let mut display = 0;
    for (i, c) in doc.rope().slice(start..start + len).chars().enumerate() {
        let w = if c == '\t' {
            TAB_WIDTH - display % TAB_WIDTH
        } else {
            1
        };
        if display + w > want {
            return i;
        }
        display += w;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_movement_keeps_preferred_column_over_short_lines() {
        let mut d = Document::from_text("abcdef\nab\nabcdef");
        d.move_to(5, false);
        vertical(&mut d, false, 1, false);
        assert_eq!(d.selection().head, 7 + 2); // clamped to end of "ab"
        vertical(&mut d, false, 1, false);
        assert_eq!(d.selection().head, 10 + 5); // back to column 5
        vertical(&mut d, true, 5, false);
        assert_eq!(d.selection().head, 5);
    }

    #[test]
    fn vertical_movement_respects_tabs_and_graphemes() {
        let mut d = Document::from_text("\tx\n12345\n👨‍👩‍👧z");
        d.move_to(1, false); // after the tab = display column 4
        vertical(&mut d, false, 1, false);
        assert_eq!(d.selection().head, 3 + 4);
        d.move_to(3 + 1, false);
        vertical(&mut d, false, 1, false);
        // Lands on a grapheme boundary, never inside the family emoji.
        let head = d.selection().head;
        assert!(head == 9 || head == 14, "{head}");
    }

    #[test]
    fn keyboard_commands() {
        let mut d = Document::from_text("    let x = 1;");
        d.move_to(14, false);
        handle_key(&mut d, Key::Home, Modifiers::NONE, false, 10);
        assert_eq!(d.selection().head, 4);
        handle_key(&mut d, Key::Home, Modifiers::NONE, false, 10);
        assert_eq!(d.selection().head, 0);
        handle_key(&mut d, Key::End, Modifiers::SHIFT, false, 10);
        assert_eq!(d.selected_text(), "    let x = 1;");
        handle_key(&mut d, Key::ArrowLeft, Modifiers::NONE, false, 10);
        assert_eq!(d.selection(), Selection::caret(0));
        handle_key(&mut d, Key::ArrowRight, Modifiers::CTRL, false, 10);
        assert_eq!(d.selection().head, 7);
        handle_key(&mut d, Key::Backspace, Modifiers::CTRL, false, 10);
        assert_eq!(d.text(), "     x = 1;");
        handle_key(
            &mut d,
            Key::Z,
            Modifiers::CTRL.plus(Modifiers::COMMAND),
            false,
            10,
        );
        assert_eq!(d.text(), "    let x = 1;");
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use eframe::egui::RawInput;

    fn run(ctx: &egui::Context, doc: &mut Document, state: &mut EditorViewState) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
            ..RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(
                    ui,
                    doc,
                    state,
                    EditorInput {
                        id: Id::new("t"),
                        palette: &crate::app::theme::LIGHT,
                        dark: false,
                        highlight: None,
                        matches: &[],
                        current_match: None,
                        name: "test",
                    },
                );
            });
        });
        out.textures_delta.clear();
    }

    #[test]
    fn cursor_is_scrolled_into_view_on_huge_lines_and_documents() {
        let long = "x".repeat(300_000);
        let many = "line\n".repeat(200_000);
        for (text, target) in [(long.as_str(), 250_000), (many.as_str(), 150_000 * 5 + 2)] {
            let ctx = egui::Context::default();
            ctx.memory_mut(|m| m.request_focus(Id::new("t")));
            let mut doc = Document::from_text(text);
            let mut state = EditorViewState::default();
            run(&ctx, &mut doc, &mut state);
            doc.set_selection(Selection::caret(target));
            state.scroll_to_cursor = true;
            for _ in 0..3 {
                run(&ctx, &mut doc, &mut state);
            }
            assert!(state.cursor_on_screen, "cursor at {target} not visible");
        }
    }
}
