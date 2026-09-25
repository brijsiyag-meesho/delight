//! Pure text navigation on a `&str` with byte offsets: grapheme, word and
//! line boundaries, clamping, and UTF-16 conversion for the platform's input
//! handler. No GPUI, so it's unit-tested directly.

use std::ops::Range;

use unicode_segmentation::GraphemeCursor;

/// The grapheme boundary before `offset` (0 at the start). Looks only at the
/// text around `offset`, so it stays fast on large inputs.
pub fn prev_grapheme(text: &str, offset: usize) -> usize {
    let offset = floor_char(text, offset);
    GraphemeCursor::new(offset, text.len(), true).prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

/// The grapheme boundary after `offset` (the length at the end).
pub fn next_grapheme(text: &str, offset: usize) -> usize {
    let offset = floor_char(text, offset);
    GraphemeCursor::new(offset, text.len(), true).next_boundary(text, 0).ok().flatten().unwrap_or(text.len())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Start of the word before `offset`, skipping separators first (⌥←).
pub fn word_left(text: &str, offset: usize) -> usize {
    let before = &text[..offset];
    before.trim_end_matches(|c: char| !is_word(c)).trim_end_matches(is_word).len()
}

/// End of the word after `offset`, skipping separators first (⌥→).
pub fn word_right(text: &str, offset: usize) -> usize {
    let after = &text[offset..];
    text.len() - after.trim_start_matches(|c: char| !is_word(c)).trim_start_matches(is_word).len()
}

/// The word around `offset` (double-click).
pub fn word_at(text: &str, offset: usize) -> Range<usize> {
    let end = word_right(text, offset);
    word_left(text, offset.min(end))..end
}

pub fn line_start(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |i| i + 1)
}

pub fn line_end(text: &str, offset: usize) -> usize {
    text[offset..].find('\n').map_or(text.len(), |i| offset + i)
}

/// The largest char boundary at or below `offset`, within the text.
pub fn floor_char(text: &str, offset: usize) -> usize {
    let mut i = offset.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `range` limited to the text, ordered, and snapped to char boundaries.
/// Offsets arrive from the platform (IME) and the mouse: never trust them.
pub fn clamp(text: &str, range: Range<usize>) -> Range<usize> {
    let end = floor_char(text, range.end);
    floor_char(text, range.start.min(end))..end
}

pub fn to_utf16(text: &str, offset: usize) -> usize {
    text[..floor_char(text, offset)].chars().map(char::len_utf16).sum()
}

pub fn from_utf16(text: &str, offset: usize) -> usize {
    let (mut utf8, mut utf16) = (0, 0);
    for c in text.chars() {
        if utf16 >= offset {
            break;
        }
        utf16 += c.len_utf16();
        utf8 += c.len_utf8();
    }
    utf8
}

pub fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    to_utf16(text, range.start)..to_utf16(text, range.end)
}

pub fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    from_utf16(text, range.start)..from_utf16(text, range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphemes_move_as_one() {
        let text = "a👍🏽é"; // thumbs-up + skin tone is one grapheme; é is one char
        assert_eq!(next_grapheme(text, 0), 1);
        assert_eq!(next_grapheme(text, 1), 1 + "👍🏽".len());
        assert_eq!(prev_grapheme(text, text.len()), text.len() - "é".len());
        assert_eq!(prev_grapheme(text, 1 + "👍🏽".len()), 1);
        assert_eq!(prev_grapheme(text, 0), 0);
        assert_eq!(next_grapheme(text, text.len()), text.len());
    }

    #[test]
    fn words_and_lines() {
        let text = "foo_bar  baz\nqux";
        assert_eq!(word_left(text, 11), 9);
        assert_eq!(word_left(text, 9), 0);
        assert_eq!(word_right(text, 0), 7);
        assert_eq!(word_right(text, 7), 12);
        assert_eq!(word_at(text, 2), 0..7);
        assert_eq!(line_start(text, 15), 13);
        assert_eq!(line_end(text, 2), 12);
        assert_eq!(line_end(text, 14), text.len());
    }

    #[test]
    fn clamps_untrusted_offsets() {
        let text = "héllo";
        assert_eq!(clamp(text, 2..99), 1..text.len(), "inside é snaps down; past the end is the end");
        assert_eq!(clamp(text, 4..1), 1..1, "reversed collapses");
    }

    #[test]
    fn converts_utf16() {
        let text = "a😀b"; // 😀 is 4 bytes, 2 UTF-16 units
        assert_eq!(to_utf16(text, 5), 3);
        assert_eq!(from_utf16(text, 3), 5);
        assert_eq!(range_from_utf16(text, &(1..3)), 1..5);
    }
}
