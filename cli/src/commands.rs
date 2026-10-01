//! One function per command. Owned by task T-06. See doc/comments-design.md 5 and 6.

use crate::anchor::{self, Anchor, Selectors};
use crate::cli::{Cli, Command, Place, StateFilter};
use crate::error::{Error, Result};
use crate::git::Git;
use crate::model::{self, Creator, Selection, Target};
use crate::output;
use crate::render;
use crate::store::{Store, SyncReport};
use crate::text;
use crate::threads::{self, Resolver};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Exit code for `accept` refusing a suggestion whose anchor moved (design 6.3).
pub const EXIT_REFUSED: i32 = 3;

/// Why a command failed. `Refused` is `accept` declining a stale suggestion,
/// which scripts branch on, so it has its own exit code.
#[derive(Debug)]
pub enum Failure {
    Error(Error),
    Refused(String),
}

impl Failure {
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Error(e) => e.exit_code(),
            Failure::Refused(_) => EXIT_REFUSED,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Error(e) => e.fmt(f),
            Failure::Refused(text) => f.write_str(text),
        }
    }
}

impl<E: Into<Error>> From<E> for Failure {
    fn from(e: E) -> Self {
        Failure::Error(e.into())
    }
}

type Outcome<T = ()> = std::result::Result<T, Failure>;

/// Runs the command in the current directory. `main` has already moved there
/// for `-C`.
pub fn run(cli: Cli) -> Outcome {
    let who = Who {
        author: cli.author,
        agent: cli.agent,
    };
    let store = || Store::open(Path::new("."));
    match cli.command {
        Command::Comment {
            file,
            place,
            message,
        } => {
            let store = store()?;
            let id = create(&store, &who, &file, &place, Kind::Comment(&message))?;
            emit(&format!("{id}\n"));
        }
        Command::Suggest {
            file,
            place,
            replace,
            message,
        } => {
            if place.line.is_some() && place.text.is_none() {
                return Err(Error::message("suggest needs --text OLD along with --line").into());
            }
            let store = store()?;
            let kind = Kind::Suggestion {
                replacement: &replace,
                reason: message.as_deref(),
            };
            let id = create(&store, &who, &file, &place, kind)?;
            emit(&format!("{id}\n"));
        }
        Command::Reply { id, message } => {
            let store = store()?;
            let id = store.with_lock(|| reply(&store, &who, &id, &message))?;
            emit(&format!("{id}\n"));
        }
        Command::List { file, state, json } => list(&store()?, &file, state, json)?,
        Command::Show { id, json } => show(&store()?, &id, json)?,
        Command::Resolve { id } => decide(&store()?, &who, &id, Action::Resolve)?,
        Command::Reopen { id } => decide(&store()?, &who, &id, Action::Reopen)?,
        Command::Reject { id } => decide(&store()?, &who, &id, Action::Reject)?,
        Command::Accept { id } => decide(&store()?, &who, &id, Action::Accept)?,
        Command::Sync { remote } => sync(&store()?, &who, &remote)?,
        Command::Render {
            file,
            output,
            source,
        } => render_file(&store()?, &file, output.as_deref(), source)?,
    }
    Ok(())
}

