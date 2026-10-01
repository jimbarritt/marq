//! The static HTML page behind `marq-comments render`. Task T-10, design 6.1.
//!
//! The page shows the markdown rendered, with every resolved anchor marked on
//! the rendered text ([`render_page`]), or as plain source with the same marks
//! ([`render_source_page`]). Either way a person (or the acceptance report) can
//! see what the comments system decided. It reads only the `list --json` shape
//! of design 6.2, and treats every string in it as untrusted: nothing reaches
//! the output without passing through [`esc`], and the document's own HTML is
//! never emitted as markup.

use crate::text;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt::Write;
use std::ops::Range;

/// Renders one markdown file and its threads as a single static HTML page, with
/// the markdown rendered (headings, lists, tables, code) and the comments marked
/// on the rendered text.
///
/// `threads` are the objects `list --json` prints (design 6.2): each has
/// `annotation`, `state`, `anchor`, `stateChanges` and `replies`.
///
/// The page has no scripts and no external resources. A thread whose anchor has
/// no usable range (status `orphaned`, or an anchor object that is missing or
/// malformed) goes to the orphan section, so no thread is dropped.
pub fn render_page(markdown: &str, threads: &[Value]) -> String {
    build_page(markdown, threads, Mode::Rendered)
}

/// The same page with the document shown as source text in a `<pre>`, with a
/// line-number gutter, so the marks sit on the exact characters the anchors
/// count.
pub fn render_source_page(markdown: &str, threads: &[Value]) -> String {
    build_page(markdown, threads, Mode::Source)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Rendered,
    Source,
}

fn build_page(markdown: &str, threads: &[Value], mode: Mode) -> String {
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
    let applied = views.iter().filter(|v| v.status == Status::Applied).count();
    let orphaned = views.iter().filter(|v| v.range.is_none()).count() - applied;

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
    if mode == Mode::Rendered {
        out.push_str(MARKDOWN_STYLE);
    }
    out.push_str("</style>\n</head>\n<body>\n<header>\n<h1>marq-comments</h1>\n");
    let noun = if views.len() == 1 {
        "thread"
    } else {
        "threads"
    };
    let _ = writeln!(
        out,
        "<p class=\"summary\">{} {noun}: {anchored} anchored, {changed} changed, {orphaned} orphaned{}</p>",
        views.len(),
        if applied > 0 {
            format!(", {applied} applied")
        } else {
            String::new()
        },
    );
    out.push_str("</header>\n<main>\n<div class=\"layout\">\n");
    let mut shown: HashSet<usize> = HashSet::new();
    match mode {
        Mode::Source => {
            out.push_str("<section class=\"source-pane\" aria-label=\"Source\">\n");
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
        }
        Mode::Rendered => {
            out.push_str("<section class=\"doc-pane\" aria-label=\"Document\">\n");
            out.push_str("<div class=\"doc-box\">\n<article class=\"markdown-body\">\n");
            shown = write_rendered(&mut out, markdown, &chars, &placed);
            out.push_str("</article>\n</div>\n</section>\n");
        }
    }

    out.push_str("<aside class=\"cards\" aria-label=\"Comments\">\n");
    if views.is_empty() {
        out.push_str("<p class=\"none\">No comments</p>\n");
    } else if placed.is_empty() {
        out.push_str("<p class=\"none\">No comments on the current text</p>\n");
    }
    for view in &placed {
        let unseen = mode == Mode::Rendered && !shown.contains(&view.index);
        write_card(&mut out, view, markdown, unseen);
    }
    out.push_str("</aside>\n</div>\n");

    if orphaned > 0 {
        out.push_str("<section class=\"orphans\">\n<h2>Orphaned comments</h2>\n");
        for view in views.iter().filter(|v| v.status == Status::Orphaned) {
            write_card(&mut out, view, markdown, false);
        }
        out.push_str("</section>\n");
    }
    if applied > 0 {
        out.push_str("<section class=\"orphans\">\n<h2>Applied deletions</h2>\n");
        for view in views.iter().filter(|v| v.status == Status::Applied) {
            write_card(&mut out, view, markdown, false);
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
    /// An accepted suggestion with no text left to point at: a deletion.
    Applied,
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
            Some("applied") => Status::Applied,
            _ => Status::Orphaned,
        };
        let range = if matches!(status, Status::Orphaned | Status::Applied) {
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
            Status::Applied => "applied",
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
            out.push_str(&mark_open(v));
        }
        let segment: String = chars[at..next].iter().collect();
        out.push_str(&esc(&segment));
        for _ in &covering {
            out.push_str("</mark>");
        }
    }
}

