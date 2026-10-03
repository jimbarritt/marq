# Mission: Comments in the marq window

| Field | Value |
|---|---|
| ID | M-COMMENTS-UI (provisional: Jim names it) |
| Territory | marq |
| Assignee | cloud agent for the code, a macOS runner for the proof (see open question 5) |
| Blocked by | Jim's answers to the open questions below |

## Objective

- Marq opens a markdown file in a git repo that holds comments on `md-comments` and shows them. Each anchored
  thread's text carries a highlight. Its card sits in a right-hand column, level with the thread's first
  highlight. The text column keeps its width and gets no gaps, as on the `render` page.
- Clicking highlighted text makes its card active: the card's highlight shows, and the card scrolls into view
  if it is off screen. Clicking a card makes its highlights active. Hovering a card highlights its text, as on
  the `render` page. Clicking anywhere else clears the active thread.
- The numbers on cards and the grey subscript numbers after highlighted text are one option, on or off, set from
  the View menu. The setting persists across launches.
- The highlight is one colour, subtler than the `render` page's `rgba(255, 212, 0, 0.30)`, with a stronger
  version for the active thread. The colour is one token per theme in `template.html`, so a change is one line.
- A card has a faint background and no border.
- Orphaned threads and applied deletions show in a section after the document, as on the `render` page.
- The line-number gutter gives the same numbers at the same heights with comments on and off.
  `cd macos && just check` passes with its existing baselines unchanged.
- The comments refresh when the markdown file changes and when `md-comments` moves (a `sync`, a new comment
  from an agent).
- A file with no comments, a file outside a git repo, and a machine with no `marq-comments` binary all render
  exactly as marq does today, with no error shown.
- All of the above is proven by an acceptance run that needs no human. It ends in one page with a pass or fail
  per scenario and a screenshot of each, so Jim reads one page, not the app.

## Purpose

M-COMMENTS built the store and the CLI, and the `render` page settled the look. Jim wants comments in the app he
reads markdown in. Agents write comments through the CLI. Marq shows them to Jim beside the text, where he reads.

## Intelligence

- The guideline is the `render` page: `cli/src/render.rs` (`render_page`, `STYLE`, `LAYOUT_SCRIPT`) and its
  design, `doc/comments-design.md` section 6.1. Jim accepted it on 2026-10-03. Its decisions carry over: the
  rendered view, one highlight colour, subscript numbers after the marked text, cards in a right-hand rail placed
  by script with a stacked fallback on narrow windows, a grey camera icon for an image inside a highlight, and the
  quiet card style. This mission changes four of them at Jim's request: numbers become an option, the colour gets
  subtler, cards lose the border, and a click selects a thread.
- `doc/adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md`: marq spawns `marq-comments list FILE --json` and reads the
  JSON. No FFI, no daemon.
- The JSON shape: `doc/comments-design.md` section 6.2. Each thread carries `annotation`, `state`, `anchor`
  (`status`, `start`, `end`, `line`, `column`, `text`, and `original` for `changed`), `stateChanges` and
  `replies`. `start` and `end` count Unicode code points in the file on disk.
- The renderer: `macos/Sources/marq/Resources/template.html`. `renderMarkdown(md, resetScroll)` renders with
  `marked`, wraps each top-level block in `<div data-source-line="N">` (`renderWithLineNumbers`), then runs
  mermaid, KaTeX, `layoutTables()` and `buildGutter()`.
- The gutter: `doc/line-number-gutter-arch.md`. Its failure modes section lists how alignment has broken before.
  Two apply here. `buildGutter()` reads only `#content > [data-source-line]`, so any wrapper element around the
  blocks blanks the gutter. A `ResizeObserver` on `#content` rebuilds the gutter, so content that changes height
  after render (cards stacked under blocks, a mark that wraps a line) moves it.
- The harness: `doc/agent-harness.md`, `macos/justfile` and the `/verify` skill. `marqMetrics()` reports on the
  render from inside the real `WKWebView`. `--dump-metrics`, `--export-png`, `--width` and `--height` make a run
  headless and reproducible. `just check` compares metrics to `macos/tests/baselines/`.
- The CLI acceptance run, `cli/ops/local/acceptance.py`, is the model for the new one: standard-library Python,
  temporary git repos, the real binary, and an HTML page of results.
