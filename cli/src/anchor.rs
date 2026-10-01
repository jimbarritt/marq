//! Selectors, and resolving an anchor against changed text. Owned by task T-05.
//!
//! The specification is doc/comments-design.md section 4. Everything here is a
//! pure function over strings: the caller reads the files and the recorded
//! version from git. Offsets count Unicode code points (see `crate::text`).

use similar::{capture_diff_slices, Algorithm, DiffOp};

use crate::text;

/// The most code points of context stored on each side of a quote (design 4.1).
pub const CONTEXT_LEN: usize = 32;

/// The selectors of one anchor: a `TextQuoteSelector`, a `TextPositionSelector`
/// and, for a whole-line anchor, an RFC 5147 `FragmentSelector`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selectors {
    /// The quoted text.
    pub exact: String,
    /// Up to `CONTEXT_LEN` code points immediately before the quote.
    pub prefix: String,
    /// Up to `CONTEXT_LEN` code points immediately after the quote.
    pub suffix: String,
    /// The code-point offset of the quote's first character.
    pub start: usize,
    /// The code-point offset just past the quote's last character.
    pub end: usize,
    /// For a whole-line anchor on line N, the RFC 5147 pair `line=N-1,N` as
    /// `Some((N - 1, N))`. `None` for an anchor on a range.
    pub line: Option<(usize, usize)>,
}

/// Where an anchor stands in the current text (design 4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// The quote is in the text at `start..end`.
    Anchored { start: usize, end: usize },
    /// The quote is gone, the line diff locates the same place, and other text
    /// stands at `start..end`. `original` is the quote.
    Changed {
        start: usize,
        end: usize,
        original: String,
    },
    /// No location.
    Orphaned,
}

/// The selectors for the code-point range `start..end` of `text`.
///
/// An empty range is an error, for the reason design 3.3 gives for a blank
/// line: an empty quote matches everywhere.
pub fn selectors_for_range(text: &str, start: usize, end: usize) -> Result<Selectors, String> {
    let chars: Vec<char> = text.chars().collect();
    if end > chars.len() {
        return Err(format!(
            "the range {start}..{end} runs past the end of the text ({} characters)",
            chars.len()
        ));
    }
    if start >= end {
        return Err(format!(
            "the range {start}..{end} is empty, and an empty quote cannot carry a comment"
        ));
    }
    let collect = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    Ok(Selectors {
        exact: collect(start, end),
        prefix: collect(start.saturating_sub(CONTEXT_LEN), start),
        suffix: collect(end, (end + CONTEXT_LEN).min(chars.len())),
        start,
        end,
        line: None,
    })
}

/// The selectors for the whole of 1-based line `line` of `text`. The quote is
/// the line's text without its `\n`.
///
/// A line that is empty or holds only whitespace is an error (design 3.3): its
/// quote would match every such line, and a whitespace-only line looks blank
/// to the reader.
pub fn selectors_for_line(text: &str, line: usize) -> Result<Selectors, String> {
    let Some((start, end)) = text::line_bounds(text, line) else {
        return Err(format!(
            "line {line} does not exist: the text has {} lines",
            text::line_count(text)
        ));
    };
    let content = text::slice(text, start, end).unwrap_or_default();
    if content.trim().is_empty() {
        return Err(format!("line {line} is blank, and a blank line cannot carry a comment"));
    }
    let mut selectors = selectors_for_range(text, start, end)?;
    selectors.line = Some((line - 1, line));
    Ok(selectors)
}

