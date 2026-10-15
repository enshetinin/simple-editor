//! Find / replace in the current document. Matching runs on a worker over a
//! rope snapshot; results carry the revision they were computed for.

use std::ops::Range;

use eframe::egui::{self, Id, Key, RichText, TextEdit};

use super::strings as s;
use super::theme::palette;
use super::{DocTab, SimpleEditorApp, TabKey};
use crate::editor::Selection;
use crate::editor::search::{SearchQuery, SearchResult};

#[derive(Debug, Default)]
pub struct FindState {
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub show_replace: bool,
    pub matches: Vec<Range<usize>>,
    pub truncated: bool,
    /// Query and revision the current `matches` belong to.
    results_for: Option<(u64, SearchQuery)>,
    generation: u64,
    pending: bool,
    pub current: Option<usize>,
    /// Select the next match once fresh results arrive (after a replace).
    advance_when_fresh: bool,
    pub focus_query: bool,
}

impl FindState {
    pub fn query(&self) -> SearchQuery {
        SearchQuery {
            text: self.query.clone(),
            case_sensitive: self.case_sensitive,
        }
    }

    pub fn is_fresh(&self, revision: u64) -> bool {
        self.results_for.as_ref() == Some(&(revision, self.query()))
    }

    pub(super) fn on_result(
        &mut self,
        generation: u64,
        revision: u64,
        query: SearchQuery,
        result: SearchResult,
    ) {
        if generation != self.generation {
            return;
        }
        self.pending = false;
        self.matches = result.matches;
        self.truncated = result.truncated;
        self.results_for = Some((revision, query));
        self.current = None;
    }
}

impl SimpleEditorApp {
    /// Opens the find bar, prefilled with a single-line selection.
    pub fn open_find(&mut self, replace: bool) {
        let Some(key) = self.active else { return };
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        let selected = tab.doc.selected_text();
        let find = tab.find.get_or_insert_with(FindState::default);
        if !selected.is_empty() && !selected.contains('\n') {
            find.query = selected;
        }
        find.show_replace |= replace;
        find.focus_query = true;
    }

    pub fn close_find(&mut self, key: TabKey) {
        if let Some(tab) = self.doc_tab_mut(key) {
            tab.find = None;
        }
        self.ctx
            .memory_mut(|m| m.request_focus(super::view::editor_id(key)));
    }

    /// Starts a search when the query or the document changed.
    pub(super) fn refresh_search(&mut self, key: TabKey) {
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        let revision = tab.doc.revision();
        let Some(find) = tab.find.as_mut() else {
            return;
        };
        if find.pending || find.is_fresh(revision) {
            return;
        }
        let query = find.query();
        if query.text.is_empty() {
            find.matches.clear();
            find.truncated = false;
            find.results_for = Some((revision, query));
            return;
        }
        find.generation += 1;
        find.pending = true;
        let (generation, rope) = (find.generation, tab.doc.rope().clone());
        self.jobs.search(key, generation, revision, rope, query);
    }

    pub fn find_next(&mut self, key: TabKey, backwards: bool) {
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        select_match(tab, backwards);
    }

    pub fn replace_current(&mut self, key: TabKey) {
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        let revision = tab.doc.revision();
        let Some(find) = tab.find.as_mut() else {
            return;
        };
        if !find.is_fresh(revision) {
            return;
        }
        let sel = tab.doc.selection();
        let is_match = find.matches.iter().any(|m| *m == sel.range());
        if is_match {
            let replacement = find.replacement.clone();
            find.advance_when_fresh = true;
            tab.doc.replace_ranges(&[sel.range()], &replacement);
            tab.view.scroll_to_cursor = true;
        } else {
            select_match(tab, false);
        }
    }

    pub fn replace_all(&mut self, key: TabKey) {
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        let revision = tab.doc.revision();
        let Some(find) = tab.find.as_ref() else {
            return;
        };
        if !find.is_fresh(revision) || find.matches.is_empty() {
            return;
        }
        let (matches, replacement) = (find.matches.clone(), find.replacement.clone());
        tab.doc.replace_ranges(&matches, &replacement);
        tab.view.scroll_to_cursor = true;
    }

