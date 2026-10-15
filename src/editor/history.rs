//! Transactional undo/redo. Each transaction is a list of primitive
//! operations plus the selections before and after, so undo restores both the
//! exact text and the cursor.

use std::time::{Duration, Instant};

use super::selection::Selection;

/// One primitive replacement, recorded in the order it was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    /// Char index where the replacement happened.
    pub at: usize,
    pub removed: String,
    pub inserted: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Single characters typed one after another; merged into one undo step.
    Typing,
    /// Consecutive Backspace presses; merged into one undo step.
    DeleteBackward,
    /// Anything else (paste, cut, replace, indent…): its own undo step.
    Other,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: u64,
    pub ops: Vec<Op>,
    pub before: Selection,
    pub after: Selection,
    kind: EditKind,
    last_edit: Instant,
}

/// Typing pauses longer than this start a new undo step.
const GROUP_TIMEOUT: Duration = Duration::from_millis(1000);
/// Oldest steps are dropped beyond this many.
const MAX_UNDO: usize = 10_000;

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// False after cursor movement or other actions that end a typing run.
    group_open: bool,
    next_id: u64,
}

impl History {
    /// Identifier of the current content state (0 = as loaded).
    pub fn state_id(&self) -> u64 {
        self.undo.last().map_or(0, |t| t.id)
    }

    pub fn break_group(&mut self) {
        self.group_open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Records applied operations, merging natural typing into the last step.
    pub fn record(&mut self, ops: Vec<Op>, before: Selection, after: Selection, kind: EditKind) {
        if ops.is_empty() {
            return;
        }
        self.redo.clear();
        let now = Instant::now();
        if self.group_open
            && kind != EditKind::Other
            && ops.len() == 1
            && let Some(last) = self.undo.last_mut()
            && last.kind == kind
            && last.ops.len() == 1
            && now.duration_since(last.last_edit) < GROUP_TIMEOUT
            && merge(&mut last.ops[0], &ops[0], kind)
        {
            last.after = after;
            last.last_edit = now;
            // Merging changes the content state: give it a fresh id so a save
            // in the middle of a typing run is still tracked correctly.
            self.next_id += 1;
            last.id = self.next_id;
            return;
        }
        self.next_id += 1;
        self.undo.push(Transaction {
            id: self.next_id,
            ops,
            before,
            after,
            kind,
            last_edit: now,
        });
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.group_open = kind != EditKind::Other;
    }

    pub fn pop_undo(&mut self) -> Option<Transaction> {
        let t = self.undo.pop()?;
        self.group_open = false;
        Some(t)
    }

    pub fn push_redo(&mut self, t: Transaction) {
        self.redo.push(t);
    }

    pub fn pop_redo(&mut self) -> Option<Transaction> {
        let t = self.redo.pop()?;
        self.group_open = false;
        Some(t)
    }

    pub fn push_undo(&mut self, t: Transaction) {
        self.undo.push(t);
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.group_open = false;
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn merge(last: &mut Op, new: &Op, kind: EditKind) -> bool {
    match kind {
        EditKind::Typing => {
            let contiguous = new.removed.is_empty()
                && last.removed.is_empty()
                && new.at == last.at + last.inserted.chars().count();
            // A new word after a space or newline starts a new step.
            let boundary = match (last.inserted.chars().last(), new.inserted.chars().next()) {
                (Some(prev), Some(next)) => {
                    next == '\n' || (!is_word_char(prev) && is_word_char(next))
                }
                _ => true,
            };
            if contiguous && !boundary {
                last.inserted.push_str(&new.inserted);
                true
            } else {
                false
            }
        }
        EditKind::DeleteBackward => {
            let contiguous = new.inserted.is_empty()
                && last.inserted.is_empty()
                && new.at + new.removed.chars().count() == last.at;
            if contiguous {
                last.removed.insert_str(0, &new.removed);
                last.at = new.at;
                true
            } else {
                false
            }
        }
        EditKind::Other => false,
    }
}
