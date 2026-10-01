//! Tests for the storage layer (T-04) against real temporary git repositories
//! and a local bare repository as the remote.

mod common;

use common::{git, git_status, Remote, TestRepo, DOC_TEXT};
use marq_comments::error::Error;
use marq_comments::model::{self, Creator, Selection, Target};
use marq_comments::store::{Store, COMMENTS_REF};
use serde_json::Value;
use std::path::Path;

const KEY: &str = "doc/plan.md";

fn creator() -> Creator {
    Creator::person("Jim", "jim@example.com")
}

fn target(blob: &str) -> Target {
    Target {
        source: KEY.into(),
        source_blob: blob.into(),
        selection: Selection {
            exact: "esbuild".into(),
            prefix: "The build uses ".into(),
            suffix: " for bundling.".into(),
            start: 23,
            end: 30,
            line: None,
        },
    }
}

/// Writes a comment on `doc/plan.md` and returns its id.
fn add_comment(store: &Store, text: &str) -> String {
    let blob = store.hash_file(&store.working_path(KEY)).unwrap();
    let annotation = model::new_comment(&creator(), text, &target(&blob));
    store
        .write_annotation(KEY, &annotation, Some(&blob))
        .unwrap();
    model::id(&annotation).unwrap().to_string()
}

fn open(repo: &TestRepo) -> Store {
    Store::open(repo.path()).unwrap()
}

fn parents_of(dir: &Path, rev: &str) -> Vec<String> {
    let line = git(dir, &["rev-list", "--parents", "-n", "1", rev]);
    line.split_whitespace().skip(1).map(String::from).collect()
}

#[test]
fn first_write_creates_an_orphan_commit() {
    let repo = TestRepo::new();
    let store = open(&repo);
    assert_eq!(store.tip().unwrap(), None);
    add_comment(&store, "Why not Vite?");

    assert!(parents_of(repo.path(), COMMENTS_REF).is_empty());
    let files = repo.git(&["ls-tree", "-r", "--name-only", COMMENTS_REF]);
    assert!(files.contains("README.md"));
    assert_eq!(
        repo.git(&["cat-file", "-p", "md-comments:format.json"]),
        "{\"layout\": 1}"
    );
    // The branch shares no history with main.
    let (status, _) = git_status(repo.path(), &["merge-base", "main", COMMENTS_REF]);
    assert_eq!(status, 1);
}

#[test]
fn writes_leave_index_and_working_tree_alone() {
    let repo = TestRepo::new();
    let store = open(&repo);
    add_comment(&store, "one");
    add_comment(&store, "two");
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
    assert_eq!(repo.git(&["diff", "--cached"]), "");
    // No temporary index is left behind.
    let leftovers = std::fs::read_dir(repo.path().join(".git"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("marq-comments-index")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn staged_changes_survive_writes() {
    let repo = TestRepo::new();
    repo.write("notes.md", "staged\n");
    repo.git(&["add", "notes.md"]);
    let store = open(&repo);
    add_comment(&store, "one");
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]), "notes.md");
}

#[test]
fn stored_files_have_sorted_keys_and_a_final_newline() {
    let repo = TestRepo::new();
    let store = open(&repo);
    let id = add_comment(&store, "Why not Vite?");
    let path = format!(
        "md-comments:documents/{KEY}/annotations/{}.json",
        model::uuid_of(&id)
    );
    let raw = git_raw(repo.path(), &["cat-file", "-p", &path]);
    assert!(raw.ends_with("}\n"));
    assert!(!raw.ends_with("\n\n"));
    let keys: Vec<&str> = raw
        .lines()
        .filter(|l| l.starts_with("  \""))
        .map(|l| l.trim_start().split('"').nth(1).unwrap())
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert!(keys.len() >= 8, "top-level keys were {keys:?}");
}

