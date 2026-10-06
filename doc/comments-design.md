# Comments design

| Field | Value |
|---|---|
| Mission | [M-COMMENTS](planning/M-COMMENTS-git-backed-comments.md), task T-02 |
| Date | 2026-09-29 |
| Status | Accepted for implementation |
| Inputs | [T-01 intel](planning/M-COMMENTS/intel-anchoring-and-prior-art.md), [cloud build intel](planning/M-COMMENTS/intel-cloud-build-and-cli-split.md), [ADR 0002](adr/0002-comments-cli-talks-to-marq-as-a-spawned-process.md), [ADR 0003](adr/0003-repo-layout-macos-and-cli-directories.md) |

This document specifies the storage format, the git layout, the anchoring and
re-anchoring method, and the CLI commands for git-backed comments and
suggestions on markdown files. Every decision carries one reason. Tasks T-03 to
T-10 build to it.

## 1. The shape of the system

`cli/` holds one Rust package, `marq-comments`. It stores comments, replies,
suggestions and state changes as W3C Web Annotations, one JSON file each, on an
orphan branch `md-comments` in the same repository as the markdown. It reads the
markdown from the working tree. It writes to the working tree only when it
accepts a suggestion.

| Decision | Reason |
|---|---|
| Rust | It builds and tests in a cloud session and on macOS, and ships as one binary with no runtime (measured in the cloud build intel). |
| A library crate (`marq_comments`) with a thin binary (`marq-comments`) | ADR 0002: a later daemon or server reuses the anchoring code without a rewrite. |
| The `git` binary through `std::process::Command`, no `gix` or `libgit2` | The brief requires the `git` binary, and it is the git the user already has configured. |
| Git 2.38 or later | `git merge-tree --write-tree`, the command that merges `md-comments` without a checkout, arrived in 2.38. Measured on 2.43; macOS ships 2.39 or later. |
| Rust 1.89 or later | The file lock in section 2.2 uses `File::lock`, stable from 1.89. |
| Dependencies: `clap`, `serde`, `serde_json`, `uuid`, `similar`, `time`; `tempfile` and `jsonschema` for tests only | Each covers one listed need: arguments, JSON, ids, line diffs, timestamps, test repos, schema checks. |

## 2. Storage: the `md-comments` branch

### 2.1 Layout

```
README.md                               what this branch is, for a person browsing it
format.json                             {"layout": 1}
documents/
  <repo-relative markdown path>/        for example documents/doc/plan.md/
    annotations/<uuid>.json             one comment, reply or suggestion
    states/<uuid>.json                  one state change
    versions/<blob id>                  the markdown text an annotation was written against
```

| Decision | Reason |
|---|---|
| The ref name is one constant, `COMMENTS_REF = "refs/heads/md-comments"`, in one module | The brief requires a later move to a custom ref to be a one-line change. |
| Every path is either a random id or a git blob id | Measured on git 2.43: two clones that add the same path with identical content merge clean, and different content at one path conflicts. Random ids and content hashes rule out the second case, so a merge of two clones never conflicts. |
| No file is ever modified or deleted after it is written | A modification is the only other way to produce a merge conflict, and the brief forbids destroying history. A state change is a new file (section 5). |
| Annotations grouped under `documents/<markdown path>/` | `list FILE` reads one directory with one `git ls-tree`, not the whole branch. |
| A reply lives in the directory of the annotation it replies to | A thread then sits in one directory, so listing a file returns its replies with no second lookup. |
| `versions/<blob id>` is a tree entry that points at the markdown blob itself | It makes the recorded blob reachable from `md-comments`, so it is pushed and fetched with the comments, and the text an annotation was written against stays available in every clone. It costs no storage when the blob is already in the repository. |
| `format.json` with a layout number | A later layout change can be detected and migrated, not guessed. |
| `README.md` at the branch root | GitHub lists the branch, and a person who opens it needs one paragraph that explains it. |
| Markdown paths are repo-relative, with `/` separators, from `git rev-parse --show-prefix` plus the argument | The same file then has the same key in every clone and from every working directory. |

### 2.2 Writing

Every write is one commit on `md-comments`, made without a checkout:

1. `git hash-object -w --stdin` for each new file, to get its blob.
2. Read the current tip with `git rev-parse -q --verify refs/heads/md-comments`. No tip means the first commit, with no parent.
3. With `GIT_INDEX_FILE` set to a temporary file: `git read-tree <tip>` (or `--empty`), then `git update-index --add --cacheinfo 100644,<blob>,<path>` for each file.
4. `git write-tree`, then `git commit-tree <tree> [-p <tip>] -m <message>`.
5. `git update-ref refs/heads/md-comments <new> <tip>` (the all-zero id when there was no tip).

| Decision | Reason |
|---|---|
| A temporary index, never the repository's own index | The user's staged changes and working tree stay untouched. Measured: both clones' `git status` stayed clean through writes and a merge. |
| `update-ref` with the expected old value, retried from step 2 up to 10 times on failure | A writer that does not take the lock below cannot overwrite another's commit: git refuses a stale old value (measured). A stale `refs/heads/md-comments.lock` left by a killed `git update-ref` is reported by file name and not deleted, because git cannot tell it from a live one. |
| An advisory file lock on `<git common dir>/marq-comments.lock`, held from reading the state to writing it | The branch is shared by every worktree of a repository, so the lock lives in the common git directory (T-07: under the per-worktree git directory, two worktrees had separate locks). A command that checks a state and then writes holds the lock across both, so one `resolve` of eight racing ones succeeds. `accept` holds it around the file edit as well (T-07: eight parallel accepts recorded eight decisions and kept three edits). Measured: the lock was proved held and released at process death by a test that kills the holder. Originally introduced by T-04: Measured by T-04: with the retries alone, 8 threads making 5 writes each starved one writer past 10 retries and lost its writes. The lock makes writers take turns, and the retry stays as the guard for a writer that skips it. `File::lock` needs Rust 1.89 or later. |
| The markdown blob is made with `git hash-object -w --no-filters` | The blob then holds the raw bytes the selector positions were measured on, whatever `core.autocrlf` says. |
| The temporary index lives in the git directory, named with the process id and a random suffix, and is removed on drop. A writer holding the lock also deletes leftover `marq-comments-index.*` files | A SIGKILL leaves one behind (one run left 32). Deleting under the lock is safe because every taker makes its index under the lock. Two processes never share one, and a crash leaves one small file in `.git`, not in the working tree. |
| Commit messages `comment <short id> on <path>`, `reply ...`, `suggest ...`, and `<state> <short id> on <path>` for a state change | `git log md-comments` reads as a history of review activity. |
| An `accepted` state change also adds `versions/<resultBlob>` | The text the accepted edit produced stays reachable from the branch, as the text a comment was written against does. |
| The commit author and committer are the annotation's creator | `git log` and the annotation agree on who wrote it. |

### 2.3 Reading

`git ls-tree -r -z --name-only refs/heads/md-comments -- documents/<path>/` (`-z` so that unusual file names are not quoted), then
`git cat-file --batch` for the files. One process for the listing and one for
the contents, whatever the number of annotations.

### 2.4 Sync

`marq-comments sync [--remote origin]`:

1. `git fetch <remote> +refs/heads/md-comments:refs/remotes/<remote>/md-comments`. A remote with no such branch is not an error: the local branch is pushed as it is. If neither side has the branch, there is nothing to do.
2. If the local branch is missing, create it at the remote tip. If one tip contains the other, fast-forward the local ref.
3. Otherwise run `git merge-base`. When the tips share no ancestor, pass `--allow-unrelated-histories` to `merge-tree` (retry without the flag when git exits with a usage error, which is git 2.38 to 2.40). Then `git merge-tree --write-tree <local> <remote>`, `git commit-tree` with both parents, and `update-ref` with the expected old value.
4. `git push <remote> refs/heads/md-comments:refs/heads/md-comments`. When git reports the push as rejected, repeat from step 1, up to 10 times with a growing random pause. Any other push failure is returned at once.

Syncs in one clone take turns through a second lock, `<git common dir>/marq-comments-sync.lock`; writes never wait on it. Git runs with `LC_ALL=C` and literal pathspecs, with `GIT_GLOB_PATHSPECS` and `GIT_ICASE_PATHSPECS` removed.

