//! Tests for the static HTML page (T-10). Threads are hand-written in the shape
//! of design 6.2, so the page is tested without a git repository.

use marq_comments::render::{render_page, render_source_page};
use serde_json::{json, Value};

const TEST_MD: &str = include_str!("../../example-docs/test.md");

struct T {
    id: &'static str,
    state: &'static str,
    anchor: Value,
    quote: &'static str,
    body: &'static str,
    author: &'static str,
    agent: bool,
    suggestion: Option<&'static str>,
    replies: Vec<Value>,
}

impl T {
    fn new(id: &'static str, anchor: Value) -> T {
        T {
            id,
            state: "open",
            anchor,
            quote: "quote",
            body: "A body.",
            author: "Jim",
            agent: false,
            suggestion: None,
            replies: Vec::new(),
        }
    }

    fn json(&self) -> Value {
        let creator = if self.agent {
            json!({"type": "Software", "name": self.author})
        } else {
            json!({"type": "Person", "name": self.author, "email": "mailto:jim@example.com"})
        };
        let selector = json!([
            {"type": "TextQuoteSelector", "exact": self.quote, "prefix": "", "suffix": ""},
            {"type": "TextPositionSelector", "start": 0, "end": 1}
        ]);
        let (motivation, body) = match self.suggestion {
            Some(replacement) => (
                "editing",
                json!([
                    {"type": "TextualBody", "value": replacement, "purpose": "editing"},
                    {"type": "TextualBody", "value": self.body, "purpose": "commenting"}
                ]),
            ),
            None => (
                "commenting",
                json!({"type": "TextualBody", "value": self.body, "format": "text/markdown"}),
            ),
        };
        json!({
            "annotation": {
                "id": format!("urn:uuid:{}-8a57-4c63-9f1b-2e6d7a90b3c4", self.id),
                "type": "Annotation",
                "created": "2026-09-29T10:15:02Z",
                "creator": creator,
                "motivation": motivation,
                "body": body,
                "target": {"source": "doc.md", "selector": selector}
            },
            "state": self.state,
            "anchor": self.anchor,
            "stateChanges": [],
            "replies": self.replies
        })
    }
}

fn anchored(start: usize, end: usize) -> Value {
    json!({"status": "anchored", "start": start, "end": end, "line": 1, "column": 1, "text": ""})
}

fn changed(start: usize, end: usize, original: &str) -> Value {
    json!({"status": "changed", "start": start, "end": end, "line": 1, "column": 1,
           "text": "now", "original": original})
}

fn orphaned() -> Value {
    json!({"status": "orphaned"})
}

fn reply(id: &str, author: &str, body: &str, agent: bool) -> Value {
    let creator = if agent { "Software" } else { "Person" };
    json!({
        "annotation": {
            "id": format!("urn:uuid:{id}-0000-4000-8000-000000000000"),
            "type": "Annotation",
            "created": "2026-09-29T10:20:00Z",
            "creator": {"type": creator, "name": author},
            "motivation": "replying",
            "body": {"type": "TextualBody", "value": body, "format": "text/markdown"},
            "target": "urn:uuid:x"
        },
        "state": "open",
        "anchor": orphaned(),
        "stateChanges": [],
        "replies": []
    })
}

/// The source view, which the tests above this line exercise.
fn page(markdown: &str, threads: &[T]) -> String {
    let values: Vec<Value> = threads.iter().map(T::json).collect();
    render_source_page(markdown, &values)
}

