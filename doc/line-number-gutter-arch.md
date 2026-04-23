# Line Number Gutter — Architecture & Mechanism

This document explains how the line-number gutter is built and why it can drift out of alignment with the rendered content. It is a reference for debugging — it does **not** prescribe a fix.

All code references are to `Sources/marq/Resources/template.html`.

---

## High-level idea

Markdown is parsed into a tree of **block tokens** by `marked.js`. We stamp each top-level block with the source line it came from, render the HTML, then measure where each stamped block ends up vertically on screen. We then draw the gutter as a stack of absolutely-positioned coloured blocks whose tops match those measurements. Each gutter block displays the source line number for the markdown block it sits beside.

The gutter and the content are **flex siblings** inside `#page-wrapper`. The gutter is a fixed-width column on the left; the content fills the rest.

---

## The pipeline (in execution order)

### 1. Lex the source

`computeLineNumbers(src)` (line 384):

- Calls `marked.lexer(src)` to get a flat list of top-level tokens.
- For each token with `.raw`, finds its character offset in the source via `indexOf`, then converts that offset to a 1-based line number using `charToLine`.
- Stamps `token._sourceLine = N` onto the token.
- Uses a moving `searchFrom` cursor so identical raw strings (e.g. blank lines, repeated headings) match in order.

Output: token list where each block knows its source line.

### 2. Render token-by-token, wrapping each block

`renderWithLineNumbers(src)` (line 415):

- Iterates tokens, calls `marked.parser([token])` for each one individually so we can wrap each piece of HTML.
- Wraps the rendered HTML in `<div data-source-line="N">…</div>` **unless** the token type is `'space'`.
- The `'space'` skip is important: marked emits a `space` token for blank lines between blocks. Wrapping those would produce invisible phantom DOM children that throw off the alternating stripe pattern in the gutter.

Output: HTML where every visible top-level block is a direct child of `#content` carrying `data-source-line`.

### 3. Insert into the DOM and trigger gutter build

`renderMarkdown(md, resetScroll)` (line 441):

- Sets `content.innerHTML = renderWithLineNumbers(md)`.
- Schedules `buildGutter()` via `requestAnimationFrame` so layout has settled.
- Also re-runs `buildGutter()` after **all images load** — image dimensions change content height, which would invalidate the first measurement.
- `mermaid.run()` and `renderMathInElement()` (KaTeX) also run here. Both can change layout heights asynchronously; only images currently trigger a re-measure.

### 4. Measure and draw the gutter

`buildGutter()` (line 504):

1. Clear the gutter.
2. Query `content.querySelectorAll(':scope > [data-source-line]')` — only **direct children** of `#content`.
3. For each element:
   - `rect = el.getBoundingClientRect()`
   - `top = rect.top - contentRect.top` (content-relative offset)
   - Push `{line, top}` onto `entries`. (Entries with duplicate line numbers are skipped via `seenLines` set.)
4. `totalHeight = content.scrollHeight`.
5. For each entry `i`:
   - `top = entries[i].top`
   - `bottom = entries[i+1].top` (or `totalHeight` for the last entry)
   - Create a `.gutter-block` `<div>` at `top`, height `bottom - top`, alternating background by index.
   - Append a `.line-number` span containing the source line.
6. Set `gutter.style.height = totalHeight + 'px'` so the gutter column extends to the bottom of the content.

A `resize` listener (line 594) re-runs `buildGutter()` because wrapping changes block positions.

---

## Layout context (CSS)

```
body              padding: 32px (16px top)

#page-wrapper     display: flex
                  max-width: 1060px

  #line-gutter    position: relative
                  width / min-width: 48px
                  border-right, margin-right: 16px

  .markdown-body  flex: 1
                  max-width: 980px
                  align-self: flex-start    ← uncommitted change
```

Inside `#line-gutter`:

