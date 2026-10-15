//! Editor for project variables and environments.

use eframe::egui::{self, Id, RichText, ScrollArea, TextEdit};

use super::jobs::{JobResult, SecretOp};
use super::strings as s;
use super::theme::{palette, primary_button};
use super::{AfterSave, SimpleEditorApp, Tab, TabKey};
use crate::http::model::Variable;
use crate::http::secrets::secret_key;
use crate::storage::project::{Environment, environments_dir, unique_id};

pub struct EnvTab {
    pub key: TabKey,
    pub variables: Vec<Variable>,
    pub environments: Vec<Environment>,
    deleted: Vec<String>,
    original: (Vec<Variable>, Vec<Environment>),
    /// 0 = project variables, n = environments[n - 1].
    pub selected: usize,
    new_env_name: String,
    pub saving: bool,
}

impl EnvTab {
    pub fn is_dirty(&self) -> bool {
        !self.deleted.is_empty()
            || self.variables != self.original.0
            || self.environments != self.original.1
    }
}

impl SimpleEditorApp {
    pub fn open_environments(&mut self) {
        if let Some(t) = self.tabs.iter().find(|t| matches!(t, Tab::Env(_))) {
            self.active = Some(t.key());
            return;
        }
        let Some(project) = self.project.as_ref().filter(|p| p.loaded) else {
            self.notice_error("Open a folder to edit its environments and variables.");
            return;
        };
        let variables = project.data.variables.clone();
        let environments = project.data.environments.clone();
        let selected = project
            .active_env
            .as_ref()
            .and_then(|id| environments.iter().position(|e| &e.id == id))
            .map_or(0, |i| i + 1);
        let key = self.alloc_key();
        self.tabs.push(Tab::Env(Box::new(EnvTab {
            key,
            original: (variables.clone(), environments.clone()),
            variables,
            environments,
            deleted: Vec::new(),
            selected,
            new_env_name: String::new(),
            saving: false,
        })));
        self.active = Some(key);
    }

    /// Writes project variables and environments (no secret values) and
    /// stores remembered secrets in the OS credential store.
    pub fn save_environments(&mut self, key: TabKey, then: AfterSave) -> bool {
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            return false;
        };
        let secrets = self.secrets.clone();
        let Some(tab) = self.tabs.iter_mut().find_map(|t| match t {
            Tab::Env(e) if e.key == key => Some(e.as_mut()),
            _ => None,
        }) else {
            return false;
        };
        if tab.saving {
            return true;
        }
        let mut ops = Vec::new();
        let mut collect = |scope: String, vars: &[Variable]| {
            for v in vars.iter().filter(|v| v.secret) {
                let key = secret_key(&root, &scope, &v.name);
                if v.remember && !v.value.is_empty() {
                    ops.push(SecretOp::Set {
                        key,
                        label: v.name.clone(),
                        value: v.value.clone(),
                    });
                } else if !v.remember {
                    ops.push(SecretOp::Delete { key });
                }
            }
        };
        collect("project".to_owned(), &tab.variables);
        for env in &tab.environments {
            collect(format!("env:{}", env.id), &env.variables);
        }
        tab.saving = true;
        self.jobs.save_environments(
            key,
            root,
            tab.variables.clone(),
            tab.environments.clone(),
            tab.deleted.clone(),
            ops,
            secrets,
            then,
        );
        true
    }

    pub(super) fn on_env_saved(&mut self, result: JobResult) {
        let JobResult::EnvironmentsSaved {
            tab: key,
            result,
            secret_errors,
            then,
        } = result
        else {
            return;
        };
        let backend = self.secrets.name().to_owned();
        let Some(tab) = self.tabs.iter_mut().find_map(|t| match t {
            Tab::Env(e) if e.key == key => Some(e.as_mut()),
            _ => None,
        }) else {
            return;
        };
        tab.saving = false;
        // A secret that could not be stored stays in memory only: show it.
        for (name, _) in &secret_errors {
            for v in tab
                .variables
                .iter_mut()
                .chain(
                    tab.environments
                        .iter_mut()
                        .flat_map(|e| e.variables.iter_mut()),
                )
                .filter(|v| &v.name == name && v.secret)
            {
                v.remember = false;
            }
        }
        match result {
            Ok(()) => {
                tab.original = (tab.variables.clone(), tab.environments.clone());
                tab.deleted.clear();
                let (vars, envs) = tab.original.clone();
                if let Some(p) = self.project.as_mut() {
                    p.data.variables = vars;
                    p.data.environments = envs;
                    if p.active_env
                        .as_ref()
                        .is_some_and(|id| !p.data.environments.iter().any(|e| &e.id == id))
                    {
                        p.active_env = None;
                    }
                }
                if !secret_errors.is_empty() {
                    let list: Vec<String> = secret_errors
                        .iter()
                        .map(|(n, e)| format!("{{{{{n}}}}} ({e})"))
                        .collect();
                    self.notice_error(format!(
                        "Could not store these secrets in {backend}; they are kept for this session only and were not written anywhere: {}",
                        list.join(", ")
                    ));
                }
                self.after_save(key, then);
            }
            Err(e) => {
                self.quit_after_saves = false;
                self.notice_error(format!(
                    "Could not save environments: {e}. Your changes are still open."
                ));
            }
        }
    }

    pub(super) fn env_tab_view(&mut self, ui: &mut egui::Ui, key: TabKey) {
        let p = palette(ui.ctx());
        let backend = self.secrets.name().to_owned();
        let mut save = false;
        let Some(tab) = self.tabs.iter_mut().find_map(|t| match t {
            Tab::Env(e) if e.key == key => Some(e.as_mut()),
            _ => None,
        }) else {
            return;
        };
        let root = self.project.as_ref().map(|p| p.root.clone());
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(s::ENVIRONMENTS);
                    if ui
                        .add_enabled(
                            tab.is_dirty() && !tab.saving,
                            primary_button(ui.ctx(), s::SAVE),
                        )
                        .on_hover_text("Save (Cmd/Ctrl+S)")
                        .clicked()
                    {
                        save = true;
                    }
                    if tab.saving {
                        ui.spinner();
                    }
                });
                ui.label(RichText::new(s::ENV_HELP).small().color(p.text_secondary));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut tab.selected, 0, s::PROJECT_VARIABLES);
                    for (i, env) in tab.environments.iter().enumerate() {
                        ui.selectable_value(&mut tab.selected, i + 1, &env.name);
                    }
                    ui.add(
                        TextEdit::singleline(&mut tab.new_env_name)
                            .hint_text(s::NEW_ENVIRONMENT_HINT)
                            .desired_width(140.0),
                    );
                    if ui
                        .add_enabled(!tab.new_env_name.trim().is_empty(), egui::Button::new("+"))
                        .on_hover_text(s::ADD_ENVIRONMENT)
                        .clicked()
                        && let Some(root) = &root
                    {
                        let name = tab.new_env_name.trim().to_owned();
                        let mut id = unique_id(&environments_dir(root), &name);
                        while tab.environments.iter().any(|e| e.id == id) {
                            id.push('x');
                        }
                        tab.environments.push(Environment {
                            id,
                            name,
                            variables: Vec::new(),
                        });
                        tab.selected = tab.environments.len();
                        tab.new_env_name.clear();
                    }
                });
                ui.separator();
                let selected = tab.selected;
                if selected > 0 && selected <= tab.environments.len() {
                    let env = &mut tab.environments[selected - 1];
                    let mut delete = false;
                    ui.horizontal(|ui| {
                        ui.label(s::NAME);
                        ui.add(TextEdit::singleline(&mut env.name).desired_width(200.0));
                        if ui.button(s::DELETE_ENVIRONMENT).clicked() {
                            delete = true;
                        }
                    });
                    if delete {
                        let env = tab.environments.remove(selected - 1);
                        tab.deleted.push(env.id);
                        tab.selected = 0;
                        return;
                    }
                }
                let vars = if selected == 0 || selected > tab.environments.len() {
                    &mut tab.variables
                } else {
                    &mut tab.environments[selected - 1].variables
                };
                variables_table(ui, vars, key, selected, &backend);
            });
        if save {
            self.save_environments(key, AfterSave::Nothing);
        }
    }
}

