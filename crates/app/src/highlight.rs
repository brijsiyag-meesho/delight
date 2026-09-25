//! Tiny syntax highlighters for tool output, using Xcode's default palette.

use std::ops::Range;

use gpui::{HighlightStyle, Hsla, rgb};

pub struct Syntax {
    pub key: Hsla,
    pub string: Hsla,
    pub number: Hsla,
    pub keyword: Hsla,
    pub comment: Hsla,
    pub punct: Hsla,
}

impl Syntax {
    pub fn xcode(dark: bool) -> Self {
        let c = |h: u32| -> Hsla { rgb(h).into() };
        if dark {
            Self {
                key: c(0x67B7A4),
                string: c(0xFC6A5D),
                number: c(0xD0BF69),
                keyword: c(0xFC5FA3),
                comment: c(0x6C7986),
                punct: gpui::hsla(0., 0., 1., 0.55),
            }
        } else {
            Self {
                key: c(0x0B4F79),
                string: c(0xC41A16),
                number: c(0x1C00CF),
                keyword: c(0x9B2393),
                comment: c(0x5D6C79),
                punct: gpui::hsla(0., 0., 0., 0.5),
            }
        }
    }
}

fn style(c: Hsla) -> HighlightStyle {
    HighlightStyle { color: Some(c), ..Default::default() }
}

pub fn highlight(language: Option<&str>, text: &str, syntax: &Syntax) -> Vec<(Range<usize>, HighlightStyle)> {
    match language {
        Some("json") => json(text, syntax),
        Some("env") => env(text, syntax),
        _ => Vec::new(),
    }
}

fn json(text: &str, s: &Syntax) -> Vec<(Range<usize>, HighlightStyle)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                let mut j = i;
                while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                    j += 1;
                }
                let is_key = j < b.len() && b[j] == b':';
                out.push((start..i, style(if is_key { s.key } else { s.string })));
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < b.len() && matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    i += 1;
                }
                out.push((start..i, style(s.number)));
            }
            b't' | b'f' | b'n' => {
                let rest = &text[i..];
                let word = ["true", "false", "null"].into_iter().find(|w| rest.starts_with(w));
                match word {
                    Some(w) => {
                        out.push((i..i + w.len(), style(s.keyword)));
                        i += w.len();
                    }
                    None => i += 1,
                }
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                out.push((i..i + 1, style(s.punct)));
                i += 1;
            }
            _ => i += 1,
        }
    }
    // Guard against slicing inside a multi-byte char on malformed input.
    out.retain(|(r, _)| text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
    out
}

fn env(text: &str, s: &Syntax) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let lead = line.len() - trimmed.len();
        if trimmed.starts_with('#') {
            out.push((offset + lead..offset + line.trim_end().len(), style(s.comment)));
        } else if let Some(eq) = line.find('=') {
            let key_start = if trimmed.starts_with("export ") {
                out.push((offset + lead..offset + lead + 6, style(s.keyword)));
                lead + 7
            } else {
                lead
            };
            if key_start < eq {
                out.push((offset + key_start..offset + eq, style(s.key)));
            }
            out.push((offset + eq..offset + eq + 1, style(s.punct)));
            let end = line.trim_end().len();
            if eq + 1 < end {
                out.push((offset + eq + 1..offset + end, style(s.string)));
            }
        }
        offset += line.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_keys_vs_values() {
        let s = Syntax::xcode(true);
        let h = json(r#"{"a": "b", "n": -1.5e3, "t": true}"#, &s);
        let colors: Vec<_> = h.iter().filter_map(|(_, st)| st.color).collect();
        assert!(colors.contains(&s.key) && colors.contains(&s.string) && colors.contains(&s.number));
    }
}
