//! The static HTML page behind `marq-comments render`. Task T-10, design 6.1.
//!
//! The page shows the markdown source as plain text with every resolved anchor
//! marked on it, so a person (or the acceptance report) can see what the
//! comments system decided. It reads only the `list --json` shape of design
//! 6.2, and treats every string in it as untrusted: nothing reaches the output
//! without passing through [`esc`].

use crate::text;
use serde_json::Value;
use std::fmt::Write;

/// Renders one markdown file and its threads as a single static HTML page.
///
/// `threads` are the objects `list --json` prints (design 6.2): each has
/// `annotation`, `state`, `anchor`, `stateChanges` and `replies`.
///
/// The page has no scripts and no external resources. A thread whose anchor has
/// no usable range (status `orphaned`, or an anchor object that is missing or
/// malformed) goes to the orphan section, so no thread is dropped.
pub fn render_page(markdown: &str, threads: &[Value]) -> String {
    let chars: Vec<char> = markdown.chars().collect();
    let views: Vec<View> = threads
        .iter()
        .enumerate()
        .map(|(index, thread)| View::new(index, thread, chars.len()))
        .collect();

    let anchored = views
        .iter()
        .filter(|v| v.status == Status::Anchored)
        .count();
    let changed = views.iter().filter(|v| v.status == Status::Changed).count();
    let orphaned = views.iter().filter(|v| v.range.is_none()).count();

    let mut placed: Vec<&View> = views.iter().filter(|v| v.range.is_some()).collect();
    // Cards follow the order of the text they point at, then creation time, so
    // the sidebar reads top to bottom like the source.
    placed.sort_by(|a, b| {
        (a.range, a.created.as_str(), a.index).cmp(&(b.range, b.created.as_str(), b.index))
    });

    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>marq-comments</title>\n<style>\n");
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n<header>\n<h1>marq-comments</h1>\n");
    let noun = if views.len() == 1 {
        "thread"
    } else {
        "threads"
    };
    let _ = writeln!(
        out,
        "<p class=\"summary\">{} {noun}: {anchored} anchored, {changed} changed, {orphaned} orphaned</p>",
        views.len(),
    );
    out.push_str("</header>\n<main>\n<div class=\"layout\">\n<section class=\"source-pane\" aria-label=\"Source\">\n");
    out.push_str("<div class=\"source-box\">\n");
    let gutter: Vec<String> = (1..=text::line_count(markdown))
        .map(|n| n.to_string())
        .collect();
    let _ = writeln!(
        out,
        "<pre class=\"gutter\" aria-hidden=\"true\">{}</pre>",
        gutter.join("\n")
    );
    out.push_str("<pre class=\"source\">");
    write_source(&mut out, &chars, &placed);
    out.push_str("</pre>\n</div>\n</section>\n");

    out.push_str("<aside class=\"cards\" aria-label=\"Comments\">\n");
    if views.is_empty() {
        out.push_str("<p class=\"none\">No comments</p>\n");
    } else if placed.is_empty() {
        out.push_str("<p class=\"none\">No comments on the current text</p>\n");
    }
    for view in &placed {
        write_card(&mut out, view, markdown);
    }
    out.push_str("</aside>\n</div>\n");

    if orphaned > 0 {
        out.push_str("<section class=\"orphans\">\n<h2>Orphaned comments</h2>\n");
        for view in views.iter().filter(|v| v.range.is_none()) {
            write_card(&mut out, view, markdown);
        }
        out.push_str("</section>\n");
    }
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Anchored,
    Changed,
    Orphaned,
}

/// What the page needs from one thread, read once and defensively.
struct View<'a> {
    index: usize,
    thread: &'a Value,
    status: Status,
    /// Code-point range, clamped to the text. `None` for an orphan.
    range: Option<(usize, usize)>,
    state: String,
    created: String,
}

