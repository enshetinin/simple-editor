//! In-document search. Runs on a rope snapshot, typically off the UI thread.

use std::ops::Range;

use ropey::Rope;

/// Stop collecting after this many matches (the UI shows "N+").
pub const MAX_MATCHES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResult {
    /// Char ranges, ascending, non-overlapping.
    pub matches: Vec<Range<usize>>,
    pub truncated: bool,
}

/// Simple case folding that keeps one char per char, so indices stay aligned.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Finds all non-overlapping matches. Queries spanning lines are not
/// supported and return no matches.
pub fn find_all(rope: &Rope, query: &SearchQuery) -> SearchResult {
    let mut result = SearchResult::default();
    if query.text.is_empty() || query.text.contains('\n') {
        return result;
    }
    let needle: String = if query.case_sensitive {
        query.text.clone()
    } else {
        query.text.chars().map(fold).collect()
    };
    let mut line_start = 0usize;
    let mut buf = String::new();
    for line in rope.lines() {
        let line_chars = line.len_chars();
        buf.clear();
        if query.case_sensitive {
            for chunk in line.chunks() {
                buf.push_str(chunk);
            }
        } else {
            buf.extend(line.chars().map(fold));
        }
        let mut last_byte = 0usize;
        let mut last_char = 0usize;
        for (byte, m) in buf.match_indices(needle.as_str()) {
            last_char += buf[last_byte..byte].chars().count();
            last_byte = byte;
            let len = m.chars().count();
            result
                .matches
                .push(line_start + last_char..line_start + last_char + len);
            if result.matches.len() >= MAX_MATCHES {
                result.truncated = true;
                return result;
            }
        }
        line_start += line_chars;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, case_sensitive: bool) -> SearchQuery {
        SearchQuery {
            text: text.to_owned(),
            case_sensitive,
        }
    }

    #[test]
    fn finds_char_ranges_with_unicode() {
        let rope = Rope::from_str("Café 👋 café\nCAFÉ x café");
        let r = find_all(&rope, &q("café", true));
        assert_eq!(r.matches, vec![7..11, 19..23]);
        let r = find_all(&rope, &q("CAFÉ", false));
        assert_eq!(r.matches, vec![0..4, 7..11, 12..16, 19..23]);
        for m in &r.matches {
            assert_eq!(rope.slice(m.clone()).to_string().to_lowercase(), "café");
        }
    }

    #[test]
    fn edges_and_unsupported() {
        let rope = Rope::from_str("aaa");
        assert_eq!(find_all(&rope, &q("aa", true)).matches, vec![0..2]);
        assert!(find_all(&rope, &q("", true)).matches.is_empty());
        assert!(find_all(&rope, &q("a\na", true)).matches.is_empty());
        let rope = Rope::from_str("x\n");
        assert_eq!(find_all(&rope, &q("x", true)).matches, vec![0..1]);
    }
}