/// The rendered view, the default.
fn rpage(markdown: &str, threads: &[T]) -> String {
    let values: Vec<Value> = threads.iter().map(T::json).collect();
    render_page(markdown, &values)
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// The source `<pre>` contents, between its tags.
fn source_html(html: &str) -> &str {
    let open = "<pre class=\"source\">";
    let from = html.find(open).expect("source pre") + open.len();
    let to = from + html[from..].find("</pre>").expect("closing pre");
    &html[from..to]
}

/// The text inside every `<mark>` that belongs to `thread` (1-based position in
/// the input), joined, with nested marks and links walked through.
fn marked_text(html: &str, thread: usize) -> String {
    marked_text_in(source_html(html), thread)
}

fn marked_text_in(source: &str, thread: usize) -> String {
    let want = format!("data-thread=\"t-{thread}\"");
    let mut stack: Vec<bool> = Vec::new();
    let mut out = String::new();
    let mut rest = source;
    while !rest.is_empty() {
        if rest.starts_with('<') {
            let end = rest.find('>').expect("tag end");
            let tag = &rest[..=end];
            if tag.starts_with("<mark") {
                stack.push(tag.contains(&want));
            } else if tag == "</mark>" {
                stack.pop();
            } else if tag.starts_with("<a ") {
                // A link marker: its digits are not source text.
                let close = rest.find("</a>").expect("link end");
                rest = &rest[close + 4..];
                continue;
            }
            rest = &rest[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            if stack.iter().any(|m| *m) {
                out.push_str(&rest[..end]);
            }
            rest = &rest[end..];
        }
    }
    unescape(&out)
}

#[test]
fn a_mark_covers_exactly_its_characters() {
    let text = "alpha beta gamma\n";
    let html = page(text, &[T::new("aaaaaaaa", anchored(6, 10))]);
    assert_eq!(marked_text(&html, 1), "beta");
    assert!(source_html(&html).contains("alpha <a class=\"ref\""));
}

#[test]
fn offsets_are_code_points_after_non_ascii_text() {
    // "é" is two bytes, "🦀" four, so byte offsets would land mid-word.
    let text = "café 🦀 crabs eat é🦀 well\n";
    let start = text.chars().position(|c| c == 'c').unwrap();
    let crabs = text.find("crabs").unwrap();
    let crabs = text[..crabs].chars().count();
    assert_ne!(crabs, text.find("crabs").unwrap());
    let html = page(
        text,
        &[
            T::new("aaaaaaaa", anchored(start, start + 4)),
            T::new("bbbbbbbb", anchored(crabs, crabs + 5)),
            T::new("cccccccc", anchored(5, 6)),
        ],
    );
    assert_eq!(marked_text(&html, 1), "café");
    assert_eq!(marked_text(&html, 2), "crabs");
    assert_eq!(marked_text(&html, 3), "🦀");
}

#[test]
fn marks_work_on_the_example_document() {
    let heading = "Marq Test Document";
    let byte = TEST_MD.find(heading).unwrap();
    let start = TEST_MD[..byte].chars().count();
    let html = page(
        TEST_MD,
        &[T::new(
            "aaaaaaaa",
            anchored(start, start + heading.chars().count()),
        )],
    );
    assert_eq!(marked_text(&html, 1), heading);
    // Text before the mark survives, escaped, in source order.
    assert!(source_html(&html).contains("# ![Marq](../assets/logo.svg) "));
}

#[test]
fn the_gutter_numbers_every_line() {
    let html = page("one\ntwo\nthree\n", &[]);
    assert!(html.contains("<pre class=\"gutter\" aria-hidden=\"true\">1\n2\n3</pre>"));
}

#[test]
fn input_is_escaped_in_source_authors_bodies_and_quotes() {
    let text = "a <b> & \"c\" 'd'\n";
    let mut t = T::new("aaaaaaaa", anchored(2, 5));
    t.author = "<i>Eve</i> & \"co\"";
    t.body = "x < y && y > z \"q\" 'r'";
    t.quote = "<q>\"&'";
    t.state = "open";
    let mut o = T::new("bbbbbbbb", orphaned());
    o.quote = "<u>quote</u>";
    o.body = "<img src=x onerror=alert(1)>";
    let html = page(text, &[t, o]);
    assert!(source_html(&html).contains("a <a class=\"ref\""));
    assert!(html.contains("&lt;b&gt;"));
    assert!(html.contains(" &amp; &quot;c&quot; &#39;d&#39;"));
    assert!(html.contains("&lt;i&gt;Eve&lt;/i&gt; &amp; &quot;co&quot;"));
    assert!(html.contains("x &lt; y &amp;&amp; y &gt; z &quot;q&quot; &#39;r&#39;"));
    assert!(html.contains("&lt;u&gt;quote&lt;/u&gt;"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!html.contains("<img"));
    assert!(!html.contains("<i>Eve"));
    assert!(!html.contains("<u>"));
}

#[test]
fn a_script_in_the_input_is_inert() {
    let evil = "</pre><script>alert(1)</script>";
    let text = format!("{evil}\nnext\n");
    let mut t = T::new("aaaaaaaa", anchored(0, evil.chars().count()));
    t.author = evil;
    t.body = evil;
    t.quote = evil;
    let mut o = T::new("bbbbbbbb", orphaned());
    o.author = evil;
    o.quote = evil;
    o.body = evil;
    o.replies = vec![reply("cccccccc", evil, evil, false)];
    let html = page(&text, &[t, o]);
    assert!(html.contains("&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("alert(1)</script>"));
    assert_eq!(html.matches("</pre>").count(), 2);
}

#[test]
fn overlapping_ranges_are_both_marked() {
    let text = "0123456789\n";
    let html = page(
        text,
        &[
            T::new("aaaaaaaa", anchored(2, 7)),
            T::new("bbbbbbbb", anchored(5, 9)),
        ],
    );
    assert_eq!(marked_text(&html, 1), "23456");
    assert_eq!(marked_text(&html, 2), "5678");
    assert!(html.contains("href=\"#t-1\""));
    assert!(html.contains("href=\"#t-2\""));
}

#[test]
fn one_range_inside_another_and_equal_ranges_are_both_marked() {
    let html = page(
        "0123456789\n",
        &[
            T::new("aaaaaaaa", anchored(1, 9)),
            T::new("bbbbbbbb", anchored(3, 5)),
            T::new("cccccccc", anchored(1, 9)),
        ],
    );
    assert_eq!(marked_text(&html, 1), "12345678");
    assert_eq!(marked_text(&html, 2), "34");
    assert_eq!(marked_text(&html, 3), "12345678");
}

#[test]
fn adjacent_ranges_do_not_bleed_into_each_other() {
    let html = page(
        "abcdef\n",
        &[
            T::new("aaaaaaaa", anchored(0, 3)),
            T::new("bbbbbbbb", anchored(3, 6)),
        ],
    );
    assert_eq!(marked_text(&html, 1), "abc");
    assert_eq!(marked_text(&html, 2), "def");
}

#[test]
fn a_range_ending_at_the_end_of_the_text_is_marked() {
    let html = page("abc", &[T::new("aaaaaaaa", anchored(1, 3))]);
    assert_eq!(marked_text(&html, 1), "bc");
}

#[test]
fn an_out_of_range_anchor_is_clamped_not_a_panic() {
    let html = page("abc", &[T::new("aaaaaaaa", anchored(2, 99))]);
    assert_eq!(marked_text(&html, 1), "c");
}

#[test]
fn marks_are_coloured_by_state() {
    let states = ["open", "resolved", "accepted", "rejected"];
    let threads: Vec<T> = states
        .iter()
        .enumerate()
        .map(|(i, state)| {
            let mut t = T::new("aaaaaaaa", anchored(i, i + 1));
            t.state = state;
            t
        })
        .collect();
    let html = page("abcd\n", &threads);
    for (i, state) in states.iter().enumerate() {
        assert!(
            html.contains(&format!(
                "<mark class=\"s-{state} anchored\" data-thread=\"t-{}\"",
                i + 1
            )),
            "{state}"
        );
    }
}

#[test]
fn a_changed_mark_differs_and_its_card_shows_the_original() {
    let mut t = T::new("aaaaaaaa", changed(0, 3, "the \"old\" <text>"));
    t.quote = "ignored when original is present";
    let html = page("now it is\n", &[t]);
    assert!(html.contains("<mark class=\"s-open changed\""));
    assert!(html.contains("was <q>the &quot;old&quot; &lt;text&gt;</q>"));
    assert!(html.contains("mark.changed"));
    assert_eq!(marked_text(&html, 1), "now");
}

#[test]
fn an_anchored_card_has_no_was_line() {
    let html = page("abc\n", &[T::new("aaaaaaaa", anchored(0, 1))]);
    assert!(!html.contains("class=\"was\""));
}

#[test]
fn cards_show_id_state_kind_author_time_and_line_column() {
    let text = "first\nsecond line\n";
    let mut t = T::new("4f0c2d1e", anchored(9, 13));
    t.state = "resolved";
    let html = page(text, &[t]);
    assert!(html.contains("<code class=\"id\">4f0c2d1e</code>"));
    assert!(html.contains("<span class=\"state s-resolved\">resolved</span>"));
    assert!(html.contains("<span class=\"kind\">comment</span>"));
    assert!(html.contains("<span class=\"author\">Jim</span>"));
    assert!(html.contains("<time>2026-09-29T10:15:02Z</time>"));
    assert!(html.contains("<span class=\"loc\">2:4</span>"));
    assert!(html.contains("A body."));
}

#[test]
fn a_suggestion_card_shows_the_old_and_new_text() {
    let mut t = T::new("aaaaaaaa", anchored(0, 5));
    t.quote = "three";
    t.suggestion = Some("five <5>");
    t.body = "Settings say five.";
    t.state = "accepted";
    let html = page("three\n", &[t]);
    assert!(html.contains("<span class=\"kind\">suggestion</span>"));
    assert!(html.contains("<del>three</del>"));
    assert!(html.contains("<ins>five &lt;5&gt;</ins>"));
    assert!(html.contains("Settings say five."));
}

#[test]
fn a_bare_suggestion_without_a_reason_renders() {
    let mut t = T::new("aaaaaaaa", anchored(0, 5));
    t.quote = "three";
    t.suggestion = Some("");
    t.body = "";
    let html = page("three\n", &[t]);
    assert!(html.contains("<del>three</del>"));
}

#[test]
fn newlines_in_a_body_are_kept() {
    let mut t = T::new("aaaaaaaa", anchored(0, 1));
    t.body = "line one\nline two";
    let html = page("abc\n", &[t]);
    assert!(html.contains("line one\nline two"));
    assert!(html.contains("white-space: pre-wrap"));
}

#[test]
fn the_orphan_section_lists_orphans_with_their_quote_and_body() {
    let mut o = T::new("bbbbbbbb", orphaned());
    o.quote = "the lost words";
    o.body = "Orphan body.";
    o.replies = vec![reply("dddddddd", "Claude", "Orphan reply.", true)];
    let html = page("abc\n", &[T::new("aaaaaaaa", anchored(0, 1)), o]);
    let section = html
        .find("<h2>Orphaned comments</h2>")
        .expect("orphan heading");
    assert!(section > html.find("</pre>").unwrap());
    let tail = &html[section..];
    assert!(tail.contains("<blockquote class=\"quote\">the lost words</blockquote>"));
    assert!(tail.contains("Orphan body."));
    assert!(tail.contains("Orphan reply."));
    assert!(!tail.contains("A body."));
    assert!(!html[..section].contains("the lost words"));
}

#[test]
fn there_is_no_orphan_section_without_orphans() {
    let html = page("abc\n", &[T::new("aaaaaaaa", anchored(0, 1))]);
    assert!(!html.contains("Orphaned comments"));
}

#[test]
fn an_anchor_the_page_cannot_place_is_listed_as_an_orphan() {
    let broken = T::new("aaaaaaaa", json!({"status": "anchored"}));
    let missing = T::new("bbbbbbbb", Value::Null);
    let html = page("abc\n", &[broken, missing]);
    assert!(html.contains("Orphaned comments"));
    assert!(html.contains("0 anchored, 0 changed, 2 orphaned"));
}

#[test]
fn a_reply_is_nested_under_its_thread_and_a_reply_to_a_reply_under_that() {
    let mut inner = reply("eeeeeeee", "Jim", "Third level.", false);
    inner["replies"] = json!([]);
    let mut first = reply("ffffffff", "Claude", "Second level.", true);
    first["replies"] = json!([inner]);
    let mut t = T::new("aaaaaaaa", anchored(0, 1));
    t.replies = vec![first];
    let html = page("abc\n", &[t]);
    let card = html.find("<article").unwrap();
    let replies = html[card..].find("<div class=\"replies\">").unwrap() + card;
    let second = html.find("Second level.").unwrap();
    let third = html.find("Third level.").unwrap();
    assert!(card < replies && replies < second && second < third);
    assert_eq!(html.matches("<div class=\"replies\">").count(), 2);
    assert!(html.contains("<code class=\"id\">ffffffff</code>"));
}

#[test]
fn an_agent_author_is_labelled_and_a_person_is_not() {
    let mut agent = T::new("aaaaaaaa", anchored(0, 1));
    agent.author = "Claude";
    agent.agent = true;
    let person = T::new("bbbbbbbb", anchored(1, 2));
    let html = page("abc\n", &[agent, person]);
    assert_eq!(html.matches("(agent)</span>").count(), 1);
    assert!(
        html.contains("<span class=\"author\">Claude</span> <span class=\"agent\">(agent)</span>")
    );
}

#[test]
fn the_summary_counts_threads_by_anchor_status() {
    let html = page(
        "abcdef\n",
        &[
            T::new("aaaaaaaa", anchored(0, 1)),
            T::new("bbbbbbbb", anchored(1, 2)),
            T::new("cccccccc", changed(2, 3, "z")),
            T::new("dddddddd", orphaned()),
        ],
    );
    assert!(html.contains("4 threads: 2 anchored, 1 changed, 1 orphaned"));
}

#[test]
fn the_summary_counts_a_reply_with_its_thread_not_alone() {
    let mut t = T::new("aaaaaaaa", anchored(0, 1));
    t.replies = vec![reply("eeeeeeee", "Jim", "r", false)];
    let html = page("abc\n", &[t]);
    assert!(html.contains("1 thread: 1 anchored, 0 changed, 0 orphaned"));
}

#[test]
fn an_empty_thread_list_shows_the_source_and_no_comments() {
    let html = page("# Title\n\nbody\n", &[]);
    assert!(html.contains("0 threads: 0 anchored, 0 changed, 0 orphaned"));
    assert!(html.contains("No comments"));
    assert!(html.contains("# Title\n\nbody\n"));
    assert!(!html.contains("<mark"));
    assert!(!html.contains("<article"));
}

#[test]
fn an_empty_source_renders() {
    let html = page("", &[]);
    assert!(html.contains("<pre class=\"source\"></pre>"));
}

#[test]
fn the_page_is_self_contained() {
    let mut o = T::new("bbbbbbbb", orphaned());
    o.quote = "see http://example.com/a and https://example.com/b";
    let mut t = T::new("aaaaaaaa", anchored(0, 3));
    t.body = "<a href=\"https://evil.example\">x</a>";
    let html = page("http://in.the.source\n", &[t, o]);
    assert!(!html.contains("<script"));
    assert!(!html.contains("<link"));
    assert!(!html.contains("<img"));
    assert!(!html.contains("@import"));
    assert!(!html.contains("url("));
    // A URL may appear as escaped text, never in an attribute value.
    for attribute in ["href=\"http", "src=\"", "action=\"", "data=\"http"] {
        assert!(!html.contains(attribute), "{attribute}");
    }
    assert!(html.contains("name=\"viewport\""));
    assert!(html.contains("prefers-color-scheme: dark"));
    assert!(html.contains("https://example.com/b"));
}

#[test]
fn a_hostile_state_does_not_reach_a_class_attribute() {
    let mut t = T::new("aaaaaaaa", anchored(0, 1));
    t.state = "open\" onmouseover=\"alert(1)";
    let html = page("abc\n", &[t]);
    assert!(!html.contains("onmouseover=\"alert"));
}

/// Writes a realistic source-view page to `target/render-sample-source.html`.
/// The rendered view's sample is written by `writes_a_rendered_sample_page`.
#[test]
fn writes_a_sample_page() {
    let html = sample_html(render_source_page);
    write_sample("render-sample-source.html", &html);
}

// ---- the rendered view ----

/// The code-point range of the `nth` (0-based) occurrence of `needle`.
fn at_nth(text: &str, needle: &str, nth: usize) -> (usize, usize) {
    let byte = text
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("{needle:?} not found"))
        .0;
    let start = text[..byte].chars().count();
    (start, start + needle.chars().count())
}

fn at(text: &str, needle: &str) -> (usize, usize) {
    at_nth(text, needle, 0)
}

fn on(text: &str, needle: &str) -> T {
    let (s, e) = at(text, needle);
    T::new("aaaaaaaa", anchored(s, e))
}

/// The document pane of the rendered view.
fn doc_html(html: &str) -> &str {
    let open = "<article class=\"markdown-body\">";
    let from = html.find(open).expect("document article") + open.len();
    let to = from + html[from..].find("</article>").expect("closing article");
    &html[from..to]
}

/// What the single thread on `needle` marks in the rendered view.
fn marks(text: &str, needle: &str) -> String {
    let html = rpage(text, &[on(text, needle)]);
    marked_text_in(doc_html(&html), 1)
}

#[test]
fn a_mark_covers_exactly_its_characters_in_a_paragraph() {
    assert_eq!(marks("Hello brave new world\n", "brave new"), "brave new");
    let html = rpage("Hello brave world\n", &[on("Hello brave world\n", "brave")]);
    assert!(html
        .contains("<p>Hello <a class=\"ref\" href=\"#t-1\">1</a><mark class=\"s-open anchored\""));
    assert!(html.contains(">brave</mark> world</p>"));
}

#[test]
fn a_mark_works_in_bold_italic_and_inline_code() {
    assert_eq!(marks("a **bold word** here\n", "bold"), "bold");
    assert_eq!(marks("a *slanted word* here\n", "word"), "word");
    assert_eq!(marks("use `cargo test` now\n", "cargo test"), "cargo test");
    assert_eq!(marks("use ``a ` b`` now\n", "a ` b"), "a ` b");
    assert_eq!(marks("a ~~struck~~ here\n", "struck"), "struck");
    let html = rpage(
        "a **bold word** here\n",
        &[on("a **bold word** here\n", "bold")],
    );
    assert!(doc_html(&html).contains("<strong>"));
}

#[test]
fn a_mark_works_in_a_heading_a_list_item_and_a_table_cell() {
    assert_eq!(marks("## The Title here\n", "Title"), "Title");
    assert_eq!(
        marks("- one\n- two items\n  - nested thing\n", "two"),
        "two"
    );
    assert_eq!(
        marks("- one\n- two items\n  - nested thing\n", "nested thing"),
        "nested thing"
    );
    assert_eq!(marks("1. first\n2. second\n", "second"), "second");
    let table = "| a | b |\n|---|---|\n| left cell | right cell |\n";
    assert_eq!(marks(table, "right cell"), "right cell");
    assert_eq!(marks(table, "a"), "a");
    assert_eq!(marks("> quoted words\n", "words"), "words");
}

#[test]
fn a_mark_works_in_a_fenced_code_block() {
    let md = "Before\n\n```rust\nlet x = 1;\nlet y = 2;\n```\n\nAfter\n";
    assert_eq!(marks(md, "x = 1"), "x = 1");
    // Across two lines, the newline between them is marked too.
    assert_eq!(marks(md, "1;\nlet y"), "1;\nlet y");
    let html = rpage(md, &[on(md, "x = 1")]);
    assert!(html.contains("<pre><code class=\"language-rust\">"));
    let indented = "para\n\n    indented code\n    more\n";
    assert_eq!(marks(indented, "indented code"), "indented code");
}

#[test]
fn a_mark_works_in_a_link_text() {
    let md = "See [the link text](https://example.com/x) now\n";
    assert_eq!(marks(md, "link text"), "link text");
    let html = rpage(md, &[on(md, "link text")]);
    assert!(html.contains("<a href=\"https://example.com/x\" rel=\"noopener noreferrer\">"));
}

#[test]
fn a_range_across_bold_and_plain_text_is_split_but_covers_all_of_it() {
    let md = "start **bold** tail end\n";
    let (s, _) = at(md, "start");
    let (_, e) = at(md, "tail");
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(s, e))]);
    assert_eq!(marked_text_in(doc_html(&html), 1), "start bold tail");
    // One mark per rendered text piece: "start ", "bold" and " tail".
    assert_eq!(html.matches("data-thread=\"t-1\"").count(), 3);
    // One reference number for the thread, not one per piece.
    assert_eq!(html.matches("class=\"ref\"").count(), 1);
}

#[test]
fn a_range_across_a_soft_line_break_and_two_paragraphs_is_marked() {
    assert_eq!(marks("line one\nline two\n", "one\nline"), "one\nline");
    let md = "first para\n\nsecond para\n";
    let (s, _) = at(md, "para");
    let (_, e) = at(md, "second");
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(s, e))]);
    assert_eq!(marked_text_in(doc_html(&html), 1), "parasecond");
}