| Decision | Reason |
|---|---|
| Full ref names in every fetch and push | An unqualified name can resolve to a different ref with no error; the tsk repository lost work to exactly this (its ADR 0008). |
| 10 push attempts with a random pause, not 3 | Measured by T-07: six processes in three clones produced a rejected push three times running. |
| A separate sync lock | Measured by T-07: 152 of 180 racing syncs in one clone failed because their fetches collided on the tracking ref. |
| The merge commit's author is `--author` when given, else git's identity, else `marq-comments` with no email | A machine with no git identity could write comments but not merge them. |
| `LC_ALL=C` for every git call | `sync` matches git's messages ("rejected", "couldn't find remote ref"), which git translates. Proved with a fake translating `git`, not a real one. |
| `--allow-unrelated-histories` when the tips share no ancestor | Measured by T-04: two clones that each write a first comment before syncing have unrelated root commits, which `merge-tree` refuses without the flag. This is the common first sync, not a corner case. The identical `README.md` and `format.json` blobs merge clean. |
| A `sync` command, not plain `git pull` | A plain merge of `md-comments` needs it checked out, and the branch never appears in the working tree. |
| A conflict from `merge-tree` is an error, with no automatic resolution | Section 2.1 makes it impossible for files the CLI wrote; a conflict means someone edited the branch by hand, and a person decides. |

## 3. Annotation format

### 3.1 Common properties

```json
{
  "@context": [
    "http://www.w3.org/ns/anno.jsonld",
    {"marq": "https://github.com/jimbarritt/marq/blob/main/doc/comments-design.md#"}
  ],
  "id": "urn:uuid:4f0c2d1e-8a57-4c63-9f1b-2e6d7a90b3c4",
  "type": "Annotation",
  "created": "2026-09-29T10:15:02Z",
  "creator": {"type": "Person", "name": "Jim", "email": "mailto:jim@example.com"},
  "generator": {"type": "Software", "name": "marq-comments 0.1.0"}
}
```

| Decision | Reason |
|---|---|
| Extensions under an inline context that maps the `marq:` prefix | The brief requires a separate JSON-LD context, and an inline one needs no hosted document; a W3C consumer reads the standard terms unchanged. |
| The prefix IRI is this document | The terms then resolve to their own definitions. |
| `id` is `urn:uuid:<random v4 uuid>` | W3C requires an IRI, a `urn:uuid` needs no host, and random ids make a short prefix unique enough to type (section 6.3). |
| The file name is the uuid, without the `urn:uuid:` part | The path stays short and still unique. |
| `created` is UTC with a `Z` suffix, to the second | The W3C model requires `xsd:dateTime` in UTC. |
| `creator.email` is stored as `mailto:<address>` and omitted when there is no email; `creator.name` is always present | An empty `mailto:` is not a valid IRI. |
| `creator` from `--author "Name <email>"`, else `git config user.name` and `user.email`; an error if neither | The brief names these sources, and an annotation without an author breaks the review record. |
| `creator.type` is `Person`, or `Software` with `--agent` | The W3C model has both types, and the marq UI later shows which comments an agent wrote. |
| `created` is whole seconds, with no fraction and no offset | The `xsd:dateTime` form allows more; one form means one parser and one sort order. |
| JSON written with sorted keys, two-space indentation and a final newline | The brief requires minimal diffs; `serde_json::Value` sorts keys by default. |

### 3.2 A comment on a word

```json
"motivation": "commenting",
"body": {"type": "TextualBody", "value": "Why not Vite?", "format": "text/markdown"},
"target": {
  "source": "doc/plan.md",
  "marq:sourceBlob": "3b18e512dba79e4c8300dd08aeb37f8e728b8dad",
  "selector": [
    {"type": "TextQuoteSelector", "exact": "esbuild", "prefix": "The build uses ", "suffix": " for bundling.\n\nIt s"},
    {"type": "TextPositionSelector", "start": 412, "end": 419}
  ]
}
```

