## Comments on markdown (marq-comments)

`marq-comments` stores comments and suggestions on markdown files in git, on the
`md-comments` branch. The markdown never holds them, so it stays clean. Each
comment anchors to a line or a word, and every read re-finds the anchor in the
current text.

### Install

```
cargo install --git https://github.com/jimbarritt/marq marq-comments
```

Needs git 2.38 or later and Rust 1.89 or later. In a cloud environment, put this
line in the environment's setup script, so every session starts with the tool.
Check with `marq-comments --version`.

### Flags to always pass

```
marq-comments --agent --author "Name <email>" <command> ...
```

`--agent` records the author as software. `--author` names the agent. Both go
before or after the command.

### The working loop

1. `marq-comments sync` first, to fetch the others' comments.
2. Before editing a markdown file that may have comments: `marq-comments list FILE`.
3. Act on each open thread, then `sync` after writing.

```
marq-comments list docs/test.md --state open
3f2012f6  open  comment  docs/test.md:3:41  "macOs"  changed  was "macOS"
  Jim, 2026-10-01 07:35
  Say which versions of macOS are supported.
  ce99d9d8  Claude (agent), 2026-10-01 07:35
    Added a line on versions in the README.
```

An id is the 8 characters in the first column. Any prefix of 6 or more works.

| Command | Use it to |
|---|---|
| `comment FILE --line N [--text WORD [--nth K]] -m TEXT` | Leave a note on a line, or on a word in it. Prints the new id. |
| `suggest FILE --line N --text OLD --replace NEW [-m WHY]` | Propose a change. `--replace ""` deletes. Prints the new id. |
| `reply ID -m TEXT` | Answer a thread, and correct yourself. |
| `resolve ID` | Close a comment you have dealt with. |
| `accept ID`, `reject ID` | Decide a suggestion. Only when the task asks. |
| `reopen ID` | Reopen a resolved comment or a rejected suggestion. |
| `show ID` | Read one thread with its state changes. |

`--range START:END` (code points, end excluded) replaces `--line` when you hold
positions already. A blank line cannot carry a comment.

### Anchor status

| Status | Meaning | What to do |
|---|---|---|
| `anchored` | The quoted text is in the file at `line:column`. | Act on the comment as written. |
| `changed` | The quoted text is gone and other text stands in its place. The line shows the new text and `was "<old>"`. | Read the comment against the new text. The comment may be done already, or no longer apply. Reply to say which. |
| `orphaned` | No location. The quote is printed with no line. | Reply with what you did. Never re-comment to attach it somewhere else. |

`accept` refuses a suggestion that is not `anchored` and exits 3. An accepted
suggestion reports `changed` afterwards: ignore its anchor.

### Rules

- Never edit the `md-comments` branch by hand and never commit it from the working tree.
- `accept` edits the working-tree file and does not commit it. Commit that file
  yourself, with the rest of your change.
- `resolve` only a thread you have dealt with. `accept` and `reject` only when
  the task says to.
- Write comment text as markdown. An empty message is an error.

### Exit codes

| Code | Meaning | What to do |
|---|---|---|
| 0 | Success | Continue. |
| 1 | Bad arguments, an invalid file or id, a wrong state (such as resolving a resolved comment), or a git failure | Read the message on standard error and fix the call. |
| 2 | A well-formed id matched no annotation | Run `list FILE` again, then `sync`. |
| 3 | `accept` refused: the suggestion is `changed` or `orphaned` | Do not force it. Reply, or make the edit in the file by hand and reply that you did. |

### Reading `list FILE --json`

One JSON array, one object per thread, `[]` when empty. Abridged:

```json
{
  "annotation": {
    "id": "urn:uuid:3f2012f6-33c4-4c21-89f8-a10fb6fcad94",
    "motivation": "commenting",
    "body": {"type": "TextualBody", "value": "Say which versions of macOS are supported."},
    "creator": {"name": "Jim", "type": "Person"}
  },
  "state": "open",
  "anchor": {"status": "anchored", "line": 3, "column": 41, "start": 90, "end": 95, "text": "macOS"},
  "stateChanges": [],
  "replies": [ { "annotation": {"motivation": "replying"}, "anchor": {"status": "anchored"}, "state": "open", "stateChanges": [], "replies": [] } ]
}
```

- `annotation.id`: `urn:uuid:` plus the uuid. Its first 8 characters are the id the commands take.
- `annotation.motivation`: `commenting`, `editing` (a suggestion) or `replying`.
- `annotation.body`: for a suggestion, an array. The body with `"purpose": "editing"` holds the replacement text.
- `state`: `open`, `resolved`, `accepted` or `rejected`. A reply carries its root's state.
- `anchor.status`: `anchored`, `changed` or `orphaned`. `orphaned` has no other anchor key.
- `anchor.line` and `anchor.column`: 1-based. `anchor.text`: the text now at the anchor.
- `anchor.original`: on `changed` only, the text the comment was written against.
- `replies`: threads in the same shape, oldest first.

### What it does not do

- It does not edit a comment body. Reply instead.
- It does not delete a comment, a reply or a suggestion.
- It does not follow a renamed file. Comments stay under the old path.
- It does not update in real time. Run `sync` to exchange comments.
- It never reattaches an orphan to other text.
