# 2. The comments CLI talks to marq as a spawned process, not a library or a daemon

Date: 2026-09-26

## Status

Accepted.

## Context

Mission [M-COMMENTS](../planning/M-COMMENTS-git-backed-comments.md) builds a
standalone Rust CLI that stores comments and suggestions on markdown files as git
commits on an orphan `md-comments` branch. Marq, later, reads what it stores. Three
mechanisms were considered for how the two programs communicate:

1. **External process.** Marq spawns the CLI per call and reads its JSON on stdout.
2. **FFI.** The CLI's core compiles as a Rust `staticlib`, `cbindgen` generates a C
   header from `extern "C"` functions, and SwiftPM links the result as a
   `binaryTarget` (a prebuilt `.xcframework`), or `uniffi`/`swift-bridge` generate
   the Swift-side glue.
3. **A resident daemon.** A background process holds parsed annotation state in
   memory and listens on a Unix domain socket; marq and the CLI become thin
   clients over it, the pattern `rust-analyzer`, `gopls`, `ccache` and the Bazel
   daemon use to amortise startup cost.

### What was measured

200 sequential spawns of a small Rust binary averaged 1.6ms each, on the same
order as `git --version` at 1.9ms (measured in this repository's cloud
container; the constant factor differs on macOS but not the order of magnitude).
The CLI's own constraint is that it "uses the `git` binary", so a `git` subprocess
is spawned on every call regardless of which of the three mechanisms is chosen.
The call pattern is marq loading or refreshing a file, the same frequency as
`renderMarkdown()` running `marked`, `mermaid` and KaTeX, already tens to hundreds
of milliseconds per document. A process spawn at this frequency is not a cost
worth designing around.

### Why FFI was rejected

`CLAUDE.md` records, as this repository's most expensive class of bug, testing
against something not just built: a stale `template.html` bundled into an old
build, an app instance still rendering with the template it started with, a debug
binary read by nothing. Every `just` harness recipe depends on `swift build` for
exactly this reason. Linking the CLI's core as a library breaks that guarantee:
`swift build` alone would no longer produce a correct binary, since the
`.xcframework` needs a `cargo build` first, and a Rust change with no matching
`.xcframework` rebuild reintroduces the stale-build trap this repository has
already paid to close once.

### Why a resident daemon was rejected

A local socket removes the spawn cost measured above, which was already noise.
Its usual justification elsewhere is either an expensive warm-up (not the case:
there is no index to build) or a capability a stateless call cannot provide, such
as pushing a live update to an open client when another process changes shared
state. That capability, real-time collaboration between two people's machines, is
a live justification for the future, not this mission: `git push`/`fetch` is
asynchronous by construction, and syncing two different people's edits live needs
a network relay or peer connection between machines. A daemon local to one
machine does not provide that: it would only ever let marq and the CLI on the
same machine skip a process spawn that was already inexpensive. Building it now
would pay its real costs, a second wire protocol on top of the JSON Schema already
planned, its own lifecycle (start, stop, a stale socket file left behind), and the
same class of staleness risk as FFI, a resident process built from old code gives
no visual cue that it is stale, unlike a marq window that is at least visibly
open, for a capability this design does not use.

## Decision

Marq spawns the CLI as an external process per call and reads its JSON output.
No library linking, no resident daemon, for this mission.

Keep the CLI's anchoring and storage logic in a library crate, with the CLI as a
thin binary over it. This costs nothing now and is why a future daemon or a
server component, if a later mission takes on real-time collaboration, would
reuse the same anchoring logic rather than duplicate it.

## Consequences

- `swift build` alone continues to produce a correct marq binary. No cross-language
  build step is added to the pipeline.
- A CLI bug prints an error or empty JSON; marq stays running. No panic can cross
  a language boundary undefined.
- The JSON Schema from mission task T-03 is the one contract between the two
  programs. There is no second, socket-level protocol to keep in step with it.
- `marq-comments list file.md` runs and is inspected standalone, identically for
  marq and for an agent in a cloud session. No second harness is needed to
  exercise library entry points.
- This decision does not build toward real-time collaboration and is not meant
  to. That capability needs a network relay or peer connection between machines,
  which is out of scope for M-COMMENTS and is a separate future mission's
  architecture, not a local IPC choice. The git-branch storage model is itself
  asynchronous, a comment is a commit, which is a fit for comment threads and a
  tension worth deciding explicitly if that later mission attempts live,
  character-level co-editing.

## References

[Intel: what a cloud session can build, and the CLI split](../planning/M-COMMENTS/intel-cloud-build-and-cli-split.md),
which measured cargo's ability to build in this environment and set out the
Rust/Swift FFI mechanism this ADR weighs against.