| Decision | Reason |
|---|---|
| `source` is the repo-relative path, a relative IRI reference | The repository has no single URL (clones, forks, a laptop), and a relative reference resolves against wherever the repository is. |
| `marq:sourceBlob` is the git blob id of the working-tree file at the time of writing; the file is read once, the selectors are computed from those bytes, and that same buffer is stored as the blob | It records the exact text the selectors describe, with or without a commit, and section 4.2 uses it to map positions forward. A second read would let an edit between the two reads put the blob and the selectors out of step. |
| Both a `TextQuoteSelector` and a `TextPositionSelector`, exactly one of each in the array | The quote survives edits and the position gives the search a starting point (T-01, Hypothesis's order). |
| `prefix` and `suffix` are always present, and are empty strings at the start or end of the file | A reader never has to ask whether a key may be missing. |
| The positions are code points, as in section 4.1 | Stated here too, because the storage layer stores them as given. |
| `body.format` is `text/markdown` | Comment text is markdown and marq renders it as markdown. |

### 3.3 A comment on a whole line

```json
"selector": [
  {"type": "FragmentSelector", "conformsTo": "http://tools.ietf.org/rfc/rfc5147", "value": "line=11,12"},
  {"type": "TextQuoteSelector", "exact": "It supports three agent presets.", "prefix": "plugin uses esbuild.\n\n", "suffix": "\n\nThe next section"},
  {"type": "TextPositionSelector", "start": 530, "end": 562}
]
```

| Decision | Reason |
|---|---|
| A `FragmentSelector` with RFC 5147 `line=N-1,N` for the 1-based line N, so the example above anchors line 12 | RFC 5147 counts lines from 0 and the end is exclusive. RFC 5147 is the plain-text fragment scheme the W3C model lists, so the line anchor needs no extension. |
| The fragment selector is present on a line comment and absent on a word comment | Its presence is what marks the anchor as a whole line when it is read back. The quote and position selectors give the recorded line. |
| The line's text as the quote, without its newline | A whole-line anchor re-anchors by the same method as a word anchor. |
| A blank line cannot carry a comment | Its quote is empty and matches every blank line. |

### 3.4 A reply

```json
"motivation": "replying",
"body": {"type": "TextualBody", "value": "Obsidian plugins ship one CJS file.", "format": "text/markdown"},
"target": "urn:uuid:4f0c2d1e-8a57-4c63-9f1b-2e6d7a90b3c4"
```

| Decision | Reason |
|---|---|
| The target is the replied-to annotation's id | The W3C `replying` motivation targets a previous annotation. |
| A reply may target a reply | Threads nest, and `list` shows the tree. |
| A comment or a reply has one `TextualBody` object as `body`, never an array | One form per motivation keeps readers simple; only a suggestion needs two bodies. |

### 3.5 A suggestion

```json
"motivation": "editing",
"body": [
  {"type": "TextualBody", "value": "five", "purpose": "editing"},
  {"type": "TextualBody", "value": "The count in settings is five.", "purpose": "commenting", "format": "text/markdown"}
],
"target": { "source": "doc/plan.md", "marq:sourceBlob": "...", "selector": [ ... "exact": "three" ... ] }
```

| Decision | Reason |
|---|---|
| Motivation `editing`, with the replacement text as a body with `purpose: editing` | The W3C model defines `editing` as a request to change the target. |
| `body` of a suggestion is an array of one or two bodies: exactly one with `purpose: editing`, and at most one with `purpose: commenting` | A suggestion may be a bare replacement, or carry its reason. |
| An optional second body with `purpose: commenting` | A suggestion usually carries its reason, and the purpose tells a consumer which body is which. |
| A suggestion always replaces a non-empty range; an insertion replaces a word with the word plus the new text, a deletion replaces with `""` | The anchor needs text to quote; an empty quote matches everywhere. |

### 3.6 A state change

Stored under `states/`, not `annotations/`:

```json
{
  "@context": [ ...same as 3.1... ],
  "id": "urn:uuid:9e3b...",
  "type": "marq:StateChange",
  "marq:annotation": "urn:uuid:4f0c2d1e-8a57-4c63-9f1b-2e6d7a90b3c4",
  "marq:state": "resolved",
  "created": "2026-09-29T11:02:40Z",
  "creator": { ... }
}
```

An `accepted` state change also carries `marq:resultBlob`, the blob id of the
markdown file after the edit, and `marq:resultStart`, the code-point offset at
which the replacement text starts in that file.

| Decision | Reason |
|---|---|
| A state change is its own file in the extension vocabulary, not an edit to the annotation | Section 2.1: no file is modified, so two clones that change one annotation's state still merge clean, and every earlier state stays in the tree as well as in history. |
| A state change carries the same `@context`, `id`, `created` and `creator` as an annotation; `generator` is optional on both | A state change is a record of who did what and when, so it needs the same provenance. |
| `marq:resultBlob` is required on `accepted` and allowed on no other state | An accepted suggestion must record the text its edit produced. |
| `marq:resultStart` is an optional non-negative integer on `accepted` | With the result blob it lets a reader anchor the accepted suggestion to the text it put in the file. Records written without it still read. |
| The annotation file carries no state property | A W3C consumer then reads a valid annotation, and the state lives only where the fold in section 5 reads it. |
| `marq:resultBlob` on `accepted` | It records exactly which text the accepted edit produced. |

## 4. Anchoring

### 4.1 Creating selectors

| Decision | Reason |
|---|---|
| Text is the file's bytes decoded as UTF-8, with no Unicode normalisation and no line-ending change | Positions must describe the file as stored; any normalisation makes them describe a different string. |
| Positions count Unicode code points | The W3C model counts characters, and code points are the unit that does not depend on an encoding. The marq UI converts to UTF-16 when it needs to. |
| `prefix` and `suffix` are up to 32 code points | Long enough to tell repeated words apart in prose, short enough that an edit a sentence away leaves them intact. |
| A file that is not valid UTF-8 is an error | Selectors over undecodable bytes cannot be quoted. |
| An empty range, a range past the end of the text, and a blank or whitespace-only line are errors when creating selectors | An empty quote matches everywhere, and a blank line matches every blank line. |
| Lines split on `\n` only; a trailing `\r` stays in the line; a trailing newline adds no line | One rule for every clone; CRLF is a known limit (section 8). |

### 4.2 Resolving an anchor

`resolve(annotation, current text)` runs these steps in order and stops at the
first that succeeds. Its result is one of three statuses:

- `anchored`: the quoted text is in the file, with a range.
- `changed`: the quoted text is gone, but the line diff locates the same place, and other text now stands there. The result carries the current range and the original quote.
- `orphaned`: no location.

The steps:

1. **Unchanged file.** The current blob id equals `marq:sourceBlob`, and the text at the position equals the quote: anchored.
2. **Mapped position.** Read the recorded version from `versions/<blob id>`, run a line diff (`similar`) from it to the current text, and map the recorded start line to the current file. If that line is unchanged and the text at the mapped position equals the quote: anchored. The mapped position is also the hint for step 3.
3. **Quote search.** Find every exact occurrence of the quote. For each, score its context: the number of code points by which the text before it ends with the stored `prefix`, plus the number by which the text after it starts with the stored `suffix`. Discard candidates below the floor (4.3). Pick the highest score, then the nearest to the hint, then the lowest offset: anchored.
4. **Changed in place.** If the diff in step 2 shows the recorded line replaced (a hunk with old and new lines, not a pure deletion), take the new line at the same index within the hunk. This step needs the recorded text, so it never runs when `versions/<blob>` is missing. For a line anchor, that whole line is the range: changed; a line edited to blank orphans. For a word anchor, which must lie inside its recorded start line, compare the old line and the new line and take the text that differs: the new text after their common prefix and before their common suffix, where the suffix is not allowed to overlap the prefix. Then:
   - If the new differing text holds a non-whitespace character, widen it to the enclosing run of non-whitespace characters: changed.
   - If it is whitespace only, or the removed old text holds whitespace (the edit crossed a word boundary, so a whole word went): orphaned.
   - Otherwise letters went from inside one run. Widen the empty position to the run around it: changed, unless that run holds no letter or digit, which is orphaned (so `double hyphen.` becoming `double .` does not anchor on the full stop).
5. Otherwise: **orphaned**. This covers a deleted line, a hunk that shrank past the recorded line, and a word removed with nothing in its place.

For a line anchor, step 3 compares whole lines equal to the quote.

| Decision | Reason |
|---|---|
| The quote must match exactly in steps 1 to 3; no fuzzy match on the anchored text | Only exact text proves an anchor is the same words; fuzzy matching re-attached anchors to wrong text in Hypothesis (T-01). |
| A separate `changed` status, reached only through the line diff | Jim's decision (2026-09-29): a typo fix or a reworded line must not lose the comment. The diff locates the place by position in the file's history, not by guessing at similar text, and the status tells the reader the text differs from the quote. |
| `changed` carries the original quote | The reader compares what the comment was about with what stands there now. |
| The range is widened to the enclosing run of non-whitespace | A typo fix often changes the middle of a word, or deletes one letter, and the differing text alone would be a fragment or empty. Widening gives the whole corrected word. |
| The three-part rule for step 4, not widening alone | Found by T-05: with a common prefix taken greedily, a typo that deletes a letter and a whole word deleted both leave an empty range in the new line, and widening alone turns the second into a changed anchor on the next word. The rule tells them apart by whether the removed text crossed a word boundary. |
| A word removed with nothing in its place orphans | The brief requires removed anchored text to report as orphaned. |
| The diff is `similar`'s Myers line diff | The design named no algorithm. A moved paragraph shows as a deletion plus an insertion, and step 3 finds it again. |
| Context scoring accepts partial matches | Nearby edits often trim a few characters of context, and that must not orphan an anchor whose own text is intact. |
| The position mapped through a line diff, before any search | It is the strongest evidence that a candidate is the same text, and it separates a moved duplicate from the original. |
| Status is computed on every read and never stored | The markdown changes without the CLI's involvement, so a stored status goes stale; Hypothesis computes it the same way (T-01). |

### 4.2.1 An accepted suggestion

The text a suggestion quotes is the text the accept replaced, so resolving that
quote against the edited file reports `changed` for ever. An accepted suggestion is
therefore anchored to its replacement instead. The reader reads the edited file
from `versions/<marq:resultBlob>`, makes selectors for the range
`[marq:resultStart, marq:resultStart + length of the replacement)`, and resolves
them against the current text, with that edited file as the recorded text. The
result is `anchored` at the replacement, and a later edit gives `changed` or
`orphaned` by the rules of 4.2.

A deletion has no replacement text, and a record without `marq:resultStart` has
no position. Both report `{"status": "applied"}`: the edit is in the file and
there is no location to show.

| Decision | Reason |
|---|---|
| Anchor an accepted suggestion to its replacement | Jim's decision (2026-10-01): an accepted suggestion read as `changed`, which looked like a fault. The replacement is what is now in the file, and the same anchoring rules then apply. |
| A fourth status, `applied`, for a deletion | There is nothing left to quote, and `orphaned` would suggest the suggestion was lost. |
| The result is computed on every read, as for other anchors | The file changes without the CLI's involvement. |

### 4.3 The floor

A candidate from step 3 is accepted only when its context score is at least half
of the stored context length: `2 * score >= len(prefix) + len(suffix)`. The mapped
position from step 2 is exempt. For a replaced line, that is the same column of the
new line at the same index in the hunk; for a deleted line there is no exempt
candidate, and the hint is the start of the hunk's new side plus the column. With no stored context (a quote that is the whole
file) every exact match qualifies.

Reason: text that moved with its paragraph keeps most of its context on both
sides, and the same word elsewhere in unrelated prose shares only a few
characters by chance. The floor separates those two cases, and it is the
explicit threshold T-01 found missing from Hypothesis.

### 4.4 Expected outcomes

These are the anchoring tests for T-05.