#[test]
fn overlapping_and_adjacent_ranges_are_marked_by_thread() {
    let md = "alpha beta gamma delta\n";
    let (a, _) = at(md, "alpha");
    let (_, b) = at(md, "beta");
    let (c, _) = at(md, "beta");
    let (_, d) = at(md, "gamma");
    let html = rpage(
        md,
        &[
            T::new("aaaaaaaa", anchored(a, b)),
            T::new("bbbbbbbb", anchored(c, d)),
        ],
    );
    let doc = doc_html(&html);
    assert_eq!(marked_text_in(doc, 1), "alpha beta");
    assert_eq!(marked_text_in(doc, 2), "beta gamma");
    // The shared word sits inside both marks, outermost first.
    assert!(doc.contains("title=\"bbbbbbbb open (anchored)\">beta</mark></mark>"));

    let md = "abcdef\n";
    let html = rpage(
        md,
        &[
            T::new("aaaaaaaa", anchored(0, 3)),
            T::new("bbbbbbbb", anchored(3, 6)),
        ],
    );
    let doc = doc_html(&html);
    assert_eq!(marked_text_in(doc, 1), "abc");
    assert_eq!(marked_text_in(doc, 2), "def");
    assert!(doc.contains("abc</mark><a class=\"ref\" href=\"#t-2\">2</a><mark"));
}