impl<'a> View<'a> {
    fn new(index: usize, thread: &'a Value, len: usize) -> Self {
        let anchor = &thread["anchor"];
        let mut status = match anchor["status"].as_str() {
            Some("anchored") => Status::Anchored,
            Some("changed") => Status::Changed,
            _ => Status::Orphaned,
        };
        let range = if status == Status::Orphaned {
            None
        } else {
            match (anchor["start"].as_u64(), anchor["end"].as_u64()) {
                (Some(start), Some(end)) if start <= end => {
                    let start = (start as usize).min(len);
                    Some((start, (end as usize).min(len)))
                }
                // A range the page cannot place is reported as an orphan
                // rather than hidden.
                _ => {
                    status = Status::Orphaned;
                    None
                }
            }
        };
        View {
            index,
            thread,
            status,
            range,
            state: thread["state"].as_str().unwrap_or("open").to_string(),
            created: thread["annotation"]["created"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        }
    }

    fn short_id(&self) -> String {
        short_id(&self.thread["annotation"])
    }

    fn is_suggestion(&self) -> bool {
        is_suggestion(&self.thread["annotation"])
    }

    fn anchor_class(&self) -> &'static str {
        match self.status {
            Status::Anchored => "anchored",
            Status::Changed => "changed",
            Status::Orphaned => "orphaned",
        }
    }
}

fn short_id(annotation: &Value) -> String {
    let id = annotation["id"].as_str().unwrap_or("");
    id.strip_prefix("urn:uuid:")
        .unwrap_or(id)
        .chars()
        .take(8)
        .collect()
}

fn is_suggestion(annotation: &Value) -> bool {
    annotation["motivation"].as_str() == Some("editing")
}

/// A CSS-safe class suffix. State comes from input, so anything unexpected
/// falls back to `open` rather than reaching a class attribute.
fn state_class(state: &str) -> &'static str {
    match state {
        "resolved" => "resolved",
        "accepted" => "accepted",
        "rejected" => "rejected",
        _ => "open",
    }
}

/// Writes the source with each range marked.
///
/// The text is cut at every range boundary. Within a segment, each range that
/// covers it opens a `<mark>`, outermost (earliest start, then longest) first,
/// so overlapping ranges nest and adjacent ones sit side by side. Each range
/// also gets a small link to its card, placed before the marks of the segment
/// where it starts and outside them, so the text inside a mark is only source.
fn write_source(out: &mut String, chars: &[char], placed: &[&View]) {
    let mut ranges: Vec<(usize, usize, &View)> = placed
        .iter()
        .filter_map(|v| v.range.map(|(s, e)| (s, e, *v)))
        .collect();
    ranges.sort_by_key(|(s, e, v)| (*s, std::cmp::Reverse(*e), v.index));

    let mut cuts: Vec<usize> = vec![0, chars.len()];
    for (s, e, _) in &ranges {
        cuts.push(*s);
        cuts.push(*e);
    }
    cuts.sort_unstable();
    cuts.dedup();

    for (i, &at) in cuts.iter().enumerate() {
        // Empty ranges have no text to mark but still get their link.
        for (_, _, v) in ranges.iter().filter(|(s, _, _)| *s == at) {
            let _ = write!(out, "<a class=\"ref\" href=\"#t-{0}\">{0}</a>", v.index + 1);
        }
        let Some(&next) = cuts.get(i + 1) else { break };
        let covering: Vec<&(usize, usize, &View)> = ranges
            .iter()
            .filter(|(s, e, _)| *s <= at && *e >= next && e > s)
            .collect();
        for (_, _, v) in &covering {
            let _ = write!(
                out,
                "<mark class=\"s-{} {}\" data-thread=\"t-{}\" title=\"{}\">",
                state_class(&v.state),
                v.anchor_class(),
                v.index + 1,
                esc(&format!(
                    "{} {} ({})",
                    v.short_id(),
                    v.state,
                    v.anchor_class()
                )),
            );
        }
        let segment: String = chars[at..next].iter().collect();
        out.push_str(&esc(&segment));
        for _ in &covering {
            out.push_str("</mark>");
        }
    }
}