| Change to the markdown | Result |
|---|---|
| None | Anchored at the same position (step 1) |
| Text added or removed in another paragraph | Anchored at the shifted position (step 2) |
| The anchored paragraph moved elsewhere in the file | Anchored at the new position (step 3, context intact) |
| The anchored word's line rewritten, word kept, context mostly kept | Anchored (step 3) |
| The anchored word changed, for example a typo fixed | Changed: range on the new word, original quote shown (step 4) |
| A commented line edited | Changed: range on the edited line (step 4) |
| The anchored word deleted, rest of the line kept | Orphaned (step 4 range empty) |
| The anchored line deleted | Orphaned |
| The anchored word deleted, the same word elsewhere in different prose | Orphaned (the floor rejects the other occurrence) |
| Two identical sentences, the second commented, text inserted above both | Anchored on the second (step 2 mapping) |

## 5. State

| Motivation | States | Transitions |
|---|---|---|
| `commenting` | `open`, `resolved` | `resolve`: open to resolved. `reopen`: resolved to open. |
| `editing` | `open`, `accepted`, `rejected` | `accept` or `reject` from open. `reopen`: rejected to open. Accepted is final. |
| `replying` | none | A reply follows its root's state. |

The current state is the last state change in (`created`, `id`) order. With none,
the state is `open`.

| Decision | Reason |
|---|---|
| State is a fold over state-change files | Resolving and accepting never delete, as the brief requires, and parallel state changes merge. |
| Order by `created`, then `id` | Every clone computes the same state from the same files. |
| A burst of N decisions on one annotation, or N replies in one thread, stamps `created` up to N seconds ahead of the clock | Found by T-07. The fold stays identical everywhere (proven), but after a sync another clone's later decision inside that window sorts before them, so "latest wins" does not always mean latest in wall time. Sub-second `created` or a logical counter would fix it and would change the format. |
| A new state change takes `created` one second past the latest existing change for that annotation when the clock is not already later; a reply likewise against its target and that target's replies | Found by T-06: `created` has whole-second resolution and ties break on a random id, so `reject` then `reopen` inside one second folded to `rejected` about half the time. A burst of commands can run a few seconds ahead of the clock. |
| `accepted` is final | An accepted edit already changed the markdown, and reopening cannot undo that. |
| `rejected` can reopen | Jim's decision (2026-09-29): rejecting changes no text, so reopening loses nothing. |
| A suggestion with both an `accepted` and a `rejected` change after a sync is reported with a warning, and the fold result stands | This happens only when two clones decide in parallel; the working-tree text shows what happened, and a person settles it. |
| `accept` re-anchors first and refuses a suggestion that is not `anchored` | Applying an edit at a stale position, or over text that changed since the suggestion, corrupts the markdown. |
| If the edited file is written but recording the `accepted` change fails, the error says the edit is in the file and the suggestion is still open | The two steps cannot be one atomic action, so the message states the half-done state. |
| `accept` writes the working-tree file and never commits it | The working branch belongs to the user; the CLI commits only to `md-comments`. |

## 6. CLI

### 6.1 Commands

| Command | Effect |
|---|---|
| `comment FILE --line N [--text WORD [--nth K]] -m TEXT` | A comment on line N, or on the Kth occurrence (default 1) of WORD within line N. |
| `comment FILE --range START:END -m TEXT` | A comment on a code-point range, for tools that already hold positions. |
| `reply ID -m TEXT` | A reply to an annotation or a reply. |
| `suggest FILE --line N --text OLD [--nth K] --replace NEW [-m TEXT]` | A suggestion to replace OLD on line N. `--range` works as for `comment`. |
| `list FILE [--state open\|resolved\|accepted\|rejected\|all] [--json]` | Annotations on FILE with threads and resolved anchors. Default `all`. |
| `show ID [--json]` | One thread, with the full state-change history. |
| `resolve ID`, `reopen ID` | State changes for a comment; `reopen` also reopens a rejected suggestion. |
| `accept ID`, `reject ID` | State changes for a suggestion; `accept` also edits the file. |
| `sync [--remote NAME]` | Fetch, merge and push `md-comments` (section 2.4). |
| `render FILE [-o OUT.html] [--source]` | A static HTML page: the markdown rendered as a document, anchored and changed ranges in `<mark>` on the rendered text, each thread's card in a column at the right, level with its highlight (a small script places them; without it, or on a narrow screen, each card sits under its block), orphans and applied deletions listed after it. `--source` shows the markdown source in a `<pre>` with a line gutter, and the cards in one list below it. |

Global flags: `--author "Name <email>"`, `--agent`, `-C DIR` (run as if in DIR,
as `git -C` does).

| Decision | Reason |
|---|---|
| Anchors given as a line plus a word | An agent and a person both think in "the word X on line N", and the line removes most ambiguity before the CLI computes offsets. |
| `--range` as well | The marq UI later holds exact positions and must not have to reconstruct a word and a line. |
| `render` shows the document rendered, with the source view behind `--source` | Jim's decision (2026-10-01): a person checks comments against the formatted document, not against markup. The source view stays because it shows exact character ranges. |
| Every mark has one quiet highlight colour, whatever the thread's state or anchor status; state shows only as a small grey label on the card, and a resolved card is dimmed | Jim's decision (2026-10-02): different colours per comment are noisy. Colour stays only inside a suggestion card, where the struck-through and replacement text are the content. |
| A thread's number is a small grey subscript after the end of its marked text, linking to its card | Jim's decision (2026-10-02): a superscript before the text is distracting. After the text, in grey, it marks the end of the comment without competing with the words. |
| On a wide screen the cards sit in a column at the right, each level with its first highlight, and the text is one continuous column; a card that would overlap the one above is pushed down below it. Without script, and on a narrow screen, each card sits directly under the block that holds its first mark | Jim's decisions (2026-10-02): comments at the right, and no gaps in the text. Putting cards inside the text rows stretched the rows and left gaps; a static page cannot align a card to a highlight, so a small script places them. The stacked fallback has no gaps either. |
| The one script is a fixed constant, `<script id="marq-layout">`, in the rendered view only; it contains no document or comment text, no network call, no `eval` and no `innerHTML`, and a test compares its text across two different documents | The page is opened from untrusted markdown, so the only code on it must not depend on the input. The source view has no script. |
| A real-browser test (`cli/tests/render_browser.rs`) checks that cards are in the rail, in order, not overlapping, and level with their marks at 1250px, and back under their blocks at 700px; it skips with a message when no Chromium is found | A mark that is not where the card is cannot be seen by string tests. |
| An image is a small grey camera icon with no text and no box; the alt text is its `aria-label` and tooltip, and a comment on any part of `![alt](src)` highlights the icon | Jim's decision (2026-10-02): the labelled placeholder was noisy. The icon is inline SVG, so the page still loads nothing. |
| Comment text is shown rendered as markdown, by the same safe renderer as the document, and heading ids in it are dropped | The body is stored as `text/markdown`. Dropped ids cannot clash with the document's own. |
| Anchors are mapped onto the rendered text through `pulldown-cmark`'s byte ranges for each event, and each thread's code-point range is converted to bytes once | One renderer reports which source characters each piece of output came from, so no second mapping is invented. |
| Where rendered text differs from its source (entities such as `&amp;`, inline code whose line breaks became spaces, text inside a container whose prefix the parser strips), the whole piece is marked | The exact characters cannot be recovered; marking the piece is the honest approximation. Backslash escapes are exact: the parser emits the escaped character as its own event. |
| A thread whose range covers only syntax that renders as nothing (`#`, `---`, a link destination, a table separator row, list markers, code fences) gets a card flagged "no rendered text" and its number at the next rendered position | The comment must still be seen, and a mark on nothing cannot be drawn. |
| The document's own HTML is shown as escaped text, except a bare `<br>`, `<br/>` or `<br />`, which is written as a fixed line break | Untrusted input must never become markup. Tables use `<br>` for paragraph breaks in a cell, and a fixed string cannot carry an attribute. |
| Only `http:`, `https:` and `mailto:` links and `#` fragments get an `href`; images show their alt text in a box and load nothing | The page is a self-contained static file and must stay safe to open. Heading ids are prefixed `md-` so they cannot meet a card id. |
| A `render` command | A cloud session cannot run marq, and a static page shows the resolved anchors on the real text for a person or a test to check. |
| `suggest --line N` requires `--text`; `--replace ""` means a deletion; an empty or blank `-m` on `comment` or `reply` is an error | A suggestion needs text to quote, and a deletion is the one case where the replacement is empty. |
| `render` without `-o` prints the page to standard output; `-C DIR` changes directory first, so a relative `-o` resolves against DIR | The same behaviour as `git -C`. |
| No command to edit or delete a comment body | Not in the brief; an edit is a reply, and deletion contradicts "nothing gets deleted". |

