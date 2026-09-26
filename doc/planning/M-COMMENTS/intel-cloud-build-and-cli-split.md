# Intel: what a cloud session can build, and the CLI split

| Field | Value |
|---|---|
| Mission | [M-COMMENTS](../M-COMMENTS-git-backed-comments.md) |
| Written in | a Claude Code cloud session, Linux container, 2026-09-26 |
| Status | findings and a recommendation. Jim decides the two open points at the end. |

Two questions from Jim:

1. Can a cloud session build and verify marq at all?
2. Should the work split into a Rust CLI for agents and the macOS app for people?

## 1. What a cloud session can and cannot do with marq

### Measured facts about the container

| Tool | Present | Note |
|---|---|---|
| `swift`, `swiftc` | no | |
| `xcodebuild` | no | |
| `just` | no | every harness recipe in `justfile` is unavailable |
| `git` | 2.43.0 | |
| `cargo`, `rustc` | 1.94.1 | |
| `python3`, `node` 22 | yes | |
| Chromium and Playwright | yes | pre-installed by the environment |

Network, through the session proxy:

- `download.swift.org` answers (302), so a Linux Swift toolchain could be fetched.
- `crates.io` sparse index and crate downloads work. A probe project with `clap`, `serde_json` and `gix` added and built clean in one `cargo build`. The `crates.io` JSON API returns 403, which affects `cargo search` and nothing else.
- Public GitHub repositories clone read-only through the proxy without being attached to the session.

### The marq package cannot build here, whatever toolchain is installed

This is a platform gap, not a toolchain gap. `Package.swift` declares
`platforms: [.macOS(.v14)]`, and the imports across both targets are:

```
3 import AppKit
1 import Foundation
1 import PDFKit
1 import WebKit
```

`AppKit`, `WebKit` and `PDFKit` do not exist on Linux. Installing Swift on Linux
gives Foundation and nothing else in that list. So neither `marq` nor `pdftool`
compiles in a cloud session, and installing a toolchain does not change that.

### So nothing in the harness runs here

Every instrument in `doc/agent-harness.md` runs the app binary: `--dump-metrics`,
`--export-png`, `--export-pdf`, `pdftool`, `just check` against the baselines. None
of them is available. What a cloud session can do for the marq app:

- Read and edit `Sources/marq/` and `template.html`.
- Answer "why is this grey" questions by grep, which `CLAUDE.md` names as the
  right first instrument anyway.
- Reason about a change. It cannot measure one.

`CLAUDE.md` says "Never test against something you did not just build" and "Judge
by measurement, not by eye". A cloud session cannot satisfy either for a rendering
change. Any change to the app from a cloud session lands unverified, and the
session must say so rather than describe it as checked. Jim builds and runs
`just check` on the desktop.

Chromium in the container is not a substitute. `CLAUDE.md` records that a Chrome
reproduction of `template.html` "produced confident, wrong answers for a whole
session" on print layout. Do not bring it back.

### One route to verification from the cloud: a macOS CI runner

The repository has no `.github/workflows`. GitHub Actions macOS runners build Swift
packages. A workflow that runs `swift build` and `just check` on `macos-latest`
would give a cloud session a verification path: push, then read the check result.

Caveats, so this is an experiment rather than a plan:

- `just check` compares metrics to golden baselines that were blessed on Jim's
  Mac. `CLAUDE.md` records that column widths depend on laid-out text and the
  font cache. A runner with different system fonts may fail every baseline for
  reasons unrelated to the change. The first run tells whether that happens.
- macOS Actions minutes cost ten times Linux minutes.
- It does nothing for the export-and-look-at-it class of check, only for the
  numeric one.

This is outside M-COMMENTS, which changes nothing in `Sources/marq/`. It matters
for the later UI mission.

## 2. The split: a Rust CLI and the macOS app

### The mission brief already assumes a separate CLI

Three lines in the brief settle most of this before Jim's question:

- Purpose: "the CLI is the first interface and marq's UI comes later".
- Constraints: "The CLI runs in a Claude Code cloud session (Linux) with no macOS
  toolchain, and on macOS. It uses the `git` binary. It does not need a running
  marq."
- Out of scope: "Any change to the marq app or `template.html`."

So the split into two programs is decided. Jim's proposal adds two things: the
language is Rust, and the CLI is a separate install. The brief leaves the language
to the agent and asks for the reason in the design doc. This section is that
reason, ahead of the design doc.

### Rust is the right language, and the cloud proves it

| Requirement from the brief | Rust | Swift CLI | Python |
|---|---|---|---|
| Builds and tests in a cloud session | yes, proven above with `cargo build` on `clap`, `serde_json`, `gix` | no toolchain here; installable, but Foundation-only on Linux | yes |
| Builds on macOS | yes | yes | yes |
| Single binary, no runtime for the user or an agent to install | yes | yes on macOS | no, needs an interpreter and packages |
| The agent doing the mission can run the test suite where it works | yes | no | yes |
| Fits a later `brew` formula (out of scope now) | yes, standard | yes | awkward |

The deciding row is the fourth. The mission's tests must run "with one command in
a cloud session". A Swift CLI would be written by an agent that cannot run its
own tests. Python passes that row but fails the single-binary row, which matters
for the agent use case: an agent in a fresh cloud environment should get the tool
with one install step, not a virtualenv.

