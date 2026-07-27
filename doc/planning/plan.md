# Marq — Implementation Plan

## What's Next

- **Next:** Task 5 — decide whether the orphaned header row is worth a JS pagination heuristic (Delta: PDF Export Fidelity)
- **Sub-doc:** (none)
- **Blockers:** None
- **Context:** See [Checkpoint: Session 2026-07-27](#checkpoint-session-2026-07-27)

## Summary

| Delta | Task | Status |
|-------|------|--------|
| [Delta: Table Layout](#delta-table-layout) | [1. Replace GitHub max-content table sizing](#task-1-replace-github-max-content-table-sizing) | ✓ DONE |
| | [2. Fair column width allocation](#task-2-fair-column-width-allocation) | ✓ DONE |
| | [3. Full-width tables and gutter tracking](#task-3-full-width-tables-and-gutter-tracking) | ✓ DONE |
| | [4. Header alignment](#task-4-header-alignment) | ✓ DONE |
| [Delta: PDF Export Fidelity](#delta-pdf-export-fidelity) | [1. Verify export against a real WebKit print run](#task-1-verify-export-against-a-real-webkit-print-run) | ✓ DONE |
| | [2. Pelican section page break](#task-2-pelican-section-page-break) | IN PROGRESS |
| | [3. Confirm CSS px to points mapping](#task-3-confirm-css-px-to-points-mapping) | ✓ DONE |
| | [4. Respect system page setup](#task-4-respect-system-page-setup) | TODO |
| | [5. Header row can be orphaned at the foot of a page](#task-5-header-row-can-be-orphaned-at-the-foot-of-a-page) | TODO |
| | [6. Print font scale solved in one step](#task-6-print-font-scale-solved-in-one-step-so-minimums-did-not-fit) | ✓ DONE |
| [Delta: Links and Navigation](#delta-links-and-navigation) | [1. Open external links in the browser](#task-1-open-external-links-in-the-browser) | ✓ DONE |
| | [2. Link destination status bar](#task-2-link-destination-status-bar) | ✓ DONE |
| | [3. Verify internal anchor links in the app](#task-3-verify-internal-anchor-links-in-the-app) | TODO |
| [Delta: Release 1.2.8](#delta-release-128) | [1. Commit and push outstanding work](#task-1-commit-and-push-outstanding-work) | TODO |
| | [2. Publish 1.2.8](#task-2-publish-128) | TODO |

## Delta: Table Layout

### Task 1: Replace GitHub max-content table sizing
- ✓ DONE — Tables lay out as real tables instead of `display: block; width: max-content`
  - WebKit does not clamp a max-content block against `max-width`; Blink does. Wide tables ran off the page in Marq while looking correct in Chrome.
  - Cells also set `vertical-align: top` — github-markdown-css never overrides the `middle` default.

### Task 2: Fair column width allocation
- ✓ DONE — `layoutTables()` measures columns and allocates max-min fair
  - Browser auto layout shares surplus in proportion to `max-content − min-content`, so one prose column starves every short one.
  - Measurement uses a hidden clone inside `#content` so it inherits the same border and padding cascade.
  - A second pass gives back any overshoot from collapsed borders — 1px was enough to leave a scrollbar parked under every table.

### Task 3: Full-width tables and gutter tracking
- ✓ DONE — Tables break out of the 980px prose measure and track the gutter
  - `availableTableWidth()` measures from the content column's left edge rather than `calc(100vw - …)`; `100vw` includes the scrollbar and hard-coding the gutter geometry broke on toggle.
  - `toggleGutter()` re-runs layout — the gutter moves the left edge by 64px.

### Task 4: Header alignment
- ✓ DONE — `th:not([align])` is left-aligned, matching GitHub
  - github-markdown-css never sets `text-align` on `th`, so a table written `|---|` fell through to the UA default of `center` and printed centred headers over left-aligned data. A table written `|:---|` looked right, which is why only one of the two showed it.
  - The `:not([align])` guard matters: marked writes declared alignment as an `align` attribute, and a presentational attribute loses to any author rule — a bare `th { text-align: left }` would silently override every `:---:` in the document.

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

### Task 6: Print font scale solved in one step, so minimums did not fit
- ✓ DONE — `fitTableFontForPrint` now measures at the scale it is considering and refines
  - A column's minimum does not scale with the font: padding and borders are fixed pixels, 11px per column, 66px across six. Solving `scale = avail / neededAtFullSize` therefore overshoots — a scale meant to bring the minimums to 640px left them at 662.
  - The allocator then handed every column ~1.4% less than its longest word, and the narrowest header printed as "Manifes/t" in a column that looked roomy.
  - A second, separate cause of the same symptom: `allocateForPrint` floored columns at *exactly* their measured minimum, and shares the surplus by content volume — so the narrow columns sat on their floor with no tolerance, and the print renderer sets the same text a shade wider than the screen measurement of it. `PRINT_COLUMN_SLACK` (3%) now pads the floor, and `fitTableFontForPrint` targets `avail / PRINT_COLUMN_SLACK` so the two agree.
  - Fixing only the first cause moved the break from the reference table's "Manifest" to the prose table's "Status" — the symptom is one column landing a hair short, and there was more than one way to land there.
  - Separately: pinning `#content` to the printable width did nothing at first — `flex: 1` grew it straight back. Measurement was happening at ~1060px for a 653px page.

### Task 5: Header row can be orphaned at the foot of a page
- TODO — A table starting near the page bottom prints its header alone, with the first row overleaf
  - WebKit honours `thead { display: table-header-group }` but ignores `break-after: avoid` on `thead`, `break-inside: avoid` on `tr`, and the same on `td`/`th` — all three tried and measured, none changed the break.
  - `tbody tr:first-child { break-before: avoid }` does change it, for the worse: the header is kept on the page and the first row is split mid-cell instead.
  - The remaining option is a JS heuristic: estimate each table's offset in the paginated flow and set `break-before: page` when its header would land in the last stretch of a page. Fragile — the estimate has to track every other break rule — so it is not obviously worth it.

### Task 4: Respect system page setup
- TODO — Margins are hard-coded to 36pt in `generatePDF`
  - Consider reading the user's Page Setup defaults instead.

## Delta: Links and Navigation

### Task 1: Open external links in the browser
- ✓ DONE — Added `decidePolicyFor` to cancel `http(s)` link navigations and hand them to `NSWorkspace`
  - Previously WebKit navigated itself to the URL, replacing the document, then `didFinish` tried to inject markdown into a page with no `renderMarkdown`.

### Task 2: Link destination status bar
- ✓ DONE — `#link-status` shows the destination bottom-left on hover
  - Shows the authored href; a relative `.md` link resolves to a long absolute `file://` path that tells the reader nothing.

### Task 3: Verify internal anchor links in the app
- TODO — TOC added to `examples/test.md`; all 23 anchors verified resolving in the repro
  - Still needs a click-through in the real app to confirm jumping works and does not reload the document.

## Delta: Release 1.2.8

### Task 1: Commit and push outstanding work
- TODO — `MarqApp.swift`, `template.html`, `examples/test.md` are modified and uncommitted

### Task 2: Publish 1.2.8
- TODO — Version already bumped to 1.2.8 in `justfile` and `Info.plist`
  - `just publish` builds, releases to GitHub, updates the tap, then waits for SPACE before pushing the cask.
  - 1.2.7 is live and contains the single-page-PDF and table-clipping bugs, so it should be superseded.

## Checkpoint: Session 2026-07-27

**What was completed this session:**
- Diagnosed and fixed table rendering: WebKit's `max-width` clamping difference, top alignment, full-window width, and max-min fair column allocation (`template.html`).
- Replaced `WKWebView.createPDF()` with `printOperation(with:)`. The old export produced a single page 11,279pt tall (~4m); the new one paginates properly to A4.
- Added an `@media print` block: hides screen chrome, tightens cell padding, wraps long code lines, adds `break-inside` rules.
- Rewrote print table layout to run the real allocator against the paper width rather than handing off to browser auto layout, which starved prose columns to ~53pt.
- Added font scaling for tables that do not fit — by width, and then by height. Height scaling took the worst test table from 5.7 pages to 1.9.
- Added external link handling and a hover status bar.
- Rebuilt `examples/test.md` as a proper test corpus: a TOC with 23 anchors, a prose-heavy six-column table, and a reference-style six-column table with long identifiers and URLs.
- Released 1.2.7 (superseded — see Delta: Release 1.2.8).

**State of the project:**
Table rendering on screen is settled and verified. PDF export is substantially better but every recent change is verified only in a Chrome repro harness, not in WebKit's own print output. Three files are modified and uncommitted, with the version already bumped to 1.2.8.

**Immediate next priorities:**
1. Export both test documents from the real app and check the result.
2. Resolve the pelican page break, or confirm the image-intrinsic-height theory.
3. Click through the TOC anchors in the app.
4. Commit, push, and publish 1.2.8.

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
