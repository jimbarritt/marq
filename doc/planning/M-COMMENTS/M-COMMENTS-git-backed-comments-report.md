# Report: M-COMMENTS, git-backed comments and suggestions for markdown

| Field | Value |
|---|---|
| Mission | [M-COMMENTS](../M-COMMENTS-git-backed-comments.md) |
| Outcome | Done on Linux. Attempt 1 of 3. Not yet run on macOS: task T-11 is a run by Jim and has not happened. |
| Written | 2026-10-01, in a Claude Code cloud session |
| Final state | `main` at the commit that adds this file. 161 tests pass. `cd cli && just acceptance` reports 12 of 12 scenarios. `cargo clippy --all-targets` and `cargo fmt --check` are clean. |

## What I did

The mission's objectives, and where each stands:

| Objective in the brief | State |
|---|---|
| `doc/comments-design.md`, one reason per decision | Done. About 490 lines, written before any code, then corrected after each build phase from what the build found. |
| A JSON Schema, every fixture validates | Done. Two schemas, 10 valid and 32 invalid fixtures, 4 tests. |
| A CLI that builds and runs on Linux and on macOS | Linux done. macOS not run (T-11). |
| Comment on a word, on a line, a reply, a suggestion, stored as W3C annotations on `md-comments` | Done. Scenarios 01 to 04. |
| List as JSON and text, anchors resolved against current content | Done. |
| An edit elsewhere keeps the anchor; removed text reports orphaned and does not re-attach | Done. Scenarios 05 to 08, 44 anchoring tests. |
| Resolve, accept, reject change state, nothing deleted | Done. State is a fold over separate state-change files. |
| Accepting applies the edit to the working-tree file | Done. It never commits. |
| Two clones add in parallel, sync, lose nothing | Done, and proven with many real processes. Twelve bugs were fixed to get there. |
| All proven by tests that run with one command in a cloud session | Done. `cd cli && cargo test`, under a minute once built. |

Work beyond the brief, at Jim's request: the repository split into `macos/` and `cli/` (ADR 0003), the example documents moved to
`example-docs/`, a `render` command that writes a static HTML page, an acceptance run that produces a page to read
(`cd cli && just acceptance`), two ADRs, and an agent guide (`doc/comments-agent-guide.md`) ready to paste into a `CLAUDE.md`.

How it was built: this session wrote the design and the acceptance run first, scaffolded the crate, then ran seven subagents in
separate git worktrees, each on its own files, and merged their branches after running the tests itself. T-05 (anchoring) and
T-07 (parallel writes) ran on Opus 5.5, the rest on Sonnet 5.5.

| Task | Model | Tokens | Time |
|---|---|---|---|
| T-03 schema | Sonnet | 83k | 3 min |
| T-04 storage and sync | Sonnet | 127k | 8 min |
| T-05 anchoring | Opus | 118k | 8 min |
| T-10 render | Sonnet | 101k | 5 min |
| T-06 commands | Sonnet | 166k | 7 min |
| T-08 docs | Sonnet | 90k | 3 min |
| T-07 parallel writes | Opus | 293k | 35 min |

About 980k subagent tokens in total. Tasks that did not depend on each other ran at the same time.

## What the briefing failed to give me

- **What a cloud session can build.** The brief did not say that marq cannot build on Linux (AppKit, WebKit and PDFKit have no Linux port) or that
  Rust builds fine through the proxy. I measured both. The brief's file list, `doc/`, `Package.swift` and a CLI directory, assumed a Swift CLI.
- **The shape of the output.** The brief required "a CLI that lists as JSON and as readable text" and nothing more. The command names, what
  `comment` prints, the JSON shape, the exit codes and the text format were all decisions, and several were first made implicitly in tests.
- **How the environment treats commits.** The auto-mode permission check blocked commits and pushes that touched `CLAUDE.md`, `.claude/settings.json`,
  and a `git reset --hard`, with no way for a cloud session to learn the rule beforehand. A human message in chat did not clear the block. Jim made the settings
  commit himself and typed a commit command that I then ran for him.
- **Machine facts.** The container's global git config has `push.negotiate true`, which prints errors on local pushes. `just` is not installed.
  Git is 2.43, which fixes what `merge-tree --write-tree` can do. None of it was written down, and each cost a probe.
- **Which task needs which model.** The brief had no guidance. I judged T-05 and T-07 to need Opus. The result supports that: the Opus agent
  found a contradiction in the design's step 4 that I had not caught, and T-07 found two bugs that lost data silently.
