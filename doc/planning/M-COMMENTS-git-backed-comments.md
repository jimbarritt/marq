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
| T-00 | Reshape the repo into `macos/` and `cli/` | The existing Swift app moved into `macos/`, a clean `cli/` ready for the CLI, every path reference updated, nothing else changed | none | none | DONE |
| T-01 | Read the intelligence | W3C selectors, Hypothesis re-anchoring and the four analyses understood | none | T-00 | DONE |
| T-02 | Write the design doc | [`doc/comments-design.md`](../comments-design.md) complete, one reason per decision | none | T-01 | DONE |
| T-03 | Write the JSON Schema | Schemas for an annotation and a state change in `cli/schema/`; fixtures in `cli/tests/fixtures/` validate (design 3, 7) | none | T-02 | DONE |
| T-04 | Build the storage layer | Crate scaffold in `cli/`; write, read and `sync` on `md-comments` through a temporary index, never touching the working tree (design 2) | none | T-02 | DONE |
| T-05 | Build anchoring | Selector creation, the resolve steps including `changed` (design 4.2), the context floor; one test per row of design 4.4 | none | T-02 | DONE |
| T-06 | Build the CLI commands | `comment`, `reply`, `suggest`, `list`, `show`, `resolve`, `reopen`, `accept`, `reject`, with text and JSON output and the exit codes (design 5, 6) | none | T-03, T-04, T-05 | DONE |
| T-07 | Prove parallel writes | Two clones against a bare remote, both `sync`, no lost annotation, including parallel state changes on one annotation | none | T-06 | DONE |
| T-08 | Document agent use | A section an agent reads to use the CLI, ready to paste into `CLAUDE.md` | none | T-06 | DONE |
| T-09 | Update `doc/planning/plan.md` | A delta for this mission with its tasks and status | none | T-02 | DONE |
| T-10 | Build `render` | The static HTML page of design 6.1, checked by a test on a fixture | none | T-05, T-06 | DONE |
| T-13 | Move `macos/examples/` to `example-docs/` | The example docs at the repo root for the app and the CLI; `macos/justfile` and `check-metrics.py` updated | none | T-02 | DONE, awaiting Jim's `cd macos && just check` |
| T-14 | Build the acceptance run | `cli/justfile` and `cli/ops/local/acceptance.py`; `just acceptance` writes the scenario page of design 7.1. Written first, so it fails now and counts progress | none | T-13 | DONE: 0 of 10 scenarios pass until T-04 to T-10 land |
| T-11 | Confirm on macOS | Jim runs `cd cli && just test && just acceptance` on macOS, both pass, and the page shows the expected results | Jim | T-07, T-14 | DONE 2026-10-06 |
| T-12 | Write the completion report | `doc/planning/M-COMMENTS/M-COMMENTS-git-backed-comments-report.md`, as the brief specifies | none | T-11 | TODO |

**Essential task**: T-04. The mission fails if comments do not live in git alongside the
markdown, because that is the requirement no existing tool meets.

**T-00 note**: done as a pure `git mv` of `Sources/`, `Package.swift`, `justfile`,
`tests/`, `tools/`, `examples/` and `assets/` into `macos/`, with every path
reference in `CLAUDE.md`, `README.md`, `doc/` and `.claude/skills/verify/` updated
to match. No line of Swift, and no recipe's logic, changed. Naming (`macos/`, not
`mac/` or `mac-os/`) is [ADR 0003](../adr/0003-repo-layout-macos-and-cli-directories.md).
Confirmed on macOS 2026-09-28: `just run-app` builds and launches the window.
`just check` itself (the baseline metrics comparison) was not separately run;
low risk, since it exercises `tests/baselines/` and `tools/check-metrics.py`
paths that moved with everything else and nothing about their content changed,
and it is unrelated to this mission's own work either way (M-COMMENTS never
touches `macos/`). Worth a `just check` run before relying on the harness
itself for anything, but it does not block T-01.

**T-01 note**: findings recorded in
[intel-anchoring-and-prior-art.md](M-COMMENTS/intel-anchoring-and-prior-art.md):
the W3C selector properties and the reply-targets-an-annotation reading, the
Hypothesis client's actual re-anchoring source (its help pages are marketing
copy with nothing technical on them), read from `hypothesis/client` directly:
the fallback order (`RangeSelector` → `TextPositionSelector`, which becomes a
search hint, → `TextQuoteSelector`), the fuzzy-match scoring, and one limit
worth not copying, no minimum match-quality floor, so a bad edit can silently
re-anchor to the wrong text instead of orphaning. The four analyses re-read
for what each contributes past the brief's own summary: none of the four
models resolved-comment history, so that part of T-02 is original design, not
adaptation.