#[test]
fn ranges_after_non_ascii_text_land_on_the_right_characters() {
    let md = "café 🦀 naïve **bold** 🦀 end\n";
    assert_eq!(marks(md, "bold"), "bold");
    assert_eq!(marks(md, "naïve"), "naïve");
    assert_eq!(marks(md, "🦀 end"), "🦀 end");
    let md = "# Crème brûlée\n\n- ünïcode 🦀 item\n";
    assert_eq!(marks(md, "item"), "item");
    assert_eq!(marks(md, "brûlée"), "brûlée");
}

#[test]
fn a_range_over_escaped_text_marks_at_least_the_escaped_text() {
    // The escapes make the rendered text differ from the source slice for the
    // backslash itself, so this is the approximate case: every character of the
    // rendered `*literal*` must be inside the mark.
    let md = "before \\*literal\\* after\n";
    let marked = marks(md, "\\*literal\\*");
    assert!(marked.contains("*literal*"), "{marked}");
    assert!(
        !marked.contains("before") && !marked.contains("after"),
        "{marked}"
    );
    // An entity is the same: the whole event is marked.
    assert_eq!(marks("fish &amp; chips\n", "&amp;"), "&");
}

#[test]
fn a_range_over_syntax_the_renderer_drops_gets_a_flagged_card_and_a_number() {
    let md = "# Title\n\nPara\n";
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(0, 1))]);
    assert_eq!(marked_text_in(doc_html(&html), 1), "");
    assert!(!doc_html(&html).contains("<mark"));
    // The number sits at the next rendered text, in the heading.
    assert!(doc_html(&html)
        .contains("<h1 id=\"md-title\"><a class=\"ref\" href=\"#t-1\">1</a>Title</h1>"));
    assert!(html.contains("id=\"t-1\""));
    assert!(html.contains("no rendered text"));

    let md = "para\n\n---\n\nafter\n";
    let (s, e) = at(md, "---");
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(s, e))]);
    assert!(!doc_html(&html).contains("<mark"));
    assert!(doc_html(&html).contains("<a class=\"ref\" href=\"#t-1\">1</a><hr>"));
    assert!(html.contains("no rendered text"));

    // A link destination has no rendered text either.
    let md = "[text](https://example.com/dest)\n";
    let html = rpage(md, &[on(md, "https://example.com/dest")]);
    assert!(!doc_html(&html).contains("<mark"));
    assert!(html.contains("no rendered text"));
    // A thread that does mark text is not flagged.
    let html = rpage(md, &[on(md, "text")]);
    assert!(!html.contains("no rendered text"));
}

