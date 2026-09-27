# 3. Repo layout: `macos/` and `cli/` as top-level directories, named without a hyphen

Date: 2026-09-27

## Status

Accepted.

## Context

Mission [M-COMMENTS](../planning/M-COMMENTS-git-backed-comments.md) adds a
separate Rust CLI to a repository that, until now, held a single Swift app at
its root. T-00 moved the existing app (`Sources/`, `Package.swift`, `justfile`,
`tests/`, `tools/`, `examples/`, `assets/`) into a new top-level directory, so a
clean `cli/` could sit beside it. Two questions followed: what to name that
directory, and whether to hyphenate it.

### Could the app support iOS too, so "macos" is presumptuous?

No. The imports across both Swift targets are `AppKit` (3 files), `Foundation`,
`PDFKit`, `WebKit`. No `UIKit`. The app is AppKit-specific by construction, not
by an unexplored option: a chrome-less floating `NSWindow`, global `NSEvent`
monitors for vim keybindings, `NSPrintOperation` for PDF export, launched by
`open -a` and CLI flags. None of this has an iOS equivalent, and the
interaction model itself, a window that tiles beside a terminal with no
navigation chrome, is a desktop idea, not a phone or tablet one. An iOS reader
would be a different app with a different UI paradigm, sharing at most the
`WKWebView` renderer (`template.html`) as a resource, not a build target of
this package.

### Why name it `macos` anyway, rather than the shorter `mac`

Because that asymmetry point cuts the other way for naming. `mac/` reads as the
one and only Apple target this repo will ever have. `macos/` costs nothing
today and correctly anticipates the one case where the code split would matter:
if an iOS reader is ever built, it becomes a new, separate top-level directory
(`ios/`) alongside `macos/` and `cli/`, not a rename of `macos/` to something
that covers both. Naming it for the platform it targets, rather than for "the
only Apple thing here", avoids that rename.

### Why no hyphen, `macos` rather than `mac-os`

A hyphen in this repo separates two distinct words that would run together
unreadably otherwise: `md-comments`, `line-number-gutter-arch.md`. "macOS" is
one proper noun, spelled that way by Apple, and lowercasing it to fit a
directory name gives "macos", the same way "iOS" lowercases to "ios", not
"i-os". `Package.swift` already declared `platforms: [.macOS(.v14)]` before the
move, and other proper-noun-style values in the repo (`bundle_id`, `app_name` in
`justfile`) are not hyphenated either. `macos` matches both patterns; `mac-os`
would match neither.

## Decision

Two top-level directories: `macos/` (the existing AppKit app, unchanged besides
its location) and `cli/` (the new Rust CLI, mission M-COMMENTS). Neither name is
hyphenated.

## Consequences

- `cli/` and `macos/` are equal siblings at the repo root. Neither depends on
  the other's build; `CLAUDE.md` states this and that all `just`/`swift build`
  commands run from inside `macos/`.
- A future iOS reader app, if ever built, is a new top-level directory
  (`ios/`), not a restructuring of `macos/`. This decision is what makes that
  addition free: nothing about today's naming needs to change for it to happen.
- Any future top-level Apple-platform directory in this repo follows the same
  spelling rule: the platform's own name, lowercased, unhyphenated.

## References

[ADR 0002](0002-comments-cli-talks-to-marq-as-a-spawned-process.md), the
decision this layout exists to support. T-00 in the
[M-COMMENTS](../planning/M-COMMENTS-git-backed-comments.md) mission, commit
`c7ef9e4`, is where the move itself happened.
