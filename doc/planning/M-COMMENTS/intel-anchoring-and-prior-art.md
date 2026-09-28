# Intel: T-01, the W3C selector model, Hypothesis re-anchoring, and the four analyses

| Field | Value |
|---|---|
| Mission | [M-COMMENTS](../M-COMMENTS-git-backed-comments.md) |
| Task | T-01: read the intelligence |
| Written in | a Claude Code cloud session, 2026-09-28 |

This is the record T-01 asks for. T-02 (the design doc) draws its anchoring and
storage decisions from what is written here.

## The W3C Web Annotation Data Model: selectors

Read from `https://www.w3.org/TR/annotation-model/`.

**`TextQuoteSelector`**: `exact` (the quoted text, after normalisation),
`prefix` (text immediately before it), `suffix` (text immediately after it).
Example from the spec:

```json
{
  "type": "TextQuoteSelector",
  "exact": "anotation",
  "prefix": "this is an ",
  "suffix": " that has some"
}
```

**`TextPositionSelector`**: `start`, `end`, character offsets, position 0
immediately before the first character.

**`refinedBy`**: a selector may be refined by one or more others. Multiple
selectors on one target are alternatives expected to resolve to the same
selection, not a fallback order the spec itself defines, so the CLI decides
its own priority among them (Hypothesis's order is below).

**Target as a `SpecificResource`**: `{"source": "<iri>", "selector": {...}}`,
exactly the shape the mission's `Blocked by`/`Target` needs for a comment on
part of a file.

**Motivations**: `commenting`, `editing` ("intends to request a change,"
covers a suggestion), `replying` ("intends to reply to a previous statement,
either an Annotation or another resource").

**Replies target other annotations**: the spec's `replying` motivation
description confirms a reply's target can be another annotation's id, though
the spec gives no worked example of it. The mission brief's "Replies are
annotations whose target is another annotation" is a correct reading, not
spelled out verbatim in the spec.

**`bodyValue`**: a body that is a single `xsd:string`, no language tag,
the simplest way to carry comment text as a body rather than a linked
resource.

**`created`**/**`creator`**: `created` is an `xsd:dateTime` in UTC (`Z`
suffix); `creator` is a human, organisation or software agent, matching the
constraint that author comes from `git config user.name`/`user.email`.

## Hypothesis's re-anchoring: read from `hypothesis/client` source, not docs

`web.hypothes.is`'s own help pages are marketing copy with no algorithm on
them; the mission brief's instruction to "read how it re-anchors before
designing yours" means the source, at
`src/annotator/anchoring/{html,types,match-quote}.ts` in the `hypothesis/client`
repository (cloned and read directly for this task).

### The fallback order, and why position feeds into quote rather than standing alone

`anchor()` in `html.ts` builds one promise chain, cheapest first, each stage
catching the previous stage's failure:

1. `RangeSelector` (a DOM range, effectively an XPath plus offset). Fastest,
   most fragile: any DOM structure change breaks it.
2. `TextPositionSelector` (character offsets into the document's text).
3. `TextQuoteSelector` (exact quote plus prefix/suffix context). Slowest,
   most robust, since it does not depend on the document's structure or
   length staying the same.
4. `MediaTimeSelector` (audio/video, not relevant here).

The detail worth taking: `TextPositionSelector.start` is not only tried on
its own, it is passed as a `hint` into the quote matcher (`options.hint`) so
that when position resolution fails and quote matching runs, the search
prefers a match near where the position selector expected one. Position and
quote are not independent fallbacks; position narrows quote's search.

**`maybeAssertQuote`**: after a `RangeSelector` or `TextPositionSelector`
resolves to a range, Hypothesis checks the resolved text against the stored
quote's `exact` and throws a mismatch if it differs, falling through to quote
matching instead of trusting a cheap selector that now silently points at
the wrong text. The quote acts as a checksum on the cheaper selectors, not
only as its own fallback selector.

### The fuzzy match itself, and a real limitation worth not copying blind

`match-quote.ts`'s `matchQuote(text, quote, {prefix, suffix, hint})`:

- Tries an exact substring match first (fast path, `indexOf` in a loop).
- Falls back to `approx-string-match` (edit-distance search) with
  `maxErrors = min(256, quote.length / 2)`, i.e. up to half the quote's
  characters may differ.
- Scores every candidate: 50% weight on similarity to the quote itself, 20%
  each on the actual surrounding text matching the stored `prefix`/`suffix`,
  2% on proximity to the position hint. Returns the single highest-scoring
  candidate.

**The limitation**: there is no minimum-quality threshold. `matchQuote`
returns null only when the search finds *no* candidate within the error
budget; if it finds even one low-scoring candidate, that candidate is
returned and treated as resolved, however poor the score. Hypothesis has one
binary outcome, resolved or orphaned, with nothing in between: an anchor can
silently attach to a materially different but superficially similar piece of
text after a large edit, and nothing surfaces that as uncertain rather than
correct.

This is the one place this mission should not copy Hypothesis directly. The
brief requires "After an edit that removes the anchored text, the CLI
reports the anchor as orphaned. It does not attach it to other text." A
scored match with no floor can violate the second half of that sentence.
**Design implication for T-02**: pick an explicit minimum score (or maximum
edit-distance fraction) below which the CLI reports an anchor orphaned
rather than silently re-anchoring to the best available guess.

### Orphan detection, mechanically

In `guest.ts`, `locate()` wraps `anchor()` in a `try`/`catch`: success
attaches a `region` to the anchor object, failure attaches none. An
annotation's `$orphan` flag is true when every target that carries a
selector failed to resolve to a region. Orphan is a plain boolean computed
from whether resolution succeeded, not a separate state stored in the
annotation itself; the annotation record is unchanged; only the client's
runtime view of it marks it orphaned. **For a CLI that has no persistent
runtime session, this means the CLI decides orphan status fresh on every
`list`/resolve call, not by reading a stored flag.**

## The four tool analyses, read in full

Already summarised at a high level in the mission brief's Intelligence
section; this is what each contributes specifically to anchoring and
storage design, past that summary.

### `agent-comments` (Obsidian, CriticMarkup)

- No comment IDs at all. A comment's identity is computed at parse time from
  `(node kind, character offset)`, so it changes whenever text before it
  changes. There is no "same comment across edits" concept; adjacency and
  position *are* the whole model.
- `SourceEdit` carries an `expected` substring and a `before` context before
  applying an edit, and `rebaseEdit` refuses if the document moved. This is
  a much smaller version of quote-plus-context verification, applied to
  edits the tool makes itself, not to re-anchoring existing comments.
- Resolve deletes the thread from the file outright. No resolved-history
  concept exists; git is the only record after that.
- Relevant to M-COMMENTS: confirms that "thread by textual adjacency, no
  stable ID" is a real, shipped design, and that it loses the ability to
  keep a resolved comment's history, which this mission's constraint
  ("History is never destroyed") explicitly rules out for M-COMMENTS.

### `obsidian-criticmarkup` ("Commentator")

- Anchors by adjacency only: a comment attaches to whichever CriticMarkup
  range immediately precedes it in the text (`right_adjacent`). No line
  anchor exists at all; a user highlights the line's text instead of a
  purpose-built line selector.
- Metadata (author, time, resolved) is optional, JSON embedded inline after
  `@@`, off by default. Confirms embedding structured metadata inline in
  markdown is possible but ships disabled, and the project's own code has a
  TODO flagging it as an injection risk, worth noting as a reason M-COMMENTS
  keeps annotations out of the markdown entirely (already a constraint).
- The vault-wide index is IndexedDB, local-only, not in git, rebuilt from
  file content and comments on staleness. This is the same shape as
  Hypothesis's own runtime orphan check: an index is a derived, disposable
  cache, never the source of truth.

### `hedgedoc`

- No comments, no suggestions, in either version. Its contribution here is
  negative evidence: even a mature, funded real-time editor with CRDT sync
  has not built either feature, and the one third-party product that has
  (HackMD) is closed source with no published anchoring model. There is no
  existing production implementation of "Yjs relative position as a comment
  anchor" to read, only CollabMD's (below).

### `collabmd`

- The only one of the four with both a working anchor-survives-edits story
  and comments actually landing near git (though excluded from it by
  default). Its anchors are Yjs relative positions (CRDT item references),
  not W3C selectors, and they are strictly weaker outside a live session:
  they resolve correctly only against the exact Yjs document history that
  created them. An external edit (a plain `git pull`, an edit outside the
  tool) deletes the Yjs snapshot outright, and the next session hydrates
  from scratch, so the relative positions reference items that no longer
  exist. The only fallback is the *original* creation-time line number,
  never updated, with no quote-based re-anchoring attempted at all.
- Confirms the mission's own reasoning for choosing W3C selectors over a
  CRDT-position anchor: a CRDT position is only ever as durable as the CRDT
  history sitting behind it, which is exactly the sidecar, disposable state
  this mission's CLI has no equivalent of (the CLI has no long-running
  session to hold a live document model in).
- Resolve deletes the thread from its `Y.Array` outright, same as the other
  three tools. None of the four keeps resolved-comment history. M-COMMENTS'
  "nothing gets deleted" constraint is not precedented by any of them; it is
  a genuine departure, not a documented pattern to follow.

## What this settles for T-02

- **Selector types**: `TextQuoteSelector` (primary, most durable) and
  `TextPositionSelector` (secondary, a hint/fast-path), per the brief's
  existing constraint. `RangeSelector`-equivalent (a raw line/character
  offset with no text-based verification) is not needed: none of the four
  tools' more DOM/CRDT-shaped equivalents survived edits any better than a
  quote does, and Hypothesis itself treats its range selector as the
  cheapest, least trusted rung, checked against quote regardless.
- **Fallback order**: position as a hint into quote matching, not two
  independent attempts. Matches Hypothesis; better than trying position
  alone and only falling back to quote on total failure, since position
  narrows quote's search rather than being wasted work on failure.
- **Orphan is a floor, not "no match at all"**: pick an explicit minimum
  match quality below which the CLI reports orphaned rather than
  re-anchoring to a low-confidence guess. This is the one place to diverge
  from Hypothesis's own code, not follow it.
- **Orphan status is computed, not stored**: consistent with Hypothesis and
  with this mission having no persistent runtime; `list` resolves every
  anchor fresh against the file's current content each time it runs.
- **Resolved-comment history is unprecedented territory**: none of the four
  tools model it, so T-02 has no existing implementation to crib from for
  *how* a resolved/accepted/rejected state coexists with "nothing gets
  deleted." That part of the design doc is original work, not adaptation.
