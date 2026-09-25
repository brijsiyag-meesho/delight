//! Input statistics for the status bar.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputStats {
    pub bytes: usize,
    pub chars: usize,
    pub lines: usize,
    pub words: usize,
}

impl InputStats {
    pub fn of(text: &str) -> Self {
        if text.is_empty() {
            return Self::default();
        }
        Self {
            bytes: text.len(),
            chars: text.chars().count(),
            lines: text.lines().count().max(1) + usize::from(text.ends_with('\n')),
            words: text.split_whitespace().count(),
        }
    }

    pub fn human_size(&self) -> String {
        human_bytes(self.bytes)
    }
}

pub fn human_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{:.2} MB", b / (KB * KB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts() {
        let s = InputStats::of("héllo world\nsecond");
        assert_eq!(s.bytes, 19);
        assert_eq!(s.chars, 18);
        assert_eq!(s.lines, 2);
        assert_eq!(s.words, 3);
        assert_eq!(InputStats::of("a\n").lines, 2);
        assert_eq!(human_bytes(2048), "2.00 KB");
    }
}
