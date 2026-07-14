# Clip Prevention

How linux-lit keeps a **partial (half) text row** from showing clipped against a
card's top rule or bottom rule. This is the consolidated reference for every
free-scroll surface: the synopsis/gloss/echoes overlay, the journal Q&A overlay,
the translations overlay, and scroll-mode (`j`/`k`) on the main card. (Extracted
and expanded from `page-turning-mechanics.md`, which keeps the *pagination*
clip — a different algorithm, see "Not the same as the paged clip" below.)

> **If you are debugging a "half line clipped at the bottom edge" bug, jump to
> "The failure checklist" at the end — it is ordered by how often each cause is
> the culprit.** The single most common cause is a surface that recomputes the
> clip only at named moments and is **missing the `value_changed` catch-all**.
>
> **First, though, check WHICH row is cut.** If the cut row is the *highlighted*
> cursor line (mid-page, room below it), it is NOT a viewport clip at all — it is
> the highlight `paragraph_background` band lacking `pixels_below_lines`. See
> "A different clip: the HIGHLIGHT band cutting descenders" and checklist #9.
>
> **On a two-column PLAY, also check the log for `PAGES: table hit` + a
> `BOTTOM_CLIP_EXACT` line whose `total` exceeds `widget_h` (clip=0).** That is a
> STALE pinned `play_pages` split rendering over-tall for the current card — a
> data bug fixed by regenerating the table, not a clip-math bug. See checklist #12.

## The two edges, two mechanisms

A scrolled text surface can clip a partial row at EITHER edge, and each edge has
its OWN fix. They are independent — a surface needs both.

- **Top edge → line-snapping.** Keep the viewport top aligned to a whole row so
  no fractional row ever sits under the title rule. There is NO top mask. If the
  snap is wrong, the first line clips and nothing hides it.
- **Bottom edge → an invisible clip box.** A `gtk4::Box` overlaid at the bottom
  of the scroll area, painted with the card background color, sized to exactly
  cover the partial last row straddling the viewport bottom (including its
  descenders).

CSS `mask-image` was tried for the fade and does **not** work — GTK4 (4.22)
silently ignores `mask-image` on widgets. Do not reach for it.

## The bottom clip box

`bottom_clip` is a `gtk4::Box`:
- `valign = End`, `halign = Fill`, `vexpand = false`, `can_target = false`
- `add_css_class("gloss-bottom-clip")` — the CSS `.gloss-bottom-clip {
  background-color: <card_bg> }` (in `theme.rs`) makes it paint the card
  background and HIDE (not recolor) whatever is beneath it.
- Added to the scroll `Overlay` with `add_overlay`, then
  `set_measure_overlay(&clip, false)` and `set_clip_overlay(&clip, true)`.

Its height is recomputed (never fixed) by the shared helper, below.

## The shared algorithm (`src/ui/mod.rs`)

The descender-correct clip math is a set of free helpers so every surface shares
ONE implementation (it used to be copy-pasted and drifted):

- **`display_rows(view) -> Vec<(top, bottom)>`** — walks each VISUAL (wrapped)
  row via `forward_display_line` + `iter_location`, adding `view.top_margin()`
  to every row so the rows are in **vadjustment / scroll-coordinate space**
  (see the coordinate-space gotcha below). Used by TextView-content surfaces.
