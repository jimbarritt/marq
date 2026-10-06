# Mission: Comments in the marq window

| Field | Value |
|---|---|
| ID | M-COMMENTS-UI |
| Territory | marq |
| Assignee | cloud agent for the code; a GitHub Actions macOS runner and Jim's Mac for the proof |
| Blocked by | none |

## Objective

- Marq opens a markdown file in a git repo that holds comments on `md-comments` and shows them. Each anchored
  thread's text carries a highlight. Its card sits in a right-hand column, level with the thread's first
  highlight. The text column keeps its width and gets no gaps, as on the `render` page.
- Clicking highlighted text makes its card active: the card's highlight shows, and the card scrolls into view
  if it is off screen. Clicking a card makes its highlights active. Hovering a card highlights its text, as on
  the `render` page. Clicking anywhere else clears the active thread.
- The numbers on cards and the grey subscript numbers after highlighted text are one option. It is off by
  default. A View menu item with a keyboard shortcut switches it. The setting persists across launches.
- A second option shows or hides all comments: highlights, subscripts and cards. It is on by default. A View menu
  item with a keyboard shortcut switches it. The setting persists across launches.
- PDF export follows the window: with comments shown, the PDF carries the highlights and the cards; with comments
  hidden, the PDF is the document alone, exactly as today.
- `just bundle` builds `marq-comments` into `Marq.app`. Marq uses that copy, so the app and the CLI it reads always
  match. The CLI stays installable on its own, for agents and the command line, as today.
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
- With comments hidden, PDF export and `--export-pdf` print the document exactly as today. With comments shown,
  the cards fit on the paper beside or below their text and the existing print measurements (`just probe-print`,
  `pdftool`) report no new problem. The print path's scale (`PRINT_SHRINK_FACTOR`, CLAUDE.md) applies to the rail
  as it does to tables.
- No change to rendering for a file with no comments. `just check` baselines stay as they are.
- Marq looks for the CLI in this order: `MARQ_COMMENTS_BIN` (the harness sets it), the copy inside `Marq.app`,
  then a debug build's `cli/target/`. No `PATH` lookup: an app started from the Finder or the Dock gets a `PATH`
  of `/usr/bin:/bin:/usr/sbin:/sbin`.
- A spawned CLI that hangs, fails or prints bad JSON costs the render nothing: marq renders the markdown first
  and adds comments when the JSON arrives, with a timeout.
- British English in code, comments and docs.

## Out of scope

- Writing comments from marq (add, reply, resolve, accept, reject).
- Real-time collaboration. Jim wants it later (ADR 0002 records it).
- The GitHub Pages idea of 2026-10-03.
- Any change to `cli/`, except a bug the acceptance run finds in it, fixed with a test.
- Homebrew packaging of the CLI.

## Jim's answers (2026-10-03)

1. The ID is M-COMMENTS-UI.
2. Numbers are off by default, switched from a View menu item with a keyboard shortcut.
3. A separate switch shows or hides all comments, from a View menu item with a keyboard shortcut. PDF export
   follows it.
4. Bundle the CLI in `Marq.app`. It still installs on its own and runs from the command line.
5. Both: a GitHub Actions workflow on a macOS runner runs the acceptance on every push to `main`, and the same
   `just` recipe runs on Jim's Mac.

## Plan

| ID | Task | Objective, in short | Delegated to | Blocked by | Status |
|---|---|---|---|---|---|
| T-00 | Read the intelligence | The `render` page, `template.html`'s render path, the gutter doc and the harness understood | none | open questions | DONE 2026-10-06 |
| T-01 | Write the design | Section 9 of `doc/comments-design.md`: data flow, offset steps, the mapping contract, cards and rail, options, refresh, metrics, print. One reason per decision | none | T-00 | DONE 2026-10-06, 9.2 and 9.3 revised after an Opus review (15 defects) |
| T-02 | Build the acceptance run first | `macos/ops/local/comments-acceptance.py` and a `just` recipe: fixture repos made with the real CLI, headless marq runs, assertions on metrics, one HTML page with screenshots. Fails until T-03 to T-06 land, and counts progress | none | T-01 | DONE 2026-10-06 |
| T-03 | Swift: fetch and refresh | Find the CLI in the order above (an empty `MARQ_COMMENTS_BIN` disables comments), spawn `list FILE --json` off the main thread with a 5 second limit, pass `{threads, source, bom, edits}` to `applyComments` as a `callAsyncJavaScript` argument, record the image rewrites as edits, discard stale results, refresh on file change and on a `refs/heads` directory event (9.6), the two View menu options (`⇧⌘C`, `⌥⌘C`) with persistence read by `object(forKey:) != nil`, the harness flags of 9.5, and the headless wait for the first comments outcome | none | T-01 | DONE 2026-10-06 |
| T-04 | Template: range to DOM | The four offset steps of 9.2, the source-equality and text-equality gates, and token-guided alignment with `paintComments()` of 9.3, across emphasis, code spans, links, headings, list items, tables, blockquotes, CRLF, tabs and ranges that cross blocks. Unit fixtures checked in headless Chromium in a cloud session | none (Opus) | T-01 | DONE 2026-10-06 |
| T-05 | Template: rail, cards, interaction | The rail outside `#content` and the 1009px switch to stacked cards (9.4), the colour tokens, borderless cards, the safe markdown instance for card bodies, the orphan section after `#page-wrapper`, numbers and visibility as classes on `<html>`, click, hover and scroll behaviour of 9.5 | none | T-04 | DONE 2026-10-06 |
| T-06 | Metrics for comments | The `comments` block of 9.7 in `marqMetrics()`, present only after a payload, and the gutter entries for comparison on and off | none | T-04 | DONE 2026-10-06 |
| T-09 | Print with comments | PDF export follows the show switch, in the stacked layout of 9.8; cards fit the paper; `just probe-print` reports no new problem; with comments hidden the print baselines are unchanged | none | T-05 | DONE 2026-10-06 |
| T-10 | Bundle the CLI | `just bundle` builds `marq-comments` in release and copies it into `Marq.app/Contents/MacOS/`; the standalone install still works | none | T-03 | DONE 2026-10-06 |
| T-11 | CI on a macOS runner | `.github/workflows/` runs `cd cli && just test`, the comments acceptance and `cd macos && just check` on every push to `main`, and uploads the results page | none | T-02 | DONE 2026-10-06, workflow written and not yet run on GitHub |
| T-12 | Isolate the baselines | `just check`, `probe`, `problems`, `shot` and `pdf` run with `MARQ_COMMENTS_BIN=` empty so this repository's own `md-comments` branch cannot change a baseline; the comments recipes set it to the built binary | none | T-03 | DONE 2026-10-06 |
| T-07 | Prove on macOS | The acceptance run and `just check` pass in CI and on Jim's Mac | CI and Jim | T-02 to T-06, T-09 to T-12 | IN PROGRESS: local run passes 2026-10-06 (acceptance 29 of 29, `just check`, `just test-mapping`); CI run and Jim's run pending |
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
- Comments shown and hidden: hidden matches the plain render and the plain baseline exactly.
- PDF with comments shown and hidden: hidden matches today's print metrics; shown reports no broken words and no
  overflow.
- The bundled `Marq.app` finds its own `marq-comments` with an empty `PATH`.
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

- Files: `macos/`, `doc/`, `.github/workflows/`, and `cli/` only for a bug fix with a test.
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