/// Writes to standard output, ignoring a closed pipe: `render | head` is not an error.
fn emit(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

// ---- who is writing ----

struct Who {
    author: Option<String>,
    agent: bool,
}

impl Who {
    /// `--author`, else git's `user.name` and `user.email`. The error names both
    /// ways to fix it.
    fn creator(&self, store: &Store) -> Result<Creator> {
        let (name, email) = match &self.author {
            Some(arg) => parse_author(arg)?,
            None => {
                let git = Git::new(store.root());
                let config = |key: &str| {
                    git.run_raw(&["config", key], None)
                        .ok()
                        .filter(|o| o.success())
                        .map(|o| o.text())
                        .unwrap_or_default()
                };
                (config("user.name"), config("user.email"))
            }
        };
        if name.is_empty() {
            return Err(Error::message(
                "there is no author: pass --author \"Name <email>\" or set git config user.name and user.email",
            ));
        }
        Ok(if self.agent {
            Creator::agent(name, email)
        } else {
            Creator::person(name, email)
        })
    }
}

/// `Name <email>` or a bare name.
fn parse_author(arg: &str) -> Result<(String, String)> {
    let (name, email) = match arg.split_once('<') {
        Some((name, rest)) => match rest.trim_end().strip_suffix('>') {
            Some(email) => (name.trim(), email.trim()),
            None => {
                return Err(Error::message(format!(
                    "{arg:?} is not an author: write it as \"Name <email>\""
                )))
            }
        },
        None => (arg.trim(), ""),
    };
    if name.is_empty() {
        return Err(Error::message(format!(
            "{arg:?} has no name: write the author as \"Name <email>\""
        )));
    }
    Ok((name.to_string(), email.to_string()))
}

// ---- creating ----

enum Kind<'a> {
    Comment(&'a str),
    Suggestion {
        replacement: &'a str,
        reason: Option<&'a str>,
    },
}

fn read_text(store: &Store, key: &str) -> Result<(String, Vec<u8>)> {
    let path = store.working_path(key);
    let bytes =
        std::fs::read(&path).map_err(|e| Error::message(format!("cannot read {key}: {e}")))?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| Error::message(format!("{key} is not valid UTF-8")))?;
    Ok((text, bytes))
}

fn parse_range(arg: &str) -> Result<(usize, usize)> {
    let bad = || Error::message(format!("{arg:?} is not a range: write it as START:END"));
    let (start, end) = arg.split_once(':').ok_or_else(bad)?;
    Ok((
        start.trim().parse().map_err(|_| bad())?,
        end.trim().parse().map_err(|_| bad())?,
    ))
}

fn selectors_for(markdown: &str, place: &Place) -> Result<Selectors> {
    let result = if let Some(range) = &place.range {
        let (start, end) = parse_range(range)?;
        anchor::selectors_for_range(markdown, start, end)
    } else {
        let line = place.line.unwrap_or(0);
        match &place.text {
            Some(word) => {
                let nth = place.nth.unwrap_or(1);
                let (start, end) =
                    text::find_in_line(markdown, line, word, nth).ok_or_else(|| {
                        Error::message(format!("line {line} has no occurrence {nth} of {word:?}"))
                    })?;
                anchor::selectors_for_range(markdown, start, end)
            }
            None => anchor::selectors_for_line(markdown, line),
        }
    };
    result.map_err(Error::Message)
}

fn require_message(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error::message("the message is empty"));
    }
    Ok(())
}

fn create(store: &Store, who: &Who, file: &str, place: &Place, kind: Kind) -> Outcome<String> {
    if let Kind::Comment(message) = &kind {
        require_message(message)?;
    }
    let creator = who.creator(store)?;
    let key = store.document_key(file)?;
    let (markdown, bytes) = read_text(store, &key)?;
    let selectors = selectors_for(&markdown, place)?;
    // The blob is made from the bytes the selectors were computed on, not from a
    // second read of the file, so an edit between the two cannot split them.
    let source_blob = store.put_blob(&bytes)?;
    let target = Target {
        source: key.clone(),
        source_blob: source_blob.clone(),
        selection: Selection {
            exact: selectors.exact,
            prefix: selectors.prefix,
            suffix: selectors.suffix,
            start: selectors.start,
            end: selectors.end,
            line: selectors.line,
        },
    };
    let annotation = match kind {
        Kind::Comment(message) => model::new_comment(&creator, message, &target),
        Kind::Suggestion {
            replacement,
            reason,
        } => model::new_suggestion(&creator, replacement, reason, &target),
    };
    store.write_annotation(&key, &annotation, Some(&source_blob))?;
    Ok(model::short_id(model::id(&annotation).unwrap_or("")))
}

