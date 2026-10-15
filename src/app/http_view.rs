use eframe::egui::{self, Id, Margin, RichText, ScrollArea, TextEdit, TextStyle};

use super::env_view::variables_table;
use super::strings as s;
use super::theme::{palette, primary_button};
use super::view::status_color;
use super::{HttpState, RequestPane, ResponsePane, ResponseView, SimpleEditorApp, Tab, TabKey};
use crate::http::model::{Auth, BodyMode, KeyValue};
use crate::http::{JsonView, Method, TextLines};

/// Lines longer than this are clipped in the response view to keep layout cheap.
pub(super) const MAX_LINE_CHARS: usize = 2_000;

enum Action {
    Send,
    SendToFile,
    Cancel,
    Save,
    CopyCurl,
    SetEnv(Option<String>),
    EditEnvironments,
}

impl SimpleEditorApp {
    pub(super) fn http_tab_view(&mut self, ui: &mut egui::Ui, key: TabKey) {
        let p = palette(ui.ctx());
        let mut actions = Vec::new();
        let envs: Vec<(String, String)> = self
            .project
            .as_ref()
            .map(|p| {
                p.data
                    .environments
                    .iter()
                    .map(|e| (e.id.clone(), e.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let active_env = self.project.as_ref().and_then(|p| p.active_env.clone());
        let has_project = self.project.as_ref().is_some_and(|p| p.loaded);
        let backend = self.secrets.name().to_owned();
        let mut download_limit = self.download_limit_mib;
        let collection_names: Vec<(String, String)> = self
            .project
            .as_ref()
            .map(|p| {
                p.data
                    .collections
                    .iter()
                    .map(|c| (c.id.clone(), c.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let Some(tab) = self.tabs.iter_mut().find_map(|t| match t {
            Tab::Http(h) if h.key == key => Some(h.as_mut()),
            _ => None,
        }) else {
            return;
        };
        let sending = matches!(tab.state, HttpState::Sending { .. });
        let source_label = tab.source.as_ref().map(|s| {
            collection_names
                .iter()
                .find(|(id, _)| *id == s.collection)
                .map_or(s.collection.clone(), |(_, n)| n.clone())
        });

        egui::Frame::NONE
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                // Name, collection, environment, save/copy.
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut tab.def.name)
                            .id(Id::new(("req_name", key)))
                            .desired_width(220.0),
                    );
                    let where_label = match &source_label {
                        Some(c) => format!("in {c}"),
                        None => s::NOT_SAVED.to_owned(),
                    };
                    ui.label(RichText::new(where_label).small().color(p.text_secondary));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(has_project, egui::Button::new(s::SAVE))
                            .on_hover_text(s::SAVE_REQUEST_HINT)
                            .clicked()
                        {
                            actions.push(Action::Save);
                        }
                        if ui
                            .button(s::COPY_CURL)
                            .on_hover_text(s::COPY_CURL_HINT)
                            .clicked()
                        {
                            actions.push(Action::CopyCurl);
                        }
                        if has_project
                            && ui
                                .small_button("✎")
                                .on_hover_text(s::EDIT_ENVIRONMENTS)
                                .clicked()
                        {
                            actions.push(Action::EditEnvironments);
                        }
                        let current = active_env
                            .as_ref()
                            .and_then(|id| envs.iter().find(|(e, _)| e == id))
                            .map_or(s::NO_ENVIRONMENT.to_owned(), |(_, n)| n.clone());
                        egui::ComboBox::from_id_salt(("env", key))
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(active_env.is_none(), s::NO_ENVIRONMENT)
                                    .clicked()
                                {
                                    actions.push(Action::SetEnv(None));
                                }
                                for (id, name) in &envs {
                                    if ui
                                        .selectable_label(active_env.as_ref() == Some(id), name)
                                        .clicked()
                                    {
                                        actions.push(Action::SetEnv(Some(id.clone())));
                                    }
                                }
                            })
                            .response
                            .on_hover_text(s::ENVIRONMENT);
                    });
                });
                ui.add_space(4.0);

                // Method + URL + Send/Cancel.
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("method", key))
                        .selected_text(RichText::new(tab.def.method.as_str()).strong())
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            for m in Method::ALL {
                                ui.selectable_value(&mut tab.def.method, m, m.as_str());
                            }
                        });
                    let button_width = 96.0;
                    let url_width = (ui.available_width() - button_width - 40.0).max(120.0);
                    let url = ui.add(
                        TextEdit::singleline(&mut tab.def.url)
                            .id(Id::new(("url", key)))
                            .hint_text(s::URL_HINT)
                            .font(TextStyle::Monospace)
                            .desired_width(url_width),
                    );
                    if url.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        actions.push(Action::Send);
                    }
                    if sending {
                        if ui
                            .add_sized([button_width, 24.0], egui::Button::new(s::CANCEL_REQUEST))
                            .clicked()
                        {
                            actions.push(Action::Cancel);
                        }
                    } else {
                        if ui
                            .add_sized([button_width, 24.0], primary_button(ui.ctx(), s::SEND))
                            .on_hover_text(s::SEND_SHORTCUT_HINT)
                            .clicked()
                        {
                            actions.push(Action::Send);
                        }
                        ui.menu_button("▾", |ui| {
                            if ui.button(s::SEND_TO_FILE).clicked() {
                                actions.push(Action::SendToFile);
                                ui.close();
                            }
                        });
                    }
                });

                // Request sections.
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let count = |v: &[KeyValue]| {
                        v.iter().filter(|x| x.enabled && !x.key.is_empty()).count()
                    };
                    let panes = [
                        (
                            RequestPane::Params,
                            format!("{} ({})", s::PARAMS, count(&tab.def.params)),
                        ),
                        (
                            RequestPane::Headers,
                            format!("{} ({})", s::HEADERS, count(&tab.def.headers)),
                        ),
                        (
                            RequestPane::Body,
                            format!("{} · {}", s::BODY, tab.def.body.mode.label()),
                        ),
                        (
                            RequestPane::Auth,
                            format!("{} · {}", s::AUTH, tab.def.auth.label()),
                        ),
                        (
                            RequestPane::Variables,
                            format!("{} ({})", s::VARIABLES, tab.def.variables.len()),
                        ),
                        (RequestPane::Settings, s::SETTINGS.to_owned()),
                    ];
                    for (pane, label) in panes {
                        ui.selectable_value(&mut tab.pane, pane, label);
                    }
                });
                ui.separator();
                ScrollArea::vertical()
                    .id_salt(("req_section", key))
                    .max_height(200.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| match tab.pane {
                        RequestPane::Params => {
                            kv_table(ui, &mut tab.def.params, key, "params", s::ADD_PARAM)
                        }
                        RequestPane::Headers => {
                            kv_table(ui, &mut tab.def.headers, key, "headers", s::ADD_HEADER)
                        }
                        RequestPane::Body => {
                            ui.horizontal(|ui| {
                                for m in BodyMode::ALL {
                                    ui.selectable_value(&mut tab.def.body.mode, m, m.label());
                                }
                                if let Some(ct) = tab.def.body.mode.content_type() {
                                    ui.label(
                                        RichText::new(format!("Content-Type: {ct}"))
                                            .small()
                                            .color(p.text_secondary),
                                    );
                                }
                                if tab.def.body.mode == BodyMode::Json
                                    && ui.small_button(s::FORMAT_JSON).clicked()
                                    && crate::http::json::validate(&tab.def.body.text).is_ok()
                                {
                                    tab.def.body.text =
                                        crate::http::json::pretty(&tab.def.body.text);
                                }
                            });
                            if tab.def.body.mode != BodyMode::None {
                                ui.add(
                                    TextEdit::multiline(&mut tab.def.body.text)
                                        .id(Id::new(("body", key)))
                                        .code_editor()
                                        .hint_text(s::BODY_HINT)
                                        .desired_rows(6)
                                        .desired_width(f32::INFINITY),
                                );
                            }
                        }
                        RequestPane::Auth => auth_editor(ui, &mut tab.def.auth, key),
                        RequestPane::Variables => {
                            ui.label(
                                RichText::new(s::REQUEST_VARS_HELP)
                                    .small()
                                    .color(p.text_secondary),
                            );
                            variables_table(ui, &mut tab.def.variables, key, 99, &backend);
                        }
                        RequestPane::Settings => {
                            ui.horizontal(|ui| {
                                ui.label(s::TIMEOUT_SECS);
                                ui.add(
                                    egui::DragValue::new(&mut tab.def.timeout_secs).range(1..=3600),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(s::DOWNLOAD_LIMIT_MIB);
                                ui.add(
                                    egui::DragValue::new(&mut download_limit).range(1..=1_048_576),
                                );
                            });
                            ui.label(
                                RichText::new(s::SETTINGS_HELP)
                                    .small()
                                    .color(p.text_secondary),
                            );
                        }
                    });

                if let HttpState::Invalid(errors) = &tab.state {
                    ui.add_space(4.0);
                    for e in errors {
                        ui.label(
                            RichText::new(format!("⚠ {}: {}", e.field, e.message)).color(p.error),
                        );
                    }
                }
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
                    HttpState::Sending {
                        started, download, ..
                    } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("{:.1} s", started.elapsed().as_secs_f32()));
                            let msg = match download {
                                Some(path) => format!("{} {}", s::DOWNLOADING_TO, path.display()),
                                None => s::SENDING.to_owned(),
                            };
                            ui.label(RichText::new(msg).color(p.text_secondary));
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
                        let detail = e.detail();
                        if !detail.is_empty() {
                            ui.label(RichText::new(detail).small().color(p.text_secondary));
                        }
                    }
                    HttpState::Done(view) => response_view(ui, view),
                }
            });

        self.download_limit_mib = download_limit;
        for action in actions {
            match action {
                Action::Send => self.send_request(key),
                Action::SendToFile => {
                    let name = self
                        .http_tab(key)
                        .map(|t| t.def.name.replace(['/', '\\', ':'], "_"))
                        .unwrap_or_default();
                    if let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() {
                        self.send_request_to(key, Some(path));
                    }
                }
                Action::Cancel => self.cancel_request(key),
                Action::Save => {
                    self.save_request(key, super::AfterSave::Nothing);
                }
                Action::CopyCurl => self.copy_as_curl(key),
                Action::SetEnv(id) => self.set_active_environment(id),
                Action::EditEnvironments => self.open_environments(),
            }
        }
    }
}