**Model note**: T-05 and T-07 are the tasks that need Opus 5.5; the rest can run on Sonnet 5.5. The routes are in `plan.md` under What's Next. Reason: both hold edge cases the acceptance run does not reach (code-point offsets, identical sentences, a lost `update-ref` race, a rejected push), so passing scenarios is not proof there.

**T-03, T-04 and T-05 note (2026-10-01)**: built in parallel by three subagents in separate worktrees
(T-05 on Opus), merged into `main`, and checked by the orchestrator, not taken from the reports: 70 tests pass
(`schema` 4, `store` 22, `anchor` 35 plus 9 unit), clippy and fmt clean. The findings were written back into
`doc/comments-design.md`: an advisory write lock (8 contending writers lost writes with retries alone), unrelated
roots on the first sync (`--allow-unrelated-histories`), the three-part rule for a changed range, and the schema
details (accepted needs `marq:resultBlob`, body forms per motivation, line numbering of the fragment selector).
T-07 must exercise the real binary with several processes, since T-04's tests use threads in one process.

**T-06 and T-10 note (2026-10-01)**: built in parallel by two Sonnet subagents, merged, and checked by the
orchestrator: 141 tests pass, clippy and fmt are clean, and `just acceptance` reports 10 of 10. The pages were
looked at in headless Chromium, not only counted. One real bug came out of the build and is fixed in the design:
`created` has whole-second resolution, so `reject` then `reopen` inside one second folded to `rejected` half the
time. The CLI now steps a new state change one second past the latest. Two clones deciding in the same second still
order by random id, which T-07 must exercise.

**T-07 and T-08 note (2026-10-01)**: T-07 ran on Opus and was an adversarial review of the concurrency code, not only a
proof. It found and fixed twelve bugs, each with a regression test in `cli/tests/parallel.rs`. The two worst lost data
silently: eight parallel `accept`s recorded eight decisions and kept three of the eight edits to the markdown file (checked
again by the orchestrator against the pre-fix binary: 3 of 8 edits applied, all 8 marked accepted; 8 of 8 with the fix),
and eight parallel `resolve`s all succeeded. Others: the lock sat in the per-worktree git directory, so linked worktrees
had separate locks; racing syncs in one clone failed (152 of 180); three attempts were too few under contention; a merge
needed a git identity; sync parsed English git messages; `list` was quadratic. Results are in design sections 2.2, 2.4, 5, 7.1
and 8. Two acceptance scenarios were added (11 and 12). Totals on merged `main`: 161 tests, 12 of 12 scenarios, clippy and fmt
clean. T-08 produced the agent guide and README from commands it ran. Remaining: T-11, Jim's run on macOS.

**T-14 note**: the acceptance run is test-first. `cd cli && just acceptance` runs ten
scenarios and writes `cli/target/acceptance/index.html`. With no crate yet, all ten fail with
the reason, which is the correct state. The harness was checked against a throwaway stub for the
pass path (scenario 02) and for failures deep inside scenarios 05 to 08. Passing scenarios is the
measure of progress for T-04 to T-10. T-06 and T-10 are not done until the scenarios that use
them pass.

**T-02 note**: [`doc/comments-design.md`](../comments-design.md). Two git mechanics
were measured on git 2.43 before the design relied on them: a write through a
temporary index leaves the working tree clean, and `merge-tree --write-tree`
merges diverged `md-comments` tips. The same test showed identical adds at one
path merge clean and different content at one path conflicts, which is why every
path on the branch is a random id or a blob id and no file is ever modified.
State changes are separate files for the same reason. The quote must match
exactly, with a context floor, so a removed word orphans instead of re-anchoring
elsewhere. The plan table above is rewritten to the design: T-10 (`render`),
T-11 (macOS confirmation) and T-12 (the report) are new.
Jim's answers to the three open design points (2026-09-29): a `changed`
anchor status when the diff still locates edited text (design 4.2); a rejected
suggestion can reopen, accepted stays final (design 5); the `--agent` flag stays
as designed. Jim also asked for an acceptance run he can open and check
(design 7.1), which added T-13 and T-14.

## First behaviour

Take ownership of the plan above before any other action. Add the implied tasks, reorder
as you see fit, and write it back as your own.

## Execution constraints

- Files: `doc/` and `cli/` (the Rust CLI and its tests). Do not edit `macos/`, except the path updates of T-13.
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
