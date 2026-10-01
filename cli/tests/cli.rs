//! Runs the built `marq-comments` binary in temporary git repositories and
//! checks its output, exit codes and what it stores (design 5 and 6).
//! Every annotation and state change on the branch is validated against the
//! JSON Schemas of `cli/schema/`.

mod common;

use common::{Remote, TestRepo};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_marq-comments");
const DOC: &str = "doc/plan.md";

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Run {
    fn ok(&self) -> &Run {
        assert_eq!(self.code, 0, "expected success, stderr: {}", self.err);
        self
    }

    fn fails(&self, code: i32) -> &Run {
        assert_eq!(
            self.code, code,
            "expected exit {code}, got {}; stdout: {}; stderr: {}",
            self.code, self.out, self.err
        );
        self
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("bad JSON ({e}): {}", self.out))
    }
}

/// Runs the binary in `dir` with git's own configuration switched off.
fn marq(dir: &Path, args: &[&str]) -> Run {
    common::init_env();
    let output = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .output()
        .expect("the binary starts");
    Run {
        code: output.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A repository whose git config names Jim, so commands need no `--author`.
fn repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.git(&["config", "user.name", "Jim"]);
    repo.git(&["config", "user.email", "jim@example.com"]);
    repo
}

fn validator(file: &str) -> jsonschema::Validator {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(file);
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

/// Validates every annotation and state-change file on `md-comments`.
fn validate_branch(repo: &TestRepo) {
    let annotation = validator("annotation.schema.json");
    let state = validator("state-change.schema.json");
    let (code, _) = common::git_status(repo.path(), &["rev-parse", "--verify", "md-comments"]);
    if code != 0 {
        return;
    }
    let names = repo.git(&["ls-tree", "-r", "--name-only", "md-comments"]);
    for path in names.lines() {
        let (schema, kind) = if path.contains("/annotations/") {
            (&annotation, "annotation")
        } else if path.contains("/states/") {
            (&state, "state change")
        } else {
            continue;
        };
        let text = repo.git(&["show", &format!("md-comments:{path}")]);
        let value: Value = serde_json::from_str(&text).unwrap();
        let errors: Vec<String> = schema
            .iter_errors(&value)
            .map(|e| format!("{e} at {}", e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{kind} {path} is invalid: {errors:?}");
    }
}

fn comment_on(repo: &TestRepo, word: &str, message: &str) -> String {
    let run = marq(
        repo.path(),
        &["comment", DOC, "--line", "3", "--text", word, "-m", message],
    );
    run.ok().out.trim().to_string()
}

fn threads(repo: &TestRepo, extra: &[&str]) -> Vec<Value> {
    let mut args = vec!["list", DOC, "--json"];
    args.extend_from_slice(extra);
    match marq(repo.path(), &args).ok().json() {
        Value::Array(items) => items,
        other => panic!("not an array: {other}"),
    }
}

fn thread_of(repo: &TestRepo, id: &str) -> Value {
    threads(repo, &[])
        .into_iter()
        .find(|t| t["annotation"]["id"].as_str().unwrap().contains(id))
        .unwrap_or_else(|| panic!("no thread {id}"))
}

fn state_of(repo: &TestRepo, id: &str) -> String {
    thread_of(repo, id)["state"].as_str().unwrap().to_string()
}

fn is_short_id(text: &str) -> bool {
    text.len() == 8 && text.chars().all(|c| c.is_ascii_hexdigit())
}

// ---- creating ----

#[test]
fn a_word_comment_prints_its_id_and_stores_a_valid_annotation() {
    let repo = repo();
    let run = marq(
        repo.path(),
        &[
            "comment",
            DOC,
            "--line",
            "3",
            "--text",
            "esbuild",
            "-m",
            "Why not Vite?",
        ],
    );
    run.ok();
    assert!(is_short_id(run.out.trim()), "stdout: {:?}", run.out);
    assert_eq!(run.out.lines().count(), 1);
    assert_eq!(run.err, "");

    let t = thread_of(&repo, run.out.trim());
    let annotation = &t["annotation"];
    assert_eq!(annotation["motivation"], "commenting");
    assert_eq!(annotation["body"]["value"], "Why not Vite?");
    assert_eq!(annotation["creator"]["type"], "Person");
    assert_eq!(annotation["creator"]["name"], "Jim");
    assert_eq!(annotation["creator"]["email"], "mailto:jim@example.com");
    assert_eq!(annotation["target"]["source"], DOC);
    assert_eq!(t["state"], "open");
    assert_eq!(t["stateChanges"], serde_json::json!([]));
    assert_eq!(t["replies"], serde_json::json!([]));
    let anchor = &t["anchor"];
    assert_eq!(anchor["status"], "anchored");
    assert_eq!(anchor["text"], "esbuild");
    assert_eq!(
        (anchor["line"].as_u64(), anchor["column"].as_u64()),
        (Some(3), Some(16))
    );
    assert_eq!(
        anchor["end"].as_u64().unwrap() - anchor["start"].as_u64().unwrap(),
        7
    );
    validate_branch(&repo);
}

#[test]
fn a_comment_without_text_anchors_the_whole_line() {
    let repo = repo();
    let run = marq(repo.path(), &["comment", DOC, "--line", "3", "-m", "Check"]);
    let t = thread_of(&repo, run.ok().out.trim());
    assert_eq!(t["anchor"]["text"], "The build uses esbuild for bundling.");
    assert_eq!(t["anchor"]["column"], 1);
    let selectors = t["annotation"]["target"]["selector"].as_array().unwrap();
    assert!(selectors
        .iter()
        .any(|s| s["type"] == "FragmentSelector" && s["value"] == "line=2,3"));
    validate_branch(&repo);
}

#[test]
fn nth_picks_the_occurrence_and_range_takes_positions() {
    let repo = repo();
    repo.write(DOC, "one the two the three the\n");
    repo.git(&["commit", "-q", "-am", "words"]);
    let second = marq(
        repo.path(),
        &[
            "comment", DOC, "--line", "1", "--text", "the", "--nth", "2", "-m", "x",
        ],
    );
    let t = thread_of(&repo, second.ok().out.trim());
    assert_eq!(t["anchor"]["start"], 12);
    let ranged = marq(repo.path(), &["comment", DOC, "--range", "8:11", "-m", "y"]);
    let t = thread_of(&repo, ranged.ok().out.trim());
    assert_eq!(t["anchor"]["text"], "two");
    assert_eq!(t["anchor"]["column"], 9);
    marq(
        repo.path(),
        &[
            "comment", DOC, "--line", "1", "--text", "the", "--nth", "4", "-m", "z",
        ],
    )
    .fails(1);
    validate_branch(&repo);
}

#[test]
fn anchors_count_code_points_not_bytes() {
    let repo = repo();
    repo.write(DOC, "café 🦀 crab\n");
    repo.git(&["commit", "-q", "-am", "unicode"]);
    let id = marq(
        repo.path(),
        &["comment", DOC, "--line", "1", "--text", "crab", "-m", "x"],
    );
    let t = thread_of(&repo, id.ok().out.trim());
    assert_eq!(t["anchor"]["start"], 7);
    assert_eq!(t["anchor"]["column"], 8);
}

#[test]
fn a_suggestion_stores_the_replacement_and_the_reason() {
    let repo = repo();
    let run = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "5",
            "--text",
            "fast",
            "--replace",
            "quick",
            "-m",
            "Shorter",
        ],
    );
    let t = thread_of(&repo, run.ok().out.trim());
    let annotation = &t["annotation"];
    assert_eq!(annotation["motivation"], "editing");
    let bodies = annotation["body"].as_array().unwrap();
    assert!(bodies
        .iter()
        .any(|b| b["purpose"] == "editing" && b["value"] == "quick"));
    assert!(bodies
        .iter()
        .any(|b| b["purpose"] == "commenting" && b["value"] == "Shorter"));
    assert_eq!(t["anchor"]["text"], "fast");

    let bare = marq(
        repo.path(),
        &["suggest", DOC, "--range", "0:6", "--replace", ""],
    );
    let t = thread_of(&repo, bare.ok().out.trim());
    assert_eq!(t["annotation"]["body"].as_array().unwrap().len(), 1);
    assert_eq!(t["anchor"]["text"], "# Plan");
    validate_branch(&repo);
}

#[test]
fn bad_anchors_and_arguments_exit_1_and_write_nothing() {
    let repo = repo();
    let cases: Vec<Vec<&str>> = vec![
        vec!["comment", DOC, "--line", "2", "-m", "blank line"],
        vec!["comment", DOC, "--line", "99", "-m", "past the end"],
        vec![
            "comment", DOC, "--line", "3", "--text", "absent", "-m", "no word",
        ],
        vec!["comment", DOC, "--range", "5:5", "-m", "empty range"],
        vec!["comment", DOC, "--range", "5-9", "-m", "bad range"],
        vec!["comment", DOC, "--range", "0:9999", "-m", "long range"],
        vec!["comment", "doc/missing.md", "--line", "1", "-m", "no file"],
        vec!["comment", "../outside.md", "--line", "1", "-m", "outside"],
        vec!["comment", DOC, "-m", "no place"],
        vec![
            "comment", DOC, "--line", "3", "--range", "0:4", "-m", "both",
        ],
        vec![
            "comment",
            DOC,
            "--text",
            "esbuild",
            "-m",
            "word without line",
        ],
        vec!["comment", DOC, "--line", "3", "-m", "  "],
        vec!["suggest", DOC, "--line", "3", "--replace", "x"],
        vec!["suggest", DOC, "--line", "3", "--text", "esbuild"],
        vec!["frobnicate"],
    ];
    for case in cases {
        let run = marq(repo.path(), &case);
        run.fails(1);
        assert_eq!(run.out, "", "{case:?}");
        assert!(!run.err.is_empty(), "{case:?}");
    }
    let (code, _) = common::git_status(repo.path(), &["rev-parse", "--verify", "md-comments"]);
    assert_ne!(code, 0, "a failed command created the comments branch");
}

#[test]
fn help_exits_0() {
    let repo = repo();
    let run = marq(repo.path(), &["--help"]);
    run.ok();
    assert!(run.out.contains("comment"));
}

// ---- who writes ----

#[test]
fn author_and_agent_set_the_creator() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "From a person");
    let run = marq(
        repo.path(),
        &[
            "--agent",
            "--author",
            "Claude <noreply@anthropic.com>",
            "reply",
            &id,
            "-m",
            "From software",
        ],
    );
    run.ok();
    let t = thread_of(&repo, &id);
    let creator = &t["replies"][0]["annotation"]["creator"];
    assert_eq!(creator["type"], "Software");
    assert_eq!(creator["name"], "Claude");
    assert_eq!(creator["email"], "mailto:noreply@anthropic.com");

    let named = marq(
        repo.path(),
        &[
            "comment", DOC, "--line", "5", "--author", "Ana", "-m", "No email",
        ],
    );
    let t = thread_of(&repo, named.ok().out.trim());
    assert_eq!(t["annotation"]["creator"]["name"], "Ana");
    assert!(t["annotation"]["creator"].get("email").is_none());
    validate_branch(&repo);
}

