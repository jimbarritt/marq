//! The `md-comments` branch. Owned by task T-04. See design section 2.
//!
//! Every write is one commit made through a temporary index, so the user's own
//! index and working tree are never touched. No file on the branch is ever
//! modified, so two clones never conflict over a file the CLI wrote.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::model::{self, Creator};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread::ThreadId;

/// The one place the ref name is written. A later move to a custom ref changes
/// this line.
pub const COMMENTS_REF: &str = "refs/heads/md-comments";

/// How often a write retries from the read of the tip when `update-ref` finds
/// the tip moved (design 2.2).
const WRITE_ATTEMPTS: u32 = 10;
/// How often `sync` repeats from the fetch when a push is rejected (design 2.4
/// said 3). Measured by T-07: six processes in three clones syncing at once
/// had one sync rejected three times running, because each rejection means
/// another clone's push landed first, and the slowest clone can lose every
/// round. Ten attempts with a growing random pause between them stayed clear
/// of that in every run.
pub const SYNC_ATTEMPTS: u32 = 10;
const LAYOUT_VERSION: u32 = 1;
const ZERO_ID: &str = "0000000000000000000000000000000000000000";

const README: &str = "# md-comments\n\n\
This branch holds the comments, replies, suggestions and state changes that \
`marq-comments` records on the markdown files of this repository, as W3C Web \
Annotations, one JSON file each. It has no common history with the other \
branches and is never checked out. Do not edit it by hand. The format is in \
doc/comments-design.md in the marq repository.\n";

/// The records stored for one markdown document, each list ordered by
/// (`created`, `id`).
#[derive(Debug, Clone, Default)]
pub struct DocumentRecords {
    pub annotations: Vec<Value>,
    pub states: Vec<Value>,
}

/// An annotation found by id, with the document key it lives under.
#[derive(Debug, Clone)]
pub struct Found {
    pub document: String,
    pub annotation: Value,
}

/// What `sync` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// The remote had an `md-comments` branch.
    pub remote_has_branch: bool,
    /// The local branch moved forward to the remote tip, or was created at it.
    pub fast_forwarded: bool,
    /// A merge commit with two parents was made.
    pub merged: bool,
    /// The remote branch was updated by a push.
    pub pushed: bool,
    /// The local tip after the sync, if any branch exists.
    pub tip: Option<String>,
}

/// A commit identity.
#[derive(Debug, Clone)]
struct Identity {
    name: String,
    email: String,
}

/// The name a merge commit carries when neither `--author` nor git names anyone.
const FALLBACK_MERGE_NAME: &str = "marq-comments";

/// What one `update-ref` with an expected old value did.
enum RefUpdate {
    Done,
    /// The ref no longer held the expected value: another writer got there first.
    Moved,
    /// The ref still holds the expected value and git refused anyway, for
    /// example over a lock file that a killed git left behind.
    Refused(Error),
}

pub struct Store {
    git: Git,
    root: PathBuf,
    /// This worktree's git directory, where temporary indexes live.
    git_dir: PathBuf,
    /// The directory every worktree of the repository shares. The branch
    /// lives there, so the lock that guards it does too.
    common_dir: PathBuf,
    prefix: String,
    locking: bool,
    /// The thread that holds the write lock through this store, so a nested
    /// write on that thread does not take it a second time: a second `flock`
    /// on a new descriptor blocks on the first, even inside one process.
    held: Mutex<Option<ThreadId>>,
}

static INDEX_COUNTER: AtomicU64 = AtomicU64::new(0);
const INDEX_PREFIX: &str = "marq-comments-index.";

/// A temporary index file that is removed when dropped.
struct TempIndex(PathBuf);

