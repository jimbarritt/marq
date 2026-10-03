//! T-07: parallel writes lose no annotation (mission M-COMMENTS, design 2, 5, 7, 8).
//!
//! Every test here starts several real processes of the built binary at once,
//! in temporary repositories with a local bare repository as the remote, because
//! that is what a person or an agent does. Threads inside one process, as in
//! `tests/store.rs`, cannot show what the advisory lock does between processes,
//! or what is left behind when a process dies.

mod common;

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Barrier;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_marq-comments");
const DOC: &str = "doc/plan.md";
const LINES: usize = 40;
const SYNC_ATTEMPTS: usize = marq_comments::store::SYNC_ATTEMPTS as usize;

/// Line N holds the word `wordN` once, so `--line N --text wordN` always finds it.
fn doc_text() -> String {
    (1..=LINES)
        .map(|i| format!("Line {i} speaks of word{i} and more.\n"))
        .collect()
}

fn word(line: usize) -> String {
    format!("word{line}")
}

// ---- running the binary ----

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Run {
    fn ok(self) -> Run {
        assert_eq!(
            self.code, 0,
            "expected success; stdout: {}; stderr: {}",
            self.out, self.err
        );
        self
    }

    fn id(self) -> String {
        let id = self.ok().out.trim().to_string();
        assert_eq!(id.len(), 8, "not an id: {id:?}");
        id
    }
}

/// The binary in `dir`, with this machine's git configuration switched off and
/// no identity in the environment, so the author comes from each clone's config.
fn command(dir: &Path, args: &[&str]) -> Command {
    common::init_env();
    let mut command = Command::new(BIN);
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("EMAIL");
    command
}

fn finish(output: std::process::Output) -> Run {
    Run {
        code: output.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn marq(dir: &Path, args: &[&str]) -> Run {
    finish(command(dir, args).output().expect("the binary starts"))
}

fn comment(dir: &Path, file: &str, line: usize, message: &str) -> String {
    let line_arg = line.to_string();
    let word = word(line);
    marq(
        dir,
        &[
            "comment", file, "--line", &line_arg, "--text", &word, "-m", message,
        ],
    )
    .id()
}

fn comment_line(dir: &Path, line: usize, message: &str) -> String {
    comment(dir, DOC, line, message)
}

fn suggest(dir: &Path, line: usize, replacement: &str) -> String {
    let line_arg = line.to_string();
    let word = word(line);
    marq(
        dir,
        &[
            "suggest",
            DOC,
            "--line",
            &line_arg,
            "--text",
            &word,
            "--replace",
            replacement,
        ],
    )
    .id()
}

fn reply(dir: &Path, id: &str, message: &str) -> String {
    marq(dir, &["reply", id, "-m", message]).id()
}

fn sync(dir: &Path) -> Run {
    marq(dir, &["sync"])
}

fn sync_with_no_guessable_identity(dir: &Path) -> Run {
    finish(
        command(dir, &["sync"])
            .env("GIT_AUTHOR_NAME", "")
            .env("GIT_AUTHOR_EMAIL", "")
            .env("GIT_COMMITTER_NAME", "")
            .env("GIT_COMMITTER_EMAIL", "")
            .output()
            .expect("the binary starts"),
    )
}

/// Waits for a child up to `limit`. `None` means it is still running.
fn wait_for(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return Some(status);
        }
        if start.elapsed() > limit {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs a write that must finish: a stale lock would make it hang, so a
/// deadline turns a hang into a failure.
fn write_within(dir: &Path, args: &[&str], limit: Duration) -> Run {
    let mut child = command(dir, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    if wait_for(&mut child, limit).is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("marq-comments {args:?} did not finish within {limit:?}: a lock blocks it");
    }
    finish(child.wait_with_output().expect("output"))
}

/// Runs `job(0..n)` on `n` threads that all start at one barrier, so the
/// processes they spawn start as close to the same instant as the OS allows.
fn parallel<T: Send>(n: usize, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let barrier = Barrier::new(n);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let barrier = &barrier;
                let job = &job;
                scope.spawn(move || {
                    barrier.wait();
                    job(i)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a worker panicked"))
            .collect()
    })
}

/// A small deterministic generator, so a failing run of the kill test can be
/// repeated exactly.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }
}

// ---- repositories ----

/// A bare remote seeded with `doc/plan.md` on `main`, and clones of it, each
/// with its own git identity.
struct World {
    root: TempDir,
    remote: PathBuf,
    clones: Vec<PathBuf>,
}

impl World {
    fn new(clones: usize) -> World {
        common::init_env();
        let root = TempDir::new().expect("temp dir");
        let remote = root.path().join("remote.git");
        std::fs::create_dir(&remote).unwrap();
        common::git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        let seed = root.path().join("seed");
        std::fs::create_dir_all(seed.join("doc")).unwrap();
        common::git(&seed, &["init", "-q", "-b", "main"]);
        std::fs::write(seed.join(DOC), doc_text()).unwrap();
        common::git(&seed, &["add", "."]);
        common::git(&seed, &["commit", "-q", "-m", "first"]);
        common::git(&seed, &["remote", "add", "origin", &path(&remote)]);
        common::git(&seed, &["push", "-q", "origin", "main"]);
        let mut world = World {
            root,
            remote,
            clones: Vec::new(),
        };
        for _ in 0..clones {
            world.add_clone();
        }
        world
    }

    fn add_clone(&mut self) -> PathBuf {
        let letter = (b'a' + self.clones.len() as u8) as char;
        let name = format!("clone-{letter}");
        common::git(
            self.root.path(),
            &["clone", "-q", &path(&self.remote), &name],
        );
        let dir = self.root.path().join(&name);
        common::git(&dir, &["config", "user.name", &format!("Clone {letter}")]);
        common::git(
            &dir,
            &["config", "user.email", &format!("{letter}@example.com")],
        );
        self.clones.push(dir.clone());
        dir
    }

    /// A new clone that syncs once, to read what the remote holds.
    fn remote_view(&mut self) -> PathBuf {
        let dir = self.add_clone();
        let run = sync(&dir).ok();
        assert!(
            run.out.contains("updated the comments branch"),
            "the fresh clone did not take the remote branch: {}",
            run.out
        );
        dir
    }
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn git_dir(dir: &Path) -> PathBuf {
    PathBuf::from(common::git(dir, &["rev-parse", "--absolute-git-dir"]))
}

fn common_dir(dir: &Path) -> PathBuf {
    PathBuf::from(common::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ))
}

fn stray_indexes(dir: &Path) -> Vec<String> {
    std::fs::read_dir(git_dir(dir))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("marq-comments-index."))
        .collect()
}

fn commit_count(dir: &Path) -> usize {
    common::git(dir, &["rev-list", "--count", "md-comments"])
        .parse()
        .unwrap()
}

fn merge_count(dir: &Path) -> usize {
    common::git(
        dir,
        &["rev-list", "--min-parents=2", "--count", "md-comments"],
    )
    .parse()
    .unwrap()
}

fn root_count(dir: &Path) -> usize {
    common::git(
        dir,
        &["rev-list", "--max-parents=0", "--count", "md-comments"],
    )
    .parse()
    .unwrap()
}

fn tip(dir: &Path) -> String {
    common::git(dir, &["rev-parse", "md-comments"])
}

fn fsck(dir: &Path) {
    let out = Command::new("git")
        .args(["fsck", "--strict", "--no-progress"])
        .current_dir(dir)
        .output()
        .expect("git starts");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "git fsck failed: {stderr}");
    assert!(
        !stderr.contains("error") && !stderr.contains("missing"),
        "git fsck reported a problem: {stderr}"
    );
}