#[test]
fn no_author_is_an_error_naming_both_options() {
    let repo = TestRepo::new();
    let run = marq(repo.path(), &["comment", DOC, "--line", "3", "-m", "x"]);
    run.fails(1);
    assert!(run.err.contains("--author"), "{}", run.err);
    assert!(run.err.contains("user.name"), "{}", run.err);
    assert_eq!(run.err.trim().lines().count(), 1);
}

#[test]
fn a_malformed_author_is_an_error() {
    let repo = repo();
    for bad in ["Name <broken", "<only@email.com>", ""] {
        marq(
            repo.path(),
            &["comment", DOC, "--line", "3", "--author", bad, "-m", "x"],
        )
        .fails(1);
    }
}

// ---- reading ----

#[test]
fn the_text_output_follows_design_6_2() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "Why not Vite?\nIt is smaller.");
    let reply = marq(
        repo.path(),
        &[
            "--agent",
            "--author",
            "Claude <c@example.com>",
            "reply",
            &id,
            "-m",
            "Obsidian plugins ship one CJS file.",
        ],
    );
    let reply_id = reply.ok().out.trim().to_string();
    let out = marq(repo.path(), &["list", DOC]).ok().out.clone();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!("{id}  open  comment  {DOC}:3:16  \"esbuild\"")
    );
    assert!(lines[1].starts_with("  Jim, 20"), "{}", lines[1]);
    assert_eq!(lines[2], "  Why not Vite?");
    assert_eq!(lines[3], "  It is smaller.");
    assert!(
        lines[4].starts_with(&format!("  {reply_id}  Claude (agent), 20")),
        "{}",
        lines[4]
    );
    assert_eq!(lines[5], "    Obsidian plugins ship one CJS file.");
    assert_eq!(lines.len(), 6);
    // The time reads `2026-09-29 10:15`: a date, a space, a time to the minute.
    let when = lines[1].split(", ").nth(1).unwrap();
    assert_eq!(when.len(), 16);
    assert_eq!(&when[10..11], " ");
}