fn reply(store: &Store, who: &Who, id: &str, message: &str) -> Outcome<String> {
    require_message(message)?;
    let creator = who.creator(store)?;
    let found = store.find_annotation(id)?;
    let target_id = model::id(&found.annotation)
        .ok_or_else(|| Error::message("the stored annotation has no id"))?;
    let mut annotation = model::new_reply(&creator, message, target_id);
    // Replies sort by `created`, so a reply made in the same second as the
    // annotation or a sibling it follows must still sort after it. The caller
    // holds the write lock, so no sibling lands between this read and the write.
    let records = store.read_document(&found.document)?;
    let latest = records
        .annotations
        .iter()
        .filter(|a| model::id(a) == Some(target_id) || model::target_id(a) == Some(target_id))
        .filter_map(model::created)
        .max();
    let created = after(
        latest,
        model::created(&annotation).unwrap_or("").to_string(),
    );
    let new_id = model::id(&annotation).unwrap_or("").to_string();
    model::stamp(&mut annotation, &new_id, &created);
    store.write_annotation(&found.document, &annotation, None)?;
    Ok(model::short_id(model::id(&annotation).unwrap_or("")))
}

// ---- reading ----

fn list(store: &Store, file: &str, filter: StateFilter, json: bool) -> Outcome {
    let key = store.document_key(file)?;
    let (markdown, _) = read_text(store, &key)?;
    let mut threads = threads::build(store, &key, &markdown)?;
    threads.retain(|t| filter.allows(t["state"].as_str().unwrap_or("open")));
    if json {
        emit(&to_json(&Value::Array(threads)));
    } else {
        warn_on_conflicts(&threads);
        emit(&output::list_text(&threads, &key));
    }
    Ok(())
}

fn show(store: &Store, id: &str, json: bool) -> Outcome {
    let found = store.find_annotation(id)?;
    let key = found.document;
    let (markdown, _) = read_text(store, &key)?;
    let threads = threads::build(store, &key, &markdown)?;
    let full_id = model::id(&found.annotation).unwrap_or("");
    let thread = threads::find(&threads, full_id)
        .ok_or_else(|| Error::message("the annotation is not part of any thread"))?;
    if json {
        emit(&to_json(thread));
    } else {
        warn_on_conflicts(std::slice::from_ref(thread));
        emit(&output::show_text(thread, &key));
    }
    Ok(())
}

fn to_json(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("a JSON value always serialises");
    text.push('\n');
    text
}

/// Design 5: after a sync, two clones may have decided one suggestion both ways.
/// The fold result stands, and a person is told.
fn warn_on_conflicts(threads: &[Value]) {
    for thread in threads {
        let states: Vec<&str> = thread["stateChanges"]
            .as_array()
            .map_or(&[][..], |v| v)
            .iter()
            .filter_map(model::state)
            .collect();
        if states.contains(&"accepted") && states.contains(&"rejected") {
            let id = model::short_id(thread["annotation"]["id"].as_str().unwrap_or(""));
            eprintln!(
                "warning: suggestion {id} was both accepted and rejected, probably in two clones; the latest change stands"
            );
        }
    }
}

// ---- state changes ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Resolve,
    Reopen,
    Accept,
    Reject,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Resolve => "resolve",
            Action::Reopen => "reopen",
            Action::Accept => "accept",
            Action::Reject => "reject",
        }
    }

    fn target_state(self) -> &'static str {
        match self {
            Action::Resolve => "resolved",
            Action::Reopen => "open",
            Action::Accept => "accepted",
            Action::Reject => "rejected",
        }
    }
}

/// The transition table of design 5. The error says why a transition is refused.
fn check_transition(action: Action, motivation: Option<&str>, state: &str, id: &str) -> Result<()> {
    let short = model::short_id(id);
    let refuse = |why: &str| {
        Err(Error::message(format!(
            "cannot {} {short}: {why}",
            action.verb()
        )))
    };
    match motivation {
        Some("replying") => {
            return refuse(
                "a reply has no state of its own, so change the thread's first annotation",
            )
        }
        Some("editing") if state == "accepted" => {
            return refuse(
                "an accepted suggestion is final, because its edit is already in the file",
            )
        }
        _ => {}
    }
    let suggestion = motivation == Some("editing");
    match action {
        Action::Resolve if suggestion => {
            refuse("a suggestion is accepted or rejected, not resolved")
        }
        Action::Resolve if state != "open" => refuse(&format!("it is {state}, not open")),
        Action::Accept | Action::Reject if !suggestion => {
            refuse("only a suggestion is accepted or rejected, and a comment is resolved")
        }
        Action::Accept | Action::Reject if state != "open" => {
            refuse(&format!("it is {state}, not open"))
        }
        Action::Reopen if state == "open" => refuse("it is already open"),
        Action::Reopen if !suggestion && state != "resolved" => refuse("it is not resolved"),
        Action::Reopen if suggestion && state != "rejected" => refuse("it is not rejected"),
        _ => Ok(()),
    }
}

