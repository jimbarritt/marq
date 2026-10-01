//! Threads, folded state and resolved anchors. Owned by task T-06.
//! See doc/comments-design.md sections 4.2, 5 and 6.2.
//!
//! A thread is a `serde_json::Value` in the shape `list --json` prints, so the
//! text output, the JSON output and `render` all read one structure.

use crate::anchor::{self, Anchor, Selectors};
use crate::error::Result;
use crate::model;
use crate::store::Store;
use crate::text;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// The state of an annotation with no state change recorded.
pub const OPEN: &str = "open";

/// Rebuilds the selectors of a comment or suggestion from its stored target.
/// `None` when the target has no quote or position selector.
pub fn selectors_of(annotation: &Value) -> Option<Selectors> {
    let list = annotation.get("target")?.get("selector")?.as_array()?;
    let find = |kind: &str| {
        list.iter()
            .find(|s| s.get("type").and_then(Value::as_str) == Some(kind))
    };
    let quote = find("TextQuoteSelector")?;
    let position = find("TextPositionSelector")?;
    let text_of = |key: &str| {
        quote
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let number = |key: &str| position.get(key)?.as_u64().map(|n| n as usize);
    // The fragment selector's presence marks a whole-line anchor (design 3.3).
    let line = find("FragmentSelector")
        .and_then(|s| s.get("value")?.as_str())
        .and_then(|v| v.strip_prefix("line="))
        .and_then(|v| v.split_once(','))
        .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)));
    Some(Selectors {
        exact: text_of("exact"),
        prefix: text_of("prefix"),
        suffix: text_of("suffix"),
        start: number("start")?,
        end: number("end")?,
        line,
    })
}

/// Resolves anchors against one version of a document, reading each recorded
/// version from the comments branch once.
pub struct Resolver<'a> {
    store: &'a Store,
    key: &'a str,
    current: &'a str,
    versions: RefCell<HashMap<String, Option<String>>>,
}

impl<'a> Resolver<'a> {
    pub fn new(store: &'a Store, key: &'a str, current: &'a str) -> Self {
        Resolver {
            store,
            key,
            current,
            versions: RefCell::new(HashMap::new()),
        }
    }

    /// Design 4.2, computed on every read and never stored.
    pub fn resolve(&self, annotation: &Value) -> Anchor {
        let Some(selectors) = selectors_of(annotation) else {
            return Anchor::Orphaned;
        };
        // A missing version only disables the line diff: the quote search still runs.
        let recorded = model::target_source_blob(annotation).and_then(|blob| {
            self.versions
                .borrow_mut()
                .entry(blob.to_string())
                .or_insert_with(|| self.store.read_version(self.key, blob).ok())
                .clone()
        });
        anchor::resolve(&selectors, recorded.as_deref(), self.current)
    }
}

/// The anchor of an accepted suggestion. The text it quoted is the text the
/// accept replaced, so resolving the stored quote reports `changed` forever.
/// Instead the replacement is anchored: its selectors are made from the edited
/// file the accept recorded, then resolved against the current text like any
/// other anchor, so a later edit gives `changed` or `orphaned` as usual.
///
/// A deletion, or an accept recorded without `marq:resultStart`, has no text to
/// anchor to and reports `applied`, with no location.
fn accepted_anchor(
    store: &Store,
    key: &str,
    current: &str,
    annotation: &Value,
    accepted: &Value,
) -> Value {
    let applied = || json!({"status": "applied"});
    let (Some(blob), Some(start), Some(replacement)) = (
        model::result_blob(accepted),
        model::result_start(accepted),
        model::suggestion_text(annotation),
    ) else {
        return applied();
    };
    let length = text::char_len(replacement);
    let Ok(edited) = store.read_version(key, blob) else {
        return applied();
    };
    match anchor::selectors_for_range(&edited, start, start + length) {
        Ok(selectors) if length > 0 => anchor_value(
            &anchor::resolve(&selectors, Some(&edited), current),
            current,
        ),
        _ => applied(),
    }
}

/// The anchor as the JSON of design 6.2.
pub fn anchor_value(anchor: &Anchor, current: &str) -> Value {
    let located = |start: usize, end: usize| -> Option<(usize, usize, usize, usize, String)> {
        let (line, column) = text::line_col(current, start)?;
        let quoted = text::slice(current, start, end)?.to_string();
        Some((start, end, line, column, quoted))
    };
    match anchor {
        Anchor::Anchored { start, end } => match located(*start, *end) {
            Some((start, end, line, column, text)) => json!({
                "status": "anchored", "start": start, "end": end,
                "line": line, "column": column, "text": text,
            }),
            None => json!({"status": "orphaned"}),
        },
        Anchor::Changed {
            start,
            end,
            original,
        } => match located(*start, *end) {
            Some((start, end, line, column, text)) => json!({
                "status": "changed", "start": start, "end": end,
                "line": line, "column": column, "text": text, "original": original,
            }),
            None => json!({"status": "orphaned"}),
        },
        Anchor::Orphaned => json!({"status": "orphaned"}),
    }
}

