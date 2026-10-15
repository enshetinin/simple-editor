//! Unicode-aware positions over a rope: grapheme clusters and words.
//!
//! Positions are char indices into the rope. Cursor movement and deletion use
//! extended grapheme clusters so that emoji sequences, flags, combining marks
//! and `\r\n` are never split.

use ropey::{Rope, RopeSlice, str_utils::byte_to_char_idx};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

/// The grapheme boundary before `char_idx` (or 0).
pub fn prev_grapheme(rope: &Rope, char_idx: usize) -> usize {
    prev_grapheme_in(rope.slice(..), char_idx)
}

/// The grapheme boundary after `char_idx` (or the end).
pub fn next_grapheme(rope: &Rope, char_idx: usize) -> usize {
    next_grapheme_in(rope.slice(..), char_idx)
}

fn prev_grapheme_in(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte, mut chunk_char, _) = slice.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match cursor.prev_boundary(chunk, chunk_byte) {
            Ok(None) => return 0,
            Ok(Some(n)) => return chunk_char + byte_to_char_idx(chunk, n - chunk_byte),
            Err(GraphemeIncomplete::PrevChunk) => {
                (chunk, chunk_byte, chunk_char, _) = slice.chunk_at_byte(chunk_byte - 1);
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let ctx = slice.chunk_at_byte(n - 1).0;
                cursor.provide_context(ctx, n - ctx.len());
            }
            // Not produced by prev_boundary; fall back to one char.
            Err(_) => return char_idx - 1,
        }
    }
}

fn next_grapheme_in(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    let len = slice.len_chars();
    if char_idx >= len {
        return len;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte, mut chunk_char, _) = slice.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match cursor.next_boundary(chunk, chunk_byte) {
            Ok(None) => return len,
            Ok(Some(n)) => return chunk_char + byte_to_char_idx(chunk, n - chunk_byte),
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_byte += chunk.len();
                (chunk, _, chunk_char, _) = slice.chunk_at_byte(chunk_byte);
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let ctx = slice.chunk_at_byte(n - 1).0;
                cursor.provide_context(ctx, n - ctx.len());
            }
            Err(_) => return char_idx + 1,
        }
    }
}

/// Snaps a position to the nearest grapheme boundary at or before it.
pub fn snap_to_grapheme(rope: &Rope, char_idx: usize) -> usize {
    let char_idx = char_idx.min(rope.len_chars());
    if char_idx == 0 || char_idx == rope.len_chars() {
        return char_idx;
    }
    let prev = prev_grapheme(rope, char_idx);
    if next_grapheme(rope, prev) == char_idx {
        char_idx
    } else {
        prev
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Newline,
    Space,
    Word,
    Punct,
}

fn class(c: char) -> CharClass {
    if c == '\n' {
        CharClass::Newline
    } else if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

/// Start of the previous word (skips whitespace first). Stops at line starts.
pub fn prev_word(rope: &Rope, char_idx: usize) -> usize {
    let mut pos = char_idx;
    let mut chars = rope.chars_at(char_idx);
    // Skip spaces (not newlines).
    while let Some(c) = chars.prev() {
        if class(c) != CharClass::Space {
            chars.next();
            break;
        }
        pos -= 1;
    }
    let Some(first) = chars.prev() else {
        return pos;
    };
    pos -= 1;
    let kind = class(first);
    if kind == CharClass::Newline {
        return pos;
    }
    while let Some(c) = chars.prev() {
        if class(c) != kind {
            break;
        }
        pos -= 1;
    }
    pos
}

/// End of the next word (skips whitespace first). Stops at line ends.
pub fn next_word(rope: &Rope, char_idx: usize) -> usize {
    let mut pos = char_idx;
    let mut chars = rope.chars_at(char_idx).peekable();
    while chars.next_if(|&c| class(c) == CharClass::Space).is_some() {
        pos += 1;
    }
    let Some(first) = chars.next() else {
        return pos;
    };
    pos += 1;
    let kind = class(first);
    if kind == CharClass::Newline {
        return pos;
    }
    while chars.next_if(|&c| class(c) == kind).is_some() {
        pos += 1;
    }
    pos
}

/// The word (or run of same-class chars) around a position, for double-click.
pub fn word_at(rope: &Rope, char_idx: usize) -> (usize, usize) {
    let len = rope.len_chars();
    if len == 0 {
        return (0, 0);
    }
    let probe = if char_idx < len { char_idx } else { len - 1 };
    let kind = class(rope.char(probe));
    if kind == CharClass::Newline {
        return (char_idx, char_idx);
    }
    let mut start = probe;
    let mut back = rope.chars_at(probe);
    while let Some(c) = back.prev() {
        if class(c) != kind {
            break;
        }
        start -= 1;
    }
    let mut end = probe;
    for c in rope.chars_at(probe) {
        if class(c) != kind {
            break;
        }
        end += 1;
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphemes_keep_clusters_together() {
        // family emoji (ZWJ sequence), flag, combining accent, CRLF, skin tone
        let text = "a👨‍👩‍👧🇪🇸e\u{301}\r\n👋🏽";
        let rope = Rope::from_str(text);
        let mut stops = vec![0];
        let mut pos = 0;
        while pos < rope.len_chars() {
            pos = next_grapheme(&rope, pos);
            stops.push(pos);
        }
        // a | 👨‍👩‍👧 (5 chars) | 🇪🇸 (2) | é (2) | \r\n (2) | 👋🏽 (2)
        assert_eq!(stops, vec![0, 1, 6, 8, 10, 12, 14]);
        let mut back = vec![rope.len_chars()];
        let mut pos = rope.len_chars();
        while pos > 0 {
            pos = prev_grapheme(&rope, pos);
            back.push(pos);
        }
        back.reverse();
        assert_eq!(back, stops);
        assert_eq!(snap_to_grapheme(&rope, 3), 1);
        assert_eq!(snap_to_grapheme(&rope, 6), 6);
    }

    #[test]
    fn graphemes_across_rope_chunks() {
        // Long enough to span many internal chunks; clusters sit on boundaries.
        let unit = "x👨‍👩‍👧";
        let text = unit.repeat(5_000);
        let rope = Rope::from_str(&text);
        let mut pos = 0;
        let mut count = 0;
        while pos < rope.len_chars() {
            pos = next_grapheme(&rope, pos);
            count += 1;
        }
        assert_eq!(count, 10_000);
        let mut pos = rope.len_chars();
        let mut count = 0;
        while pos > 0 {
            pos = prev_grapheme(&rope, pos);
            count += 1;
        }
        assert_eq!(count, 10_000);
    }

    #[test]
    fn word_motion() {
        let rope = Rope::from_str("let café_1 = foo.bar;\nnext");
        assert_eq!(next_word(&rope, 0), 3);
        assert_eq!(next_word(&rope, 3), 10);
        assert_eq!(prev_word(&rope, 10), 4);
        assert_eq!(next_word(&rope, 20), 21); // stops at newline
        assert_eq!(prev_word(&rope, 22), 21);
        assert_eq!(word_at(&rope, 6), (4, 10));
        assert_eq!(word_at(&rope, 15), (13, 16));
    }
}
