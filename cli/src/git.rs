//! A thin wrapper around the `git` binary. Owned by task T-04.
//!
//! No git library: the design requires the git the user already has configured.

use crate::error::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What a git process produced. `status` is -1 when a signal ended it.
#[derive(Debug, Clone)]
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status == 0
    }

    /// Standard output as text with surrounding whitespace removed.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }
}

/// A git invocation context: a directory plus extra environment variables.
#[derive(Debug, Clone)]
pub struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Git {
            dir: dir.into(),
            env: Vec::new(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A copy that also sets `key` for every command, for example `GIT_INDEX_FILE`.
    pub fn with_env(&self, key: &str, value: impl Into<String>) -> Self {
        let mut copy = self.clone();
        copy.env.push((key.to_string(), value.into()));
        copy
    }

    /// Runs git and returns its output whatever the exit status. Only a failure
    /// to start git is an error, so callers can read statuses that carry meaning
    /// (`merge-base --is-ancestor`, `merge-tree`, `rev-parse --verify`).
    pub fn run_raw(&self, args: &[&str], input: Option<&[u8]>) -> Result<Output> {
        let mut command = Command::new("git");
        command
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let mut child = command
            .spawn()
            .map_err(|e| Error::Io(format!("could not start git: {e}")))?;
        // A writer thread keeps a large input from deadlocking against git's
        // output pipes.
        let writer = input.map(|bytes| {
            let mut stdin = child.stdin.take().expect("stdin was piped");
            let bytes = bytes.to_vec();
            std::thread::spawn(move || {
                // A broken pipe means git stopped reading; its status says why.
                let _ = stdin.write_all(&bytes);
            })
        });
        let output = child.wait_with_output()?;
        if let Some(handle) = writer {
            let _ = handle.join();
        }
        Ok(Output {
            status: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    /// Runs git and fails with git's stderr on a non-zero status.
    pub fn run_bytes(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let output = self.run_raw(args, input)?;
        if output.success() {
            Ok(output.stdout)
        } else {
            Err(Error::Git {
                command: args.join(" "),
                stderr: output.stderr,
            })
        }
    }

    /// Runs git and returns trimmed standard output.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let bytes = self.run_bytes(args, None)?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    }

    /// Runs git with `input` on standard input and returns trimmed standard output.
    pub fn run_with_input(&self, args: &[&str], input: &[u8]) -> Result<String> {
        let bytes = self.run_bytes(args, Some(input))?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    }
}