/// The folded state of `annotation_id`: the last state change in
/// (`created`, `id`) order, or `open`. `states` must already be in that order,
/// as `Store::read_document` returns them.
pub fn fold_state(annotation_id: &str, states: &[Value]) -> String {
    states
        .iter()
        .rfind(|s| model::state_annotation(s) == Some(annotation_id))
        .and_then(|s| model::state(s))
        .unwrap_or(OPEN)
        .to_string()
}

/// Whether an annotation starts a thread, as opposed to replying inside one.
pub fn is_root(annotation: &Value) -> bool {
    model::motivation(annotation) != Some("replying")
}

/// The root threads of a document, each with its replies nested, in `created`
/// order. `current` is the markdown now in the working tree.
///
/// State changes and replies are grouped by the id they point at in one pass
/// each. Scanning every record for every root was quadratic: found by T-07,
/// `list` on 1000 comments spent 0.6 s of CPU in a release build on those scans.
pub fn build(store: &Store, key: &str, current: &str) -> Result<Vec<Value>> {
    let records = store.read_document(key)?;
    let resolver = Resolver::new(store, key, current);
    // Each group keeps the (`created`, `id`) order `read_document` returns,
    // so the last change in a group is the fold of design 5.
    let mut changes_of: HashMap<&str, Vec<&Value>> = HashMap::new();
    for change in &records.states {
        if let Some(about) = model::state_annotation(change) {
            changes_of.entry(about).or_default().push(change);
        }
    }
    let mut children: HashMap<&str, Vec<&Value>> = HashMap::new();
    for reply in records.annotations.iter().filter(|a| !is_root(a)) {
        if let Some(parent) = model::target_id(reply) {
            children.entry(parent).or_default().push(reply);
        }
    }
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for annotation in records.annotations.iter().filter(|a| is_root(a)) {
        let id = model::id(annotation).unwrap_or("");
        let own: &[&Value] = changes_of.get(id).map_or(&[], Vec::as_slice);
        let state = own
            .last()
            .and_then(|s| model::state(s))
            .unwrap_or(OPEN)
            .to_string();
        let anchor = match own.last() {
            Some(last)
                if state == "accepted" && model::motivation(annotation) == Some("editing") =>
            {
                accepted_anchor(store, key, current, annotation, last)
            }
            _ => anchor_value(&resolver.resolve(annotation), current),
        };
        let changes: Vec<Value> = own.iter().map(|s| (*s).clone()).collect();
        seen.insert(id.to_string());
        let replies = replies_of(id, &children, &state, &anchor, &mut seen);
        roots.push(json!({
            "annotation": annotation,
            "state": state,
            "anchor": anchor,
            "stateChanges": changes,
            "replies": replies,
        }));
    }
    Ok(roots)
}

/// A reply carries its root's state and anchor and has no state changes of its
/// own. `seen` stops a hand-edited cycle of replies from recursing forever.
fn replies_of(
    parent_id: &str,
    children: &HashMap<&str, Vec<&Value>>,
    state: &str,
    anchor: &Value,
    seen: &mut HashSet<String>,
) -> Vec<Value> {
    let mut replies = Vec::new();
    for reply in children.get(parent_id).map_or(&[][..], Vec::as_slice) {
        let id = model::id(reply).unwrap_or("");
        if !seen.insert(id.to_string()) {
            continue;
        }
        let nested = replies_of(id, children, state, anchor, seen);
        replies.push(json!({
            "annotation": reply,
            "state": state,
            "anchor": anchor,
            "stateChanges": [],
            "replies": nested,
        }));
    }
    replies
}

/// The thread object for `id`, searched through every thread and reply.
pub fn find<'a>(threads: &'a [Value], id: &str) -> Option<&'a Value> {
    for thread in threads {
        if thread["annotation"]["id"].as_str() == Some(id) {
            return Some(thread);
        }
        if let Some(replies) = thread["replies"].as_array() {
            if let Some(found) = find(replies, id) {
                return Some(found);
            }
        }
    }
    None
}
