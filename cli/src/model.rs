//! Annotation and state-change records. Owned by task T-04. See design section 3.
//!
//! Records are `serde_json::Value`, not typed structs, so that a stored
//! annotation is written back unchanged and a field this version does not know
//! survives a read and a write. Builders produce the shapes of design 3 and
//! accessors read them.

use crate::error::{Error, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// The prefix IRI of the `marq:` extension terms (design 3.1).
const MARQ_CONTEXT_IRI: &str =
    "https://github.com/jimbarritt/marq/blob/main/doc/comments-design.md#";

/// The shortest id prefix an `ID` argument may use (design 6.3).
pub const MIN_ID_PREFIX: usize = 6;

/// Who wrote an annotation: a person, or software with `--agent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creator {
    pub name: String,
    /// Without the `mailto:` scheme; the stored form adds it.
    pub email: String,
    pub agent: bool,
}

impl Creator {
    pub fn person(name: impl Into<String>, email: impl Into<String>) -> Self {
        Creator {
            name: name.into(),
            email: strip_mailto(&email.into()).to_string(),
            agent: false,
        }
    }

    pub fn agent(name: impl Into<String>, email: impl Into<String>) -> Self {
        Creator {
            agent: true,
            ..Creator::person(name, email)
        }
    }

    /// The stored shape: `{"type", "name", "email": "mailto:..."}`. A creator
    /// with no email carries no `email` key, as an empty IRI would be invalid.
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "type".into(),
            json!(if self.agent { "Software" } else { "Person" }),
        );
        map.insert("name".into(), json!(self.name));
        if !self.email.is_empty() {
            map.insert("email".into(), json!(format!("mailto:{}", self.email)));
        }
        Value::Object(map)
    }

    pub fn from_value(value: &Value) -> Option<Creator> {
        let name = value.get("name")?.as_str()?.to_string();
        let email = value
            .get("email")
            .and_then(Value::as_str)
            .map(strip_mailto)
            .unwrap_or("")
            .to_string();
        let agent = value.get("type").and_then(Value::as_str) == Some("Software");
        Some(Creator { name, email, agent })
    }
}

fn strip_mailto(email: &str) -> &str {
    email.strip_prefix("mailto:").unwrap_or(email)
}

/// The quote and position of a selected range, passed as plain values because
/// anchoring is written separately. `start` and `end` are the caller's offsets
/// (code points, by design 4.1) and are stored as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub exact: String,
    pub prefix: String,
    pub suffix: String,
    pub start: usize,
    pub end: usize,
    /// For a whole-line comment, the RFC 5147 pair: `line=N-1,N` is `(N-1, N)`.
    pub line: Option<(usize, usize)>,
}

/// What a comment or suggestion points at: a file at a recorded version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The repo-relative path of the markdown file.
    pub source: String,
    /// The blob id of the working-tree file when the annotation was written.
    pub source_blob: String,
    pub selection: Selection,
}

impl Target {
    pub fn to_value(&self) -> Value {
        let s = &self.selection;
        let mut selectors = Vec::new();
        if let Some((from, to)) = s.line {
            selectors.push(json!({
                "type": "FragmentSelector",
                "conformsTo": "http://tools.ietf.org/rfc/rfc5147",
                "value": format!("line={from},{to}"),
            }));
        }
        selectors.push(json!({
            "type": "TextQuoteSelector",
            "exact": s.exact,
            "prefix": s.prefix,
            "suffix": s.suffix,
        }));
        selectors.push(json!({
            "type": "TextPositionSelector",
            "start": s.start,
            "end": s.end,
        }));
        json!({
            "source": self.source,
            "marq:sourceBlob": self.source_blob,
            "selector": selectors,
        })
    }
}

/// The current time as UTC with a `Z` suffix, to the second (design 3.1).
pub fn now_utc() -> String {
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("zero is a valid nanosecond");
    now.format(&Rfc3339)
        .expect("RFC 3339 formatting of a UTC time cannot fail")
}