#[test]
fn a_thread_after_the_last_rendered_text_still_gets_its_number() {
    let md = "text\n\n---\n";
    let (s, e) = at(md, "---");
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(s, e))]);
    assert_eq!(html.matches("class=\"ref\"").count(), 1);
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(md.len(), md.len()))]);
    assert_eq!(html.matches("class=\"ref\"").count(), 1);
}

#[test]
fn raw_html_in_the_document_is_shown_as_text() {
    let md = "<div onclick=\"x()\">hi</div>\n\ninline <b>bold</b> and </pre><script>alert(1)</script>\n\n<script>\nalert(2)\n</script>\n";
    let html = rpage(md, &[]);
    let doc = doc_html(&html);
    assert!(!doc.contains("<div"));
    assert!(!doc.contains("<b>"));
    assert!(!doc.contains("<script"));
    assert!(!doc.contains("</pre><script"));
    assert!(doc.contains("&lt;div onclick=&quot;x()&quot;&gt;"));
    assert!(doc.contains("&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(doc.contains("&lt;b&gt;bold&lt;/b&gt;"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("onclick=\"x"));
}

#[test]
fn a_range_over_raw_html_marks_its_escaped_text() {
    let md = "a <b>bold</b> c\n";
    assert_eq!(marks(md, "<b>bold</b>"), "<b>bold</b>");
    let html = rpage(md, &[on(md, "<b>")]);
    assert!(html.contains("&lt;b&gt;</mark>"));
}

