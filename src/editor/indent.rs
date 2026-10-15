//! Indentation style detection.

use ropey::Rope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    Spaces(u8),
}

impl Default for IndentStyle {
    fn default() -> Self {
        IndentStyle::Spaces(4)
    }
}

impl IndentStyle {
    pub fn unit(self) -> String {
        match self {
            IndentStyle::Tabs => "\t".to_owned(),
            IndentStyle::Spaces(n) => " ".repeat(n as usize),
        }
    }

    pub fn width(self, tab_width: usize) -> usize {
        match self {
            IndentStyle::Tabs => tab_width,
            IndentStyle::Spaces(n) => n as usize,
        }
    }

    pub fn label(self) -> String {
        match self {
            IndentStyle::Tabs => "Tabs".to_owned(),
            IndentStyle::Spaces(n) => format!("Spaces: {n}"),
        }
    }
}

/// Guesses the indentation from the first lines of a document.
pub fn detect(rope: &Rope) -> IndentStyle {
    const MAX_LINES: usize = 2_000;
    let mut tabs = 0usize;
    let mut space_lines = 0usize;
    // Histogram of indentation increases between consecutive lines.
    let mut deltas = [0usize; 9];
    let mut prev_spaces = 0usize;
    for line in rope.lines().take(MAX_LINES) {
        let mut chars = line.chars();
        match chars.next() {
            Some('\t') => {
                tabs += 1;
                continue;
            }
            Some(' ') => {}
            Some('\n') | None => continue,
            Some(_) => {
                prev_spaces = 0;
                continue;
            }
        }
        let spaces = 1 + chars.take_while(|&c| c == ' ').count();
        space_lines += 1;
        if spaces > prev_spaces {
            let delta = spaces - prev_spaces;
            if delta < deltas.len() {
                deltas[delta] += 1;
            }
        }
        prev_spaces = spaces;
    }
    if tabs > space_lines {
        return IndentStyle::Tabs;
    }
    if space_lines == 0 {
        return IndentStyle::default();
    }
    let best = [2usize, 4, 8, 3]
        .into_iter()
        .max_by_key(|&n| deltas[n])
        .unwrap_or(4);
    if deltas[best] == 0 {
        IndentStyle::default()
    } else {
        IndentStyle::Spaces(best as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_styles() {
        let two = Rope::from_str("{\n  a: {\n    b: 1\n  }\n}\n");
        assert_eq!(detect(&two), IndentStyle::Spaces(2));
        let four = Rope::from_str("fn x() {\n    if y {\n        z();\n    }\n}\n");
        assert_eq!(detect(&four), IndentStyle::Spaces(4));
        let tabs = Rope::from_str("func x() {\n\tif y {\n\t\tz()\n\t}\n}\n");
        assert_eq!(detect(&tabs), IndentStyle::Tabs);
        assert_eq!(
            detect(&Rope::from_str("plain\ntext\n")),
            IndentStyle::Spaces(4)
        );
    }
}
