# Mission: Git-backed comments and suggestions for markdown

| Field | Value |
|---|---|
| ID | M-COMMENTS |
| Territory | marq |
| Assignee | cloud agent |
| Blocked by | none |

## Objective

- `doc/comments-design.md` exists and specifies the storage format, the git layout, the
  anchoring and re-anchoring method, and the CLI commands. Every design decision in it
  carries a one-line reason.
- A JSON Schema for the stored annotation exists in the repo, and every annotation in the
  test fixtures validates against it.
- A CLI builds and runs on Linux (a Claude Code cloud session) and on macOS.
- Given a git repo holding markdown files, the CLI adds a comment anchored to a word, a
  comment anchored to a whole line, a reply to a comment, and a suggestion (replace a range
  with new text). Each result is a W3C Web Annotation stored on the `md-comments` branch.
- The CLI lists the annotations for a file, as JSON and as readable text, with each anchor
  resolved against the file's current content.
- After an edit to the markdown elsewhere in the file, an anchor still resolves to the same
  text. After an edit that removes the anchored text, the CLI reports the anchor as
  orphaned. It does not attach it to other text.
- Resolving a comment, and accepting or rejecting a suggestion, changes the annotation's
  state. The earlier state stays in the `md-comments` branch history. Nothing gets deleted.
- Accepting a suggestion applies the edit to the markdown file on the working branch.
- Two clones add annotations in parallel, push, fetch and merge, and neither loses an
  annotation. Proven by a test against a local bare repo as the remote.
- All of the above is proven by tests that run with one command in a cloud session.

## Purpose

Jim needs collaborative markdown review with comment mode and suggestion mode, anchored to
a word or a line, and with version and comment history stored alongside the markdown in
git. Marq is the viewer that will show the comments. Agents need the same comments, so the
CLI is the first interface and marq's UI comes later.

## Intelligence

- Research note, the requirement and the options considered:
  https://github.com/jimbarritt/dotfiles/blob/main/doc/collaborative-markdown-editing/2026-09-26T06-44-23Z-collaborative-markdown-editing-with-git-backed-history.v1.md