```
.gutter-block     position: absolute
                  left: 0; right: 0
                  display: flex
                  align-items: flex-start
                  padding-top: 4px
                  padding-right: 8px
                  justify-content: flex-end
                  (top + height set in JS)
```

Key properties of the geometry:

- `.gutter-block` is positioned absolutely **inside the gutter column**, not inside the content column. So its `top` is relative to the gutter's top-left.
- The JS computes `top` as `el.getBoundingClientRect().top − contentRect.top`. This is the offset of the block within the **content** column.
- For the gutter block to line up with the content block, **the gutter's top edge must be at the same y-coordinate as the content's top edge** in the viewport. Otherwise every gutter row is shifted by the same delta.
- That assumption is satisfied when the two flex siblings share a top alignment — which they do under the default `align-items: stretch` and also under `align-self: flex-start` on the content.

---

## Where alignment can go wrong (failure modes seen historically)

These are the failure modes the codebase has hit before. None is a current diagnosis — they are listed so a fresh look knows what shape the bugs tend to take.

1. **Top-edge mismatch between gutter and content.**
   If for any reason `#line-gutter.getBoundingClientRect().top !== #content.getBoundingClientRect().top`, every gutter block is shifted by the constant delta. Symptom: alignment off by a uniform amount across the whole page (not worsening down the page).

2. **Content height inflated by gutter height (resolved by `align-self: flex-start`).**
   `buildGutter` sets `gutter.style.height = content.scrollHeight`. Under default flex `align-items: stretch`, that explicit gutter height stretches the row, which stretches `.markdown-body` via flex-stretch, which inflates `content.scrollHeight` on the **next** render. When switching from a tall file to a short file, the old (large) gutter height bled into the new content's measured `scrollHeight`. Symptom: trailing whitespace at the bottom of small files after viewing a large one.

3. **Phantom `data-source-line` divs from `space` tokens.**
   Wrapping `space` tokens produced empty but DOM-present children. They were picked up by `buildGutter`'s query, but had zero height and the same `top` as the next visible block — which interacted badly with the alternating stripe colouring (and prior versions of the dedup logic). Symptom: alternating stripes wrong / off-by-one. Resolved by the `token.type !== 'space'` guard at line 432.

4. **Measurement before async layout completes.**
   Mermaid diagrams and KaTeX both render asynchronously and can change block heights after `buildGutter` runs. Only image loads currently trigger a re-measure. Symptom: drift that gets worse the further down the page you go, especially in documents containing diagrams or math.

5. **Selector scope.**
   `:scope > [data-source-line]` deliberately matches only **direct children** of `#content`. If anything ever wraps blocks in an extra container (a future renderer change, a plugin, a wrapping class) the gutter goes blank or under-counts. The same goes for the dedup `seenLines` set: two blocks legitimately starting on the same source line will collapse to one gutter entry.

6. **Last-block extends to `scrollHeight`.**
   The final gutter block's height is `totalHeight - lastTop`. If `totalHeight` is wrong for any reason (e.g. failure mode 2, or images not yet loaded), the bottom block over- or under-shoots and the gutter's bottom edge no longer matches the content's bottom edge.

---

## Debug session — 2026-04-23

### Confirmed: failure mode (2) is real

Live DevTools measurements on `move-window-to-space-flow.md` after window-resizing:

```
content scrollHeight: 4419
last block bottom (rel): 3348.66
phantom px: 1070.34
gutter offsetHeight: 4419
```

i.e. `#content`'s reported height was 1070px larger than where its last visible block actually ended. The flex-stretch feedback loop described in failure mode (2) inflates `scrollHeight` once `gutter.style.height` is set, and resizes compound it (each rebuild reads the inflated value).

### Fix applied: `align-self: flex-start` on `.markdown-body`

Adding `align-self: flex-start` opts content out of flex-stretch. After applying it live and calling `buildGutter()`:

