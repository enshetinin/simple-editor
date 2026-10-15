use eframe::egui::{self, Id, Margin, RichText, ScrollArea, TextEdit, TextStyle};

use super::strings as s;
use super::theme::{palette, primary_button};
use super::view::status_color;
use super::{HttpState, ResponsePane, ResponseView, SimpleEditorApp, Tab, TabKey};
use crate::http::{BodyKind, Method};

/// Lines longer than this are clipped in the response view to keep layout cheap.
pub(super) const MAX_LINE_CHARS: usize = 2_000;

impl SimpleEditorApp {
    pub(super) fn http_tab(&mut self, ui: &mut egui::Ui, key: TabKey) {
        let p = palette(ui.ctx());
        let mut send = false;
        let mut cancel = false;
        let Some(Tab::Http(tab)) = self.tabs.iter_mut().find(|t| t.key() == key) else {
            return;
        };
        let sending = matches!(tab.state, HttpState::Sending { .. });

        egui::Frame::NONE
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                // Method + URL + Send/Cancel.
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("method", key))
                        .selected_text(RichText::new(tab.draft.method.as_str()).strong())
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            for m in Method::ALL {
                                ui.selectable_value(&mut tab.draft.method, m, m.as_str());
                            }
                        });
                    let button_width = 88.0;
                    let url_width = (ui.available_width() - button_width - 8.0).max(120.0);
                    let url = ui.add(
                        TextEdit::singleline(&mut tab.draft.url)
                            .id(Id::new(("url", key)))
                            .hint_text(s::URL_HINT)
                            .font(TextStyle::Monospace)
                            .desired_width(url_width),
                    );
                    if url.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        send = true;
                    }
                    if sending {
                        if ui
                            .add_sized([button_width, 24.0], egui::Button::new(s::CANCEL_REQUEST))
                            .clicked()
                        {
                            cancel = true;
                        }
                    } else if ui
                        .add_sized([button_width, 24.0], primary_button(ui.ctx(), s::SEND))
                        .on_hover_text(s::SEND_SHORTCUT_HINT)
                        .clicked()
                    {
                        send = true;
                    }
                });
                if let HttpState::Invalid(e) = &tab.state {
                    ui.label(RichText::new(format!("⚠ {e}")).color(p.error));
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s::BODY).strong());
                    egui::ComboBox::from_id_salt(("body_kind", key))
                        .selected_text(tab.draft.body_kind.label())
                        .show_ui(ui, |ui| {
                            for k in [BodyKind::Json, BodyKind::Text] {
                                ui.selectable_value(&mut tab.draft.body_kind, k, k.label());
                            }
                        });
                    ui.label(
                        RichText::new(format!(
                            "Content-Type: {}",
                            tab.draft.body_kind.content_type()
                        ))
                        .small()
                        .color(p.text_secondary),
                    );
                });
                ScrollArea::vertical()
                    .id_salt(("body_scroll", key))
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            TextEdit::multiline(&mut tab.draft.body)
                                .id(Id::new(("body", key)))
                                .code_editor()
                                .hint_text(s::BODY_HINT)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    });
            });
        ui.separator();

        egui::Frame::NONE
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.label(RichText::new(s::RESPONSE).strong());
                match &mut tab.state {
                    HttpState::Idle | HttpState::Invalid(_) => {
                        ui.label(RichText::new(s::NOT_SENT).color(p.text_secondary));
                    }
                    HttpState::Sending { started, .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("{:.1} s", started.elapsed().as_secs_f32()));
                            ui.label(RichText::new(s::SENDING).color(p.text_secondary));
                        });
                    }
                    HttpState::Cancelled => {
                        ui.label(RichText::new(s::CANCELLED).color(p.text_secondary));
                    }
                    HttpState::Failed(e) => {
                        ui.label(
                            RichText::new(format!("✖ {}", e.title()))
                                .strong()
                                .color(p.error),
                        );
                        ui.label(e.hint());
                        if !e.detail().is_empty() {
                            ui.label(RichText::new(e.detail()).small().color(p.text_secondary));
                        }
                    }
                    HttpState::Done(view) => response_view(ui, view),
                }
            });

        if send {
            self.send_request(key);
        }
        if cancel {
            self.cancel_request(key);
        }
    }
}

fn response_view(ui: &mut egui::Ui, view: &mut ResponseView) {
    let p = palette(ui.ctx());
    let r = &view.response;
    ui.horizontal_wrapped(|ui| {
        let status = format!("{} {}", r.status, r.reason.unwrap_or(""));
        ui.label(
            RichText::new(status.trim())
                .strong()
                .color(status_color(p, r.status)),
        );
        ui.separator();
        ui.label(format!("{} ms", r.elapsed.as_millis()));
        ui.separator();
        let size = human_bytes(r.bytes_received);
        if r.truncated {
            ui.label(
                RichText::new(format!(
                    "Truncated: showing first {} (read {size})",
                    human_bytes(r.body.len() as u64)
                ))
                .color(p.warning),
            );
        } else {
            ui.label(size);
        }
        ui.separator();
        ui.label(RichText::new(&r.version).color(p.text_secondary));
    });
    ui.label(
        RichText::new(format!("URL: {}", r.final_url))
            .small()
            .color(p.text_secondary),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut view.pane, ResponsePane::Body, s::RESPONSE_BODY);
        ui.selectable_value(
            &mut view.pane,
            ResponsePane::Headers,
            format!("{} ({})", s::RESPONSE_HEADERS, r.headers.len()),
        );
        if view.pane == ResponsePane::Body
            && let Some(text) = &r.body_text
            && !text.is_empty()
            && ui.small_button(s::COPY_BODY).clicked()
        {
            ui.ctx().copy_text(text.clone());
        }
    });
    ui.separator();

    match view.pane {
        ResponsePane::Headers => {
            ScrollArea::both()
                .id_salt("resp_headers")
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui::Grid::new("resp_headers_grid")
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for (k, v) in &r.headers {
                                ui.label(RichText::new(k).monospace().strong());
                                ui.label(RichText::new(v).monospace());
                                ui.end_row();
                            }
                        });
                });
        }
        ResponsePane::Body => match &r.body_text {
            _ if r.body.is_empty() => {
                ui.label(RichText::new(s::EMPTY_BODY).color(p.text_secondary));
            }
            None => {
                ui.label(RichText::new(s::BINARY_BODY).color(p.text_secondary));
            }
            Some(text) => {
                if view.has_long_lines {
                    ui.label(
                        RichText::new(s::LONG_LINES_CLIPPED)
                            .small()
                            .color(p.warning),
                    );
                }
                let row_height = ui.text_style_height(&TextStyle::Monospace);
                ScrollArea::both()
                    .id_salt("resp_body")
                    .auto_shrink(false)
                    .show_rows(ui, row_height, view.lines.len(), |ui, rows| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        // Only visible lines are laid out.
                        for range in view.lines[rows].iter() {
                            let line = &text[range.clone()];
                            let shown = match line.char_indices().nth(MAX_LINE_CHARS) {
                                Some((cut, _)) => &line[..cut],
                                None => line,
                            };
                            ui.label(RichText::new(shown).monospace());
                        }
                    });
            }
        },
    }
}

pub fn human_bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    let f = n as f64;
    if f < KIB {
        format!("{n} B")
    } else if f < KIB * KIB {
        format!("{:.1} KiB", f / KIB)
    } else {
        format!("{:.1} MiB", f / (KIB * KIB))
    }
}
