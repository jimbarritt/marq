//! Code-point text helpers. Owned by task T-05. See doc/comments-design.md 4.1.
//!
//! Every offset here counts Unicode code points (Rust `char`s), never bytes,
//! because the W3C selectors count characters and the stored positions must not
//! depend on an encoding. Lines split on `\n` only: a `\r` before it stays part
//! of the line's text, so positions describe the file as stored (design 8
//! records CRLF working copies as a known limit).
//!
//! Lines and columns are 1-based. A text with a trailing newline has no extra
//! empty line after it: `"a\nb\n"` has two lines, as an editor shows it, and the
//! empty text has none.

/// The number of code points in `text`.
pub fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// The byte offset of code-point offset `offset`, or `None` when `offset` is
/// past the end. `offset == char_len(text)` gives `text.len()`.
pub fn byte_offset(text: &str, offset: usize) -> Option<usize> {
    if offset == 0 {
        return Some(0);
    }
    let mut chars = text.char_indices();
    match chars.nth(offset) {
        Some((byte, _)) => Some(byte),
        None if char_len(text) == offset => Some(text.len()),
        None => None,
    }
}

/// The text between code-point offsets `start` (inclusive) and `end`
/// (exclusive), or `None` when the range is reversed or runs past the end.
pub fn slice(text: &str, start: usize, end: usize) -> Option<&str> {
    if start > end {
        return None;
    }
    let from = byte_offset(text, start)?;
    let to = from + byte_offset(&text[from..], end - start)?;
    Some(&text[from..to])
}

/// The bounds of every line, as code-point `(start, end)` pairs. `end` excludes
/// the `\n` and includes any `\r` before it.
pub fn line_bounds_all(text: &str) -> Vec<(usize, usize)> {
    let mut bounds = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for c in text.chars() {
        if c == '\n' {
            bounds.push((start, offset));
            start = offset + 1;
        }
        offset += 1;
    }
    if start < offset {
        bounds.push((start, offset));
    }
    bounds
}

/// The number of lines in `text`.
pub fn line_count(text: &str) -> usize {
    line_bounds_all(text).len()
}

/// The code-point `(start, end)` of 1-based line `line`, excluding its `\n`,
/// or `None` when the line does not exist.
pub fn line_bounds(text: &str, line: usize) -> Option<(usize, usize)> {
    if line == 0 {
        return None;
    }
    line_bounds_all(text).get(line - 1).copied()
}

/// The text of 1-based line `line`, without its `\n`, or `None` when the line
/// does not exist.
pub fn line_text(text: &str, line: usize) -> Option<&str> {
    let (start, end) = line_bounds(text, line)?;
    slice(text, start, end)
}

/// The 1-based `(line, column)` of code-point offset `offset`, or `None` when
/// `offset` is past the end.
///
/// The line is one more than the number of `\n` before the offset, so an
/// offset that points at a `\n` belongs to the line that newline ends, and the
/// offset just after a trailing newline is column 1 of the line after the last.
pub fn line_col(text: &str, offset: usize) -> Option<(usize, usize)> {
    let mut line = 1;
    let mut line_start = 0;
    let mut seen = 0;
    for c in text.chars() {
        if seen == offset {
            break;
        }
        seen += 1;
        if c == '\n' {
            line += 1;
            line_start = seen;
        }
    }
    if seen < offset {
        return None;
    }
    Some((line, offset - line_start + 1))
}

/// The code-point offset of 1-based `(line, column)`, or `None` when the line
/// does not exist or the column lies past the end of the line. The column one
/// past the last character, the position of the line's end, is allowed.
pub fn offset_at(text: &str, line: usize, column: usize) -> Option<usize> {
    let (start, end) = line_bounds(text, line)?;
    if column == 0 || start + column - 1 > end {
        return None;
    }
    Some(start + column - 1)
}