/// The time one second after `latest`, when `now` is not already later.
///
/// `created` has whole-second resolution, so two records made in one second
/// would be ordered by their random ids, and a reject followed at once by a
/// reopen could fold to `rejected`. Stepping past the latest record keeps the
/// order the user acted in. A burst of commands can run a few seconds ahead of
/// the clock, which the format allows.
fn after(latest: Option<&str>, now: String) -> String {
    let Some(latest) = latest.filter(|l| *l >= now.as_str()) else {
        return now;
    };
    match step_second(latest) {
        Some(next) => next,
        None => now,
    }
}

/// `latest` plus one second, for the `YYYY-MM-DDTHH:MM:SSZ` form `now_utc` writes.
fn step_second(latest: &str) -> Option<String> {
    use time::format_description::well_known::Rfc3339;
    use time::{Date, Duration, Month, PrimitiveDateTime, Time};
    let number = |range: std::ops::Range<usize>| latest.get(range)?.parse::<u32>().ok();
    let date = Date::from_calendar_date(
        latest.get(0..4)?.parse().ok()?,
        Month::try_from(number(5..7)? as u8).ok()?,
        number(8..10)? as u8,
    )
    .ok()?;
    let time = Time::from_hms(
        number(11..13)? as u8,
        number(14..16)? as u8,
        number(17..19)? as u8,
    )
    .ok()?;
    let next = PrimitiveDateTime::new(date, time).assume_utc() + Duration::seconds(1);
    next.format(&Rfc3339).ok()
}

fn order_after_earlier_changes(
    store: &Store,
    key: &str,
    annotation_id: &str,
    change: &mut Value,
) -> Result<()> {
    let records = store.read_document(key)?;
    let latest = records
        .states
        .iter()
        .filter(|s| model::state_annotation(s) == Some(annotation_id))
        .filter_map(model::created)
        .max();
    let created = after(latest, model::created(change).unwrap_or("").to_string());
    let id = model::id(change).unwrap_or("").to_string();
    model::stamp(change, &id, &created);
    Ok(())
}

fn current_state(store: &Store, key: &str, annotation: &Value) -> Result<String> {
    let records = store.read_document(key)?;
    Ok(threads::fold_state(
        model::id(annotation).unwrap_or(""),
        &records.states,
    ))
}

/// Runs a state change with the write lock held from the read of the current
/// state to the write of the new one, and for `accept` around the edit of the
/// file too. Within one repository two processes then cannot both find an
/// annotation open and both decide it, and two `accept`s on one file cannot
/// each write over the other's edit. Between clones, parallel decisions stay
/// possible, and the fold of design 5 settles them.
fn decide(store: &Store, who: &Who, id: &str, action: Action) -> Outcome {
    store.with_lock(|| match action {
        Action::Accept => accept(store, who, id),
        _ => change_state(store, who, id, action),
    })
}

fn change_state(store: &Store, who: &Who, id: &str, action: Action) -> Outcome {
    let creator = who.creator(store)?;
    let found = store.find_annotation(id)?;
    let full_id = model::id(&found.annotation).unwrap_or("").to_string();
    let state = current_state(store, &found.document, &found.annotation)?;
    check_transition(
        action,
        model::motivation(&found.annotation),
        &state,
        &full_id,
    )?;
    let mut change = model::new_state_change(&creator, &full_id, action.target_state(), None);
    order_after_earlier_changes(store, &found.document, &full_id, &mut change)?;
    store.write_state(&found.document, &change, None)?;
    Ok(())
}