    /// Draws the find bar above the editor.
    pub(super) fn find_bar(&mut self, ui: &mut egui::Ui, key: TabKey) {
        self.refresh_search(key);
        let p = palette(ui.ctx());
        let mut action: Option<FindAction> = None;
        let Some(tab) = self.doc_tab_mut(key) else {
            return;
        };
        let revision = tab.doc.revision();
        let head = tab.doc.selection();
        let Some(find) = tab.find.as_mut() else {
            return;
        };
        if find.advance_when_fresh && find.is_fresh(revision) {
            find.advance_when_fresh = false;
            action = Some(FindAction::Next);
        }
        // Keep the "n of N" indicator in sync with the selection.
        if find.is_fresh(revision) {
            find.current = find.matches.iter().position(|m| *m == head.range());
        }
        egui::Frame::NONE
            .fill(p.background)
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let query_id = Id::new(("find_query", key));
                    let response = ui.add(
                        TextEdit::singleline(&mut find.query)
                            .id(query_id)
                            .hint_text(s::FIND_HINT)
                            .desired_width(240.0),
                    );
                    if std::mem::take(&mut find.focus_query) {
                        response.request_focus();
                    }
                    if response.has_focus() || response.lost_focus() {
                        let (enter, shift, escape) = ui.input(|i| {
                            (
                                i.key_pressed(Key::Enter),
                                i.modifiers.shift,
                                i.key_pressed(Key::Escape),
                            )
                        });
                        if enter {
                            action = Some(if shift {
                                FindAction::Prev
                            } else {
                                FindAction::Next
                            });
                            response.request_focus();
                        }
                        if escape {
                            action = Some(FindAction::Close);
                        }
                    }
                    ui.toggle_value(&mut find.case_sensitive, "Aa")
                        .on_hover_text(s::MATCH_CASE);
                    let status = if find.query.is_empty() {
                        String::new()
                    } else if !find.is_fresh(revision) {
                        s::SEARCHING.to_owned()
                    } else if find.matches.is_empty() {
                        s::NO_RESULTS.to_owned()
                    } else {
                        let total = if find.truncated {
                            format!("{}+", find.matches.len())
                        } else {
                            find.matches.len().to_string()
                        };
                        match find.current {
                            Some(i) => format!("{} of {total}", i + 1),
                            None => format!("{total} matches"),
                        }
                    };
                    ui.label(RichText::new(status).color(p.text_secondary).small());
                    if ui.small_button("↑").on_hover_text(s::PREV_MATCH).clicked() {
                        action = Some(FindAction::Prev);
                    }
                    if ui.small_button("↓").on_hover_text(s::NEXT_MATCH).clicked() {
                        action = Some(FindAction::Next);
                    }
                    ui.toggle_value(&mut find.show_replace, s::REPLACE);
                    if ui.small_button("×").on_hover_text(s::CLOSE_FIND).clicked() {
                        action = Some(FindAction::Close);
                    }
                });
                if find.show_replace {
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(&mut find.replacement)
                                .id(Id::new(("find_replace", key)))
                                .hint_text(s::REPLACE_HINT)
                                .desired_width(240.0),
                        );
                        let fresh = find.is_fresh(revision) && !find.matches.is_empty();
                        if ui
                            .add_enabled(fresh, egui::Button::new(s::REPLACE))
                            .clicked()
                        {
                            action = Some(FindAction::Replace);
                        }
                        if ui
                            .add_enabled(fresh, egui::Button::new(s::REPLACE_ALL))
                            .clicked()
                        {
                            action = Some(FindAction::ReplaceAll);
                        }
                        if find.truncated {
                            ui.label(RichText::new(s::TOO_MANY_MATCHES).small().color(p.warning));
                        }
                    });
                }
            });
        match action {
            Some(FindAction::Next) => self.find_next(key, false),
            Some(FindAction::Prev) => self.find_next(key, true),
            Some(FindAction::Replace) => self.replace_current(key),
            Some(FindAction::ReplaceAll) => self.replace_all(key),
            Some(FindAction::Close) => self.close_find(key),
            None => {}
        }
    }
}

enum FindAction {
    Next,
    Prev,
    Replace,
    ReplaceAll,
    Close,
}

/// Selects the next (or previous) match relative to the selection, wrapping.
fn select_match(tab: &mut DocTab, backwards: bool) {
    let Some(find) = tab.find.as_mut() else {
        return;
    };
    if find.matches.is_empty() {
        return;
    }
    let sel = tab.doc.selection();
    let index = if backwards {
        let i = find.matches.partition_point(|m| m.start < sel.start());
        if i == 0 {
            find.matches.len() - 1
        } else {
            i - 1
        }
    } else {
        let i = find.matches.partition_point(|m| m.start < sel.end());
        let i = if find.matches.get(i).is_some_and(|m| *m == sel.range()) {
            i + 1
        } else {
            i
        };
        if i >= find.matches.len() { 0 } else { i }
    };
    let m = find.matches[index].clone();
    find.current = Some(index);
    tab.doc.set_selection(Selection::new(m.start, m.end));
    tab.view.scroll_to_cursor = true;
}