/// `unseen` is true for a thread that has a range but no rendered text to mark
/// (the range covers only syntax the renderer drops), so the card says why the
/// page shows no highlight for it.
fn write_card(out: &mut String, view: &View, markdown: &str, unseen: bool) {
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
            if unseen {
                out.push_str(" <span class=\"flag\">no rendered text</span>");
            }
        }
        None if view.status == Status::Applied => {
            out.push_str(" <span class=\"flag\">applied</span>")
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

// ---- the rendered view ----

/// One thread's range on the markdown, in bytes, with the opening tag of its
/// mark prepared.
struct Span {
    start: usize,
    end: usize,
    number: usize,
    index: usize,
    open: String,
}

/// Writes the markdown as HTML with each thread's range marked, and returns the
/// indexes of the threads that got at least one mark.
///
/// Anchors count code points in the source, and pulldown-cmark reports each
/// event's byte range in the source, so the ranges are converted once and then
/// compared with event ranges directly.
///
/// The mapping from source to rendered text is exact where the rendered text is
/// the source slice: plain text, the lines of a code block, and the content of a
/// code span. It is approximate where the two differ (a backslash escape, an
/// entity, a code span whose line breaks became spaces, text in a block quote
/// whose `>` prefixes were dropped): the whole event is marked when any range
/// touches it. Syntax the renderer drops (`#`, `---`, link destinations, table
/// rules) has no text to mark, so a range over only such syntax marks nothing.
fn write_rendered(
    out: &mut String,
    markdown: &str,
    chars: &[char],
    placed: &[&View],
) -> HashSet<usize> {
    let mut offsets: Vec<usize> = markdown.char_indices().map(|(i, _)| i).collect();
    offsets.push(markdown.len());
    debug_assert_eq!(offsets.len(), chars.len() + 1);

    let mut spans: Vec<Span> = placed
        .iter()
        .filter_map(|v| {
            let (s, e) = v.range?;
            Some(Span {
                start: offsets[s],
                end: offsets[e],
                number: v.index + 1,
                index: v.index,
                open: mark_open(v),
            })
        })
        .collect();
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end), s.index));

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    let events: Vec<(Event, Range<usize>)> = Parser::new_ext(markdown, options)
        .into_offset_iter()
        .collect();

    let mut doc = Doc {
        md: markdown,
        out,
        spans,
        next_ref: 0,
        marked: HashSet::new(),
    };
    doc.run(&events);
    doc.flush_refs(usize::MAX);
    doc.marked
}

struct Doc<'a> {
    md: &'a str,
    out: &'a mut String,
    /// Sorted by start, then longest first, then thread order.
    spans: Vec<Span>,
    /// Spans before this index have had their numbered link written.
    next_ref: usize,
    marked: HashSet<usize>,
}