### 6.2 Output

Text by default, one thread per block:

```
4f0c2d1e  open  comment  doc/plan.md:12:16  "esbuild"
  Jim, 2026-09-29 10:15
  Why not Vite?
  9a7b2c11  Claude (agent), 2026-09-29 10:20
    Obsidian plugins ship one CJS file.
```

An anchored thread prints `file:line:column` and the anchored text in quotes. A changed one prints the new text, then `changed`, then `was "<original quote>"`. An orphan prints `orphaned` in place of the location, then the stored quote. An accepted deletion prints `applied`. A suggestion adds a line `replace with "NEW"` before its reason. `show` appends a `state changes:` list. Quotes are not escaped.

`comment`, `reply` and `suggest` print the new annotation's 8-character id on one
line and nothing else, so a script can capture it. The other commands print
nothing on success.

`--json` prints one JSON array, one object per thread. Each object has:

| Key | Value |
|---|---|
| `annotation` | The stored annotation, unchanged |
| `state` | `open`, `resolved`, `accepted` or `rejected` |
| `anchor` | `{"status": "anchored", "start", "end", "line", "column", "text"}`; or `{"status": "changed", ..., "text", "original"}`; or `{"status": "orphaned"}`; or `{"status": "applied"}` for an accepted deletion. `text` is the text of the range |
| `stateChanges` | The stored state-change records, unchanged, in fold order |
| `replies` | Thread objects in this same shape, in `created` order. A reply object carries its root's `state` and a copy of its root's `anchor`, and its `stateChanges` is `[]` |

| Decision | Reason |
|---|---|
| JSON output embeds the stored annotation unchanged, with computed fields beside it | A consumer gets the W3C record as stored, and the computed fields never mix into it. |
| Lines and columns are 1-based in output | Editors and `file:line:col` links count from 1. |
| `show` on a reply id shows the subtree rooted at that reply, with the root's state | The reply is the unit the person named. |
| Warnings, such as a thread decided both `accepted` and `rejected`, go to standard error in text mode only | Standard output stays parseable. |
| `sync` prints one line: `nothing to sync`, `updated the comments branch from <remote>`, `merged the comments from <remote> with the local ones`, `pushed the comments branch to <remote>`, or `the comments branch is up to date with <remote>` | A person sees what happened. |
| The three creating commands print only the id | A script or an agent takes the output as the id, with no parsing. |
| `list --json` is one array, not one object per line | One `json.loads` reads it, and an empty file gives `[]`. |

### 6.3 Ids and exit codes

An `ID` argument accepts the full uuid, the `urn:uuid:` form, or any unique
prefix of at least 6 hex characters. Reason: `git` users type short hashes, and
random uuids make 6 characters unique in practice.

| Exit code | Meaning |
|---|---|
| 0 | Success |
| 1 | Error: git failure, bad arguments, invalid file |
| 2 | An `ID` matched no annotation, or more than one |
| 3 | `accept` refused a suggestion whose anchor is `changed` or `orphaned` |

A usage error from the argument parser, and an id argument that is too short or not hexadecimal, also exit 1: the parser's own default of 2 is remapped, because 2 is reserved for a lookup that matched none or several. Reason: scripts and agents branch on the cause without parsing messages.

## 7. Testing

`cd cli && cargo test` runs everything, in a cloud session and on macOS.

| Decision | Reason |
|---|---|
| Anchoring unit tests over strings, one per row of 4.4 | The algorithm is pure and the table is its specification. |
| Integration tests run the built binary against temporary repositories, with a local bare repository as the remote | The brief requires tests that never push to GitHub. |
| Tests run git with `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1` and a fixed author | Measured: this container's global `push.negotiate true` printed errors on local pushes. Tests must not depend on the machine's git configuration. |
| JSON fixtures in `cli/tests/fixtures/`, each validated against the schema from T-03 | The brief requires every fixture to validate. |
| The parallel-write test: two clones each add annotations, both `sync`, and each ends with every annotation | This is the brief's proof of parallel writes (T-07). |

### 7.1 The acceptance run

`cd cli && just acceptance` runs `cli/ops/local/acceptance.py`. The script runs
`cargo build`, copies `example-docs/test.md` into a temporary git repository,
runs the scenarios below with the real binary, and writes `cli/target/acceptance/index.html`: one section per scenario, with the
`marq-comments render` page and the `list` output for that step. It prints the
path. `just acceptance-open` also opens the page (`open` on macOS, `xdg-open` on
Linux). The exit code is 0 when every scenario passes and 1 otherwise.

The run exists before the CLI does. While the crate is missing, or a command is
not written, a scenario stops at its first failed check and says which one, so
the count of passing scenarios measures progress. `--only 07` runs the scenarios
whose slug contains `07`, and `--no-build` skips `cargo build`. Setting
`MARQ_COMMENTS_BIN` to another binary skips the build and runs against that
binary, which is how the harness itself is tested.

| Scenario | What the page shows |
|---|---|
| A comment on a word, then an agent reply | The word marked, the thread with a Person and a Software author |
| A comment on a whole line | The line marked |
| A suggestion, accepted | The file text after the edit, state `accepted` |
| A suggestion, rejected, then reopened | State `open`, both state changes in the history |
| A paragraph added above the comments | Every anchor `anchored` at its shifted position |
| A commented paragraph moved further down | The anchor `anchored` at the new position |
| A typo fixed in a commented word | The anchor `changed`, with the original quote |
| A commented word deleted | The anchor `orphaned`, listed below the text |
| A comment resolved | State `resolved` |
| A second clone adds a comment, both `sync` | Both clones list every comment |
| Three clones decide in parallel and converge (added by T-07) | A page from each clone, identical history, both decisions, the warning on each clone |
| Twelve processes write at once (added by T-07) | Nothing lost, exactly one resolve wins, 19 commits, no merges, `fsck` clean |

| Decision | Reason |
|---|---|
| A `justfile` at `cli/` with `build`, `test`, `acceptance` and `acceptance-open` | Jim runs one command and gets a page to check, the same pattern as `macos/justfile`. |
| The script in Python, standard library only | Jim prefers Python for scripting, and no install step is needed on macOS or Linux. |
| The script lives at `cli/ops/local/` | Jim's layout for scripts run on a local machine. |
| The example is `example-docs/test.md`, moved from `macos/examples/` to the repo root | The app and the CLI both use it, so it belongs to neither directory. |
| Scenarios run on a copy in a temporary repository | The acceptance run never changes `example-docs/` or this repository's `md-comments` branch. |
| `render` is covered by tests for marks in each markdown construct, escaping, unsafe links, and whether every `#` link in `example-docs/test.md` resolves | The rendered view is where the mapping can go wrong, and a mark on the wrong word is the failure a person would not notice. |
| Output under `cli/target/`, ignored by `cli/.gitignore` | Nothing generated reaches a commit. |
| The first failed check stops a scenario | Later steps use ids and state from the earlier ones, so continuing would report noise. |
| The scenarios find their lines and words in `test.md` by text | Editing the example document does not break them, unless a word they use disappears; a setup check says so. |

## 8. Known limits