#[test]
fn a_changed_anchor_prints_changed_and_the_original() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "x");
    repo.write(
        DOC,
        "# Plan\n\nThe build uses esbuil for bundling.\n\nIt is fast.\n",
    );
    let out = marq(repo.path(), &["list", DOC]).ok().out.clone();
    assert_eq!(
        out.lines().next().unwrap(),
        format!("{id}  open  comment  {DOC}:3:16  \"esbuil\"  changed  was \"esbuild\"")
    );
    let t = thread_of(&repo, &id);
    assert_eq!(t["anchor"]["status"], "changed");
    assert_eq!(t["anchor"]["original"], "esbuild");
    assert_eq!(t["anchor"]["text"], "esbuil");
}

#[test]
fn an_orphaned_anchor_prints_orphaned_and_the_stored_quote() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "x");
    repo.write(
        DOC,
        "# Plan\n\nThe build uses  for bundling.\n\nIt is fast.\n",
    );
    let out = marq(repo.path(), &["list", DOC]).ok().out.clone();
    assert_eq!(
        out.lines().next().unwrap(),
        format!("{id}  open  comment  orphaned  \"esbuild\"")
    );
    let t = thread_of(&repo, &id);
    assert_eq!(t["anchor"], serde_json::json!({"status": "orphaned"}));
}