fn accept(store: &Store, who: &Who, id: &str) -> Outcome {
    let creator = who.creator(store)?;
    let found = store.find_annotation(id)?;
    let key = found.document.clone();
    let annotation = &found.annotation;
    let full_id = model::id(annotation).unwrap_or("").to_string();
    let state = current_state(store, &key, annotation)?;
    check_transition(
        Action::Accept,
        model::motivation(annotation),
        &state,
        &full_id,
    )?;

    // The edit is applied only at a position that still holds the suggested text.
    let (markdown, _) = read_text(store, &key)?;
    let short = model::short_id(&full_id);
    let (start, end) = match Resolver::new(store, &key, &markdown).resolve(annotation) {
        Anchor::Anchored { start, end } => (start, end),
        Anchor::Changed { .. } => {
            return Err(Failure::Refused(format!(
                "cannot accept {short}: the text it quotes has changed since it was written"
            )))
        }
        Anchor::Orphaned => {
            return Err(Failure::Refused(format!(
                "cannot accept {short}: the text it quotes is gone from {key}"
            )))
        }
    };
    let replacement = model::suggestion_text(annotation)
        .ok_or_else(|| Error::message(format!("{short} has no replacement text")))?;
    let from = text::byte_offset(&markdown, start);
    let to = text::byte_offset(&markdown, end);
    let (Some(from), Some(to)) = (from, to) else {
        return Err(Error::message(format!("{short} points outside {key}")).into());
    };
    let edited = format!("{}{replacement}{}", &markdown[..from], &markdown[to..]);

    let path = store.working_path(&key);
    std::fs::write(&path, edited.as_bytes())
        .map_err(|e| Error::message(format!("cannot write {key}: {e}")))?;
    let result_blob = store.hash_file(&path)?;
    let change = model::new_state_change(&creator, &full_id, "accepted", Some(&result_blob));
    let mut change = model::with_result_start(change, start);
    order_after_earlier_changes(store, &key, &full_id, &mut change)?;
    store
        .write_state(&key, &change, Some(&result_blob))
        .map_err(|e| {
            Error::message(format!(
                "the edit is in {key} but recording it failed, so {short} is still open: {e}"
            ))
        })?;
    Ok(())
}

// ---- sync and render ----

/// `--author`, when given, also names the author of a merge commit.
fn sync(store: &Store, who: &Who, remote: &str) -> Outcome {
    let author = match &who.author {
        Some(arg) => {
            let (name, email) = parse_author(arg)?;
            Some(Creator::person(name, email))
        }
        None => None,
    };
    let report = store.sync_as(remote, author.as_ref())?;
    emit(&sync_lines(&report, remote));
    Ok(())
}

fn sync_lines(report: &SyncReport, remote: &str) -> String {
    let mut lines = Vec::new();
    if report.tip.is_none() {
        lines.push(format!(
            "nothing to sync: there is no comments branch here or on {remote}"
        ));
    }
    if report.fast_forwarded {
        lines.push(format!("updated the comments branch from {remote}"));
    }
    if report.merged {
        lines.push(format!(
            "merged the comments from {remote} with the local ones"
        ));
    }
    if report.pushed {
        lines.push(format!("pushed the comments branch to {remote}"));
    }
    if lines.is_empty() {
        lines.push(format!("the comments branch is up to date with {remote}"));
    }
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn render_file(store: &Store, file: &str, output: Option<&Path>, source: bool) -> Outcome {
    let key = store.document_key(file)?;
    let (markdown, _) = read_text(store, &key)?;
    let threads = threads::build(store, &key, &markdown)?;
    let page = if source {
        render::render_source_page(&markdown, &threads)
    } else {
        render::render_page(&markdown, &threads)
    };
    match output {
        Some(path) => {
            let path: PathBuf = path.to_path_buf();
            std::fs::write(&path, page)
                .map_err(|e| Error::message(format!("cannot write {}: {e}", path.display())))?;
        }
        None => emit(&page),
    }
    Ok(())
}