impl TempIndex {
    fn new(git_dir: &Path) -> Self {
        let n = INDEX_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        TempIndex(git_dir.join(format!(
            "{INDEX_PREFIX}{}.{n}.{}",
            std::process::id(),
            &nonce[..8]
        )))
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Store {
    /// Opens the store for the repository containing `dir`, which may be any
    /// directory inside it. The prefix of `dir` below the repository root is
    /// kept so document keys come out repo-relative.
    pub fn open(dir: &Path) -> Result<Store> {
        let probe = Git::new(dir);
        let root = PathBuf::from(probe.run(&["rev-parse", "--show-toplevel"])?);
        let prefix = probe
            .run_bytes(&["rev-parse", "--show-prefix"], None)
            .map(|b| {
                String::from_utf8_lossy(&b)
                    .trim_end_matches('\n')
                    .to_string()
            })?;
        let git_dir = PathBuf::from(probe.run(&["rev-parse", "--absolute-git-dir"])?);
        // In a linked worktree the git directory is `.git/worktrees/<name>`,
        // but every worktree shares `refs/heads/md-comments` in the common
        // directory. A lock in the per-worktree directory let two worktrees
        // write at once (found by T-07).
        let common_dir = PathBuf::from(probe.run(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?);
        Ok(Store {
            git: Git::new(&root),
            root,
            git_dir,
            common_dir,
            prefix,
            locking: true,
            held: Mutex::new(None),
        })
    }

    /// Turns the advisory write lock off or on. Only the tests of the
    /// `update-ref` retry turn it off; every command leaves it on.
    pub fn set_locking(&mut self, on: bool) {
        self.locking = on;
    }

    /// The repository root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory the store was opened in, relative to the root, with `/`
    /// separators and a trailing `/`, or empty at the root.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The repo-relative key for a path given from the directory the store was
    /// opened in. An absolute path inside the repository is accepted too.
    pub fn document_key(&self, arg: &str) -> Result<String> {
        let path = Path::new(arg);
        let relative: PathBuf = if path.is_absolute() {
            let root = self
                .root
                .canonicalize()
                .unwrap_or_else(|_| self.root.clone());
            let inside = path
                .strip_prefix(&self.root)
                .or_else(|_| path.strip_prefix(&root))
                .map_err(|_| Error::message(format!("{arg} is outside the repository")))?;
            inside.to_path_buf()
        } else {
            Path::new(&self.prefix).join(path)
        };
        let mut parts: Vec<String> = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if parts.pop().is_none() {
                        return Err(Error::message(format!("{arg} is outside the repository")));
                    }
                }
                _ => return Err(Error::message(format!("{arg} is not a repository path"))),
            }
        }
        if parts.is_empty() {
            return Err(Error::message("a markdown file path is needed"));
        }
        Ok(parts.join("/"))
    }

    /// The working-tree path of a document key.
    pub fn working_path(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }

    /// `git hash-object -w` of a working-tree file, unfiltered so the blob holds
    /// the bytes the selector offsets were measured on. Returns the blob id.
    pub fn hash_file(&self, path: &Path) -> Result<String> {
        let path = path.to_string_lossy();
        self.git
            .run(&["hash-object", "-w", "--no-filters", "--", &path])
    }

    /// Stores bytes as a blob and returns its id.
    pub fn put_blob(&self, bytes: &[u8]) -> Result<String> {
        self.git
            .run_with_input(&["hash-object", "-w", "--stdin"], bytes)
    }

    /// The current tip of the comments branch, or `None` before the first write.
    pub fn tip(&self) -> Result<Option<String>> {
        let out = self
            .git
            .run_raw(&["rev-parse", "-q", "--verify", COMMENTS_REF], None)?;
        Ok(out.success().then(|| out.text()))
    }

    // ---- writing ----

    /// Writes an annotation under `documents/<key>/annotations/`. When
    /// `version_blob` is given, the entry `versions/<blob>` pointing at that
    /// existing markdown blob goes into the same commit. Returns the new commit.
    pub fn write_annotation(
        &self,
        key: &str,
        annotation: &Value,
        version_blob: Option<&str>,
    ) -> Result<String> {
        let id = require_id(annotation)?;
        let verb = match model::motivation(annotation) {
            Some("replying") => "reply",
            Some("editing") => "suggest",
            _ => "comment",
        };
        let message = format!("{verb} {} on {key}", model::short_id(id));
        self.write_record(key, "annotations", annotation, version_blob, &message)
    }