#[test]
fn a_suggestion_prints_its_replacement() {
    let repo = repo();
    let run = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "5",
            "--text",
            "fast",
            "--replace",
            "quick",
            "-m",
            "Shorter",
        ],
    );
    let id = run.ok().out.trim().to_string();
    let out = marq(repo.path(), &["list", DOC]).ok().out.clone();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!("{id}  open  suggestion  {DOC}:5:7  \"fast\"")
    );
    assert_eq!(lines[2], "  replace with \"quick\"");
    assert_eq!(lines[3], "  Shorter");
}

#[test]
fn list_on_a_file_with_no_comments_prints_nothing_or_an_empty_array() {
    let repo = repo();
    let text = marq(repo.path(), &["list", DOC]);
    text.ok();
    assert_eq!(text.out, "");
    let json = marq(repo.path(), &["list", DOC, "--json"]);
    assert_eq!(json.ok().json(), serde_json::json!([]));
}

#[test]
fn list_state_filters_the_root_threads() {
    let repo = repo();
    let open = comment_on(&repo, "esbuild", "open one");
    let resolved = comment_on(&repo, "bundling", "resolved one");
    let accepted = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "5",
            "--text",
            "fast",
            "--replace",
            "quick",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    let rejected = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "1",
            "--text",
            "Plan",
            "--replace",
            "Scheme",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    marq(repo.path(), &["resolve", &resolved]).ok();
    marq(repo.path(), &["accept", &accepted]).ok();
    marq(repo.path(), &["reject", &rejected]).ok();

    let ids = |state: &str| -> Vec<String> {
        threads(&repo, &["--state", state])
            .iter()
            .map(|t| t["annotation"]["id"].as_str().unwrap()[9..17].to_string())
            .collect()
    };
    assert_eq!(ids("open"), vec![open.clone()]);
    assert_eq!(ids("resolved"), vec![resolved.clone()]);
    assert_eq!(ids("accepted"), vec![accepted.clone()]);
    assert_eq!(ids("rejected"), vec![rejected.clone()]);
    assert_eq!(ids("all").len(), 4);
    assert_eq!(threads(&repo, &[]).len(), 4);
    let text = marq(repo.path(), &["list", DOC, "--state", "resolved"]);
    assert!(text
        .ok()
        .out
        .starts_with(&format!("{resolved}  resolved  comment")));
    marq(repo.path(), &["list", DOC, "--state", "bogus"]).fails(1);
}

#[test]
fn show_prints_one_thread_with_its_state_history() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "Why?");
    let other = comment_on(&repo, "bundling", "Another");
    marq(repo.path(), &["resolve", &id]).ok();
    marq(repo.path(), &["reopen", &id]).ok();
    let text = marq(repo.path(), &["show", &id]);
    let out = text.ok().out.clone();
    assert!(out.starts_with(&format!("{id}  open  comment")), "{out}");
    assert!(!out.contains(&other), "{out}");
    assert!(out.contains("state changes:"), "{out}");
    let resolved_at = out.find("resolved  Jim").expect("resolved listed");
    let open_at = out.find("open  Jim").expect("open listed");
    assert!(resolved_at < open_at, "{out}");

    let json = marq(repo.path(), &["show", &id, "--json"]);
    let t = json.ok().json();
    assert!(t.is_object());
    assert_eq!(t["annotation"]["body"]["value"], "Why?");
    let changes: Vec<&str> = t["stateChanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["marq:state"].as_str().unwrap())
        .collect();
    assert_eq!(changes, ["resolved", "open"]);
}

#[test]
fn json_has_the_shape_of_design_6_2() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "Root");
    let reply = marq(repo.path(), &["reply", &id, "-m", "Child"]);
    reply.ok();
    marq(repo.path(), &["resolve", &id]).ok();
    let t = thread_of(&repo, &id);
    let mut keys: Vec<&str> = t.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        ["anchor", "annotation", "replies", "state", "stateChanges"]
    );
    assert_eq!(t["state"], "resolved");
    assert_eq!(t["stateChanges"][0]["type"], "marq:StateChange");
    assert_eq!(
        t["stateChanges"][0]["marq:annotation"],
        t["annotation"]["id"]
    );
    // A reply carries its root's state and has the same keys.
    let child = &t["replies"][0];
    assert_eq!(child["state"], "resolved");
    assert_eq!(child["annotation"]["motivation"], "replying");
    assert!(child["replies"].as_array().unwrap().is_empty());
    assert!(child["stateChanges"].as_array().unwrap().is_empty());
}

