# Marq

Native macOS markdown viewer: Swift/AppKit wrapping a `WKWebView`.

- `Sources/marq/MarqApp.swift` — the app: window, menus, navigation, PDF export, CLI.
- `Sources/marq/Resources/template.html` — the whole renderer. Vendored `marked`,
  `highlight.js`, `mermaid`, KaTeX, plus all the layout JavaScript. Swift injects
  markdown by calling `renderMarkdown(md, resetScroll)`.
- `Sources/pdftool/main.swift` — measurement for exported PDFs.
- `doc/planning/plan.md` — task tracking, and the source of truth for it.

## Verifying a change

Marq is a window. Nothing about it is observable from a terminal unless the app
is asked to say what it did, so it has been given a mouth: see `just --list`,
and the `/verify` skill for which instrument answers which question.

**Read before you measure.** The renderer is one file. A question like "why are
these rows grey" is a `grep` of `template.html` — including its vendored CSS —
not a probe. Reaching for measurement before reaching for the source has been
the single most expensive habit in this repo.

**Never test against something you did not just build.** Every `just` harness
recipe depends on `swift build` for this reason:

- `template.html` is copied into `.build/.../marq_marq.bundle` at build time, so
  editing it and running without rebuilding measures the old file.
- The template is read once, at launch. A Marq that has been open all session is
  still rendering with the template it started with.

**Judge by measurement, not by eye.** A table that looked full width was at 80%
of the measure, twice, in separate sessions. `just problems` and `just check`
answer that in a number.

## Gotchas that have cost real time

- WebKit does not lay a print job out at the paper size. It lays it out **1.25×
  larger and shrinks it** (`PRINT_SHRINK_FACTOR`), so the printable width in
  points is not the width in CSS px — 523pt of A4 is 653 CSS px of layout.
  Points passed straight through size every table a fifth too narrow.
- A Chrome reproduction of `template.html` cannot model that print pipeline. It
  was the right instrument for screen layout and the wrong one for paper, and it
  produced confident, wrong answers for a whole session. `just probe-print`
  measures inside the real engine instead.
- Forcing the `@media print` block on at page load invalidates any measurement:
  its `!important` rules override the inline `width: max-content` that
  `measureColumns()` sets on its clone, so every column measures at min-content.
- A debug binary has no bundle identifier, so `defaults write com.jimbarritt.marq`
  is read by nothing. Values passed as `-key value` arrive in the argument domain
  as **strings**, so `object(forKey:) as? Int` silently returns nil.
- Measure only after `document.fonts.ready`. Column widths come from laid-out
  text, and a cold font cache gives a different answer from a warm one —
  `marqMetricsWhenSettled()` waits, and that is what makes the baselines stable.
- PDFKit's `characterBounds(at:)` is not indexed like `page.string`: newlines
  have no glyph, so the index runs behind by one per line already passed.

## Conventions

- British English in code, comments and docs.
- Comments explain *why*, especially where the code encodes something measured
  rather than assumed. Do not add comments that restate the line below them.
- Do not commit unless asked.
