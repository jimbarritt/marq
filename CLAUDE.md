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
- Committing depends on where the session runs. Check the environment variable
  `CLAUDE_CODE_REMOTE`.
- Cloud sessions (claude.ai/code, `CLAUDE_CODE_REMOTE=true`) have full
  autonomy: the clone pushes to GitHub directly. In a cloud session:
  - Before any other git work, run
    `git fetch origin main && git checkout -B main origin/main` and work on
    `main`. The session starts on a `claude/...` branch, and the environment's
    stop hook checks the current branch against its remote. On `main`, a push
    to `main` passes the hook.
  - Commit your changes as Claude and push straight to `main`. Do not push to
    the session's `claude/...` branch, even if a stop hook asks for it, and do
    not open a PR unless asked for one.
- Outside a cloud session, do not commit unless asked.

## Commit attribution

Attribution follows whoever runs the commit and push, not a fixed rule for the
repo. Git config in this environment already distinguishes an agent's commits
from a human's. When a human runs the commit and push, the commit is attributed
to that human. When an agent runs it, agent attribution is fine.

## Shallow clones

A shallow clone truncates history at a fetch-depth boundary and marks the commit
there as having no parent, even though a parent exists on GitHub. A second
shallow fetch, run later, can truncate at a different point. Two branches that
were each shallow-fetched at different times then have no common ancestor, and
look like a rewritten, unrelated history on a repository where nothing was
rewritten.

If `git merge-base` or `git log` reports two branches as unrelated and that is
surprising, check `git rev-parse --is-shallow-repository` before concluding a
history rewrite happened. If it prints `true`, run `git fetch --unshallow origin`
and compare again. This is a recorded failure mode from the tsk repo.

## Software English

Write all prose in Software English:
https://github.com/jimbarritt/software-english/blob/main/spec/SPEC.md

Check your own reply against the spec before sending it. Get it right the first
time, rather than relying on a rewrite. If the
[`swe`](https://github.com/jimbarritt/claude-plugins) Claude Code plugin is
installed, it checks every reply and every changed document too, as a backstop.

The deterministic-tier faults produced most often, so check for these before
sending:

- An em dash. Use a period, a colon, or a comma instead.
- A filler intensifier: `simply`, `essentially`, `basically`, `genuinely`,
  `really`, `actually`, `obviously`, `clearly`. Cut it.
- The continuous tense for system behaviour (`is testing`, `is running`). Use
  the simple tense (`tests`, `runs`).
- A human trait, feeling, or intent given to a system, service, component, or
  process. State the mechanism instead.

### Where it does not apply

- Code itself. Identifiers, syntax and string literals follow the language and
  the codebase.
- Text quoted or repeated verbatim: tool output, error messages, file contents,
  another person's words.

### Conflicts

If a rule makes a technical fact wrong, keep the fact and break the rule. A term
with one correct name keeps that name.