/// A new random `urn:uuid:` id.
pub fn new_id() -> String {
    format!("urn:uuid:{}", uuid::Uuid::new_v4())
}

/// The uuid of an id, without `urn:uuid:`. This is also the file name stem.
pub fn uuid_of(id: &str) -> &str {
    id.strip_prefix("urn:uuid:").unwrap_or(id)
}

/// The first 8 characters of the uuid, for messages and text output.
pub fn short_id(id: &str) -> String {
    uuid_of(id).chars().take(8).collect()
}

/// Cleans an `ID` argument: the full uuid, the `urn:uuid:` form, or a prefix of
/// at least 6 hex characters. Returns the lower-case uuid prefix.
pub fn normalise_id_arg(arg: &str) -> Result<String> {
    let cleaned = uuid_of(arg.trim()).to_ascii_lowercase();
    let valid = cleaned.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    let hex_count = cleaned.chars().filter(char::is_ascii_hexdigit).count();
    if !valid || hex_count < MIN_ID_PREFIX {
        return Err(Error::message(format!(
            "{arg} is not an id: give a full uuid or at least {MIN_ID_PREFIX} hex characters of one"
        )));
    }
    Ok(cleaned)
}

/// Whether an `ID` argument names `id`.
pub fn matches_id(arg: &str, id: &str) -> bool {
    match normalise_id_arg(arg) {
        Ok(prefix) => uuid_of(id).to_ascii_lowercase().starts_with(&prefix),
        Err(_) => false,
    }
}

/// Resolves an `ID` argument against known ids. Fails with `NoMatch` or
/// `Ambiguous`, both exit code 2.
pub fn resolve_id<'a>(arg: &str, ids: impl IntoIterator<Item = &'a str>) -> Result<String> {
    let mut found: Vec<String> = ids
        .into_iter()
        .filter(|id| matches_id(arg, id))
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    match found.len() {
        0 => Err(Error::NoMatch(arg.to_string())),
        1 => Ok(found.remove(0)),
        _ => Err(Error::Ambiguous {
            arg: arg.to_string(),
            candidates: found,
        }),
    }
}

fn base(creator: &Creator, kind: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "@context".into(),
        json!(["http://www.w3.org/ns/anno.jsonld", {"marq": MARQ_CONTEXT_IRI}]),
    );
    map.insert("id".into(), json!(new_id()));
    map.insert("type".into(), json!(kind));
    map.insert("created".into(), json!(now_utc()));
    map.insert("creator".into(), creator.to_value());
    map.insert(
        "generator".into(),
        json!({
            "type": "Software",
            "name": format!("marq-comments {}", env!("CARGO_PKG_VERSION")),
        }),
    );
    map
}

fn markdown_body(text: &str) -> Value {
    json!({"type": "TextualBody", "value": text, "format": "text/markdown"})
}

/// A comment on a word or, when the selection has a line pair, a whole line.
pub fn new_comment(creator: &Creator, text: &str, target: &Target) -> Value {
    let mut map = base(creator, "Annotation");
    map.insert("motivation".into(), json!("commenting"));
    map.insert("body".into(), markdown_body(text));
    map.insert("target".into(), target.to_value());
    Value::Object(map)
}

/// A reply. `target_id` is the id of the annotation replied to, which may be a reply.
pub fn new_reply(creator: &Creator, text: &str, target_id: &str) -> Value {
    let mut map = base(creator, "Annotation");
    map.insert("motivation".into(), json!("replying"));
    map.insert("body".into(), markdown_body(text));
    map.insert("target".into(), json!(target_id));
    Value::Object(map)
}

/// A suggestion: the replacement text, and an optional reason as a second body.
pub fn new_suggestion(
    creator: &Creator,
    replacement: &str,
    reason: Option<&str>,
    target: &Target,
) -> Value {
    let mut bodies = vec![json!({
        "type": "TextualBody",
        "value": replacement,
        "purpose": "editing",
    })];
    if let Some(reason) = reason {
        bodies.push(json!({
            "type": "TextualBody",
            "value": reason,
            "purpose": "commenting",
            "format": "text/markdown",
        }));
    }
    let mut map = base(creator, "Annotation");
    map.insert("motivation".into(), json!("editing"));
    map.insert("body".into(), Value::Array(bodies));
    map.insert("target".into(), target.to_value());
    Value::Object(map)
}

