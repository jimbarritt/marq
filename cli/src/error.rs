//! The crate's error type. Owned by task T-04.
//!
//! The commands map an error to the exit codes of design 6.3 through
//! [`Error::exit_code`], so every module reports failures the same way.

use std::fmt;

/// Exit code for any error that is not an id lookup failure.
pub const EXIT_ERROR: i32 = 1;
/// Exit code for an id that matched no annotation or more than one.
pub const EXIT_ID: i32 = 2;

#[derive(Debug)]
pub enum Error {
    /// A git command exited with a failure status. `stderr` is git's own text.
    Git { command: String, stderr: String },
    /// The `git` binary could not be started, or a file could not be read or written.
    Io(String),
    /// A stored file is not valid JSON, or does not have the expected shape.
    Json(String),
    /// An id argument matched no annotation.
    NoMatch(String),
    /// An id argument matched more than one annotation. `candidates` are full ids.
    Ambiguous {
        arg: String,
        candidates: Vec<String>,
    },
    /// `git merge-tree` reported a conflict between two tips of the comments branch.
    Conflict(String),
    /// Any other failure, with a plain sentence for the user.
    Message(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn message(text: impl Into<String>) -> Self {
        Error::Message(text.into())
    }

    /// The process exit code of design 6.3 that this error maps to.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::NoMatch(_) | Error::Ambiguous { .. } => EXIT_ID,
            _ => EXIT_ERROR,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Git { command, stderr } => {
                let stderr = stderr.trim();
                if stderr.is_empty() {
                    write!(f, "git {command} failed")
                } else {
                    write!(f, "git {command} failed: {stderr}")
                }
            }
            Error::Io(text) | Error::Json(text) | Error::Message(text) => f.write_str(text),
            Error::NoMatch(arg) => write!(f, "no annotation matches the id {arg}"),
            Error::Ambiguous { arg, candidates } => {
                write!(
                    f,
                    "the id {arg} matches {} annotations: {}",
                    candidates.len(),
                    candidates.join(", ")
                )
            }
            Error::Conflict(text) => write!(
                f,
                "the comments branches conflict and need a person to resolve them: {}",
                text.trim()
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e.to_string())
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Error::Message(format!("stored text is not valid UTF-8: {e}"))
    }
}