fn write_card(out: &mut String, view: &View, markdown: &str) {
    let annotation = &view.thread["annotation"];
    let anchor = &view.thread["anchor"];
    let kind = if view.is_suggestion() {
        "suggestion"
    } else {
        "comment"
    };
    let _ = writeln!(
        out,
        "<article class=\"card s-{} {}\" id=\"t-{}\">",
        state_class(&view.state),
        view.anchor_class(),
        view.index + 1
    );
    let _ = write!(
        out,
        "<header class=\"card-head\"><span class=\"num\">{}</span> <code class=\"id\">{}</code> \
         <span class=\"state s-{}\">{}</span> <span class=\"kind\">{}</span>",
        view.index + 1,
        esc(&view.short_id()),
        state_class(&view.state),
        esc(&view.state),
        kind
    );
    match view.range {
        Some((start, _)) => {
            // Recomputed from the offset, so the card agrees with the marked
            // text even if the stored line and column were absent.
            let (line, column) = text::line_col(markdown, start).unwrap_or((1, 1));
            let _ = write!(
                out,
                " <span class=\"loc\">{line}:{column}</span>{}",
                if view.status == Status::Changed {
                    " <span class=\"flag\">changed</span>"
                } else {
                    ""
                }
            );
        }
        None => out.push_str(" <span class=\"flag\">orphaned</span>"),
    }
    out.push_str("</header>\n");
    write_byline(out, annotation);

    let quote = quote_of(annotation);
    if view.status == Status::Changed {
        let original = anchor["original"].as_str().unwrap_or(&quote);
        let _ = writeln!(out, "<p class=\"was\">was <q>{}</q></p>", esc(original));
    }
    if view.status == Status::Orphaned {
        let _ = writeln!(
            out,
            "<blockquote class=\"quote\">{}</blockquote>",
            esc(&quote)
        );
    }
    write_body(out, annotation);
    write_replies(out, &view.thread["replies"]);
    out.push_str("</article>\n");
}

/// The stored `TextQuoteSelector` `exact`, whichever shape `selector` has
/// (the design stores an array; a single object is accepted too).
fn quote_of(annotation: &Value) -> String {
    let selector = &annotation["target"]["selector"];
    let found = match selector {
        Value::Array(items) => items
            .iter()
            .find(|s| s["type"].as_str() == Some("TextQuoteSelector")),
        Value::Object(_) => Some(selector),
        _ => None,
    };
    found
        .and_then(|s| s["exact"].as_str())
        .unwrap_or("")
        .to_string()
}

fn write_byline(out: &mut String, annotation: &Value) {
    let creator = &annotation["creator"];
    let name = creator["name"].as_str().unwrap_or("unknown");
    let agent = if creator["type"].as_str() == Some("Software") {
        " <span class=\"agent\">(agent)</span>"
    } else {
        ""
    };
    let created = annotation["created"].as_str().unwrap_or("");
    let _ = writeln!(
        out,
        "<p class=\"by\"><span class=\"author\">{}</span>{agent} <time>{}</time></p>",
        esc(name),
        esc(created)
    );
}

/// The comment text and, for a suggestion, the replaced and replacement text.
fn write_body(out: &mut String, annotation: &Value) {
    let body = &annotation["body"];
    let bodies: Vec<&Value> = match body {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![body],
        _ => Vec::new(),
    };
    if is_suggestion(annotation) {
        let replacement = bodies
            .iter()
            .find(|b| b["purpose"].as_str() == Some("editing"))
            .and_then(|b| b["value"].as_str())
            .unwrap_or("");
        let _ = writeln!(
            out,
            "<div class=\"edit\"><del>{}</del> <span class=\"arrow\">&rarr;</span> <ins>{}</ins></div>",
            esc(&quote_of(annotation)),
            esc(replacement)
        );
    }
    for b in bodies
        .iter()
        .filter(|b| b["purpose"].as_str() != Some("editing"))
    {
        if let Some(value) = b["value"].as_str() {
            let _ = writeln!(out, "<p class=\"body\">{}</p>", esc(value));
        }
    }
}

fn write_replies(out: &mut String, replies: &Value) {
    let Some(replies) = replies.as_array().filter(|r| !r.is_empty()) else {
        return;
    };
    out.push_str("<div class=\"replies\">\n");
    for reply in replies {
        let annotation = &reply["annotation"];
        let _ = writeln!(
            out,
            "<div class=\"reply\"><code class=\"id\">{}</code>",
            esc(&short_id(annotation))
        );
        write_byline(out, annotation);
        write_body(out, annotation);
        write_replies(out, &reply["replies"]);
        out.push_str("</div>\n");
    }
    out.push_str("</div>\n");
}

