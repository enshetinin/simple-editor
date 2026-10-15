use std::ops::Range;

use tree_sitter::InputEdit;

use super::Hl;

/// A highlighted byte range of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub hl: Hl,
}

/// Moves spans through an edit so colors stay attached to their text until a
/// fresh result arrives.
pub fn shift_spans(spans: &mut Vec<Span>, edit: &InputEdit) {
    let old_end = edit.old_end_byte;
    let new_end = edit.new_end_byte;
    let start = edit.start_byte;
    let map = |pos: usize, is_end: bool| -> usize {
        if pos >= old_end && (pos > start || !is_end) {
            pos - old_end + new_end
        } else if pos > start {
            // Inside the replaced region: collapse to its edges.
            if is_end { start } else { new_end }
        } else {
            pos
        }
    };
    for span in spans.iter_mut() {
        span.start = map(span.start, false);
        span.end = map(span.end, true);
    }
    spans.retain(|s| s.end > s.start);
}

/// Highlight spans for a byte range of one document revision, kept up to
/// date with local edits between results.
#[derive(Debug, Default, Clone)]
pub struct HighlightSpans {
    pub revision: u64,
    pub range: Range<usize>,
    /// Sorted by start, non-overlapping.
    pub spans: Vec<Span>,
    /// Edits applied after `revision` (revision reached, edit).
    log: Vec<(u64, InputEdit)>,
}

const LOG_LIMIT: usize = 512;

impl HighlightSpans {
    /// Empty spans for a document currently at `revision`.
    pub fn at_revision(revision: u64) -> Self {
        Self {
            revision,
            ..Self::default()
        }
    }

    /// Records a local edit that produced `revision`.
    pub fn on_edit(&mut self, revision: u64, edit: InputEdit) {
        shift_spans(&mut self.spans, &edit);
        self.range.start = shift_point(self.range.start, &edit);
        self.range.end = shift_point(self.range.end, &edit);
        self.log.push((revision, edit));
        if self.log.len() > LOG_LIMIT {
            let drop = self.log.len() - LOG_LIMIT;
            self.log.drain(..drop);
        }
    }

    /// Accepts a worker result computed at `revision`, adjusting it through
    /// newer local edits. Returns false if it is too old to use.
    pub fn accept(&mut self, revision: u64, range: Range<usize>, mut spans: Vec<Span>) -> bool {
        if revision < self.revision {
            return false;
        }
        let pending: Vec<_> = self.log.iter().filter(|(r, _)| *r > revision).collect();
        let first_logged = self.log.first().map_or(u64::MAX, |(r, _)| *r);
        let current = self.log.last().map_or(revision, |(r, _)| *r).max(revision);
        if !pending.is_empty() && first_logged > revision + 1 {
            return false; // the log no longer covers the gap
        }
        let mut range = range;
        for (_, edit) in &pending {
            shift_spans(&mut spans, edit);
            range.start = shift_point(range.start, edit);
            range.end = shift_point(range.end, edit);
        }
        self.revision = current;
        self.range = range;
        self.spans = spans;
        self.log.retain(|(r, _)| *r > current);
        true
    }

    pub fn clear(&mut self) {
        self.spans.clear();
        self.range = 0..0;
    }
}

fn shift_point(pos: usize, edit: &InputEdit) -> usize {
    if pos >= edit.old_end_byte {
        pos - edit.old_end_byte + edit.new_end_byte
    } else if pos > edit.start_byte {
        edit.new_end_byte
    } else {
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Point;

    fn edit(start: usize, old_end: usize, new_end: usize) -> InputEdit {
        InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            start_position: Point::new(0, start),
            old_end_position: Point::new(0, old_end),
            new_end_position: Point::new(0, new_end),
        }
    }

    fn span(start: usize, end: usize) -> Span {
        Span {
            start,
            end,
            hl: Hl::Keyword,
        }
    }

    #[test]
    fn insertion_shifts_following_spans_and_grows_containing_one() {
        let mut spans = vec![span(0, 3), span(5, 8), span(10, 12)];
        shift_spans(&mut spans, &edit(6, 6, 9)); // insert 3 bytes at 6
        assert_eq!(spans, vec![span(0, 3), span(5, 11), span(13, 15)]);
    }

    #[test]
    fn deletion_trims_and_drops() {
        let mut spans = vec![span(0, 3), span(4, 6), span(7, 10)];
        shift_spans(&mut spans, &edit(2, 8, 2)); // delete 2..8
        assert_eq!(spans, vec![span(0, 2), span(2, 4)]);
    }

    #[test]
    fn insertion_at_span_end_does_not_extend_it() {
        let mut spans = vec![span(0, 3)];
        shift_spans(&mut spans, &edit(3, 3, 5));
        assert_eq!(spans, vec![span(0, 3)]);
    }

    #[test]
    fn stale_result_is_adjusted_by_newer_edits() {
        let mut h = HighlightSpans::default();
        h.on_edit(1, edit(0, 0, 2)); // revision 1: insert 2 bytes at 0
        // Worker answered for revision 0 (before the edit).
        assert!(h.accept(0, 0..10, vec![span(0, 3)]));
        assert_eq!(h.spans, vec![span(2, 5)]);
        assert_eq!(h.revision, 1);
        // Older than what we have: ignored.
        assert!(!h.accept(0, 0..10, vec![span(0, 1)]));
    }
}