/// Editable table of variables (name, value, secret, remember).
pub fn variables_table(
    ui: &mut egui::Ui,
    vars: &mut Vec<Variable>,
    key: TabKey,
    salt: usize,
    backend: &str,
) {
    let p = palette(ui.ctx());
    let mut remove = None;
    ScrollArea::vertical()
        .id_salt(("vars", key, salt))
        .max_height(ui.available_height().max(120.0) - 40.0)
        .show(ui, |ui| {
            egui::Grid::new(("vars_grid", key, salt))
                .num_columns(5)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new(s::NAME).small().strong());
                    ui.label(RichText::new(s::VALUE).small().strong());
                    ui.label(RichText::new(s::SECRET).small().strong());
                    ui.label(RichText::new(s::REMEMBER).small().strong())
                        .on_hover_text(format!("{} {backend}", s::REMEMBER_HINT));
                    ui.label("");
                    ui.end_row();
                    for (i, v) in vars.iter_mut().enumerate() {
                        ui.add(
                            TextEdit::singleline(&mut v.name)
                                .id(Id::new(("var_name", key, salt, i)))
                                .font(egui::TextStyle::Monospace)
                                .desired_width(160.0),
                        );
                        ui.add(
                            TextEdit::singleline(&mut v.value)
                                .id(Id::new(("var_value", key, salt, i)))
                                .password(v.secret)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(260.0),
                        );
                        if ui.checkbox(&mut v.secret, "").changed() && !v.secret {
                            v.remember = false;
                        }
                        ui.add_enabled(v.secret, egui::Checkbox::without_text(&mut v.remember));
                        if ui.small_button("×").on_hover_text(s::REMOVE_ROW).clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            if ui.button(s::ADD_VARIABLE).clicked() {
                vars.push(Variable::plain("", ""));
            }
        });
    if let Some(i) = remove {
        vars.remove(i);
    }
    ui.label(
        RichText::new(s::SECRET_NOTE)
            .small()
            .color(p.text_secondary),
    );
}