// ---- replies ----

#[test]
fn a_reply_to_a_reply_nests() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "Root");
    let first = marq(repo.path(), &["reply", &id, "-m", "First"])
        .ok()
        .out
        .trim()
        .to_string();
    let second = marq(repo.path(), &["reply", &first, "-m", "Second"])
        .ok()
        .out
        .trim()
        .to_string();
    assert!(is_short_id(&second));
    let t = thread_of(&repo, &id);
    assert_eq!(t["replies"].as_array().unwrap().len(), 1);
    let nested = &t["replies"][0]["replies"];
    assert_eq!(nested.as_array().unwrap().len(), 1);
    assert_eq!(nested[0]["annotation"]["body"]["value"], "Second");
    assert_eq!(
        nested[0]["annotation"]["target"],
        t["replies"][0]["annotation"]["id"]
    );
    // Replies are not threads of their own in `list`.
    assert_eq!(threads(&repo, &[]).len(), 1);

    let out = marq(repo.path(), &["list", DOC]).ok().out.clone();
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[3].starts_with(&format!("  {first}  Jim")), "{out}");
    assert_eq!(lines[4], "    First");
    assert!(lines[5].starts_with(&format!("    {second}  Jim")), "{out}");
    assert_eq!(lines[6], "      Second");
    validate_branch(&repo);
}

#[test]
fn replies_keep_the_order_they_were_made_in() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "Root");
    for n in 0..4 {
        marq(repo.path(), &["reply", &id, "-m", &format!("reply {n}")]).ok();
    }
    let t = thread_of(&repo, &id);
    let bodies: Vec<&str> = t["replies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["annotation"]["body"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(bodies, ["reply 0", "reply 1", "reply 2", "reply 3"]);
}

#[test]
fn a_reply_is_stored_under_the_document_of_its_target() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "Root");
    repo.write("other/notes.md", "Some notes.\n");
    // Run from a different directory: the reply still lands under doc/plan.md.
    marq(
        &repo.path().join("other"),
        &["reply", &id, "-m", "From elsewhere"],
    )
    .ok();
    let t = thread_of(&repo, &id);
    assert_eq!(t["replies"].as_array().unwrap().len(), 1);
}

// ---- state changes ----

#[test]
fn resolve_and_reopen_a_comment() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "x");
    let before = repo.git(&["ls-tree", "-r", "--name-only", "md-comments"]);
    let run = marq(repo.path(), &["resolve", &id]);
    run.ok();
    assert_eq!((run.out.as_str(), run.err.as_str()), ("", ""));
    assert_eq!(state_of(&repo, &id), "resolved");
    marq(repo.path(), &["reopen", &id]).ok();
    assert_eq!(state_of(&repo, &id), "open");
    // Nothing is deleted: the annotation stays and each change adds a file.
    let after = repo.git(&["ls-tree", "-r", "--name-only", "md-comments"]);
    assert!(before.lines().all(|l| after.lines().any(|a| a == l)));
    assert_eq!(after.lines().filter(|l| l.contains("/states/")).count(), 2);
    validate_branch(&repo);
}

#[test]
fn state_changes_made_within_one_second_keep_their_order() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "x");
    for _ in 0..3 {
        marq(repo.path(), &["resolve", &id]).ok();
        marq(repo.path(), &["reopen", &id]).ok();
    }
    marq(repo.path(), &["resolve", &id]).ok();
    let t = thread_of(&repo, &id);
    let states: Vec<&str> = t["stateChanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["marq:state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        ["resolved", "open", "resolved", "open", "resolved", "open", "resolved"]
    );
    assert_eq!(t["state"], "resolved");
}

