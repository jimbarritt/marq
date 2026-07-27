---
name: verify
description: Check a visual or layout change in Marq — which instrument to reach for, in cost order. Use when a change touches template.html, table layout, print/PDF output, or anything about how the app looks, and when asked to verify, check, measure, or confirm a rendering change.
---

# Verifying a change to Marq

Marq is a window, so every check has to be asked of the app rather than read off
the terminal. The instruments below are ordered by cost. **Start at the top.**
Most questions are answered before you reach the bottom, and the expensive
habit in this repo has been reaching for measurement before reaching for the
source.

## 0. Read the source first

The entire renderer is `Sources/marq/Resources/template.html`, vendored CSS
included. Anything of the form "why is this element grey / centred / spaced like
that" is a `grep`, not a probe. One such question was answered by a JS probe
patched into the app twice, at eleven times the cost of the `grep` that settled
it (`tr:nth-child(2n)`, in the vendored GitHub stylesheet).

## 1. `just problems FILE` — is anything wrong at all?

Screen and print layout, reporting only the failures: columns whose text is
being broken mid-word, tables overflowing their wrapper, tables pinned at the
minimum font scale, images that did not load. Plus each table's fill percentage
and font scale.

This is the first thing to run after any layout change.

## 2. `just probe FILE [WIDTH]` / `just probe-print FILE` — the numbers

Full metrics as JSON, from inside the real WKWebView. Per table: rendered width,
what percentage of the available measure it fills, the font scale print settled
on, and per column the header, allocated width, max-content width and
`naturalMin` — the width of its longest unbroken word.

`width < naturalMin` is where a header prints as "Manifes/t". It is reported as
`breaksWords`, and it is the most load-bearing number in the file.

`probe` measures the window (state a width — allocation is a function of the
available measure). `probe-print` measures the A4 page, without exporting
anything. Use `probe-print` for any question about paper: a Chrome copy of the
template cannot see WebKit's 1.25× print scaling and will answer confidently and
wrongly.

## 3. `just check` — did anything else move?

Compares every fixture's metrics to `tests/baselines/`: table shape, fill
percentage, font scale, broken columns, and the exported page count. Several
bugs here were regressions of an earlier fix — "Manifest" breaking, then
"Status" once "Manifest" was fixed — each found days later by a human looking at
a screenshot. This catches that class in one command.

Run it before saying a layout change is done. If the change is intended,
`just bless` and read the diff before committing the new baselines.

## 4. Look at it

- `just shot FILE` — PNG of the whole screen render, headless. Prints the path.
- `just pdf FILE` — export and report page count and per-page text extent.
- `just pages .harness/test.pdf 3 5` — those pages as PNGs, to actually look at.

## 5. Measure the printed page

- `just pdf-columns PDF PAGE` — the column borders as printed, and the gaps
  between them: the real column widths, in points.
- `just pdf-chars PDF PAGE Manifest` — per-glyph bounds for a matching string.
  A broken word is found here, not by squinting at a render.
- `just pdf-text PDF [PAGE]` — lines with their x extents.

## Rules

- **Never test against something you did not just build.** Every recipe above
  depends on `swift build`. `template.html` is copied into the SPM bundle at
  build time, and the template is read once at launch, so a Marq that has been
  open all session is rendering code that is no longer on disk. Two separate
  sessions lost time to exactly this.
- **State the width.** Table allocation is a function of the available measure;
  metrics taken at whatever size the window happened to open at compare to
  nothing.
- **Don't build a new instrument before checking these.** If one is genuinely
  missing, add it to `pdftool` or as a flag on the app, where the next session
  will find it — not to a scratchpad that is deleted when this session ends.
  Eleven instruments were built and thrown away across three sessions; two were
  independently reinvented.
- **Clean up.** Headless runs carry a watchdog (`--timeout`, 60s default), but
  anything launched by hand does not: `just kill-probes`. A print job with no
  supervisor once ran 42 minutes at 100% CPU and wrote a 17 GB file.