#[test]
fn only_http_https_mailto_and_fragment_links_get_an_href() {
    let md = "[js](javascript:alert(1)) [rel](docs/x.md) [data](data:text/html,x) \
              [web](https://example.com/a) [plain](http://example.com) [mail](mailto:a@b.example) \
              [up](JAVASCRIPT:x) [frag](#some-heading) <https://auto.example/p> <me@auto.example>\n";
    let html = rpage(md, &[]);
    let doc = doc_html(&html);
    assert!(!doc.to_lowercase().contains("href=\"javascript"));
    assert!(!doc.contains("href=\"docs"));
    assert!(!doc.contains("href=\"data:"));
    assert!(doc.contains("<span class=\"nolink\" title=\"link: javascript:alert(1)\">js</span>"));
    assert!(doc.contains("<span class=\"nolink\" title=\"link: docs/x.md\">rel</span>"));
    assert!(doc.contains("<a href=\"https://example.com/a\" rel=\"noopener noreferrer\">web</a>"));
    assert!(doc.contains("<a href=\"http://example.com\" rel=\"noopener noreferrer\">plain</a>"));
    assert!(doc.contains("<a href=\"mailto:a@b.example\" rel=\"noopener noreferrer\">mail</a>"));
    assert!(doc.contains("<a href=\"#md-some-heading\">frag</a>"));
    assert!(doc.contains("<a href=\"https://auto.example/p\" rel=\"noopener noreferrer\">"));
    assert!(doc.contains("<a href=\"mailto:me@auto.example\" rel=\"noopener noreferrer\">"));
    // A quote in a destination cannot leave the attribute.
    let html = rpage("[x](https://example.com/\"onmouseover=\"alert(1))\n", &[]);
    assert!(!html.contains("\"onmouseover"));
}

#[test]
fn an_image_is_a_box_with_its_alt_text_and_loads_nothing() {
    let md = "An ![alt *text* here](https://tracker.example/p.png \"a title\") image\n";
    let html = rpage(md, &[on(md, "text")]);
    let doc = doc_html(&html);
    assert!(!html.contains("<img"));
    assert!(!html.contains("src="));
    assert!(doc.contains(
        "<span class=\"img\" title=\"image: https://tracker.example/p.png (a title)\">alt <em>"
    ));
    assert_eq!(marked_text_in(doc, 1), "text");
}

#[test]
fn the_rendered_page_loads_nothing_for_any_input() {
    let hostile = "<img src=x onerror=alert(1)>\n\n![a](https://x.example/a.png)\n\n\
                   <link rel=stylesheet href=//x.example/s.css>\n<iframe src=//x.example></iframe>\n\
                   <style>body{background:url(//x.example/b.png)}</style>\n\n[a](javascript:alert(1))\n\n\
                   `<script>` and <script src=//x.example/s.js></script>\n";
    for (md, word) in [(hostile, "alert"), (TEST_MD, "Test")] {
        let html = rpage(md, &[on(md, word)]);
        // Text such as `src=` or `url(` may appear escaped inside the hostile
        // document's own words, so the strict list applies to the example
        // document, and the markup forms to both.
        let strict: &[&str] = if md == TEST_MD {
            &["src=", "url(", "@import"]
        } else {
            &["src=\"", "src='"]
        };
        for banned in [
            "<img",
            "<script",
            "<iframe",
            "<link",
            "<style>body",
            "<input src",
        ]
        .iter()
        .chain(strict)
        {
            assert!(!html.contains(banned), "{banned}");
        }
        // The page's own single style element is the only one.
        assert_eq!(html.matches("<style").count(), 1);
        for attribute in ["src=\"", "action=\"", "data=\"http"] {
            assert!(!html.contains(attribute), "{attribute}");
        }
        // External links are allowed, always with rel.
        for part in html.split("href=\"http").skip(1) {
            let tag = &part[..part.find('>').unwrap()];
            assert!(tag.contains("rel=\"noopener noreferrer\""), "{tag}");
        }
    }
}

/// The values of every `id="..."` attribute.
fn ids(html: &str) -> Vec<String> {
    html.split(" id=\"")
        .skip(1)
        .map(|p| p[..p.find('"').unwrap()].to_string())
        .collect()
}

#[test]
fn heading_ids_and_fragment_links_agree_and_cannot_collide_with_cards() {
    let md = "# Hello World\n\n## Hello World\n\n## t-1\n\n### GPT-5.4 & Co.\n\n## Math / LaTeX\n\n\
              ## Ünïcode Ünderscore_ok\n\n[a](#hello-world) [b](#hello-world-1) [c](#t-1) [d](#math--latex)\n";
    let (s, e) = at(md, "Hello");
    let html = rpage(md, &[T::new("aaaaaaaa", anchored(s, e))]);
    let all = ids(&html);
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(all.len(), unique.len(), "duplicate ids: {all:?}");
    for want in [
        "md-hello-world",
        "md-hello-world-1",
        "md-t-1",
        "md-gpt-54--co",
        "md-math--latex",
        "md-ünïcode-ünderscore_ok",
        "t-1",
    ] {
        assert!(all.iter().any(|i| i == want), "{want} in {all:?}");
    }
    // The card keeps `t-1`; the heading titled "t-1" does not take it.
    assert_eq!(html.matches("id=\"t-1\"").count(), 1);
    for link in [
        "#md-hello-world\"",
        "#md-hello-world-1\"",
        "#md-t-1\"",
        "#md-math--latex\"",
    ] {
        assert!(html.contains(&format!("href=\"{link}")), "{link}");
    }
}