/// Every file on the branch, by path, read with one `ls-tree` and one `cat-file`.
fn branch_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let listing = Command::new("git")
        .args(["ls-tree", "-r", "-z", "md-comments"])
        .current_dir(dir)
        .output()
        .expect("git starts");
    assert!(listing.status.success());
    let mut entries = Vec::new();
    for entry in listing.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let entry = String::from_utf8(entry.to_vec()).unwrap();
        let (meta, file) = entry.split_once('\t').unwrap();
        let blob = meta.split(' ').nth(2).unwrap().to_string();
        entries.push((file.to_string(), blob));
    }
    let input: String = entries.iter().map(|(_, b)| format!("{b}\n")).collect();
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    let mut files = BTreeMap::new();
    let mut at = 0;
    for (file, _) in entries {
        let newline = out.stdout[at..].iter().position(|b| *b == b'\n').unwrap();
        let header = String::from_utf8_lossy(&out.stdout[at..at + newline]).into_owned();
        at += newline + 1;
        let size: usize = header.rsplit(' ').next().unwrap().parse().unwrap();
        files.insert(file, out.stdout[at..at + size].to_vec());
        at += size + 1;
    }
    files
}

fn validator(file: &str) -> jsonschema::Validator {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(file);
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

/// Every annotation and state change on the branch parses, validates against
/// its schema, and points at an annotation that is on the branch too: no
/// half-written record and no dangling state change or reply.
fn validate_branch(dir: &Path) {
    let annotation_schema = validator("annotation.schema.json");
    let state_schema = validator("state-change.schema.json");
    let mut annotation_ids = BTreeSet::new();
    let mut pointers = Vec::new();
    for (file, bytes) in branch_files(dir) {
        let is_annotation = file.contains("/annotations/");
        if !is_annotation && !file.contains("/states/") {
            continue;
        }
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("{file} is not valid JSON ({e})"));
        let schema = if is_annotation {
            &annotation_schema
        } else {
            &state_schema
        };
        let errors: Vec<String> = schema.iter_errors(&value).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{file} is invalid: {errors:?}");
        let id = value["id"].as_str().unwrap().to_string();
        let stem = file.rsplit('/').next().unwrap().trim_end_matches(".json");
        assert_eq!(id, format!("urn:uuid:{stem}"), "{file} holds another id");
        if is_annotation {
            if let Some(target) = value["target"].as_str() {
                pointers.push((file.clone(), target.to_string()));
            }
            annotation_ids.insert(id);
        } else {
            let about = value["marq:annotation"].as_str().unwrap().to_string();
            pointers.push((file.clone(), about));
        }
    }
    for (file, target) in pointers {
        assert!(
            annotation_ids.contains(&target),
            "{file} points at {target}, which is not on the branch"
        );
    }
}

/// The checks every test ends with on a clone.
fn check_clone(dir: &Path) {
    fsck(dir);
    validate_branch(dir);
}

// ---- reading threads ----

fn threads(dir: &Path, file: &str) -> Vec<Value> {
    let run = marq(dir, &["list", file, "--json"]).ok();
    match serde_json::from_str(&run.out).expect("list --json prints JSON") {
        Value::Array(items) => items,
        other => panic!("not an array: {other}"),
    }
}

/// What two clones must agree on: every thread with its id, folded state,
/// state changes in fold order and replies in order.
fn summary(thread: &Value) -> Value {
    json!({
        "id": thread["annotation"]["id"],
        "state": thread["state"],
        "changes": thread["stateChanges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].clone())
            .collect::<Vec<_>>(),
        "replies": thread["replies"]
            .as_array()
            .unwrap()
            .iter()
            .map(summary)
            .collect::<Vec<_>>(),
    })
}

fn summaries(dir: &Path, file: &str) -> Vec<Value> {
    threads(dir, file).iter().map(summary).collect()
}

fn short(id: &str) -> String {
    id.trim_start_matches("urn:uuid:").chars().take(8).collect()
}

/// The 8-character ids of every annotation in the threads, replies included.
fn all_ids(threads: &[Value]) -> BTreeSet<String> {
    fn walk(thread: &Value, ids: &mut BTreeSet<String>) {
        ids.insert(short(thread["annotation"]["id"].as_str().unwrap()));
        for reply in thread["replies"].as_array().unwrap() {
            walk(reply, ids);
        }
    }
    let mut ids = BTreeSet::new();
    for thread in threads {
        walk(thread, &mut ids);
    }
    ids
}

fn count_annotations(threads: &[Value]) -> usize {
    fn walk(thread: &Value) -> usize {
        1 + thread["replies"]
            .as_array()
            .unwrap()
            .iter()
            .map(walk)
            .sum::<usize>()
    }
    threads.iter().map(walk).sum()
}

fn thread_of<'a>(threads: &'a [Value], id: &str) -> &'a Value {
    threads
        .iter()
        .find(|t| short(t["annotation"]["id"].as_str().unwrap()) == id)
        .unwrap_or_else(|| panic!("no thread {id}"))
}

