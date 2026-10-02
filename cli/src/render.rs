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
use std::collections::{HashMap, HashSet};
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
    // the list reads top to bottom like the source.
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
    let body_class = if mode == Mode::Rendered {
        "rendered-view"
    } else {
        "source-view"
    };
    let _ = write!(
        out,
        "</style>\n</head>\n<body class=\"{body_class}\">\n<header>\n<h1>marq-comments</h1>\n"
    );
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
    out.push_str("</header>\n<main>\n");
    match mode {
        Mode::Source => {
            out.push_str("<div class=\"layout\">\n");
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
            out.push_str("<aside class=\"cards\" aria-label=\"Comments\">\n");
            if views.is_empty() {
                out.push_str("<p class=\"none\">No comments</p>\n");
            } else if placed.is_empty() {
                out.push_str("<p class=\"none\">No comments on the current text</p>\n");
            }
            for view in &placed {
                write_card(&mut out, view, markdown, false);
            }
            out.push_str("</aside>\n</div>\n");
        }
        Mode::Rendered => {
            if views.is_empty() {
                out.push_str("<p class=\"none\">No comments</p>\n");
            } else if placed.is_empty() {
                out.push_str("<p class=\"none\">No comments on the current text</p>\n");
            }
            let doc = write_rendered(markdown, &chars, &placed);
            // Each card goes in the margin (the strip under the block) of the
            // block that holds its first mark. `placed` is already in position then creation order.
            let mut margins: Vec<Vec<&View>> = vec![Vec::new(); doc.blocks.len()];
            let mut loose: Vec<&View> = Vec::new();
            for view in &placed {
                match doc.owner.get(&view.index) {
                    Some(&block) => margins[block].push(view),
                    None => loose.push(view),
                }
            }
            out.push_str("<div class=\"rendered\">\n");
            for (block, cards) in doc.blocks.iter().zip(&margins) {
                write_row(
                    &mut out,
                    block.class,
                    &block.html,
                    cards,
                    markdown,
                    &doc.marked,
                );
            }
            // A thread with a range but no block to sit under (an empty
            // document) still gets its card.
            if !loose.is_empty() {
                write_row(&mut out, "", "", &loose, markdown, &doc.marked);
            }
            out.push_str("</div>\n");
        }
    }

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

