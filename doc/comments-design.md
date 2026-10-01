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
| `update-ref` with the expected old value, retried from step 2 up to 10 times on failure | A writer that does not take the lock below cannot overwrite another's commit: git refuses a stale old value (measured). |
| An advisory file lock on `<git dir>/marq-comments.lock` around every write and every sync merge | Measured by T-04: with the retries alone, 8 threads making 5 writes each starved one writer past 10 retries and lost its writes. The lock makes writers take turns, and the retry stays as the guard for a writer that skips it. `File::lock` needs Rust 1.89 or later. |
| The markdown blob is made with `git hash-object -w --no-filters` | The blob then holds the raw bytes the selector positions were measured on, whatever `core.autocrlf` says. |
| The temporary index lives in the git directory, named with the process id and a random suffix, and is removed on drop | Two processes never share one, and a crash leaves one small file in `.git`, not in the working tree. |
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
4. `git push <remote> refs/heads/md-comments:refs/heads/md-comments`. When git reports the push as rejected, repeat from step 1, up to 3 times. Any other push failure is returned at once.

| Decision | Reason |
|---|---|
| Full ref names in every fetch and push | An unqualified name can resolve to a different ref with no error; the tsk repository lost work to exactly this (its ADR 0008). |
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
| `marq:sourceBlob` is the git blob id of the working-tree file at the time of writing, from `git hash-object -w` | It records the exact text the selectors describe, with or without a commit, and section 4.2 uses it to map positions forward. |
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
markdown file after the edit.

| Decision | Reason |
|---|---|
| A state change is its own file in the extension vocabulary, not an edit to the annotation | Section 2.1: no file is modified, so two clones that change one annotation's state still merge clean, and every earlier state stays in the tree as well as in history. |
| A state change carries the same `@context`, `id`, `created` and `creator` as an annotation; `generator` is optional on both | A state change is a record of who did what and when, so it needs the same provenance. |
| `marq:resultBlob` is required on `accepted` and allowed on no other state | An accepted suggestion must record the text its edit produced. |
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
| `accepted` is final | An accepted edit already changed the markdown, and reopening cannot undo that. |
| `rejected` can reopen | Jim's decision (2026-09-29): rejecting changes no text, so reopening loses nothing. |
| A suggestion with both an `accepted` and a `rejected` change after a sync is reported with a warning, and the fold result stands | This happens only when two clones decide in parallel; the working-tree text shows what happened, and a person settles it. |
| `accept` re-anchors first and refuses a suggestion that is not `anchored` | Applying an edit at a stale position, or over text that changed since the suggestion, corrupts the markdown. |
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
| `render FILE [-o OUT.html]` | A plain HTML page: the markdown source in a `<pre>`, anchored and changed ranges in `<mark>`, threads beside them, orphans listed. |

Global flags: `--author "Name <email>"`, `--agent`, `-C DIR` (run as if in DIR,
as `git -C` does).

| Decision | Reason |
|---|---|
| Anchors given as a line plus a word | An agent and a person both think in "the word X on line N", and the line removes most ambiguity before the CLI computes offsets. |
| `--range` as well | The marq UI later holds exact positions and must not have to reconstruct a word and a line. |
| A `render` command | A cloud session cannot run marq, and a static page shows the resolved anchors on the real text for a person or a test to check. |
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

An orphan prints `orphaned` in place of `file:line:column`, with the stored quote.
A changed anchor prints `changed` after the location, then `was "<original quote>"`.

`comment`, `reply` and `suggest` print the new annotation's 8-character id on one
line and nothing else, so a script can capture it. The other commands print
nothing on success.

`--json` prints one JSON array, one object per thread. Each object has:

| Key | Value |
|---|---|
| `annotation` | The stored annotation, unchanged |
| `state` | `open`, `resolved`, `accepted` or `rejected` |
| `anchor` | `{"status": "anchored", "start", "end", "line", "column", "text"}`; or `{"status": "changed", ..., "text", "original"}`; or `{"status": "orphaned"}`. `text` is the text of the range |
| `stateChanges` | The stored state-change records, unchanged, in fold order |
| `replies` | Thread objects in this same shape, in `created` order |

| Decision | Reason |
|---|---|
| JSON output embeds the stored annotation unchanged, with computed fields beside it | A consumer gets the W3C record as stored, and the computed fields never mix into it. |
| Lines and columns are 1-based in output | Editors and `file:line:col` links count from 1. |
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

Reason: scripts and agents branch on the cause without parsing messages.

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

| Decision | Reason |
|---|---|
| A `justfile` at `cli/` with `build`, `test`, `acceptance` and `acceptance-open` | Jim runs one command and gets a page to check, the same pattern as `macos/justfile`. |
| The script in Python, standard library only | Jim prefers Python for scripting, and no install step is needed on macOS or Linux. |
| The script lives at `cli/ops/local/` | Jim's layout for scripts run on a local machine. |
| The example is `example-docs/test.md`, moved from `macos/examples/` to the repo root | The app and the CLI both use it, so it belongs to neither directory. |
| Scenarios run on a copy in a temporary repository | The acceptance run never changes `example-docs/` or this repository's `md-comments` branch. |
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