- **Renaming a markdown file** leaves its annotations under the old path. A later `move` command writes new copies under the new path. The brief does not cover renames.
- **CRLF working copies** (`core.autocrlf`) change positions between clones. Linux and macOS clones do not convert by default.
- **Clock skew** between clones can order two state changes differently from wall-clock order. Every clone still computes the same order from the same files.
- **Large files** are diffed by line in step 2, so the cost grows with the number of lines, not characters.
- **Two edits on one line** make step 4 compare whole lines, so the changed range widens across both. A character diff mapped through the quote's own range would be more precise.
- **A quote of several words** whose edit is in one word reports a changed range of that word only, not the quote's full extent.
- **A substring match counts.** After an edit, `reloading` anchors inside `reloadingg`, because the mapped position is exempt from the floor and the design asks for exact text, not word boundaries. A UI may want to show it.
- **A long quote with little surviving context** orphans even when it is unique in the file, because the floor counts context only.
- **Both sides of the context lost** while the quote stays intact and its column shifts gives a `changed` range that contains the intact quote.
- **Two clones deciding in the same second** order their state changes by random id. Every clone computes the same result from the same files, but which decision wins is arbitrary. A fraction in `created` would remove the tie.
- **A process killed between editing the file and recording `accepted`** leaves the edit in place and the suggestion open, with no message.
- **A stale git ref lock** left by a killed `git update-ref` blocks writes until a person removes it. The tool names the file and does not delete it.
- **`flock` is not reliable on network filesystems.**
- **`git gc --prune=now` during a write** could remove a blob before its commit lands.
- **Writes in one repository are serialised**, at about 31 ms each in a debug build.
- **A deleted line next to a different added line** reads as a rewrite of the line, because the line diff pairs them, so the anchor is `changed` on the new line, not `orphaned`.
- **Marks on rendered text are approximate** where the rendered text differs from the source, as in section 6.1: entities, inline code with line breaks, and text in containers whose prefix the parser strips. A range that covers only syntax with no rendered text has a card and no mark.
- **Wide tables** in the rendered view sit in one centred column, so a table with long cells is narrow and tall. It scrolls sideways inside its own box.
- **Clustered comments** push later cards down the rail, far below their highlights: three cards on one line sat at 1150, 1282 and 1373px. A card taller than the space before the next highlight pushes every later card down, and nothing is truncated or scrolled. Card numbers can run out of order down the rail, because cards are placed by the position of their highlight.
- **On resize** every card is moved out of and back into the rail each time. That is fine at this size and would flicker on a very large page. A mark in a wide table that scrolls sideways can take its top from a scrolled-out cell; this is untested.

## 9. Comments in the marq window

Mission [M-COMMENTS-UI](planning/M-COMMENTS-UI-comments-in-marq.md). Marq spawns
`marq-comments list FILE --json` (ADR 0002), reads the JSON of section 6.2, and
draws each thread as a highlight in the document and a card in a right-hand rail.
The `render` page (6.1) is the guideline for the look. Marq stays a viewer.

### 9.1 Data flow

1. `injectMarkdown()` rewrites relative image paths and calls `renderMarkdown(md, resetScroll)` as it does today. The render does not wait for comments.
2. Swift stores the file text as read and the list of image-path rewrites it made, `[location, length, replacement]` in UTF-16 units of the file text.
3. Swift spawns the CLI off the main thread: `marq-comments -C DIR list FILE --json`, with a 5 second limit, after which it kills the process.
4. On exit 0 with valid JSON, and only when no newer `injectMarkdown()` has run, Swift calls `applyComments(payload)` through `callAsyncJavaScript` with `payload` as an argument. The payload is one JSON string: `{threads, source, bom, edits}`, where `bom` is true when the file's bytes start with `EF BB BF`.
5. The page marks the text, builds the cards and places the rail. Any failure at steps 3 and 4 leaves the plain render in place, with no message.

| Decision | Reason |
|---|---|
| The CLI is found in this order: `MARQ_COMMENTS_BIN`, the copy in `Marq.app/Contents/MacOS/`, then `cli/target/debug` and `cli/target/release` beside a debug build. No `PATH` lookup. An empty `MARQ_COMMENTS_BIN` disables comments | An app started from the Finder has a `PATH` of `/usr/bin:/bin:/usr/sbin:/sbin`. The empty value lets `just check` run with comments off, so its baselines do not depend on which comments this repository's own `md-comments` branch holds. |
| The payload crosses as a `callAsyncJavaScript` argument, not as text built into a script | `injectMarkdown()` builds a JavaScript template literal and escapes three characters. Comment text is arbitrary and arrives from other writers, so a fourth character would break out of the literal. An argument needs no escaping. |
| The render goes first and comments follow | A hung, failing or absent CLI then costs the render nothing, as the mission requires. |
| Swift discards a result when a newer `injectMarkdown()` has run, and the page discards a payload whose rewritten source differs from the markdown it holds (9.2) | The file can change while the CLI runs. Both checks are cheap and the second cannot be skipped by a Swift bug. |
| A headless run waits for the first comments outcome (a payload, an empty list, a failure, the timeout, or comments disabled by an empty `MARQ_COMMENTS_BIN`, which counts as an immediate outcome) before it settles and measures | Comments arrive after the render. Without the wait, a metrics run races the CLI. |
| The page keeps the last payload and re-applies it after any wholesale replacement of `#content` | Search restores `originalHTML` (9.3). |

### 9.2 From a file offset to a rendered offset

The CLI's `start` and `end` count code points in the file on disk (4.1). The text
that marked tokenises is a different string, so the page converts each offset
through the steps below. All of them are plain functions of `source`, `bom` and
`edits`.

| Step | Transform | Reason it exists |
|---|---|---|
| 0 | When `bom` is true, subtract one code point from the offset, and an offset of 0 stays 0 | The CLI decodes the bytes with the byte order mark as a character. Swift's string reader drops it, so `source` and `md` lack it. |
| 1 | Code points to UTF-16 units in `source`: add one for each astral character before the offset | JavaScript strings count UTF-16 units. |
| 2 | File text to rewritten text: add `len(replacement) - length` for every edit that ends at or before the offset. An offset inside an edit snaps to its start, and an end inside an edit snaps to its end | `injectMarkdown()` rewrites image destinations, so the text rendered is not the file. |
| 3 | Rewritten text to normalised text: subtract one for every `\r\n` before the offset. A lone `\r` becomes `\n` and shifts nothing | The template literal and marked's `lexer` both turn `\r\n` and `\r` into `\n`. |
| 4 | Normalised text to lexed text, by marked's own rule: `text.replace(/^( *)(\t+)/gm, ...)`, which gives each tab in the first tab run after the leading spaces of a line four spaces. The page runs that regex with a callback and records each shift. `^` with the `m` flag also matches after U+2028 and U+2029. An offset inside a tab run snaps outward | marked 12.0.1 does this before it tokenises. Probed: `"\tindented"` gives a `code` token with raw `"    indented"`, and `" \t \tfoo"` expands only the first run. Reusing the regex keeps the page equal to marked on inputs that a hand-written split would not match. |

The lexed text is the result of steps 3 and 4 applied to `md`. It is not the
concatenation of the top-level tokens' raw text. The lexer consumes link
reference definitions (`[x]: url`, and `[^1]: note`) without emitting a token, and
it can add a line feed when a paragraph merges with indented code that follows
it. The page therefore locates each token in the lexed text by search from a
cursor, and accepts a token only when the gap between the cursor and the match is
empty, whitespace, or whole definition lines. A token that fails is unmarked and
does not move the cursor.

Three gates guard the result, and each failure unmarks threads and leaves their cards.

- **Gate 1.** The lexed text built from `source`, `bom` and `edits` must equal the lexed text of `md`. When it fails, no thread is marked and the metrics report `mapper: source-mismatch`.
- **Gate 2.** For an `anchored` or `changed` thread, the slice of `source` between the CLI's code-point offsets must equal `anchor.text`. It checks the code-point arithmetic, and it does not depend on the rewrites.
- **Gate 3.** The slice of the lexed text between the converted offsets must equal steps 2 to 4 applied to the slice of `source`. A range that spans a rewritten image passes, because both sides carry the rewrite.

| Decision | Reason |
|---|---|
| The page does the conversion, from `source`, `bom` and `edits` that Swift passes | A cloud session cannot run Swift, and the mapping is the part that needs unit fixtures in headless Chromium. In Swift it could not be tested there. |
| `source` is the file text with its line endings intact, the edits carry their replacement text, and `bom` is read from the file's bytes | The page can rebuild the exact string it rendered and compare, so an offset that does not apply to the rendered text is detected instead of used. Probed in Swift: `String(contentsOfFile:encoding:)` drops a leading `EF BB BF`. |
| Tokens are located by search with a gap rule, not by summing raw lengths | Probed on all 32 `.md` files in the repository and on 200,000 generated inputs: the sum of raw lengths matches the lexed text except where a link definition is consumed or a paragraph merges with indented code. |
| CRLF files map exactly | Step 3 covers them. The CLI's own limit on CRLF clones (section 8) concerns positions that differ between clones, not within one. |
| `computeLineNumbers()` is not changed in its output | The gutter's entries must be identical with comments on and off. The mapping computes its own token offsets, which also covers the tokens that `indexOf` fails to locate today. Wrapping more tokens in `data-source-line` is a decision for Jim. |

