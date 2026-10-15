//! JSON pretty printing for responses. Re-indents tokens instead of building
//! a value tree, so key order, number spelling and string escapes are kept
//! exactly as the server sent them. Runs on a worker thread.

/// Validates `text` as JSON. Returns the parser error message if invalid.
pub fn validate(text: &str) -> Result<(), String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text)
        .map(|_| ())
        .map_err(|e| format!("line {}, column {}: {e}", e.line(), e.column()))
}

/// Pretty prints valid JSON with two-space indentation.
pub fn pretty(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut indent = 0usize;
    let mut i = 0;
    let newline = |out: &mut String, indent: usize| {
        out.push('\n');
        for _ in 0..indent {
            out.push_str("  ");
        }
    };
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' => {
                // Copy the whole string, honoring escapes.
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                out.push_str(&text[start..i.min(bytes.len())]);
                continue;
            }
            b'{' | b'[' => {
                out.push(c as char);
                // Empty container stays on one line.
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'}' || bytes[j] == b']') {
                    out.push(bytes[j] as char);
                    i = j + 1;
                    continue;
                }
                indent += 1;
                newline(&mut out, indent);
            }
            b'}' | b']' => {
                indent = indent.saturating_sub(1);
                newline(&mut out, indent);
                out.push(c as char);
            }
            b',' => {
                out.push(',');
                newline(&mut out, indent);
            }
            b':' => out.push_str(": "),
            c if c.is_ascii_whitespace() => {}
            _ => {
                // Numbers, true/false/null: copy the run as is.
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b',' | b'}' | b']' | b':' | b'"')
                    && !bytes[i].is_ascii_whitespace()
                {
                    i += 1;
                }
                out.push_str(&text[start..i]);
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Whether a response should be treated as JSON.
pub fn looks_like_json(content_type: Option<&str>, body: &str) -> bool {
    let ct_json = content_type.is_some_and(|ct| {
        let ct = ct.to_ascii_lowercase();
        ct.contains("/json") || ct.contains("+json")
    });
    ct_json || matches!(body.trim_start().as_bytes().first(), Some(b'{' | b'['))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_keeps_order_numbers_and_escapes() {
        let bs = '\\';
        let src = format!(
            r#"{{"z":1,"a":[1.50,2e10,{{}}],"s":"x{bs}"y,{{}}","e":[],"n":null,"u":"ñ{bs}u00e9"}}"#
        );
        let p = pretty(&src);
        let expected = format!(
            "{{\n  \"z\": 1,\n  \"a\": [\n    1.50,\n    2e10,\n    {{}}\n  ],\n  \"s\": \"x{bs}\"y,{{}}\",\n  \"e\": [],\n  \"n\": null,\n  \"u\": \"ñ{bs}u00e9\"\n}}"
        );
        assert_eq!(p, expected);
        // Re-parsing gives the same value.
        let a: serde_json::Value = serde_json::from_str(&src).unwrap();
        let b: serde_json::Value = serde_json::from_str(&p).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn validation_and_detection() {
        assert!(validate("{\"a\":1}").is_ok());
        assert!(validate("{\"a\":}").unwrap_err().contains("line 1"));
        assert!(looks_like_json(Some("application/problem+json"), "x"));
        assert!(looks_like_json(None, "  [1]"));
        assert!(!looks_like_json(Some("text/html"), "<p>"));
    }
}
