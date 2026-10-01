//! Tests for the static HTML page (T-10). Threads are hand-written in the shape
//! of design 6.2, so the page is tested without a git repository.

use marq_comments::render::render_page;
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

fn page(markdown: &str, threads: &[T]) -> String {
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
    let source = source_html(html);
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

/// Writes a realistic page to `target/render-sample.html` so a person can open
/// it. It asserts only that the file was written.
#[test]
fn writes_a_sample_page() {
    let text = "# Plan\n\nThe build uses esbuild for bundling.\nWe ship café menus and 🦀 stickers.\n\
                The count in settings is three.\nThis sentence was reworded later.\n\
                A very long line that goes on and on to check that the source pane scrolls sideways rather than pushing the page wider than the screen.\n";
    let at = |needle: &str| {
        let byte = text.find(needle).unwrap();
        let start = text[..byte].chars().count();
        (start, start + needle.chars().count())
    };
    let (s, e) = at("esbuild");
    let mut open = T::new("4f0c2d1e", anchored(s, e));
    open.quote = "esbuild";
    open.body = "Why not Vite?\nIt would need a plugin per target.";
    open.replies = vec![{
        let mut r = reply(
            "9a7b2c11",
            "Claude",
            "Obsidian plugins ship one CJS file.",
            true,
        );
        r["replies"] = json!([reply("1b2c3d4e", "Jim", "Fair enough.", false)]);
        r
    }];

    let (s, e) = at("café menus");
    let mut resolved = T::new("2a3b4c5d", anchored(s, e));
    resolved.state = "resolved";
    resolved.quote = "café menus";
    resolved.body = "Is this the right spelling?";

    let (s, e) = at("three");
    let mut accepted = T::new("5e6f7a8b", anchored(s, e));
    accepted.state = "accepted";
    accepted.suggestion = Some("five");
    accepted.quote = "three";
    accepted.body = "The count in settings is five.";
    accepted.author = "Claude";
    accepted.agent = true;

    let (s, e) = at("🦀 stickers");
    let mut rejected = T::new("6c7d8e9f", anchored(s, e));
    rejected.state = "rejected";
    rejected.suggestion = Some("crab stickers");
    rejected.quote = "🦀 stickers";
    rejected.body = "Emoji may not print.";

    let (s, e) = at("esbuild for");
    let mut overlap = T::new("7d8e9fa0", anchored(s + 3, e));
    overlap.quote = "build for";
    overlap.body = "An overlapping range.";

    let (s, e) = at("reworded");
    let mut moved = T::new("8e9fa0b1", changed(s, e, "written"));
    moved.quote = "written";
    moved.body = "Check the wording.";

    let mut lost = T::new("9fa0b1c2", orphaned());
    lost.quote = "A sentence that was deleted.";
    lost.body = "This line has gone.";
    lost.replies = vec![reply("a0b1c2d3", "Jim", "Do we still need it?", false)];

    let html = page(
        text,
        &[open, resolved, accepted, rejected, overlap, moved, lost],
    );
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("render-sample.html");
    std::fs::write(&path, html).unwrap();
    assert!(path.exists());
}