- Code analyses of existing tools, in the same directory
  (https://github.com/jimbarritt/dotfiles/tree/main/doc/collaborative-markdown-editing):
  - `*-collabmd-code-analysis.v1.md`: sidecar JSON comments with Yjs relative positions,
    kept out of git. Resolving deletes the thread. Line-number fallback on external edits.
  - `*-obsidian-criticmarkup-code-analysis.v1.md`: inline CriticMarkup. Suggestion mode by
    a CodeMirror transaction filter. Anchors are fragile because they have no ID.
  - `*-agent-comments-code-analysis.v1.md`: inline CriticMarkup with threads and agent
    integration through `AGENTS.md` and `CLAUDE.md` instructions.
  - `*-hedgedoc-code-analysis.v1.md`: SQL revisions stored as a snapshot plus a patch. No
    comments.
- W3C Web Annotation Data Model: https://www.w3.org/TR/annotation-model/
  - `TextQuoteSelector` (exact, prefix, suffix) and `TextPositionSelector` (start, end).
    A target can carry both.
  - Replies are annotations whose target is another annotation.
  - The `editing` motivation covers a suggested change.
  - W3C Web Annotation Vocabulary: https://www.w3.org/TR/annotation-vocab/
- Hypothesis (https://github.com/hypothesis) implements the W3C model, including
  re-anchoring from a quote selector. Read how it re-anchors before designing yours.
- `doc/line-number-gutter-arch.md`: marq already maps rendered blocks back to source lines
  through `data-source-line`. The future marq UI builds on this.
- `CLAUDE.md` and `doc/planning/plan.md` in this repo: architecture and conventions.
- A cloud session can fetch custom refs, for example `refs/tsk/*`. Claude on iOS cannot
  push to custom refs. That is why comments start on the `md-comments` branch, not a
  custom ref or git notes.
- [Intel: what a cloud session can build, and the CLI split](M-COMMENTS/intel-cloud-build-and-cli-split.md):
  the marq package cannot build in a cloud session (AppKit, WebKit and PDFKit have no
  Linux port), so no harness instrument runs there. Rust is the CLI language: `cargo`
  fetches and builds crates through the session proxy, proven. Layout is decided:
  `cli/` in this repo, permanently.
- [ADR 0002](../adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md): marq
  reads the CLI's output as a spawned process, not a linked library or a resident
  daemon. Covers the measured spawn cost, why FFI risks the stale-build trap
  `CLAUDE.md` already records, and why a local daemon does not itself provide
  real-time collaboration between machines, that needs a network relay, a separate,
  later architecture question.

## Decision authority

You decide:

- The CLI's implementation language, within the constraints below. Record the reason in
  the design doc.
- The file layout on the `md-comments` branch, and the file naming.
- How the annotation records the markdown version it anchors to.
- The CLI's command names and output formats.
- Any extension properties beyond the W3C model, for example comment state. Put them in
  a separate JSON-LD context.

Jim decides:

- Anything that changes the constraints below.
- The marq UI design. A later mission covers it.

## Constraints

- Annotations follow the W3C Web Annotation Data Model. Extensions are additive. A W3C
  consumer that ignores the extensions still reads a valid annotation.
- Annotations live as JSON files committed on an orphan branch named `md-comments`
  (`refs/heads/md-comments`) in the same repo as the markdown. They never appear in the
  markdown's working tree.
- The ref name lives in one place in the code, so a later move to a custom ref is a
  one-line change.
- The markdown files stay free of comment markup.
- History is never destroyed. Resolve, accept and reject change state. They do not delete.
- JSON gets written with sorted keys and fixed indentation, so diffs stay minimal and
  deltas stay small.
- In the common case, parallel writes from separate clones merge without a conflict. One
  file per annotation, with a unique ID, meets this.
- Anchors cover a range within a line (a word) and a whole line.
- The CLI runs in a Claude Code cloud session (Linux) with no macOS toolchain, and on
  macOS. It uses the `git` binary. It does not need a running marq.
- Marq stays a viewer. Nothing in this mission turns marq into an editor.
- British English in code, comments and docs.

## Out of scope

- Any change to the marq app or `template.html`.
- Real-time collaboration, presence and CRDTs.
- Authentication and user accounts. The author comes from `git config user.name` and
  `user.email`, or a flag.
- Rendering CriticMarkup, or importing from or exporting to it.
- A server or web UI.
- Homebrew packaging and release of the CLI.
- An MCP server. The CLI plus a section in `CLAUDE.md` or `AGENTS.md` gives agents access.

## Plan

| ID | Task | Objective, in short | Delegated to | Blocked by | Status |
|---|---|---|---|---|---|
| T-00 | Reshape the repo into `macos/` and `cli/` | The existing Swift app moved into `macos/`, a clean `cli/` ready for the CLI, every path reference updated, nothing else changed | none | none | DONE, awaiting Jim's macOS build confirmation |
| T-01 | Read the intelligence | W3C selectors, Hypothesis re-anchoring and the four analyses understood | none | T-00 | TODO |
| T-02 | Write the design doc | `doc/comments-design.md` complete, one reason per decision | none | T-01 | TODO |
| T-03 | Write the JSON Schema | Schema in the repo, fixtures validate | none | T-02 | TODO |
| T-04 | Build the storage layer | Read and write annotation files on `md-comments` without touching the working tree | none | T-02 | TODO |
| T-05 | Build anchoring | Quote and position selectors resolve, re-anchor after edits, report orphans | none | T-02 | TODO |
| T-06 | Build the CLI commands | Add, reply, suggest, list, resolve, accept, reject all work | none | T-04, T-05 | TODO |
| T-07 | Prove parallel writes | Two clones against a bare remote merge with no lost annotation | none | T-06 | TODO |
| T-08 | Document agent use | A section an agent reads to use the CLI, ready to paste into `CLAUDE.md` | none | T-06 | TODO |
| T-09 | Update `doc/planning/plan.md` | A delta for this mission with its tasks and status | none | T-02 | TODO |

**Essential task**: T-04. The mission fails if comments do not live in git alongside the
markdown, because that is the requirement no existing tool meets.

**T-00 note**: done as a pure `git mv` of `Sources/`, `Package.swift`, `justfile`,
`tests/`, `tools/`, `examples/` and `assets/` into `macos/`, with every path
reference in `CLAUDE.md`, `README.md`, `doc/` and `.claude/skills/verify/` updated
to match. No line of Swift, and no recipe's logic, changed. A cloud session has no
Swift toolchain (see the intel doc), so this cannot be built or checked here.
**T-01 does not start until Jim confirms `cd macos && swift build && just check`
still passes on macOS.** Naming (`macos/`, not `mac/` or `mac-os/`) is
[ADR 0003](../adr/0003-repo-layout-macos-and-cli-directories.md).

## First behaviour

Take ownership of the plan above before any other action. Add the implied tasks, reorder
as you see fit, and write it back as your own.

## Execution constraints

- Files: `doc/` and `cli/` (the Rust CLI and its tests). Do not edit `macos/`.
- Tests use temporary git repos and a local bare repo as the remote. They never push to
  GitHub.
- Commit and push your work before the session ends. The cloud container gets deleted.
- Attempt limit: 3.

## Report on completion

Write the report to `doc/planning/M-COMMENTS/M-COMMENTS-git-backed-comments-report.md`.
Outcome: done, failed, or blocked, with attempt count. Your account: what you did, what
this briefing failed to give you, and what you found wrong in it, including in your own
work. Feedback only. Do not write an open question, a request for a decision, or anything
that waits on a reply.
