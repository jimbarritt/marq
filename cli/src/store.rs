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

/// The one place the ref name is written. A later move to a custom ref changes
/// this line.
pub const COMMENTS_REF: &str = "refs/heads/md-comments";

/// How often a write retries from the read of the tip when `update-ref` finds
/// the tip moved (design 2.2).
const WRITE_ATTEMPTS: u32 = 10;
/// How often `sync` repeats from the fetch when a push is rejected (design 2.4).
const SYNC_ATTEMPTS: u32 = 3;
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

pub struct Store {
    git: Git,
    root: PathBuf,
    git_dir: PathBuf,
    prefix: String,
    locking: bool,
}

static INDEX_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temporary index file that is removed when dropped.
struct TempIndex(PathBuf);

impl TempIndex {
    fn new(git_dir: &Path) -> Self {
        let n = INDEX_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        TempIndex(git_dir.join(format!(
            "marq-comments-index.{}.{n}.{}",
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
        Ok(Store {
            git: Git::new(&root),
            root,
            git_dir,
            prefix,
            locking: true,
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
        let _lock = self.lock_writes()?;
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
            match self.update_ref(&commit, expected) {
                Ok(true) => return Ok(commit),
                Ok(false) => {
                    last_error = Some(Error::message(
                        "the comments branch kept moving while writing, after 10 attempts",
                    ))
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_error.expect("the loop ran at least once"))
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

    /// `update-ref` with the expected old value. `Ok(false)` means git refused
    /// because the ref no longer holds `expected`.
    fn update_ref(&self, new: &str, expected: &str) -> Result<bool> {
        let out = self
            .git
            .run_raw(&["update-ref", COMMENTS_REF, new, expected], None)?;
        Ok(out.success())
    }

    /// Takes an advisory lock shared by every `marq-comments` process on this
    /// repository, held until the guard drops. With 8 writers each retry has
    /// about a one in eight chance, and 10 retries starve one of them often
    /// enough to lose a write (measured). The lock makes writers on one machine
    /// take turns. `update-ref` with the expected old value stays the guard
    /// for anything that does not take the lock.
    fn lock_writes(&self) -> Result<Option<std::fs::File>> {
        if !self.locking {
            return Ok(None);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.git_dir.join("marq-comments.lock"))?;
        file.lock()?;
        Ok(Some(file))
    }

    // ---- reading ----

    /// Every file path on the branch below `documents/`, from one `ls-tree`.
    fn list_paths(&self, tip: &str, under: &str) -> Result<Vec<String>> {
        let bytes = self.git.run_bytes(
            &["ls-tree", "-r", "-z", "--name-only", tip, "--", under],
            None,
        )?;
        Ok(bytes
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect())
    }

    /// The contents of many files at one commit, from one `cat-file --batch`.
    fn read_files(&self, tip: &str, paths: &[String]) -> Result<Vec<Vec<u8>>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let input: String = paths.iter().map(|p| format!("{tip}:{p}\n")).collect();
        let out = self
            .git
            .run_bytes(&["cat-file", "--batch"], Some(input.as_bytes()))?;
        let mut files = Vec::with_capacity(paths.len());
        let mut at = 0;
        for path in paths {
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
        let mut annotation_paths = Vec::new();
        let mut state_paths = Vec::new();
        for path in self.list_paths(&tip, &dir)? {
            let Some(rest) = path.strip_prefix(&dir) else {
                continue;
            };
            match rest.split_once('/') {
                Some(("annotations", name)) if is_json_name(name) => annotation_paths.push(path),
                Some(("states", name)) if is_json_name(name) => state_paths.push(path),
                _ => {}
            }
        }
        let mut records = DocumentRecords {
            annotations: self.parse_files(&tip, &annotation_paths)?,
            states: self.parse_files(&tip, &state_paths)?,
        };
        model::sort_records(&mut records.annotations);
        model::sort_records(&mut records.states);
        Ok(records)
    }

    fn parse_files(&self, tip: &str, paths: &[String]) -> Result<Vec<Value>> {
        self.read_files(tip, paths)?
            .iter()
            .zip(paths)
            .map(|(bytes, path)| {
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
        let bytes = self
            .read_files(&tip, std::slice::from_ref(&path))?
            .remove(0);
        let annotation = serde_json::from_slice(&bytes)
            .map_err(|e| Error::Json(format!("{path} is not valid JSON: {e}")))?;
        Ok(Found {
            document,
            annotation,
        })
    }

    // ---- sync ----

    /// Fetches, merges and pushes the comments branch (design 2.4).
    pub fn sync(&self, remote: &str) -> Result<SyncReport> {
        let branch = COMMENTS_REF
            .strip_prefix("refs/heads/")
            .unwrap_or(COMMENTS_REF);
        let tracking = format!("refs/remotes/{remote}/{branch}");
        let refspec = format!("+{COMMENTS_REF}:{tracking}");
        let mut report = SyncReport::default();
        let mut last_push_error = None;

        for _ in 0..SYNC_ATTEMPTS {
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
                self.integrate(rt, &tracking, &mut report)?;
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
    fn integrate(&self, remote_tip: &str, tracking: &str, report: &mut SyncReport) -> Result<()> {
        let _lock = self.lock_writes()?;
        for attempt in 0..WRITE_ATTEMPTS {
            if attempt > 0 {
                backoff(attempt);
            }
            let Some(local) = self.tip()? else {
                if self.update_ref(remote_tip, ZERO_ID)? {
                    report.fast_forwarded = true;
                    return Ok(());
                }
                continue;
            };
            if local == remote_tip || self.is_ancestor(remote_tip, &local)? {
                return Ok(());
            }
            if self.is_ancestor(&local, remote_tip)? {
                if self.update_ref(remote_tip, &local)? {
                    report.fast_forwarded = true;
                    return Ok(());
                }
                continue;
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
            let commit = self.commit_tree(&tree, &[&local, remote_tip], &message, None)?;
            if self.update_ref(&commit, &local)? {
                report.merged = true;
                return Ok(());
            }
        }
        Err(Error::message(
            "the comments branch kept moving while merging, after 10 attempts",
        ))
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