#[test]
fn every_fragment_link_in_the_example_document_has_a_target() {
    let html = rpage(TEST_MD, &[]);
    let all = ids(&html);
    let mut checked = 0;
    for part in html.split("href=\"#").skip(1) {
        let target = &part[..part.find('"').unwrap()];
        if target.is_empty() {
            continue;
        }
        assert!(target.starts_with("md-"), "{target}");
        assert!(all.iter().any(|i| i == target), "{target} has no heading");
        checked += 1;
    }
    assert!(checked >= 20, "{checked} links checked");
}

#[test]
fn a_table_renders_as_a_table_with_alignment() {
    let md = "| L | C | R |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n";
    let html = rpage(md, &[]);
    let doc = doc_html(&html);
    assert!(doc.contains("<div class=\"table-wrap\"><table>"));
    assert!(doc.contains("<thead><tr><th class=\"a-left\">L</th><th class=\"a-center\">C</th><th class=\"a-right\">R</th></tr></thead>"));
    assert!(doc.contains("<tbody>"));
    assert_eq!(doc.matches("<tr>").count(), 3);
    assert!(doc.contains("<td class=\"a-right\">6</td>"));
    assert!(doc.contains("</tbody></table></div>"));
}

#[test]
fn task_lists_show_disabled_checkboxes() {
    let md = "- [x] done\n- [ ] todo\n- plain\n";
    let html = rpage(md, &[on(md, "todo")]);
    let doc = doc_html(&html);
    assert!(doc.contains("<li class=\"task\"><input type=\"checkbox\" disabled checked> done</li>"));
    assert!(doc.contains("<input type=\"checkbox\" disabled> "));
    assert!(doc.contains("<li>plain</li>"));
    assert_eq!(marked_text_in(doc, 1), "todo");
}

#[test]
fn blocks_render_as_html() {
    let md = "# H1\n\n## H2\n\n###### H6\n\nA *em* **strong** ~~gone~~ `code`.\n\n> quote\n\n---\n\n\
              1. one\n2. two\n\nbreak\n\n5. five\n\n```\nplain\n```\n\nNote[^1]\n\n[^1]: The note.\n";
    let html = rpage(md, &[]);
    let doc = doc_html(&html);
    for want in [
        "<h1 id=\"md-h1\">H1</h1>",
        "<h2 id=\"md-h2\">H2</h2>",
        "<h6 id=\"md-h6\">H6</h6>",
        "<em>em</em>",
        "<strong>strong</strong>",
        "<s>gone</s>",
        "<code>code</code>",
        "<blockquote>",
        "<hr>",
        "<ol>",
        "<ol start=\"5\">",
        "<pre><code>plain\n</code></pre>",
        "<a href=\"#md-fn-1\">1</a>",
        "id=\"md-fn-1\"",
    ] {
        assert!(doc.contains(want), "{want}\n{doc}");
    }
    // Strikethrough must not use `del`, which the cards style as a deletion.
    assert!(!doc.contains("<del>"));
}

#[test]
fn the_rendered_page_keeps_the_cards_orphans_applied_and_summary() {
    let md = "# Title\n\nSome words here\n";
    let mut open = on(md, "words");
    open.replies = vec![reply("9a7b2c11", "Claude", "A reply.", true)];
    let mut resolved = on(md, "Some");
    resolved.state = "resolved";
    let mut accepted = on(md, "here");
    accepted.state = "accepted";
    accepted.suggestion = Some("there");
    accepted.quote = "here";
    let mut rejected = on(md, "Title");
    rejected.state = "rejected";
    let (s, e) = at(md, "Title");
    let mut moved = T::new("cccccccc", changed(s, e, "terms"));
    moved.quote = "terms";
    let mut lost = T::new("dddddddd", orphaned());
    lost.quote = "A line that went.";
    let deleted = T::new("eeeeeeee", json!({"status": "applied"}));
    let html = rpage(
        md,
        &[open, resolved, accepted, rejected, moved, lost, deleted],
    );
    assert!(html.contains("7 threads: 4 anchored, 1 changed, 1 orphaned, 1 applied"));
    assert!(html.contains("<aside class=\"cards\""));
    assert!(html.contains("<h2>Orphaned comments</h2>"));
    assert!(html.contains("<h2>Applied deletions</h2>"));
    assert!(html.contains("A line that went."));
    assert!(html.contains("<p class=\"was\">was <q>terms</q></p>"));
    assert!(html.contains("<del>here</del>"));
    assert!(html.contains("<ins>there</ins>"));
    assert!(html.contains("(agent)"));
    for class in [
        "s-open anchored",
        "s-resolved anchored",
        "s-accepted anchored",
        "s-rejected anchored",
        "s-open changed",
    ] {
        assert!(
            html.contains(&format!("<mark class=\"{class}\"")),
            "{class}"
        );
    }
    for n in 1..=5 {
        assert!(html.contains(&format!("<a class=\"ref\" href=\"#t-{n}\">{n}</a>")));
        assert!(html.contains("<article class=\"card s-"));
        assert!(html.contains(&format!("id=\"t-{n}\"")));
    }
    // No line-number gutter in the rendered view, and no source pre.
    assert!(!html.contains("class=\"gutter\""));
    assert!(!html.contains("<pre class=\"source\">"));
    assert!(html.contains("@media (max-width: 60rem)"));
    assert!(html.contains("prefers-color-scheme: dark"));
}

#[test]
fn the_rendered_page_handles_no_threads_and_no_text() {
    let html = rpage("# Title\n\nbody\n", &[]);
    assert!(html.contains("0 threads: 0 anchored, 0 changed, 0 orphaned"));
    assert!(html.contains("No comments"));
    assert!(html.contains("<h1 id=\"md-title\">Title</h1>"));
    assert!(!html.contains("<mark"));
    assert!(!html.contains("<article class=\"card"));
    let html = rpage("", &[]);
    assert!(html.contains("<article class=\"markdown-body\">\n</article>"));
    // A range past the end is clamped, not a panic.
    let html = rpage("abc\n", &[T::new("aaaaaaaa", anchored(2, 999))]);
    // The final newline is not rendered text, so only the `c` is marked.
    assert_eq!(marked_text_in(doc_html(&html), 1), "c");
}