/// Resolves an anchor against `current`, by the steps of design 4.2.
///
/// `recorded` is the text the selectors were written against. When it is
/// `None` the line diff is unavailable: step 2 and step 4 do not run, and step
/// 3 uses `selectors.start` as its hint.
pub fn resolve(selectors: &Selectors, recorded: Option<&str>, current: &str) -> Anchor {
    let cur = Doc::new(current);
    let exact: Vec<char> = selectors.exact.chars().collect();
    if exact.is_empty() {
        return Anchor::Orphaned;
    }
    let is_line = selectors.line.is_some();
    let anchored = |start: usize| Anchor::Anchored {
        start,
        end: start + exact.len(),
    };

    // Step 1: the text is the one the selectors describe.
    if recorded == Some(current) && cur.occurs_at(selectors.start, &exact, is_line) {
        return anchored(selectors.start);
    }

    // Step 2: map the recorded start line through a line diff.
    let mut hint = selectors.start;
    let mut mapped = None;
    let mut replaced = None;
    if let Some(recorded) = recorded {
        let old = Doc::new(recorded);
        if selectors.start < old.chars.len() {
            let old_line = old.line_of(selectors.start);
            let column = selectors.start - old.line_start(old_line);
            let mapping = map_line(&old.line_strs(), &cur.line_strs(), old_line);
            hint = cur.line_start(mapping.hint_line) + column;
            match mapping.kind {
                Kind::Unchanged(line) => {
                    let position = cur.line_start(line) + column;
                    if cur.occurs_at(position, &exact, is_line) {
                        return anchored(position);
                    }
                }
                Kind::Replaced(Some(line)) => {
                    // The same column of the line that took the recorded line's
                    // place: if the quote stands there it is the same text, so
                    // the floor does not apply to it (design 4.3).
                    mapped = Some(cur.line_start(line) + column);
                    replaced = Some((old, old_line, line));
                }
                Kind::Replaced(None) | Kind::Deleted => {}
            }
        }
    }

    // Step 3: search for the quote, scored by its context.
    if let Some(position) = search(&cur, selectors, &exact, is_line, hint, mapped) {
        return anchored(position);
    }

    // Step 4: the recorded line was replaced, so other text stands in its place.
    if let Some((old, old_line, new_line)) = replaced {
        if let Some((start, end)) = changed_in_place(selectors, &exact, &old, old_line, &cur, new_line)
        {
            return Anchor::Changed {
                start,
                end,
                original: selectors.exact.clone(),
            };
        }
    }

    // Step 5.
    Anchor::Orphaned
}

/// A text as code points, with its line bounds.
struct Doc<'a> {
    text: &'a str,
    chars: Vec<char>,
    lines: Vec<(usize, usize)>,
}

impl<'a> Doc<'a> {
    fn new(text: &'a str) -> Self {
        Doc {
            text,
            chars: text.chars().collect(),
            lines: text::line_bounds_all(text),
        }
    }

    /// The lines for the diff, matching `lines` one for one.
    fn line_strs(&self) -> Vec<&'a str> {
        if self.text.is_empty() {
            return Vec::new();
        }
        let mut lines: Vec<&str> = self.text.split('\n').collect();
        if self.text.ends_with('\n') {
            lines.pop();
        }
        lines
    }

    /// The 0-based line holding code-point offset `offset`.
    fn line_of(&self, offset: usize) -> usize {
        self.chars[..offset].iter().filter(|&&c| c == '\n').count()
    }

    /// The offset of 0-based line `line`, or the end of the text past the last.
    fn line_start(&self, line: usize) -> usize {
        self.lines.get(line).map_or(self.chars.len(), |&(start, _)| start)
    }

    /// Whether `exact` stands at `position`; for a line anchor, also that it
    /// fills a whole line (design 4.2: step 3 compares whole lines).
    fn occurs_at(&self, position: usize, exact: &[char], is_line: bool) -> bool {
        let end = position + exact.len();
        if end > self.chars.len() || self.chars[position..end] != *exact {
            return false;
        }
        !is_line
            || ((position == 0 || self.chars[position - 1] == '\n')
                && (end == self.chars.len() || self.chars[end] == '\n'))
    }
}

/// What the line diff did to the recorded line.
enum Kind {
    /// Kept, now at this 0-based line.
    Unchanged(usize),
    /// Inside a hunk with old and new lines; the new line at the same index
    /// within the hunk, or `None` when the hunk shrank past it.
    Replaced(Option<usize>),
    /// Removed with nothing in its place.
    Deleted,
}

struct Mapping {
    kind: Kind,
    /// The current line nearest to where the recorded line was, for the hint.
    hint_line: usize,
}

fn map_line(old: &[&str], new: &[&str], old_line: usize) -> Mapping {
    for op in capture_diff_slices(Algorithm::Myers, old, new) {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } if (old_index..old_index + len).contains(&old_line) => {
                let line = new_index + old_line - old_index;
                return Mapping {
                    kind: Kind::Unchanged(line),
                    hint_line: line,
                };
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } if (old_index..old_index + old_len).contains(&old_line) => {
                let within = old_line - old_index;
                let line = (within < new_len).then_some(new_index + within);
                return Mapping {
                    kind: Kind::Replaced(line),
                    hint_line: line.unwrap_or(new_index + new_len),
                };
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } if (old_index..old_index + old_len).contains(&old_line) => {
                return Mapping {
                    kind: Kind::Deleted,
                    hint_line: new_index,
                };
            }
            _ => {}
        }
    }
    Mapping {
        kind: Kind::Deleted,
        hint_line: new.len(),
    }
}