impl Doc<'_> {
    fn run(&mut self, events: &[(Event, Range<usize>)]) {
        let slugs = heading_ids(events);
        let mut heading = 0;
        let mut links: Vec<&'static str> = Vec::new();
        let mut aligns: Vec<Alignment> = Vec::new();
        let mut in_head = false;
        let mut column = 0;

        for (i, (event, range)) in events.iter().enumerate() {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => self.out.push_str("<p>"),
                    Tag::Heading { level, .. } => {
                        let id = slugs.get(heading).cloned().unwrap_or_default();
                        heading += 1;
                        let _ = write!(self.out, "<h{} id=\"{}\">", *level as usize, esc(&id));
                    }
                    Tag::BlockQuote(_) => self.out.push_str("<blockquote>\n"),
                    Tag::CodeBlock(kind) => {
                        self.out.push_str("<pre><code");
                        if let CodeBlockKind::Fenced(info) = kind {
                            let lang: String = info
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .chars()
                                .filter(|c| c.is_ascii_alphanumeric() || "_+-#.".contains(*c))
                                .collect();
                            if !lang.is_empty() {
                                let _ = write!(self.out, " class=\"language-{lang}\"");
                            }
                        }
                        self.out.push('>');
                    }
                    // The document's own HTML is shown as text, never as markup.
                    Tag::HtmlBlock => self.out.push_str("<pre class=\"raw-html\">"),
                    Tag::List(None) => self.out.push_str("<ul>\n"),
                    Tag::List(Some(1)) => self.out.push_str("<ol>\n"),
                    Tag::List(Some(n)) => {
                        let _ = writeln!(self.out, "<ol start=\"{n}\">");
                    }
                    Tag::Item => {
                        let task = matches!(events.get(i + 1), Some((Event::TaskListMarker(_), _)));
                        self.out
                            .push_str(if task { "<li class=\"task\">" } else { "<li>" });
                    }
                    Tag::FootnoteDefinition(label) => {
                        let _ = write!(
                            self.out,
                            "<div class=\"footnote\" id=\"md-fn-{}\"><sup>{}</sup> ",
                            esc(&slug(label)),
                            esc(label)
                        );
                    }
                    Tag::Table(a) => {
                        aligns = a.clone();
                        self.out.push_str("<div class=\"table-wrap\"><table>\n");
                    }
                    Tag::TableHead => {
                        in_head = true;
                        column = 0;
                        self.out.push_str("<thead><tr>");
                    }
                    Tag::TableRow => {
                        column = 0;
                        self.out.push_str("<tr>");
                    }
                    Tag::TableCell => {
                        let class = match aligns.get(column) {
                            Some(Alignment::Left) => " class=\"a-left\"",
                            Some(Alignment::Center) => " class=\"a-center\"",
                            Some(Alignment::Right) => " class=\"a-right\"",
                            _ => "",
                        };
                        column += 1;
                        let _ = write!(self.out, "<{}{class}>", if in_head { "th" } else { "td" });
                    }
                    Tag::Emphasis => self.out.push_str("<em>"),
                    Tag::Strong => self.out.push_str("<strong>"),
                    // `<s>`, not `<del>`: the page already styles `del` as a
                    // rejected deletion in a suggestion card.
                    Tag::Strikethrough => self.out.push_str("<s>"),
                    Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        ..
                    } => links.push(self.open_link(*link_type, dest_url, title)),
                    Tag::Image {
                        dest_url, title, ..
                    } => {
                        // Never an `<img>`: the page must load nothing. The box
                        // shows the alt text; the source is in the tooltip.
                        let mut tip = format!("image: {dest_url}");
                        if !title.is_empty() {
                            let _ = write!(tip, " ({title})");
                        }
                        let _ = write!(self.out, "<span class=\"img\" title=\"{}\">", esc(&tip));
                    }
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Paragraph => self.out.push_str("</p>\n"),
                    TagEnd::Heading(level) => {
                        let _ = writeln!(self.out, "</h{}>", *level as usize);
                    }
                    TagEnd::BlockQuote(_) => self.out.push_str("</blockquote>\n"),
                    TagEnd::CodeBlock => self.out.push_str("</code></pre>\n"),
                    TagEnd::HtmlBlock => self.out.push_str("</pre>\n"),
                    TagEnd::List(true) => self.out.push_str("</ol>\n"),
                    TagEnd::List(false) => self.out.push_str("</ul>\n"),
                    TagEnd::Item => self.out.push_str("</li>\n"),
                    TagEnd::FootnoteDefinition => self.out.push_str("</div>\n"),
                    TagEnd::Table => self.out.push_str("</tbody></table></div>\n"),
                    TagEnd::TableHead => {
                        in_head = false;
                        self.out.push_str("</tr></thead>\n<tbody>\n");
                    }
                    TagEnd::TableRow => self.out.push_str("</tr>\n"),
                    TagEnd::TableCell => {
                        self.out.push_str(if in_head { "</th>" } else { "</td>" });
                    }
                    TagEnd::Emphasis => self.out.push_str("</em>"),
                    TagEnd::Strong => self.out.push_str("</strong>"),
                    TagEnd::Strikethrough => self.out.push_str("</s>"),
                    TagEnd::Link => self.out.push_str(links.pop().unwrap_or("</span>")),
                    TagEnd::Image => self.out.push_str("</span>"),
                    _ => {}
                },
                Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                    self.text(text, range);
                }
                Event::Code(code) => self.code(code, range),
                Event::SoftBreak => self.text("\n", range),
                Event::HardBreak => self.atom("<br>\n", range),
                Event::Rule => self.atom("<hr>\n", range),
                Event::TaskListMarker(done) => self.atom(
                    if *done {
                        "<input type=\"checkbox\" disabled checked> "
                    } else {
                        "<input type=\"checkbox\" disabled> "
                    },
                    range,
                ),
                Event::FootnoteReference(label) => {
                    let html = format!(
                        "<sup class=\"fnref\"><a href=\"#md-fn-{}\">{}</a></sup>",
                        esc(&slug(label)),
                        esc(label)
                    );
                    self.atom(&html, range);
                }
                _ => {}
            }
        }
    }

    /// Opens a link. Only `http:`, `https:`, `mailto:` and `#fragment`
    /// destinations become an `href`; anything else (`javascript:`, `data:`, a
    /// relative path) is shown as text with the destination in a tooltip, since
    /// the page is a static file and a relative path would point nowhere.
    /// Returns the closing tag.
    fn open_link(&mut self, kind: LinkType, dest: &str, title: &str) -> &'static str {
        let dest = if kind == LinkType::Email && !dest.to_ascii_lowercase().starts_with("mailto:") {
            format!("mailto:{dest}")
        } else {
            dest.to_string()
        };
        let lower = dest.to_ascii_lowercase();
        let tip = if title.is_empty() {
            String::new()
        } else {
            format!(" title=\"{}\"", esc(title))
        };
        if let Some(fragment) = dest.strip_prefix('#') {
            // Heading ids carry a prefix so they cannot meet the `t-N` ids of
            // the cards; a fragment link gets the same prefix.
            let target = if fragment.is_empty() {
                "#".to_string()
            } else {
                format!("#md-{fragment}")
            };
            let _ = write!(self.out, "<a href=\"{}\"{tip}>", esc(&target));
            "</a>"
        } else if ["http://", "https://", "mailto:"]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            let _ = write!(
                self.out,
                "<a href=\"{}\" rel=\"noopener noreferrer\"{tip}>",
                esc(&dest)
            );
            "</a>"
        } else {
            let _ = write!(
                self.out,
                "<span class=\"nolink\" title=\"{}\">",
                esc(&format!("link: {dest}"))
            );
            "</span>"
        }
    }

    /// Writes the numbered link of every thread that starts at or before `upto`
    /// and has none yet. The link sits before the marks of the text it starts
    /// in, outside them, so the text inside a mark is only document text. A
    /// thread that starts in syntax with no rendered text gets its number at
    /// the next rendered position.
    fn flush_refs(&mut self, upto: usize) {
        while let Some(span) = self.spans.get(self.next_ref) {
            if span.start > upto {
                break;
            }
            let _ = write!(
                self.out,
                "<a class=\"ref\" href=\"#t-{0}\">{0}</a>",
                span.number
            );
            self.next_ref += 1;
        }
    }

    /// A piece of rendered text with no source text of its own to mark.
    fn atom(&mut self, html: &str, range: &Range<usize>) {
        self.flush_refs(range.end.saturating_sub(1).max(range.start));
        self.out.push_str(html);
    }

    fn text(&mut self, text: &str, range: &Range<usize>) {
        if self.md.get(range.clone()) == Some(text) {
            self.exact(text, range.start);
        } else {
            self.whole(text, range);
        }
    }

    /// Inline code. The rendered text is the source without its backtick
    /// fences, and without one space at each end when both ends have one.
    fn code(&mut self, code: &str, range: &Range<usize>) {
        let slice = self.md.get(range.clone()).unwrap_or("");
        let fence = slice.bytes().take_while(|b| *b == b'`').count();
        let inner = slice.get(fence..slice.len().saturating_sub(fence));
        let base = match inner {
            Some(i) if i == code => Some(range.start + fence),
            Some(i)
                if i.len() >= 2
                    && i.strip_prefix(' ').and_then(|r| r.strip_suffix(' ')) == Some(code) =>
            {
                Some(range.start + fence + 1)
            }
            _ => None,
        };
        self.flush_refs(base.unwrap_or(range.start));
        self.out.push_str("<code>");
        match base {
            Some(base) => self.exact(code, base),
            None => self.whole(code, range),
        }
        self.out.push_str("</code>");
    }

    /// `text` is the source at `base..base + text.len()`. The text is cut at
    /// every range boundary inside it, and each piece is wrapped in one `<mark>`
    /// per covering range, outermost (earliest start, then longest) first, so
    /// overlapping ranges nest and adjacent ones sit side by side.
    fn exact(&mut self, text: &str, base: usize) {
        let end = base + text.len();
        let mut cuts = vec![base, end];
        for s in &self.spans {
            for p in [s.start, s.end] {
                if p > base && p < end && text.is_char_boundary(p - base) {
                    cuts.push(p);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            self.flush_refs(a);
            let covering: Vec<usize> = (0..self.spans.len())
                .filter(|&i| {
                    let s = &self.spans[i];
                    s.start <= a && s.end >= b && s.end > s.start
                })
                .collect();
            self.marked_run(&text[a - base..b - base], &covering);
        }
    }

    /// The approximation: rendered text that is not the source slice cannot be
    /// split at a source boundary, so the whole event is marked for every range
    /// that overlaps it.
    fn whole(&mut self, text: &str, range: &Range<usize>) {
        self.flush_refs(range.end.saturating_sub(1).max(range.start));
        let covering: Vec<usize> = (0..self.spans.len())
            .filter(|&i| {
                let s = &self.spans[i];
                s.start < range.end && s.end > range.start && s.end > s.start
            })
            .collect();
        self.marked_run(text, &covering);
    }

    fn marked_run(&mut self, text: &str, covering: &[usize]) {
        for &i in covering {
            self.out.push_str(&self.spans[i].open);
            self.marked.insert(self.spans[i].index);
        }
        self.out.push_str(&esc(text));
        for _ in covering {
            self.out.push_str("</mark>");
        }
    }
}

/// The id of each heading, in document order, GitHub style: lower case, spaces
/// to hyphens, punctuation dropped, a numeric suffix on a repeat. The `md-`
/// prefix keeps every id apart from the cards' `t-N` ids and from each other
/// page id.
fn heading_ids(events: &[(Event, Range<usize>)]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Heading { .. }) = &events[i].0 {
            let mut text = String::new();
            let mut image = 0;
            i += 1;
            while i < events.len() {
                match &events[i].0 {
                    Event::End(TagEnd::Heading(_)) => break,
                    Event::Start(Tag::Image { .. }) => image += 1,
                    Event::End(TagEnd::Image) => image -= 1,
                    Event::Text(t) | Event::Code(t) if image == 0 => text.push_str(t),
                    _ => {}
                }
                i += 1;
            }
            let mut base = slug(&text);
            if base.is_empty() {
                base = "section".to_string();
            }
            let mut id = base.clone();
            let mut n = 0;
            while !used.insert(id.clone()) {
                n += 1;
                id = format!("{base}-{n}");
            }
            ids.push(format!("md-{id}"));
        }
        i += 1;
    }
    ids
}

