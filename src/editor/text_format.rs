//! Decoding and encoding of file bytes, preserving BOM and line endings.
//!
//! Supported on-disk format in H0: UTF-8, with or without BOM. Anything else is
//! rejected explicitly instead of being converted silently.

use std::borrow::Cow;
use std::fmt;
use std::io::{self, Write};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// Only `\n`. The buffer holds the text as-is.
    Lf,
    /// Only `\r\n`. The buffer holds `\n`; `\r\n` is restored on save.
    Crlf,
    /// Mixed endings (or stray `\r`). The buffer keeps the exact original
    /// characters and nothing is converted on save.
    Mixed,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
            LineEnding::Mixed => "Mixed (preserved)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFormat {
    pub bom: bool,
    pub line_ending: LineEnding,
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            bom: false,
            line_ending: LineEnding::Lf,
        }
    }
}

impl TextFormat {
    pub fn encoding_label(&self) -> &'static str {
        if self.bom { "UTF-8 with BOM" } else { "UTF-8" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The content looks binary (contains NUL bytes).
    Binary,
    /// The content is not valid UTF-8 (byte offset of the first invalid sequence).
    InvalidUtf8 { offset: usize },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Binary => write!(f, "the file looks binary and cannot be edited as text"),
            DecodeError::InvalidUtf8 { offset } => write!(
                f,
                "the file is not valid UTF-8 (first invalid byte at offset {offset}); \
                 other encodings are not supported yet"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes file bytes into editor text plus the format needed to write them back.
pub fn decode(bytes: &[u8]) -> Result<(String, TextFormat), DecodeError> {
    let (bom, body) = match bytes.strip_prefix(UTF8_BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    if body.contains(&0) {
        return Err(DecodeError::Binary);
    }
    let text = std::str::from_utf8(body).map_err(|e| DecodeError::InvalidUtf8 {
        offset: e.valid_up_to() + if bom { UTF8_BOM.len() } else { 0 },
    })?;

    let (line_ending, text) = classify_and_normalize(text);
    Ok((text, TextFormat { bom, line_ending }))
}

fn classify_and_normalize(text: &str) -> (LineEnding, String) {
    let bytes = text.as_bytes();
    let mut crlf = 0usize;
    let mut lone_lf = 0usize;
    let mut lone_cr = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\n' if i > 0 && bytes[i - 1] == b'\r' => crlf += 1,
            b'\n' => lone_lf += 1,
            b'\r' if bytes.get(i + 1) != Some(&b'\n') => lone_cr += 1,
            _ => {}
        }
    }
    match (crlf, lone_lf, lone_cr) {
        (0, _, 0) => (LineEnding::Lf, text.to_owned()),
        (_, 0, 0) => (LineEnding::Crlf, text.replace("\r\n", "\n")),
        _ => (LineEnding::Mixed, text.to_owned()),
    }
}

/// Normalizes text entering the buffer (typing, paste) so a LF/CRLF document
/// only ever holds `\n`. Mixed documents keep the exact characters.
pub fn normalize_input(text: &str, line_ending: LineEnding) -> Cow<'_, str> {
    if line_ending == LineEnding::Mixed || !text.contains('\r') {
        return Cow::Borrowed(text);
    }
    Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Writes buffer chunks to `out` using the format the file was loaded with.
pub fn encode_to<'a, W: Write + ?Sized>(
    chunks: impl Iterator<Item = &'a str>,
    format: &TextFormat,
    out: &mut W,
) -> io::Result<()> {
    if format.bom {
        out.write_all(UTF8_BOM)?;
    }
    for chunk in chunks {
        match format.line_ending {
            LineEnding::Lf | LineEnding::Mixed => out.write_all(chunk.as_bytes())?,
            LineEnding::Crlf => {
                let mut rest = chunk.as_bytes();
                while let Some(pos) = rest.iter().position(|&b| b == b'\n') {
                    out.write_all(&rest[..pos])?;
                    out.write_all(b"\r\n")?;
                    rest = &rest[pos + 1..];
                }
                out.write_all(rest)?;
            }
        }
    }
    Ok(())
}

/// Encodes a whole string (convenience for tests and small buffers).
pub fn encode(text: &str, format: &TextFormat) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 32 + UTF8_BOM.len());
    encode_to(std::iter::once(text), format, &mut out).expect("writing to a Vec cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(bytes: &[u8]) -> TextFormat {
        let (text, format) = decode(bytes).expect("decodes");
        assert_eq!(
            encode(&text, &format),
            bytes,
            "round trip must preserve bytes"
        );
        format
    }

    #[test]
    fn preserves_lf_crlf_mixed_and_bom() {
        assert_eq!(round_trip(b"a\nb\n").line_ending, LineEnding::Lf);
        assert_eq!(round_trip(b"a\r\nb\r\n").line_ending, LineEnding::Crlf);
        assert_eq!(round_trip(b"a\r\nb\nc").line_ending, LineEnding::Mixed);
        assert_eq!(round_trip(b"a\rb\n").line_ending, LineEnding::Mixed);
        assert_eq!(round_trip(b"a\r\r\n").line_ending, LineEnding::Mixed);
        let f = round_trip("\u{FEFF}héllo 👋🏽\r\né\u{301}\r\n".as_bytes());
        assert!(f.bom);
        assert_eq!(f.line_ending, LineEnding::Crlf);
        assert_eq!(round_trip(b"").line_ending, LineEnding::Lf);
    }

    #[test]
    fn crlf_buffer_is_normalized_and_new_lines_become_crlf() {
        let (mut text, format) = decode(b"one\r\ntwo").unwrap();
        assert_eq!(text, "one\ntwo");
        text.push_str("\nthree");
        text.push_str(&normalize_input("\r\nfour\rfive", format.line_ending)); // pasted
        assert_eq!(
            encode(&text, &format),
            b"one\r\ntwo\r\nthree\r\nfour\r\nfive"
        );
        assert_eq!(normalize_input("a\r\nb", LineEnding::Mixed), "a\r\nb");
    }

    #[test]
    fn crlf_encoding_across_chunk_boundaries() {
        let format = TextFormat {
            bom: false,
            line_ending: LineEnding::Crlf,
        };
        let mut out = Vec::new();
        encode_to(["a\n", "\nb", "c\n"].into_iter(), &format, &mut out).unwrap();
        assert_eq!(out, b"a\r\n\r\nbc\r\n");
    }

    #[test]
    fn rejects_binary_and_invalid_utf8() {
        assert_eq!(decode(b"PK\x03\x04\x00\x00"), Err(DecodeError::Binary));
        assert_eq!(
            decode(b"caf\xE9"),
            Err(DecodeError::InvalidUtf8 { offset: 3 })
        );
    }
}
