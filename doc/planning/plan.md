# Marq — Implementation Plan

## What's Next

- **Next:** Mission [M-COMMENTS](M-COMMENTS-git-backed-comments.md) — git-backed comments and suggestions for markdown
- **Sub-doc:** [M-COMMENTS-git-backed-comments.md](M-COMMENTS-git-backed-comments.md) — the mission owns its own task breakdown (T-00 onward)
- **Blockers:** None. Jim to confirm `cd macos && just check` still passes after T-13 moved `macos/examples/` to `example-docs/`. The acceptance run exists (T-14): `cd cli && just acceptance`, 0 of 10 scenarios pass. T-03, T-04 and T-05 start next, and each one's measure is the acceptance count. Design: [doc/comments-design.md](../comments-design.md).
- **Model:** build T-03, T-04, T-06, T-08 and T-10 on Sonnet 5.5. T-05 (anchoring) and T-07 (parallel writes and `sync`) need Opus 5.5, because a subtle error there can pass the acceptance run and still be wrong. A session cannot change its own model, so use one of two routes. **Route 1 (default):** the session hands T-05 or T-07 to a subagent started with the Agent tool's `model: "opus"`, with a prompt that points at `doc/comments-design.md`, the task row, and `cd cli && just acceptance`, and asks for the unit tests the design lists. **Route 2:** if no subagent can run, stop and tell Jim: "switch to Opus before T-05" (`/model claude-opus-5-5`), and switch back after T-07.
- **Status (2026-10-01):** T-00 to T-10, T-13 and T-14 are done and merged. 166 tests pass and `cd cli && just acceptance` reports 12 of 12. The completion report is [M-COMMENTS/M-COMMENTS-git-backed-comments-report.md](M-COMMENTS/M-COMMENTS-git-backed-comments-report.md). **Remaining: T-11, Jim runs `cd cli && just test && just acceptance` on macOS** (Rust 1.89 or later, git 2.38 or later), and `cd macos && just check` for the T-13 move. Follow-ups, none blocking: `created` has whole-second resolution (design 5 and 8); the GitHub install line in the agent guide is not tested; the marq UI mission that reads this CLI's `list --json`. Agent branches `worktree-agent-*` are merged and can be deleted. Commit with explicit paths, never `git add -A`: agent worktrees live under `.claude/worktrees/`.
- **Jim, 2026-10-03:** the rendered page is enough to move to the real thing. Idea, as said: use the render in a GitHub pipeline to render docs to a GitHub Pages site, with an interactive page where the comments can be toggled on and off. Not yet scoped or decided.
- **Next mission, drafted 2026-10-03:** [M-COMMENTS-UI](M-COMMENTS-UI-comments-in-marq.md), comments in the marq window. Jim answered its five questions the same day.
- **Local build checks, 2026-10-03 (partial):** toolchain meets the minimums (Rust 1.93.1, git 2.50.1, Swift 6.2.4). `cd cli && just test`: 18 passed, 2 failed in `tests/parallel.rs`. `sync_merges_with_no_git_identity_anywhere` read the author as `Jim Barritt|jmdb@AMA-3.local` where it expects `marq-comments|`. Git guesses a name from the macOS account record and an email from the hostname even with the global config off. Fixed: the test's unidentified syncs set the four `GIT_AUTHOR_*` and `GIT_COMMITTER_*` variables to empty (`sync_with_no_guessable_identity` in `tests/parallel.rs`). `six_thousand_records_list_in_linear_time` took 21 to 24s against a limit of 8s. Cause: the time is linear (about 7 ms per comment), and a first read of a freshly written loose git object costs about 4.5 ms on this Mac, against 0.05s for the same 6,000 objects in a pack. Fixed in the CLI, test unchanged: `cat_file_batches` in `cli/src/store.rs` splits the object read over 8 parallel `git cat-file --batch` processes from 256 objects up, and joins the outputs in order. Measured on 2,000 fresh objects: 4.4s with one process, 0.5s with eight. After the fix `cd cli && cargo test --no-fail-fast` passes all of `parallel` (20 of 20). `keys_are_repo_relative_from_a_subdirectory` in `tests/store.rs` failed because `document_key` canonicalised the repository root but not the path argument, so `/var/folders/...` never matched `/private/var/folders/...`. A CLI bug for any path through a symlink on macOS. Fixed: `document_key` now also tries the canonicalised argument (`canonical_path` in `cli/src/store.rs`). `cargo test --no-fail-fast` is green. Then, 2026-10-05: `cd cli && just acceptance` passes 12 of 12. `cd macos && just check` first reported `missingImages` 0 to 1 for `example-docs/test.md`: its title image `../assets/logo.svg` broke when T-13 moved the file, and it now reads `../macos/assets/logo.svg`. After that, `just check` passes with the baselines unchanged, and `just problems`, `just shot` and `just pdf` run. The build, run and verify loop works end to end on this Mac. Both local checks are done. T-11 of M-COMMENTS still needs Jim's own run.
- **Git switch, 2026-10-03 (not built):** the global `~/.claude/settings.json` denied `Bash(git add *)`, which blocked staging. Deny rules beat ask and allow at every scope, so a repo setting cannot override a user deny. The global rule is now `Bash(git add -A*)`, `Bash(git add --all*)` and `Bash(git add .)`, so `git add` with explicit paths works. `~/.claude/settings.json` is a symlink to `~/Code/github/jimbarritt/dotfiles/home/claude/settings.json`, and the edit is uncommitted in that repo. The planned per-repo switch (`.claude/git-switch.sh` with `jq`, toggling `Bash(git add *)` in the gitignored `.claude/settings.local.json`, plus `git-on` and `git-off` recipes in `macos/justfile`) was not written. The auto-mode classifier denied the script twice as self-modification, because the file it edits holds the session's own permission rules. A Bash permission rule from Jim, or Jim writing the script, clears it. The switch is optional.
- **Push over HTTPS, 2026-10-03:** pushes over SSH needed a 1Password approval for the SSH key each time. `gh auth setup-git` (global) now makes `gh` the credential helper for `https://github.com`, using the keyring token with `repo` scope. In this repo, `git config url."https://github.com/".insteadOf git@github.com:` sends the `origin` remote over HTTPS, and the remote URL itself is unchanged. Other repos stay on SSH. The trade: the token is readable by any process running as Jim, where the 1Password agent asked for approval per use.
- **Context:** See [Delta: Git-backed Comments](#delta-git-backed-comments-mission-m-comments) below
- **Before verifying any layout change:** `just problems`, then `just check`. See the `/verify` skill.

## Summary

| Delta | Task | Status |
|-------|------|--------|
| [Delta: Git-backed Comments](#delta-git-backed-comments-mission-m-comments) | [Mission M-COMMENTS](M-COMMENTS-git-backed-comments.md) | TODO |
| [Delta: Verification Harness](#delta-verification-harness) | [1. Make the app observable](#task-1-make-the-app-observable) | ✓ DONE |
| | [2. Measurement for exported PDFs](#task-2-measurement-for-exported-pdfs) | ✓ DONE |
| | [3. Recipes and golden baselines](#task-3-recipes-and-golden-baselines) | ✓ DONE |
| | [4. Supervision](#task-4-supervision) | ✓ DONE |
| | [5. Write it down](#task-5-write-it-down) | ✓ DONE |
| | [6. Screen table overflow at 960px](#task-6-screen-table-overflow-at-960px) | TODO |
| [Delta: PDF Export Fidelity](#delta-pdf-export-fidelity) | [1. Verify export against a real WebKit print run](#task-1-verify-export-against-a-real-webkit-print-run) | ✓ DONE |
| | [2. Pelican section page break](#task-2-pelican-section-page-break) | IN PROGRESS |
| | [3. Confirm CSS px to points mapping](#task-3-confirm-css-px-to-points-mapping) | ✓ DONE |
| | [4. Respect system page setup](#task-4-respect-system-page-setup) | TODO |
| | [5. Header row can be orphaned at the foot of a page](#task-5-header-row-can-be-orphaned-at-the-foot-of-a-page) | TODO |
| | [6. Print font scale solved in one step](#task-6-print-font-scale-solved-in-one-step-so-minimums-did-not-fit) | ✓ DONE |

Archived Deltas: see the [archive index](archive/index.md)

## Delta: Git-backed Comments (Mission M-COMMENTS)

Mission briefing: [M-COMMENTS-git-backed-comments.md](M-COMMENTS-git-backed-comments.md).

A CLI, separate from the marq app, stores comments and suggestions on markdown
files as W3C Web Annotations committed to an orphan `md-comments` branch
alongside the file's own history. Marq stays a viewer; the mission covers the
CLI only, and a later mission covers the marq UI that reads these annotations.

The mission carries its own task plan (T-00 through T-12) and decision
authority. Status and progress are tracked in the mission file itself, not
duplicated here — see its `## Plan` table.

**Repo reshape (2026-09-27):** the Swift app moved from the repo root into
`macos/` (T-00), so a fresh `cli/` has a clean sibling to build the Rust CLI in.
Every entry below this line, and every dated Checkpoint in this file, predates
that move and states paths as they were at the repo root at the time it was
written — `Sources/marq/`, `Package.swift`, `justfile`, `tests/`, `tools/`,
`examples/`, `assets/` are now `macos/Sources/marq/`, `macos/Package.swift`, and
so on. On 2026-09-29 (T-13) `macos/examples/` moved again, to `example-docs/` at
the repo root. `CLAUDE.md`, `README.md` and `doc/` outside this file carry the current
paths.

## Delta: Verification Harness

Built from the recommendations in
`~/Code/github/jimbarritt/ag-seminar/notes/interesting-claude-sessions/marq-harness-analysis.md`:
eleven instruments were improvised and thrown away across three sessions, two of
them reinvented from scratch later, and eight verifications turned out to prove
nothing. The one that survived was a CLI flag on the app itself, so the rest are
built the same way.

### Task 1: Make the app observable
- ✓ DONE — `--export-png`, `--dump-metrics [--print]`, `--width/--height`, `--settle`, `--timeout`, `--help`
  - `marqMetrics()` in `template.html` reports what the layout did, from inside the real WKWebView: per table its fill percentage, font scale, and per column the allocated width against `naturalMin`, the width of its longest unbroken word. `width < naturalMin` is exactly where "Manifes/t" comes from, and it is now a boolean.
  - `--print` measures the A4 page without exporting anything. It reproduces the hard-won numbers from the print work in one command: printable width 653 CSS px, tables at 100% fill, scales 0.6 and 0.682.
  - Metrics wait on `document.fonts.ready` before measuring. Without it the same command returned 110.61% and 120.32% fill for the same table depending on how warm the font cache was — the first baselines recorded the cold figure.

### Task 2: Measurement for exported PDFs
- ✓ DONE — `Sources/pdftool`, an executable in the same package
  - `info`, `pages`, `text`, `chars`, `vlines`. Replaces `render.swift`, `lines.swift`, `chars.swift`, `vlines.swift` and `bars.swift`, three of which had been written twice.
  - PDFKit's `characterBounds(at:)` is not indexed like `page.string` — newlines have no glyph, so the index runs behind by one per line already passed. Feeding it the string index returns another glyph's rectangle, drifting further down the page; extracted text came back as a shuffled crossword and every position read off it was wrong.

### Task 3: Recipes and golden baselines
- ✓ DONE — `just problems / probe / probe-print / shot / pdf / pages / pdf-text / pdf-chars / pdf-columns / check / bless`
  - All depend on `swift build`, which makes the stale-build and stale-bundle class of error structurally impossible.
  - `just check` compares fixture metrics to `tests/baselines/`. Verified by breaking `MIN_PRINT_TABLE_SCALE` on purpose: it caught the font scale change, six columns starting to break words, and the page count going 12 → 13.

### Task 4: Supervision
- ✓ DONE — headless runs carry a watchdog, default 60s
  - It runs on a background queue and calls `exit()` rather than `terminate()`, so a jammed main thread — the exact failure that once ran 42 minutes at 100% CPU and wrote 17 GB — cannot stop it firing. Verified.
  - `just kill-probes` for anything started by hand.

### Task 5: Write it down
- ✓ DONE — `CLAUDE.md` and a `/verify` skill
  - The repo had neither. `/verify` orders the instruments by cost and starts with "read the source first", which is the habit the analysis identified as most expensive.

### Task 6: Screen table overflow at 960px
- TODO — the harness's first finding, not a regression
  - `just problems examples/test.md` reports the prose table overflowing its wrapper on screen: it renders 1078px wide against an 896px measure, 120% of it, so it sits behind a horizontal scrollbar.
  - Print is unaffected. The question is whether a scrollable table is the intended answer at that width, or whether the screen path should scale the font as print does.

## Delta: PDF Export Fidelity

### Task 1: Verify export against a real WebKit print run
- ✓ DONE — `marq file.md --export-pdf out.pdf` exports headlessly and quits, so export is scriptable
  - The previous round was verified only in Chrome. Worse, the PDFs being judged came from an app instance launched before the print-layout work: the template is read once at launch, so a running Marq keeps rendering with the template it started with.
  - Verified by rasterising the PDF and reading character bounds out of it with PDFKit, rather than by eye.

### Task 2: Pelican section page break
- IN PROGRESS — Large blank area before the pelican image grid
  - Measured: page 1 had 565pt free and the table is 420pt tall, so it should have fitted. Something reserves more space during pagination than the final render uses.
  - Added explicit `break-inside: auto` on `.table-wrapper`, table and tbody — unverified.
  - Second theory if that fails: WebKit reserves the SVGs' intrinsic height during pagination while painting them scaled, in which case no break rule helps and explicit image sizing is needed.

### Task 3: Confirm CSS px to points mapping
- ✓ DONE — They do not map 1:1. WebKit lays a print job out 1.25x larger than the paper and shrinks it
  - Measured with probe elements in the printed PDF: `width: 400px` printed 320.25pt, `width: 100%` printed the full 523pt. So 523pt of paper is 653 CSS px of layout.
  - Passing points straight through laid every table out a fifth too narrow and shrank table text to fit a width that was never the constraint — the reference table hit the 60% floor where 73% fits.
  - `layoutTablesForPrint` now takes points and applies `PRINT_SHRINK_FACTOR`; print column widths are percentages and the content column is pinned to the printable width while measuring, so measurement matches the page that gets printed.

### Task 4: Respect system page setup
- TODO — Margins are hard-coded to 36pt in `generatePDF`
  - Consider reading the user's Page Setup defaults instead.

### Task 5: Header row can be orphaned at the foot of a page
- TODO — A table starting near the page bottom prints its header alone, with the first row overleaf
  - WebKit honours `thead { display: table-header-group }` but ignores `break-after: avoid` on `thead`, `break-inside: avoid` on `tr`, and the same on `td`/`th` — all three tried and measured, none changed the break.
  - `tbody tr:first-child { break-before: avoid }` does change it, for the worse: the header is kept on the page and the first row is split mid-cell instead.
  - The remaining option is a JS heuristic: estimate each table's offset in the paginated flow and set `break-before: page` when its header would land in the last stretch of a page. Fragile — the estimate has to track every other break rule — so it is not obviously worth it.

### Task 6: Print font scale solved in one step, so minimums did not fit
- ✓ DONE — `fitTableFontForPrint` now measures at the scale it is considering and refines
  - A column's minimum does not scale with the font: padding and borders are fixed pixels, 11px per column, 66px across six. Solving `scale = avail / neededAtFullSize` therefore overshoots — a scale meant to bring the minimums to 640px left them at 662.
  - The allocator then handed every column ~1.4% less than its longest word, and the narrowest header printed as "Manifes/t" in a column that looked roomy.
  - A second, separate cause of the same symptom: `allocateForPrint` floored columns at *exactly* their measured minimum, and shares the surplus by content volume — so the narrow columns sat on their floor with no tolerance, and the print renderer sets the same text a shade wider than the screen measurement of it. `PRINT_COLUMN_SLACK` (3%) now pads the floor, and `fitTableFontForPrint` targets `avail / PRINT_COLUMN_SLACK` so the two agree.
  - Fixing only the first cause moved the break from the reference table's "Manifest" to the prose table's "Status" — the symptom is one column landing a hair short, and there was more than one way to land there.
  - Separately: pinning `#content` to the printable width did nothing at first — `flex: 1` grew it straight back. Measurement was happening at ~1060px for a 653px page.

## Checkpoint: Session 2026-07-27 (harness)

**What was completed this session:**
- Built the verification harness the analysis note recommended: metrics and PNG export as CLI flags on the app, `pdftool` for measuring exported PDFs, eleven `just` recipes, golden baselines with `just check`, a watchdog on every headless run, and `CLAUDE.md` plus a `/verify` skill.
- Three defects found and fixed in the harness itself while building it, each of the "verification that verifies nothing" kind: PDFKit's newline-shifted character indexing, line clustering anchored on whichever glyph came first, and metrics measured before the web fonts had loaded.

**State of the project:**
`just check` is green on both fixtures and stable across runs. Nothing about the shipped rendering behaviour changed — the only edits to the render path are additive. Uncommitted: `MarqApp.swift`, `main.swift`, `template.html`, `Package.swift`, `justfile`, `README.md`, new `Sources/pdftool/`, `tools/`, `tests/baselines/`, `CLAUDE.md`, `.claude/`.

**Immediate next priorities:**
1. Orphaned table header at a page foot (PDF Export Fidelity, Task 5) — a decision, not a fix.
2. The pelican page break (PDF Export Fidelity, Task 2) — now cheap to test: `just pdf` then `just pages`.
3. Screen table overflow at 960px (Verification Harness, Task 6) — the harness's first finding.

## Checkpoint: Session 2026-07-27 (late evening)

**What was completed this session:**
- Zoom controls: View menu driving `webView.pageZoom` over fixed steps, a local event monitor for bare Cmd-=, level persisted, and export pinned to 1.0.
- Internal navigation: history entries became positions rather than file paths, so Back undoes an anchor jump — and also returns you to your reading position when coming back out of a linked document.

**State of the project:**
Both open items in Delta: Zoom Controls and Delta: Links and Navigation are closed and verified. Uncommitted: `MarqApp.swift`, `template.html`, `plan.md`. Released version is still 1.2.9, so neither feature is in the tap yet.

**Immediate next priorities:**
1. Orphaned table header at a page foot (PDF Export Fidelity, Task 5) — a decision, not a fix.
2. The pelican page break (PDF Export Fidelity, Task 2), still IN PROGRESS with an unverified theory.
3. Respect system page setup (PDF Export Fidelity, Task 4).

## Checkpoint: Session 2026-07-27 (evening)

**What was completed this session:**
- Added `marq file.md --export-pdf out.pdf`: renders, exports and quits. Export is now scriptable, which is what made everything below checkable.
- Found that WebKit lays a print job out 1.25x larger than the paper and shrinks it, so the printable width in points is not the width in CSS px. Every table had been sized for a page a fifth narrower than the real one.
- Made print column widths percentages of a full-width table, pinned `#content` to the printable width while measuring, and fixed `flex: 1` quietly undoing that pinning.
- Rewrote the print font fit to measure at the scale it is considering and refine, and gave the column floors 3% of slack. Between them these stopped headers printing as "Manifes/t" and "Statu/s".
- Left-aligned `th:not([align])`, matching GitHub — the centred headers were the UA default showing through.
- Released 1.2.9 from `3d08307`, superseding a 1.2.8 that had been cut an hour before the print work landed.

**State of the project:**
Table rendering is settled on screen and on paper, and both are verified against real WebKit print output rather than a Chrome harness. Working tree clean at `3d08307`, 1.2.9 live and in the tap.

**Immediate next priorities:**
1. Zoom controls (Delta: Zoom Controls) — the next piece of work.
2. Orphaned table header at a page foot (PDF Export Fidelity, Task 5) — needs a decision, not a fix; CSS cannot do it.
3. The pelican page break (PDF Export Fidelity, Task 2) and Back after an anchor jump (Links, Task 4) are both open and untouched today.

## Checkpoint: Session 2026-07-27 (afternoon)

**What was completed this session:**
- Diagnosed and fixed table rendering: WebKit's `max-width` clamping difference, top alignment, full-window width, and max-min fair column allocation (`template.html`).
- Replaced `WKWebView.createPDF()` with `printOperation(with:)`. The old export produced a single page 11,279pt tall (~4m); the new one paginates properly to A4.
- Added an `@media print` block: hides screen chrome, tightens cell padding, wraps long code lines, adds `break-inside` rules.
- Rewrote print table layout to run the real allocator against the paper width rather than handing off to browser auto layout, which starved prose columns to ~53pt.
- Added font scaling for tables that do not fit — by width, and then by height. Height scaling took the worst test table from 5.7 pages to 1.9.
- Added external link handling and a hover status bar.
- Rebuilt `examples/test.md` as a proper test corpus: a TOC with 23 anchors, a prose-heavy six-column table, and a reference-style six-column table with long identifiers and URLs.
- Released 1.2.7 (superseded — see Delta: Release 1.2.9).

**State of the project:**
Table rendering on screen is settled and verified. PDF export is substantially better but every recent change is verified only in a Chrome repro harness, not in WebKit's own print output. Three files are modified and uncommitted, with the version already bumped to 1.2.8.

**Immediate next priorities:**
1. Export both test documents from the real app and check the result.
2. Resolve the pelican page break, or confirm the image-intrinsic-height theory.
3. Click through the TOC anchors in the app.
4. Commit, push, and publish (became 1.2.9 — see the evening checkpoint).

---

## Implementation Notes

### Architecture

Marq is a Swift/AppKit app wrapping a `WKWebView`. `Sources/marq/Resources/template.html` holds the entire renderer — vendored `marked`, `highlight.js`, `mermaid`, KaTeX, plus all the layout JavaScript. Swift injects markdown by calling `renderMarkdown(md, resetScroll)`.

### Table layout

The core problem is that CSS automatic table layout optimises for the wrong thing in documents: it shares surplus width in proportion to how much each column *could* use, so a single prose column starves every short one. `layoutTables()` measures each column at `max-content` and `min-content` using a hidden clone and allocates the width itself.

Screen and paper need different splits. On screen the surplus is large and an equal split among unsatisfied columns looks tidy. On paper the remainder is small, and an equal split is the worst choice — a one-sentence column and a four-paragraph column end up the same width, so the table sprawls over pages. Print therefore floors columns at their natural (unbroken) minimum and shares the remainder by content volume.

### Print font scaling

A table's text area falls roughly with the square of the font size, which makes font size a far stronger lever on page count than any reallocation of column widths. Tables scale down when too wide for the paper, and again when too tall, floored at 60% of body size.

### Verifying an export

`marq file.md --export-pdf out.pdf` renders, exports and quits. Judge the result
by measurement, not by eye: PDFKit's `page.characterBounds(at:)` gives per-glyph
rectangles in points, so "does the table fill the measure" is a number, and a
probe element of known CSS width printed into the page gives the px-to-point
factor directly. Two rounds of wrong conclusions came from eyeballing renders —
a table that looked full width was 80% of it.

The template is read once, at launch. A running Marq keeps using the template it
started with, so verifying against an app instance that has been open all session
tests code that is no longer on disk.

### Testing gotchas

Verifying print CSS by forcing the `@media print` block on at page load is invalid: its `!important` rules override the inline `width: max-content` that `measureColumns()` sets on its clone, so every column measures identical to min-content and the allocation silently degenerates. The repro harness extracts the print rules into a `media="not all"` stylesheet and enables it *after* screen layout, which is the real sequence.

Twice this session a bug survived because it was verified against a scratchpad artefact rather than the committed one — including a test corpus that only exercised the case that already passed. Verify the thing that ships.