`gix` (pure Rust git) built fine, but the brief's constraint is "It uses the `git`
binary". Shelling out to `git` through `std::process::Command` meets it and keeps
the dependency list short. The plumbing commands `hash-object`, `update-index`
with `GIT_INDEX_FILE` pointed at a temporary index, `write-tree`, `commit-tree`
and `update-ref` write a commit to `refs/heads/md-comments` without a checkout and
without touching the working tree, which is the constraint "They never appear in
the markdown's working tree". A temporary worktree is the fallback if plumbing
turns out awkward. `gix` stays out unless a measured problem calls for it.

### What "separate install" means for the repository layout

Two layouts meet "separate install":

| | CLI in this repo, under `cli/` | CLI in its own repo |
|---|---|---|
| Matches the brief's execution constraint "new source and test directories for the CLI" | yes | no, the constraint would change |
| Design doc, JSON Schema, CLI and marq stay in one history | yes | schema and design doc need a home; the CLI repo is the natural one, which moves them away from marq |
| `cargo install --git` for an agent | works with `--root`/subdirectory flags, one longer command | one short command |
| Homebrew formula later | possible, formula points at a subdirectory | standard |
| Swift package stays clean | yes, Cargo and SwiftPM ignore each other | yes |
| Push flow already set up for a cloud session | yes, `main` here | a second repo to attach and push to |

**Decided (Jim): `cli/` in this repo, permanently.** No planned move to a
separate repository. Packaging, if it ever needs one, points a formula at the
subdirectory rather than moving the code.

### The Rust/Swift FFI option, and why it stays out of this mission

Swift can call Rust as a library, not only as a separate process: the Rust crate
compiles as a `staticlib`, `cbindgen` generates a C header from `extern "C"`
functions at the boundary (complex data crosses as JSON or `repr(C)` structs),
and SwiftPM consumes the result as a `binaryTarget` pointing at a prebuilt
`.xcframework`, or `uniffi`/`swift-bridge` generate the glue instead of writing
it by hand. This is a real option for the later UI mission, not a dead end — see
the option table below.

The cost is specific to marq's own conventions. `CLAUDE.md`: "every `just`
harness recipe depends on `swift build`", precisely so a stale build cannot be
tested against by accident. Linking a Rust library breaks that invariant:
`swift build` alone no longer produces a correct binary, because the
`.xcframework` has to be built by `cargo` first, and a change to the Rust source
with no matching `.xcframework` rebuild is a new version of the exact stale-build
trap the harness exists to close. That cost is real whether or not it is worth
paying, so it is left to the UI mission, not decided here, since this mission
touches no Swift.

### How marq reads the annotations, later

The hard part of this mission is anchoring: resolving a quote selector against a
file that has changed, and reporting an orphan rather than attaching to the wrong
text. Whatever the marq UI does, that logic must exist once.

| Option | Consequence |
|---|---|
| marq shells out to the CLI and reads its JSON | one implementation of anchoring; marq needs the CLI installed, and shows an install message when it is missing |
| marq reads the `md-comments` branch itself in Swift | anchoring written twice, in two languages, which drift |
| Rust library with a C ABI, linked into marq (feasible — see above) | one implementation; breaks the "every harness recipe depends on `swift build`" invariant, because `swift build` alone can no longer produce a correct binary |
| A resident daemon over a Unix socket | removes a process-spawn cost already measured as noise; adds a second wire protocol, its own lifecycle, and the same staleness risk as FFI, for a local-machine-only capability. Does not itself provide real-time collaboration between two people's machines, since that needs a network relay, not local IPC. See [ADR 0002](../../adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md) |

Recommendation for the later UI mission: marq shells out to the CLI. Decided and
recorded in [ADR 0002](../../adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md).
The CLI's `list` command with JSON output and resolved anchors, which the brief
already requires, is the interface. The JSON Schema in the repo is the contract
between the two programs. `data-source-line` on rendered blocks, from
`doc/line-number-gutter-arch.md`, is where a resolved line anchor meets the
rendered page.

### What an agent in a cloud environment needs

- `git` present. It is, in this container.
- The CLI installed. `cargo install --git <repo>` works where the proxy serves
  the repository, as it does for public GitHub. The right place for that
  install is the cloud environment's setup script, so every session starts with
  the tool. A prebuilt release binary is a later, faster option; release is out
  of scope.
- A section in `CLAUDE.md` that names the commands. That is T-08 in the brief.

## Consequences for the brief

Feedback, not questions.

- "Files: `doc/`, `Package.swift`, new source and test directories for the CLI".
  With Rust, `Package.swift` does not change. The files list becomes `doc/`,
  `cli/` and its tests.
- T-09 "Update `doc/planning/plan.md`" is partly done: `plan.md` references this
  mission as of commit `772c5af`. The delta gets its task rows when T-02 fixes
  the plan.
- The brief says the agent decides the language and records the reason. The
  reason is above and moves into `doc/comments-design.md` at T-02.

## Open points for Jim

1. ~~CLI under `cli/` in this repo now, moving out at packaging time, or a
   separate repository from the start.~~ Decided: `cli/` in this repo,
   permanently.
2. Whether a macOS CI runner for `swift build` and `just check` is worth one
   experiment before the UI mission.
3. ~~Whether the later UI mission links the CLI as a Rust library via FFI, or
   shells out to it as a process, or a resident daemon.~~ Decided: shell out.
   See [ADR 0002](../../adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md),
   which also records that this is a local-machine choice and does not itself
   provide the real-time collaboration Jim wants for a later mission, that
   needs a network relay between machines, a separate architecture question.