fn git_raw(dir: &Path, args: &[&str]) -> String {
    common::init_env();
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn two_writes_make_two_commits_and_both_read_back() {
    let repo = TestRepo::new();
    let store = open(&repo);
    let first = add_comment(&store, "first");
    let second = add_comment(&store, "second");
    assert_eq!(repo.git(&["rev-list", "--count", COMMENTS_REF]), "2");
    assert_eq!(parents_of(repo.path(), COMMENTS_REF).len(), 1);

    let records = store.read_document(KEY).unwrap();
    assert_eq!(records.annotations.len(), 2);
    assert!(records.states.is_empty());
    let ids: Vec<&str> = records.annotations.iter().filter_map(model::id).collect();
    assert!(ids.contains(&first.as_str()) && ids.contains(&second.as_str()));
    let texts: Vec<&str> = records
        .annotations
        .iter()
        .filter_map(model::body_text)
        .collect();
    assert!(texts.contains(&"first") && texts.contains(&"second"));
    assert_eq!(
        store.read_document("other.md").unwrap().annotations.len(),
        0
    );
}

#[test]
fn commit_author_and_message_follow_the_annotation() {
    let repo = TestRepo::new();
    let store = open(&repo);
    let id = add_comment(&store, "hello");
    assert_eq!(
        repo.git(&["log", "-1", "--format=%an <%ae>|%cn <%ce>", COMMENTS_REF]),
        "Jim <jim@example.com>|Jim <jim@example.com>"
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s", COMMENTS_REF]),
        format!("comment {} on {KEY}", model::short_id(&id))
    );
}

#[test]
fn the_version_entry_resolves_to_the_exact_markdown() {
    let repo = TestRepo::new();
    let store = open(&repo);
    add_comment(&store, "hello");
    let records = store.read_document(KEY).unwrap();
    let blob = model::target_source_blob(&records.annotations[0])
        .unwrap()
        .to_string();
    let spec = format!("md-comments:documents/{KEY}/versions/{blob}");
    assert_eq!(git_raw(repo.path(), &["cat-file", "-p", &spec]), DOC_TEXT);
    assert_eq!(store.read_version(KEY, &blob).unwrap(), DOC_TEXT);
    assert!(store.read_version(KEY, "0123456789abcdef").is_err());

    // The version stays readable after the working file changes.
    repo.write(KEY, "changed\n");
    assert_eq!(store.read_version(KEY, &blob).unwrap(), DOC_TEXT);
}

#[test]
fn an_accepted_state_carries_its_result_blob_and_version() {
    let repo = TestRepo::new();
    let store = open(&repo);
    let id = add_comment(&store, "hello");
    repo.write(KEY, "after the edit\n");
    let result = store.hash_file(&store.working_path(KEY)).unwrap();
    let change = model::new_state_change(&creator(), &id, "accepted", Some(&result));
    store.write_state(KEY, &change, Some(&result)).unwrap();
    let records = store.read_document(KEY).unwrap();
    assert_eq!(records.states.len(), 1);
    assert_eq!(
        model::result_blob(&records.states[0]),
        Some(result.as_str())
    );
    assert_eq!(
        store.read_version(KEY, &result).unwrap(),
        "after the edit\n"
    );
}

#[test]
fn id_prefix_lookup_finds_none_and_ambiguous() {
    let repo = TestRepo::new();
    let store = open(&repo);
    let id = add_comment(&store, "findable");
    let uuid = model::uuid_of(&id).to_string();

    for arg in [
        id.clone(),
        uuid.clone(),
        uuid[..8].to_string(),
        uuid[..6].to_uppercase(),
    ] {
        let found = store.find_annotation(&arg).unwrap();
        assert_eq!(found.document, KEY);
        assert_eq!(model::id(&found.annotation), Some(id.as_str()));
    }

    let missing = if uuid.starts_with("ffffff") {
        "000000"
    } else {
        "ffffff"
    };
    let err = store.find_annotation(missing).unwrap_err();
    assert!(matches!(err, Error::NoMatch(_)));
    assert_eq!(err.exit_code(), 2);
    // Too short to be an id at all is a general error.
    assert_eq!(
        store.find_annotation(&uuid[..5]).unwrap_err().exit_code(),
        1
    );

    // Add comments until two share a 6-character prefix: write them with
    // chosen ids so the test does not depend on luck.
    let blob = store.hash_file(&store.working_path(KEY)).unwrap();
    for (n, tail) in ["aaaa", "bbbb"].iter().enumerate() {
        let mut a = model::new_comment(&creator(), "twin", &target(&blob));
        model::stamp(
            &mut a,
            &format!("urn:uuid:abcdef12-0000-4000-8000-0000000{n}{tail}"),
            "2026-09-29T10:00:00Z",
        );
        store.write_annotation(KEY, &a, None).unwrap();
    }
    let err = store.find_annotation("abcdef").unwrap_err();
    match &err {
        Error::Ambiguous { candidates, .. } => assert_eq!(candidates.len(), 2),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    assert_eq!(err.exit_code(), 2);
    assert!(store
        .find_annotation("abcdef12-0000-4000-8000-00000000aaaa")
        .is_ok());
}

#[test]
fn keys_are_repo_relative_from_a_subdirectory() {
    let repo = TestRepo::new();
    let sub = repo.path().join("doc");
    let store = Store::open(&sub).unwrap();
    assert_eq!(store.prefix(), "doc/");
    assert_eq!(store.document_key("plan.md").unwrap(), KEY);
    assert_eq!(store.document_key("./plan.md").unwrap(), KEY);
    assert_eq!(store.document_key("../doc/plan.md").unwrap(), KEY);
    assert_eq!(store.document_key("../README.md").unwrap(), "README.md");
    assert!(store.document_key("../../escape.md").is_err());
    let absolute = repo.path().join("doc/plan.md");
    assert_eq!(
        store.document_key(&absolute.to_string_lossy()).unwrap(),
        KEY
    );

    let key = store.document_key("plan.md").unwrap();
    let blob = store.hash_file(&store.working_path(&key)).unwrap();
    let a = model::new_comment(&creator(), "from a subdirectory", &target(&blob));
    store.write_annotation(&key, &a, Some(&blob)).unwrap();

    let at_root = open(&repo);
    assert_eq!(at_root.prefix(), "");
    assert_eq!(at_root.read_document(KEY).unwrap().annotations.len(), 1);
    assert_eq!(at_root.list_documents().unwrap(), vec![KEY.to_string()]);
}

#[test]
fn sync_fast_forwards() {
    let (remote, a) = Remote::with_first_clone();
    let b = TestRepo::clone_of(remote.path());
    let (sa, sb) = (open(&a), open(&b));

    add_comment(&sa, "from a");
    let report = sa.sync("origin").unwrap();
    assert!(report.pushed && !report.remote_has_branch);
    assert_eq!(
        remote.git(&["rev-parse", COMMENTS_REF]),
        sa.tip().unwrap().unwrap()
    );

    // B has no branch: it is created at the remote tip.
    let report = sb.sync("origin").unwrap();
    assert!(report.fast_forwarded && !report.merged && !report.pushed);
    assert_eq!(sb.read_document(KEY).unwrap().annotations.len(), 1);

    // A moves ahead, B fast-forwards.
    add_comment(&sa, "second from a");
    sa.sync("origin").unwrap();
    let report = sb.sync("origin").unwrap();
    assert!(report.fast_forwarded && !report.merged);
    assert_eq!(sb.read_document(KEY).unwrap().annotations.len(), 2);

    // Nothing to do when both agree.
    let report = sb.sync("origin").unwrap();
    assert!(!report.fast_forwarded && !report.merged && !report.pushed);
    assert_eq!(repo_status(&a), "");
    assert_eq!(repo_status(&b), "");
}

fn repo_status(repo: &TestRepo) -> String {
    repo.git(&["status", "--porcelain"])
}

#[test]
fn sync_merges_two_clones_that_each_added_annotations() {
    let (remote, a) = Remote::with_first_clone();
    let b = TestRepo::clone_of(remote.path());
    let (sa, sb) = (open(&a), open(&b));

    let a_ids = [add_comment(&sa, "a1"), add_comment(&sa, "a2")];
    let b_ids = [add_comment(&sb, "b1"), add_comment(&sb, "b2")];

    // A pushes first. B then merges the two unrelated roots and pushes.
    sa.sync("origin").unwrap();
    let report = sb.sync("origin").unwrap();
    assert!(report.merged && report.pushed);
    assert_eq!(parents_of(b.path(), COMMENTS_REF).len(), 2);
    assert_eq!(
        remote.git(&["rev-parse", COMMENTS_REF]),
        sb.tip().unwrap().unwrap()
    );

    // A fast-forwards onto the merge and sees all four.
    let report = sa.sync("origin").unwrap();
    assert!(report.fast_forwarded && !report.merged);
    for store in [&sa, &sb] {
        let records = store.read_document(KEY).unwrap();
        assert_eq!(records.annotations.len(), 4);
        let ids: Vec<&str> = records.annotations.iter().filter_map(model::id).collect();
        for id in a_ids.iter().chain(&b_ids) {
            assert!(ids.contains(&id.as_str()), "{id} was lost");
        }
    }
    assert_eq!(repo_status(&a), "");
    assert_eq!(repo_status(&b), "");
}

#[test]
fn sync_merges_diverged_clones_with_a_common_ancestor() {
    let (remote, a) = Remote::with_first_clone();
    let b = TestRepo::clone_of(remote.path());
    let (sa, sb) = (open(&a), open(&b));
    add_comment(&sa, "root");
    sa.sync("origin").unwrap();
    sb.sync("origin").unwrap();

    add_comment(&sa, "a only");
    add_comment(&sb, "b only");
    sa.sync("origin").unwrap();
    let report = sb.sync("origin").unwrap();
    assert!(report.merged && report.pushed);
    assert_eq!(parents_of(b.path(), COMMENTS_REF).len(), 2);
    assert_eq!(sb.read_document(KEY).unwrap().annotations.len(), 3);
}

#[test]
fn a_push_rejected_because_the_remote_moved_is_retried() {
    let (remote, a) = Remote::with_first_clone();
    let b = TestRepo::clone_of(remote.path());
    let mover = TestRepo::clone_of(remote.path());
    let (sa, sb, smover) = (open(&a), open(&b), open(&mover));

    add_comment(&sb, "b1");
    sb.sync("origin").unwrap();
    smover.sync("origin").unwrap();
    add_comment(&sa, "a1");

    // On A's first push, a hook moves the remote from a third clone, so the
    // push finds the remote ahead of what A fetched.
    let marker = remote.path().join("hook-ran");
    let script = format!(
        "#!/bin/sh\n\
         [ -e '{marker}' ] && exit 0\n\
         touch '{marker}'\n\
         unset GIT_QUARANTINE_PATH GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_DIR\n\
         cd '{mover}' || exit 1\n\
         tip=$(git rev-parse {COMMENTS_REF}) || exit 1\n\
         tree=$(git rev-parse \"$tip^{{tree}}\") || exit 1\n\
         new=$(git commit-tree \"$tree\" -p \"$tip\" -m moved) || exit 1\n\
         git update-ref {COMMENTS_REF} \"$new\" || exit 1\n\
         git push -q '{remote}' {COMMENTS_REF}:{COMMENTS_REF} || exit 1\n\
         exit 0\n",
        marker = marker.display(),
        mover = mover.path().display(),
        remote = remote.path().display(),
    );
    let hook = remote.path().join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let report = sa.sync("origin").unwrap();
    assert!(marker.exists(), "the hook never moved the remote");
    assert!(report.pushed);
    let tip = sa.tip().unwrap().unwrap();
    assert_eq!(remote.git(&["rev-parse", COMMENTS_REF]), tip);
    // A's tip contains the moved remote commit and both annotations.
    assert_eq!(sa.read_document(KEY).unwrap().annotations.len(), 2);
    let moved = smover.tip().unwrap().unwrap();
    let (status, _) = git_status(a.path(), &["merge-base", "--is-ancestor", &moved, &tip]);
    assert_eq!(status, 0);
}

#[test]
fn eight_threads_of_five_writes_lose_nothing() {
    let repo = TestRepo::new();
    let path = repo.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                let store = Store::open(&path).unwrap();
                (0..5)
                    .map(|n| add_comment(&store, &format!("thread {t} write {n}")))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    // Join every thread before asserting, so a failure does not delete the
    // repository under writers that are still running.
    let results: Vec<_> = handles.into_iter().map(|h| h.join()).collect();
    let mut written: Vec<String> = results
        .into_iter()
        .flat_map(|r| r.expect("a writer thread failed"))
        .collect();
    written.sort();

    let store = open(&repo);
    let records = store.read_document(KEY).unwrap();
    assert_eq!(records.annotations.len(), 40);
    let mut read: Vec<String> = records
        .annotations
        .iter()
        .filter_map(model::id)
        .map(String::from)
        .collect();
    read.sort();
    assert_eq!(read, written);

    // One commit per write, in a line.
    assert_eq!(repo.git(&["rev-list", "--count", COMMENTS_REF]), "40");
    let merges = repo.git(&["rev-list", "--merges", COMMENTS_REF]);
    assert_eq!(merges, "");
    assert_eq!(
        repo.git(&["rev-list", "--max-parents=0", COMMENTS_REF])
            .lines()
            .count(),
        1
    );
    assert_eq!(repo_status(&repo), "");
}

#[test]
fn writers_without_the_lock_retry_on_a_moved_tip() {
    let repo = TestRepo::new();
    let path = repo.path().to_path_buf();
    let handles: Vec<_> = (0..2)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                store.set_locking(false);
                for n in 0..4 {
                    add_comment(&store, &format!("thread {t} write {n}"));
                }
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join()).collect();
    for r in results {
        r.expect("a writer thread failed");
    }
    assert_eq!(open(&repo).read_document(KEY).unwrap().annotations.len(), 8);
    assert_eq!(repo.git(&["rev-list", "--count", COMMENTS_REF]), "8");
    assert_eq!(repo.git(&["rev-list", "--merges", COMMENTS_REF]), "");
}

#[test]
fn a_merge_conflict_surfaces_as_an_error() {
    let (remote, a) = Remote::with_first_clone();
    let b = TestRepo::clone_of(remote.path());
    let (sa, sb) = (open(&a), open(&b));

    // Two writes to one path with different content: what a hand edit of the
    // branch would produce, and what the CLI itself never does.
    let path = format!("documents/{KEY}/annotations/same.json");
    let one = sa.put_blob(b"{\"id\": \"one\"}\n").unwrap();
    let two = sb.put_blob(b"{\"id\": \"two\"}\n").unwrap();
    sa.write_entries(&[(path.clone(), one)], &creator(), "one")
        .unwrap();
    sb.write_entries(&[(path, two)], &creator(), "two").unwrap();

    sa.sync("origin").unwrap();
    let before = sb.tip().unwrap();
    let err = sb.sync("origin").unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        sb.tip().unwrap(),
        before,
        "a conflict must not move the branch"
    );
}