/// Step 3: every exact occurrence of the quote, scored by context, with the
/// floor of design 4.3. Returns the start of the best candidate.
fn search(
    cur: &Doc,
    selectors: &Selectors,
    exact: &[char],
    is_line: bool,
    hint: usize,
    mapped: Option<usize>,
) -> Option<usize> {
    let prefix: Vec<char> = selectors.prefix.chars().collect();
    let suffix: Vec<char> = selectors.suffix.chars().collect();
    let context = prefix.len() + suffix.len();

    let positions: Box<dyn Iterator<Item = usize>> = if is_line {
        Box::new(cur.lines.iter().map(|&(start, _)| start))
    } else {
        Box::new(0..cur.chars.len())
    };

    positions
        .filter(|&p| cur.occurs_at(p, exact, is_line))
        .map(|p| {
            let before = common_suffix(&cur.chars[..p], &prefix);
            let after = common_prefix(&cur.chars[p + exact.len()..], &suffix);
            (p, before + after)
        })
        .filter(|&(p, score)| 2 * score >= context || Some(p) == mapped)
        .min_by_key(|&(p, score)| (std::cmp::Reverse(score), p.abs_diff(hint), p))
        .map(|(p, _)| p)
}

/// Step 4: the range in `new_line` of the current text that stands where the
/// quote stood in `old_line` of the recorded text, or `None` when nothing does.
fn changed_in_place(
    selectors: &Selectors,
    exact: &[char],
    old: &Doc,
    old_line: usize,
    cur: &Doc,
    new_line: usize,
) -> Option<(usize, usize)> {
    let (old_start, old_end) = *old.lines.get(old_line)?;
    let (new_start, new_end) = *cur.lines.get(new_line)?;
    let new = &cur.chars[new_start..new_end];

    if selectors.line.is_some() {
        // A blank line cannot carry a comment (design 3.3), so a line edited
        // down to blank has nothing left to point at.
        return (!new.iter().all(|c| c.is_whitespace())).then_some((new_start, new_end));
    }

    // The word comparison works within one line, so the quote must lie in the
    // recorded line, and the recorded text must hold it where the selectors say.
    if selectors.end > old_end
        || selectors.start < old_start
        || old.chars.get(selectors.start..selectors.end) != Some(exact)
    {
        return None;
    }
    let old = &old.chars[old_start..old_end];

    let same_start = common_prefix(old, new);
    // The common suffix may not overlap the common prefix in either line.
    let same_end = common_suffix(old, new).min(old.len().min(new.len()) - same_start);
    let removed = &old[same_start..old.len() - same_end];
    let (from, to) = (same_start, new.len() - same_end);
    let added = &new[from..to];

    let (from, to) = if added.iter().any(|c| !c.is_whitespace()) {
        widen(new, from, to)
    } else if !added.is_empty() || removed.iter().any(|c| c.is_whitespace()) {
        // Text replaced by whitespace, or a removal that crossed a word
        // boundary: a word went with nothing in its place.
        return None;
    } else {
        // Letters removed from inside one run, such as a typo fixed by
        // deleting a letter. What remains of the run is the corrected word,
        // unless only punctuation remains, which means the word itself went.
        let (from, to) = widen(new, from, to);
        if !new[from..to].iter().any(|c| c.is_alphanumeric()) {
            return None;
        }
        (from, to)
    };
    (from < to).then_some((new_start + from, new_start + to))
}

/// Widens `from..to` in `line` to the enclosing run of non-whitespace.
fn widen(line: &[char], mut from: usize, mut to: usize) -> (usize, usize) {
    while from > 0 && !line[from - 1].is_whitespace() {
        from -= 1;
    }
    while to < line.len() && !line[to].is_whitespace() {
        to += 1;
    }
    (from, to)
}

/// The number of leading code points `a` and `b` share.
fn common_prefix(a: &[char], b: &[char]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// The number of trailing code points `a` and `b` share.
fn common_suffix(a: &[char], b: &[char]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn widening_stops_at_whitespace_including_carriage_return() {
        let line = chars("say reloadng\r");
        assert_eq!(widen(&line, 8, 8), (4, 12));
        assert_eq!(widen(&line, 3, 3), (0, 3));
        assert_eq!(widen(&line, 4, 4), (4, 12));
    }

    #[test]
    fn common_context_counts_code_points() {
        assert_eq!(common_suffix(&chars("x café"), &chars("y café")), 5);
        assert_eq!(common_prefix(&chars("🦀 a"), &chars("🦀 b")), 2);
        assert_eq!(common_prefix(&chars(""), &chars("a")), 0);
    }

    #[test]
    fn a_line_of_the_diff_matches_a_line_of_the_bounds() {
        for text in ["", "\n", "a", "a\n", "a\n\n", "a\r\nb", "\n\nx\n"] {
            let doc = Doc::new(text);
            assert_eq!(doc.line_strs().len(), doc.lines.len(), "{text:?}");
        }
    }
}