/// A state change for `annotation_id`. `result_blob` is the markdown blob after
/// the edit, carried by `accepted`.
pub fn new_state_change(
    creator: &Creator,
    annotation_id: &str,
    state: &str,
    result_blob: Option<&str>,
) -> Value {
    let mut map = base(creator, "marq:StateChange");
    map.insert("marq:annotation".into(), json!(annotation_id));
    map.insert("marq:state".into(), json!(state));
    if let Some(blob) = result_blob {
        map.insert("marq:resultBlob".into(), json!(blob));
    }
    Value::Object(map)
}

/// Replaces `id` and `created`, for tests and tools that need a fixed record.
pub fn stamp(record: &mut Value, id: &str, created: &str) {
    if let Some(map) = record.as_object_mut() {
        map.insert("id".into(), json!(id));
        map.insert("created".into(), json!(created));
    }
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let ordered: BTreeMap<&String, Value> =
                map.iter().map(|(k, v)| (k, sorted(v))).collect();
            // Inserting in order sorts the map whether or not `preserve_order`
            // is enabled by another crate in the dependency graph.
            let mut out = Map::new();
            for (k, v) in ordered {
                out.insert(k.clone(), v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The stored form: sorted keys, two-space indentation, a final newline (design 3.1).
pub fn to_stored_json(value: &Value) -> String {
    let mut text =
        serde_json::to_string_pretty(&sorted(value)).expect("a JSON value always serialises");
    text.push('\n');
    text
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

pub fn id(record: &Value) -> Option<&str> {
    str_field(record, "id")
}

pub fn created(record: &Value) -> Option<&str> {
    str_field(record, "created")
}

pub fn motivation(record: &Value) -> Option<&str> {
    str_field(record, "motivation")
}

pub fn target(record: &Value) -> Option<&Value> {
    record.get("target")
}

/// The id a reply points at, when the target is a plain id string.
pub fn target_id(record: &Value) -> Option<&str> {
    target(record)?.as_str()
}

/// The repo-relative path in an object target.
pub fn target_source(record: &Value) -> Option<&str> {
    str_field(target(record)?, "source")
}

pub fn target_source_blob(record: &Value) -> Option<&str> {
    str_field(target(record)?, "marq:sourceBlob")
}

pub fn creator(record: &Value) -> Option<Creator> {
    Creator::from_value(record.get("creator")?)
}

/// The comment text: the body of a comment or reply, or the `commenting` body
/// of a suggestion.
pub fn body_text(record: &Value) -> Option<&str> {
    match record.get("body")? {
        Value::Array(items) => items
            .iter()
            .find(|b| str_field(b, "purpose") == Some("commenting"))
            .and_then(|b| str_field(b, "value")),
        body => str_field(body, "value"),
    }
}

/// The replacement text of a suggestion.
pub fn suggestion_text(record: &Value) -> Option<&str> {
    record
        .get("body")?
        .as_array()?
        .iter()
        .find(|b| str_field(b, "purpose") == Some("editing"))
        .and_then(|b| str_field(b, "value"))
}

/// The `marq:state` of a state change.
pub fn state(record: &Value) -> Option<&str> {
    str_field(record, "marq:state")
}

/// The annotation a state change is about.
pub fn state_annotation(record: &Value) -> Option<&str> {
    str_field(record, "marq:annotation")
}

pub fn result_blob(record: &Value) -> Option<&str> {
    str_field(record, "marq:resultBlob")
}

/// Orders records by (`created`, `id`), the order design 5 folds state in.
pub fn sort_records(records: &mut [Value]) {
    records.sort_by(|a, b| {
        (created(a).unwrap_or(""), id(a).unwrap_or(""))
            .cmp(&(created(b).unwrap_or(""), id(b).unwrap_or("")))
    });
}
