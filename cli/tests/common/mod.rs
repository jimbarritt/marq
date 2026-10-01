//! Shared helpers for integration tests: temporary git repositories and a local
//! bare repository as the remote. Nothing here touches the network.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use tempfile::TempDir;

static INIT: Once = Once::new();

/// Makes git ignore this machine's configuration and use a fixed identity.
/// Set once for the whole test process so every spawned git inherits it.
pub fn init_env() {
    INIT.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_AUTHOR_NAME", "Test Author");
        std::env::set_var("GIT_AUTHOR_EMAIL", "author@example.com");
        std::env::set_var("GIT_COMMITTER_NAME", "Test Author");
        std::env::set_var("GIT_COMMITTER_EMAIL", "author@example.com");
    });
}

/// Runs git in `dir`, panics on failure, returns trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    init_env();
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git starts");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Like `git` but returns the exit status and stdout without panicking.
pub fn git_status(dir: &Path, args: &[&str]) -> (i32, String) {
    init_env();
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git starts");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    )
}

pub const DOC_TEXT: &str = "# Plan\n\nThe build uses esbuild for bundling.\n\nIt is fast.\n";

/// A repository with one commit on `main` holding `doc/plan.md`.
pub struct TestRepo {
    pub dir: TempDir,
}

impl TestRepo {
    pub fn new() -> TestRepo {
        init_env();
        let dir = TempDir::new().expect("temp dir");
        git(dir.path(), &["init", "-q", "-b", "main"]);
        let repo = TestRepo { dir };
        repo.write("doc/plan.md", DOC_TEXT);
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "first"]);
        repo
    }

    /// A clone of `remote`, with the markdown file already committed there.
    pub fn clone_of(remote: &Path) -> TestRepo {
        init_env();
        let dir = TempDir::new().expect("temp dir");
        git(dir.path(), &["clone", "-q", &remote.to_string_lossy(), "."]);
        TestRepo { dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn write(&self, rel: &str, text: &str) {
        let path = self.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub fn git(&self, args: &[&str]) -> String {
        git(self.path(), args)
    }
}

/// A bare repository, and a clone-able source with `doc/plan.md` on `main`.
pub struct Remote {
    pub dir: TempDir,
}

impl Remote {
    /// A bare remote seeded from a fresh repository, plus that repository as
    /// the first working clone with `origin` pointing at the remote.
    pub fn with_first_clone() -> (Remote, TestRepo) {
        let remote = Remote {
            dir: TempDir::new().expect("temp dir"),
        };
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let first = TestRepo::new();
        first.git(&["remote", "add", "origin", &remote.path().to_string_lossy()]);
        first.git(&["push", "-q", "origin", "main"]);
        (remote, first)
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        git(self.path(), args)
    }
}

pub fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn pathbuf(p: &Path) -> PathBuf {
    p.to_path_buf()
}