    /// Writes a state change under `documents/<key>/states/`. `version_blob` is
    /// the `marq:resultBlob` of an accepted suggestion, kept reachable the same
    /// way as an annotation's source version.
    pub fn write_state(
        &self,
        key: &str,
        state_change: &Value,
        version_blob: Option<&str>,
    ) -> Result<String> {
        require_id(state_change)?;
        let state = model::state(state_change).unwrap_or("state");
        let about = model::state_annotation(state_change)
            .map(model::short_id)
            .unwrap_or_default();
        let message = format!("{state} {about} on {key}");
        self.write_record(key, "states", state_change, version_blob, &message)
    }

    fn write_record(
        &self,
        key: &str,
        folder: &str,
        record: &Value,
        version_blob: Option<&str>,
        message: &str,
    ) -> Result<String> {
        let id = require_id(record)?;
        let blob = self.put_blob(model::to_stored_json(record).as_bytes())?;
        let mut entries = vec![(
            format!("documents/{key}/{folder}/{}.json", model::uuid_of(id)),
            blob,
        )];
        if let Some(version) = version_blob {
            entries.push((
                format!("documents/{key}/versions/{version}"),
                version.to_string(),
            ));
        }
        let identity = model::creator(record).map(|c| Identity {
            name: c.name,
            email: c.email,
        });
        self.commit_entries(&entries, identity.as_ref(), message)
    }

    /// Makes one commit on the comments branch that adds `entries` (path, blob
    /// id). The first commit has no parent and also writes `README.md` and
    /// `format.json`. Returns the new commit id.
    pub fn write_entries(
        &self,
        entries: &[(String, String)],
        creator: &Creator,
        message: &str,
    ) -> Result<String> {
        let identity = Identity {
            name: creator.name.clone(),
            email: creator.email.clone(),
        };
        self.commit_entries(entries, Some(&identity), message)
    }

    fn commit_entries(
        &self,
        entries: &[(String, String)],
        identity: Option<&Identity>,
        message: &str,
    ) -> Result<String> {
        self.with_lock(|| {
            self.remove_stray_indexes();
            let mut last_error = None;
            for attempt in 0..WRITE_ATTEMPTS {
                if attempt > 0 {
                    backoff(attempt);
                }
                let tip = self.tip()?;
                let index = TempIndex::new(&self.git_dir);
                let git = self
                    .git
                    .with_env("GIT_INDEX_FILE", index.0.to_string_lossy());
                match &tip {
                    Some(tip) => git.run(&["read-tree", tip])?,
                    None => git.run(&["read-tree", "--empty"])?,
                };
                let mut all: Vec<(String, String)> = Vec::new();
                if tip.is_none() {
                    let format = format!("{{\"layout\": {LAYOUT_VERSION}}}\n");
                    all.push(("README.md".into(), self.put_blob(README.as_bytes())?));
                    all.push(("format.json".into(), self.put_blob(format.as_bytes())?));
                }
                all.extend(entries.iter().cloned());
                for (path, blob) in &all {
                    git.run(&[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("100644,{blob},{path}"),
                    ])?;
                }
                let tree = git.run(&["write-tree"])?;
                let parents: Vec<&str> = tip.iter().map(String::as_str).collect();
                let commit = self.commit_tree(&tree, &parents, message, identity)?;
                let expected = tip.as_deref().unwrap_or(ZERO_ID);
                match self.update_ref(&commit, expected)? {
                    RefUpdate::Done => return Ok(commit),
                    RefUpdate::Moved => {
                        last_error = Some(Error::message(format!(
                            "the comments branch kept moving while writing, after {WRITE_ATTEMPTS} attempts"
                        )))
                    }
                    RefUpdate::Refused(e) => last_error = Some(e),
                }
            }
            Err(last_error.expect("the loop ran at least once"))
        })
    }