fn slug(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| match c {
            ' ' | '-' => Some('-'),
            '_' => Some('_'),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// The opening `<mark>` tag of a thread.
fn mark_open(v: &View) -> String {
    format!(
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
    )
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
header h1 { font-size: 1.1rem; margin: 16px 0 4px; }
.orphans h2 { font-size: 1.05rem; margin: 24px 0 8px; }
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

/// Added after [`STYLE`] on the rendered page: GitHub-like markdown, with its
/// own colour variables and a dark variant.
const MARKDOWN_STYLE: &str = r#":root {
  --md-code: rgba(130, 140, 150, 0.22); --md-block: #f3f5f7; --md-zebra: #f6f8fa;
  --md-rule: #d1d9e0; --md-quote: #59636e;
}
@media (prefers-color-scheme: dark) {
  :root {
    --md-code: rgba(150, 160, 170, 0.25); --md-block: #1f2429; --md-zebra: #1d2227;
    --md-rule: #3d444d; --md-quote: #9aa4ae;
  }
}
.doc-box { min-width: 0; border: 1px solid var(--line); border-radius: 6px; background: var(--panel);
  padding: 8px 32px 24px; }
.markdown-body { max-width: 52rem; margin: 0 auto; font: 16px/1.6 -apple-system, BlinkMacSystemFont,
  "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; overflow-wrap: break-word; }
.markdown-body > :first-child { margin-top: 16px; }
.markdown-body h1, .markdown-body h2, .markdown-body h3, .markdown-body h4,
.markdown-body h5, .markdown-body h6 { margin: 24px 0 16px; font-weight: 600; line-height: 1.25; }
.markdown-body h1 { font-size: 2em; padding-bottom: .3em; border-bottom: 1px solid var(--md-rule); }
.markdown-body h2 { font-size: 1.5em; padding-bottom: .3em; border-bottom: 1px solid var(--md-rule); }
.markdown-body h3 { font-size: 1.25em; }
.markdown-body h4 { font-size: 1em; }
.markdown-body h5 { font-size: .875em; }
.markdown-body h6 { font-size: .85em; color: var(--muted); }
.markdown-body p, .markdown-body ul, .markdown-body ol, .markdown-body blockquote,
.markdown-body pre, .markdown-body .table-wrap { margin: 0 0 16px; }
.markdown-body ul, .markdown-body ol { padding-left: 2em; }
.markdown-body li > ul, .markdown-body li > ol { margin: 0; }
.markdown-body li + li { margin-top: .25em; }
.markdown-body li > p { margin: 16px 0 0; }
.markdown-body li.task { list-style: none; }
.markdown-body li.task input { margin: 0 .4em .25em -1.4em; vertical-align: middle; }
.markdown-body a:not(.ref) { color: var(--link); text-decoration: none; }
.markdown-body a:not(.ref):hover { text-decoration: underline; }
.markdown-body .nolink { text-decoration: underline dotted; text-underline-offset: 3px; cursor: help; }
.markdown-body blockquote { padding: 0 1em; color: var(--md-quote); border-left: .25em solid var(--md-rule); }
.markdown-body blockquote > :last-child { margin-bottom: 0; }
.markdown-body hr { height: .25em; padding: 0; margin: 24px 0; background: var(--md-rule); border: 0; }
.markdown-body code { font: 85% ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  padding: .2em .4em; background: var(--md-code); border-radius: 6px; }
.markdown-body pre { padding: 16px; overflow: auto; background: var(--md-block); border-radius: 6px;
  font: 85%/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; white-space: pre; tab-size: 4; }
.markdown-body pre code { padding: 0; background: none; font: inherit; border-radius: 0; }
.markdown-body pre.raw-html { color: var(--md-quote); }
.markdown-body .table-wrap { overflow-x: auto; }
.markdown-body table { border-collapse: collapse; width: 100%; }
.markdown-body th, .markdown-body td { padding: 6px 13px; border: 1px solid var(--md-rule); vertical-align: top; text-align: left; }
.markdown-body th { font-weight: 600; background: var(--md-block); }
.markdown-body tbody tr:nth-child(even) { background: var(--md-zebra); }
.markdown-body .a-center { text-align: center; }
.markdown-body .a-right { text-align: right; }
.markdown-body .img { display: inline-block; padding: 0 .5em; border: 1px dashed var(--md-rule);
  border-radius: 6px; background: var(--md-code); color: var(--muted); font-size: .9em; }
.markdown-body .img::before { content: "\1F5BC\FE0E\A0"; }
.markdown-body .footnote { font-size: .875em; color: var(--md-quote); margin: 0 0 8px; }
.markdown-body :target { background: var(--open); }
@media (max-width: 40rem) {
  .doc-box { padding: 4px 14px 16px; }
  .markdown-body { font-size: 15px; }
  .markdown-body h1 { font-size: 1.7em; }
  .markdown-body th, .markdown-body td { padding: 4px 8px; }
}
"#;