#[test]
fn every_refused_transition_exits_1_and_changes_nothing() {
    let repo = repo();
    let comment = comment_on(&repo, "esbuild", "a comment");
    let reply = marq(repo.path(), &["reply", &comment, "-m", "a reply"])
        .ok()
        .out
        .trim()
        .to_string();
    let suggest = |old: &str, new: &str| {
        marq(
            repo.path(),
            &[
                "suggest",
                DOC,
                "--line",
                "1",
                "--text",
                old,
                "--replace",
                new,
            ],
        )
        .ok()
        .out
        .trim()
        .to_string()
    };
    let to_accept = suggest("Plan", "Scheme");
    let to_reject = suggest("#", "##");
    marq(repo.path(), &["accept", &to_accept]).ok();
    marq(repo.path(), &["reject", &to_reject]).ok();
    let resolved = comment_on(&repo, "bundling", "to resolve");
    marq(repo.path(), &["resolve", &resolved]).ok();
    let open_suggestion = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "5",
            "--text",
            "fast",
            "--replace",
            "quick",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();

    let refused: Vec<(&str, &String)> = vec![
        // a comment
        ("resolve", &resolved),
        ("reopen", &comment),
        ("accept", &comment),
        ("reject", &comment),
        // a resolved comment
        ("accept", &resolved),
        ("reject", &resolved),
        // an open suggestion
        ("resolve", &open_suggestion),
        ("reopen", &open_suggestion),
        // an accepted suggestion is final
        ("resolve", &to_accept),
        ("reopen", &to_accept),
        ("accept", &to_accept),
        ("reject", &to_accept),
        // a rejected suggestion
        ("resolve", &to_reject),
        ("accept", &to_reject),
        ("reject", &to_reject),
        // a reply has no state
        ("resolve", &reply),
        ("reopen", &reply),
        ("accept", &reply),
        ("reject", &reply),
    ];
    let before = repo.git(&["rev-parse", "md-comments"]);
    let file = std::fs::read_to_string(repo.path().join(DOC)).unwrap();
    for (verb, id) in refused {
        let run = marq(repo.path(), &[verb, id]);
        run.fails(1);
        assert_eq!(run.out, "", "{verb} {id}");
        assert_eq!(
            run.err.trim().lines().count(),
            1,
            "{verb} {id}: {}",
            run.err
        );
        assert!(run.err.contains("cannot"), "{verb} {id}: {}", run.err);
    }
    assert_eq!(repo.git(&["rev-parse", "md-comments"]), before);
    assert_eq!(
        std::fs::read_to_string(repo.path().join(DOC)).unwrap(),
        file
    );
    let accepted = marq(repo.path(), &["reopen", &to_accept]);
    assert!(accepted.err.contains("final"), "{}", accepted.err);
    assert_eq!(state_of(&repo, &to_accept), "accepted");
    validate_branch(&repo);
}

#[test]
fn a_rejected_suggestion_reopens_and_can_then_be_accepted() {
    let repo = repo();
    let id = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "5",
            "--text",
            "fast",
            "--replace",
            "quick",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    marq(repo.path(), &["reject", &id]).ok();
    assert_eq!(state_of(&repo, &id), "rejected");
    marq(repo.path(), &["reopen", &id]).ok();
    marq(repo.path(), &["accept", &id]).ok();
    assert_eq!(state_of(&repo, &id), "accepted");
}

#[test]
fn accept_applies_the_edit_to_the_working_tree_without_committing() {
    let repo = repo();
    let head = repo.git(&["rev-parse", "HEAD"]);
    let id = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "3",
            "--text",
            "esbuild",
            "--replace",
            "Vite",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    let run = marq(repo.path(), &["accept", &id]);
    run.ok();
    assert_eq!((run.out.as_str(), run.err.as_str()), ("", ""));
    let text = std::fs::read_to_string(repo.path().join(DOC)).unwrap();
    assert_eq!(
        text,
        "# Plan\n\nThe build uses Vite for bundling.\n\nIt is fast.\n"
    );
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.git(&["status", "--porcelain"]), format!("M {DOC}"));

    let t = thread_of(&repo, &id);
    assert_eq!(t["state"], "accepted");
    let change = &t["stateChanges"][0];
    assert_eq!(change["marq:state"], "accepted");
    let blob = change["marq:resultBlob"].as_str().unwrap();
    assert_eq!(repo.git(&["hash-object", DOC]), blob);
    // The text the edit produced stays reachable from the comments branch.
    let version = format!("md-comments:documents/{DOC}/versions/{blob}");
    assert_eq!(repo.git(&["show", &version]), text.trim_end());
    validate_branch(&repo);
}

#[test]
fn accept_handles_multibyte_text_and_deletions() {
    let repo = repo();
    repo.write(DOC, "café 🦀 crab and more\n");
    repo.git(&["commit", "-q", "-am", "unicode"]);
    let id = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "1",
            "--text",
            "🦀 crab ",
            "--replace",
            "",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    marq(repo.path(), &["accept", &id]).ok();
    assert_eq!(
        std::fs::read_to_string(repo.path().join(DOC)).unwrap(),
        "café and more\n"
    );
}

