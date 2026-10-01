//! Text output of threads. Owned by task T-06. See doc/comments-design.md 6.2.
//!
//! These functions read the thread objects of `threads::build`, the same
//! values `--json` prints, so the two outputs cannot disagree.

use crate::model;
use crate::threads;
use serde_json::Value;

/// One thread per block, as in design 6.2. `key` is the document's repo-relative path.
pub fn thread_text(thread: &Value, key: &str) -> String {
    let annotation = &thread["annotation"];
    let mut out = String::new();
    let kind = match model::motivation(annotation) {
        Some("editing") => "suggestion",
        Some("replying") => "reply",
        _ => "comment",
    };
    out.push_str(&format!(
        "{}  {}  {}  {}\n",
        model::short_id(model::id(annotation).unwrap_or("")),
        thread["state"].as_str().unwrap_or("open"),
        kind,
        location(thread, annotation, key),
    ));
    body_lines(&mut out, annotation, 2);
    replies_text(&mut out, thread, 1);
    out
}

/// A thread block followed by the state changes that led to its state.
pub fn show_text(thread: &Value, key: &str) -> String {
    let mut out = thread_text(thread, key);
    let changes = thread["stateChanges"].as_array().map_or(&[][..], |v| v);
    if !changes.is_empty() {
        out.push_str("  state changes:\n");
        for change in changes {
            out.push_str(&format!(
                "    {}  {}\n",
                change["marq:state"].as_str().unwrap_or("?"),
                who_when(change),
            ));
        }
    }
    out
}

/// Every thread block, separated by nothing but the blocks' own newlines.
pub fn list_text(threads: &[Value], key: &str) -> String {
    threads.iter().map(|t| thread_text(t, key)).collect()
}

fn replies_text(out: &mut String, thread: &Value, depth: usize) {
    let Some(replies) = thread["replies"].as_array() else {
        return;
    };
    for reply in replies {
        let annotation = &reply["annotation"];
        let indent = "  ".repeat(depth);
        out.push_str(&format!(
            "{indent}{}  {}\n",
            model::short_id(model::id(annotation).unwrap_or("")),
            who_when(annotation),
        ));
        body_lines(out, annotation, depth * 2 + 2);
        replies_text(out, reply, depth + 1);
    }
}

/// The author line, then the suggested text and the comment body, indented.
fn body_lines(out: &mut String, annotation: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    if model::motivation(annotation) != Some("replying") || indent == 2 {
        out.push_str(&format!("{pad}{}\n", who_when(annotation)));
    }
    if let Some(replacement) = model::suggestion_text(annotation) {
        out.push_str(&format!("{pad}replace with {}\n", quoted(replacement)));
    }
    if let Some(text) = model::body_text(annotation) {
        for line in text.lines() {
            out.push_str(&format!("{pad}{line}\n"));
        }
    }
}

/// `Name, 2026-09-29 10:15`, with `(agent)` after a software author. The time is UTC.
fn who_when(record: &Value) -> String {
    let creator = model::creator(record);
    let name = creator.as_ref().map_or("unknown", |c| c.name.as_str());
    let agent = if creator.as_ref().is_some_and(|c| c.agent) {
        " (agent)"
    } else {
        ""
    };
    let created = model::created(record).unwrap_or("");
    let when: String = created
        .chars()
        .take(16)
        .collect::<String>()
        .replace('T', " ");
    format!("{name}{agent}, {when}")
}

/// The anchor part of a block header: `file:line:column "text"`, with
/// `changed` and `was "original"` when the text differs, or `orphaned` and the
/// stored quote.
fn location(thread: &Value, annotation: &Value, key: &str) -> String {
    let anchor = &thread["anchor"];
    match anchor["status"].as_str() {
        Some("anchored") => format!("{}  {}", place(anchor, key), quoted(text_of(anchor))),
        Some("changed") => format!(
            "{}  {}  changed  was {}",
            place(anchor, key),
            quoted(text_of(anchor)),
            quoted(anchor["original"].as_str().unwrap_or(""))
        ),
        _ => {
            let quote = threads::selectors_of(annotation).map_or(String::new(), |s| s.exact);
            if model::motivation(annotation) == Some("replying") {
                "orphaned".to_string()
            } else {
                format!("orphaned  {}", quoted(&quote))
            }
        }
    }
}

fn place(anchor: &Value, key: &str) -> String {
    format!(
        "{key}:{}:{}",
        anchor["line"].as_u64().unwrap_or(0),
        anchor["column"].as_u64().unwrap_or(0)
    )
}

fn text_of(anchor: &Value) -> &str {
    anchor["text"].as_str().unwrap_or("")
}

/// Double quotes without escaping, so the text reads as it stands in the file.
fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}