### 9.3 Marking rendered text

**Contract.** For a thread whose anchor status is `anchored` or `changed`, the
marks cover only rendered characters whose source characters lie inside the
converted range. A mark is never widened to its leaf or its block. When some
characters of the range cannot be mapped, the marks cover the rest and the card is
flagged `partly-marked`. When none can, the thread has no mark and its card says so.

**Strategy: token-guided alignment on the live DOM.**

1. Lex `md` with `marked.lexer` and locate every token in the lexed text (9.2). A top-level token's source span is its located raw text, which is exact.
2. Pair the tokens that get a wrapper (`_sourceLine` set, type not `space`) with `#content > [data-source-line]`. The pairing is accepted only when the two lists have the same length and each element's `data-source-line` equals the token's `_sourceLine`. Otherwise the whole document is unmarked and the metrics report `mapper: pairing`. An unclosed `<div>` in the markdown closes the wrapper early and nests the following wrappers, which breaks the count.
3. Build the leaves of each token in order (`text`, `escape`, `codespan`, `code`, `br`, `image`) with their expected rendered text, taken from the token and decoded as the DOM decodes it. The rendered length of an entity is the length of its decoded text, which is not always one unit: `&fjlig;` gives two, `&#x1F600;` gives two UTF-16 units, and `&foo;` is not decoded.
4. Map each leaf's characters to source characters by a monotone match inside the top-level token's source span, not inside the child's own `raw`. The child's `raw` is not reliable: marked strips blockquote and list prefixes, inserts a line feed for each list continuation line, expands tabs again inside containers, removes the escape of `|` in table cells, and trims code span padding. The matcher takes the next source character when it equals the next rendered character, and otherwise skips a source character only if it is syntax by the rules for the enclosing constructs:
   - a blockquote marker run `>` and one space at each line start;
   - a list marker and its task box `[ ]` or `[x]`, and the continuation indent of a line;
   - the delimiters of emphasis, strong, strikethrough and links, and the whole destination and title of a link or image;
   - a backslash before a punctuation character, with the pair treated as an atom;
   - the backtick run and one padding space of a code span, a `\` before `|` in a table cell, the `|` separators, and the separator row;
   - the fence lines and the indent of a code block.
5. Verify the map. Every rendered character must match exactly one source character with the same value, the whole source span must be consumed, and every skipped character must be syntax by step 4. A leaf that fails is unmappable. An unmappable leaf takes a mark only when the converted range contains its whole source span.
6. Align the leaves with the block element's text, which is the concatenation of its text nodes, search marks included, and without `.code-copy-btn`. Each leaf must start at the cursor, or after a gap of whitespace only, and when the leaves are used up only whitespace may remain. A block that fails takes no marks. Raw HTML inside a container renders text that no leaf accounts for, so it fails the block.
7. For each thread, intersect the converted range with the mapped leaves, split the text nodes at the ends, and wrap each fragment in `<mark class="cm" data-thread="ID">`. A range that crosses elements gives one mark per text node.
8. Append `<a class="cm-ref">N</a>` after the thread's last mark when numbers are on.

| Decision | Reason |
|---|---|
| Align against expected text instead of searching the DOM for the anchor's text | The constraint is that a highlight never marks other text. A search for a repeated word marks the wrong occurrence. Alignment fixes the position from the source and uses the DOM only to confirm it. |
| Not stamping `data-s` and `data-e` attributes during the render | The render would need to run again when the JSON arrives, which restarts mermaid and KaTeX and moves the scroll. A file with no comments would render different HTML. |
| The matcher works inside the top-level token's span and verifies every character | A mapping that starts at a child's offset and runs on contiguously marks the wrong characters in a blockquote and in a list continuation. Probed: in `"> first line\n> second line\n"` a range on `second` lands on `cond l`. Equal characters cannot detect a wrong start in `1. 1`, so the marker rule is explicit. |
| A leaf that fails to align fails its block | A wrong alignment would put marks on the wrong text. One unmarked card costs less than one wrong highlight. |
| A code block's "Copy" button is not part of the block's text | Its label is a text node inside the block, and it would fail every code block. |
| A mermaid block is identified by its token (`code` with `lang` mermaid) and takes no marks, whether or not the diagram has rendered. A block that holds `.katex` takes no marks, judged on the DOM at paint time. Both give the card flag `typeset-block` | The payload can arrive before `mermaid.run` resolves, when the div still holds diagram source. KaTeX also typesets prose such as `costs $5 and $6`, and the DOM is the only reliable witness. T-04 may relax this for leaves before the first typeset element. |
| A range that covers only syntax that renders as nothing (`#`, `---`, a link destination, a table separator row, a list marker, a code fence) gives a card flagged `no-rendered-text` and no mark | The comment is still shown. A mark on nothing cannot be drawn. The same rule as 6.1. |
| A `changed` thread is marked at its current range, and its card shows the original quote | 4.2. |
| `orphaned` and `applied` threads have no mark and are listed after the document | The mission requires the same section as the `render` page. It sits outside `#content`, after `#page-wrapper`, indented to the text column, so no stray child enters `#content`. |
| Overlapping threads give nested marks, and the overlap shows darker | One colour with transparency needs no rule for overlaps. |
| `paintComments()` is idempotent. It unwraps every `mark.cm`, removes every `a.cm-ref` with its text, and paints again from the model. It runs after each of the three places that assign `content.innerHTML` (`renderMarkdown`, `clearSearch`, `performSearch`), after each payload, and when the options change | `originalHTML` is captured at two points: before KaTeX runs when there is no mermaid, and after mermaid when there is. A snapshot can hold marks from an older payload. The model is the one source. Unwrapping a number would leave its digits in the text. |
| A payload that arrives while a search is active paints over the search marks, which split text nodes | Alignment reads the concatenated text of the block, so the splits do not matter, and a search mark nested inside a comment mark is valid. |
| The second marked instance for card bodies: `new marked.Marked()` with `html` escaped as text, `image` as its alt text, and `link` kept only for `http:`, `https:` and `mailto:` | Card bodies are `text/markdown` written by other people. The template's marked passes raw HTML through, and the page can post to `webkit.messageHandlers`. This is the rule of 6.1: untrusted input never becomes markup. Authors, ids, dates and anchor text enter through `textContent`. |

### 9.4 Cards, rail and layout

The rail is `<aside id="comment-rail">`, a flex sibling after `#content` inside
`#page-wrapper`. `#content` keeps its markup and its children. Each card is
`position: absolute` inside the rail, and its top is the first mark's top relative
to `#content`, pushed down by 8px where it would overlap the card above. A card
with no mark sits at the position of the block that holds its range, or at the
foot of the rail when it has none.