    /// Removes temporary indexes, and git's locks on them, that a killed
    /// writer left in this worktree's git directory. It runs only with the
    /// lock held: every writer that takes the lock makes its index while
    /// holding it, so no index found here is in use. With locking off (the
    /// retry tests of tests/store.rs) another writer may be using one, so
    /// nothing is removed.
    fn remove_stray_indexes(&self) {
        if !self.holds_lock() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.git_dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(INDEX_PREFIX)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn commit_tree(
        &self,
        tree: &str,
        parents: &[&str],
        message: &str,
        identity: Option<&Identity>,
    ) -> Result<String> {
        let mut git = self.git.clone();
        if let Some(who) = identity {
            for role in ["AUTHOR", "COMMITTER"] {
                git = git
                    .with_env(&format!("GIT_{role}_NAME"), who.name.clone())
                    .with_env(&format!("GIT_{role}_EMAIL"), who.email.clone());
            }
        }
        let mut args = vec!["commit-tree", tree];
        for parent in parents {
            args.push("-p");
            args.push(parent);
        }
        args.push("-m");
        args.push(message);
        git.run(&args)
    }

    /// `update-ref` with the expected old value. A refusal counts as `Moved`
    /// only when the ref did move. Otherwise git's own reason is kept, so a
    /// stale `md-comments.lock` is reported as that, not as a busy branch
    /// (found by T-07: it read "kept moving ... after 10 attempts").
    fn update_ref(&self, new: &str, expected: &str) -> Result<RefUpdate> {
        let out = self
            .git
            .run_raw(&["update-ref", COMMENTS_REF, new, expected], None)?;
        if out.success() {
            return Ok(RefUpdate::Done);
        }
        if self.tip()?.as_deref().unwrap_or(ZERO_ID) != expected {
            return Ok(RefUpdate::Moved);
        }
        Ok(RefUpdate::Refused(Error::Git {
            command: format!("update-ref {COMMENTS_REF} {new} {expected}"),
            stderr: out.stderr,
        }))
    }

    /// Runs `f` holding the write lock. A write inside `f` does not take the
    /// lock again. Commands use this to read the branch, check a transition and
    /// write as one step: found by T-07, eight processes that each read a
    /// comment as open before any of them wrote all resolved it, and two
    /// `accept`s that each read the file before either wrote lost one edit.
    pub fn with_lock<T, E: From<Error>>(
        &self,
        f: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        if self.holds_lock() {
            return f();
        }
        let guard = self.lock_writes()?;
        if guard.is_some() {
            *self.holder() = Some(std::thread::current().id());
        }
        let result = f();
        *self.holder() = None;
        drop(guard);
        result
    }

    /// Whether the calling thread holds the write lock through this store.
    /// Another thread sharing the store does not count: it takes the lock on
    /// its own descriptor and waits its turn.
    fn holds_lock(&self) -> bool {
        *self.holder() == Some(std::thread::current().id())
    }

    fn holder(&self) -> MutexGuard<'_, Option<ThreadId>> {
        // A panic while the mutex was held leaves a plain value behind, still usable.
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes an advisory lock shared by every `marq-comments` process on this
    /// repository and every worktree of it, held until the file drops. With 8
    /// writers each retry has about a one in eight chance, and 10 retries
    /// starve one of them often enough to lose a write (measured). The lock
    /// makes writers on one machine take turns. `update-ref` with the expected
    /// old value stays the guard for anything that does not take the lock.
    ///
    /// The kernel releases an `flock` when its process dies, however it dies,
    /// and Rust opens files close-on-exec, so a git child that outlives a
    /// killed writer does not hold it (both proven in tests/parallel.rs).
    fn lock_writes(&self) -> Result<Option<std::fs::File>> {
        self.lock_file("marq-comments.lock")
    }

    /// Takes the lock that makes syncs of this repository take turns. Found by
    /// T-07: twelve syncs started at once in one clone failed 152 times in 180,
    /// because their `git fetch`es collided on the lock of the remote-tracking
    /// ref ("unable to update local ref"). It is a separate file from the write
    /// lock, so a local write never waits for the network; a sync takes the
    /// write lock only around its merge, always after this one.
    fn lock_syncs(&self) -> Result<Option<std::fs::File>> {
        self.lock_file("marq-comments-sync.lock")
    }

    fn lock_file(&self, name: &str) -> Result<Option<std::fs::File>> {
        if !self.locking {
            return Ok(None);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.common_dir.join(name))?;
        file.lock()?;
        Ok(Some(file))
    }

    // ---- reading ----

    /// Every file path on the branch below `under`, with its blob id, from one
    /// `ls-tree`.
    fn list_entries(&self, tip: &str, under: &str) -> Result<Vec<(String, String)>> {
        let bytes = self
            .git
            .literal_pathspecs()
            .run_bytes(&["ls-tree", "-r", "-z", tip, "--", under], None)?;
        bytes
            .split(|b| *b == 0)
            .filter(|e| !e.is_empty())
            .map(|entry| {
                // `<mode> <type> <id>\t<path>`
                let bad = || {
                    Error::message(format!(
                        "unexpected git ls-tree output: {}",
                        String::from_utf8_lossy(entry)
                    ))
                };
                let tab = entry.iter().position(|b| *b == b'\t').ok_or_else(bad)?;
                let meta = String::from_utf8_lossy(&entry[..tab]);
                let id = meta.split(' ').nth(2).ok_or_else(bad)?.to_string();
                let path = String::from_utf8_lossy(&entry[tab + 1..]).into_owned();
                Ok((path, id))
            })
            .collect()
    }

    fn list_paths(&self, tip: &str, under: &str) -> Result<Vec<String>> {
        Ok(self
            .list_entries(tip, under)?
            .into_iter()
            .map(|(path, _)| path)
            .collect())
    }

    /// The contents of many objects, from one `cat-file --batch`. `entries`
    /// pairs a path, used in messages, with the object to read: a blob id, or
    /// `<tip>:<path>`. Blob ids are what `read_document` passes: git resolves
    /// each `<tip>:<path>` by searching the directory's tree from the start,
    /// so 1000 of them in one directory cost a million entry comparisons
    /// (found by T-07: 0.5 s of the 0.6 s `list` took on 1000 comments).
    fn read_files(&self, entries: &[(String, String)]) -> Result<Vec<Vec<u8>>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let input: String = entries.iter().map(|(_, o)| format!("{o}\n")).collect();
        let out = self
            .git
            .run_bytes(&["cat-file", "--batch"], Some(input.as_bytes()))?;
        let mut files = Vec::with_capacity(entries.len());
        let mut at = 0;
        for (path, _) in entries {
            let newline = out[at..]
                .iter()
                .position(|b| *b == b'\n')
                .ok_or_else(|| Error::message("git cat-file ended early"))?;
            let header = String::from_utf8_lossy(&out[at..at + newline]).into_owned();
            at += newline + 1;
            let size = match header.rsplit(' ').next() {
                Some("missing") => {
                    return Err(Error::message(format!("{path} is missing from the branch")))
                }
                Some(n) => n
                    .parse::<usize>()
                    .map_err(|_| Error::message(format!("unexpected git output: {header}")))?,
                None => return Err(Error::message(format!("unexpected git output: {header}"))),
            };
            if at + size > out.len() {
                return Err(Error::message("git cat-file ended early"));
            }
            files.push(out[at..at + size].to_vec());
            at += size + 1;
        }
        Ok(files)
    }

