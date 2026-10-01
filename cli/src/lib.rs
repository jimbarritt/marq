//! Git-backed comments and suggestions on markdown files.
//!
//! The design is in `doc/comments-design.md`. Each module below has one owner
//! task, named in its header comment, so parallel work does not touch the same file.

pub mod anchor; // T-05: selectors, resolving an anchor against changed text
pub mod cli; // T-06: command-line arguments
pub mod commands; // T-06: one function per command
pub mod error; // T-04: the crate's error type
pub mod git; // T-04: a thin wrapper around the `git` binary
pub mod model; // T-04: annotation and state-change records
pub mod output; // T-06: text and JSON output
pub mod render; // T-10: the static HTML page
pub mod store; // T-04: the md-comments branch
pub mod text; // T-05: code-point text helpers
pub mod threads; // T-06: threads, folded state and resolved anchors
