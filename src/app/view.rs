use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Color32, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText,
    ScrollArea, TextEdit, ThemePreference, WidgetInfo, WidgetType,
};

use super::strings as s;
use super::theme::{self, palette, primary_button};
use super::{AfterSave, Dialog, SimpleEditorApp, Tab, TabKey};
use crate::workspace::DirState;

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const CLOSE_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const NEW_FILE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const SEND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);

enum Command {
    Save,
    SaveAs,
    Open,
    CloseTab,
    NewFile,
    Send,
}

impl SimpleEditorApp {
    /// Draws the whole window. Separate from `eframe::App` so it can run headless in tests.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.dialog.is_none() {
            for command in self.take_shortcuts(&ctx) {
                self.run_command(command);
            }
        }

        let p = palette(&ctx);
        let bar = egui::Frame::NONE
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.border))
            .inner_margin(Margin::symmetric(12, 6));

        egui::Panel::top("top_bar")
            .frame(bar)
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status_bar")
            .frame(bar.inner_margin(Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(240.0)
            .min_size(160.0)
            .frame(
                egui::Frame::NONE
                    .fill(p.background)
                    .inner_margin(Margin::same(8)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.surface))
            .show(ui, |ui| self.central(ui));

        self.dialogs(&ctx);
        if self.take_quit_request() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn take_shortcuts(&self, ctx: &egui::Context) -> Vec<Command> {
        let http_active = matches!(self.active_tab(), Some(Tab::Http(_)));
        ctx.input_mut(|i| {
            let mut out = Vec::new();
            // Shift+Cmd+S first: plain Cmd+S would also match it logically.
            if i.consume_shortcut(&SAVE_AS) {
                out.push(Command::SaveAs);
            }
            if i.consume_shortcut(&SAVE) {
                out.push(Command::Save);
            }
            if i.consume_shortcut(&OPEN) {
                out.push(Command::Open);
            }
            if i.consume_shortcut(&CLOSE_TAB) {
                out.push(Command::CloseTab);
            }
            if i.consume_shortcut(&NEW_FILE) {
                out.push(Command::NewFile);
            }
            if http_active && i.consume_shortcut(&SEND) {
                out.push(Command::Send);
            }
            out
        })
    }

    fn run_command(&mut self, command: Command) {
        match command {
            Command::Save => self.save_active(false),
            Command::SaveAs => self.save_active(true),
            Command::Open => self.open_file_dialog(),
            Command::NewFile => self.new_file(),
            Command::CloseTab => {
                if let Some(key) = self.active {
                    self.request_close_tab(key);
                }
            }
            Command::Send => {
                if let Some(key) = self.active {
                    self.send_request(key);
                }
            }
        }
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let key = self.active?;
        self.tabs.iter().find(|t| t.key() == key)
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(RichText::new(s::APP_NAME).strong().color(p.action));
            ui.separator();
            let project = self
                .workspace
                .project_name()
                .unwrap_or_else(|| s::NO_PROJECT.to_owned());
            ui.label(RichText::new(project).strong().color(p.text));
            ui.add_space(8.0);
            if ui.button(s::OPEN_FOLDER).clicked() {
                self.open_folder_dialog();
            }
            if ui
                .button(s::OPEN_FILE)
                .on_hover_text("Open file (Cmd/Ctrl+O)")
                .clicked()
            {
                self.open_file_dialog();
            }
            if ui
                .button(s::NEW_FILE)
                .on_hover_text("New file (Cmd/Ctrl+N)")
                .clicked()
            {
                self.new_file();
            }
            if ui.button(s::NEW_REQUEST).clicked() {
                self.new_request();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (label, next) = match self.theme_preference {
                    ThemePreference::System => (s::THEME_SYSTEM, ThemePreference::Light),
                    ThemePreference::Light => (s::THEME_LIGHT, ThemePreference::Dark),
                    ThemePreference::Dark => (s::THEME_DARK, ThemePreference::System),
                };
                if ui.button(label).clicked() {
                    self.theme_preference = next;
                    ui.ctx().set_theme(next);
                }
                let can_save = matches!(self.active_tab(), Some(Tab::Doc(_)));
                if ui
                    .add_enabled(can_save, egui::Button::new(s::SAVE))
                    .on_hover_text("Save (Cmd/Ctrl+S)")
                    .clicked()
                {
                    self.save_active(false);
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        ui.horizontal(|ui| {
            let secondary = |t: String| RichText::new(t).color(p.text_secondary).small();
            match self.active_tab() {
                Some(Tab::Doc(t)) => {
                    let f = t.doc.format();
                    ui.label(secondary(t.doc.language().to_owned()));
                    ui.separator();
                    ui.label(secondary(format!(
                        "Ln {}, Col {}",
                        t.line_col.0, t.line_col.1
                    )));
                    ui.separator();
                    ui.label(secondary("Tab inserts \\t".to_owned()));
                    ui.separator();
                    ui.label(secondary(f.encoding_label().to_owned()));
                    ui.separator();
                    ui.label(secondary(f.line_ending.label().to_owned()));
                    if t.doc.is_dirty() {
                        ui.separator();
                        ui.label(secondary("Unsaved changes".to_owned()));
                    }
                }
                Some(Tab::Http(_)) => {
                    ui.label(secondary("HTTP request".to_owned()));
                }
                None => {
                    ui.label(secondary(String::new()));
                }
            }
            if !self.workspace.opening.is_empty() {
                ui.separator();
                ui.spinner();
                ui.label(secondary(format!(
                    "Opening {} file(s)…",
                    self.workspace.opening.len()
                )));
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        let heading = |t: &str| RichText::new(t).small().strong().color(p.text_secondary);
        ScrollArea::vertical()
            .auto_shrink(false)
            .id_salt("sidebar_scroll")
            .show(ui, |ui| {
                ui.label(heading(s::SECTION_FILES));
                ui.add_space(4.0);
                match self.workspace.root.clone() {
                    Some(root) => self.dir_rows(ui, &root, 0),
                    None => {
                        if ui.button(s::OPEN_FOLDER).clicked() {
                            self.open_folder_dialog();
                        }
                    }
                }
                ui.add_space(12.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(heading(s::SECTION_HTTP));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("+").on_hover_text(s::NEW_REQUEST).clicked() {
                            self.new_request();
                        }
                    });
                });
                let requests: Vec<(TabKey, String)> = self
                    .tabs
                    .iter()
                    .filter_map(|t| match t {
                        Tab::Http(h) => {
                            Some((h.key, format!("{}  {}", h.draft.method.as_str(), h.title)))
                        }
                        Tab::Doc(_) => None,
                    })
                    .collect();
                if requests.is_empty() {
                    ui.label(
                        RichText::new(s::NO_REQUESTS)
                            .color(p.text_secondary)
                            .small(),
                    );
                }
                for (key, label) in requests {
                    if ui
                        .selectable_label(self.active == Some(key), label)
                        .clicked()
                    {
                        self.active = Some(key);
                    }
                }
            });
    }

    fn dir_rows(&mut self, ui: &mut egui::Ui, dir: &Path, depth: usize) {
        let p = palette(ui.ctx());
        let entries = match self.workspace.dirs.get(dir) {
            Some(DirState::Loaded(entries)) => entries.clone(),
            Some(DirState::Failed(e)) => {
                ui.label(
                    RichText::new(format!("Cannot read folder: {e}"))
                        .color(p.error)
                        .small(),
                );
                return;
            }
            Some(DirState::Loading) | None => {
                ui.label(RichText::new(s::LOADING).color(p.text_secondary).small());
                return;
            }
        };
        if entries.is_empty() && depth == 0 {
            ui.label(
                RichText::new(s::EMPTY_FOLDER)
                    .color(p.text_secondary)
                    .small(),
            );
        }
        let indent = 12.0 * depth as f32;
        let mut toggle: Option<PathBuf> = None;
        let mut open: Option<PathBuf> = None;
        for entry in &entries {
            let expanded = self.workspace.expanded.contains(&entry.path);
            ui.horizontal(|ui| {
                ui.add_space(indent);
                let icon = match (entry.is_dir, expanded) {
                    (true, true) => "▾ ",
                    (true, false) => "▸ ",
                    (false, _) => "   ",
                };
                let link = if entry.is_symlink { " ↗" } else { "" };
                let resp = ui.selectable_label(false, format!("{icon}{}{link}", entry.name));
                if resp.clicked() {
                    if entry.is_dir {
                        toggle = Some(entry.path.clone());
                    } else {
                        open = Some(entry.path.clone());
                    }
                }
            });
            if entry.is_dir && expanded {
                self.dir_rows(ui, &entry.path, depth + 1);
            }
        }
        if let Some(dir) = toggle {
            self.workspace.toggle(&dir);
        }
        if let Some(path) = open {
            self.open_path(path);
        }
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        self.notices_bar(ui);
        if self.tabs.is_empty() {
            self.welcome(ui);
            return;
        }
        // Tab strip.
        egui::Frame::NONE
            .fill(p.background)
            .inner_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ScrollArea::horizontal()
                    .id_salt("tab_strip")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| self.tab_strip(ui));
                    });
            });
        ui.add(egui::Separator::default().spacing(0.0));

        let Some(key) = self.active else { return };
        let Some(index) = self.tab_index(key) else {
            return;
        };
        match &mut self.tabs[index] {
            Tab::Doc(tab) => {
                let output = ScrollArea::both()
                    .id_salt(("doc_scroll", tab.key))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        TextEdit::multiline(&mut tab.doc.text)
                            .id(Id::new(("doc_edit", tab.key)))
                            .code_editor()
                            .lock_focus(true)
                            .frame(egui::Frame::NONE)
                            .margin(Margin::symmetric(12, 8))
                            .desired_width(f32::INFINITY)
                            .min_size(ui.available_size())
                            .show(ui)
                    })
                    .inner;
                if output.response.response.changed() {
                    tab.doc.mark_edited();
                }
                if let Some(range) = output.cursor_range {
                    tab.update_cursor(range.primary.index.0);
                }
            }
            Tab::Http(_) => self.http_tab(ui, key),
        }
    }

    fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        let mut activate = None;
        let mut close = None;
        for tab in &self.tabs {
            let key = tab.key();
            let selected = self.active == Some(key);
            let dirty = tab.is_dirty();
            let title = tab.title();
            let prefix = match tab {
                Tab::Http(h) => format!("{} ", h.draft.method.as_str()),
                Tab::Doc(_) => String::new(),
            };
            let frame = egui::Frame::NONE
                .fill(if selected { p.surface } else { p.background })
                .stroke(egui::Stroke::new(
                    1.0,
                    if selected { p.action } else { p.border },
                ))
                .corner_radius(theme::RADIUS)
                .inner_margin(Margin::symmetric(8, 2));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut text = RichText::new(format!("{prefix}{title}"));
                    if selected {
                        text = text.strong().color(p.text);
                    } else {
                        text = text.color(p.text_secondary);
                    }
                    let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                    let accessible = if dirty {
                        format!("{title}, {}", s::MODIFIED)
                    } else {
                        title.clone()
                    };
                    resp.widget_info(|| {
                        WidgetInfo::selected(
                            WidgetType::SelectableLabel,
                            true,
                            selected,
                            &accessible,
                        )
                    });
                    if resp.clicked() {
                        activate = Some(key);
                    }
                    if dirty {
                        ui.label(RichText::new("●").color(p.action))
                            .on_hover_text(s::UNSAVED_TITLE);
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Close tab (Cmd/Ctrl+W)")
                        .clicked()
                    {
                        close = Some(key);
                    }
                });
            });
        }
        if let Some(key) = activate {
            self.active = Some(key);
        }
        if let Some(key) = close {
            self.request_close_tab(key);
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(16.0));
            ui.label(RichText::new(s::APP_NAME).size(28.0).strong().color(p.text));
            ui.label(RichText::new(s::WELCOME_TITLE).color(p.text_secondary));
            ui.add_space(16.0);
            ui.label(
                RichText::new(s::WELCOME_HINT)
                    .color(p.text_secondary)
                    .small(),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let width = 3.0 * 150.0;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                if ui.add(primary_button(ui.ctx(), s::OPEN_FOLDER)).clicked() {
                    self.open_folder_dialog();
                }
                if ui.button(s::OPEN_FILE).clicked() {
                    self.open_file_dialog();
                }
                if ui.button(s::NEW_REQUEST).clicked() {
                    self.new_request();
                }
            });
        });
    }

    fn notices_bar(&mut self, ui: &mut egui::Ui) {
        if self.notices.is_empty() {
            return;
        }
        let p = palette(ui.ctx());
        let mut dismiss = None;
        for (i, notice) in self.notices.iter().enumerate() {
            let color = if notice.is_error {
                p.error
            } else {
                p.text_secondary
            };
            egui::Frame::NONE
                .fill(color.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(1.0, color))
                .corner_radius(theme::RADIUS)
                .inner_margin(Margin::symmetric(12, 6))
                .outer_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let label = if notice.is_error { "Error:" } else { "Info:" };
                        ui.label(RichText::new(label).strong().color(color));
                        ui.label(RichText::new(&notice.text).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button(s::DISMISS).clicked() {
                                dismiss = Some(i);
                            }
                        });
                    });
                });
        }
        if let Some(i) = dismiss {
            self.notices.remove(i);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &self.dialog else { return };
        let p = palette(ctx);
        enum Choice {
            None,
            Save(TabKey, AfterSave),
            Overwrite(TabKey, AfterSave),
            SaveCopy(TabKey, AfterSave),
            Discard(TabKey),
            SaveAll,
            DiscardAll,
            Cancel,
        }
        let mut choice = Choice::None;
        let dirty_names: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.is_dirty())
            .map(Tab::title)
            .collect();
        let tab_name = |key: TabKey| {
            self.tabs
                .iter()
                .find(|t| t.key() == key)
                .map(Tab::title)
                .unwrap_or_default()
        };
        let modal = egui::Modal::new(Id::new("app_dialog")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            match dialog {
                Dialog::CloseTab(key) => {
                    ui.heading(s::UNSAVED_TITLE);
                    ui.label(format!("{} \"{}\"?", s::UNSAVED_ONE, tab_name(*key)));
                    ui.label(RichText::new(s::UNSAVED_HINT).color(p.text_secondary));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button(ui.ctx(), s::SAVE)).clicked() {
                            choice = Choice::Save(*key, AfterSave::CloseTab(*key));
                        }
                        if ui.button(s::DONT_SAVE).clicked() {
                            choice = Choice::Discard(*key);
                        }
                        if ui.button(s::CANCEL).clicked() {
                            choice = Choice::Cancel;
                        }
                    });
                }
                Dialog::Quit => {
                    ui.heading(s::UNSAVED_TITLE);
                    ui.label(s::UNSAVED_MANY);
                    for name in &dirty_names {
                        ui.label(format!("• {name}"));
                    }
                    ui.label(RichText::new(s::UNSAVED_HINT).color(p.text_secondary));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button(ui.ctx(), s::SAVE_ALL)).clicked() {
                            choice = Choice::SaveAll;
                        }
                        if ui.button(s::DISCARD_ALL).clicked() {
                            choice = Choice::DiscardAll;
                        }
                        if ui.button(s::CANCEL).clicked() {
                            choice = Choice::Cancel;
                        }
                    });
                }
                Dialog::Conflict { tab, then } => {
                    ui.heading(s::CONFLICT_TITLE);
                    ui.label(format!("\"{}\"", tab_name(*tab)));
                    ui.label(s::CONFLICT_BODY);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(s::OVERWRITE).clicked() {
                            choice = Choice::Overwrite(*tab, *then);
                        }
                        if ui.button(s::SAVE_AS).clicked() {
                            choice = Choice::SaveCopy(*tab, *then);
                        }
                        if ui.add(primary_button(ui.ctx(), s::CANCEL)).clicked() {
                            choice = Choice::Cancel;
                        }
                    });
                }
            }
        });
        if modal.should_close() && matches!(choice, Choice::None) {
            choice = Choice::Cancel;
        }
        match choice {
            Choice::None => {}
            Choice::Cancel => self.dialog = None,
            Choice::Save(key, then) => {
                self.dialog = None;
                self.save_tab(key, false, false, then);
            }
            Choice::Overwrite(key, then) => {
                self.dialog = None;
                self.save_tab(key, false, true, then);
            }
            Choice::SaveCopy(key, then) => {
                self.dialog = None;
                self.save_tab(key, true, false, then);
            }
            Choice::Discard(key) => {
                self.dialog = None;
                self.close_tab_now(key);
            }
            Choice::SaveAll => {
                self.dialog = None;
                self.save_all_and_quit();
            }
            Choice::DiscardAll => self.quit_now(),
        }
    }
}

pub fn status_color(p: &theme::Palette, status: u16) -> Color32 {
    match status {
        200..=299 => p.success,
        300..=399 => p.action,
        400..=499 => p.warning,
        _ => p.error,
    }
}