    /// The annotations and state changes recorded for a document. An empty
    /// result when the branch or the document does not exist.
    pub fn read_document(&self, key: &str) -> Result<DocumentRecords> {
        let Some(tip) = self.tip()? else {
            return Ok(DocumentRecords::default());
        };
        let dir = format!("documents/{key}/");
        let mut annotation_entries = Vec::new();
        let mut state_entries = Vec::new();
        for (path, id) in self.list_entries(&tip, &dir)? {
            let Some(rest) = path.strip_prefix(&dir) else {
                continue;
            };
            match rest.split_once('/') {
                Some(("annotations", name)) if is_json_name(name) => {
                    annotation_entries.push((path, id))
                }
                Some(("states", name)) if is_json_name(name) => state_entries.push((path, id)),
                _ => {}
            }
        }
        let mut records = DocumentRecords {
            annotations: self.parse_files(&annotation_entries)?,
            states: self.parse_files(&state_entries)?,
        };
        model::sort_records(&mut records.annotations);
        model::sort_records(&mut records.states);
        Ok(records)
    }

    fn parse_files(&self, entries: &[(String, String)]) -> Result<Vec<Value>> {
        self.read_files(entries)?
            .iter()
            .zip(entries)
            .map(|(bytes, (path, _))| {
                serde_json::from_slice(bytes)
                    .map_err(|e| Error::Json(format!("{path} is not valid JSON: {e}")))
            })
            .collect()
    }