#[test]
fn the_example_document_renders_with_threads_in_each_kind_of_block() {
    let title = at(TEST_MD, "Test");
    let cell = at(TEST_MD, "Avast ye scurvy dogs");
    let code = at(TEST_MD, "fibonacci(n - 1)");
    let item = at(TEST_MD, "Another nested");
    let threads: Vec<T> = [title, cell, code, item]
        .iter()
        .map(|(s, e)| T::new("aaaaaaaa", anchored(*s, *e)))
        .collect();
    let html = rpage(TEST_MD, &threads);
    let doc = doc_html(&html);
    assert_eq!(marked_text_in(doc, 1), "Test");
    assert_eq!(marked_text_in(doc, 2), "Avast ye scurvy dogs");
    assert_eq!(marked_text_in(doc, 3), "fibonacci(n - 1)");
    assert_eq!(marked_text_in(doc, 4), "Another nested");
    assert!(doc.contains("<table>"));
    assert!(doc.contains("<pre><code class=\"language-mermaid\">"));
    assert!(doc.contains("<input type=\"checkbox\" disabled checked>"));
    assert!(doc.contains("<h2 id=\"md-contents\">Contents</h2>"));
    assert!(doc.contains("<s>strikethrough</s>"));
    assert!(doc.contains("<hr>"));
}

#[test]
fn a_thread_at_many_offsets_of_the_example_document_does_not_panic() {
    // A short thread at every seventh offset, in one page: exercises every
    // construct's boundary handling, including multi-byte characters.
    let len = TEST_MD.chars().count();
    let threads: Vec<T> = (0..len)
        .step_by(7)
        .map(|s| T::new("aaaaaaaa", anchored(s, (s + 3).min(len))))
        .collect();
    let html = rpage(TEST_MD, &threads);
    assert!(html.contains("<mark"));
    assert!(!html.contains("<script"));
}

#[test]
fn the_source_view_is_not_the_rendered_view() {
    let md = "# Title\n\nbody\n";
    let html = page(md, &[]);
    assert!(html.contains("<pre class=\"source\"># Title\n\nbody\n</pre>"));
    assert!(!html.contains("markdown-body"));
}

fn sample_html(render: fn(&str, &[Value]) -> String) -> String {
    let text = TEST_MD;
    let mut open = on(text, "document for **Marq**");
    open.id = "4f0c2d1e";
    open.quote = "document for **Marq**";
    open.body = "Why not call it a viewer?\nIt also adds comments.";
    open.replies = vec![{
        let mut r = reply("9a7b2c11", "Claude", "The CLI renders it too.", true);
        r["replies"] = json!([reply("1b2c3d4e", "Jim", "Fair enough.", false)]);
        r
    }];

    let mut resolved = on(text, "sub-document");
    resolved.id = "2a3b4c5d";
    resolved.state = "resolved";
    resolved.quote = "sub-document";
    resolved.body = "Is the link right?";

    let mut accepted = on(text, "Avast ye scurvy dogs");
    accepted.id = "5e6f7a8b";
    accepted.state = "accepted";
    accepted.suggestion = Some("Ahoy me hearties");
    accepted.quote = "Avast ye scurvy dogs";
    accepted.body = "Friendlier.";
    accepted.author = "Claude";
    accepted.agent = true;

    let mut rejected = on(text, "Second item");
    rejected.id = "6c7d8e9f";
    rejected.state = "rejected";
    rejected.suggestion = Some("Item two");
    rejected.quote = "Second item";
    rejected.body = "Consistent naming.";

    let first = at(text, "Hoist the mainsail afore the parrot");
    let second = at(text, "afore the parrot pilfers");
    let mut overlap_a = T::new("7d8e9fa0", anchored(first.0, first.1));
    overlap_a.body = "Two threads share these words.";
    let mut overlap_b = T::new("7d8e9fa1", anchored(second.0, second.1));
    overlap_b.body = "The second overlaps the first.";

    let (s, e) = at(text, "fibonacci(n - 1)");
    let mut moved = T::new("8e9fa0b1", changed(s, e, "fib(n - 1)"));
    moved.quote = "fib(n - 1)";
    moved.body = "The function was renamed.";

    let (s, e) = at(text, "✨");
    let mut emoji = T::new("8e9fa0b2", anchored(s, e));
    emoji.body = "After non-ASCII text, bytes and code points differ.";

    let rule = text.find("\n---\n").unwrap() + 1;
    let (s, e) = at(text, &text[rule..rule + 3]);
    let mut hr = T::new("8e9fa0b3", anchored(s, e));
    hr.body = "A rule has no rendered text.";

    let mut lost = T::new("9fa0b1c2", orphaned());
    lost.quote = "A sentence that was deleted.";
    lost.body = "This line has gone.";
    lost.replies = vec![reply("a0b1c2d3", "Jim", "Do we still need it?", false)];

    let mut deleted = T::new("b1c2d3e4", json!({"status": "applied"}));
    deleted.quote = "removed words";
    deleted.suggestion = Some("");
    deleted.state = "accepted";
    deleted.body = "Cut it.";

    let values: Vec<Value> = [
        open, resolved, accepted, rejected, overlap_a, overlap_b, moved, emoji, hr, lost, deleted,
    ]
    .iter()
    .map(T::json)
    .collect();
    render(text, &values)
}

fn write_sample(name: &str, html: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    assert!(path.exists());
}

/// Writes the rendered view of `example-docs/test.md`, with threads of every
/// state and status, to `target/render-sample.html` so a person can open it.
#[test]
fn writes_a_rendered_sample_page() {
    let html = sample_html(render_page);
    assert!(html.contains("<table>"));
    write_sample("render-sample.html", &html);
}