- The highlight colour. Notion highlights commented text in yellow
  ([Notion help](https://www.notion.com/help/comments-mentions-and-reminders)). Notion does not publish the CSS
  value of its comment highlight, and three searches found no source for it. What third parties document is
  Notion's yellow background colour: `#FBF3DB` or `#FAF3DD` in light mode and `#372E20` in dark mode
  ([matthiasfrank.de](https://matthiasfrank.de/en/notion-colors/),
  [notioneers.eu](https://notioneers.eu/en/insights/notion-colors-codes)), and a yellow selection colour of
  `rgba(233, 168, 0, 0.2)` light and `rgba(255, 220, 73, 0.5)` dark. Start from these. To get Notion's exact
  value, inspect a commented span in Notion in a browser's developer tools.

## Decision authority

You decide:

- How a code-point range in the file maps to text in the rendered DOM, within the constraints.
- How the JSON reaches the page: an extra argument to `renderMarkdown`, or a separate call after it.
- The CSS and the script, within the look the `render` page settled.
- The shape of the `comments` block in `marqMetrics()` and the harness flags that drive it.
- The task order below, and any task the plan implies.

Jim decides:

- The open questions below.
- Anything that adds to, or changes, the look the `render` page settled, beyond the four changes in the
  Intelligence section.
- Any change to the CLI's JSON shape. Prefer reading what `list --json` prints today.

## Constraints

- Marq stays a viewer. No adding, replying, resolving, accepting or rejecting from the app in this mission.
- Marq reads comments only by spawning the CLI (ADR 0002). It never reads `md-comments` itself.
- The line-number gutter does not change. No element wraps the top-level blocks, nothing without
  `data-source-line` sits among the children of `#content`, and the gutter entries are identical with comments
  on and off. The rail lives outside `#content`.
- A highlight marks only the text its anchor covers. When the anchored text cannot be found in the rendered
  DOM, the card shows without a highlight. It never highlights other text.
- `injectMarkdown()` in `MarqApp.swift` rewrites relative image paths before rendering, so the markdown the page
  receives is not the file on disk, and code-point offsets into the file do not hold in it. The mapping accounts
  for this.
- Search (`clearSearch`) restores `originalHTML`. The highlights survive a search and its clearing.
- PDF export and `--export-pdf` print the document as today, with no comments (see open question 3).
- No change to rendering for a file with no comments. `just check` baselines stay as they are.
- A spawned CLI that hangs, fails or prints bad JSON costs the render nothing: marq renders the markdown first
  and adds comments when the JSON arrives, with a timeout.
- British English in code, comments and docs.

## Out of scope

- Writing comments from marq (add, reply, resolve, accept, reject).
- Real-time collaboration. Jim wants it later (ADR 0002 records it).
- The GitHub Pages idea of 2026-10-03.
- Any change to `cli/`, except a bug the acceptance run finds in it, fixed with a test.
- Homebrew packaging of the CLI.

## Open questions for Jim

1. The mission's ID and name.
2. The numbers option: on or off by default? A View menu item, a keyboard shortcut, or both?
3. Is there also a switch to hide all comments? PDF export with comments: never, or an option?
4. How marq finds `marq-comments`. A Mac app started from the Finder or the Dock gets a `PATH` of
   `/usr/bin:/bin:/usr/sbin:/sbin`, so a binary in `~/.cargo/bin` or `/opt/homebrew/bin` is not found by name.
   Options: a fixed list of places plus `MARQ_COMMENTS_BIN`; a path in preferences; the CLI built into
   `Marq.app` by `just bundle`.
5. Who runs the macOS acceptance. A cloud session cannot build Swift. Options: Jim runs one command on his Mac
   and reads the page; or a GitHub Actions workflow on a macOS runner runs it on every push to `main`, with the
   page as a build artifact. The second needs no human. It adds `.github/workflows/`, and on a private repo
   macOS runner minutes cost ten times Linux minutes.

## Plan

| ID | Task | Objective, in short | Delegated to | Blocked by | Status |
|---|---|---|---|---|---|
| T-00 | Read the intelligence | The `render` page, `template.html`'s render path, the gutter doc and the harness understood | none | open questions | TODO |
| T-01 | Write the design | A new section of `doc/comments-design.md` for marq: the DOM mapping, the data flow, the toggles, the metrics block. One reason per decision | none | T-00 | TODO |
| T-02 | Build the acceptance run first | `macos/ops/local/comments-acceptance.py` and a `just` recipe: fixture repos made with the real CLI, headless marq runs, assertions on metrics, one HTML page with screenshots. Fails until T-03 to T-06 land, and counts progress | none | T-01 | TODO |
| T-03 | Swift: fetch and refresh | Find the CLI, spawn `list FILE --json` off the main thread with a timeout, pass the JSON to the page, refresh on file change and on `md-comments` change, the View menu option and its persistence, a harness flag for each toggle | none | T-01 | TODO |
| T-04 | Template: range to DOM | Map each anchor's code-point range to rendered text and wrap it in marks, across emphasis, code spans, links, headings, list items, tables, and ranges that cross blocks. Unit fixtures checked in headless Chromium in a cloud session | none (Opus) | T-01 | TODO |
| T-05 | Template: rail, cards, interaction | The rail outside `#content`, the stacked fallback, the subtler colour tokens, borderless cards, numbers as a class toggle, click and hover selection | none | T-04 | TODO |
| T-06 | Metrics for comments | A `comments` block in `marqMetrics()`: per thread its status, marked text, mark and card rectangles, the vertical offset between them, overlaps, the active thread; and the gutter entries, for comparison on and off | none | T-04 | TODO |
| T-07 | Prove on macOS | The acceptance run passes on macOS and `just check` passes with baselines unchanged | Jim or CI (question 5) | T-02 to T-06 | TODO |
| T-08 | Write the completion report | `doc/planning/M-COMMENTS-UI/` report, as below | none | T-07 | TODO |

**Essential task**: T-04. Every other part of the look exists on the `render` page. Marking the right text in a
DOM that `marked` built, with no source offsets, is the part with no precedent in this repo.

**Model note**: T-04 runs on Opus 5.5. A wrong mapping passes a casual look and marks the wrong word. The rest
runs on Sonnet 5.5.

## Acceptance: what the run checks

Each scenario builds a temporary git repo with the real `marq-comments`, runs headless marq on it at a fixed
width, and asserts on the `comments` and gutter metrics. None needs a human.

- A comment on one word: one highlight, its text equals the anchor's `text`, its card's top within a few pixels
  of the highlight's top.
- A comment on a whole line, and a comment on a word inside `**bold**`, `` `code` ``, a link, a heading, a list
  item and a table cell.
- A range that crosses two blocks: highlights in both, one card.
- Two threads close together: cards do not overlap, and the second moves down rather than covering the first.
- A reply: one card with the reply inside it.
- A `changed` anchor, an orphaned thread, an accepted suggestion and an applied deletion, each shown as on the
  `render` page.
- Numbers on and numbers off: subscript and card numbers present, then absent, and nothing else moves.
- A simulated click on a highlight: its card active. A simulated click on a card: its highlights active.
- The gutter: identical entries with comments on and off, on every fixture.
- No comments, no git repo, no CLI, a CLI that hangs: the render matches the plain baseline.
- A new comment added while marq runs: the card appears without a reload.
- A narrow window: cards stacked under their blocks, the gutter unchanged.
- `just check`: the existing baselines unchanged.

## First behaviour

Take ownership of the plan above before any other action. Add the implied tasks, reorder as you see fit, and
write it back as your own.

## Execution constraints

- Files: `macos/`, `doc/`, and `cli/` only for a bug fix with a test.
- A cloud session cannot build or run Swift. Do what a cloud session can prove there (the DOM mapping in headless
  Chromium against a copy of `template.html`'s script), and leave the proof of record to T-07. CLAUDE.md
  records that a Chrome reproduction is the wrong instrument for print. It is adequate for screen layout and for
  the mapping.
- Never test against something you did not just build (CLAUDE.md).
- Commit with explicit paths, never `git add -A`.
- Commit and push your work before the session ends.
- Attempt limit: 3.

## Report on completion

Write the report to `doc/planning/M-COMMENTS-UI/` (the directory follows the ID Jim chooses). Outcome: done,
failed, or blocked, with attempt count. Your account: what you did, what this briefing failed to give you, and
what you found wrong in it, including in your own work. Feedback only.