/// The fold of design 5, computed here from the stored records and not by the
/// binary: the last state change in (`created`, `id`) order.
fn independent_fold(thread: &Value) -> String {
    let mut changes: Vec<(String, String, String)> = thread["stateChanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["created"].as_str().unwrap().to_string(),
                s["id"].as_str().unwrap().to_string(),
                s["marq:state"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let printed: Vec<String> = changes.iter().map(|c| c.1.clone()).collect();
    changes.sort();
    let sorted: Vec<String> = changes.iter().map(|c| c.1.clone()).collect();
    assert_eq!(printed, sorted, "stateChanges are not in fold order");
    changes.last().map_or("open".to_string(), |c| c.2.clone())
}

/// Syncs the clones in the given order, each once, and fails on any error.
fn sync_round(world: &World, order: &[usize]) {
    for &i in order {
        sync(&world.clones[i]).ok();
    }
}

/// Syncs until every clone's tip equals the remote's: two rounds over all
/// clones reach that after any set of local writes.
fn converge(world: &World) {
    let all: Vec<usize> = (0..world.clones.len()).collect();
    sync_round(world, &all);
    sync_round(world, &all);
    let remote = common::git(&world.remote, &["rev-parse", "refs/heads/md-comments"]);
    for clone in &world.clones {
        assert_eq!(tip(clone), remote, "{} did not converge", clone.display());
    }
}

/// Every clone lists the same threads, and the same as the remote holds.
fn assert_same_everywhere(world: &mut World, file: &str) -> Vec<Value> {
    let first = summaries(&world.clones[0], file);
    for clone in &world.clones[1..] {
        assert_eq!(
            summaries(clone, file),
            first,
            "{} differs from {}",
            clone.display(),
            world.clones[0].display()
        );
    }
    let view = world.remote_view();
    assert_eq!(summaries(&view, file), first, "the remote differs");
    for clone in &world.clones {
        check_clone(clone);
    }
    first
}

// ---- fake git binaries ----

fn real_git() -> PathBuf {
    let paths = std::env::var_os("PATH").expect("PATH is set");
    std::env::split_paths(&paths)
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .expect("git is on PATH")
}

/// A directory holding a `git` script that stands in front of the real one.
/// `{GIT}` in `body` is the real git.
fn fake_git(under: &Path, body: &str) -> PathBuf {
    let bin = under.join("fake-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("git");
    std::fs::write(&script, body.replace("{GIT}", &path(&real_git()))).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn path_with(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").expect("PATH is set")
    )
}

fn install_hook(dir: &Path, name: &str, body: &str) {
    let hook = git_dir(dir).join("hooks").join(name);
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, body).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
}

// ============================================================================
// 1. One clone, many processes
// ============================================================================

struct Made {
    comments: Vec<String>,
    replies: Vec<String>,
    changes: usize,
}

#[test]
fn twelve_processes_in_one_clone_lose_nothing() {
    for round in 0..3 {
        let world = World::new(1);
        let a = &world.clones[0];
        let seed = comment_line(a, 1, "the comment every worker replies to");

        let made = parallel(12, |p| {
            let mut comments = Vec::new();
            let mut replies = Vec::new();
            let mut changes = 0;
            for k in 0..5 {
                let line = 2 + (p * 5 + k) % (LINES - 1);
                comments.push(comment_line(a, line, &format!("worker {p}, comment {k}")));
            }
            match p % 3 {
                0 => {
                    replies.push(reply(a, &seed, &format!("worker {p} replies")));
                    replies.push(reply(a, &comments[0], "a reply to my own"));
                }
                1 => {
                    for verb in ["resolve", "reopen"] {
                        marq(a, &[verb, &comments[0]]).ok();
                    }
                    marq(a, &["resolve", &comments[1]]).ok();
                    changes = 3;
                }
                _ => {
                    replies.push(reply(a, &seed, &format!("worker {p} replies too")));
                    marq(a, &["resolve", &comments[1]]).ok();
                    changes = 1;
                }
            }
            Made {
                comments,
                replies,
                changes,
            }
        });

        let listed = threads(a, DOC);
        let ids = all_ids(&listed);
        let mut expected = BTreeSet::from([seed.clone()]);
        let mut writes = 1;
        for m in &made {
            expected.extend(m.comments.iter().cloned());
            expected.extend(m.replies.iter().cloned());
            writes += m.comments.len() + m.replies.len() + m.changes;
        }
        assert_eq!(
            expected.len(),
            1 + 12 * 5 + 12,
            "round {round}: ids collided"
        );
        assert_eq!(ids, expected, "round {round}: an annotation is missing");
        assert_eq!(count_annotations(&listed), expected.len());
        assert_eq!(
            thread_of(&listed, &seed)["replies"]
                .as_array()
                .unwrap()
                .len(),
            8,
            "round {round}: a reply to the seed is missing"
        );
        for (p, m) in made.iter().enumerate() {
            match p % 3 {
                1 => {
                    assert_eq!(thread_of(&listed, &m.comments[0])["state"], "open");
                    assert_eq!(thread_of(&listed, &m.comments[1])["state"], "resolved");
                }
                2 => assert_eq!(thread_of(&listed, &m.comments[1])["state"], "resolved"),
                _ => {}
            }
        }

        // One commit per write, in one line: nothing was written over.
        assert_eq!(commit_count(a), writes, "round {round}: a commit is lost");
        assert_eq!(merge_count(a), 0);
        assert_eq!(root_count(a), 1);
        check_clone(a);
        assert_eq!(stray_indexes(a), Vec::<String>::new());
        let later = write_within(
            a,
            &[
                "comment", DOC, "--line", "2", "--text", "word2", "-m", "later",
            ],
            Duration::from_secs(20),
        );
        later.ok();
        assert_eq!(commit_count(a), writes + 1);
    }
}

#[test]
fn racing_decisions_in_one_clone_take_turns() {
    let world = World::new(1);
    let a = &world.clones[0];

    // Eight processes resolve one comment: the transition is checked and
    // written under one lock, so exactly one finds it open.
    let comment = comment_line(a, 1, "resolve me once");
    let codes = parallel(8, |_| marq(a, &["resolve", &comment]).code);
    assert_eq!(codes.iter().filter(|c| **c == 0).count(), 1, "{codes:?}");
    assert!(codes.iter().all(|c| *c == 0 || *c == 1), "{codes:?}");
    let listed = threads(a, DOC);
    let changes = thread_of(&listed, &comment)["stateChanges"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(changes, 1, "a comment was resolved more than once");

    // Accept and reject race on one suggestion inside one clone: one wins,
    // and the clone never records both.
    let suggestion = suggest(a, 2, "WORD2");
    let codes = parallel(8, |i| {
        let verb = if i % 2 == 0 { "accept" } else { "reject" };
        marq(a, &[verb, &suggestion]).code
    });
    assert_eq!(codes.iter().filter(|c| **c == 0).count(), 1, "{codes:?}");
    let listed = threads(a, DOC);
    let decided = thread_of(&listed, &suggestion);
    assert_eq!(decided["stateChanges"].as_array().unwrap().len(), 1);
    let text = std::fs::read_to_string(a.join(DOC)).unwrap();
    assert_eq!(
        text.contains("WORD2"),
        decided["state"] == "accepted",
        "the file and the state disagree"
    );

    // Eight suggestions on eight lines accepted at once: each `accept` reads,
    // edits and writes the whole file, so without the lock one edit
    // overwrites another and is lost from the file while recorded as accepted.
    let suggestions: Vec<String> = (3..11)
        .map(|line| suggest(a, line, &format!("EDIT{line}")))
        .collect();
    let codes = parallel(8, |i| marq(a, &["accept", &suggestions[i]]).code);
    assert!(codes.iter().all(|c| *c == 0), "{codes:?}");
    let text = std::fs::read_to_string(a.join(DOC)).unwrap();
    for line in 3..11 {
        assert!(
            text.contains(&format!("EDIT{line}")),
            "the edit on line {line} is lost from the file:\n{text}"
        );
    }
    check_clone(a);
}

// ============================================================================
// 2. Two and three clones, syncs in different orders, syncs racing syncs
// ============================================================================

#[test]
fn syncs_racing_in_one_clone_all_succeed() {
    let world = World::new(2);
    let (a, b) = (&world.clones[0], &world.clones[1]);
    let mut ids = BTreeSet::new();
    for line in 1..4 {
        ids.insert(comment_line(b, line, "from b"));
    }
    sync(b).ok();
    // A's first comments make a root with no common ancestor with B's.
    for line in 4..7 {
        ids.insert(comment_line(a, line, "from a"));
    }
    // Each round the remote moves, A writes, and twelve syncs start in A at
    // once. Once A's remote-tracking ref exists, their fetches all update it.
    for round in 0..6 {
        ids.insert(comment_line(b, 10 + round, "b moves the remote"));
        sync(b).ok();
        ids.insert(comment_line(a, 20 + round, "a writes"));
        let runs = parallel(12, |_| sync(a));
        for run in &runs {
            assert_eq!(
                run.code, 0,
                "round {round}: a racing sync failed: {}",
                run.err
            );
        }
    }
    sync(b).ok();
    assert_eq!(all_ids(&threads(a, DOC)), ids);
    assert_eq!(summaries(a, DOC), summaries(b, DOC));
    assert_eq!(root_count(a), 2);
    check_clone(a);
}

#[test]
fn three_clones_converge_through_racing_syncs() {
    let mut world = World::new(3);
    let clones = world.clones.clone();
    let mut ids: BTreeSet<String> = BTreeSet::new();

    // Each clone writes before any sync: three unrelated root commits.
    let first: Vec<Vec<String>> = parallel(3, |c| {
        (0..3)
            .map(|k| comment_line(&clones[c], 1 + c * 3 + k, &format!("first from {c}")))
            .collect()
    });
    for list in &first {
        ids.extend(list.iter().cloned());
    }

    // Every clone syncs at once, and clone A twice: sync races sync across
    // clones and inside one clone.
    let runs = parallel(4, |i| sync(&clones[i % 3]));
    for run in &runs {
        assert_eq!(run.code, 0, "a racing first sync failed: {}", run.err);
    }
    converge(&world);
    assert_eq!(
        root_count(&clones[0]),
        3,
        "the first sync was not unrelated"
    );

    // Everyone replies to the same comment, decides on their own comments and
    // keeps writing while the other clones sync.
    let shared = first[0][0].clone();
    let suggestion = suggest(&clones[1], 20, "TWENTY");
    ids.insert(suggestion.clone());
    sync(&clones[1]).ok();
    let second: Vec<Vec<String>> = parallel(6, |i| {
        let dir = &clones[i % 3];
        if i >= 3 {
            // Syncs racing the writes and each other.
            let mut made = Vec::new();
            for _ in 0..3 {
                let run = sync(dir);
                assert_eq!(run.code, 0, "a racing sync failed: {}", run.err);
                made.push(comment_line(dir, 30 + i, "between syncs"));
            }
            return made;
        }
        let c = i;
        let mut made = vec![
            reply(dir, &shared, &format!("clone {c} replies")),
            reply(dir, &shared, &format!("clone {c} replies again")),
        ];
        let own = &first[c][1];
        marq(dir, &["resolve", own]).ok();
        if c == 0 {
            marq(dir, &["reopen", own]).ok();
        }
        made.push(comment_line(dir, 25 + c, "late comment"));
        made
    });
    for list in &second {
        ids.extend(list.iter().cloned());
    }

    // Converge through syncs in three different orders.
    sync_round(&world, &[2, 1, 0]);
    sync_round(&world, &[0, 2, 1]);
    sync_round(&world, &[1, 0, 2]);
    converge(&world);

    let agreed = assert_same_everywhere(&mut world, DOC);
    let listed = threads(&clones[2], DOC);
    assert_eq!(all_ids(&listed), ids, "an annotation is missing");
    assert_eq!(count_annotations(&listed), ids.len());
    assert_eq!(
        thread_of(&listed, &shared)["replies"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(thread_of(&listed, &first[0][1])["state"], "open");
    assert_eq!(thread_of(&listed, &first[1][1])["state"], "resolved");
    assert_eq!(thread_of(&listed, &first[2][1])["state"], "resolved");
    assert_eq!(agreed.len(), listed.len());
}

// ============================================================================
// 3. Parallel decisions on one annotation
// ============================================================================

const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

#[test]
fn parallel_decisions_fold_the_same_whatever_the_sync_order() {
    // The six orders one after another, then all three clones syncing at once.
    let cases: Vec<Option<[usize; 3]>> = ORDERS.iter().copied().map(Some).chain([None]).collect();
    for order in cases {
        let mut world = World::new(3);
        let clones = world.clones.clone();
        let (a, b, c) = (&clones[0], &clones[1], &clones[2]);
        let comment = comment_line(a, 1, "decide me");
        let suggestion = suggest(a, 2, "WORD2");
        converge(&world);

        // A resolves and accepts, B resolves and reopens, C rejects what A accepts.
        parallel(3, |i| match i {
            0 => {
                marq(a, &["resolve", &comment]).ok();
                marq(a, &["accept", &suggestion]).ok();
            }
            1 => {
                marq(b, &["resolve", &comment]).ok();
                marq(b, &["reopen", &comment]).ok();
            }
            _ => {
                marq(c, &["reject", &suggestion]).ok();
            }
        });

        match order {
            Some(order) => {
                sync_round(&world, &order);
                sync_round(&world, &order);
            }
            None => {
                for run in parallel(3, |i| sync(&clones[i])) {
                    assert_eq!(run.code, 0, "a racing sync failed: {}", run.err);
                }
                converge(&world);
            }
        }
        let agreed = assert_same_everywhere(&mut world, DOC);
        let listed = threads(a, DOC);
        assert_eq!(agreed, listed.iter().map(summary).collect::<Vec<_>>());
        for thread in &listed {
            assert_eq!(thread["state"], independent_fold(thread).as_str());
        }
        let decided = thread_of(&listed, &suggestion);
        let states: BTreeSet<&str> = decided["stateChanges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["marq:state"].as_str().unwrap())
            .collect();
        assert_eq!(states, BTreeSet::from(["accepted", "rejected"]));
        assert_eq!(
            thread_of(&listed, &comment)["stateChanges"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        for clone in &clones {
            let run = marq(clone, &["list", DOC]).ok();
            assert!(
                run.err.contains(&format!(
                    "suggestion {suggestion} was both accepted and rejected"
                )),
                "{}: no warning on standard error: {:?}",
                clone.display(),
                run.err
            );
            let json = marq(clone, &["list", DOC, "--json"]).ok();
            assert!(json.err.is_empty(), "JSON mode warned: {}", json.err);
        }
    }
}

#[test]
fn many_state_changes_inside_one_second_fold_identically() {
    for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
        let mut world = World::new(3);
        let clones = world.clones.clone();
        let comments: Vec<String> = (1..5)
            .map(|line| comment_line(&clones[0], line, "toggle me"))
            .collect();
        converge(&world);

        // Each clone toggles every comment six times, as fast as it can, so
        // the three clones stamp many changes with the same seconds.
        parallel(3, |c| {
            for comment in &comments {
                for k in 0..6 {
                    let verb = if k % 2 == 0 { "resolve" } else { "reopen" };
                    marq(&clones[c], &[verb, comment]).ok();
                }
            }
        });
        sync_round(&world, &order);
        sync_round(&world, &order);
        assert_same_everywhere(&mut world, DOC);

        let listed = threads(&clones[0], DOC);
        let mut ties = 0;
        for comment in &comments {
            let thread = thread_of(&listed, comment);
            let changes = thread["stateChanges"].as_array().unwrap();
            assert_eq!(changes.len(), 18);
            assert_eq!(thread["state"], independent_fold(thread).as_str());
            let mut seconds: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
            for change in changes {
                seconds
                    .entry(change["created"].as_str().unwrap())
                    .or_default()
                    .insert(change["creator"]["name"].as_str().unwrap());
            }
            ties += seconds.values().filter(|names| names.len() > 1).count();
        }
        // The case design 8 calls arbitrary was exercised: clones share seconds.
        assert!(ties > 0, "no two clones wrote in the same second");
    }
}

// ============================================================================
// 4. A remote that moves between fetch and push
// ============================================================================

/// A `pre-push` hook in one clone that, on its first `moves` runs, makes
/// another clone comment and sync, so the remote moves after the fetch and
/// before the push lands. Each run appends a line to `count`.
fn moving_hook(count: &Path, other: &Path, moves: usize) -> String {
    format!(
        "#!/bin/sh\n\
         cat > /dev/null\n\
         echo push >> '{count}'\n\
         n=$(wc -l < '{count}')\n\
         if [ \"$n\" -le {moves} ]; then\n\
           unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX\n\
           line=$((n + 10))\n\
           '{bin}' -C '{other}' comment {DOC} --line $line --text word$line -m \"moved on push $n\" > /dev/null || exit 1\n\
           '{bin}' -C '{other}' sync > /dev/null || exit 1\n\
         fi\n\
         exit 0\n",
        count = count.display(),
        other = other.display(),
        bin = BIN,
    )
}

fn pushes(count: &Path) -> usize {
    std::fs::read_to_string(count)
        .map(|t| t.lines().count())
        .unwrap_or(0)
}

#[test]
fn a_remote_that_moves_once_costs_one_retry() {
    let mut world = World::new(2);
    let (a, b) = (world.clones[0].clone(), world.clones[1].clone());
    let mut ids = BTreeSet::from([comment_line(&b, 1, "b before")]);
    sync(&b).ok();
    ids.insert(comment_line(&a, 2, "a"));
    let count = world.root.path().join("pushes");
    install_hook(&a, "pre-push", &moving_hook(&count, &b, 1));

    let run = sync(&a).ok();
    assert!(run.out.contains("pushed"), "{}", run.out);
    assert_eq!(pushes(&count), 2, "one rejected push, one that landed");
    let view = world.remote_view();
    let listed = threads(&view, DOC);
    assert_eq!(count_annotations(&listed), 3);
    assert!(all_ids(&listed).is_superset(&ids));
    check_clone(&a);
}

#[test]
fn a_remote_that_keeps_moving_fails_after_bounded_retries_and_loses_nothing() {
    let mut world = World::new(2);
    let (a, b) = (world.clones[0].clone(), world.clones[1].clone());
    comment_line(&b, 1, "b before");
    sync(&b).ok();
    let mine = comment_line(&a, 2, "a, kept through every rejection");
    let count = world.root.path().join("pushes");
    install_hook(&a, "pre-push", &moving_hook(&count, &b, 99));

    let run = sync(&a);
    assert_eq!(run.code, 1, "{}", run.out);
    assert!(run.err.contains("rejected"), "{}", run.err);
    assert_eq!(pushes(&count), SYNC_ATTEMPTS, "the retry is not bounded");
    // The local branch still holds A's comment and everything merged so far.
    let local = all_ids(&threads(&a, DOC));
    assert!(local.contains(&mine));
    check_clone(&a);

    std::fs::remove_file(git_dir(&a).join("hooks/pre-push")).unwrap();
    sync(&a).ok();
    let view = world.remote_view();
    let listed = threads(&view, DOC);
    // B's first comment, one B made from the hook on each attempt, and A's.
    assert_eq!(count_annotations(&listed), 2 + SYNC_ATTEMPTS);
    assert!(all_ids(&listed).contains(&mine));
}

#[test]
fn sync_parses_git_in_any_language() {
    let mut world = World::new(2);
    let (a, b) = (world.clones[0].clone(), world.clones[1].clone());
    // Stands in for a git with translations installed: the same exit status,
    // with the two messages `sync` reads translated unless LC_ALL is C.
    let bin = fake_git(
        world.root.path(),
        "#!/bin/sh\n\
         if [ \"$LC_ALL\" = C ]; then exec {GIT} \"$@\"; fi\n\
         err=$(mktemp)\n\
         {GIT} \"$@\" 2> \"$err\"\n\
         status=$?\n\
         sed -e \"s/couldn't find remote ref/Konnte Remote-Referenz nicht finden/\" -e 's/rejected/abgewiesen/' \"$err\" >&2\n\
         rm -f \"$err\"\n\
         exit $status\n",
    );
    let german = |dir: &Path, args: &[&str]| {
        finish(
            command(dir, args)
                .env("PATH", path_with(&bin))
                .env("LANG", "de_DE.UTF-8")
                .env_remove("LC_ALL")
                .output()
                .unwrap(),
        )
    };
    comment_line(&a, 1, "a");
    // The remote has no comments branch yet.
    german(&a, &["sync"]).ok();
    comment_line(&b, 2, "b");
    sync(&b).ok();
    comment_line(&a, 3, "a again");
    let count = world.root.path().join("pushes");
    install_hook(&a, "pre-push", &moving_hook(&count, &b, 1));
    german(&a, &["sync"]).ok();
    assert_eq!(pushes(&count), 2);
    let view = world.remote_view();
    assert_eq!(count_annotations(&threads(&view, DOC)), 4);
}

// ============================================================================
// 5. Crash safety
// ============================================================================

/// Kills a whole process group, git children included.
fn kill_group(child: &Child) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stderr(Stdio::null())
        .status();
}

#[test]
fn killed_writers_leave_a_consistent_branch() {
    let world = World::new(1);
    let a = &world.clones[0];
    let seed = comment_line(a, 1, "the seed");
    let ref_lock = common_dir(a).join("refs/heads/md-comments.lock");
    let mut rng = Lcg(7);
    let mut stale_ref_locks = 0;
    for round in 0..48 {
        // Every fourth round kills git's children too, which can leave git's
        // own ref lock behind; the others kill only marq-comments.
        let group = round % 4 == 3;
        let line = (2 + round % (LINES - 1)).to_string();
        let word = word(2 + round % (LINES - 1));
        let jobs: Vec<Vec<&str>> = vec![
            vec![
                "comment", DOC, "--line", &line, "--text", &word, "-m", "killed?",
            ],
            vec!["reply", &seed, "-m", "killed reply?"],
            vec![if round % 2 == 0 { "resolve" } else { "reopen" }, &seed],
            vec![
                "suggest",
                DOC,
                "--line",
                &line,
                "--text",
                &word,
                "--replace",
                "X",
            ],
        ];
        let mut children: Vec<Child> = jobs
            .iter()
            .map(|args| {
                let mut c = command(a, args);
                c.stdout(Stdio::null()).stderr(Stdio::null());
                if group {
                    c.process_group(0);
                }
                c.spawn().unwrap()
            })
            .collect();
        std::thread::sleep(Duration::from_millis(rng.below(80)));
        for child in &mut children {
            if group {
                kill_group(child);
            } else {
                let _ = child.kill();
            }
        }
        for mut child in children {
            let _ = child.wait();
        }
        // A git child of a killed writer may still be finishing its
        // `update-ref`; its lock goes within milliseconds. One that is still
        // there after two seconds belongs to a git that was killed.
        let start = Instant::now();
        while ref_lock.exists() && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(20));
        }
        if ref_lock.exists() {
            // Git cannot tell a stale lock from a live one, so a write must
            // stop and say which file, not hang or blame a busy branch.
            stale_ref_locks += 1;
            let run = write_within(
                a,
                &[
                    "comment", DOC, "--line", "3", "--text", "word3", "-m", "blocked",
                ],
                Duration::from_secs(20),
            );
            assert_eq!(run.code, 1);
            assert!(run.err.contains("md-comments.lock"), "{}", run.err);
            std::fs::remove_file(&ref_lock).unwrap();
        }
    }
    eprintln!("stale git ref locks after group kills: {stale_ref_locks}");

    check_clone(a);
    let before = all_ids(&threads(a, DOC));
    let fresh = write_within(
        a,
        &[
            "comment",
            DOC,
            "--line",
            "4",
            "--text",
            "word4",
            "-m",
            "after the kills",
        ],
        Duration::from_secs(20),
    )
    .id();
    assert_eq!(
        stray_indexes(a),
        Vec::<String>::new(),
        "a write under the lock removes indexes that killed writers left"
    );
    let after = all_ids(&threads(a, DOC));
    assert!(after.is_superset(&before));
    assert!(after.contains(&fresh));
    sync(a).ok();
    check_clone(a);
    let mut world = world;
    let view = world.remote_view();
    assert_eq!(all_ids(&threads(&view, DOC)), after);
}

/// A `git` that, when `MARQ_TEST_STALL` names a file, writes its pid there and
/// sleeps in place of `update-ref`, so a writer stops while it holds the lock.
const STALL_GIT: &str = "#!/bin/sh\n\
    if [ \"$1\" = update-ref ] && [ -n \"$MARQ_TEST_STALL\" ]; then\n\
      echo $$ > \"$MARQ_TEST_STALL.tmp\" && mv \"$MARQ_TEST_STALL.tmp\" \"$MARQ_TEST_STALL\"\n\
      exec sleep 60\n\
    fi\n\
    exec {GIT} \"$@\"\n";

/// Stops a writer in `holder` inside the lock, shows a writer in `waiter`
/// waits for it, kills the holder with SIGKILL, and shows the waiter then
/// finishes: the lock is shared, held, and released at process death.
fn holder_blocks_waiter_until_killed(scratch: &Path, holder: &Path, waiter: &Path) {
    let bin = fake_git(scratch, STALL_GIT);
    let marker = scratch.join("stalled");
    let mut held = command(
        holder,
        &[
            "comment", DOC, "--line", "5", "--text", "word5", "-m", "held",
        ],
    )
    .env("PATH", path_with(&bin))
    .env("MARQ_TEST_STALL", &marker)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
    let start = Instant::now();
    while !marker.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the writer never stalled"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let sleeper = std::fs::read_to_string(&marker).unwrap().trim().to_string();

    let mut waiting = command(
        waiter,
        &[
            "comment", DOC, "--line", "6", "--text", "word6", "-m", "waited",
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let early = waiting.try_wait().unwrap();
    // SIGKILL: no destructor runs, so only the kernel can release the lock.
    held.kill().unwrap();
    held.wait().unwrap();
    let finished = wait_for(&mut waiting, Duration::from_secs(20));
    let _ = Command::new("kill").args(["-KILL", &sleeper]).status();
    assert!(
        early.is_none(),
        "the second writer did not wait for the lock the first one held"
    );
    assert!(
        finished.is_some(),
        "the lock of a killed process still blocks a write"
    );
    let run = finish(waiting.wait_with_output().unwrap());
    let id = run.id();
    assert!(all_ids(&threads(waiter, DOC)).contains(&id));
}

#[test]
fn a_killed_writer_releases_the_lock() {
    let world = World::new(1);
    let a = &world.clones[0];
    comment_line(a, 1, "first");
    holder_blocks_waiter_until_killed(world.root.path(), a, a);
    check_clone(a);
}

#[test]
fn one_store_shared_by_two_threads_still_takes_turns() {
    use marq_comments::error::Error;
    use marq_comments::store::Store;
    let world = World::new(1);
    let a = &world.clones[0];
    comment_line(a, 1, "first");
    let store = Store::open(a).unwrap();
    let (held, wait) = std::sync::mpsc::channel();
    let waited = std::thread::scope(|scope| {
        let store = &store;
        scope.spawn(move || {
            store
                .with_lock(|| -> Result<(), Error> {
                    held.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(800));
                    Ok(())
                })
                .unwrap();
        });
        wait.recv().unwrap();
        // The other thread holds the lock through this same store. Taking it
        // here must wait for that thread, not pass as a nested call would.
        let start = Instant::now();
        store.with_lock(|| -> Result<(), Error> { Ok(()) }).unwrap();
        start.elapsed()
    });
    assert!(
        waited >= Duration::from_millis(500),
        "the second thread did not wait: {waited:?}"
    );
}

#[test]
fn a_stale_git_ref_lock_is_named_and_does_not_look_like_a_race() {
    let world = World::new(1);
    let a = &world.clones[0];
    comment_line(a, 1, "first");
    let lock = common_dir(a).join("refs/heads/md-comments.lock");
    std::fs::write(&lock, "").unwrap();
    let run = write_within(
        a,
        &[
            "comment", DOC, "--line", "2", "--text", "word2", "-m", "blocked",
        ],
        Duration::from_secs(20),
    );
    assert_eq!(run.code, 1);
    assert!(run.err.contains("md-comments.lock"), "{}", run.err);
    assert!(!run.err.contains("kept moving"), "{}", run.err);
    std::fs::remove_file(&lock).unwrap();
    comment_line(a, 2, "after");
}

// ============================================================================
// 6. Scale, documents, paths, directories, worktrees
// ============================================================================

#[test]
fn a_thousand_comments_list_completely_and_quickly() {
    let world = World::new(1);
    let a = &world.clones[0];
    let start = Instant::now();
    let made: Vec<Vec<String>> = parallel(20, |p| {
        (0..50)
            .map(|k| comment_line(a, 1 + (p * 50 + k) % LINES, &format!("{p}/{k}")))
            .collect()
    });
    let writing = start.elapsed();
    let expected: BTreeSet<String> = made.into_iter().flatten().collect();
    assert_eq!(expected.len(), 1000);

    let start = Instant::now();
    let listed = threads(a, DOC);
    let listing = start.elapsed();
    let start = Instant::now();
    let text = marq(a, &["list", DOC]).ok();
    let listing_text = start.elapsed();
    eprintln!(
        "1000 comments: written by 20 processes in {writing:?}; list --json {listing:?}; list {listing_text:?}"
    );
    assert_eq!(all_ids(&listed), expected);
    assert_eq!(listed.len(), 1000);
    assert!(listed
        .iter()
        .all(|t| t["anchor"]["status"] == "anchored" && t["state"] == "open"));
    assert_eq!(text.out.matches("  open  comment  ").count(), 1000);
    assert!(listing < Duration::from_secs(5), "list took {listing:?}");
    assert_eq!(commit_count(a), 1000);
    check_clone(a);
}

/// Writes records straight onto the branch with plain git, in one commit, as
/// the CLI would store them. Writing thousands through the binary takes
/// minutes; this takes seconds.
fn bulk_write(dir: &Path, records: &[Value], text: &str) {
    use marq_comments::model;
    let staging = TempDir::new().unwrap();
    let mut files = Vec::new();
    let mut paths = String::new();
    for (n, record) in records.iter().enumerate() {
        let folder = if record["type"] == "Annotation" {
            "annotations"
        } else {
            "states"
        };
        let id = model::uuid_of(record["id"].as_str().unwrap()).to_string();
        let file = staging.path().join(format!("{n}.json"));
        std::fs::write(&file, model::to_stored_json(record)).unwrap();
        paths.push_str(&format!("{}\n", file.display()));
        files.push(format!("documents/{DOC}/{folder}/{id}.json"));
    }
    let text_file = staging.path().join("text");
    std::fs::write(&text_file, text).unwrap();
    paths.push_str(&format!("{}\n", text_file.display()));
    let ids = git_with_input(dir, &["hash-object", "-w", "--stdin-paths"], &paths, &[]);
    let ids: Vec<&str> = ids.lines().collect();
    let text_blob = ids[records.len()];
    let mut info = format!("100644 {text_blob}\tdocuments/{DOC}/versions/{text_blob}\n");
    for (file, id) in files.iter().zip(&ids) {
        info.push_str(&format!("100644 {id}\t{file}\n"));
    }
    let index = staging.path().join("index");
    let env = [("GIT_INDEX_FILE", index.to_str().unwrap())];
    git_with_input(dir, &["read-tree", "md-comments"], "", &env);
    git_with_input(dir, &["update-index", "--index-info"], &info, &env);
    let tree = git_with_input(dir, &["write-tree"], "", &env);
    let parent = tip(dir);
    let commit = common::git(dir, &["commit-tree", &tree, "-p", &parent, "-m", "bulk"]);
    common::git(
        dir,
        &["update-ref", "refs/heads/md-comments", &commit, &parent],
    );
}

fn git_with_input(dir: &Path, args: &[&str], input: &str, env: &[(&str, &str)]) -> String {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn six_thousand_records_list_in_linear_time() {
    use marq_comments::{anchor, model, text};
    let world = World::new(1);
    let a = &world.clones[0];
    comment_line(a, 1, "made by the binary");
    let markdown = doc_text();
    let source_blob = git_with_input(a, &["hash-object", "--stdin"], &markdown, &[]);
    let creator = model::Creator::person("Bulk", "bulk@example.com");
    let mut records = Vec::new();
    let mut comments = Vec::new();
    for n in 0..4000 {
        let line = 1 + n % LINES;
        let (start, end) = text::find_in_line(&markdown, line, &word(line), 1).unwrap();
        let s = anchor::selectors_for_range(&markdown, start, end).unwrap();
        let target = model::Target {
            source: DOC.to_string(),
            source_blob: source_blob.clone(),
            selection: model::Selection {
                exact: s.exact,
                prefix: s.prefix,
                suffix: s.suffix,
                start: s.start,
                end: s.end,
                line: s.line,
            },
        };
        let record = model::new_comment(&creator, &format!("bulk {n}"), &target);
        comments.push(record["id"].as_str().unwrap().to_string());
        records.push(record);
    }
    for n in 0..1000 {
        records.push(model::new_reply(&creator, "bulk reply", &comments[n * 4]));
        records.push(model::new_state_change(
            &creator,
            &comments[n * 4 + 1],
            "resolved",
            None,
        ));
    }
    bulk_write(a, &records, &markdown);

    let start = Instant::now();
    let listed = threads(a, DOC);
    let took = start.elapsed();
    eprintln!("6000 records: list --json {took:?}");
    assert_eq!(listed.len(), 4001);
    assert_eq!(count_annotations(&listed), 5001);
    let resolved = listed.iter().filter(|t| t["state"] == "resolved").count();
    assert_eq!(resolved, 1000);
    // Measured on debug builds: 1.7 s, and 23.4 s before T-07 removed the
    // quadratic tree lookups and record scans.
    assert!(took < Duration::from_secs(8), "list took {took:?}");
    check_clone(a);
}

#[test]
fn two_documents_written_in_parallel_stay_apart() {
    let world = World::new(1);
    let a = &world.clones[0];
    let other = "notes/other.md";
    std::fs::create_dir_all(a.join("notes")).unwrap();
    std::fs::write(a.join(other), doc_text()).unwrap();
    let made = parallel(10, |p| {
        let file = if p % 2 == 0 { DOC } else { other };
        let ids: Vec<String> = (0..4)
            .map(|k| comment(a, file, 1 + p + k, &format!("{p}/{k}")))
            .collect();
        (file, ids)
    });
    for file in [DOC, other] {
        let expected: BTreeSet<String> = made
            .iter()
            .filter(|(f, _)| *f == file)
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect();
        let listed = threads(a, file);
        assert_eq!(all_ids(&listed), expected, "{file}");
        assert!(listed
            .iter()
            .all(|t| t["annotation"]["target"]["source"] == file));
    }
    check_clone(a);
}

#[test]
fn unusual_paths_are_written_in_parallel_and_kept_apart() {
    let world = World::new(1);
    let a = &world.clones[0];
    // `[draft] *plan?.md` read as a glob would match `d planX.md`.
    let files = [
        "notes/my plan ü.md",
        "docs/日本語 file.md",
        "odd/[draft] *plan?.md",
        "odd/d planX.md",
        "odd/comma,name.md",
        "odd/-dash.md",
    ];
    for file in files {
        let full = a.join(file);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, doc_text()).unwrap();
    }
    let made = parallel(files.len() * 2, |i| {
        let file = files[i % files.len()];
        let ids: Vec<String> = (0..2)
            .map(|k| comment(a, file, 1 + i + k, &format!("{i}/{k}")))
            .collect();
        (file, ids)
    });
    for file in files {
        let expected: BTreeSet<String> = made
            .iter()
            .filter(|(f, _)| *f == file)
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect();
        assert_eq!(expected.len(), 4);
        let listed = threads(a, file);
        assert_eq!(all_ids(&listed), expected, "{file}");
        let show = marq(a, &["show", expected.iter().next().unwrap()]).ok();
        assert!(show.out.contains(file), "{}", show.out);
        // Pathspec magic from the user's environment changes nothing.
        for var in [
            "GIT_GLOB_PATHSPECS",
            "GIT_ICASE_PATHSPECS",
            "GIT_NOGLOB_PATHSPECS",
        ] {
            let run = finish(
                command(a, &["list", file, "--json"])
                    .env(var, "1")
                    .output()
                    .unwrap(),
            )
            .ok();
            let listed: Vec<Value> = serde_json::from_str(&run.out).unwrap();
            assert_eq!(all_ids(&listed), expected, "{file} with {var}");
        }
    }
    // The same file read from its own directory.
    let listed = threads(&a.join("odd"), "[draft] *plan?.md");
    assert_eq!(listed.len(), 4);
    let keys: BTreeSet<String> = branch_files(a)
        .keys()
        .filter_map(|p| p.strip_prefix("documents/"))
        .filter_map(|p| p.split_once("/annotations/").map(|(k, _)| k.to_string()))
        .collect();
    let expected: BTreeSet<String> = files.iter().map(|f| f.to_string()).collect();
    assert_eq!(keys, expected);
    check_clone(a);
}

#[test]
fn processes_in_subdirectories_and_with_dash_c_share_one_document() {
    let world = World::new(1);
    let a = &world.clones[0];
    let outside = world.root.path();
    let a_doc = a.join("doc");
    let made = parallel(16, |i| {
        let lines = [1 + i, 20 + i];
        lines
            .iter()
            .map(|line| {
                let line_arg = line.to_string();
                let word = word(*line);
                let place = ["--line", line_arg.as_str(), "--text", word.as_str()];
                let (dir, mut args): (&Path, Vec<&str>) = match i % 4 {
                    0 => (a, vec!["comment", DOC]),
                    1 => (&a_doc, vec!["comment", "plan.md"]),
                    2 => (
                        outside,
                        vec!["-C", a_doc.to_str().unwrap(), "comment", "plan.md"],
                    ),
                    _ => (outside, vec!["-C", a.to_str().unwrap(), "comment", DOC]),
                };
                args.extend(place);
                args.extend(["-m", "from somewhere"]);
                marq(dir, &args).id()
            })
            .collect::<Vec<_>>()
    });
    let expected: BTreeSet<String> = made.into_iter().flatten().collect();
    assert_eq!(expected.len(), 32);
    assert_eq!(all_ids(&threads(a, DOC)), expected);
    assert_eq!(all_ids(&threads(&a_doc, "plan.md")), expected);
    let keys: BTreeSet<String> = branch_files(a)
        .keys()
        .filter(|p| p.contains("/annotations/"))
        .map(|p| p.split("/annotations/").next().unwrap().to_string())
        .collect();
    assert_eq!(keys, BTreeSet::from([format!("documents/{DOC}")]));
    check_clone(a);
}

#[test]
fn two_worktrees_of_one_repository_write_in_parallel_through_one_lock() {
    let world = World::new(1);
    let a = &world.clones[0];
    let wt = world.root.path().join("worktree");
    common::git(a, &["worktree", "add", "-q", "-b", "side", &path(&wt)]);
    assert!(
        wt.join(".git").is_file(),
        "a linked worktree has a .git file"
    );

    let made: Vec<String> = parallel(12, |i| {
        let dir = if i % 2 == 0 { a } else { &wt };
        comment_line(dir, 1 + i, "from a worktree")
    });
    let expected: BTreeSet<String> = made.into_iter().collect();
    assert_eq!(all_ids(&threads(a, DOC)), expected);
    assert_eq!(all_ids(&threads(&wt, DOC)), expected);
    assert_eq!(commit_count(a), 12);
    assert!(common_dir(a).join("marq-comments.lock").exists());
    assert!(
        !git_dir(&wt).join("marq-comments.lock").exists(),
        "the linked worktree took a lock of its own"
    );
    assert!(stray_indexes(a).is_empty() && stray_indexes(&wt).is_empty());
    check_clone(a);

    // A writer stopped inside the lock in the main checkout holds back a
    // writer in the linked worktree.
    holder_blocks_waiter_until_killed(world.root.path(), a, &wt);
}

#[test]
fn sync_merges_with_no_git_identity_anywhere() {
    let world = World::new(2);
    let (a, b) = (&world.clones[0], &world.clones[1]);
    for dir in [a, b] {
        common::git(dir, &["config", "--unset", "user.name"]);
        common::git(dir, &["config", "--unset", "user.email"]);
    }
    let author = |dir: &Path, line: usize, name: &str| {
        let line_arg = line.to_string();
        let word = word(line);
        marq(
            dir,
            &[
                "--author", name, "comment", DOC, "--line", &line_arg, "--text", &word, "-m", "hi",
            ],
        )
        .id()
    };
    author(a, 1, "Ann <ann@example.com>");
    author(b, 2, "Bob <bob@example.com>");
    sync(a).ok();
    // With --author, the merge commit is the author's.
    marq(b, &["--author", "Bob <bob@example.com>", "sync"]).ok();
    assert_eq!(merge_count(b), 1);
    assert_eq!(
        common::git(b, &["log", "-1", "--format=%an <%ae>", "md-comments"]),
        "Bob <bob@example.com>"
    );
    // Without it, and with no identity in git, the merge still happens.
    author(a, 3, "Ann <ann@example.com>");
    author(b, 4, "Bob <bob@example.com>");
    sync_with_no_guessable_identity(a).ok();
    sync_with_no_guessable_identity(b).ok();
    assert_eq!(
        common::git(b, &["log", "-1", "--format=%p", "md-comments"])
            .split(' ')
            .count(),
        2,
        "B's tip is not a merge"
    );
    assert_eq!(
        common::git(b, &["log", "-1", "--format=%an|%ae", "md-comments"]),
        "marq-comments|"
    );
    sync(a).ok();
    assert_eq!(summaries(a, DOC), summaries(b, DOC));
    assert_eq!(count_annotations(&threads(a, DOC)), 4);
}