fn kv_table(ui: &mut egui::Ui, rows: &mut Vec<KeyValue>, key: TabKey, salt: &str, add: &str) {
    let mut remove = None;
    egui::Grid::new((salt, key))
        .num_columns(4)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (i, kv) in rows.iter_mut().enumerate() {
                ui.checkbox(&mut kv.enabled, "")
                    .on_hover_text(s::ENABLED_HINT);
                ui.add(
                    TextEdit::singleline(&mut kv.key)
                        .id(Id::new((salt, "k", key, i)))
                        .hint_text(s::NAME)
                        .font(TextStyle::Monospace)
                        .desired_width(200.0),
                );
                ui.add(
                    TextEdit::singleline(&mut kv.value)
                        .id(Id::new((salt, "v", key, i)))
                        .hint_text(s::VALUE)
                        .font(TextStyle::Monospace)
                        .desired_width(320.0),
                );
                if ui.small_button("×").on_hover_text(s::REMOVE_ROW).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.button(add).clicked() {
        rows.push(KeyValue::new("", ""));
    }
}

fn auth_editor(ui: &mut egui::Ui, auth: &mut Auth, key: TabKey) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        let mut kind = match auth {
            Auth::None => 0,
            Auth::Bearer { .. } => 1,
            Auth::Basic { .. } => 2,
        };
        let before = kind;
        ui.selectable_value(&mut kind, 0, "None");
        ui.selectable_value(&mut kind, 1, "Bearer token");
        ui.selectable_value(&mut kind, 2, "Basic auth");
        if kind != before {
            *auth = match kind {
                1 => Auth::Bearer {
                    token: String::new(),
                },
                2 => Auth::Basic {
                    username: String::new(),
                    password: String::new(),
                },
                _ => Auth::None,
            };
        }
    });
    match auth {
        Auth::None => {}
        Auth::Bearer { token } => {
            ui.horizontal(|ui| {
                ui.label(s::TOKEN);
                let masked = !token.contains("{{");
                ui.add(
                    TextEdit::singleline(token)
                        .id(Id::new(("token", key)))
                        .password(masked)
                        .font(TextStyle::Monospace)
                        .desired_width(360.0),
                );
            });
        }
        Auth::Basic { username, password } => {
            ui.horizontal(|ui| {
                ui.label(s::USERNAME);
                ui.add(
                    TextEdit::singleline(username)
                        .id(Id::new(("user", key)))
                        .desired_width(200.0),
                );
                ui.label(s::PASSWORD);
                let masked = !password.contains("{{");
                ui.add(
                    TextEdit::singleline(password)
                        .id(Id::new(("pass", key)))
                        .password(masked)
                        .desired_width(200.0),
                );
            });
        }
    }
    ui.label(RichText::new(s::AUTH_HELP).small().color(p.text_secondary));
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
    if let Some(path) = &r.saved_to {
        ui.label(
            RichText::new(format!(
                "{} {} ({})",
                s::SAVED_TO,
                path.display(),
                human_bytes(r.bytes_received)
            ))
            .color(p.success),
        );
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut view.pane, ResponsePane::Body, s::RESPONSE_BODY);
        ui.selectable_value(
            &mut view.pane,
            ResponsePane::Headers,
            format!("{} ({})", s::RESPONSE_HEADERS, r.headers.len()),
        );
        if view.pane == ResponsePane::Body {
            if matches!(r.json, JsonView::Pretty(_)) {
                ui.separator();
                ui.selectable_value(&mut view.pretty, true, s::PRETTY);
                ui.selectable_value(&mut view.pretty, false, s::RAW);
            }
            if let Some(text) = &r.body_text
                && !text.is_empty()
                && ui.small_button(s::COPY_BODY).clicked()
            {
                ui.ctx().copy_text(text.clone());
            }
        }
    });
    match &r.json {
        JsonView::Invalid(e) => {
            ui.label(
                RichText::new(format!("{} {e}", s::INVALID_JSON))
                    .small()
                    .color(p.warning),
            );
        }
        JsonView::Truncated => {
            ui.label(RichText::new(s::JSON_TRUNCATED).small().color(p.warning));
        }
        _ => {}
    }
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
        ResponsePane::Body => {
            if r.saved_to.is_some() {
                return;
            }
            if r.body.is_empty() {
                ui.label(RichText::new(s::EMPTY_BODY).color(p.text_secondary));
                return;
            }
            if r.body_text.is_none() {
                ui.label(RichText::new(s::BINARY_BODY).color(p.text_secondary));
                return;
            }
            let lines: &TextLines = match (&r.json, view.pretty) {
                (JsonView::Pretty(pretty), true) => pretty,
                _ => &r.body_lines,
            };
            text_lines(ui, lines);
        }
    }
}

/// Virtualized read-only text: only visible lines are laid out.
fn text_lines(ui: &mut egui::Ui, lines: &TextLines) {
    let p = palette(ui.ctx());
    if lines.longest > MAX_LINE_CHARS {
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
        .show_rows(ui, row_height, lines.lines.len(), |ui, rows| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for range in lines.lines[rows].iter() {
                let line = &lines.text[range.clone()];
                let shown = match line.char_indices().nth(MAX_LINE_CHARS) {
                    Some((cut, _)) => &line[..cut],
                    None => line,
                };
                ui.label(RichText::new(shown).monospace());
            }
        });
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