- **How to run agents in parallel without collisions.** The brief's single-owner plan did not say that parallel agents need separate worktrees,
  disjoint files, and a shared scaffold, or that `Cargo.lock` and `lib.rs` are the places they collide.

## What I found wrong in the briefing

- **"One file per annotation, with a unique ID" does not make parallel merges conflict-free.** Measured on git 2.43: two clones that add the
  same path with identical content merge clean, and different content at one path conflicts. Two clones that each write a first comment also have
  unrelated root commits, which `merge-tree` refuses without `--allow-unrelated-histories`. The constraint holds only with random ids, content-hash
  names, no file ever modified, and that flag.
- **"Resolve changes the annotation's state" and "parallel writes merge without a conflict" pull against each other.** A state kept inside the
  annotation file is a modification, which can conflict. State had to move into separate state-change files, and the annotation file carries no state.
- **"Out of scope: real-time collaboration" was read as settled.** Jim said he wants it later. A local socket daemon does not provide it, because it
  needs a network relay between machines. ADR 0002 records both points.
- **"Files: `doc/`, `Package.swift`"** is wrong for a Rust crate. The brief now says `doc/` and `cli/`.

## What I found wrong in my own work

Design errors, found by the agents or the acceptance run, not by me before building:

- **Design 4.2 step 4 contradicted itself.** The Opus agent showed that a deleted letter and a deleted word both leave an empty range under a greedy
  common prefix, so my rule turned a deleted word into a changed anchor on the next word. It wrote a three-part rule, now in the design.
- **No write lock, then the lock in the wrong place.** I designed ten `update-ref` retries as enough. Eight contending writers lost writes. The lock
  that fixed it sat in the per-worktree git directory, so linked worktrees had separate locks. T-07 moved it to the common directory.
- **Three sync attempts and no sync lock.** Racing syncs in one clone failed 152 times in 180. Three attempts starved under contention.
- **Output shapes left open.** The acceptance run forced the decisions: ids printed by the creating commands, `list --json` as one array, the
  `annotation`, `state`, `anchor`, `stateChanges` and `replies` keys.
- **A false statement in the design:** "Cargo already ignores `target/`". It does not without a `.gitignore`.
- **Whole-second `created` timestamps.** `reject` then `reopen` inside one second folded to `rejected` half the time. The fix steps `created`
  forward, which makes a burst of N decisions run up to N seconds ahead of the clock. A sub-second timestamp would be the clean fix and would change
  the format. Both are in the design's limits.

Process mistakes, all mine, all corrected in later commits:

- `git add -A` committed the three agent worktrees as embedded repositories and I pushed it (`5777867`). They were pointers to a commit already on
  `main`, so nothing was exposed. Removed in `9acc547` and ignored.
- A compiled `.pyc` went into `5538c80` and was removed in `f25e764`.
- Several times a prefix replacement left a table row with the original reason attached to the wrong cell. A check I ran over every design table
  row, for the right cell count, now finds none.
- In conversation, before the build, I stated that a daemon had "no capability gap left to close". That was my reading of the brief, not Jim's
  decision, and I retracted it.

## What is not verified

- **macOS.** Nothing in `cli/` has run on macOS. The crate needs Rust 1.89 or later (for `File::lock`) and git 2.38 or later.
- **The T-13 move.** `macos/examples/` became `example-docs/`. `macos/justfile` and `tools/check-metrics.py` point at the new path. A cloud session
  cannot build Swift, so `cd macos && just check` has not been run since.
- **The GitHub install line** in the agent guide. The file form was verified. The GitHub form depends on the repository being reachable.
- **A real translated git**, and **a stale `refs/heads/md-comments.lock` from a real kill**: proven only with a fake `git` and a hand-made lock file.
- **`flock` on a network filesystem.** Documented as unreliable, not tested.

## After the report

On 2026-10-01 Jim asked for the accepted-suggestion display to be fixed, and it was. Until then an accepted suggestion listed as `changed`. It is now
anchored to the text it put in the file, using a new `marq:resultStart` on the `accepted` state change. A deletion reports `applied`. Four tests, an
extended acceptance scenario 03 and the render page cover it. Scenario 03's new checks fail on the previous binary and pass on this one.

## Limits that remain

The design's section 8 lists them. The ones a person will meet first: two clones deciding in the same
second order by random id, identically on every clone; renamed files keep their comments under the old path; CRLF working copies change positions
between clones; every write in one repository takes the lock, about 31 ms each in a debug build.