- **`bottom_clip_height(rows, top_y, viewport_h, content_h) -> i32`** — the
  **pure** clip math. Finds the bottom of the last row that fits ENTIRELY above
  the viewport bottom (`top_y + viewport_h`), returns
  `viewport_bottom − last_full_bottom` to cover the leftover partial row. Three
  guards: empty viewport → 0; document ends inside the viewport → cover only the
  slack below `content_h`; a single row taller than the viewport → 0 (don't blank
  a row that can't fit). Unit-tested in `ui::bottom_clip_tests`, including a
  non-uniform-row case a uniform-step estimate gets wrong.
- **`recompute_overlay_bottom_clip(view, clip, scrolled)`** — the GTK wrapper for
  a TextView-content scrolled window: reads `display_rows`, calls
  `bottom_clip_height`, sets the clip's `height_request`.
- **`line_yrange_rows(view, top_val, viewport_h)`** — the logical-line analog of
  `display_rows`, for scroll-mode (`j`/`k`) which clips on whole-line
  `line_yrange` geometry, not wrapped rows.
- **`recompute_overlay_bottom_clip_box(clip, scrolled)`** — the variant for an
  overlay whose scrolled child is a widget **Box** of WHOLE-WIDGET rows (no inner
  TextView that wraps). A Box never splits such a child across the edge, so there
  is no partial wrapped row — it only covers trailing slack when the content ends
  inside the viewport. **Caveat — a Box of TextViews still wraps.** If the Box's
  children are wrapping TextViews, each renders a partial wrapped row at the
  viewport edge, and this box-slack guard (which clips 0 on overflow) leaves that
  row cut. **The lesson learned the hard way:** the 2-col translation overlay was
  exactly this (paired original/translation TextViews stacked in a scrolled vbox),
  and a per-row mask across two independently-wrapping columns proved fragile
  (coordinate-mapping bugs, an un-snapped top row, the highlight off-screen on
  open). The fix was to **stop scrolling and paginate** — see "Pagination instead
  of a mask" below. A Box of wrapping TextViews is a sign you may want pagination,
  not a clip.

## Pagination instead of a mask (the translation overlay)

When a surface stacks **wrapping TextViews** and you find yourself fighting the
bottom clip across them, the robust answer is the main card's strategy:
**paginate** — render only the whole units (here, whole speaker blocks) that fit,
so the last unit ends above the bottom edge and **no partial row is ever
rendered**. No mask, no scroll, no `compute_point` coordinate math, no settle
race. The 2-col translation overlay (`src/ui/translation_overlay.rs`) does this:
`paginate(block_heights, page_height)` (pure, unit-tested) packs whole blocks per
page; block heights are measured with a standalone `pango::Layout` (synchronous,
no GTK allocation); the cursor's page is rendered around the reader cursor, so the
highlight paints immediately. The bottom-clip machinery it used to need
(`attach_custom`/`Custom` guard, a per-row translation mask) was deleted. See
`docs/plans/2026-06-27-paginated-translation-overlay-design.md`.

**Pin every rendered TextView's `height_request` to its MEASURED height — do
not trust GTK's lazy natural height.** A paginated surface rebuilds its content
widgets on every page turn (`render_page` swaps fresh `TextView`s into the
already-mapped `content_vbox`). A freshly-built `TextView` reports a COLLAPSED
natural height (0px, or one row) until its pango layout runs on a later pass —
so on a page-turn re-render GTK can allocate the new blocks at that collapsed
height and paint BEFORE the real layout settles. The whole page then squeezes
into a thin band at the top of the card, the first line half-cut, the rest blank
— and it is **timing-dependent** ("sometimes clips, sometimes not"), because some
turns settle in time and some don't. `queue_resize()` does NOT reliably fix it
(the race is that the natural-height measurement itself hasn't run). The durable
fix is to `set_height_request(measured_h)` on each column view from the SAME
synchronous `measure_text_height` the pagination already computes
(`set_view_height` in `translation_overlay.rs`): `measured_text_h + line_spacing
* num_paragraphs + descender_pad`. With an explicit height the block can never
collapse, independent of when GTK lazily measures. See failure checklist #13.

**That pinned height MUST include a descender allowance, or the last line clips
when `line_spacing == 0`.** `measure_text_height` returns pango's LOGICAL height
(`Layout::pixel_size().1`), which rounds down to the whole pixel at the
baseline+descent boundary — but a GTK `TextView` paints the final line's
descender ink to the font's full descent, one or two px past that logical bottom.
When there is below-line spacing (`line_spacing > 0`, prose) that spacing absorbs
the overhang; but **verse plays pass `line_spacing == 0`** (the reading card
renders verse tight — see `rebuild_translation_overlay`), so a view pinned to
exactly `text_h` slices the last line's descenders (`y`/`g`/`p`/trailing comma).
The tell: the OVERLAY's bottom-most line (e.g. "By your leave.") shows cut
descenders even though the page ends well ABOVE the card's bottom edge — so it is
NOT a page-fill/bottom-cap clip, it is the last line's own pinned view boundary
mid-card. Fix: add `descender_pad(body_font_size)` (≈15% of the point size,
floored at 3px) to the pinned height in `set_view_height`, AND mirror the same
pad into the pagination height accounting (`block_height`, and the split-budget
seed in `split_oversize_blocks`) so pages don't over-pack now that each view is
taller. See failure checklist #14. (`translation_overlay.rs`, 2026-07-14.)

**Per-row geometry is mandatory for prose, never a uniform row-step.** The
synopsis/gloss/journal buffers join paragraphs into single multi-row buffer
lines with per-tag `pixels_above_lines`/`scale`, so rows are NOT uniform.
`line_yrange` (logical-line granular) collapses a wrapped paragraph to one
paragraph-tall "row" and clips the wrong amount; a uniform `step` estimate cuts
the last line's descenders. The journal overlay's original descender bug was
exactly this — it used a `line_yrange` row-step before the unification.

## A different clip: the HIGHLIGHT band cutting descenders (not the viewport)

Not every "descenders cut at the bottom" is a viewport/page-edge clip. The cursor
line is highlighted by a `cursor-line` `TextTag` with `paragraph_background`. That
band paints the paragraph's logical-line rectangle — which, **with no per-line
spacing, ends flush at the line's logical bottom and slices the glyph
descenders** of the highlighted line (`y`, `g`, `p`, a trailing comma). This is
NOT the bottom-clip box, NOT pagination, and NOT a viewport-edge partial row — it
happens on ANY highlighted line, mid-page, with plenty of room below it.

- **Tell:** the pink/tinted highlight band's bottom edge cuts through the
  descenders of the highlighted line, while the lines above/below are fine and the
  page is nowhere near full. A page-edge clip instead cuts the LAST visible row;
  this cuts whatever row is *highlighted*.
- **Cause:** the surface's `TextView` set no `pixels_below_lines` (and/or
  `pixels_above_lines`). GTK's `paragraph_background` covers only the logical line
  box; the inter-line spacing is what gives the band room below the descenders.
  The MAIN reading card never shows this because it sets
  `pixels_above_lines`/`pixels_below_lines = config.line_spacing` (default 5px) on
  `text_view`/`right_view` (`src/app/mod.rs`).
- **Fix:** set `set_pixels_above_lines(line_spacing)` +
  `set_pixels_below_lines(line_spacing)` on the overlay's TextViews, matching the
  main card. Thread `config.line_spacing` to the surface rather than hardcoding.
  **If the surface PAGINATES from measured block heights** (the translation
  overlay), also add the new spacing to the height measurement
  (`2 * line_spacing * num_paragraphs` per block — GTK adds the spacing above AND
  below every paragraph) so pages don't over-pack now that lines are taller.

The 2-col translation overlay (`src/ui/translation_overlay.rs`) hit exactly this:
its paginated columns set no line spacing, so the cursor line's descenders were
sliced in BOTH columns. Fixed by threading `line_spacing` through `RenderCtx` →
`make_column`/the interlude view + correcting `block_height`. See failure
checklist #9.

## When the clip MUST be recomputed (the three paths)

The clip height is only correct for the viewport geometry it was computed
against. Geometry changes at three kinds of moment, and a surface must recompute
at ALL THREE or it clips:

- **(a) On open / reveal — the `changed`-signal handler + idle backstop.**
  `set_visible` and `apply_font` recompute the vadjustment *range* on a LATER
  layout pass, so an inline or single-idle recompute runs against a 0-height or
  stale viewport and over-clips (hides the whole body until the first scroll).
  The fix connects a **one-shot handler on the vadjustment `changed` signal**
  (fires when the range is recomputed, i.e. layout settled), snaps to top,
  recomputes the clip, then disconnects — with an `idle_add_local_once` backstop
  for the case where `changed` already fired. This is `reset_scroll_top` in the
  gloss overlay.
- **(b) On the named scroll methods.** Explicit `update_bottom_clip()` calls
  inside `scroll_*_to_top` / `scroll_*_to_bottom` / the paged scroll fns.
- **(c) On EVERY value change — the `value_changed` catch-all.** A handler on the
  scrolled `vadjustment().connect_value_changed` that calls
  `recompute_overlay_bottom_clip`. Path (a)'s `changed` handler fires only while
  the *range* shifts (during an open); once the range is stable and the user
  merely scrolls, only `value_changed` moves, and without (c) the clip keeps its
  stale height and a partial row pokes through on scroll.

## BottomClipGuard owns the three paths (use it for any free-scroll surface)

`src/ui/bottom_clip_guard.rs` packages the lifecycle so a surface cannot drop a
path: `attach()` (TextView) / `attach_box()` (Box child) build the clip box AND
wire path (c) in one call (via `wire_recompute_signals`); `on_open()` is path
(a); `recompute()` is path (b). The gloss, journal, and translation overlays all
attach a guard. When adding a new free-scroll surface, attach a guard — do not
hand-wire the handlers. Failure checklist item #1 below becomes "confirm the
surface attaches a BottomClipGuard."

**Path (c) is TWO signals, not one.** `wire_recompute_signals` connects BOTH
`vadjustment().connect_value_changed` (a SCROLL moves the partial row) AND
`vadjustment().connect_page_size_notify` (the viewport HEIGHT changes — e.g. an
ask card re-pins the scroll height). The page_size signal is essential: a
height-only change leaves the scroll VALUE unchanged, so `value_changed` never
fires and the clip would stay stale (half-line behind the ask card). See "The fix
(DONE)" below for the full ask-card story.

## What the bottom clip CANNOT fix — occlusion is not clipping

The bottom-clip masks a **partial row straddling the viewport edge**. It does
NOT help when a fully-laid-out row is **occluded by a widget drawn on top of an
UNCHANGED viewport**. The journal Q&A "ask card" bug was exactly this: opening
the ask card did **not** shrink the scrolled viewport (proven by runtime diag:
`page_size` stays constant across the open, sync and idle) — the ask card
overflowed the fixed-height card container and overlapped the bottom of the
scroll area, so the lower text rows rendered *behind* it, fully
visible-but-occluded. There is no viewport resize to react to and no partial
edge row to mask, so NO clip recompute (path a/b/c) can fix it. **If text shows
behind a card whose opening did not change `page_size`, the bug is
layout/occlusion, not clipping — the fix is to make the overlapping widget claim
real layout space so the scroll viewport shrinks to end above it.**

### The fix (DONE): `AskCardHost` + fixed-scroll-height + page_size-notify clip

Status: **fixed** on `fix/ask-card-host`, user-verified on real hardware. Both
the journal Q&A and gloss synopsis/add-edit/echoes ask cards now shrink the
scroll viewport on open so the reading text ends ABOVE the card — and stay
correct across repeated open/close/overlay-toggle cycles (the earlier attempts
held the FIRST open then reverted). Three pieces, all required:

**1. Fixed-scroll-height (the height value).** Turn the scroll's `vexpand`
**OFF** and pin its height EXPLICITLY. Closed height = `card_height −
fixed_chrome − footer`; open height = `card_height − fixed_chrome − ask`, so the
scroll yields the ask card's slot. `AskCardHost::pin_scroll_height(h)` sets
`height_request` **and** `min_content_height` **and** `max_content_height` all to
`h` — `height_request` ALONE is only a *minimum*, so GTK was free to allocate the
scroll TALLER when the `valign=Center` container had room (the revert: first
ask-open held 817, a later one read 1025). min == max == request is a hard pin.

**2. `queue_resize()` after pinning (the relayout TRIGGER).** Setting
`*_content_height` only updates the *requested* size; without an explicit
invalidation GTK does not reliably re-run allocation, so `page_size` stayed at
the old value on a later open ("sometimes the shrink happened, sometimes not" —
a relayout race). `pin_scroll_height` calls `scrolled.queue_resize()` to force
the relayout NOW.

**3. `connect_page_size_notify` on the clip (the clip TRIGGER).** The
`BottomClipGuard` originally recomputed the clip only on `value_changed` (a
SCROLL). But opening the ask card changes the viewport HEIGHT (`page_size`) while
the scroll VALUE stays put — so `value_changed` never fired and the clip kept its
stale, taller-viewport height (the half-line poked out behind the card, and the
closed page clipped at the bottom). The guard now ALSO wires
`vadjustment().connect_page_size_notify` → recompute, so the clip is recomputed
exactly when GTK finishes the relayout to the new height — no fixed-idle race.
This is `wire_recompute_signals` in `bottom_clip_guard.rs`, used by both
`attach` and `attach_box`.

**The lesson:** when you imperatively resize a scroll viewport, you need all
three — pin the height as a hard min==max, `queue_resize()` to make the relayout
happen, and react to `page_size` (not just `value`) to recompute anything that
depends on the viewport height. A fixed `idle_add_local_once` is NOT a reliable
substitute for `page_size`-notify — it races the relayout (1ms-vs-10ms gap
decided pass/fail).

**`AskCardHost` (`src/ui/ask_card.rs`)** owns the lifecycle so neither overlay
hand-wires it: `size(card_width, card_height, fixed_chrome_h, footer_h)` records
the geometry and pins the closed scroll height; `open(title, hint)` pins the
shrunk height + hides the toggled footer + recomputes; `close()` restores the
STORED closed height (not a re-measure — the footer's `preferred_size()` reads 0
right after it is re-shown) + shows the footer + recomputes. It composes the
existing `BottomClipGuard` via a boxed recompute closure (the guard isn't
`Clone`).

- `fixed_chrome_h` = the non-scroll, non-ask chrome ABOVE the scroll that stays
  visible while the ask card is open. `footer_h` = the TOGGLED footer hidden on
  open. Journal: `fixed_chrome` = title, `footer` = the nav-hint row. Gloss
  (which now hides its hr + keybind hints when the ask card opens, like journal):
  `footer` = the hint row, `fixed_chrome` varies by show mode — synopsis/result =
  title, echoes = source header + rule, glossing-loading = title (footer already
  hidden, no ask card).
- **Precondition:** the hosted `ScrolledWindow` must be `vexpand(false)`, and
  EVERY show path that makes the scroll visible must call `ask_host.size(...)`
  (after the chrome visibility is set) or the explicit height keeps its last
  value.

### Root cause (proven) and the rejected attempts — DON'T re-try these

Keep this so the dead-ends aren't re-explored. Branch `fix/ask-card-host`;
specs/plans `docs/superpowers/{specs,plans}/2026-06-26-ask-card-host*`.

**Why it occluded.** Both overlay containers are added **non-measured**
(`attach_overlay_panel` → `set_measure_overlay(container, false)`), so the
Overlay handed them the full window height and the `vexpand` scroll filled it
(`page_size` ≈ 1025, a *minimum* that can grow). When `ask.open()` revealed the
ask card the box needed ~258px more; being `valign=Center` the extra extended
off-pane rather than shrinking the scroll. The scroll never yielded → the ask
card drew over the bottom ~258px of unchanged text. The proof diagnostic: log
`scrolled.vadjustment().page_size()` sync AND on idle in `open_ask_card`; if both
equal the closed value, the viewport did not shrink (that equality WAS the bug).

**Rejected attempts (all run on real hardware; numbers are `page_size`):**

- **(c) value_changed clip recompute / the BottomClipGuard refactor.** WRONG
  TARGET — no resize to react to, so no clip recompute helps. (The guard work is
  independently good and merged, but does NOT fix occlusion.) Pressing A produced
  NO `value_changed`; `page_size` stayed 1025.
- **Cap the scroll on open via `set_height_request(cur - ask_nat)`, vexpand left
  ON.** Shrank on first open but RACED (a later open logged idle=1025); reading
  `cur` from the live `scrolled.height()` is stale on re-open. REJECTED.
- **`set_measure_overlay(container, true)` + `min_content_height(80)`.** INVERTED:
  the CLOSED state shrank and the OPEN state stayed 1025. REJECTED.

The accepted mechanism is the **fixed-scroll-height** one described above (vexpand
OFF + explicit height, owned by `AskCardHost`). SUCCESS criterion: ask-OPEN
`page_size` SMALLER than ask-CLOSED, consistently across repeated open/close.

  **The gloss overlay has (c)** (`gloss_overlay.rs`, connected in `new()` right
  after `bottom_clip` is created, calling `recompute_overlay_bottom_clip`). A
  surface that copies only (a) and (b) but not (c) WILL clip the moment its
  viewport changes outside a named method. (Note: the journal Q&A "text behind
  the ask card" bug was NOT this — it was occlusion, fixed by the
  fixed-scroll-height host above, not by a clip path. Once the host shrinks the
  viewport, (c) keeps the clip honest for the new height — but (c) alone never
  fixed it, because without the shrink there was no partial edge row to mask.)

## Coordinate-space gotcha — `display_rows` must add `top_margin`

`iter_location` returns **buffer** coordinates (y = 0 at the first text line; the
view's `top_margin` is NOT included), but the vadjustment scrolls over
`top_margin + text + bottom_margin`, so `adj.value()` / `adj.upper()` are
`top_margin` larger. Comparing the two directly shifts every row up by
`top_margin`. Symptom — **both edges clip at once**: the bottom clip under-counts
the last partial row (pokes through under the footer) AND the top snap returns a
top `top_margin` px above the real row top (first line clips under the title).
`display_rows` therefore adds `view.top_margin()` to every row. (The main reading
card sidesteps this by using `line_yrange`, whose y already includes the offsets
— but overlays can't, because their multi-row paragraphs need per-visual-row
rects.)

## Not the same as the paged clip (do NOT "dedup")

Three clip strategies coexist deliberately; merging them changes behavior:

- **Free-scroll partial-row mask** (this doc) — overlays + scroll-mode. Masks the
  one partial wrapped row at the viewport bottom from live `display_rows` /
  `line_yrange_rows` geometry.
- **Paged clip** (`scroll.rs::update_bottom_clip`) — the MAIN reading card. Sums
  `line_yrange` heights from a known `page_top` to a column-split/section
  boundary, with `descender_guard`/`BASE_BOTTOM_MARGIN`/`exact_end`. A different
  strategy (it knows the page boundary; the free-scroll mask doesn't).
  Both its `line_yrange`-summing clips (the two-column `exact_end` branch and
  the single-column final clip) additionally subtract a descender allowance
  (`paged_bottom_clip` + `descender_allowance`: the font descent capped at the
  boundary line's blank strip, zeroed while that line carries the cursor
  highlight) so the last visible line keeps its flush descender ink — see
  failure-checklist #10.
  **Exception — the over-tall single paragraph.** `line_yrange` is per-BUFFER-line.
  When ONE prose paragraph (one buffer line) wraps TALLER than `usable_height`,
  `visible_range` fits zero buffer lines (`count == 0`) and the paged clip can't
  pick a boundary inside the paragraph — it has no per-row granularity. That case
  (only) borrows the free-scroll per-row helpers: clip below the last full VISUAL
  row via `bottom_clip_height(display_rows(view), scroll_val, usable_height, …)`
  plus the `widget_height − usable_height` reserve. This is NOT a dedup of the two
  strategies — it is the paged clip delegating its one sub-paragraph case to the
  per-row math, the same way scroll-mode already does. See the over-tall-paragraph
  entry in the failure checklist.
  **The paged clip is page_top-relative — NEVER call it on a cursor-scrolled
  view.** `update_bottom_clip` assumes the scroll is snapped to `page_top` and adds
  `scroll_offset = scroll_val − expected_y(page_top)` to the clip height (correct
  for the small offset of translation line-nav). When the view is scrolled to a
  cursor-CENTERED position far from `page_top`'s top — the inline-translation
  (`Ctrl+Alt+i`) reveal sets `adj.value` to a ¼-down-the-cursor target while
  `page_top` stays put — that offset is thousands of px, the clip balloons past the
  viewport height, and the card-colored clip covers EVERYTHING: the card goes
  BLANK until the first scroll. Continuously-scrolled views (the inline-translation
  interlinear) must use the scroll-aware `scrolloff_bottom_clip_widgets` (the
  `j`/`k` path), NOT `refresh_bottom_clip`/`update_bottom_clip`. See
  failure-checklist #7.
- **Box-slack guard** (`recompute_overlay_bottom_clip_box`) — for a Box of
  whole-widget rows; covers only trailing slack. NOT for a Box of wrapping
  TextViews (those render a partial row at the edge it can't mask). The
  translation overlay was that case and now **paginates** instead of scrolling —
  no clip at all (see "Pagination instead of a mask"). The box-slack guard remains
  for a future Box-of-whole-widgets surface.

Likewise the top-snap algorithms differ (`snap_value_to_line` per-`display_rows`
row vs scroll-mode's `snap_value_to_line_top` via `line_at_y` vs uniform
`row_step` rounding) — not duplicates. See
`docs/plans/2026-06-25-clip-prevention-design.md`.

## Margins (cosmetic, separate from clipping)

The gloss scroll overlay carries `set_margin_top(24)` + `set_margin_bottom(20)`
for breathing room below the title rule / above the footer; the snap and clip
work on top of these. They are NOT part of the clip mechanism — a surface can
clip with or without them.

## The failure checklist (ordered by frequency)

When a half line clips at the bottom edge of a scrolled surface:

1. **Missing the `value_changed` catch-all (path c).** The surface recomputes
   the clip on open and on named scroll methods, but has no
   `vadjustment().connect_value_changed(|_| recompute_overlay_bottom_clip(...))`.
   Any viewport change outside a named method (a free scroll, or something
   opening below it that shrinks the viewport — the ask card) leaves a stale
   clip. **This is the most common cause.** Fix: add path (c), mirroring the
   gloss overlay.
2. **Uniform row-step instead of per-row geometry.** The surface sized its clip
   from `line_yrange` or a fixed `step` on a multi-row prose buffer → descenders
   cut. Fix: route through `recompute_overlay_bottom_clip` / `display_rows`.
3. **`display_rows` not adding `top_margin`** → both edges clip at once
   (coordinate-space gotcha above).
4. **Recompute runs against unsettled geometry on open** (0-height viewport) and
   there is no `changed`-signal handler / idle backstop (path a) → whole body
   over-clipped until the first scroll.
5. **A new surface reserves no real layout space for an element below it
   (OCCLUSION, not clipping).** If a card opens below a `vexpand` scroll and the
   scroll keeps full height, the overflow renders *behind* the card — there is no
   partial edge row, so NO clip path (a/b/c) helps. This was the ask-card bug.
   Fix: make the scroll YIELD the space — vexpand OFF + explicit height via
   `AskCardHost` (see "occlusion is not clipping" above). The tell: opening the
   card does not change `page_size`.
6. **MAIN CARD only — an over-tall single prose paragraph renders flush to the
   bottom edge (no gap, or a half-cut last row).** The paged `update_bottom_clip`
   counts whole BUFFER lines (`line_yrange`). When one paragraph wraps taller than
   `usable_height`, `visible_range` returns `count == 0`; the old code then set the
   clip to **0**, so the paragraph filled the card flush. The tell: a prose page
   that is one continuous paragraph (the displayed range is a single buffer line,
   `last_fit == page_top`) with the last line touching the card's bottom rule. A
   FIXED-pixel reserve "fixes" the flush but cuts mid-glyph-row (checklist #2 in a
   new guise). Fix: clip at a clean visual-row boundary via
   `bottom_clip_height(display_rows(view), scroll_val, usable_height, content_h)`
   + the `widget_height − usable_height` reserve, in the `count == 0` branch of
   `update_bottom_clip`. Diagnosing this is far easier with the clip box painted a
   visible color for one run (`LIT_DEBUG_CLIP_COLOR='#ff0000'`, see "Verifying") —
   flush pages show NO clip band, over-tall-but-fitting pages show the expected
   band; that one screenshot
   separates "clip is 0" from "clip is mis-sized." Exposed by the prose
   NYTimes-column narrowing (commit on `feat/prose-nyt-column`), but it was a
   latent edge case for any single paragraph taller than the viewport.
7. **The whole card goes BLANK after a reveal/toggle that scrolls to a
   cursor-centered position (paged clip on a cursor-scrolled view).** Tell: the
   surface is blank (card background only) right after the action and the first
   `j`/`k`/scroll fixes it; the log shows a `BOTTOM_CLIP` with `clip` SEVERAL TIMES
   `widget_h` and a large `offset=` (e.g. `clip=2679 widget_h=1112 offset=2526`).
   Cause: the PAGED `update_bottom_clip`/`refresh_bottom_clip` was called while the
   scroll value is a cursor-centered target NOT equal to `page_top`'s top, so its
   `scroll_offset = scroll_val − expected_y(page_top)` is huge and inflates the
   clip past the viewport, covering everything. This is NOT path-(a) unsettled
   geometry (#4) — the geometry is settled; the clip STRATEGY is wrong for the
   view. Fix: on a continuously-scrolled view use the scroll-aware
   `scrolloff_bottom_clip_widgets` (+ a 100ms scroll-aware backstop for the
   post-`reapply_font` relayout) and do NOT call `refresh_bottom_clip`. The inline
   translation (`Ctrl+Alt+i` → `ToggleTranslations`) `show_translations` reveal hit
   exactly this — its idle already used the scroll-aware clip but a trailing
   `refresh_bottom_clip(state)` (paged) clobbered it. See "The paged clip is
   page_top-relative" above.
8. **A Box-child overlay whose children are wrapping TextViews cuts the bottom
   row.** Tell: the surface scrolls a `gtk4::Box` (so it attached the box-slack
   guard), the Box stacks TextViews (e.g. paired translation columns), and the
   bottom row is sliced through its glyphs once content overflows. Cause: the
   box-slack guard (`recompute_overlay_bottom_clip_box`) clips 0 on overflow
   because it assumes whole-widget rows — but a TextView wraps and renders a
   partial row at the edge. A per-row mask across multiple independently-wrapping
   TextViews was tried for the translation overlay and proved fragile
   (coordinate-mapping bugs, an un-snapped top row, the highlight off-screen on
   open). **The durable fix is to paginate, not mask** — render only the whole
   units that fit so no partial row exists (see "Pagination instead of a mask").
   The translation overlay now does this.
9. **The HIGHLIGHT band cuts the highlighted line's descenders (not a viewport
   clip).** Tell: the cursor-line `paragraph_background` band's bottom edge
   slices the descenders of the *highlighted* line, mid-page, with room to spare —
   not the page's last row. Cause: the surface's TextView set no
   `pixels_below_lines`, so the band ends flush at the line's logical bottom.
   Fix: set `pixels_above_lines`/`pixels_below_lines = config.line_spacing` on the
   TextViews (matching the main card); if the surface paginates from measured
   heights, add the spacing to the measurement too. See "A different clip: the
   HIGHLIGHT band cutting descenders" above. This is NOT a bottom-clip-box bug —
   no clip path (a/b/c) is involved.
10. **MAIN CARD — the LAST line of a page/column has its descenders (g/y/p/comma
    tails) sliced by the card-colored bottom clip.** Tell: the bottom line of a
    column (two-column verse worst — `pixels_above/below_lines` are 0 there so
    ink runs flush to the logical line bottom) is cut through its descenders,
    while a ~40px+ reserve still sits below the clip. Cause: both
    `line_yrange`-summing clips in `update_bottom_clip` (the two-column
    `exact_end` branch and the single-column final clip) put the clip's top edge
    at `Σ line_yrange` = the last line's LOGICAL bottom, and descender ink
    renders flush to (or 1px past) that bottom. Fixed (2026-07-04) by dropping
    the top edge by a **descender allowance** (`paged_bottom_clip` +
    `descender_allowance` in `src/input/scroll.rs`): the font descent capped at
    the **boundary line's guaranteed-blank strip** (`boundary_blank_budget` =
    its `pixels_above_lines` + the font's ascent internal leading scaled by the
    line's smallest tag scale). The cap matters because the shared buffer
    renders the NEXT line immediately below the clip's top edge, merely hidden:
    - an uncapped reveal exposes the next line's ascender tops (a base-font
      probe alone over-revealed a 0.75-scale speaker label by 1px — measure the
      BOUNDARY line, not the base font);
    - a whitespace-only boundary line caps at its own short (0.25-scale) box
      height, or the reveal punches through it into the line after;
    - while the boundary line carries the CURSOR highlight, its
      `paragraph_background` band paints from the box TOP, so the allowance
      collapses to 0 — `update_highlight` re-schedules the affected column's
      clip when the cursor crosses the stored `left_clip_boundary` /
      `right_clip_boundary` (for the left column the boundary is the right
      column's FIRST line, on-page and routinely highlighted).
    Diagnose with `LIT_DEBUG_CLIP_COLOR='#ff0000'` (below) — the band's top
    edge visibly crosses the descenders. NOT the free-scroll partial-row mask
    and NOT the highlight band (#9): it hits the LAST line of a column
    regardless of the cursor. Do NOT add such an allowance to the visual-row
    paths (overlays, over-tall branch, scroll-mode): wrapped rows tile with
    zero inter-row spacing, so there is no blank budget — any reveal there
    exposes the next row's ink.

11. **TOP edge, right after a WORK SWITCH — first line of the (left) column
    half-cut, and in two-column mode the left column repeats the right
    column's opening lines.** Not a clip-box bug at all: a stale
    `page_top_offset` (the sub-line pixel offset from the PREVIOUS work's
    prose row-fill grid, e.g. 833px set by a `PAGES_PROSE: resnap off-grid`)
    survived the switch, and the resize-tick snap
    (`snap_scroll_to_line_offset(page_top, offset)`) scrolled the view
    `offset` px past the new work's page top — `offset > 0` skips the
    whole-line alignment on purpose. The right view is scrolled separately to
    `cs.split`, hence the overlap tell. Fixed by resetting
    `state.page_top_offset = 0` in `display_work_at_with_prepared` (all its
    position-restore paths set `page_top_line` only); prose targets get their
    offset re-derived by `resnap_prose_to_table`. If it recurs, check for a
    new path that sets `page_top_line` without the offset.

12. **MAIN CARD, two-column play — the LAST line of the LEFT column is half-cut
    with NO clip band below it, and the RIGHT column is nearly empty. A STALE
    `play_pages` TABLE split, not a clip-math bug.** Tell: `linux-lit-dev.log`
    shows `PAGES: table hit` for this work AND a
    `BOTTOM_CLIP_EXACT: … total=T clip=0 page_top=P end=E` whose `total >
    widget_h` (the left column [P, E-1] sums TALLER than the viewport — e.g.
    `total=1148 widget_h=1112 clip=0`). The clip math is correct — `paged_bottom_clip`
    can't clip a negative, so `clip=0` and the overflowing last line pokes out
    with nothing to mask it. Reproduces on STARTUP onto that spread and via any
    nav that lands on it (journal-close→`{`, `x`/`y`, `G`); the journal/scene
    step is NOT the cause, just how you first land there. **Root cause:** the
    pinned table stored a left-column `split` that fit at the geometry it was
    GENERATED at, but the current reader **card** height is smaller, so the split
    now overflows. The layout fingerprint
    (`v1|family|size|ascent|descent|charw|WxH|spacing|margins|cols`) keys on the
    **toplevel WINDOW** size (`state.window.width()/height()`), NOT the card's
    `text_view.height()` — so a change that shrinks the CARD without moving any
    fingerprint input (a reader-clip/margin/spacing tweak whose effect on the
    card isn't a fingerprint field, or a table generated before the card settled
    to its final height) leaves the stale table a valid `table hit`. Confirm by
    comparing the stored table's `end` against a LIVE `column_split` at the real
    card height: a fresh `column_split` fits (`total < usable`, `clip > 0`) while
    the stored `end` is one line too far. **Fix: regenerate the table** (the data
    IS the bug — do NOT patch the reader). Close nothing; deleting the rows is
    safe (`DELETE FROM play_pages WHERE work_abbrev='<ABBR>'; DELETE FROM
    play_pages_meta WHERE work_abbrev='<ABBR>';`) and the app regenerates at the
    true current geometry on next load — `record_spreads`→`column_split` respects
    `usable = widget_h − guard − BASE_BOTTOM_MARGIN` and `validate_spreads` fit-
    checks each left column, so a fresh table cannot overflow. `validate-play-pages`
    reports **PASS** for this bug (it checks structure — overlaps/gaps/ordering —
    not fit against the CURRENT card), so a PASS does not rule it out; the
    `total > widget_h` log line is the decisive tell. **Latent recurrence:** the
    fingerprint not distinguishing card height from window height means this can
    reappear for any play whose table predates a card-height change under an
    unchanged fingerprint — if it recurs across many works at once, the durable
    fix is to fold the card's `text_view.height()` (or the `usable` it derives)
    into `layout_fingerprint` so a card-height change invalidates stale tables
    automatically. (`Cym-Arkangel`, 2026-07-13: table generated 2026-07-04 with
    `end=3941`/`total=1148`; regenerated to `end=3937`/`total=1034`/`clip=73` at
    the same `1920x1200` fingerprint.)

13. **A PAGINATED overlay (translation overlay) squeezes a whole page into a thin
    band at the top on a page turn — the first line half-cut, the rest of the card
    blank, and INTERMITTENT ("sometimes clips, sometimes not").** NOT a viewport
    clip, a highlight-band clip, or a stale table — a LAYOUT/ALLOCATION race.
    `render_page` swaps fresh `TextView`s into the already-mapped `content_vbox`;
    a fresh TextView reports a COLLAPSED natural height (0px / one row) until its
    pango layout runs on a later pass, so GTK can allocate the new blocks at that
    collapsed height and paint before layout settles. Tell (from an idle-deferred
    geometry log): the block `TextView`s allocate `h=0`/`h=26` and the
    `content_vbox` height is a small varying value (40, 62, 186…) while the pinned
    `scrolled` stays at its full `min==max content_height` — i.e. the SCROLL is
    pinned but the CONTENT collapsed. Reproduces most on short pages and pages
    whose first block is a stage-direction interlude, and via any re-render path
    (`x`/`y` page turn, `[`/`{` scene jump, cursor sync). `queue_resize()` does
    NOT reliably fix it — the natural-height measurement itself hasn't run.
    **Fix: pin each column view's `height_request` to its measured text height**
    (`set_view_height` = `measure_text_height(...) + line_spacing * paragraphs`,
    the same synchronous pango measurement pagination already uses), so a block
    can never collapse regardless of GTK's lazy-measure timing. See "Pin every
    rendered TextView's height_request" under "Pagination instead of a mask."
    (`translation_overlay.rs`, 2026-07-14.)

14. **A PAGINATED overlay's BOTTOM-MOST line has its descenders sliced, even
    though the page ends well ABOVE the card's bottom edge (whitespace below).**
    NOT a page-fill/bottom-cap clip (#12) and NOT the collapse race (#13): the
    last line is cut at its OWN pinned view boundary mid-card. Cause: each column
    view is pinned to `set_height_request(measure_text_height(...) + line_spacing
    * paras)`, and `measure_text_height` returns pango's LOGICAL height
    (`Layout::pixel_size().1`, rounded down at the baseline+descent boundary) while
    a GTK TextView paints the final line's descender ink to the font's full
    descent — 1–2px past that logical bottom. With `line_spacing > 0` the below-
    line spacing absorbs it, but **verse plays pass `line_spacing == 0`** (verse
    renders tight, mirroring the reading card), so the pin equals `text_h` exactly
    and the last line's `y`/`g`/`p`/comma tails clip. Tell: the OVERLAY's last line
    (e.g. "By your leave." in the 2-col translation overlay) shows cut descenders
    with clear whitespace beneath the block. Fix: add a `descender_pad(body_font_
    size)` (≈15% of the point, floored 3px) to the pinned view height AND to the
    pagination height accounting (`block_height` + the split-budget seed) so pages
    don't over-pack. Diagnose with `LIT_DEBUG_CLIP_COLOR` for the page-edge clips,
    but this one is font-metric rounding — the pixel e2e / real display is the
    proof. (`translation_overlay.rs`, 2026-07-14.)

## The CLIP_WARN tripwire (grep this FIRST)

A debug-gated, on-by-default detector logs `CLIP_WARN` when a surface's clip
math diverges into a clip-class failure — so a regression shows up in the log,
not only by eye. **When any clip bug is reported, `rg CLIP_WARN linux-lit-dev.log`
before anything else.** It is a pure detector (mutates nothing) and is silent in
normal operation (verified: an 89-step nav-fuzz over a two-column play fired the
clip path 104× with zero warnings). It does NOT replace the pixel e2e tests —
it flags geometry divergence, not cut glyphs — but it points straight at the
surface and the checklist item. Three sites, all gated on `logging::debug_mode()`:

- **Translation overlay** (`translation_overlay.rs`, `render_page`): idle check
  comparing each block's ALLOCATED vs MEASURED height → `CLIP_WARN: translation …
  COLLAPSED …` (#13) or `… OVERFLOW …` (bottom clip).
- **Overlays — gloss/journal/synopsis** (`ui/mod.rs`,
  `recompute_overlay_bottom_clip`, the shared path all three funnel through):
  `CLIP_WARN: overlay clip_h=… >= viewport_h=…` when the clip would blank the
  surface (#7).
- **Main reading card** (`input/scroll.rs`, `update_bottom_clip`):
  `CLIP_WARN: main-card two-col OVERFLOW total=… > widget_h=…` (#12) and
  `CLIP_WARN: main-card single-col clip=… >= widget_h=…` (#7).

## Verifying

Real GTK pixel layout is what matters; the headless `cage` + `grim` flow lays
out fonts/metrics differently and can confirm the mechanism RUNS and roughly
looks right but cannot prove pixel-exact edges. Confirm on the real display:
open the surface, scroll to the bottom, and check the bottom edge shows only a
whole line. The pixel-level e2e invariants are `tests/line_clipping.rs` (main
card) and `tests/overlay_clipping.rs` (synopsis overlay), both `#[ignore]`d and
run via `./scripts/e2e-env.sh cargo test --test line_clipping --test
overlay_clipping -- --ignored --nocapture`.

**Seeing the clip box:** launch with `LIT_DEBUG_CLIP_COLOR='#ff0000'` (any CSS
color) to paint every bottom-clip box — the main card's `.card-bottom` AND the
overlays' `.gloss-bottom-clip` — that color for the run. This replaces the old
"hand-edit `.card-bottom` to `#ff0000` in theme.rs, rebuild, revert" dance; the
knob is a no-op when unset. The `exact_end` branch also logs
`BOTTOM_CLIP_EXACT: widget_h=.. total=.. allowance=.. clip=..` (it used to be
the only silent clip path).

**Detector calibration (scripts/check_line_clipping.py):** two lessons from the
descender-allowance work. (1) Row segmentation merges runs separated by ≤2px —
a descender tip tapers so thin that its connecting rows fall under the 1%-width
ink threshold, and the detached tip read as a fake 1px "clipped row" the moment
the clip stopped covering real descender ink. (2) A short EDGE row counts as
clipped only if it is also shorter than every interior row — a complete
0.75-scale speaker label at the page top is legitimately under the body-text
median. Both were detector false positives that only surfaced once production
rendered MORE of the glyphs, i.e. "when a clip e2e fails, first ask whether the
assertion is measuring the pre-fix rendering."

## Key files

- `src/ui/mod.rs` — `display_rows`, `bottom_clip_height`,
  `recompute_overlay_bottom_clip`, `line_yrange_rows`,
  `recompute_overlay_bottom_clip_box` (the shared free-scroll helpers).
- `src/ui/gloss_overlay.rs` — the reference surface: `reset_scroll_top` (path a),
  `scroll_gloss`/`snap_value_to_line` (top snap + path b), the `value_changed`
  handler (path c), `update_bottom_clip` (one-line call to the shared helper).
- `src/ui/journal_overlay.rs` — the journal Q&A overlay (mirrors all three clip
  paths; routes the ask card through `AskCardHost`).
- `src/ui/ask_card.rs` — `AskCard` (the shared input widget) + `AskCardHost` (the
  fixed-scroll-height ask-card lifecycle: the occlusion fix). Used by both the
  journal and gloss overlays.
- `src/ui/translation_overlay.rs` — the Box-child variant.
- `src/input/scroll.rs` — `update_bottom_clip` (the MAIN card's *paginated*
  clip, NOT this algorithm — except its `count == 0` over-tall-paragraph branch,
  which delegates to the shared `display_rows`/`bottom_clip_height` per-row math),
  `scrolloff_bottom_clip_widgets` (scroll-mode, routed through the shared helper),
  `snap_value_to_line_top`.
- `src/theme.rs` — the `.gloss-bottom-clip` background CSS.
- `docs/troubleshooting/page-turning-mechanics.md` — the paged clip + pagination.
- `docs/plans/2026-06-25-clip-prevention-design.md` — the unification
  design.