#[test]
fn sync_with_no_branch_anywhere_does_nothing() {
    let (_remote, a) = Remote::with_first_clone();
    let report = open(&a).sync("origin").unwrap();
    assert_eq!(report.tip, None);
    assert!(!report.pushed);
}

#[test]
fn stored_shapes_have_the_fields_of_design_3() {
    let c = Creator::person("Jim", "mailto:jim@example.com");
    let t = target("3b18e512dba79e4c8300dd08aeb37f8e728b8dad");

    let comment = model::new_comment(&c, "Why not Vite?", &t);
    for key in [
        "@context",
        "id",
        "type",
        "created",
        "creator",
        "generator",
        "motivation",
        "body",
        "target",
    ] {
        assert!(comment.get(key).is_some(), "comment lacks {key}");
    }
    assert_eq!(comment["type"], "Annotation");
    assert_eq!(comment["motivation"], "commenting");
    assert!(model::id(&comment).unwrap().starts_with("urn:uuid:"));
    let created = model::created(&comment).unwrap();
    assert!(
        created.ends_with('Z') && created.len() == 20,
        "created was {created}"
    );
    assert_eq!(comment["creator"]["email"], "mailto:jim@example.com");
    assert_eq!(comment["creator"]["type"], "Person");
    assert_eq!(comment["body"]["format"], "text/markdown");
    assert_eq!(comment["@context"][0], "http://www.w3.org/ns/anno.jsonld");
    assert!(comment["@context"][1]["marq"].is_string());
    assert_eq!(model::target_source(&comment), Some(KEY));
    let selectors = comment["target"]["selector"].as_array().unwrap();
    assert_eq!(selectors[0]["type"], "TextQuoteSelector");
    assert_eq!(selectors[0]["exact"], "esbuild");
    assert_eq!(selectors[1]["type"], "TextPositionSelector");
    assert_eq!(selectors[1]["start"], 23);
    assert_eq!(model::body_text(&comment), Some("Why not Vite?"));

    let mut line = t.clone();
    line.selection.line = Some((11, 12));
    let on_line = model::new_comment(&c, "line", &line);
    let selectors = on_line["target"]["selector"].as_array().unwrap();
    assert_eq!(selectors.len(), 3);
    assert_eq!(selectors[0]["type"], "FragmentSelector");
    assert_eq!(
        selectors[0]["conformsTo"],
        "http://tools.ietf.org/rfc/rfc5147"
    );
    assert_eq!(selectors[0]["value"], "line=11,12");

    let reply = model::new_reply(
        &c,
        "Obsidian plugins ship one CJS file.",
        model::id(&comment).unwrap(),
    );
    assert_eq!(reply["motivation"], "replying");
    assert_eq!(model::target_id(&reply), model::id(&comment));

    let suggestion = model::new_suggestion(&c, "five", Some("The count is five."), &t);
    assert_eq!(suggestion["motivation"], "editing");
    let bodies = suggestion["body"].as_array().unwrap();
    assert_eq!(bodies[0]["purpose"], "editing");
    assert_eq!(bodies[1]["purpose"], "commenting");
    assert_eq!(model::suggestion_text(&suggestion), Some("five"));
    assert_eq!(model::body_text(&suggestion), Some("The count is five."));
    let bare = model::new_suggestion(&c, "", None, &t);
    assert_eq!(bare["body"].as_array().unwrap().len(), 1);
    assert_eq!(model::body_text(&bare), None);

    let agent = model::new_state_change(
        &Creator::agent("Bot", "bot@example.com"),
        "urn:uuid:x",
        "resolved",
        None,
    );
    assert_eq!(agent["type"], "marq:StateChange");
    assert_eq!(agent["creator"]["type"], "Software");
    assert_eq!(model::state(&agent), Some("resolved"));
    assert_eq!(model::state_annotation(&agent), Some("urn:uuid:x"));
    assert!(agent.get("marq:resultBlob").is_none());
    let accepted = model::new_state_change(&c, "urn:uuid:x", "accepted", Some("abc123"));
    assert_eq!(model::result_blob(&accepted), Some("abc123"));
    assert_eq!(model::creator(&accepted), Some(c));
}

