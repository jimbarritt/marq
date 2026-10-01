# marq-comments

`marq-comments` is a command-line tool for comments and suggestions on markdown
files. It stores them in git, as W3C Web Annotations on the `md-comments` branch,
so the markdown stays clean and the review history travels with the repository.

## Install

```
cargo install --git https://github.com/jimbarritt/marq marq-comments
```

It needs git 2.38 or later and Rust 1.89 or later. It builds and runs on Linux
and macOS. From a checkout, `cd cli && cargo build` writes
`target/debug/marq-comments`.

An AI agent reads [`doc/comments-agent-guide.md`](../doc/comments-agent-guide.md).
Paste it into the `CLAUDE.md` or `AGENTS.md` of a repository that uses the tool.

## A short tour

Run these in a git repository that holds `docs/test.md`.

```
$ marq-comments comment docs/test.md --line 3 --text macOS -m "Say which versions of macOS are supported."
3f2012f6

$ marq-comments --agent --author "Claude <noreply@anthropic.com>" reply 3f2012f6 -m "Added a line on versions in the README."
ce99d9d8

$ marq-comments list docs/test.md
3f2012f6  open  comment  docs/test.md:3:41  "macOS"
  Jim, 2026-10-01 07:35
  Say which versions of macOS are supported.
  ce99d9d8  Claude (agent), 2026-10-01 07:35
    Added a line on versions in the README.

$ marq-comments resolve 3f2012f6

$ marq-comments sync
pushed the comments branch to origin
```

`comment`, `reply` and `suggest` print the new id. An id is 8 characters, and
any prefix of 6 or more works. `list --json` gives the same threads as data.
`suggest`, `accept`, `reject`, `reopen`, `show` and `render` complete the set:
run `marq-comments --help`.

When the markdown changes, `list` re-finds each anchor and reports it as
`anchored`, `changed` (the text was edited, and the line shows the original) or
`orphaned` (the text is gone). It never attaches a comment to other text.

## The acceptance run

```
cd cli && just acceptance
```

The run builds the binary, copies `example-docs/test.md` into temporary
repositories, and drives the real binary through ten scenarios: a comment, a
reply, a suggestion accepted and rejected, edits that move, change and delete
anchored text, and two clones that sync. It writes `cli/target/acceptance/index.html`,
with the commands, their output and the rendered result for each scenario. Pass
`--only 07` to run one scenario. `just acceptance-open` also opens the page.

## The tests

```
cd cli && just test
```

This runs `cargo test`: unit tests for anchoring and storage, and integration
tests that run the binary against temporary repositories and a local bare
repository as the remote. Nothing pushes to GitHub.

## Where the design lives

- [`doc/comments-design.md`](../doc/comments-design.md): storage layout, annotation
  format, anchoring, state, commands, exit codes, and known limits. Each decision
  carries its reason.
- [`doc/adr/`](../doc/adr/): [0002](../doc/adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md)
  (marq reads the CLI as a spawned process) and
  [0003](../doc/adr/0003-repo-layout-macos-and-cli-directories.md) (the `cli/`
  directory beside `macos/`).
- [`doc/planning/M-COMMENTS-git-backed-comments.md`](../doc/planning/M-COMMENTS-git-backed-comments.md):
  the mission and its tasks.