/// The code-point `(start, end)` of the `occurrence`th (1-based) occurrence of
/// `word` within 1-based line `line`, or `None` when there is no such
/// occurrence. Occurrences do not overlap, as in `str::match_indices`, and a
/// match is any substring: `word` need not stand between spaces.
pub fn find_in_line(
    text: &str,
    line: usize,
    word: &str,
    occurrence: usize,
) -> Option<(usize, usize)> {
    if word.is_empty() || occurrence == 0 {
        return None;
    }
    let (line_start, _) = line_bounds(text, line)?;
    let content = line_text(text, line)?;
    let (byte, _) = content.match_indices(word).nth(occurrence - 1)?;
    let start = line_start + char_len(&content[..byte]);
    Some((start, start + char_len(word)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // "é" is two bytes, "🦀" four, and "e\u{301}" is two code points that
    // render as one letter, so byte, code-point and grapheme counts all differ.
    const MIXED: &str = "café 🦀\ne\u{301}t\u{e9} crab\r\nlast";

    #[test]
    fn slice_counts_code_points_not_bytes() {
        assert_eq!(slice(MIXED, 0, 4), Some("café"));
        assert_eq!(slice(MIXED, 5, 6), Some("🦀"));
        assert_eq!(slice(MIXED, 7, 9), Some("e\u{301}"));
        assert_eq!(slice(MIXED, 0, char_len(MIXED)), Some(MIXED));
        assert_eq!(slice(MIXED, 3, 2), None);
        assert_eq!(slice(MIXED, 0, char_len(MIXED) + 1), None);
        assert_eq!(slice("", 0, 0), Some(""));
    }

    #[test]
    fn lines_split_on_newline_only() {
        assert_eq!(line_count(MIXED), 3);
        assert_eq!(line_text(MIXED, 1), Some("café 🦀"));
        assert_eq!(line_text(MIXED, 2), Some("e\u{301}t\u{e9} crab\r"));
        assert_eq!(line_text(MIXED, 3), Some("last"));
        assert_eq!(line_text(MIXED, 4), None);
        assert_eq!(line_text(MIXED, 0), None);
        assert_eq!(line_bounds(MIXED, 2), Some((7, 17)));
    }

    #[test]
    fn a_trailing_newline_adds_no_line() {
        assert_eq!(line_count(""), 0);
        assert_eq!(line_count("\n"), 1);
        assert_eq!(line_count("a\nb\n"), 2);
        assert_eq!(line_count("a\nb"), 2);
        assert_eq!(line_count("a\n\n"), 2);
        assert_eq!(line_text("a\n\n", 2), Some(""));
    }

    #[test]
    fn line_col_is_one_based_in_code_points() {
        assert_eq!(line_col(MIXED, 0), Some((1, 1)));
        assert_eq!(line_col(MIXED, 5), Some((1, 6)));
        assert_eq!(line_col(MIXED, 6), Some((1, 7)));
        assert_eq!(line_col(MIXED, 7), Some((2, 1)));
        assert_eq!(line_col(MIXED, 18), Some((3, 1)));
        assert_eq!(line_col(MIXED, char_len(MIXED)), Some((3, 5)));
        assert_eq!(line_col(MIXED, char_len(MIXED) + 1), None);
        assert_eq!(line_col("a\n", 2), Some((2, 1)));
        assert_eq!(line_col("", 0), Some((1, 1)));
    }

    #[test]
    fn offset_at_inverts_line_col() {
        for offset in 0..char_len(MIXED) {
            let (line, column) = line_col(MIXED, offset).unwrap();
            assert_eq!(offset_at(MIXED, line, column), Some(offset));
        }
        assert_eq!(offset_at(MIXED, 1, 0), None);
        assert_eq!(offset_at(MIXED, 1, 9), None);
        assert_eq!(offset_at(MIXED, 9, 1), None);
    }

    #[test]
    fn find_in_line_returns_the_nth_occurrence() {
        let text = "one\nthe cat, the café, the 🦀 the\n";
        assert_eq!(find_in_line(text, 2, "the", 1), Some((4, 7)));
        assert_eq!(find_in_line(text, 2, "the", 2), Some((13, 16)));
        assert_eq!(find_in_line(text, 2, "the", 4), Some((29, 32)));
        assert_eq!(find_in_line(text, 2, "the", 5), None);
        assert_eq!(find_in_line(text, 2, "the", 0), None);
        assert_eq!(find_in_line(text, 2, "", 1), None);
        assert_eq!(find_in_line(text, 1, "the", 1), None);
        assert_eq!(find_in_line(text, 3, "the", 1), None);
        let (start, end) = find_in_line(text, 2, "🦀", 1).unwrap();
        assert_eq!(slice(text, start, end), Some("🦀"));
        assert_eq!(find_in_line("aaaa", 1, "aa", 2), Some((2, 4)));
    }
}