/// Escapes text for element content and for double-quoted attributes.
fn esc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#":root {
  color-scheme: light dark;
  --bg: #fbfbfa; --fg: #1f2328; --muted: #5d6670; --line: #d4d8dc;
  --panel: #ffffff; --gutter: #eef0f2;
  --open: rgba(255, 200, 0, 0.38); --open-edge: #b58100;
  --resolved: rgba(130, 140, 150, 0.22); --resolved-edge: #78818a;
  --accepted: rgba(40, 170, 80, 0.30); --accepted-edge: #1f8a47;
  --rejected: rgba(225, 70, 70, 0.26); --rejected-edge: #c0392b;
  --link: #0b5cad;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14171a; --fg: #e3e6e9; --muted: #98a2ac; --line: #343a40;
    --panel: #1b1f23; --gutter: #22272c;
    --open: rgba(255, 196, 0, 0.30); --open-edge: #e0b030;
    --resolved: rgba(150, 160, 170, 0.22); --resolved-edge: #8a949e;
    --accepted: rgba(60, 200, 100, 0.28); --accepted-edge: #4cc27a;
    --rejected: rgba(240, 90, 90, 0.30); --rejected-edge: #ee7a7a;
    --link: #6cb2ff;
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }
header, main { max-width: 90rem; margin: 0 auto; padding: 0 16px; }
h1 { font-size: 1.1rem; margin: 16px 0 4px; }
h2 { font-size: 1.05rem; margin: 24px 0 8px; }
.summary { margin: 0 0 16px; color: var(--muted); }
.layout { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: 16px; align-items: start; }
@media (max-width: 60rem) { .layout { grid-template-columns: minmax(0, 1fr); } }
.source-box { display: flex; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); overflow: hidden; }
pre { margin: 0; font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.gutter { padding: 8px 8px; text-align: right; color: var(--muted); background: var(--gutter);
  border-right: 1px solid var(--line); user-select: none; }
.source { flex: 1; min-width: 0; overflow-x: auto; padding: 8px 12px; white-space: pre; tab-size: 4; }
mark { color: inherit; border-radius: 2px; padding: 1px 0; }
mark.s-open { background: var(--open); box-shadow: inset 0 -2px 0 var(--open-edge); }
mark.s-resolved { background: var(--resolved); box-shadow: inset 0 -2px 0 var(--resolved-edge); }
mark.s-accepted { background: var(--accepted); box-shadow: inset 0 -2px 0 var(--accepted-edge); }
mark.s-rejected { background: var(--rejected); box-shadow: inset 0 -2px 0 var(--rejected-edge); }
mark.changed { box-shadow: none; text-decoration: underline dashed 2px; text-underline-offset: 3px; }
.ref { font: 600 10px/1 system-ui, sans-serif; vertical-align: super; color: var(--link);
  text-decoration: none; user-select: none; padding: 0 1px; }
.cards { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
.card { border: 1px solid var(--line); border-left: 4px solid var(--open-edge); border-radius: 6px;
  background: var(--panel); padding: 8px 12px; min-width: 0; overflow-wrap: anywhere; }
.card.s-resolved { border-left-color: var(--resolved-edge); }
.card.s-accepted { border-left-color: var(--accepted-edge); }
.card.s-rejected { border-left-color: var(--rejected-edge); }
.card.s-resolved .body { color: var(--muted); }
.card:target { outline: 2px solid var(--link); }
.card-head { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: baseline; }
.num { font-weight: 700; }
.id { font: 12px ui-monospace, Menlo, Consolas, monospace; color: var(--muted); }
.state { font-size: 12px; font-weight: 600; padding: 0 6px; border-radius: 9px; border: 1px solid var(--open-edge); }
.state.s-resolved { border-color: var(--resolved-edge); color: var(--muted); }
.state.s-accepted { border-color: var(--accepted-edge); }
.state.s-rejected { border-color: var(--rejected-edge); }
.kind, .loc, .flag { font-size: 12px; color: var(--muted); }
.flag { font-weight: 600; }
.by { margin: 2px 0 6px; color: var(--muted); font-size: 13px; }
.author { color: var(--fg); font-weight: 600; }
.agent { font-style: italic; }
.body { margin: 4px 0; white-space: pre-wrap; }
.was, .quote { margin: 4px 0; font-size: 13px; color: var(--muted); }
.quote { border-left: 3px solid var(--line); padding-left: 8px; margin-left: 0; white-space: pre-wrap; }
.edit { margin: 4px 0; font: 13px ui-monospace, Menlo, Consolas, monospace; white-space: pre-wrap; }
del { background: var(--rejected); text-decoration-thickness: 1px; }
ins { background: var(--accepted); text-decoration: none; }
.replies { margin-top: 8px; border-left: 2px solid var(--line); padding-left: 10px; }
.reply { margin-top: 6px; }
.none { color: var(--muted); }
.orphans .card { margin-bottom: 12px; max-width: 60rem; }
"#;
