//! Anchoring tests (T-05). The first group is one test per row of design 4.4,
//! on the example document the acceptance run edits; the rest are the edge
//! cases the acceptance run cannot reach.

use marq_comments::anchor::{
    resolve, selectors_for_line, selectors_for_range, Anchor, Selectors, CONTEXT_LEN,
};
use marq_comments::text;

const DOC: &str = include_str!("../../example-docs/test.md");

/// The selectors for the `n`th occurrence of `word` on 1-based `line`.
fn on_word(text: &str, line: usize, word: &str, n: usize) -> Selectors {
    let (start, end) = text::find_in_line(text, line, word, n)
        .unwrap_or_else(|| panic!("{word:?} #{n} is not on line {line}"));
    selectors_for_range(text, start, end).unwrap()
}

/// The 1-based line of the first line holding `needle`.
fn line_of(text: &str, needle: &str) -> usize {
    (1..=text::line_count(text))
        .find(|&l| text::line_text(text, l).unwrap().contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not in the text"))
}

/// The code-point offset of the first `needle` in `text`.
fn offset_of(text: &str, needle: &str) -> usize {
    text::char_len(&text[..text.find(needle).expect("needle in text")])
}

fn replace_once(text: &str, old: &str, new: &str) -> String {
    assert_eq!(text.matches(old).count(), 1, "{old:?} must occur once");
    text.replacen(old, new, 1)
}

/// Asserts an `Anchored` result and returns the anchored text.
fn anchored(result: &Anchor, current: &str) -> String {
    match *result {
        Anchor::Anchored { start, end } => text::slice(current, start, end).unwrap().to_string(),
        ref other => panic!("expected anchored, got {other:?}"),
    }
}

/// Asserts a `Changed` result and returns the new text and the original quote.
fn changed(result: &Anchor, current: &str) -> (String, String) {
    match result {
        Anchor::Changed {
            start,
            end,
            original,
        } => (
            text::slice(current, *start, *end).unwrap().to_string(),
            original.clone(),
        ),
        other => panic!("expected changed, got {other:?}"),
    }
}

fn start_of(result: &Anchor) -> usize {
    match *result {
        Anchor::Anchored { start, .. } | Anchor::Changed { start, .. } => start,
        Anchor::Orphaned => panic!("expected a location, got orphaned"),
    }
}

// The edits of acceptance scenarios 05 to 08.

const OPENING: &str = "This is a test document for **Marq**, a macOS markdown viewer.\n";
const ADDED: &str = "\nA new paragraph, added above every comment.\n";

fn paragraph_added_above(text: &str) -> String {
    replace_once(text, OPENING, &format!("{OPENING}{ADDED}"))
}

fn paragraph_moved_to_end(text: &str) -> String {
    let first = text.find("Every entry below").unwrap();
    let marker = "produces a double hyphen.\n";
    let last = text.find(marker).unwrap() + marker.len();
    format!(
        "{}{}\n{}",
        &text[..first],
        text[last..].trim_start_matches('\n'),
        &text[first..last]
    )
}

fn reloading(text: &str) -> Selectors {
    on_word(text, line_of(text, "reloading"), "reloading", 1)
}

fn awkward(text: &str) -> Selectors {
    on_word(text, line_of(text, "Note the awkward"), "awkward", 1)
}

// Design 4.4, one test per row.

#[test]
fn row_01_no_change_anchors_at_the_same_position() {
    let s = reloading(DOC);
    let result = resolve(&s, Some(DOC), DOC);
    assert_eq!(
        result,
        Anchor::Anchored {
            start: s.start,
            end: s.end
        }
    );
}

#[test]
fn row_02_text_added_in_another_paragraph_shifts_the_anchor() {
    let s = reloading(DOC);
    let current = paragraph_added_above(DOC);
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(anchored(&result, &current), "reloading");
    assert_eq!(start_of(&result), s.start + text::char_len(ADDED));
    let (old_line, _) = text::line_col(DOC, s.start).unwrap();
    let (new_line, _) = text::line_col(&current, start_of(&result)).unwrap();
    assert_eq!(new_line, old_line + 2);
}

#[test]
fn row_02_text_removed_in_another_paragraph_shifts_the_anchor() {
    let s = reloading(DOC);
    let current = replace_once(DOC, OPENING, "");
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(anchored(&result, &current), "reloading");
    assert_eq!(start_of(&result), s.start - text::char_len(OPENING));
}

#[test]
fn row_03_paragraph_moved_anchors_at_the_new_position() {
    let s = reloading(DOC);
    let current = paragraph_moved_to_end(DOC);
    assert!(current.ends_with("produces a double hyphen.\n"));
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(anchored(&result, &current), "reloading");
    assert_eq!(start_of(&result), offset_of(&current, "reloading"));
}

#[test]
fn row_04_line_rewritten_word_kept_context_mostly_kept_anchors() {
    let s = reloading(DOC);
    let current = replace_once(
        DOC,
        "heading without leaving the document or reloading it.",
        "the heading, without leaving the page or reloading it.",
    );
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(anchored(&result, &current), "reloading");
    assert_eq!(start_of(&result), offset_of(&current, "reloading"));
}

#[test]
fn row_05_typo_fixed_is_changed_on_the_new_word() {
    let s = reloading(DOC);
    let current = replace_once(DOC, "reloading", "reloadng");
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(
        changed(&result, &current),
        ("reloadng".to_string(), "reloading".to_string())
    );
    assert_eq!(start_of(&result), s.start);
}

#[test]
fn row_06_commented_line_edited_is_changed_on_the_line() {
    let line = line_of(DOC, "reloading");
    let s = selectors_for_line(DOC, line).unwrap();
    let current = replace_once(DOC, "Note the awkward slugs:", "Note the slugs:");
    let result = resolve(&s, Some(DOC), &current);
    let (now, original) = changed(&result, &current);
    assert_eq!(now, text::line_text(&current, line).unwrap());
    assert_eq!(original, text::line_text(DOC, line).unwrap());
}

#[test]
fn row_07_word_deleted_rest_of_line_kept_orphans() {
    let s = awkward(DOC);
    let current = replace_once(DOC, "Note the awkward slugs:", "Note the slugs:");
    assert_eq!(resolve(&s, Some(DOC), &current), Anchor::Orphaned);
}

#[test]
fn row_08_anchored_line_deleted_orphans() {
    let line = line_of(DOC, "reloading");
    let whole = text::line_text(DOC, line).unwrap();
    let current = replace_once(DOC, &format!("{whole}\n"), "");
    assert_eq!(
        resolve(&selectors_for_line(DOC, line).unwrap(), Some(DOC), &current),
        Anchor::Orphaned
    );
    assert_eq!(
        resolve(&reloading(DOC), Some(DOC), &current),
        Anchor::Orphaned
    );
}

#[test]
fn row_09_word_deleted_same_word_elsewhere_orphans() {
    assert!(DOC.matches("awkward").count() > 1);
    let s = awkward(DOC);
    let current = replace_once(DOC, "Note the awkward slugs:", "Note the slugs:");
    assert!(current.contains("awkward"));
    assert_eq!(resolve(&s, Some(DOC), &current), Anchor::Orphaned);
    assert_eq!(resolve(&s, None, &current), Anchor::Orphaned);
}

const TWINS: &str = "# Notes\n\nThe build is fast.\n\nThe build is fast.\n\nEnd.\n";

#[test]
fn row_10_two_identical_sentences_the_second_commented_anchors_on_the_second() {
    let line = 5;
    let s = on_word(TWINS, line, "fast", 1);
    let current = TWINS.replacen("# Notes\n", "# Notes\n\nAn introduction.\n", 1);
    let result = resolve(&s, Some(TWINS), &current);
    assert_eq!(anchored(&result, &current), "fast");
    assert_eq!(
        text::line_col(&current, start_of(&result)).unwrap().0,
        line + 2
    );
}

// Edge cases.

#[test]
fn identical_sentences_without_the_recorded_text_pick_the_one_nearest_the_old_position() {
    let s = on_word(TWINS, 5, "fast", 1);
    let current = TWINS.replacen("# Notes\n", "# Notes\nAn intro.\n", 1);
    let result = resolve(&s, None, &current);
    assert_eq!(text::line_col(&current, start_of(&result)).unwrap().0, 6);
}

#[test]
fn identical_sentences_with_the_second_line_edited_keep_the_second() {
    // The second sentence's line changes elsewhere, so step 2 cannot confirm
    // it; the mapped position escapes the floor and the hint breaks the tie.
    let s = on_word(TWINS, 5, "fast", 1);
    let current = "# Notes\n\nIntro.\n\nThe build is fast.\n\nThe build is fast!\n\nEnd.\n";
    let result = resolve(&s, Some(TWINS), current);
    assert_eq!(anchored(&result, current), "fast");
    assert_eq!(text::line_col(current, start_of(&result)).unwrap().0, 7);
}

const UNICODE: &str =
    "# Café 🦀\n\nUn naïve résumé, e\u{301}crit vite.\nLe crabe 🦀 dit: reloading.\n";

#[test]
fn non_ascii_offsets_count_code_points() {
    let s = on_word(UNICODE, 4, "reloading", 1);
    assert_eq!(s.start, offset_of(UNICODE, "reloading"));
    assert_ne!(s.start, UNICODE.find("reloading").unwrap());
    let current = UNICODE.replacen("\n\n", "\n\nÜber 🦀🦀 ñ.\n\n", 1);
    let result = resolve(&s, Some(UNICODE), &current);
    assert_eq!(anchored(&result, &current), "reloading");
    assert_eq!(
        start_of(&result),
        s.start + text::char_len("Über 🦀🦀 ñ.\n\n")
    );
    let result = resolve(&s, None, &current);
    assert_eq!(anchored(&result, &current), "reloading");
}

#[test]
fn non_ascii_typo_fixes_are_changed_on_the_whole_word() {
    let s = on_word(UNICODE, 3, "naïve", 1);
    let current = UNICODE.replacen("naïve", "naive", 1);
    assert_eq!(
        changed(&resolve(&s, Some(UNICODE), &current), &current),
        ("naive".to_string(), "naïve".to_string())
    );

    // Removing a combining mark deletes one code point and changes no
    // visible letter count.
    let s = on_word(UNICODE, 3, "e\u{301}crit", 1);
    let current = UNICODE.replacen("e\u{301}crit", "ecrit", 1);
    assert_eq!(
        changed(&resolve(&s, Some(UNICODE), &current), &current).0,
        "ecrit"
    );

    let s = on_word(UNICODE, 4, "🦀", 1);
    let current = UNICODE.replacen("crabe 🦀 dit", "crabe 🦞 dit", 1);
    assert_eq!(
        changed(&resolve(&s, Some(UNICODE), &current), &current).0,
        "🦞"
    );
}

const CRLF: &str = "Title\r\n\r\nThe first line here.\r\nIt says reloading now.\r\nLast line.\r\n";

#[test]
fn crlf_text_keeps_the_carriage_return_in_the_line() {
    let line = selectors_for_line(CRLF, 4).unwrap();
    assert_eq!(line.exact, "It says reloading now.\r");
    let current = CRLF.replacen("Title\r\n", "Title\r\nMore\r\n", 1);
    assert_eq!(
        anchored(&resolve(&line, Some(CRLF), &current), &current),
        line.exact
    );

    let word = on_word(CRLF, 4, "now.", 1);
    let current = CRLF.replacen("now.", "nw.", 1);
    let (now, _) = changed(&resolve(&word, Some(CRLF), &current), &current);
    assert_eq!(now, "nw.");

    let edited = CRLF.replacen("reloading", "reloadng", 1);
    let (now, _) = changed(&resolve(&line, Some(CRLF), &edited), &edited);
    assert_eq!(now, "It says reloadng now.\r");
}

#[test]
fn the_word_at_the_start_of_the_file_has_an_empty_prefix() {
    let text = "Alpha beta gamma.\nDelta.\n";
    let s = on_word(text, 1, "Alpha", 1);
    assert_eq!(s.prefix, "");
    let current = "Alpha beta gamma.\nDelta.\nEpsilon.\n";
    assert_eq!(
        anchored(&resolve(&s, Some(text), current), current),
        "Alpha"
    );
    assert_eq!(anchored(&resolve(&s, None, current), current), "Alpha");
    let current = "Alpah beta gamma.\nDelta.\n";
    assert_eq!(
        changed(&resolve(&s, Some(text), current), current).0,
        "Alpah"
    );
}

#[test]
fn the_word_at_the_end_of_the_file_has_an_empty_suffix() {
    let text = "Alpha beta.\nThe end";
    let s = selectors_for_range(text, text::char_len(text) - 3, text::char_len(text)).unwrap();
    assert_eq!((s.exact.as_str(), s.suffix.as_str()), ("end", ""));
    let current = "Intro.\nAlpha beta.\nThe end";
    let result = resolve(&s, Some(text), current);
    assert_eq!(anchored(&result, current), "end");
    assert_eq!(start_of(&result), text::char_len(current) - 3);
    assert_eq!(anchored(&resolve(&s, None, current), current), "end");
    let current = "Alpha beta.\nThe";
    assert_eq!(resolve(&s, Some(text), current), Anchor::Orphaned);
}

#[test]
fn a_quote_that_occurs_many_times_keeps_its_own_occurrence() {
    let line = line_of(DOC, "Note the awkward");
    let s = on_word(DOC, line, "the", 2);
    assert!(DOC.matches("the").count() > 20);
    let current = paragraph_added_above(DOC);
    let expected = text::find_in_line(&current, line + 2, "the", 2).unwrap().0;
    assert_eq!(start_of(&resolve(&s, Some(DOC), &current)), expected);
    // Without the recorded text, the context alone picks it out.
    assert_eq!(start_of(&resolve(&s, None, &current)), expected);
    // And after the paragraph moves.
    let current = paragraph_moved_to_end(DOC);
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(start_of(&result), offset_of(&current, "the awkward slugs"));
}

#[test]
fn a_candidate_at_the_floor_anchors_and_one_below_orphans() {
    let s = selectors_for_range("abcdXYZefgh", 4, 7).unwrap();
    assert_eq!((s.prefix.as_str(), s.suffix.as_str()), ("abcd", "efgh"));
    // Context 8, floor 4. "cd" + "ef" scores exactly 4.
    let at = "zzcdXYZefzz";
    assert_eq!(anchored(&resolve(&s, None, at), at), "XYZ");
    assert_eq!(anchored(&resolve(&s, Some("abcdXYZefgh"), at), at), "XYZ");
    // "d" + "ef" scores 3.
    let below = "zzzdXYZefzz";
    assert_eq!(resolve(&s, None, below), Anchor::Orphaned);

    // With an odd context of 7, the floor is 3.5, so 4 passes and 3 fails.
    let s = selectors_for_range("abcXYZefgh", 3, 6).unwrap();
    assert_eq!(
        anchored(&resolve(&s, None, "zbcXYZefzz"), "zbcXYZefzz"),
        "XYZ"
    );
    assert_eq!(resolve(&s, None, "zzcXYZefzz"), Anchor::Orphaned);
}

#[test]
fn the_full_context_is_thirty_two_code_points_each_side() {
    let s = reloading(DOC);
    assert_eq!(text::char_len(&s.prefix), CONTEXT_LEN);
    assert_eq!(text::char_len(&s.suffix), CONTEXT_LEN);
    assert_eq!(s.prefix, "without leaving the document or ");
}

#[test]
fn the_highest_score_wins_over_the_nearest() {
    let text = "aaaa\nthree ef XYZ gh four\n";
    let s = on_word(text, 2, "XYZ", 1);
    // The near candidate sits at the hint and passes the floor with a trimmed
    // suffix; the far one keeps all its context and wins.
    let current = format!(
        "aaaa\nthree ef XYZ gh fxxx\n{}aaaa\nthree ef XYZ gh four\n",
        "\n".repeat(50)
    );
    let far = text::char_len(&current[..current.rfind("XYZ").unwrap()]);
    assert_eq!(start_of(&resolve(&s, None, &current)), far);
}

#[test]
fn a_tie_goes_to_the_lowest_offset_when_equally_near() {
    let s = selectors_for_range("--XY--", 2, 4).unwrap();
    let current = "XY..XY";
    // Both candidates lose all their context, so the floor of 2 rejects both.
    assert_eq!(resolve(&s, None, current), Anchor::Orphaned);
    // With no stored context both qualify, both score 0 and both lie two
    // from the hint, so the lower offset wins.
    let whole = selectors_for_range("XY", 0, 2).unwrap();
    let s = Selectors {
        start: 2,
        end: 4,
        ..whole
    };
    assert_eq!(start_of(&resolve(&s, None, current)), 0);
}

#[test]
fn a_typo_that_deletes_one_letter_is_changed() {
    let text = "We keep the parameter here.\n";
    let s = on_word(text, 1, "parameter", 1);
    let current = "We keep the paramter here.\n";
    assert_eq!(
        changed(&resolve(&s, Some(text), current), current).0,
        "paramter"
    );
    // The first letter: the common prefix ends on the space before the word.
    let current = "We keep the arameter here.\n";
    assert_eq!(
        changed(&resolve(&s, Some(text), current), current).0,
        "arameter"
    );
    // The last letter.
    let current = "We keep the paramete here.\n";
    assert_eq!(
        changed(&resolve(&s, Some(text), current), current).0,
        "paramete"
    );
}

#[test]
fn a_transposition_is_changed() {
    let s = reloading(DOC);
    let current = replace_once(DOC, "reloading", "relaoding");
    assert_eq!(
        changed(&resolve(&s, Some(DOC), &current), &current).0,
        "relaoding"
    );
}

#[test]
fn a_word_deleted_between_two_spaces_orphans() {
    let s = awkward(DOC);
    let current = replace_once(DOC, "Note the awkward slugs:", "Note the  slugs:");
    assert_eq!(resolve(&s, Some(DOC), &current), Anchor::Orphaned);
}

#[test]
fn a_word_at_the_end_of_a_line_before_punctuation() {
    let line = line_of(DOC, "double hyphen.");
    let s = on_word(DOC, line, "hyphen", 1);
    // A typo fix: the widened range takes the full stop with it.
    let current = replace_once(DOC, "double hyphen.", "double hypen.");
    assert_eq!(
        changed(&resolve(&s, Some(DOC), &current), &current).0,
        "hypen."
    );
    // The word deleted with its space.
    let current = replace_once(DOC, "double hyphen.", "double.");
    assert_eq!(resolve(&s, Some(DOC), &current), Anchor::Orphaned);
    // The word deleted, its space kept: only the full stop remains.
    let current = replace_once(DOC, "double hyphen.", "double .");
    assert_eq!(resolve(&s, Some(DOC), &current), Anchor::Orphaned);
    // The word replaced by another.
    let current = replace_once(DOC, "double hyphen.", "double dash.");
    assert_eq!(
        changed(&resolve(&s, Some(DOC), &current), &current).0,
        "dash."
    );
}

#[test]
fn a_word_replaced_by_whitespace_orphans() {
    let text = "say foo-bar now\n";
    let s = on_word(text, 1, "foo-bar", 1);
    let current = "say foo bar now\n";
    assert_eq!(resolve(&s, Some(text), current), Anchor::Orphaned);
}

#[test]
fn a_whole_line_anchor_follows_edits_moves_and_duplicates() {
    let line = line_of(DOC, "reloading");
    let s = selectors_for_line(DOC, line).unwrap();
    assert_eq!(s.line, Some((line - 1, line)));
    let whole = text::line_text(DOC, line).unwrap();

    // Moved with its paragraph.
    let current = paragraph_moved_to_end(DOC);
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(anchored(&result, &current), whole);
    assert_eq!(start_of(&result), offset_of(&current, whole));

    // Duplicated right after itself: the original keeps it.
    let current = replace_once(DOC, &format!("{whole}\n"), &format!("{whole}\n{whole}\n"));
    let result = resolve(&s, Some(DOC), &current);
    assert_eq!(start_of(&result), s.start);

    // Duplicated at the end of the file: both with and without the diff.
    let current = format!("{DOC}\n{whole}\n");
    assert_eq!(start_of(&resolve(&s, Some(DOC), &current)), s.start);
    assert_eq!(start_of(&resolve(&s, None, &current)), s.start);

    // Edited to blank: nothing left to point at.
    let current = replace_once(DOC, &format!("{whole}\n"), "   \n");
    assert_eq!(resolve(&s, Some(DOC), &current), Anchor::Orphaned);
}

#[test]
fn a_whole_line_anchor_matches_whole_lines_only() {
    let text = "intro\nfast\nmiddle\n";
    let s = selectors_for_line(text, 2).unwrap();
    // "fast" survives inside a longer line, which is not the same line.
    let current = "intro\nvery fast\nmiddle\n";
    assert_eq!(resolve(&s, None, current), Anchor::Orphaned);
    assert_eq!(
        changed(&resolve(&s, Some(text), current), current).0,
        "very fast"
    );
}

#[test]
fn blank_and_missing_lines_cannot_carry_a_comment() {
    assert!(selectors_for_line(DOC, 2).is_err());
    assert!(selectors_for_line("a\n \t\nb", 2).is_err());
    assert!(selectors_for_line("a\r\n\r\nb", 2).is_err());
    assert!(selectors_for_line(DOC, 0).is_err());
    assert!(selectors_for_line(DOC, text::line_count(DOC) + 1).is_err());
    assert!(selectors_for_range(DOC, 5, 5).is_err());
    assert!(selectors_for_range(DOC, 6, 5).is_err());
    assert!(selectors_for_range("abc", 1, 4).is_err());
}

#[test]
fn without_the_recorded_text_only_the_search_runs() {
    let s = reloading(DOC);
    assert_eq!(anchored(&resolve(&s, None, DOC), DOC), "reloading");
    let current = paragraph_moved_to_end(DOC);
    assert_eq!(
        anchored(&resolve(&s, None, &current), &current),
        "reloading"
    );
    // No diff, so no changed status.
    let current = replace_once(DOC, "reloading", "reloadng");
    assert_eq!(resolve(&s, None, &current), Anchor::Orphaned);
}

#[test]
fn an_empty_file() {
    assert!(selectors_for_range("", 0, 0).is_err());
    assert!(selectors_for_line("", 1).is_err());
    let s = reloading(DOC);
    assert_eq!(resolve(&s, Some(DOC), ""), Anchor::Orphaned);
    assert_eq!(resolve(&s, None, ""), Anchor::Orphaned);
    let line = selectors_for_line(DOC, 1).unwrap();
    assert_eq!(resolve(&line, Some(DOC), ""), Anchor::Orphaned);
}

#[test]
fn an_anchor_on_the_whole_file() {
    let text = "One line.\nTwo lines.\n";
    let s = selectors_for_range(text, 0, text::char_len(text)).unwrap();
    assert_eq!((s.prefix.as_str(), s.suffix.as_str()), ("", ""));
    assert_eq!(anchored(&resolve(&s, Some(text), text), text), text);
    // With no context, any exact match qualifies.
    let current = format!("Before.\n{text}After.\n");
    assert_eq!(anchored(&resolve(&s, None, &current), &current), text);
    assert_eq!(anchored(&resolve(&s, Some(text), &current), &current), text);
    // An edit inside a quote that spans lines has no in-line comparison.
    let current = "One line.\nTwo lnes.\n";
    assert_eq!(resolve(&s, Some(text), current), Anchor::Orphaned);
}

// A property check by plain loops with a fixed seed: whatever the edit,
// `resolve` does not panic, an `Anchored` range holds exactly the quote, and a
// `Changed` range is in bounds and not empty.

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: deterministic, and good enough to shuffle edits.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const WORDS: &[&str] = &[
    "the",
    "a",
    "café",
    "🦀",
    "e\u{301}",
    "naïve",
    "build",
    "fast",
    "\n",
    "\n",
    "\r\n",
    " ",
    ".",
    "reloading",
    "the",
    "x",
];

fn random_text(rng: &mut Rng, words: usize) -> String {
    let mut out = String::new();
    for _ in 0..words {
        out.push_str(WORDS[rng.below(WORDS.len())]);
        if rng.below(3) > 0 {
            out.push(' ');
        }
    }
    out
}

fn random_edit(rng: &mut Rng, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let at = rng.below(len + 1);
    let to = (at + rng.below(12)).min(len);
    let head: String = chars[..at].iter().collect();
    let middle: String = chars[at..to].iter().collect();
    let tail: String = chars[to..].iter().collect();
    match rng.below(5) {
        0 => format!("{head}{}{middle}{tail}", random_text(rng, 3)),
        1 => format!("{head}{tail}"),
        2 => format!("{head}{}{tail}", random_text(rng, 2)),
        3 => format!("{tail}{head}{middle}"),
        _ => format!("{head}{middle}{middle}{tail}"),
    }
}

#[test]
fn random_edits_never_panic_or_anchor_on_other_text() {
    let mut rng = Rng(0x5EED_CAFE_F00D_0001);
    let mut outcomes = [0usize; 3];
    for _ in 0..1500 {
        let recorded = if rng.below(4) == 0 {
            let lines: Vec<&str> = DOC.lines().collect();
            let from = rng.below(lines.len());
            lines[from..(from + 12).min(lines.len())].join("\n")
        } else {
            random_text(&mut rng, 40)
        };
        let len = text::char_len(&recorded);
        let selectors = if rng.below(3) == 0 {
            match selectors_for_line(&recorded, 1 + rng.below(text::line_count(&recorded).max(1))) {
                Ok(s) => s,
                Err(_) => continue,
            }
        } else {
            let start = rng.below(len);
            let end = (start + 1 + rng.below(10)).min(len);
            match selectors_for_range(&recorded, start, end) {
                Ok(s) => s,
                Err(_) => continue,
            }
        };
        let mut current = recorded.clone();
        for _ in 0..1 + rng.below(3) {
            current = random_edit(&mut rng, &current);
        }
        let given = if rng.below(4) == 0 {
            None
        } else {
            Some(recorded.as_str())
        };
        let result = resolve(&selectors, given, &current);
        let current_len = text::char_len(&current);
        match result {
            Anchor::Anchored { start, end } => {
                outcomes[0] += 1;
                assert_eq!(
                    text::slice(&current, start, end),
                    Some(selectors.exact.as_str()),
                    "{selectors:?} in {current:?}"
                );
                if selectors.line.is_some() {
                    let (line, column) = text::line_col(&current, start).unwrap();
                    assert_eq!(column, 1);
                    assert_eq!(text::line_bounds(&current, line), Some((start, end)));
                }
            }
            Anchor::Changed {
                start,
                end,
                ref original,
            } => {
                outcomes[1] += 1;
                assert!(
                    start < end && end <= current_len,
                    "{result:?} in {current:?}"
                );
                assert_eq!(original, &selectors.exact);
                assert!(given.is_some(), "changed needs the line diff");
            }
            Anchor::Orphaned => outcomes[2] += 1,
        }
    }
    // Every status occurs, so the loop exercises every step.
    assert!(outcomes.iter().all(|&n| n > 20), "{outcomes:?}");
}