    /// The document keys that have any record on the branch, sorted.
    pub fn list_documents(&self) -> Result<Vec<String>> {
        let Some(tip) = self.tip()? else {
            return Ok(Vec::new());
        };
        let mut keys: Vec<String> = self
            .document_files(&tip)?
            .into_iter()
            .map(|(key, _, _)| key)
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    /// (document key, folder, file name) for every file under `documents/`.
    fn document_files(&self, tip: &str) -> Result<Vec<(String, String, String)>> {
        let mut files = Vec::new();
        for path in self.list_paths(tip, "documents/")? {
            let Some(rest) = path.strip_prefix("documents/") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() < 3 {
                continue;
            }
            let n = parts.len();
            let folder = parts[n - 2];
            if matches!(folder, "annotations" | "states" | "versions") {
                files.push((
                    parts[..n - 2].join("/"),
                    folder.to_string(),
                    parts[n - 1].to_string(),
                ));
            }
        }
        Ok(files)
    }

    /// The markdown text an annotation was written against, from
    /// `versions/<blob>`. Errors when the entry is not on the branch.
    pub fn read_version(&self, key: &str, blob: &str) -> Result<String> {
        if blob.is_empty() || !blob.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::message(format!("{blob} is not a blob id")));
        }
        let tip = self
            .tip()?
            .ok_or_else(|| Error::message("there is no comments branch yet"))?;
        let path = format!("documents/{key}/versions/{blob}");
        let out = self
            .git
            .run_raw(&["cat-file", "blob", &format!("{tip}:{path}")], None)?;
        if !out.success() {
            return Err(Error::message(format!(
                "no recorded version {blob} for {key}: {}",
                out.stderr.trim()
            )));
        }
        Ok(String::from_utf8(out.stdout)?)
    }

    /// Finds an annotation by full uuid, `urn:uuid:` form or a prefix of at
    /// least 6 hex characters, across every document. Fails with `NoMatch` or
    /// `Ambiguous`.
    pub fn find_annotation(&self, arg: &str) -> Result<Found> {
        model::normalise_id_arg(arg)?;
        let Some(tip) = self.tip()? else {
            return Err(Error::NoMatch(arg.to_string()));
        };
        let mut found: Vec<(String, String)> = Vec::new();
        for (key, folder, name) in self.document_files(&tip)? {
            if folder != "annotations" {
                continue;
            }
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            if model::matches_id(arg, stem) {
                found.push((key, stem.to_string()));
            }
        }
        let ids: Vec<String> = found.iter().map(|(_, s)| format!("urn:uuid:{s}")).collect();
        let id = model::resolve_id(arg, ids.iter().map(String::as_str))?;
        let (document, stem) = found
            .into_iter()
            .find(|(_, s)| format!("urn:uuid:{s}") == id)
            .expect("resolve_id returned one of the ids");
        let path = format!("documents/{document}/annotations/{stem}.json");
        let object = format!("{tip}:{path}");
        let bytes = self.read_files(&[(path.clone(), object)])?.remove(0);
        let annotation = serde_json::from_slice(&bytes)
            .map_err(|e| Error::Json(format!("{path} is not valid JSON: {e}")))?;
        Ok(Found {
            document,
            annotation,
        })
    }

    // ---- sync ----

    /// Fetches, merges and pushes the comments branch (design 2.4). A merge
    /// commit is attributed to git's own identity.
    pub fn sync(&self, remote: &str) -> Result<SyncReport> {
        self.sync_as(remote, None)
    }

    /// `sync`, with merge commits attributed to `author` when given. Without
    /// it, and when git has no identity either, merges are attributed to
    /// `marq-comments`: found by T-07, a clone whose only identity came from
    /// `--author` wrote comments and then could not sync, because
    /// `commit-tree` refused the merge.
    pub fn sync_as(&self, remote: &str, author: Option<&Creator>) -> Result<SyncReport> {
        let branch = COMMENTS_REF
            .strip_prefix("refs/heads/")
            .unwrap_or(COMMENTS_REF);
        let tracking = format!("refs/remotes/{remote}/{branch}");
        let refspec = format!("+{COMMENTS_REF}:{tracking}");
        let mut report = SyncReport::default();
        let mut last_push_error = None;
        let identity = author.map(|a| Identity {
            name: a.name.clone(),
            email: a.email.clone(),
        });
        let _turn = self.lock_syncs()?;

        for attempt in 0..SYNC_ATTEMPTS {
            if attempt > 0 {
                sync_backoff(attempt);
            }
            let fetch = self.git.run_raw(&["fetch", remote, &refspec], None)?;
            let remote_tip = if fetch.success() {
                Some(self.git.run(&["rev-parse", "--verify", &tracking])?)
            } else if fetch.stderr.contains("couldn't find remote ref") {
                None
            } else {
                return Err(Error::Git {
                    command: format!("fetch {remote} {refspec}"),
                    stderr: fetch.stderr,
                });
            };
            report.remote_has_branch = remote_tip.is_some();
            if let Some(rt) = &remote_tip {
                self.integrate(rt, &tracking, identity.as_ref(), &mut report)?;
            }
            report.tip = self.tip()?;
            let Some(local) = report.tip.clone() else {
                return Ok(report);
            };
            if remote_tip.as_deref() == Some(local.as_str()) {
                return Ok(report);
            }
            let push = self.git.run_raw(
                &["push", remote, &format!("{COMMENTS_REF}:{COMMENTS_REF}")],
                None,
            )?;
            if push.success() {
                report.pushed = true;
                return Ok(report);
            }
            let retryable = push.stderr.contains("rejected");
            last_push_error = Some(Error::Git {
                command: format!("push {remote} {COMMENTS_REF}:{COMMENTS_REF}"),
                stderr: push.stderr,
            });
            if !retryable {
                break;
            }
        }
        Err(last_push_error.expect("the loop ran at least once"))
    }

    /// Brings the remote tip into the local branch: create, fast-forward, keep
    /// when local is ahead, or merge without a checkout.
    fn integrate(
        &self,
        remote_tip: &str,
        tracking: &str,
        author: Option<&Identity>,
        report: &mut SyncReport,
    ) -> Result<()> {
        self.with_lock(|| self.integrate_locked(remote_tip, tracking, author, report))
    }

    fn integrate_locked(
        &self,
        remote_tip: &str,
        tracking: &str,
        author: Option<&Identity>,
        report: &mut SyncReport,
    ) -> Result<()> {
        let mut last_error = None;
        for attempt in 0..WRITE_ATTEMPTS {
            if attempt > 0 {
                backoff(attempt);
            }
            let Some(local) = self.tip()? else {
                match self.update_ref(remote_tip, ZERO_ID)? {
                    RefUpdate::Done => {
                        report.fast_forwarded = true;
                        return Ok(());
                    }
                    RefUpdate::Moved => continue,
                    RefUpdate::Refused(e) => {
                        last_error = Some(e);
                        continue;
                    }
                }
            };
            if local == remote_tip || self.is_ancestor(remote_tip, &local)? {
                return Ok(());
            }
            if self.is_ancestor(&local, remote_tip)? {
                match self.update_ref(remote_tip, &local)? {
                    RefUpdate::Done => {
                        report.fast_forwarded = true;
                        return Ok(());
                    }
                    RefUpdate::Moved => continue,
                    RefUpdate::Refused(e) => {
                        last_error = Some(e);
                        continue;
                    }
                }
            }
            // Two clones that each wrote their first comment before syncing
            // have root commits with no common ancestor. git 2.41 and later
            // refuse that merge unless told it is intended; the identical
            // README.md and format.json blobs then merge clean.
            let related = self
                .git
                .run_raw(&["merge-base", &local, remote_tip], None)?
                .success();
            let mut args = vec!["merge-tree", "--write-tree"];
            if !related {
                args.push("--allow-unrelated-histories");
            }
            args.push(&local);
            args.push(remote_tip);
            let mut merge = self.git.run_raw(&args, None)?;
            if merge.status == 129 && !related {
                // git 2.38 to 2.40 do not know the flag and merge unrelated
                // histories without it.
                merge = self
                    .git
                    .run_raw(&["merge-tree", "--write-tree", &local, remote_tip], None)?;
            }
            if merge.status == 1 {
                return Err(Error::Conflict(merge.text()));
            }
            if !merge.success() {
                return Err(Error::Git {
                    command: format!("merge-tree --write-tree {local} {remote_tip}"),
                    stderr: merge.stderr,
                });
            }
            let tree = merge.text().lines().next().unwrap_or("").to_string();
            let message = format!("merge {tracking} into {COMMENTS_REF}");
            let identity = match author {
                Some(who) => Some(who.clone()),
                None => self.fallback_merge_identity()?,
            };
            let commit =
                self.commit_tree(&tree, &[&local, remote_tip], &message, identity.as_ref())?;
            match self.update_ref(&commit, &local)? {
                RefUpdate::Done => {
                    report.merged = true;
                    return Ok(());
                }
                RefUpdate::Moved => {}
                RefUpdate::Refused(e) => last_error = Some(e),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            Error::message(format!(
                "the comments branch kept moving while merging, after {WRITE_ATTEMPTS} attempts"
            ))
        }))
    }

    /// `None` when git has an author and a committer identity of its own,
    /// which a merge commit then carries. Otherwise the fixed name
    /// `marq-comments` with no email, so the merge still happens.
    fn fallback_merge_identity(&self) -> Result<Option<Identity>> {
        for var in ["GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"] {
            if !self.git.run_raw(&["var", var], None)?.success() {
                return Ok(Some(Identity {
                    name: FALLBACK_MERGE_NAME.to_string(),
                    email: String::new(),
                }));
            }
        }
        Ok(None)
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let out = self
            .git
            .run_raw(&["merge-base", "--is-ancestor", ancestor, descendant], None)?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(Error::Git {
                command: format!("merge-base --is-ancestor {ancestor} {descendant}"),
                stderr: out.stderr,
            }),
        }
    }
}

fn require_id(record: &Value) -> Result<&str> {
    model::id(record).ok_or_else(|| Error::message("the record has no id"))
}

fn is_json_name(name: &str) -> bool {
    name.ends_with(".json") && !name.contains('/')
}

/// A short random pause, longer with each attempt, so processes that collided
/// on `update-ref` do not collide again in step.
fn backoff(attempt: u32) {
    let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]) % 16;
    let millis = jitter + u64::from(attempt) * 3;
    std::thread::sleep(std::time::Duration::from_millis(millis));
}

/// The pause before `sync` fetches again after a rejected push. A round of
/// fetch, merge and push between clones takes tens of milliseconds, so the
/// pause is of that order, random so that clones that collided do not collide
/// again in step, and longer with each attempt.
fn sync_backoff(attempt: u32) {
    let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]) % 50;
    let millis = jitter + u64::from(attempt) * 20;
    std::thread::sleep(std::time::Duration::from_millis(millis));
}