/// One row of the rendered page: a block of the document and, directly under
/// it in the same column, the cards of the threads that start in it.
fn write_row(
    out: &mut String,
    class: &str,
    html: &str,
    cards: &[&View],
    markdown: &str,
    marked: &HashSet<usize>,
) {
    let _ = write!(out, "<section class=\"blk{class}\">");
    let _ = write!(out, "<div class=\"doc markdown-body\">\n{html}</div>");
    out.push_str("<aside class=\"margin\">");
    if !cards.is_empty() {
        out.push('\n');
    }
    for view in cards {
        write_card(out, view, markdown, !marked.contains(&view.index));
    }
    out.push_str("</aside></section>\n");
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
/// also gets a small grey number linking to its card, placed after the end of
/// its text and outside every mark, so the text inside a mark is only source.
/// A range that ends inside a word gets its number after the word, so the
/// number never splits it.
fn write_source(out: &mut String, chars: &[char], placed: &[&View]) {
    let mut ranges: Vec<(usize, usize, &View)> = placed
        .iter()
        .filter_map(|v| v.range.map(|(s, e)| (s, e, *v)))
        .collect();
    ranges.sort_by_key(|(s, e, v)| (*s, std::cmp::Reverse(*e), v.index));

    // Where each range's number goes. An empty range has no text to follow, so
    // its number sits at its own position.
    let ref_at: Vec<usize> = ranges
        .iter()
        .map(|(s, e, _)| {
            let mut at = *e;
            if e > s {
                while chars.get(at).is_some_and(|c| c.is_alphanumeric()) {
                    at += 1;
                }
            }
            at
        })
        .collect();

    let mut cuts: Vec<usize> = vec![0, chars.len()];
    for ((s, e, _), r) in ranges.iter().zip(&ref_at) {
        cuts.extend([*s, *e, *r]);
    }
    cuts.sort_unstable();
    cuts.dedup();

    for (i, &at) in cuts.iter().enumerate() {
        for ((_, _, v), _) in ranges.iter().zip(&ref_at).filter(|(_, r)| **r == at) {
            out.push_str(&ref_link(v.index + 1));
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

/// The small grey number that links a thread's text to its card.
fn ref_link(number: usize) -> String {
    format!("<a class=\"ref\" href=\"#t-{number}\">{number}</a>")
}

/// `unseen` is true for a thread that has a range but no rendered text to mark
/// (the range covers only syntax the renderer drops), so the card says why the
/// page shows no highlight for it.
fn write_card(out: &mut String, view: &View, markdown: &str, unseen: bool) {
    let annotation = &view.thread["annotation"];
    let anchor = &view.thread["anchor"];
    // Only a resolved thread is dimmed; the rest look alike, so an open thread
    // stands out without a colour of its own.
    let dim = if state_class(&view.state) == "resolved" {
        " dim"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "<article class=\"card{dim}\" id=\"t-{}\" data-state=\"{}\" data-status=\"{}\">",
        view.index + 1,
        state_class(&view.state),
        view.anchor_class(),
    );
    let _ = write!(
        out,
        "<header class=\"card-head\"><span class=\"num\">{}</span>",
        view.index + 1,
    );
    if state_class(&view.state) != "open" {
        let _ = write!(out, " <span class=\"state\">{}</span>", esc(&view.state));
    }
    if view.is_suggestion() {
        out.push_str(" <span class=\"kind\">suggestion</span>");
    }
    match view.range {
        Some((start, _)) => {
            // Recomputed from the offset, so the card agrees with the marked
            // text even if the stored line and column were absent.
            let (line, column) = text::line_col(markdown, start).unwrap_or((1, 1));
            let _ = write!(out, " <span class=\"loc\">{line}:{column}</span>");
            if view.status == Status::Changed {
                out.push_str(" <span class=\"flag\">changed</span>");
            }
            if unseen {
                out.push_str(" <span class=\"flag\">no rendered text</span>");
            }
        }
        None if view.status == Status::Applied => {
            out.push_str(" <span class=\"flag\">applied</span>")
        }
        None => out.push_str(" <span class=\"flag\">orphaned</span>"),
    }
    let _ = write!(out, " <code class=\"id\">{}</code>", esc(&view.short_id()));
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
            let _ = writeln!(out, "<div class=\"body\">{}</div>", comment_html(value));
        }
    }
}

/// A comment's text as HTML. Comment bodies are markdown (`format` is
/// `text/markdown`), so the card shows them rendered, through the same safe
/// renderer as the document: raw HTML is escaped, only safe links get an `href`,
/// images load nothing. Heading ids are dropped, because two ids with one value
/// would clash with the document's own headings.
fn comment_html(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let rendered = write_rendered(value, &chars, &[]);
    let html: String = rendered.blocks.iter().map(|b| b.html.as_str()).collect();
    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(at) = rest.find(" id=\"md-") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 5..];
        match after.find('"') {
            Some(end) => rest = &after[end + 1..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
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

/// One top-level block of the rendered document.
struct Block {
    /// The block's HTML, with its threads' numbers in place.
    html: String,
    /// An extra class for the row, with a leading space, or empty.
    class: &'static str,
}

struct Rendered {
    blocks: Vec<Block>,
    /// The block each thread's card belongs under, by thread index. A thread
    /// with no block to sit under is absent.
    owner: HashMap<usize, usize>,
    /// The threads that got at least one mark.
    marked: HashSet<usize>,
}

/// Renders the markdown as HTML in top-level blocks, with each thread's range
/// marked and a numbered link after the end of its text.
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
///
/// An image is the one exception to both rules. It is drawn as a camera icon
/// with no text, so there is no text to cut: a range that overlaps any part of
/// `![alt](src)` marks the whole icon, so a card is never left without a mark.
fn write_rendered(markdown: &str, chars: &[char], placed: &[&View]) -> Rendered {
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

    let count = spans.len();
    let mut doc = Doc {
        md: markdown,
        out: String::new(),
        blocks: Vec::new(),
        open: false,
        implicit: false,
        class: "",
        depth: 0,
        spans,
        next_pos: 0,
        pos: vec![None; count],
        first_block: vec![None; count],
        last_piece: vec![None; count],
        marked: HashSet::new(),
    };
    doc.run(&events);
    doc.finish_block();
    doc.place_remaining();
    doc.assemble()
}

/// A position in the output: the block, and the byte offset within its HTML.
type At = (usize, usize);

struct Doc<'a> {
    md: &'a str,
    /// The HTML of the block being written.
    out: String,
    blocks: Vec<Block>,
    /// Whether `out` is a block in progress.
    open: bool,
    /// The open block was started by loose inline content, not by a block tag,
    /// so the next block tag closes it.
    implicit: bool,
    class: &'static str,
    /// How many block-level tags are open. A block ends when it returns to 0.
    depth: usize,
    /// Sorted by start, then longest first, then thread order.
    spans: Vec<Span>,
    /// Spans before this index have had their position recorded.
    next_pos: usize,
    /// Where each span starts in the output: the position of a number for a
    /// thread with no rendered text, and the fallback block for its card.
    pos: Vec<Option<At>>,
    /// The block of each span's first mark, which holds its card.
    first_block: Vec<Option<usize>>,
    /// Just after the closing tags of each span's latest marked piece, which
    /// is where its number goes once the last piece is known.
    last_piece: Vec<Option<At>>,
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
        let mut skip_to = 0;

        for (i, (event, range)) in events.iter().enumerate() {
            if i < skip_to {
                continue;
            }
            if let Event::Start(Tag::Image { dest_url, .. }) = event {
                let (end, alt) = image_alt(events, i);
                skip_to = end + 1;
                self.image(dest_url, &alt, range);
                continue;
            }
            match event {
                Event::Start(tag) if starts_block(tag) => {
                    if self.depth == 0 {
                        self.finish_implicit();
                        self.begin_block(match tag {
                            Tag::Heading { .. } => " head",
                            Tag::FootnoteDefinition(_) => " note",
                            _ => "",
                        });
                    }
                    self.depth += 1;
                }
                Event::Rule if self.depth == 0 => {
                    self.finish_implicit();
                    self.begin_block(" rule");
                }
                _ => {}
            }
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
                    _ => {}
                },
                // A bare line break is the one piece of inline HTML that is safe to
                // emit, and tables use it for paragraph breaks inside a cell. The
                // fixed string is written, never the document's own text.
                Event::InlineHtml(text) if is_line_break_tag(text) => self.atom("<br>", range),
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
            match event {
                Event::End(tag) if ends_block(tag) => {
                    self.depth = self.depth.saturating_sub(1);
                    if self.depth == 0 {
                        self.finish_block();
                    }
                }
                Event::Rule if self.depth == 0 => self.finish_block(),
                _ => {}
            }
        }
    }

    fn begin_block(&mut self, class: &'static str) {
        self.open = true;
        self.implicit = false;
        self.class = class;
    }

    fn finish_block(&mut self) {
        if self.open {
            self.blocks.push(Block {
                html: std::mem::take(&mut self.out),
                class: self.class,
            });
            self.open = false;
        }
    }

    fn finish_implicit(&mut self) {
        if self.implicit {
            self.finish_block();
        }
    }

    /// Loose inline content with no block around it (not produced by the
    /// parser today) still lands in a block rather than being lost.
    fn ensure_block(&mut self) {
        if !self.open {
            self.begin_block("");
            self.implicit = true;
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

    /// Records the position of every thread that starts at or before `upto`
    /// and has none yet. A thread with no rendered text gets its number here,
    /// before the next rendered text, outside any mark; for one that has marks
    /// the position only matters as a fallback.
    fn record_positions(&mut self, upto: usize) {
        self.ensure_block();
        let at = (self.blocks.len(), self.out.len());
        while let Some(span) = self.spans.get(self.next_pos) {
            if span.start > upto {
                break;
            }
            self.pos[self.next_pos] = Some(at);
            self.next_pos += 1;
        }
    }

    /// Threads that start after all rendered text sit at the end of the last
    /// block.
    fn place_remaining(&mut self) {
        let Some(last) = self.blocks.last() else {
            return;
        };
        let at = (self.blocks.len() - 1, last.html.len());
        for pos in &mut self.pos[self.next_pos..] {
            *pos = Some(at);
        }
        self.next_pos = self.spans.len();
    }

    /// Puts each thread's number after its last marked piece (or at its
    /// recorded position when it has no marks) and settles which block each
    /// card belongs under.
    fn assemble(mut self) -> Rendered {
        let mut inserts: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); self.blocks.len()];
        let mut owner = HashMap::new();
        for (i, span) in self.spans.iter().enumerate() {
            let marked = self.last_piece[i].is_some();
            if let Some((block, offset)) = self.last_piece[i].or(self.pos[i]) {
                inserts[block].push((offset, i, marked));
            }
            if let Some(block) = self.first_block[i].or(self.pos[i].map(|p| p.0)) {
                owner.insert(span.index, block);
            }
        }
        for (block, list) in self.blocks.iter_mut().zip(inserts) {
            // A number that follows marked text moves to the end of the word it
            // landed in, so that it never splits one.
            let mut list: Vec<(usize, usize)> = list
                .into_iter()
                .map(|(mut at, i, after_text)| {
                    while after_text
                        && block.html[at..]
                            .chars()
                            .next()
                            .is_some_and(char::is_alphanumeric)
                    {
                        at += block.html[at..].chars().next().map_or(1, char::len_utf8);
                    }
                    (at, i)
                })
                .collect();
            list.sort_unstable();
            let mut html = String::with_capacity(block.html.len() + list.len() * 40);
            let mut done = 0;
            for (at, i) in list {
                html.push_str(&block.html[done..at]);
                html.push_str(&ref_link(self.spans[i].number));
                done = at;
            }
            html.push_str(&block.html[done..]);
            block.html = html;
        }
        Rendered {
            blocks: self.blocks,
            owner,
            marked: self.marked,
        }
    }

    /// An image: one grey camera icon and nothing else. Never an `<img>`, so
    /// the page loads nothing. The alt text and source are on the wrapper as
    /// the accessible name and the tooltip, not as visible text. Every range
    /// that overlaps the image's source marks the icon as a whole.
    fn image(&mut self, src: &str, alt: &str, range: &Range<usize>) {
        self.record_positions(range.end.saturating_sub(1).max(range.start));
        let label = if alt.trim().is_empty() { "image" } else { alt };
        let html = format!(
            "<span class=\"img\" role=\"img\" aria-label=\"{}\" title=\"{}\">{CAMERA}</span>",
            esc(label),
            esc(&format!("{label} ({src})")),
        );
        let covering: Vec<usize> = (0..self.spans.len())
            .filter(|&i| {
                let s = &self.spans[i];
                s.start < range.end && s.end > range.start && s.end > s.start
            })
            .collect();
        self.marked_html(&html, &covering);
    }

    /// A piece of rendered text with no source text of its own to mark.
    fn atom(&mut self, html: &str, range: &Range<usize>) {
        self.record_positions(range.end.saturating_sub(1).max(range.start));
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
        self.record_positions(base.unwrap_or(range.start));
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
            self.record_positions(a);
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
        self.record_positions(range.end.saturating_sub(1).max(range.start));
        let covering: Vec<usize> = (0..self.spans.len())
            .filter(|&i| {
                let s = &self.spans[i];
                s.start < range.end && s.end > range.start && s.end > s.start
            })
            .collect();
        self.marked_run(text, &covering);
    }

    fn marked_run(&mut self, text: &str, covering: &[usize]) {
        self.marked_html(&esc(text), covering);
    }

    /// `html` is already safe to emit; it is wrapped in one `<mark>` per
    /// covering range.
    fn marked_html(&mut self, html: &str, covering: &[usize]) {
        for &i in covering {
            self.out.push_str(&self.spans[i].open);
            self.marked.insert(self.spans[i].index);
            self.first_block[i].get_or_insert(self.blocks.len());
        }
        self.out.push_str(html);
        for _ in covering {
            self.out.push_str("</mark>");
        }
        let at = (self.blocks.len(), self.out.len());
        for &i in covering {
            self.last_piece[i] = Some(at);
        }
    }
}

/// The camera icon that stands for an image. Stroke only, in `currentColor`, so
/// the grey comes from the wrapper's `color`. It holds no `<image>`, link or
/// style, so it loads nothing.
const CAMERA: &str = "<svg viewBox=\"0 0 24 24\" width=\"18\" height=\"18\" fill=\"none\" \
stroke=\"currentColor\" stroke-width=\"1.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\" \
aria-hidden=\"true\" focusable=\"false\"><path d=\"M4 8h3l1.5-2.5h7L17 8h3a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z\"/>\
<circle cx=\"12\" cy=\"13.5\" r=\"3.5\"/></svg>";

/// The index of the event that closes the image opened at `start`, and the
/// plain text inside it (its alt text).
fn image_alt(events: &[(Event, Range<usize>)], start: usize) -> (usize, String) {
    let mut depth = 0;
    let mut alt = String::new();
    for (i, (event, _)) in events.iter().enumerate().skip(start) {
        match event {
            Event::Start(Tag::Image { .. }) => depth += 1,
            Event::End(TagEnd::Image) => {
                depth -= 1;
                if depth == 0 {
                    return (i, alt);
                }
            }
            Event::Text(t) | Event::Code(t) | Event::InlineHtml(t) | Event::Html(t) => {
                alt.push_str(t)
            }
            Event::SoftBreak | Event::HardBreak => alt.push(' '),
            _ => {}
        }
    }
    (events.len().saturating_sub(1), alt)
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

/// Block-level tags. Everything else (items, rows, cells, inline tags) lives
/// inside one of these, so a block ends where the first of them closes.
fn starts_block(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
    )
}

fn ends_block(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
    )
}

