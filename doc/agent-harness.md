# The layout harness

Marq is a window. Nothing about it is observable from a terminal, so for a long
while every check on how it renders had to be improvised: a standalone WebKit
host, a Chrome copy of `template.html` served on localhost, AppleScript driving
the Print dialog, `screencapture`. Eleven such instruments were built and thrown
away across three days of work on table layout and PDF export; two were
reinvented from scratch weeks later, because they had only ever lived in a
temporary directory. Eight verification results turned out to prove nothing —
stale builds, a stale app instance, a debug binary with no bundle identifier, a
Chrome reproduction that could not model WebKit's print pipeline.

The one instrument that survived all of it was a flag on the app itself:
`--export-pdf`, three edits, working first time. This harness is the rest of
that idea. **The fastest way to make a GUI observable is to give it a mouth, not
to build it eyes from outside.**

## What it consists of

### 1. The app reports on itself

`marqMetrics()` in `template.html` walks the rendered document and returns JSON.
It runs inside the same `WKWebView` that draws the window, which is the point: a
Chrome reproduction of the template cannot see that WebKit lays a print job out
1.25× larger than the paper and shrinks it, and so answers questions about paper
confidently and wrongly.

For each table it reports the rendered width, what percentage of the available
measure it fills, the font scale the print path settled on, and per column: the
header, the allocated width, the max-content width, and `naturalMin` — the width
of its longest unbroken word.

`width < naturalMin` is exactly the condition that prints a header as
"Manifes/t". It is reported as `breaksWords`, and it is the most load-bearing
number in the file. A `problems` block summarises the failures — broken words,
overflowing tables, tables pinned at the minimum font scale, images that never
loaded — so the common case is one glance rather than a JSON read.

Exposed as:

```
marq FILE --dump-metrics OUT        # "-" for stdout
marq FILE --dump-metrics OUT --print  # measure the A4 page, not the window
marq FILE --export-png OUT.png
marq FILE --export-pdf OUT.pdf
marq FILE --width N --height N      # so a measurement is reproducible
marq FILE --settle S --timeout S
```

`--print` reports the page without exporting anything, which turns "will this
table fit, and at what scale" into a four-second question.

### 2. `pdftool` measures the exported PDF

A second executable in the same package (`Sources/pdftool`), so `swift build`
builds it alongside the app and it can never be stale. It replaces `render.swift`,
`lines.swift`, `chars.swift`, `vlines.swift` and `bars.swift`, three of which had
been written twice.

| Command | Answers |
|---|---|
| `info` | page count, page size, per-page text extent — *does the table fill the measure* |
| `pages` | render pages to PNG, to actually look at |
| `text` | lines with their x extents |
| `chars` | per-glyph bounds, optionally only for a matching string |
| `vlines` | column borders as printed, and the gaps between them — the real column widths |

### 3. Recipes, so the plumbing is never retyped

```
just problems FILE              # anything listed is a bug — start here
just probe FILE [WIDTH]         # screen metrics as JSON
just probe-print FILE           # page metrics as JSON
just shot FILE                  # whole-document PNG
just pdf FILE                   # export, and report the shape of the result
just pages PDF FROM TO          # pages as PNGs
just pdf-text / pdf-chars / pdf-columns
just check [FIXTURES]           # compare metrics to tests/baselines/
just bless [FIXTURES]           # record the current metrics as the baseline
just kill-probes
```

Every one of them depends on `swift build`. That is not convenience: it makes
the stale-build class of error structurally impossible. `template.html` is
copied into `.build/.../marq_marq.bundle` at build time, and the template is
read once at launch, so a Marq that has been open all session is rendering code
that is no longer on disk. Two separate sessions lost time to exactly that.

### 4. Golden baselines

`just check` compares each fixture's metrics to `tests/baselines/`: table shape,
fill percentage, font scale, broken columns, and the exported page count.

Several bugs in this repo were regressions of an earlier fix — "Manifest"
breaking, then "Status" once "Manifest" was fixed, then header alignment — and
each was found days later by a human looking at a screenshot. This catches that
class in one command, before anyone sees it.

The baselines record only what should be stable. Exact pixel heights are
deliberately excluded: they move with any typographic change and would make the
check noise.

### 5. Supervision

Every headless run carries a watchdog, 60 seconds by default. It runs on a
background queue and calls `exit()` rather than `terminate()`, so a jammed main
thread cannot stop it firing — which matters, because the failure it exists for
was a print job that ran 42 minutes at 100% CPU and wrote a 17 GB file before
anyone noticed. `just kill-probes` handles anything started by hand.

## Using it

Cheapest instrument first. Most questions are answered before the bottom of this
list, and reaching for measurement before reaching for the source has been the
single most expensive habit in this repo.

1. **Read `template.html`.** The whole renderer is one file, vendored CSS
   included. "Why is this element grey" is a `grep`. One such question was
   instead answered by patching a JS probe into the app twice, at roughly eleven
   times the cost of the `grep` that settled it (`tr:nth-child(2n)`, in the
   vendored GitHub stylesheet).
2. `just problems FILE` — is anything wrong at all?
3. `just probe` / `just probe-print` — the numbers behind it.
4. `just check` — did anything else move?
5. `just shot`, `just pdf`, `just pages` — look at it.
6. `just pdf-columns`, `just pdf-chars` — measure the printed page.

The `/verify` skill in `.claude/skills/` says the same thing to an agent.

If an instrument is genuinely missing, add it to `pdftool` or as a flag on the
app, where the next session will find it — not to a scratchpad that is deleted
when the session ends.

## Things that have caught this harness out

Three defects were found in the harness itself while it was being built, each of
the "verification that verifies nothing" kind. They are worth knowing about
because they are all invisible: the tool returns plausible numbers either way.

- **Measure only after `document.fonts.ready`.** Column widths come from
  laid-out text, so a run against a cold font cache measures fallback metrics.
  The same command reported 110.61% and 120.32% fill for the same table
  depending on cache warmth, and the first recorded baselines captured the cold
  figure. `marqMetricsWhenSettled()` waits; that is what makes the baselines
  stable.
- **PDFKit's `characterBounds(at:)` is not indexed like `page.string`.** Line
  breaks have no glyph, so the bounds index runs one behind for every newline
  already passed. Feeding it the string index returns a different glyph's
  rectangle, drifting further down the page — extracted text comes back looking
  like a shuffled crossword, and any position read off it is a confident lie.
- **Clustering glyphs into lines cannot anchor on whichever glyph comes first.**
  One line spans about 5pt of glyph bottoms: an apostrophe rides 3pt above
  x-height, a descender drops below it. Anchoring on the first glyph seen
  measures every other glyph from a mark that may be at either extreme, and
  strands the descenders in a line of their own — `g g , , g p` under the row
  they belong to.

And from the layout work that prompted all this:

- WebKit lays a print job out **1.25× larger than the paper and shrinks it**, so
  the printable width in points is not the width in CSS px: 523pt of A4 is 653
  CSS px of layout. Passing points straight through sizes every table a fifth
  too narrow.
- Forcing the `@media print` block on at page load invalidates any measurement
  taken afterwards: its `!important` rules override the inline
  `width: max-content` that `measureColumns()` sets on its clone, so every
  column measures at min-content and the allocation silently degenerates.
- A debug binary has no bundle identifier, so `defaults write com.jimbarritt.marq`
  is read by nothing. Values passed as `-key value` arrive in the argument domain
  as **strings**, so `object(forKey:) as? Int` silently returns nil.