| Decision | Reason |
|---|---|
| The rail exists only when comments are shown and at least one thread has a card. With no threads, `#page-wrapper` keeps its 1060px and the layout is today's | A file with no comments must render as it does now, and a rail that grows the wrapper would narrow the text column below a 1349px window. |
| The rail is on when the body's content width is at least 1009px: 65px of gutter and margin, 640px of text, a 24px gap and a 280px rail. Below that, cards stack under the block that holds their first mark and the rail is removed | The `render` page does the same below 60rem. 640px is the narrowest column that tables still lay out in. |
| With the rail on, `#page-wrapper` grows to `max-width: 1364px` and the text column takes the rest, up to its 980px cap | A rail inside the 1060px wrapper would shrink the text column and move every table width. Growing the wrapper keeps the text column at 980px on any window of 1349px or more, and narrows it only below that. |
| Placement runs from the existing `ResizeObserver` on `#content` and from the `resize` handler, after `buildGutter()` | A cause of layout shift that rebuilds the gutter moves the marks too. The placement writes only to the rail, so it cannot resize `#content` and loop. |
| `a.cm-ref` uses `font-size: .7em; line-height: 0; vertical-align: sub`, as `.ref` does on the `render` page. It is `display: none` when numbers are off | A subscript with a normal line height would grow the line box and move every gutter entry below it. The acceptance run compares the gutter with numbers on and off. |
| Colours are two tokens in `:root`: `--cm-mark: rgba(233, 168, 0, 0.20)` and `--cm-mark-on: rgba(233, 168, 0, 0.38)`. A card is `rgba(31, 35, 40, 0.04)` with no border. The numbers are `#8b949e`, the gutter's grey | The template has one theme, so one pair of tokens is one per theme. The values are Notion's documented selection yellow in light mode, which is paler than the `render` page's `rgba(255, 212, 0, 0.30)`. A change is one line, and a dark theme adds one block. |
| The card holds the fields of the `render` page card: state label, short id, author and time, the body as markdown, the edit lines of a suggestion, the original quote of a changed anchor, and the replies | The look is settled. The mission changes only the border, the colour and the numbers. |
| Threads are numbered in document order, with unmarked threads after, ties by `created` | The `render` page numbers by list order, which leaves the numbers out of order down the rail (section 8). |

### 9.5 Selection and options

| Decision | Reason |
|---|---|
| A click on a `mark.cm` makes its thread active. A click on a card makes it active. A click elsewhere clears it. A click that ends a text selection, or lands on a link, does not change it | A drag to select text for copying must not move the selection of a thread. Links keep their behaviour. |
| An active thread adds `.on` to its marks and its card. Hovering a card adds `.hover` to its marks | One strong colour state for each, as on the `render` page. |
| Activating from a click on text scrolls the card into view with `block: 'nearest'`. Activating from a card does not scroll the text | The card is the thing that can be off screen. The reader is already looking at the text. |
| The active thread persists across a repaint | The model holds its id. |
| Two options, `commentsShown` (default true) and `commentNumbers` (default false), live in `UserDefaults` and are read with the `object(forKey:) != nil` pattern the zoom code uses | `bool(forKey:)` returns false for an unset key, which would turn comments off for a new user. |
| A headless run ignores the stored options and takes the defaults, or the flags | Zoom does the same. A reader's last setting must not change a measurement. |
| Show Comments is `⇧⌘C` and Comment Numbers is `⌥⌘C`, in the View menu with a checkmark | Neither is used by an existing menu item or by the template's keys. `⌘C` stays Copy. |
| The options set a class on `<html>` (`cm-numbers`), and hiding comments runs `paintComments()` with nothing to paint: no marks, no numbers and no rail are in the DOM, and the wrapper keeps its 1060px | With comments hidden the DOM and the layout are today's, so the print baselines and the claim of identical print output rest on equal DOM, not on unstyled marks that still split text nodes. Whether WebKit shapes ligatures across an unstyled inline boundary was not measured, and this avoids the question. |
| Harness flags: `--comments show\|hide`, `--comment-numbers on\|off`, `--comments-click mark:ID\|card:ID`. The click flag dispatches a real `click` event on the element | The event goes through the handler a reader's click uses. |

### 9.6 Refresh

| Decision | Reason |
|---|---|
| A change to the markdown file re-renders, then spawns the CLI again | The existing `FileWatcher` calls `loadAndInject()`, and the anchors are computed against the new text. |
| Marq watches `<git-common-dir>/refs/heads` as a directory, and `<git-common-dir>/reftable` when it exists. It resolves the directory once with `git rev-parse --git-common-dir` when a file opens | `git update-ref` writes `refs/heads/md-comments.lock` and renames it over the ref. A watch on the ref file's descriptor follows the replaced inode and misses the next update. The directory sees the create and the rename. A worktree shares the common directory's refs (`cli/src/store.rs`). |
| A ref event or a file event starts a 0.3 second debounce, then one spawn. The result is dropped when its JSON text equals the last one | `sync` and agents write in bursts, and unrelated writes in `refs/heads` give events with nothing new. |
| A file outside a git repository has no ref watch and no comments | The CLI exits 1, and the render is the plain render. |

### 9.7 Metrics

`marqMetrics()` gains a `comments` block when a payload has been applied, and
omits it otherwise. `tools/check-metrics.py` reads named keys, so the baselines
are unchanged.

| Field | Meaning |
|---|---|
| `shown`, `numbers` | The two options as applied |
| `layout` | `rail`, `stacked` or `none` |
| `mapper` | `ok`, or `source-mismatch` when the gate of 9.2 failed |
| `threadCount`, `markedCount`, `unmarkedCount`, `orphanCount` | Counts |
| `active` | The active thread's id, or null |
| `threads[]` | One entry for each thread, in number order |
| `threads[].id`, `number`, `state`, `status` | From the CLI, and the number of 9.4 |
| `threads[].anchorText` | The CLI's `anchor.text` |
| `threads[].markedText` | The text of the thread's marks joined in document order |
| `threads[].exact` | `markedText === anchorText`. True for a plain word. Acceptance asserts `markedText` against the rendered words the scenario states, for ranges that span markup |
| `threads[].flag` | `null`, `no-rendered-text`, `typeset-block`, `unaligned-block` or `source-mismatch` |
| `threads[].marks[]` | `{top, left, width, height}` relative to `#content` |
| `threads[].card` | `{top, left, width, height, inRail}`, or null |
| `threads[].offsetPx` | Card top minus first mark top |
| `threads[].replies` | Reply count |
| `overlaps[]` | Pairs of thread ids whose cards intersect |
| `orphans[]` | `{id, status}` for the threads listed after the document |
| `gutter[]` | `{line, top}` as `buildGutter()` reads them, for comparison with comments hidden |
| `markColour` | The computed background of a mark |

| Decision | Reason |
|---|---|
| The block holds only what a rectangle or a string can assert | The acceptance run asserts on numbers (CLAUDE.md: judge by measurement, not by eye). |
| `gutter[]` is part of it | It is the direct check of the constraint that the gutter does not change. |

### 9.8 Print

With comments shown, PDF export and `--export-pdf` print the highlights and the cards, in the stacked layout: each card under the block that holds its first mark, with `print-color-adjust: exact` on the marks. With comments hidden, the print path is today's. Task T-09 owns the detail.

| Decision | Reason |
|---|---|
| Print always uses the stacked layout | `layoutTablesForPrint()` sizes tables to the full printable width, and a rail would change that measure and reopen the `PRINT_SHRINK_FACTOR` work. Stacking moves cards in the flow and leaves the tables' width alone. |
| The cards move into the flow in `layoutTablesForPrint()` and back in `restoreTableLayoutAfterPrint()` | The export must leave the window as it found it. |
| `just probe-print` and `pdftool` report on the shown case | They are the instruments that see the real print engine. A Chrome reproduction cannot (CLAUDE.md). |

### 9.9 Limits

- **Typeset blocks** (KaTeX, mermaid) take no marks (9.3).
- **A document with an unclosed HTML block** (an unbalanced `<div>`) fails the pairing of 9.3 step 2 and shows every card unmarked.
- **A document with a merged paragraph and indented code** (`x`, then an indented line, then `---`) changes marked's raw text for that token, so the token is unmarked.
- **Ranges that cross a table row boundary** map only by the separators listed in 9.3 step 4, and unmappable cells take a mark only when the range contains the whole cell.
- **A file whose lexing changes beyond 9.2** (a future marked version) fails the gate and shows every card unmarked, until 9.2 is updated.
- **A range inside an image's alt text** has no rendered text, because an image is an element with no text node. The thread has a card and no mark.
- **Overlapping threads** give a darker overlap with no way to tell the two apart by colour.
- **A reftable repository** is watched through `reftable/`. A repository whose refs are only packed and updated by an external tool that does not write loose refs or reftable files is not watched, and the reader refreshes with ⌘R.
- **The `source` text is passed twice** (the render and the payload). The cost is one file's text.
- **Orphan and applied cards** use the full width below both columns, so they are wider than the cards in the rail.