/// Whether `html` is exactly `<br>`, `<br/>` or `<br />`, in any letter case.
fn is_line_break_tag(html: &str) -> bool {
    matches!(
        html.trim().to_ascii_lowercase().as_str(),
        "<br>" | "<br/>" | "<br />"
    )
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

/// The opening `<mark>` tag of a thread. Every mark looks the same; the
/// attributes carry the state and status for tests and for later tooling, and
/// nothing in the style sheet reads them.
fn mark_open(v: &View) -> String {
    format!(
        "<mark data-thread=\"t-{}\" data-state=\"{}\" data-status=\"{}\" title=\"{}\">",
        v.index + 1,
        state_class(&v.state),
        v.anchor_class(),
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
  --bg: #fbfbfa; --fg: #1f2328; --muted: #6b737c; --line: #dfe2e5;
  --panel: #ffffff; --gutter: #f0f2f4;
  --mark: rgba(255, 212, 0, 0.30); --ref: #80878f;
  --del: #a4504b; --ins: #3d7a50; --link: #0b5cad;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14171a; --fg: #e3e6e9; --muted: #8f99a3; --line: #2f353b;
    --panel: #1b1f23; --gutter: #22272c;
    --mark: rgba(255, 200, 0, 0.22); --ref: #9aa4ae;
    --del: #d98a85; --ins: #7fbf92; --link: #6cb2ff;
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }
header, main { max-width: 60rem; margin: 0 auto; padding: 0 16px; }
header h1 { font-size: 1.1rem; margin: 16px 0 4px; }
.orphans h2 { font-size: 1.05rem; margin: 24px 0 8px; }
.summary { margin: 0 0 16px; color: var(--muted); }
.layout { min-width: 0; }
.source-box { display: flex; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); overflow: hidden; }
pre { margin: 0; font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.gutter { padding: 8px 8px; text-align: right; color: var(--muted); background: var(--gutter);
  border-right: 1px solid var(--line); user-select: none; }
.source { flex: 1; min-width: 0; overflow-x: auto; padding: 8px 12px; white-space: pre; tab-size: 4; }
mark { background: var(--mark); color: inherit; border-radius: 2px; padding: 0; }
.ref { font-size: .7em; line-height: 0; vertical-align: sub; color: var(--ref);
  text-decoration: none; user-select: none; padding: 0 1px; }
.cards { display: flex; flex-direction: column; gap: 8px; min-width: 0; margin-top: 16px; }
.card { border: 1px solid var(--line); border-radius: 6px; background: var(--panel);
  padding: 8px 10px; min-width: 0; font-size: 13px; line-height: 1.45; overflow-wrap: anywhere; }
.card.dim { opacity: .6; }
.card:target { outline: 1px solid var(--muted); outline-offset: 1px; }
.card-head { display: flex; flex-wrap: wrap; gap: 0 10px; align-items: baseline; font-size: 12px; color: var(--muted); }
.num { color: var(--ref); }
.id { font: 11px ui-monospace, Menlo, Consolas, monospace; }
.by { margin: 2px 0 4px; color: var(--muted); font-size: 12px; }
.agent { font-style: italic; }
.body { margin: 4px 0; font-size: 14px; overflow-wrap: anywhere; }
.body > :first-child { margin-top: 0; }
.body > :last-child { margin-bottom: 0; }
.body p, .body ul, .body ol, .body pre, .body blockquote { margin: 0 0 6px; }
.body h1, .body h2, .body h3, .body h4 { font-size: 14px; margin: 6px 0 4px; border: 0; padding: 0; }
.body pre { overflow-x: auto; }
.body table { display: block; overflow-x: auto; }
.was, .quote { margin: 4px 0; font-size: 12px; color: var(--muted); }
.quote { border-left: 2px solid var(--line); padding-left: 8px; margin-left: 0; white-space: pre-wrap; }
.edit { margin: 4px 0; font: 13px ui-monospace, Menlo, Consolas, monospace; white-space: pre-wrap; }
del { color: var(--del); text-decoration: line-through; }
ins { color: var(--ins); text-decoration: none; }
.arrow { color: var(--muted); }
.replies { margin-top: 8px; border-left: 1px solid var(--line); padding-left: 10px; }
.reply { margin-top: 6px; }
.none { color: var(--muted); }
.orphans .card { margin-bottom: 8px; }
"#;

/// Added after [`STYLE`] on the rendered page: GitHub-like markdown in a
/// readable column, with each block's comments under it, and its own colour
/// variables and dark variant.
const MARKDOWN_STYLE: &str = r#":root {
  --md-code: rgba(130, 140, 150, 0.22); --md-block: #f3f5f7; --md-zebra: #f6f8fa;
  --md-rule: #d1d9e0; --md-quote: #59636e; --img: #98a0a8;
}
@media (prefers-color-scheme: dark) {
  :root {
    --md-code: rgba(150, 160, 170, 0.25); --md-block: #1f2429; --md-zebra: #1d2227;
    --md-rule: #3d444d; --md-quote: #9aa4ae; --img: #6f7882;
  }
}
.rendered-view header, .rendered-view main { max-width: 48rem; }
.rendered { margin-top: 8px; }
.blk { margin-bottom: 16px; }
.blk.head { margin-top: 24px; }
.blk.rule { margin: 8px 0; }
.blk.note { margin-bottom: 8px; }
.doc { min-width: 0; }
.margin { min-width: 0; display: flex; flex-direction: column; gap: 8px;
  margin: 8px 0 0 8px; padding-left: 12px; border-left: 1px solid var(--line); }
.margin:empty { display: none; }
.markdown-body { font: 16px/1.6 -apple-system, BlinkMacSystemFont,
  "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; overflow-wrap: break-word; }
.markdown-body > :is(p, ul, ol, blockquote, pre, div, hr, h1, h2, h3, h4, h5, h6) { margin: 0; }
.markdown-body h1, .markdown-body h2, .markdown-body h3, .markdown-body h4,
.markdown-body h5, .markdown-body h6 { font-weight: 600; line-height: 1.25; }
.markdown-body h1 { font-size: 2em; padding-bottom: .3em; border-bottom: 1px solid var(--md-rule); }
.markdown-body h2 { font-size: 1.5em; padding-bottom: .3em; border-bottom: 1px solid var(--md-rule); }
.markdown-body h3 { font-size: 1.25em; }
.markdown-body h4 { font-size: 1em; }
.markdown-body h5 { font-size: .875em; }
.markdown-body h6 { font-size: .85em; color: var(--muted); }
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
.markdown-body hr { height: .25em; padding: 0; background: var(--md-rule); border: 0; }
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
.markdown-body .img { color: var(--img); }
.markdown-body .img svg { width: 1.1em; height: 1.1em; vertical-align: -.15em; }
.markdown-body .footnote { font-size: .875em; color: var(--md-quote); }
.markdown-body :target { background: var(--mark); }
.markdown-body :is(h1, h2, h3, h4, h5, h6) .ref { font-size: 11px; }
@media (max-width: 40rem) {
  .markdown-body { font-size: 15px; }
  .markdown-body h1 { font-size: 1.7em; }
  .markdown-body th, .markdown-body td { padding: 4px 8px; }
}
"#;