#[test]
fn accept_refuses_a_suggestion_whose_text_changed_with_exit_3() {
    let repo = repo();
    let id = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "3",
            "--text",
            "esbuild",
            "--replace",
            "Vite",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    let edited = "# Plan\n\nThe build uses esbuil for bundling.\n\nIt is fast.\n";
    repo.write(DOC, edited);
    let run = marq(repo.path(), &["accept", &id]);
    run.fails(3);
    assert_eq!(run.out, "");
    assert!(run.err.contains("changed"), "{}", run.err);
    assert_eq!(
        std::fs::read_to_string(repo.path().join(DOC)).unwrap(),
        edited
    );
    assert_eq!(state_of(&repo, &id), "open");
    assert!(thread_of(&repo, &id)["stateChanges"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn accept_refuses_an_orphaned_suggestion_with_exit_3() {
    let repo = repo();
    let id = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "3",
            "--text",
            "esbuild",
            "--replace",
            "Vite",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    repo.write(
        DOC,
        "# Plan\n\nThe build uses  for bundling.\n\nIt is fast.\n",
    );
    let run = marq(repo.path(), &["accept", &id]);
    run.fails(3);
    assert!(run.err.contains("gone"), "{}", run.err);
    assert_eq!(state_of(&repo, &id), "open");
}

#[test]
fn accept_still_applies_after_unrelated_edits_move_the_text() {
    let repo = repo();
    let id = marq(
        repo.path(),
        &[
            "suggest",
            DOC,
            "--line",
            "5",
            "--text",
            "fast",
            "--replace",
            "quick",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    repo.write(
        DOC,
        "# Plan\n\nA new paragraph.\n\nThe build uses esbuild for bundling.\n\nIt is fast.\n",
    );
    marq(repo.path(), &["accept", &id]).ok();
    assert!(std::fs::read_to_string(repo.path().join(DOC))
        .unwrap()
        .ends_with("It is quick.\n"));
}

// ---- ids ----

#[test]
fn ids_match_by_prefix_full_uuid_and_urn_form() {
    let repo = repo();
    let id = comment_on(&repo, "esbuild", "x");
    let full = thread_of(&repo, &id)["annotation"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let uuid = full.trim_start_matches("urn:uuid:").to_string();
    for arg in [&id[..6], &id[..], &uuid, &full] {
        marq(repo.path(), &["show", arg]).ok();
    }
    marq(repo.path(), &["show", &id.to_uppercase()]).ok();
}

#[test]
fn an_id_that_matches_nothing_exits_2() {
    let repo = repo();
    comment_on(&repo, "esbuild", "x");
    for verb in ["show", "resolve", "reopen", "accept", "reject"] {
        let run = marq(repo.path(), &[verb, "ffffffff"]);
        run.fails(2);
        assert!(run.err.contains("ffffffff"), "{}", run.err);
    }
    marq(repo.path(), &["reply", "ffffffff", "-m", "x"]).fails(2);
}

#[test]
fn an_id_before_any_comment_exists_exits_2() {
    let repo = repo();
    marq(repo.path(), &["show", "abcdef"]).fails(2);
}

#[test]
fn an_id_that_matches_several_annotations_exits_2() {
    use marq_comments::model::{self, Creator, Selection, Target};
    use marq_comments::store::Store;
    let repo = repo();
    let store = Store::open(repo.path()).unwrap();
    let creator = Creator::person("Jim", "jim@example.com");
    let target = Target {
        source: DOC.to_string(),
        source_blob: repo.git(&["hash-object", DOC]),
        selection: Selection {
            exact: "esbuild".into(),
            prefix: String::new(),
            suffix: String::new(),
            start: 0,
            end: 0,
            line: None,
        },
    };
    for (n, suffix) in ["1", "2"].iter().enumerate() {
        let mut annotation = model::new_comment(&creator, "x", &target);
        let id = format!("urn:uuid:abcdef12-0000-4000-8000-00000000000{suffix}");
        model::stamp(&mut annotation, &id, &format!("2026-01-01T00:00:0{n}Z"));
        store.write_annotation(DOC, &annotation, None).unwrap();
    }
    let run = marq(repo.path(), &["show", "abcdef"]);
    run.fails(2);
    assert!(
        run.err.contains("abcdef12-0000-4000-8000-000000000001"),
        "{}",
        run.err
    );
    assert!(
        run.err.contains("abcdef12-0000-4000-8000-000000000002"),
        "{}",
        run.err
    );
    marq(
        repo.path(),
        &["show", "abcdef12-0000-4000-8000-000000000002"],
    )
    .ok();
}

#[test]
fn an_id_that_is_too_short_or_not_hex_exits_1() {
    let repo = repo();
    comment_on(&repo, "esbuild", "x");
    for arg in ["abc", "zzzzzzzz", ""] {
        marq(repo.path(), &["show", arg]).fails(1);
    }
}

// ---- -C ----

#[test]
fn dash_c_runs_as_if_in_that_directory() {
    let repo = repo();
    let elsewhere = tempfile::TempDir::new().unwrap();
    let dir = repo.path().to_string_lossy().into_owned();
    let run = marq(
        elsewhere.path(),
        &[
            "-C", &dir, "comment", DOC, "--line", "3", "--text", "esbuild", "-m", "via -C",
        ],
    );
    let id = run.ok().out.trim().to_string();
    assert_eq!(state_of(&repo, &id), "open");
    let listed = marq(elsewhere.path(), &["list", DOC, "-C", &dir]);
    assert!(listed.ok().out.contains(&id));
    // A relative output path resolves against -C, as it does for `git -C`.
    marq(
        elsewhere.path(),
        &["-C", &dir, "render", DOC, "-o", "page.html"],
    )
    .ok();
    assert!(repo.path().join("page.html").exists());
    marq(elsewhere.path(), &["-C", "/no/such/directory", "list", DOC]).fails(1);
}

#[test]
fn a_path_is_taken_from_the_working_directory_inside_the_repository() {
    let repo = repo();
    let doc_dir = repo.path().join("doc");
    let id = marq(
        &doc_dir,
        &[
            "comment",
            "plan.md",
            "--line",
            "3",
            "--text",
            "esbuild",
            "-m",
            "from doc/",
        ],
    )
    .ok()
    .out
    .trim()
    .to_string();
    assert_eq!(thread_of(&repo, &id)["annotation"]["target"]["source"], DOC);
    let out = marq(&doc_dir, &["list", "./plan.md"]).ok().out.clone();
    assert!(out.contains(&format!("{DOC}:3:16")), "{out}");
    marq(&doc_dir, &["resolve", &id]).ok();
    assert_eq!(state_of(&repo, &id), "resolved");
}

#[test]
fn outside_a_repository_is_an_error() {
    let bare = tempfile::TempDir::new().unwrap();
    std::fs::write(bare.path().join("a.md"), "text\n").unwrap();
    let run = marq(bare.path(), &["list", "a.md"]);
    run.fails(1);
    assert!(!run.err.is_empty());
}

// ---- sync and render ----

#[test]
fn sync_pushes_merges_and_reports_each_outcome() {
    let (remote, first) = Remote::with_first_clone();
    first.git(&["config", "user.name", "Jim"]);
    first.git(&["config", "user.email", "jim@example.com"]);
    let second = TestRepo::clone_of(remote.path());
    second.git(&["config", "user.name", "Ana"]);
    second.git(&["config", "user.email", "ana@example.com"]);

    let none = marq(first.path(), &["sync"]);
    assert!(none.ok().out.contains("nothing to sync"), "{}", none.out);

    let a = comment_on(&first, "esbuild", "from A");
    let pushed = marq(first.path(), &["sync"]);
    assert!(pushed.ok().out.contains("pushed"), "{}", pushed.out);
    let b = comment_on(&second, "bundling", "from B");
    let merged = marq(second.path(), &["sync"]);
    assert!(merged.ok().out.contains("merged"), "{}", merged.out);
    let updated = marq(first.path(), &["sync"]);
    assert!(updated.ok().out.contains("updated"), "{}", updated.out);
    let current = marq(first.path(), &["sync"]);
    assert!(current.ok().out.contains("up to date"), "{}", current.out);

    for repo in [&first, &second] {
        let ids: Vec<String> = threads(repo, &[])
            .iter()
            .map(|t| t["annotation"]["id"].as_str().unwrap()[9..17].to_string())
            .collect();
        assert!(ids.contains(&a) && ids.contains(&b), "{ids:?}");
        validate_branch(repo);
    }
    assert_eq!(
        remote.git(&["rev-parse", "md-comments"]),
        first.git(&["rev-parse", "md-comments"])
    );
}

#[test]
fn sync_with_a_missing_remote_is_an_error() {
    let repo = repo();
    comment_on(&repo, "esbuild", "x");
    marq(repo.path(), &["sync"]).fails(1);
    marq(repo.path(), &["sync", "--remote", "elsewhere"]).fails(1);
}

#[test]
fn render_writes_a_page_to_a_file_or_to_stdout() {
    let repo = repo();
    comment_on(&repo, "esbuild", "x");
    let out = repo.path().join("out.html");
    let run = marq(repo.path(), &["render", DOC, "-o", &out.to_string_lossy()]);
    assert_eq!(run.ok().out, "");
    assert!(!std::fs::read_to_string(&out).unwrap().is_empty());
    let stdout = marq(repo.path(), &["render", DOC]);
    assert!(stdout.ok().out.contains("<html") || stdout.out.contains("<!doctype"));
    marq(repo.path(), &["render", "doc/missing.md"]).fails(1);
}