```
content scrollHeight: 3365
last block bottom (rel): 3348.66
phantom px: 16.34   ← normal trailing margin-bottom
gutter offsetHeight: 3365
```

Trailing block bug eliminated. `align-self: flex-start` is the right tool for failure mode (2).

### Remaining bug: initial-load drift

After baking `align-self: flex-start` into the CSS and rebuilding, the trailing block bug is gone, but **initial paint** of a freshly opened document shows the gutter line numbers shifted relative to their content. A single window-resize fixes it (the `resize` listener triggers a fresh `buildGutter` which measures from a now-stable layout).

This matches failure mode (4) in shape — measurement before layout has fully settled — even on documents with no mermaid / katex / images. Likely contributors: highlight.js styling code blocks, GitHub markdown CSS cascade, font metrics. The current `requestAnimationFrame(buildGutter)` runs one frame after `innerHTML` is set, which isn't always enough.

### Fix options for initial-load drift

**A. ResizeObserver on `#content`.** Add a `ResizeObserver` that re-runs `buildGutter` whenever `#content`'s box changes. Catches every cause of layout shift in one mechanism: initial settling, font loads, async highlight/mermaid/katex, image loads, window resize. Can subsume the existing image-load loop and `resize` listener. **Cost:** small JS addition. **Risk:** must guard against re-entrant rebuilds (ResizeObserver fires when we set `gutter.style.height` if that affects content sizing — with `align-self: flex-start` it shouldn't, but worth verifying).

**B. Defer initial `buildGutter` further.** Replace the single `requestAnimationFrame` with nested rAF (`rAF(() => rAF(buildGutter))`) or a `setTimeout(buildGutter, 50)`. Buys an extra frame for layout to settle. **Cost:** trivial. **Risk:** fragile — works until something takes >2 frames, then fails silently again. Doesn't help with later async layout changes.

**C. Wait for `document.fonts.ready`.** `await document.fonts.ready` before initial `buildGutter`. **Cost:** trivial. **Risk:** probably not the cause here (we use system fonts only) — would only help if a remote/web font is added later.

### Decision: **option A** (ResizeObserver).

Replaces fragile timing assumptions with a declarative "rebuild whenever content geometry changes" rule. Same observer also handles the existing `resize`-listener case and the image-load case, so it can simplify the code rather than just adding more.

---

## Outcome

Option A was implemented in `Sources/marq/Resources/template.html`:

- Added `ResizeObserver` on `#content` (near bottom of the script block) that calls `buildGutter()` whenever content geometry changes. Comment notes the re-entrancy safety (depends on `align-self: flex-start` preventing content from being resized when `gutter.style.height` is set).
- Removed the old `window.addEventListener('resize', ...)` listener — subsumed by the observer.
- Trimmed the image-load loop in `renderMarkdown` — it no longer calls `buildGutter` (observer handles it); its remaining job is just re-applying `window.scrollTo(0, 0)` after all images load (so the page doesn't end up scrolled mid-image-load).

Side change in `Sources/marq/MarqApp.swift`: added `webView.isInspectable = true` (gated to `if #available(macOS 13.3, *)`). Enables Safari to attach Web Inspector to the embedded WKWebView via Safari → Develop → [Mac name] → marq. Critical for future debugging.

Verification tested live with `/Users/jmdb/Code/github/ubiqtek/tilr/doc/arch/move-window-to-space-flow.md`:

- Initial paint: line numbers correctly aligned (95→heading, 97→code block, 103→paragraph, 105→bullet) without needing a manual resize.
- Aggressive resizing: alignment stays correct, no trailing-block reappearance.
- Phantom px (scrollHeight − last block bottom) stays at ~16px (the normal trailing margin).

Both failure modes resolved: (2) trailing block from flex-stretch feedback loop, and the initial-load drift symptom (which fit the shape of failure mode 4 even without mermaid/katex/images — likely caused by highlight.js/CSS settling after the initial requestAnimationFrame).