#[test]
fn stored_json_keeps_unknown_fields_and_sorts_keys() {
    let source: Value = serde_json::from_str(
        r#"{"zeta": 1, "id": "urn:uuid:a", "extra": {"b": [1, {"y": 1, "x": 2}], "a": null}}"#,
    )
    .unwrap();
    let text = model::to_stored_json(&source);
    assert!(text.ends_with("}\n"));
    assert!(text.find("\"extra\"").unwrap() < text.find("\"id\"").unwrap());
    assert!(text.find("\"id\"").unwrap() < text.find("\"zeta\"").unwrap());
    assert!(text.find("\"x\"").unwrap() < text.find("\"y\"").unwrap());
    assert!(text.contains("\n  \"id\""));
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back, source);
    assert_eq!(model::to_stored_json(&back), text);
}

#[test]
fn id_helpers_match_full_urn_and_prefix_forms() {
    let id = "urn:uuid:4f0c2d1e-8a57-4c63-9f1b-2e6d7a90b3c4";
    assert_eq!(model::short_id(id), "4f0c2d1e");
    assert_eq!(model::uuid_of(id), "4f0c2d1e-8a57-4c63-9f1b-2e6d7a90b3c4");
    assert!(model::new_id().starts_with("urn:uuid:"));
    for arg in [
        id,
        "4f0c2d1e-8a57-4c63-9f1b-2e6d7a90b3c4",
        "4f0c2d",
        "urn:uuid:4F0C2D1E",
        "4f0c2d1e-8a",
    ] {
        assert!(model::matches_id(arg, id), "{arg}");
    }
    assert!(
        !model::matches_id("4f0c2", id),
        "five characters are too short"
    );
    assert!(!model::matches_id("4f0c2e", id));
    assert!(!model::matches_id("zzzzzzzz", id));
    assert_eq!(model::resolve_id("4f0c2d", [id]).unwrap(), id);
    assert!(matches!(
        model::resolve_id("111111", [id]),
        Err(Error::NoMatch(_))
    ));
}

#[test]
fn opening_outside_a_repository_is_a_git_error() {
    common::init_env();
    let dir = tempfile::TempDir::new().unwrap();
    let err = Store::open(dir.path()).err().expect("not a repository");
    assert!(matches!(err, Error::Git { .. }));
    assert!(err.to_string().contains("not a git repository"), "{err}");
}
