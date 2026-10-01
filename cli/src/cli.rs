//! Command-line arguments. Owned by task T-06. See doc/comments-design.md 6.1.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "marq-comments",
    version,
    about = "Git-backed comments and suggestions on markdown files"
)]
pub struct Cli {
    /// Who writes the annotation, as "Name <email>". Defaults to git's user.name and user.email.
    #[arg(long, global = true, value_name = "NAME <EMAIL>")]
    pub author: Option<String>,

    /// Record the author as software, not a person.
    #[arg(long, global = true)]
    pub agent: bool,

    /// Run as if started in DIR, as `git -C` does.
    #[arg(short = 'C', global = true, value_name = "DIR")]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// Where an annotation anchors: a line, a word on a line, or a code-point range.
#[derive(clap::Args, Debug, Clone)]
pub struct Place {
    /// The 1-based line.
    #[arg(long, value_name = "N", required_unless_present = "range")]
    pub line: Option<usize>,

    /// A word on that line, to anchor on it instead of the whole line.
    #[arg(long, value_name = "WORD", requires = "line")]
    pub text: Option<String>,

    /// Which occurrence of the word on that line, counting from 1.
    #[arg(long, value_name = "K", requires = "text")]
    pub nth: Option<usize>,

    /// A code-point range START:END of the file, end exclusive.
    #[arg(
        long,
        value_name = "START:END",
        conflicts_with_all = ["line", "text", "nth"],
    )]
    pub range: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Comment on a line, a word or a range. Prints the new id.
    Comment {
        file: String,
        #[command(flatten)]
        place: Place,
        /// The comment, as markdown.
        #[arg(short, long)]
        message: String,
    },
    /// Reply to a comment, a suggestion or another reply. Prints the new id.
    Reply {
        id: String,
        #[arg(short, long)]
        message: String,
    },
    /// Suggest replacing text. Prints the new id.
    Suggest {
        file: String,
        #[command(flatten)]
        place: Place,
        /// The replacement text. An empty string deletes the anchored text.
        #[arg(long, value_name = "NEW")]
        replace: String,
        /// Why, as markdown.
        #[arg(short, long)]
        message: Option<String>,
    },
    /// List the threads on a file.
    List {
        file: String,
        #[arg(long, value_enum, default_value_t = StateFilter::All)]
        state: StateFilter,
        #[arg(long)]
        json: bool,
    },
    /// Show one thread with its full state history.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Resolve an open comment.
    Resolve { id: String },
    /// Reopen a resolved comment or a rejected suggestion.
    Reopen { id: String },
    /// Accept an open suggestion and apply its edit to the working-tree file.
    Accept { id: String },
    /// Reject an open suggestion.
    Reject { id: String },
    /// Fetch, merge and push the comments branch.
    Sync {
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// Write the file and its threads as one HTML page.
    Render {
        file: String,
        /// Write the page here. Without it the page goes to standard output.
        #[arg(short, long, value_name = "OUT.html")]
        output: Option<PathBuf>,
        /// Show the markdown source in a `<pre>` instead of the rendered document.
        #[arg(long)]
        source: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    All,
    Open,
    Resolved,
    Accepted,
    Rejected,
}

impl StateFilter {
    /// Whether a thread in `state` passes this filter.
    pub fn allows(self, state: &str) -> bool {
        match self {
            StateFilter::All => true,
            StateFilter::Open => state == "open",
            StateFilter::Resolved => state == "resolved",
            StateFilter::Accepted => state == "accepted",
            StateFilter::Rejected => state == "rejected",
        }
    }
}
