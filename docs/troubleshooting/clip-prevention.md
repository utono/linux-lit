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
> pinned `play_pages` problem, not a clip-math bug — but do NOT reach for
> "regenerate the table" first. Two different causes share that tell: the page
> TOP being off-grid (common — the table is fine and regenerating changes
> nothing), or the stored split genuinely not fitting (stale table). Checklist
> #12 tells them apart in one query.

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
`docs/superpowers/specs/2026-06-27-paginated-translation-overlay-design.md`.

**The chat panel (`src/ui/chat_panel.rs`) now paginates the same way.** It used
to free-scroll a `Box` of wrapping `Label`s behind a `BottomClipGuard::attach_box`
mask plus a hand-rolled top row-snap in `render_rows_focused_cursor` — a long
series of partial-row clips at BOTH edges. The swap: `row_widget_specs` expands
the transcript into one `ChatWidget` per label; `chat_pagination::widget_heights`
+ `pagination::paginate_grouped` pack whole widgets into pages at
`ChatPanel::transcript_budget()`; `render_page` rebuilds `transcript_box` from
ONLY `specs[page.start..page.end]` and never touches the vadjustment (the page
fits by construction). The `clip_guard` field, the `Overlay`/`attach_box` wrap,
`on_open`, and the `.chat-panel .gloss-bottom-clip` CSS override were all deleted.
`ChatState.pages`/`page_idx` hold the current pagination.

- **The `chat-a-src-lead` height gap (mis-measure → bottom clip or underfill).**
  The first source row after a gloss carries a SECOND CSS class `chat-a-src-lead`
  (a **26px** top gap — chosen to EQUAL `.chat-a-gloss`'s own 26px padding-top so
  the gloss↔source rhythm is symmetric: gloss → gap → source → gap → gloss). It
  is applied via a COMPOUND selector
  `.chat-transcript label.chat-a-src-lead { padding-top: 26px }` (specificity
  0,0,2,1) so it out-specificities EVERY single-class base source rule
  (`.chat-a-speaker`/`.chat-a-verse`/`.chat-a-stage`/`.chat-a-*-flush`, all
  0,0,1,0) REGARDLESS of stylesheet order — the src-lead row's rendered
  padding-top is 26 for every source class. (This replaced the old
  source-order collision, where only `.chat-a-speaker` — ordered before src-lead
  — won and the rest got a 0 gap; the visible symptom was NO extra gap before a
  prose/`verse-flush` source block.) padding-top is NON-additive (26 REPLACES the
  base), and `class_pad(primary)` already added the base padding-top, so
  `chat_pagination::src_lead_extra_pad` returns `26 − base padding-top` (≥0) per
  class: speaker→12, verse/verse-flush→26, stage/stage-flush→18. Do NOT use
  `class_pad("chat-a-src-lead")` (that double-counts the base pad).
  - **SYNC WARNING:** THREE things must stay in lockstep or the src-lead row is
    mis-measured (undercount → bottom clip; overcount → underfill): (1) the CSS
    selector stays COMPOUND (`.chat-transcript label.chat-a-src-lead`) so it wins
    for all source classes; (2) `SRC_LEAD_PADDING_TOP` (`chat_pagination.rs`)
    equals `.chat-a-src-lead { padding-top }` (theme.rs); (3) the per-class base
    padding-top values `src_lead_extra_pad` subtracts match theme.rs.

- **The content-box `padding-bottom` undercount (page packs one row too many →
  bottom clip).** The pagination budget is `transcript_scroll.height()`, but the
  `.chat-transcript` content box has its OWN `padding-bottom: 16px` (theme.rs) —
  height the widgets inside `transcript_box` can never use. If the budget doesn't
  subtract it, pagination packs one row's overflow past the bottom and the last
  line clips. Fix: `transcript_budget()` subtracts `CHAT_TRANSCRIPT_PAD_V`
  (mirrors `.chat-transcript { padding-bottom }`) plus a 2px `CHAT_BUDGET_SAFETY`
  that absorbs pango logical-vs-ink rounding so a page never packs to a hairline
  overflow. Both consts live at the top of `chat_panel.rs` beside
  `transcript_budget`; `CHAT_TRANSCRIPT_PAD_V` must track the CSS padding-bottom.

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

## Coordinate-space gotcha — SOURCE vs DISPLAY char offsets (italics)

Same class of bug as `top_margin` above, in the CHARACTER axis instead of the
pixel axis. Two char coordinate spaces coexist on any
prose/prose_book/epic_translation line carrying inline `_word_` markup:

- **SOURCE** — offsets into `line_mapping.canonical_text`, underscores present.
  Everything the DB stores (`phrase_timestamps.start_char/end_char`) is here.
- **DISPLAY** — offsets into the buffer line after `strip_italics_for_fill`
  removed the paired `_`. Everything read back off the buffer is here.

`apply_char_range_tag` converts SOURCE → DISPLAY via
`italics::translate_offset(italic_offset_map[bl], off)` before setting iters,
so the rendered tint is right. Any OTHER consumer that slices buffer text with
a DB offset must translate too.

Symptom — a span shifted left by exactly the number of `_` earlier in the line,
mangling both ends: `and how can it be otherwise,` reads as
`d how can it be otherwise, w`. Diagnosed 2026-07-25 (TT line 1761145). The
offending consumer was the `KARAOKE:` trace, which sliced the stripped
`line_text` with untranslated `sc`/`ec` — so the LOG lied while the screen was
correct. That inverted the usual debugging assumption and briefly looked like a
litdb grouping defect; the DB span was clean the whole time.

Rule: when a logged/derived span disagrees with what is on screen, check which
coordinate space each side is in BEFORE suspecting the data. Guarded by
`phrase_highlight::tests::trace_slice_translates_italic_offsets`.

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
`docs/superpowers/specs/2026-06-25-clip-prevention-design.md`.

## Margins (cosmetic, separate from clipping)

The gloss scroll overlay carries `set_margin_top(24)` + `set_margin_bottom(20)`
for breathing room below the title rule / above the footer; the snap and clip
work on top of these. They are NOT part of the clip mechanism — a surface can
clip with or without them.

## Inherited leading (cosmetic, separate from clipping)

**Tell:** verse inside a PROSE work renders with a paragraph-sized gap between
every line — the lines look double-spaced, and fewer of them fit per page — while
the same verse in a play/poem work looks correct.

**Root cause:** `display_work` (`src/app/mod.rs`) sets the VIEW-level
`pixels_above_lines`/`pixels_below_lines` from `config.line_spacing` when
`is_prose_work(work_type)` is true, and to 0 otherwise. That gate is per-WORK,
but `block_type` is per-LINE: a `prose_book` such as LoJ (Boswell quoting
Virgil's first eclogue) holds 2,067 `verse` rows, and each inherited
`2 * line_spacing` of prose leading. `apply_block_typography` tagged those rows
with `verse-indent-{tier}` (left_margin) and a per-STANZA 12px `verse-stanza-gap`,
but nothing cancelled the view-level leading, so the gap appeared between every
line rather than between stanzas.

**Fix (2026-07-26):** a `verse-tight` TextTag (`pixels_above_lines(0)` +
`pixels_below_lines(0)`) applied to every non-empty verse row in
`apply_block_typography`. A TextTag overrides the view default, so verse sets
tight while the surrounding prose keeps its configured leading.

- **Where Markdown appears is a CORPUS question, not a code question
  (2026-07-29).** Before building a renderer, count what the data actually
  contains. Every LLM answer prompt in `api_prompts` mandates "Flowing prose
  only — no markdown, no bullet or numbered lists, no headers", and journal Q&A
  matches that exactly: 0 of 56 entries carry `##` or bullets, so block
  rendering there would have been risk for content that does not exist.
  Vocab-word GLOSSES are the exception — 25 use `### Etymology`, 61 use `>`
  quote lines, 25 use `---` rules, and 62 use `**headword**` — because they are
  hand-authored rather than prompt-governed. Same feature request, opposite
  answer per surface; a `sqlite3 … LIKE '%### %'` count settles it in seconds.
- **Not every surface is a TextBuffer — check before reaching for TextTags
  (2026-07-29).** The reader renders prose through THREE different mechanisms,
  and a styling fix has to match the one in play: the journal overlay does a
  flat `set_text` (tags over char ranges); the gloss overlay/synopsis insert
  piecewise through `populate_verse_buffer` (tags per element, each with its own
  local offset base); the chat transcript and the **vocab popup** are
  `gtk4::Label`s, which take **Pango markup** (`<b>`/`<i>`) and cannot use tags
  at all. The 62 `**headword**` gloss rows surface in the vocab POPUP, not the
  gloss overlay — wiring only the overlay would have looked correct in code and
  changed nothing on screen. Confirm which widget actually renders the content
  before choosing the mechanism.
- **A buffer-wide font tag OUTRANKS every style tag under it (2026-07-29).**
  *Tell:* a style tag is applied over a correct range and the text renders
  unstyled — no error, no missing tag, the markers/offsets all correct. In the
  journal overlay, `*italic*` spans stripped and tagged with `md-italic`
  rendered fully upright. *Root cause:* `apply_font_to_views`
  (`src/ui/mod.rs`) REMOVES and re-adds a `journal-font` tag across the whole
  buffer on every render, so it lands at the top of the tag table and its
  upright font wins over any lower tag's `Style::Italic`. This is why
  `reassert_italic_tags` exists — but it only re-raises `gloss-stage` and
  `gloss-bracket`, so any newly-added style tag hits the same wall. *Fix:*
  re-raise the tag with `set_priority(table.size() - 1)` AFTER `apply_font`
  runs. Give competing tags DISTINCT priorities (`- 1`, `- 2`): GTK keeps
  priorities unique and setting one shuffles the others.
- **Tag-table insertion order IS priority.** GTK resolves competing values by
  tag priority, which defaults to insertion order (later-added wins) — the same
  rule behind the vocab-tag ordering bug. `verse-tight` is created BEFORE
  `verse-stanza-gap` in `ensure_block_typography_tags` so a stanza-opening line,
  which carries both, still gets its 12px gap. Reversing the two would silently
  flatten every stanza break.
- **Empty verse rows keep the gap tag only** — they ARE the stanza separator, so
  tightening them would erase the break.
- **A per-work gate cannot express per-line typography.** Whenever a work type
  mixes block types, the class-wide default has to be cancellable per line;
  reach for a tag rather than widening the `is_prose_work` branch.
- **The stanza-gap guard was ALSO wrong, and hid behind the first fix.** The
  `verse-tight` tag alone moved the pitch 44px → 38px, which looked like
  progress but was only half the story: `verse-stanza-gap` (12px) was gated on
  `prev_src != Some(wi)` — "this buffer line starts a new SOURCE line" — which
  is true for EVERY line of a work whose verse rows are 1:1 with buffer lines.
  Instrumenting the two tag counts proved it: **2067 verse-tight, 2067
  stanza-gap** on a work with 2,067 verse rows and ZERO empty separator rows.
  The guard is now `!prev_was_verse` (did the previous buffer line render as
  non-empty verse?), which yields 413 real stanza openings and a 26px pitch.
- **Anchor "correct" to a reference surface, not to a delta.** The target was
  "render like Shakespeare verse in the two-column layout"; measuring Hamlet's
  right column gave a hard number (26px) that turned a subjective "still looks
  loose" into arithmetic: 26 base + 12 stanza gap = the observed 38. Without
  that reference the half-fix would have shipped.
- **Measure the pitch, don't eyeball it.** Pixel-measured ink-band pitch on the
  same page at 1920x1200: 44px (broken) → 38px (verse-tight only) → 26px (both
  fixes), matching Hamlet's two-column verse exactly. Glyph ink heights are
  unchanged throughout (~15-20px), which is what proves the delta is spacing
  rather than font size.
- **`pixels_above_lines` can only OPEN a block.** A third pass was needed: with
  the stanza gap correct, a verse block still closed with no gap below its last
  line (measured 43px above the block vs 32px below), because the gap tag sets
  `pixels_above` and nothing was tagging the closing edge — the following prose
  line contributed only its own leading. Fixed with a `verse-stanza-gap-below`
  tag (`pixels_below_lines(12)`) applied when the NEXT buffer line is not
  non-empty verse, which needs a lookahead — tracking only the previous line
  cannot see a block's last row. Now 44px on both edges.
- **Blast radius is checkable in one query.** `block_type='verse'` exists on
  exactly two works (LoJ, TT), both prose — plays and poems never enter
  `apply_block_typography`, so their verse typography is untouched.

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
2b. **A straddling line charged its FULL height (prose row-fill).** A prose page
   can start AND end mid-paragraph, and each straddling line is only partly on
   the card. Charging the full height breaks BOTH edges, with two different
   tells:
   - overstating the TOP (`top_off` ignored) makes the clip too SHORT → the page
     OVER-RENDERS past its stored end. Tell: content that belongs to the next
     page is visible, classically a chapter heading trailing the previous
     chapter (BH-Barrett page 112 at `(924,188)` revealed 188px = "CHAPTER X" +
     its subtitle).
   - overstating the BOTTOM (`bottom_head` ignored) makes the page read as
     OVERFULL → `paged_bottom_clip` floors to 0 and the last line is sliced with
     nothing masking it, plus a spurious `CLIP_WARN … OVERFLOW` (page 113 at
     `(931,0)..(935,317)` measured 1175 vs 1098; true height 1065).
   Fix: `exact_page_content_height` charges the first line `h - top_off` and the
   last line `min(bottom_head, h)`. Both are 0/None for two-column/play pages,
   so those are unaffected. Mirrors `viewport::is_line_start_visible` (top) and
   `prose_pages::page_px` (both edges) — when the render-side measurement and
   `page_px` disagree, the render side is wrong.
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
    with NO clip band below it, and the RIGHT column is nearly empty. A
    `play_pages` TABLE problem, not a clip-math bug.** Tell: `linux-lit-dev.log`
    shows `PAGES: table hit` for this work AND a
    `BOTTOM_CLIP_EXACT: … total=T clip=0 page_top=P end=E` whose `total >
    widget_h` (the left column [P, E-1] sums TALLER than the viewport — e.g.
    `total=1148 widget_h=1112 clip=0`). The clip math is correct — `paged_bottom_clip`
    can't clip a negative, so `clip=0` and the overflowing last line pokes out
    with nothing to mask it. Reproduces on STARTUP onto that spread and via any
    nav that lands on it (journal-close→`{`, `x`/`y`, `G`); the journal/scene
    step is NOT the cause, just how you first land there.

    **There are TWO distinct root causes with this same tell.** Establish which
    one you have BEFORE doing anything — they need opposite fixes, and the
    off-grid one (A) is the more common:

    - **(A) The page TOP is off-grid** — the table is fine. `page_top` is not a
      stored `left_start`, so the render path mixes a table top with a live
      `end`. See "THE USUAL CAUSE", immediately below.
    - **(B) The stored SPLIT genuinely doesn't fit** — the table is stale for
      the current card height. See "Root cause (B)" and "FINGERPRINT FIX"
      further down.

    Decide with one query: is the overflowing `page_top` a `left_start` in
    `play_pages` for the ACTIVE fingerprint? If NO ⇒ (A). If YES ⇒ (B).

    **THE USUAL CAUSE IS NOT STALENESS — CHECK THIS FIRST (2026-07-27).** If
    the log shows `PAGES: table hit` AND the overflowing `BOTTOM_CLIP_EXACT`
    line's `page_top` is NOT a stored `left_start`, the table is fine and the
    TOP is wrong. `spread_for_top` is an exact-top match, so a `page_top` the
    table doesn't start a spread at used to fall straight through to the live
    `column_split` — pairing a table-chosen top with a live-chosen `end` and
    rendering a column WIDER than either engine would choose alone. Observed on
    Ant-Arkangel startup: `STARTUP: snap near-end` chose 5340 (from
    `last_page_top`, which walked the LIVE page chain), the table had no spread
    starting there, and the live split supplied `end=5381` ⇒ 1102px into a
    1098px viewport. Two fixes, both needed: `last_page_top` returns the
    TABLE's last spread when a table is active, and the render path falls back
    to `page_for_line` (containment) before the live engine. Diagnostic: log
    `effective_top` alongside `spread_for_top(...)` — a `None` there with a
    live table is the tell. Regression:
    `page_table::tests::a_top_inside_a_spread_resolves_to_that_spread`.

    **Root cause (B), the stale-table route:** the pinned table stored a
    left-column `split` that fit at the geometry it was
    GENERATED at, but the current reader **card** height is smaller, so the split
    now overflows. Historically the layout fingerprint keyed on the **toplevel
    WINDOW** size (`state.window.width()/height()`), NOT the card's
    `text_view.height()` — so a change that shrank the CARD without moving any
    fingerprint input (a reader-clip/margin/spacing tweak whose effect on the
    card isn't a fingerprint field, or a table generated before the card settled
    to its final height) left the stale table a valid `table hit`. **As of `v5`
    the view height IS fingerprinted** (see "FINGERPRINT FIX" below), so this
    root cause is closed for new tables. Confirm by
    comparing the stored table's `end` against a LIVE `column_split` at the real
    card height: a fresh `column_split` fits (`total < usable`, `clip > 0`) while
    the stored `end` is one line too far. **Fix for (B): FIRST check whether a
    derived layout input is missing from the fingerprint** (that is the real bug
    — a table that can go stale while still matching); regenerating by hand only
    clears the symptom for one work and it will come back. To clear it for one
    work, deleting the rows is safe (`DELETE FROM play_pages WHERE
    work_abbrev='<ABBR>'; DELETE FROM play_pages_meta WHERE
    work_abbrev='<ABBR>';`) and the app regenerates at the
    true current geometry on next load — `record_spreads`→`column_split` respects
    `usable = widget_h − guard − BASE_BOTTOM_MARGIN` and `validate_spreads` fit-
    checks each left column.

    **A fresh table is NOT proof the bug is gone.** An earlier version of this
    entry said a regenerated table "cannot overflow" — false. Regeneration only
    rules out (B): a table generated at the correct geometry, `validated=1`,
    still rendered the clipped line when the TOP was off-grid (A). If you
    regenerate and the clip survives, you have (A); stop deleting rows and go
    read the paragraph below.

    `validate-play-pages` reports **PASS** for BOTH causes (it checks structure —
    overlaps/gaps/ordering — not fit against the CURRENT card, and not the top
    the renderer will use), so a PASS rules out neither; the
    `total > widget_h` log line is the decisive tell. (`Cym-Arkangel`,
    2026-07-13: table generated 2026-07-04 with `end=3941`/`total=1148`;
    regenerated to `end=3937`/`total=1034`/`clip=73` at the same `1920x1200`
    fingerprint.)

    **FINGERPRINT FIX LANDED 2026-07-27 (`v5`).** Separately, the recurrence this
    entry predicted happened (`Ant-Arkangel` page 89: a 1102px left column in a
    1071px usable height, cutting "A simple countryman that brought her figs.";
    `validate-play-pages` reported PASS, as documented above). The card's
    `text_view.height()` is now a `layout_fingerprint` field
    (`FingerprintParts::view_height`), so a table baked at a different CARD
    height no longer matches and is regenerated automatically — no manual
    DELETE. NOTE: this closes the STALE-TABLE route to the overflow only; it
    does NOT make the `PASS`-but-overflowing state unreachable, as first
    believed. A v5 table regenerated at the correct geometry still rendered the
    clipped line, because the top itself was off-grid (see the paragraph
    above) — which is the more common cause. The version
    bump `v4`→`v5` invalidated all ~200 stored tables at once; they regenerate
    on next load of each work. Regression test:
    `page_table::tests::view_height_change_invalidates_the_fingerprint` (same
    window size, different view height ⇒ different fingerprint).

    **If this signature EVER reappears, run the A/B query FIRST** (is the
    overflowing `page_top` a stored `left_start`?). If NO, a path is setting a
    page top off the table grid — find it; do not touch the fingerprint. Only
    if YES is it a fingerprint gap: something changed the card height without
    moving a fingerprint field, so add that input rather than regenerating by
    hand.

12b. **TRANSLATION OVERLAY — the RIGHT column's lines are cut mid-word at the
    card's right edge ("comfo…", "Bol…", "your positio…").** A real horizontal
    clip, and the only overlay that can have one: the translation overlay is the
    sole TWO-column overlay, but it took the shared `overlay_card_size` width,
    which is measured for ONE text column. Halving it gave each column ~469px —
    enough for the ~63-char verse the LEFT column is sized around, but the
    modernized English on the right runs longer.
    - Tell: the cut lines all end at the SAME x, and it is not a natural line
      end. Measure the rightmost ink per row — a hard shared boundary means a
      width clip, not a font/rasterisation artifact (contrast #19/#19b, where
      the glyphs are whole and merely thin).
    - Fix: `TRANSLATION_CARD_MIN_W` (`translation_overlay.rs`), applied in
      `translations.rs` as `overlay_card_size().max(MIN_W).min(window_width)`.
      NOT a change to `overlay_card_size` itself — nine other call sites share
      it, and overlays are deliberately 1-col width.
    - **Measure px/char in the COLUMN THAT CLIPS.** A first pass derived
      ~7.4px/char from the verse column and picked 1164, which STILL cut 56-58
      char translations. Sampling rendered translation lines gives ~9.6px/char:
      modernized prose is wider per character than Shakespeare's verse. p99 of
      `line_translations` is 64 chars (p99.9 = 128; the longest single row is
      436, not worth sizing for), so 64 x 9.6 ~= 614px/column -> a 1366px card.
      Verified: longest line now clears the card edge by 116px.
    - Related history: the side margins were already tightened from `/12` to
      `/24` for this same clipping, which bought room but did not fix it — the
      card itself was too narrow. (2026-07-31.)

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

15. **MAIN CARD, two-column play — columns underfill by ~1 line (a persistent
    ~40px blank band at each column bottom), or after tightening that band the
    last column line's descenders slice.** The two-column FILL decision
    (`column_split`, `viewport.rs`) and the two-column CLIP
    (`update_bottom_clip`'s `exact_end` branch, `scroll.rs`) reserve DIFFERENT
    bottom bands, and they must be kept consistent:
    - The `exact_end` clip sums the ACTUAL line heights `[page_top, end-1]` and
      reserves only a `descender_allowance` (~5px, capped by the boundary blank
      budget) below the last line — NOT the full `BASE_BOTTOM_MARGIN`.
    - So the fill must NOT reserve the full `BASE_BOTTOM_MARGIN(40)` — that wastes
      ~1 line per column (the underfill). The two-column fill instead reserves
      `descender_guard + TWO_COLUMN_BOTTOM_MARGIN` (a small pad, `scroll.rs`),
      sized to what the clip consumes. Single-column paged pages KEEP
      `BASE_BOTTOM_MARGIN` (their clip DOES cover the whole band, `scroll.rs`
      `reserve = widget_height - usable_height`).
    - Tell (underfill): `column_split`'s `usable` is `card_h - guard - 40` while
      the clip band is only ~5px — logged fill leaves ~40px slack under a full
      column. Tell (slice): `TWO_COLUMN_BOTTOM_MARGIN` too small — the last line
      sits under the descender allowance; raise it.
    - `TWO_COLUMN_BOTTOM_MARGIN` is chosen EMPIRICALLY (smallest reserve with a
      clean last-line descender at 1920×1200); verify with
      `LIT_DEBUG_CLIP_COLOR='#ff0000'` (the red band must clear the descenders).
    - The two fill sites (`viewport.rs` left/right column `usable`), the
      first-spread short-opening probe, AND `validate_spreads`
      (`page_table.rs`, the generator's fit check) must ALL use the same
      constant, or generation rejects the fuller spreads and falls back to
      no-table. Changing the reserve shifts every two-column spread boundary, so
      the layout fingerprint version was bumped (`v2`→`v3`) to regenerate stale
      `play_pages` tables. NON-obvious downstream effect: the shifted breaks move
      the nav-fuzz cursor, so a SearchJump can target the work's trailing stage
      direction and trip the `viewport fill < 10%` guard — that is a harness
      artifact (the anchor still covers the last DIALOGUE line), fixed by
      exempting pure non-dialogue tail landings in `nav_test.rs`, NOT a clip bug.
    (`viewport.rs`/`scroll.rs`/`page_table.rs`, 2026-07-16;
    docs/superpowers/specs/2026-07-15-two-column-fill-reserve-design.md.)

16. **GLOSS OVERLAY, prose gloss — pages underfill by ~one unit (page fills to
    ~55%, a `⌄` marker floats mid-card over a wide blank band, and the footer
    shows 3 pages where 2 would do).** A prose gloss (e.g. a novel/essay reader
    gloss, or the TT front-matter "mock Dedication") alternates a SPEAKERLESS
    Source block (the quoted verse, `<speaker>UNKNOWN</speaker>` dropped from
    display) with its Explication paragraph. `repaginate` measured every block
    with `block_height_overhead`, whose old `else` branch charged EVERY non-
    speaker-source block a full `text_h + line_h` — including the speakerless
    source. But a speakerless source renders as plain wrapped verse lines with
    NO trailing paragraph gap (`gloss-verse` sets no `pixels_below_lines`; the
    inter-unit gap lives entirely on the FOLLOWING explication's `gloss-para`
    pad). So each Source paid ~one phantom `line_h` (~28px at Charter 17): on a
    page of 6 blocks that front-loaded ~84px, closing the page a whole unit
    early (3 units / 721px against a 974px budget → 253px wasted; four units
    would fit at 930px).
    - Tell: an alternating prose gloss paginates into more pages than the ink
      needs, each non-last page ending ~200-250px short with the `⌄` marker
      hovering over blank space (NOT flush at the card bottom).
    - Root cause: `block_height_overhead(is_source=true, has_speaker=false, …)`
      fell through to the explication `text_h + line_h` charge.
    - Fix: give the speakerless-source case its own branch charging only
      `text_h + SPEAKERLESS_SOURCE_PAD` (8px, covering the view-wide
      `pixels_below_lines(4)` + sub-pixel leading). Speaker-carrying sources
      (plays) and synopsis blocks (always `Explication`-kind) are untouched, so
      the "Gist:" synopsis clip guard (#14-ish, the per-block `line_h` reserve)
      still stands. Diagnose by dumping `repaginate`'s per-block charged heights
      vs. the budget (a temporary log/test) — the leaded `text_h` IS the true
      rendered text height, so the over-count is exactly the sum of the phantom
      per-source `line_h`s. (`gloss_overlay.rs::block_height_overhead`,
      2026-07-17.)

16b. **GLOSS OVERLAY, speaker gloss — a 2-block gloss splits across two
    half-empty pages (each ~46% full).** Same family as #16 but a DIFFERENT
    term, and it bites the SPEAKER path that #16 explicitly left alone. Two
    independent over-charges stacked, both in `repaginate`:
    - **The wrap width was computed from the wrong left edge.**
      `set_prose_margins` sets `left_margin = column_edge + QUOTE_BODY_INDENT`
      (the body indent is folded INTO the view) and `right_margin =
      column_edge`. `wrap_for` read `left_margin` back as if it WERE the column
      edge and then subtracted a full source `indent` on top — double-charging
      `QUOTE_BODY_INDENT` and assuming a symmetric `2 * left`. Every block was
      measured ~26px narrower than it renders, so every block was charged extra
      wrapped lines. (VF ch.1 at 1920x1236: charged wrap 640, true wrap 666.)
    - **The speaker reserve was proportional, not flat.**
      `block_height_overhead(is_source=true, has_speaker=true, …)` charged
      `text_h * 1.15 + SPEAKER_BLOCK_OVERHEAD`. The 1.15 predates leaded
      measurement: `text_h` now comes from `measure_text_height_leaded`, which
      ALREADY charges `OVERLAY_LINE_LEADING` per wrapped line, so the
      multiplier re-charged leading that was already counted — the identical
      phantom-height mistake #16 fixed on the speakerless branch. Because it
      SCALED with block size, a long source paid the most: VF ch.1's source was
      charged 520px for 424px of rendered ink.
    - Tell: a gloss with only two blocks (one Source + one Explication) reports
      `GLOSS-PAGES: n=2` with each page's ink band under half the budget, and
      the `⌄` marker floating mid-card. Distinguish from #16 by the block
      count and by `has_speaker` — #16 needs an ALTERNATING speakerless prose
      gloss; this fires on a single speaker-carrying source.
    - Root cause: `card_w - 2 * left - indent` (wrong edge, wrong symmetry) and
      the `* 1.15` slack. Combined: 555+419=974 vs a 906 budget → split.
    - Fix: derive the wrap from the real geometry —
      `card_w - column_edge - indent - right_margin`, where `column_edge =
      left_margin - QUOTE_BODY_INDENT` and `right_margin` is read from the view
      (synopsis reads `left_margin` directly; it applies no extra indent). And
      charge the speaker reserve FLAT: `text_h + SPEAKER_BLOCK_OVERHEAD`.
      Result: wrap 640→680, source 555→520→(flat) and the gloss fits ONE page
      at 92% fill (was 46%), 191px clear of the footer.
    - **Diagnose with numbers, never by eye**: temporarily log per-block
      `wrap`/`h` from `repaginate` and pixel-measure the rendered ink band and
      the card edges from a `grim` capture at PRODUCTION geometry (1920x1236 —
      720p gives budget 409 and will not reproduce). The charged height minus
      the measured ink band IS the over-count. (`gloss_overlay.rs::repaginate`
      + `block_height_overhead`, 2026-07-31.)

16c. **CARD PICKER list — a row is sliced through the middle at the top or
    bottom of the scroll viewport.** Not a text-clip at all: a `ScrolledWindow`
    scrolls by PIXEL, so nothing makes its viewport end on a row boundary. Two
    independent causes, and fixing only one leaves the slice visible:
    - **`scroll_row_into_view` parked the selected row flush against an edge.**
      `adj.set_value(y)` (top) / `y + h - page_size` (bottom) makes the SELECTED
      row exactly flush — which guarantees the row just past it straddles that
      edge. Symptom: the sliced row MOVES as you arrow through the list, always
      one row beyond the selection. Fix: snap the landing to a whole-row
      multiple (floor going up, ceil going down), then clamp into
      `[lower, upper - page_size]` so the last page still reaches the end.
    - **The viewport height was not a multiple of the row height.** The card
      sizes the list to whatever is left after header/entry/footer, so the
      BOTTOM row was permanently half-drawn even with no scrolling at all.
      Fix: `snap_list_viewport` sets the LIST's `margin_bottom` to
      `avail % row_h`, so the scrollable content ends on a row boundary. Driven
      from the list's `connect_map` + `connect_selected_rows_changed` (GTK4 has
      no `connect_size_allocate`; the row height is only known once rows exist
      and CSS is applied, so the read is idle-deferred).
      **`set_max_content_height` does NOT work here and was the first,
      failed attempt** — the scrolled window is `vexpand`, so the card stretches
      it and `max_content_height` constrains only the NATURAL height REQUEST,
      not the final allocation. The cap was silently overridden, the widget kept
      reporting its stretched height, and the snap converged on reading back its
      own output: `row_h=29 avail=727 target=725`, a permanent 2px overhang that
      still sliced the bottom row. Pad the content, don't try to shrink a
      `vexpand` viewport.
    - Tell: distinguish from a text clip by looking at WHAT is cut — here a
      whole ROW (background, label, and count together) is bisected, not a
      glyph's ascender/descender. If the cut row changes as you arrow, it is
      the scroll-landing half; if it is always the bottom row, it is the
      viewport-height half.
    - **Verify with the list FILTERED and scrolled, not just arrowed.** The
      first fix attempt passed an arrow-through of the unfiltered Authors list
      and still shipped the bug: typing in the filter entry repopulates the
      list (new row count, new natural height) and is the state the user hit.
      Drive it as: open picker → Enter (Works level) → type a filter char →
      arrow to the bottom, and check BOTH edges.
    - **FIXED 2026-07-31 by DELETING THE FOOTER, not by snapping anything.**
      The footer below the list is what created the problem: with it, the card's
      height minus the chrome was a non-multiple of the 45px row pitch, and that
      remainder is the strip the partial row was drawn in. Remove it and the
      `vexpand` list becomes the LAST child, running to the card's bottom edge —
      there is no leftover strip, so no partial row can exist. **The diagnostic
      that cracked it was the user pointing at a picker with NO bug**: the Q&A
      picker (`journal_picker.rs`) uses the same `new_picker_list` and the same
      45px pitch, and differs only in having no footer. Compare against a
      working sibling BEFORE theorising about the broken one.
      Four snapping attempts were tried first and all reverted; they are kept
      below because each one is a real GTK fact worth not rediscovering.
    - **The two measurements every attempt got wrong:**
      - **Row PITCH is 45px; `row.height()` returns 29px.** `height()` is the
        row's inner allocation, not the row-to-row step (which includes CSS
        `padding: 8px 14px` and inter-row spacing). Snapping to 29 yields a ~2px
        correction where the true remainder is ~32-40px — nearly a whole row,
        i.e. exactly the sliver on screen. Derive pitch from two ADJACENT rows'
        `compute_bounds().y()`; it assumes nothing about padding.
      - **`.library-picker scrolledwindow` has 14px vertical CSS padding**
        (`4px 8px 10px`) — inside the widget allocation, outside the viewport.
        Any target allocation is `pitch * n + 14`.
    - **What GTK actually does (read from source, not docs):**
      - `max_content_height` appears ONLY in `measure()`, never in
        `size_allocate`. It constrains the natural size REQUEST; a `vexpand`
        child is stretched past it. The cap is inert. (Attempt 1.)
      - `gtk_viewport_size_allocate` clamps `upper` to
        `MAX(allocated_height, measured_height)`, and `page_size == viewport
        height` exactly. So padding the LIST's `margin_bottom` cannot move where
        the scroll range ends — also inert, and a total no-op when the list is
        shorter than the viewport. (Attempt 2.)
      - GTK4 has NO scroll snapping for `ListBox`/`ListView`. `step_increment`
        is `viewport_size * 0.1` (GtkViewport) or `page_size * 0.1`
        (GtkListBase) — never a row height. Do not go looking for a built-in.
      - Under the AUTOMATIC scrollbar policy, `gtk_scrolled_window_size_allocate`
        re-runs allocation until scrollbar-visibility flags stabilise and can
        force both scrollbars visible on a late pass, changing the viewport
        height under any snap. `hscrollbar_policy(Never)` is a prerequisite.
      - **A `ScrolledWindow`'s MINIMUM height is ~0.** So `height_request` is a
        request, not a floor: put it in a box beside ANY `vexpand` sibling and
        GTK satisfies the scrolled window's minimum and gives the surplus to the
        greedy sibling. Measured: request 720, actual allocation **58**, with a
        `vexpand` spacer taking the rest. (Attempt 4 — the spacer that was added
        precisely to stop the card collapsing is what starved the list.)
      - Reading `avail` from `scrolled.height()` is a feedback loop: once a
        request is set the widget reports THAT height, so each pass snaps its
        own output; and on first map, pre-layout, it is a few px tall, so the
        list locks to ONE row. Derive `avail` from the card minus siblings.
        (Attempt 3. The arithmetic then came out right — `pitch=45 card_h=888
        siblings=147 avail=741 → request 720` = 16 rows — and it STILL failed,
        on the minimum-height issue above.)
    - **Lesson: do not put a widget below a scrolling list in a fixed-height
      card.** Whatever space the list cannot fill in whole rows becomes a strip
      that renders a sliced row. Footers on card pickers were removed for this
      reason (library, concordance-works, echo, echo-turns, settings); their
      keybind hints live in the Ctrl+/ overlay, where the Q&A picker's already
      did. If a footer is ever genuinely needed, the card height must be chosen
      so `card_h - chrome` is an exact multiple of the row pitch.
    - Fixed in `library_picker.rs` + `concordance_works_picker.rs` +
      `echo_picker.rs` + `echo_turns_picker.rs` + `settings_overlay.rs` (footer
      removed) and `picker_nav.rs::scroll_row_into_view` (landing snaps to whole
      rows). (2026-07-31.)

17. **A clip E2E (`overlay_clipping` / `line_clipping`) FAILS with the viewport
    rect never appearing (`TEST_OVERLAY_VIEWPORT_RECT never appeared …`) OR a
    "clip" on a work you didn't expect — a STALE TEST, not a production clip.**
    The clip e2e tests drive the app through the CURRENT keymap and load
    whatever `config-dev.json` `last_work` is; both drift out from under a test
    frozen at write-time. Two distinct tells:
    - **Overlay rect never logged** → the test's key script sends a bind that
      MOVED. `overlay_clipping` originally sent plain `h` (then `ShowSynopsisOverlay`)
      and `3` (then `JumpToNextScene`); both rebound (`h` → `CursorNextDialogueNoSeek`,
      synopsis → **`Ctrl+h`**; scene-next → **`{`**/`braceleft`). The overlay
      never opened, so no rect. **Verify the test's keys against
      `src/input/keymap_config.rs` AND `~/.config/linux-lit/keymap.json` before
      touching any clip code** — the production synopsis overlay was fine.
      Fix: update the test's key script (`h.chord(&["ctrl"], "h")`,
      `h.key("braceleft", …)`), not production. (2026-07-20.)
    - **A clip on an unexpected work** (the attribution run's Bleak House
      `first row h=52 margin=0` top clip) → the run's `config-dev.json`
      `last_work` differed from the canonical shared config. The tests have NO
      work-override env var; they load `last_work` (currently `Cym`, a two-column
      play, which passes cleanly). A clip reported against a work the shared
      config does not load is not reproducible from the canonical state — confirm
      the `last_work` the failing run used before assuming a production bug.
    Neither is a clip-math bug: no clip path (a/b/c) is involved. Diagnose by
    reading `target/ui/*.png` — a genuine overlay/main-card render with a real
    clip vs. a card that never changed state (overlay never opened).

18. **A fixed-scroll-height OVERLAY renders FULL-WINDOW height, top-anchored and
    underfilled — content at the top, a huge blank band below, and a stale/
    left-packed header line at the card's very top.** NOT a clip-path bug: a
    chrome widget above the scroll is VISIBLE but UNACCOUNTED in the scroll
    budget, so the `valign=Center` container's natural height exceeds
    `card_height` (a height_request is only a minimum) and the card grows to the
    window edges. The 2026-07-21 instance: the synopsis running head added a
    `title_scene` label sharing the gloss overlay's title row; the gloss-result
    path hid `title` directly (never calling `set_gloss_title_style`), leaving
    `title_scene` visible with the previous synopsis's text — with the hexpand
    `title` hidden, the box packed it LEFT (its `halign End` had no slack to act
    on), which is the "position text at top-left" tell. Meanwhile `size_scroll`
    charged `title_pref_h()` = 0 (title hidden), so the visible row's ~60px
    pushed the container past the card.
    - Tell: overlay flush to the window's top/bottom (no root band, no rounded
      corners) while the synopsis view of the SAME widget sizes correctly;
      content underfills because the scroll got the full `card_height` budget.
    - Rule: every show path must leave the title-row labels in a state the
      sizing call accounts for — a label visible in ANY mode must be measured
      (or hidden) in EVERY mode. The gloss/journal overlays now show the
      running head in their result views and charge its row height
      (`size_scroll(card_height, title_pref_h())` / journal `size_card`'s
      `head_h + UNACCOUNTED_CHROME_MARGINS`), keeping
      title + margins + scroll + footer == card_height.

19. **OVERLAY FOOTER — the "X / Y" / "Q&A n of m" counters read as clipped
    against the card's bottom rule, but nothing is actually cut.** NOT a clip
    path (a/b/c) and NOT occlusion: the glyphs are fully rendered, just crowded.
    The tell that separates it from a real clip is **arithmetic on the two ends
    of the same card** — measure the gap above the header ink and below the
    footer ink. A genuine clip severs glyphs; this one leaves them whole but
    lopsided. Measured on the 2026-07-29 report (journal Q&A, BH-Barrett ch. 5):
    header 29px above vs footer **14px** below, on a 1172px card — and the
    horizontal inset was already symmetric (`5 / 5` ended 40px inside the card
    edge, exactly matching `Chapter 5`), which is what ruled out a width/clip
    problem in one measurement.
    - **Cause:** the shared `build_footer_row` (`src/ui/footer.rs`) used a
      symmetric `margin_top(12)`/`margin_bottom(12)`, while BOTH callers'
      running heads use `margin_top(24)`/`margin_bottom(12)`
      (`journal_overlay.rs` `head_row`, `gloss_overlay.rs` `title`). So the head
      strip was 24px deep and the foot strip only 12 — the card was lopsided by
      design, and it became noticeable only after the MAIN reading card moved
      30px of padding from its running head to its foot over 2026-07-28
      (`TOP_SPACER_HEIGHT` 74 -> 44 against `SINGLE_COLUMN_BOTTOM_MARGIN` /
      `TWO_COLUMN_BOTTOM_MARGIN` 22 -> 52, five 5px passes). That series touched
      only `app/mod.rs` + `scroll.rs`; the overlays derive their budget from
      `main_card_rect` and their own chrome, so they never saw the shift and kept
      the pre-shift proportions next to a newly deep-footed reading card.
    - **Fix:** `FOOTER_MARGIN_BOTTOM = 24` in `footer.rs`, mirroring the head's
      `margin_top(24)`. Verified headless on the gloss overlay (the other caller
      of the same builder): footer gap 14 -> **24px** against a 29px header.
    - **Why it costs no text and needs no constant bumped:** both callers pin
      their scroll height from the footer's LIVE `preferred_size()`
      (`journal_overlay::size_card`, `gloss_overlay::size_scroll`), so the extra
      12px comes out of the scroll viewport automatically and the row grid stays
      consistent. A footer whose height were hardcoded anywhere would have
      needed a matching bump — check for that before changing this margin again.
    - **Lesson:** when a padding change is described as "move N px from the head
      to the foot," the head/foot pair on EVERY surface that mirrors that strip
      is in scope, not just the one whose constants you edited. The overlays
      mirrored the head side (a literal 24) but had no corresponding foot
      constant to move, so the asymmetry was invisible to a diff of the reading
      card's three constants.
    - **The HORIZONTAL half of the same report (`FOOTER_END_INSET`).** After the
      bottom margin was fixed, the counter still read as "clipped in the lower
      right" — and it was NOT clipped: magnifying the corner 6x showed `1 / 5`
      whole, at a 41px right inset that matched the running head's exactly. The
      complaint was crowding, not severance. Two process notes worth more than
      the constant:
      - **"It matches the header" is not a correctness test** when the header is
        itself too tight. Symmetry with a wrong reference reads as proof and
        isn't. Anchor to how the element should look, not to another element.
      - **When measurement says clean and the user says clipped, MAGNIFY before
        re-asserting.** A 6x `Image.crop(...).resize(..., NEAREST)` of the corner
        settled in one look what three rounds of ink-span arithmetic could not,
        and reframed the bug from "clipping" to "inset." Pixel-measuring is the
        house rule for edges, but an ink-extent number cannot distinguish
        "severed" from "merely tight" — crop and look.
      - Fix: the row's `margin_end` is `FOOTER_END_INSET` (56), no longer
        `text_margins` (40); the LEFT stays 40 so the footer label remains flush
        with the body text. Verified 41 -> **57px** headless (gloss overlay,
        1920x1236) with the left inset and 24px bottom gap unchanged. The
        `hint` label's unconditional `margin_end(12)` was dropped in the same
        change — it was empty in every state but visual mode, so it only muddied
        the inset arithmetic.
      - **The `hint` label was then removed outright** (2026-07-29, follow-up),
        along with `set_journal_hint`/`set_journal_visual_hint`/
        `set_gloss_hint`/`set_gloss_visual_hint`/`set_synopsis_hint`/
        `set_synopsis_visual_hint`, the `BlockVisualCfg::set_hint` fn-pointer
        field, and the echoes view's inline keybind line. The footer row now
        holds ONE child (the caller's left label) plus whatever counter the
        caller appends. Two things to know before touching it again:
        - **`left` must stay VISIBLE even when blank.** It is the row's only
          `hexpand` child, so hiding it collapses the right-aligned counter to
          the left edge (`gloss_overlay::hide_citation` documents this; it was
          written when `hint` was the right-aligned victim, and the hazard is
          unchanged now that the counter plays that role). Verified after
          removal: counter still at a 57px right inset, not collapsed.
        - **Nothing became undocumented.** The visual-mode legend's binds are in
          the gloss/journal legends (`Shift+V`), and every echoes bind the
          inline line advertised (`A`/`s`/`d`/`D`/`R`/`a`/`Esc`) is already in
          `echo_keybinds_overlay.rs` — more completely than the hint had them.
          Check the per-surface legend before deleting any on-screen help text;
          if a bind lives ONLY in the text being removed, move it to the legend
          in the same change.
      - **THE ACTUAL ROOT CAUSE (found third, after two wrong fixes): the footer
        labels had NO CSS class, so one glyph washed out.** The padding and inset
        fixes above are real improvements, but neither was what the user was
        pointing at. The report was "the top of the numeral 5 is obscured" — and
        it was, by a rasterisation artifact:
        - Both callers `remove_css_class("gloss-hint")` to drop its border-top,
          and that was the ONLY rule styling the row, so every footer label
          rendered at GTK's DEFAULT size in the body-text colour.
        - At that inherited size the digit `5`'s flat top bar landed on a pixel
          boundary and rasterised at ~13% coverage — raw R **217** against a 250
          card — while its own stem stayed solid at **109**. `3` and `/` were
          unaffected (different stroke geometry), and the SAME `5` in the running
          head, at a defined size, rasterised its bar solid at **133**. One glyph
          in one position looked broken; everything around it was fine.
        - `.gloss-position` (`font-size: 14px; color: {dim}`) already existed in
          `theme.rs` for exactly this row and was applied to NOTHING — dead CSS
          since the footer was extracted into `footer.rs`. Wiring it up
          (`FOOTER_LABEL_CLASS`, on the row's `left` label AND both callers'
          `position_label`) fixes the artifact and makes the footer read as dim
          chrome like the header (ink `87,82,121` → `184,179,190`).
        - Proof it is the SIZE, not the colour: rendering `5` straight through
          Pango in Charis at 12/13/14/15/16px in the dim colour gives a top-bar
          min of **184** (full ink at that colour) at EVERY size. Only the
          unstyled default washed out.
      - **Process lesson — three wrong fixes before the right one.** "Clipping in
        the lower right" was read as (1) bottom padding, then (2) right inset,
        then (3) clipped glyph tops, before the cause turned out to be unstyled
        labels. What finally cracked it:
        - **The user's later phrasing WAS the diagnosis.** "Obscured, not
          clipped" plus "`3 / ` is fine but the `5` isn't" narrowed it to a
          single glyph — unguessable from "clipping," and it ruled out geometry
          outright, since both digits share the same box and inset.
        - **Read RAW pixel values, not a thresholded ink mask.** Every earlier
          check applied `isbg()` with a tolerance, which classified the 217 bar
          as "no ink here" — the mask HID the defect and produced three
          confident "measures clean" verdicts. Printing actual values exposed it
          at once. When a user insists something is wrong and the mask says
          clean, print the numbers.
        - **A flat-topped glyph is not evidence of a clip.** The `5`/`1` flat
          tops were briefly called truncation; the header's `5` has the same
          shape, so that is the serif face, not a cut. Compare the same glyph
          elsewhere before concluding.
    (`gloss_overlay.rs`, `journal_overlay.rs`, 2026-07-21.)

19b. **CARD PICKER header — the count ("27 AUTHORS") reads as though its
    leading digit is sliced off, but nothing crops it.** The same
    washed-out-glyph family as #19, on a different surface, and it repeated
    #19's mistake of chasing the wrong cause first. Diagnosis, in the order
    that actually worked:
    - **Pixel-dump the glyph before theorising.** A column profile showed the
      `2` fully formed with clean header background 3px to its left, and ~140px
      of clear header to the right of the whole string — no container edge
      anywhere near it. That killed "clipped" outright.
    - **Magnify (#19's own lesson).** A 6x NEAREST crop showed ALL TEN glyphs
      eroded along their tops, not one letter cut at an edge. A single sliced
      glyph means geometry; a whole string thinning at the same height means
      rasterisation.
    - **Cause:** 13px at weight 400 rendered the string only ~9px tall, and
      hinting ate the tops of the thin serif strokes. The `2` broke into a
      `#..##` fragment and the rest showed mid-stroke gaps.
    - **Fix:** give the crumb the header title's full face — size, `font-weight:
      700`, `letter-spacing: 2px`, and opacity — so the two header labels are
      one typographic pair. Glyph height 9 -> 10px and the letterforms come out
      solid. Note the ACTIVE skin is `HeaderBand` (12px title); `RootFill` is
      dead code with a 14px title, so match each skin's OWN title rather than
      copying a number between them.
    - **A contrast fix was tried first and DID NOT WORK — do not retry it.**
      The crumb measured 3.92:1 against the teal header (under the 4.5:1 AA
      floor) while the title passed at 4.71:1, which looked like a compelling
      cause. Raising opacity 0.75 -> 0.85 took the measured contrast to 6.66:1
      and changed the rendered glyphs NOT AT ALL. That negative result is what
      proved the defect was stroke rendering, not contrast; the change was
      reverted rather than shipped. **Sub-threshold contrast and eroded strokes
      look alike in a screenshot — the discriminator is whether raising contrast
      changes anything, so measure the glyphs before AND after, not just the
      ratio.**
    - **Generalised lesson (shared with #19):** for small chrome text reported
      as clipped, suspect the FACE (size/weight/undefined style) before
      geometry. Both #19 and this one were a numeral rendering thin at a size
      nobody had pinned, and both cost multiple wrong fixes by treating the
      report's word "clipped" as a description of the mechanism rather than of
      the appearance. (`theme.rs` `.library-picker-crumb`, 2026-07-31.)

15. **A hand-drawn Cairo surface lays annotations out against the FIRST
    wrapped line instead of the whole wrapped block (OVERSTRIKE, not
    clipping).** The syntax diagram drew its POS row and band rules starting
    from the passage's first line origin, so on any selection that wrapped to
    two or more visual lines the annotation stack was painted straight
    THROUGH the following lines of text. Nothing clips and nothing is
    dropped — the glyphs and the rules simply occupy the same pixels.
    - Tell: text and rules interleaved in one band of rows; the span
      validator reports zero drops (`SYNTAX: N bands, M pos tags` with no
      `dropped band` lines), so the data is fine and only the picture is
      wrong. Pixel-scan confirms it: band rules at rows 123-158 with the
      passage's second line at ~140.
    - Root cause: a Pango layout's `pixel_size().1` (full wrapped height) was
      available but the annotation origin used a single `line_h` instead.
    - **Root cause (proven, and NOT what it first looked like):** the
      annotation origin was already per-line and correct. The real defect was
      that a legibility FLOOR (`MIN_BAND_ROW_H`) made deep stacks
      structurally unable to fit the natural leading: with `line_h` 27,
      clearance 18 and 5 rows, `interior_row_height` returns the 12px floor,
      putting the outermost rule at (5-1)x12 = 48px — 21px into the next
      line. No per-line arithmetic can fix that; the GAP is the only free
      variable.
    - Fix: `line_spacing_for(rows, natural_line_h, clearance)` computes the
      height the text must be SET at (`layout.set_line_spacing`, a FACTOR of
      natural height in Pango 1.44+), then annotations anchor at
      `natural_line_h` below each line's top — the widening IS the gap they
      occupy, so anchoring at the widened `line_h` just pushes the stack onto
      the next line again.
    - **Second-order trap:** after widening, an interior-vs-last-line row
      height split becomes the bug. The last line kept a generous
      budget-derived `rh` and painted 605px-wide rules exactly where line 2
      had been laid out. Once the gap is uniform, use ONE row height for
      every line.
    - **Third-order trap:** the commentary below must clear
      `max(stack_bottom, text_top + th)`. Widened leading can put the text
      block's own bottom below the last rule, so keying off the band stack
      alone puts the note back inside the diagram.
    - Related, same surface: a label wider than the span it annotates needs
      elision or suppression, or adjacent short spans smear together
      (`PUNCTADJ`, `SCONJ DETNOUN`). Measure the label against its span width
      before drawing it. Band-label collision needs a VERTICAL TOLERANCE
      (~the label's text height), not exact `row_y` equality — labels float
      above their rules, so adjacent DEPTHS overprint while comparing equal
      rows reports no collision.
    - Verification: pixel-scan for rows that are simultaneously text-heavy
      and rule-heavy. Whole-band overstrike went from most of the annotation
      band to 4 minor label grazes out of 15 rule rows on a 21-band,
      5-wrapped-line paragraph.
    (`syntax_overlay.rs`, 2026-07-26 — found by the mandatory headless check,
    NOT by the build or the unit tests, both of which were green. Three
    successive headless re-measures were needed; each fix exposed the next
    layer, and none was visible from logs.)

    **UPDATE, same day — cage PASSED this and the real GL renderer did not.**
    The first real-renderer run of this surface showed the annotations
    unreadable on a 10-band, 2-line sentence: labels printed on their own
    rules and on each other, band rules struck through the POS row, rules
    overran the text. Cage had reported "4 minor grazes". Every constant on
    this surface had been tuned against software rendering. Four more layers,
    each exposed by fixing the one before:
    - **A hardcoded label offset against a smaller row floor.** Labels floated
      a fixed 14px above their rules while `MIN_BAND_ROW_H` was 12px, so a
      compressed stack put every label above its OWN rule and onto its
      neighbour's. Any label offset MUST derive from the row height, and the
      row floor from the label height — otherwise the two invert under
      compression. Same for the collision tolerance: a constant 13px
      under-reports once `rh` shrinks below it.
    - **Pango draws from the text's TOP-left, not its baseline.** An offset of
      `rh - 2` still left glyphs sitting on the rule. The label's own measured
      HEIGHT is what must clear it (`layout_text` already returns it — the
      draw site was discarding it as `_`). Measure, don't guess.
    - **Two elements anchored at the same y.** The POS row and the outermost
      band rule were both placed at `natural_line_h`, so the rule struck the
      tags through BY CONSTRUCTION, not by drift. When two stacked elements
      share an anchor, one of them needs the other's height added.
    - **A width test is not a collision test.** A DEEP band sits high in the
      stack, so its label lands in the POS row regardless of how wide the band
      is. Test the actual geometry (does the label's top clear the POS row's
      floor?) rather than a proxy. Corollary learned by over-correcting:
      tightening the width proxy to `lw <= span` suppressed 5 of 6 labels —
      "appositive noun phrase" is legitimately wider than the 2-3 word span it
      names. A clean diagram missing most of its labels is WORSE than a
      slightly overhanging one; overhang is cosmetic, a missing label is lost
      meaning.
    - **Reserve space for the innermost band's label.** With the stack
      starting immediately below the POS row, the innermost band (depth =
      rows-1, offset 0) has nowhere to put its label. Reserve one label height
      (`STACK_TOP_OFFSET = POS_ROW_H + LABEL_H`) instead of suppressing it.
    - **Name the shared offset once.** The spacing budget, the drawn `row_y`,
      and `band_stack_bottom` must use an IDENTICAL offset; they drifted apart
      twice while it was open-coded at each site.
    - **Unit tests that pin superseded geometry are evidence, not obstacles.**
      Two row-height tests failed on the raised floor. Both were updated, not
      deleted: the new contract is that the legibility FLOOR wins over a tight
      budget (`line_spacing_for` absorbs the overflow), and that a stack fits
      the SET line height rather than the natural one.
    (`syntax_overlay.rs`, 2026-07-26 — **the lesson is the renderer, not the
    arithmetic**: a layout "verified" only in cage is unverified. Run the real
    GL check before believing any pixel-level acceptance on this surface.)

    **RETIRED (2026-07-26).** The surface this entry describes no longer
    exists. After four rounds of layout fixes in one day — each exposing the
    next, with cage passing layouts the real GL renderer rejected — the Cairo
    diagram was replaced by `syntax-gloss`, a prose gloss type rendered by the
    existing overlay (spec:
    `docs/superpowers/specs/2026-07-26-syntax-gloss-design.md`). The entry is
    kept because its LESSONS generalize to any annotation layer drawn over
    text: derive offsets from measured content rather than constants, never
    anchor two stacked elements at the same origin, and test the real renderer
    before believing a headless pass. The specific fix history is now
    archaeology.

19. **The INVERSE of a clip: single-column PROSE paints content the page was
    supposed to END before — a chapter heading (and the next chapter's opening
    paragraph) rendering below the end of the previous chapter.** Nothing is
    sliced; too much is shown. The stored `prose_pages` grid is CORRECT and the
    page top is CANONICAL, so both usual suspects check out and the diagnosis
    stalls (see page-turning-mechanics.md for the two off-grid variants that
    look identical on screen). **Check the page top against the table before
    reading further: if it is NOT a stored `start_line`, this is #20, not this
    entry — the clip is innocent and a landing is at fault.**
    - Tell: the clip line reads `BOTTOM_CLIP_ROWFILL … row_clip=0` on a page
      whose stored end is EARLIER than where the rows stop fitting. An
      `exact_end`-governed page logs `BOTTOM_CLIP_EXACT` instead — so
      **`ROWFILL` on a page that should end early is the signature.**
      Confirm by querying the active table: `SELECT start_line_id,
      end_line_id FROM prose_pages WHERE …` — if `end_line_id` is the chapter
      heading's line, the pagination did its job and the clip ignored it.
    - Root cause: the single-column prose bottom clip was purely GEOMETRIC. It
      covers from the last visual row that fits `usable_height` to the card
      edge and never consulted the stored page. Correct when a page ends
      because it ran out of room; wrong when it ends EARLY BY RULE — which is
      what the chapter clamp does. The two-column path never had this bug
      because it always passes the stored split as `exact_end`.
    - Fix: `scroll::prose_exact_end_for_current_page` supplies the exclusive
      `exact_end` for single-column prose from
      `prose_table_last_line_for_top`. **Both clip-scheduling sites must use
      it** — `refresh_bottom_clip` gated `exact_end` on `column_count() == 2`,
      so fixing only the render path left a second live route to the bug.
    - The `+ 1` (inclusive last rendered line → exclusive end) is load-bearing:
      an off-by-one there paints exactly one line too many, which IS this bug.
      `prose_pages::last_rendered_line` is the pure conversion, with a
      regression test built from real BH-Barrett page-82 values
      (`(686,0)..(697,0)`, buffer 697 = the "CHAPTER VIII" line).
    - **Reproducing it under cage needs TWO things** (both fixed/verified
      2026-07-27, and it was reproducible neither way before):
      1. `wlr-randr --output HEADLESS-1 --custom-mode 1920x1236` — **1236, not
         1200.** Pagination keys on the TEXT VIEW height; 1236 yields the
         production `text_view.height = 1098`, while 1200 yields 1062, a 36px
         miss that changes the grid and hides the bug.
      2. Wait for the table to REGENERATE after that resize. The resize lands
         after the app maps, so the first table is built at 720p and dropped;
         until the resize tick learned to regenerate, the run had no table at
         all and table mode never engaged (the geometric clip is then
         correct, so nothing looked wrong).
      Confirm both from the log: `RESIZE_TICK: text_view.height changed … ->
      1098`, then `PAGES_PROSE: page N/M`. The fix's own tell is
      `BOTTOM_CLIP_EXACT … clip=389 page_top=686` where it previously read
      `BOTTOM_CLIP_ROWFILL … row_clip=0`.

20. **`ROWFILL` where `EXACT` belongs, but the CLIP code is innocent — a
    LANDING put the reader off-grid.** Same engine-swap signature as #19, and
    the two are easy to confuse. The difference: in #19 the page top is
    canonical and the clip ignored the stored end; here the clip is working
    correctly and the PAGE TOP itself is not a stored boundary, so there is no
    stored page to match and the clip legitimately falls back to row-fill.
    **Do not debug the clip for this one** — fix wherever the landing came
    from.
    - Tell: a `BOTTOM_CLIP_ROWFILL` whose `page_top` equals the CURSOR line,
      immediately after a jump/overlay-close, on a work that was logging
      `BOTTOM_CLIP_EXACT` a moment earlier. The paired
      `PAINT: first frame for page_top=<cursor line>` is the confirmation — a
      stored page top is rarely the exact line you jumped to.
    - Distinguish from #19 in one query: look up the page top in the active
      table. #19's top IS a `start_line`; this one is not (BH-Barrett landed on
      47 when the stored page was `(42, 603)`).
    - Root cause: a landing computed its page geometrically instead of reading
      `prose_pages` — most often via a bare-`usize` page top that structurally
      cannot carry the row offset a prose boundary needs. Full write-up (three
      call sites, one shared fix) in **page-turning-mechanics.md, "A landing
      that drops out of table mode"**.
    - **Which landing?** That entry's audit sub-section lists every
      close-to-reader path by category. As of 2026-07-27 every known landing
      reads the grid — overlay closes, jumps, the resume, cross-work landings,
      centering landings, and the translations hide. So a NEW occurrence is a
      new call site, not one of the known ones: find whoever set
      `page_top_line` without going through
      `canonical_page_top_offset_for` / `prose_table_boundary_for_line` /
      `restore_saved_position_resnap`. A close that only calls
      `return_to_reader_mode` is still fine when that surface never moved the
      reader (synopsis, every picker) — do not "fix" those.
    - Fix: land through `navigation::canonical_page_top_offset_for` and pass
      BOTH halves to `set_page_instant_offset`. Dropping the offset lands on
      the right line and still mis-frames by up to a full paragraph.
    - Startup variant: `DISPLAY_WORK: resumed saved position … page_top=N`
      followed by `PAGES_PROSE: resnap off-grid` means the resume guessed and
      the safety net corrected it. The net is defence-in-depth, not the
      mechanism — and it can paint LATE (23.6s in one observed run), leaving
      the wrong page on screen meanwhile. A resnap on a clean launch is a bug
      upstream of the clip.

21. **A SECOND CARD behind the reader — HORIZONTAL overflow, not a text clip.**
    Not a clipped line at all: a cream rectangle peeking out past the card's
    right edge, reading as another card stacked behind the reader. No text is
    cut, so none of the bottom-clip paths (a/b/c) apply and `CLIP_WARN` is
    silent. **Do not debug the clip code for this one.**
    - Tell: pixel-scan the rows and find TWO right edges at different x. The
      reader's own edge is rounded + anti-aliased (`border-radius: 12px`); the
      stray one is a HARD square edge, and it starts `TOP_SPACER_HEIGHT` (44px)
      BELOW the card top — because `top_spacer` covers the overflow for exactly
      the running-head strip's height, and the columns below it do not.
      Measured 2026-08-03 on Per-Amb: card right edge 1723, stray edge 1732.
    - Root cause: the card was allocated NARROWER than the block it must hold.
      `apply_tiled_mode` hard-sizes each column with
      `set_width_request(MIN_TWO_COLUMN_COLUMN_WIDTH)`, so the
      `[col | divider | col]` block is a fixed width that CANNOT shrink;
      `columns_hbox` is `halign: Center`, which has no way to compress an
      oversized child. If `target_card_width`'s two-column floor reserves less
      than the block really needs, the difference renders past the card's right
      edge and paints its `.card-bottom` cream there. Nothing in that ancestor
      chain sets `Overflow::Hidden` (only the two `ScrolledWindow`s do), so the
      overflow is never clipped away.
    - The specific defect: the floor reserved `+ 8` for the divider while
      `.column-divider` actually costs 17px (`min-width: 1px` + `margin: 0 8px`).
      At 1920 the card floored at 1528 while the block needed 1537 — a 9px
      overhang, exactly the measured 1732-vs-1723.
    - Fix: `two_col_floor = 2 * MIN_TWO_COLUMN_COLUMN_WIDTH + COLUMN_DIVIDER_WIDTH`
      (`src/app/layout.rs`). Guarded by
      `two_column_card_is_wide_enough_for_the_block_it_must_hold`.
    - Generalisation: any constant reserving room for chrome must count the
      chrome's FULL CSS box (margins included), not the rule's `min-width`.
      Rounding DOWN is safe where the value is subtracted from a budget
      (`TWO_COLUMN_CHROME_ALLOWANCE`) and unsafe where the card must be wide
      enough to HOLD the block — the two uses need opposite rounding, which is
      why they are now separate constants.
    - Latent since the columns were fixed-width, but only became VISIBLE with
      the `CardLayout` manager (a5ed3a0c): a `width_request` used to propagate a
      child's minimum up so the box could never be under-allocated, whereas
      `CardLayout::allocate` hands the child exactly `card_width` with no
      renegotiation against what the child measured.

22. **MAIN CARD, single-column PROSE — DEEP pages overflow by 44-127px on a
    FRESHLY generated table, because generation measured before the FONT was in
    effect.** The reported symptom is text cut mid-glyph at the bottom rule deep
    in a novel (BH-Barrett ch. 26, pages 335+), with
    `CLIP_WARN: main-card prose-1col OVERFLOW total=… > widget_h=… clip=0`.
    - Tell (the one that separates this from every other prose overflow): the
      overflow is **positional**. Early pages are healthy (clips 53-87px);
      deep pages all overflow. Anything that only drives pages 1-15 passes
      while the bug is fully present. Second tell: the generation-time census
      reports `PAGES_PROSE_DRIFT: summary … over_usable=0` — a CLEAN bill of
      health — because it reads the same wrong heights generation used and so
      agrees with itself. **Only a render-side assertion can see this.**
    - Root cause: the reader's body font is a buffer-wide `font-size` TextTag
      applied by `reapply_font`, NOT the view's CSS font. Applying it
      invalidates every line's layout, but GTK re-measures lazily, so a
      `line_yrange` sweep run before the view processes that invalidation
      returns heights for the PREVIOUS, smaller face. Heights decompose as
      `40 + 28*(rows-1)`: every paragraph of 4+ wrapped rows came back exactly
      one row (~29px) short, while 1-3 row lines were unaffected — which is why
      dialogue-heavy openings look fine and dense later chapters do not. The
      per-page deltas sum to the whole overflow (measured: +115px on the page
      that rendered 1189 against widget_h 1164).
    - **Two back-to-back sweeps CANNOT detect it.** Both run after the same
      invalidation and read the same cache, so `PAGES_PROSE_SWEEP:
      changed_between_sweeps=0 delta_sum=0` is reported while every height is
      wrong. That zero was read for a whole session as proof that lazy
      validation was eliminated; it proves only that the cache is stable, not
      that it is correct. Do not re-run that experiment expecting an answer.
    - Ruled out by measurement, each costing a cycle: stale table (reproduces
      on a fresh `validated=1` table); wrap width (`wrap_w=899` identical at
      both moments); tags (the growing lines carry `tags=[]` at both moments);
      `pv5`-vs-`pv6` page version (both engines store BYTE-IDENTICAL boundaries
      over the overflow range — only the page NUMBERING differs).
    - Fix: pump the main loop (`queue_resize` + bounded
      `MainContext::iteration`) at the top of `record_prose_pages`, BEFORE the
      validating sweep, so the sweep measures the face the view will render.
      Plus a convergence guard — the second sweep is now load-bearing, not a
      diagnostic: if any height still moves between passes, `record_prose_pages`
      returns `Err` and the caller falls back to the live engine rather than
      pinning an over-packed table into lit.db, where it would outlive the
      session.
    - **Fixing generation is only HALF the fix — bump the page version too.**
      The layout fingerprint is a function of geometry + font. Neither changes
      when the measuring bug is fixed, so every table ALREADY stored with the
      bad heights stays a valid `table hit` forever and the corrected generator
      never runs. This was caught only because the user's screenshots still
      showed the bug after the generation fix shipped: their log read
      `PAGES_PROSE: table hit (793 pages)`, never `generated`. `pv7` forces the
      miss. Skip a version rather than reuse one belonging to a reverted
      change (`pv6` = the reverted `e1b17ac0`, whose tables carry the same bad
      heights). **Generalisation: any fix that changes what a stored table
      CONTAINS, without changing what the fingerprint MEASURES, must bump the
      version — otherwise it is a no-op for every existing reader.**
    - Real-renderer acceptance (required by this doc, cage is software
      rendering): measured on the user's own display at 1920x1200 — bottom
      clearance 62/86/66px across three consecutive ch. 26 pages, vs 6/16px on
      the same pages before the fix, with text continuous across every page
      boundary (a whole clause had been swallowed between pages).
    - Guard: `deep_prose_pages_never_overflow_the_card` (`tests/prose_page_fit.rs`)
      lands at ch. 26 via `LIT_START_SCENE=26.0` and asserts on render-side
      `CLIP_WARN`/`BOTTOM_CLIP_EXACT`. Verified failing before the fix (5 pages
      over by 23-114px) and passing after (all pages 1081-1113 against 1164,
      clips 46-78).
    - Diagnosing a recurrence: `LIT_TRACE_HEIGHTS=<a>:<b>` dumps the
      generation-time height vector, `LIT_TRACE_TAGS=<a>:<b>` dumps per-line
      tags+text+height at BOTH generation and render (diff them), and
      `LIT_TRACE_PANGO=1` compares every line against an independent Pango
      layout. Caveat: the Pango probe carries a ±1px line-box offset, so trust
      its CHANGES between sweeps, not its absolute values.

23. **MAIN CARD, single-column PROSE — DEEP pages overflow with the SAME
    `CLIP_WARN … OVERFLOW` tell, the SAME positional pattern, and the SAME
    `40 + 28*(rows-1)` decomposition as item 22, on a build that ALREADY HAS
    item 22's font-in-effect fix.** Symptom alone cannot tell these apart —
    read the discriminator below FIRST.
    - **The one-step discriminator: regenerate twice and diff the page
      count.** Item 22's bug is deterministic — the same fingerprint always
      produces the same table. This bug is a RACE: the same fingerprint,
      content, and geometry produced **801, 806, and 808 pages** across three
      separate cold generations. Grep two fresh `PAGES_PROSE: generated N
      pages` runs; if `N` differs, this is item 23, not item 22 — no amount
      of re-reading item 22's fix will explain it. (Confirming item 22's fix
      is already present — the font-in-effect preamble in
      `record_prose_pages` — is corroborating, not sufficient by itself,
      since a build can carry both fixes and still race.)
    - Tell: `CLIP_WARN: main-card prose-1col OVERFLOW` on a `PAGES_PROSE:
      table hit` page deep in a prose work (BH-Barrett ch. 37, pages 335+),
      on a table generated by a build that already has item 22's fix.
    - The census does **not** discriminate either bug — both agree with
      themselves and disagree with the render, so a clean
      `PAGES_PROSE_DRIFT: over_usable=0` at generation time rules out
      neither. Don't spend a cycle on it; go straight to the page-count diff.
    - Root cause: the buffer-wide `line_yrange` sweep in
      `record_prose_pages` reads GTK's lazily-validated line layout. GTK
      gates full validation on proximity to the viewport — lines far from
      wherever the viewport happened to sit at generation time return a
      PROVISIONAL estimate, systematically short by whole rows, which GTK
      then caches as final. Because the estimate depends on scroll history
      and timing rather than on content + geometry alone, generation is not
      a pure function of its fingerprint — hence the race. Measured:
      `GEN_HEIGHTS == POSTWALK_HEIGHTS` byte-identical (the boundary walk
      itself moves nothing) while `RENDER_HEIGHTS` was **+225px across 12
      lines**, every delta an exact ~28px row multiple.
    - Why the existing convergence guard could not see it: it swept twice
      and compared, but both reads hit the same stale cache — it logged
      `changed_between_sweeps=0 delta_sum=0` on the very run that rendered
      225px over. **A check that compares a measurement against a second
      copy of itself proves self-consistency, not correctness** — the same
      blind spot item 22's guard had, but item 22's fix (pumping the loop
      once before sweeping) does not force validation of lines the viewport
      never approached, so it does not cover this case.
    - Fix: `validate_all_lines_by_scrolling` steps the vadjustment through
      the WHOLE buffer in viewport-sized stops (not per-page), pumping
      `MainContext` at each stop, before the measuring sweep runs — forcing
      every line's layout rather than relying on proximity. Cost ~1.6x
      generation time (972ms -> 1593ms on BH-Barrett), cached per
      work+fingerprint, so it runs once per work+geometry, not per load. The
      guard now compares two independently-DRIVEN validation passes (not two
      reads of one cache) via `heights_disagree`; on mismatch
      `record_prose_pages` returns `Err` and the caller falls back to the
      live engine. `pv7` -> `pv8` evicts every table generated under the
      race, since the fingerprint doesn't and can't encode a timing-
      dependent race.
    - Guard: `deep_ch37_prose_pages_never_overflow_the_card` (`tests/
      prose_page_fit.rs`) drives to ch. 37 and asserts render-side
      `CLIP_WARN`/`BOTTOM_CLIP_EXACT`. Verified failing before the fix (56
      OVERFLOW lines across 14 pages on a fresh regeneration) and passing
      after (0 overflow lines, fresh regeneration confirmed, `pv8`).
    - Diagnostics: `LIT_TRACE_HEIGHTS=<a>:<b>` for the three-way
      GEN/POSTWALK/RENDER diff (the vector that exposed the +225px);
      `LIT_TRACE_PANGO=1` for an independent measure.
    - Dead ends — record these so nobody re-walks them: Pango is NOT a
      drop-in oracle here either — it disagrees with `line_yrange` even on
      DISPLAYED lines, under-counting wrapped rows by a margin that scales
      with row count, and two corrections were measured and refuted
      (per-line `dialogue-indent` tag margins — the tag never applies to
      block-aware works like BH-Barrett; and view-level line spacing zeroed
      in favour of tags — real, but per-paragraph, so it cannot produce a
      row-scaling deficit). Also: comparing a pre-validation sweep against a
      post-validation sweep is NOT a usable guard — it disagreed on
      1283/7300 lines on every cold generation, because the pre-validation
      sweep IS the lazy estimate that validation exists to correct, so
      "disagreement" cannot distinguish a working fix from a broken one.
    - Known non-fix for THIS bug (do not re-try it): item 22's font-in-
      effect preamble alone. It is real and necessary for its own bug, but
      it addresses a different mechanism (a stale face, not an unvalidated
      viewport-distant line) and does not stop the race.

24. **MAIN CARD, single-column PROSE — the FIRST row crowds the running head
    while the FOOT pools unused cream. NOT a clip; a head/foot BALANCE bug,
    and the two symptoms are ONE defect.**
    - Tell: the user reports "not enough breathing room at top, too much at
      bottom." Measured on four user-labelled captures at 1920x1200: the
      REJECTED pages cleared the running-head labels by **6px**, the
      "about right" ones by **25-31px**, while 60-93px sat unused at the foot.
    - **The top gap the user means is NOT the card's outer margin.** Window
      edge -> card top, and card top -> header ink (17px), were CONSTANT across
      every capture, good and bad alike. Only `hdr -> first body row` varied.
      Measure THAT before touching `CARD_MARGIN_TOP` — the outer margin looks
      like the obvious knob and is the wrong one.
    - Root cause: `TOP_SPACER_HEIGHT` 74 -> 44 (2026-07-28) overshot. It only
      bites when a page's first row is a PARAGRAPH CONTINUATION — a row with
      tall ascenders and no paragraph gap above it. A page opening on a
      heading or a paragraph start looks fine at the same constant, which is
      why the regression shipped.
    - Fix (2026-08-05): `TOP_SPACER_HEIGHT` 44 -> 58, with BOTH bottom reserves
      52 -> 38. Keep the three in step: `top_spacer` is a SIBLING above the
      text view, so +14 to it takes 14 off `text_view.height()`, and -14 to the
      reserve restores `usable = view_h - guard - BOTTOM_MARGIN` exactly. Row
      grid preserved, so no pinned page table changes shape.
    - **`SINGLE_COLUMN_BOTTOM_MARGIN` is NOT a whitespace knob on its own.**
      Measured: trimming it alone moved the visible foot gap the WRONG way
      (61 -> 68). Freed pixels only surface if they buy a WHOLE extra row;
      otherwise they reappear as residue. It works ONLY paired with the spacer.
    - **The strip's height and the head's position were ONE knob, and that is
      why the obvious fix backfires.** With the labels centred
      (`valign: Center`) they ride the strip's midpoint, so growing the strip
      moves the running head down along with the text. Measured: at
      `TOP_SPACER_HEIGHT` 63 the label sat at y=48-57 instead of 41-50 and the
      visible head-to-text gap SHRANK **32 -> 25px** — the opposite of the
      intent — even though the body text itself moved down 13px.
    - **Fix (2026-08-05, follow-up): TOP-ALIGN the labels and split the knob.**
      `running_head_work` / `running_head_division` are now `Align::Start` with
      `RUNNING_HEAD_TOP_OFFSET` (20), chosen to reproduce their measured
      position at the moment they stopped being centred. `TOP_SPACER_HEIGHT`
      (58 -> 66, reserves 38 -> 30) now controls ONLY where the TEXT starts.
      Result: head label unmoved at y=48-57, first body row 82 -> 95, clearance
      **32 -> 38px**, row count unchanged at 34. Raise the offset to move the
      HEAD; raise the spacer to move the TEXT. Do not "simplify" the labels
      back to `Align::Center` — that re-couples them.
    - Scope note: the running-head strip is built once in `build_window` and is
      MAIN-CARD only; the overlays keep their own head pair
      (`division_synopsis.rs`), and `card_focus_rule` lives in
      `page_turn_overlay` (hidden outside chat layout), so neither is affected.
    - Note `top_spacer_height` IS in the `play_pages`/`prose_pages`
      fingerprint, so tables self-heal (regenerate once per work on next open)
      even though usable height is unchanged. Expected, not a defect.
    - **Retracted claim — do not re-inherit it:** an earlier note held that
      `TOP_SPACER_HEIGHT` 44 -> 58 "BREAKS the synopsis overlay (2/2 fail,
      `TEST_OVERLAY_VIEWPORT_RECT` never appears)." It does not. Verified
      2026-08-05 by stashing the change: `overlay_clipping` fails **identically
      2/2 at the 44/52/52 baseline**, and nothing in the synopsis overlay reads
      the constant. It is a PRE-EXISTING `overlay_clipping` failure that a
      one-run comparison during harness flux mis-attributed to the edit.

25. **ANY OVERLAY CARD — no top/bottom padding on the FIRST open, correct on
    the second.** Tell: the card sits ~5px from the screen edge instead of
    `CARD_MARGIN_TOP`'s 14, and an Escape + reopen fixes it. Pixel-measure
    before believing it: on a 1920x1200 screen the correct card is **1172**
    (`1200 - CARD_VERTICAL_OUTER_MARGIN`), the bad one **1191** — 19px too
    tall, which is exactly the missing 9+10px of gap.

    Root cause: `main_card_rect` derives the height from inputs that are BOTH
    stale during the compositor's settle. This bug has now been fixed three
    times at successively deeper layers, so check them in order:

    - **The allocation is stale.** `content_hbox.height()` left over from
      before a resize is still `> 0`, so "`alloc_h > 0`" is not "settled"
      (measured: alloc 692 while the window was already 1236).
    - **The tolerance was too loose.** Accepting the allocation when within
      `CARD_VERTICAL_OUTER_MARGIN` (28px) of the window-derived height let the
      SAME bug back in at a different geometry: 1191 vs 1172 is only 19px off,
      inside 28. The tolerance was the very quantity being subtracted, so it
      could not tell "settled" from "one resize behind". Both inputs express
      the same quantity, so the check must be **equality**, not proximity.
    - **The WINDOW is stale too.** This is the one that finally explains a
      first-vs-second-open difference. Under the equality rule, 1191 is
      *unreachable* at `window_h = 1200` (every allocation yields 1172), which
      proves `window.height()` itself reported **1219** on the first call. The
      resize tick names the cause in `app/mod.rs`: "first open before dwl
      applies the final tile geometry." No rule over `(alloc, window)` can
      recover the right answer when both are pre-settle.

    Fix: the **monitor** height is the outermost authority
    (`settled_card_height_on` / `monitor_height` in `app/layout.rs`) — it is a
    property of the output, not of a window still being configured, so it does
    not fluctuate mid-settle. `monitor_h <= 0` means "unknown" and falls back
    to the window/allocation rule.

    Generalizable lesson: **a first-open/second-open difference means an input
    is pre-settle, and the fix is to find an input that cannot be** — not to
    widen a tolerance around the fluctuating one. Tolerances hide this class of
    bug rather than fixing it; each widening here bought one geometry and
    failed at the next.

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
  `CLIP_WARN: main-card {prose-1col|two-col} OVERFLOW total=… > widget_h=…`
  (#12) and `CLIP_WARN: main-card single-col clip=… >= widget_h=…` (#7).
  **The surface label was hardcoded `two-col` until 2026-07-27** even though the
  exact-clip path also serves single-column prose row-fill pages — a prose
  overflow therefore read as a play-pagination problem and sent one
  investigation to the wrong engine. The line now also carries `top_off=` and
  `bottom_head=`, which is usually enough to classify the failure without
  reading any code (see below).

### An OVERFLOW an ORDER OF MAGNITUDE over `widget_h` is a WIDTH bug

Check the ratio before reading any pagination code. A genuinely overfull page
overshoots by tens or low hundreds of px. A `total` that is 5-20x `widget_h`
is not a pagination fault at all — the text is being laid out against a
near-zero WIDTH, so every word wraps onto its own row and the height explodes:

```
CLIP_WARN: main-card prose-1col OVERFLOW total=8566 > widget_h=1128 ... end=345
```

17 lines measuring 8566px (2026-08-03, BH-Barrett). At ~28px/row that should
have been ~480px. The page table was correct and reported `table hit`; the card
had collapsed to 281px on a 1920px window.

**The tell that separates it from every other clip failure:** the window
geometry in the log is healthy (`RESIZE_TICK: vbox.width … -> 1920`) and
`CARD_SIZING` reports a correct `card_w=1050`, yet the card on screen is a
narrow strip. `CARD_SIZING` logs what the code INTENDED, not what was drawn —
never treat it as confirmation that the card is that wide.

**Root cause (2026-08-03):** `apply_card_sizing` computed `card_w` and then
discarded it, calling `set_width_request(-1)`. Nothing else in the chain from
`content_hbox` down to the text view set a width, and a `WrapMode::Word`
`GtkTextView`'s minimum width is only its margins (`gtk_text_view_measure`
never consults the text layout horizontally — it is NOT "the widest word", a
plausible-sounding guess that sends you looking at the text instead of the
widget tree). Fix: request the window-clamped `card_w` on `content_hbox`.

**Verify in pixels, not logs.** The oracle is `TEST_VIEWPORT_RECT` (the real
allocation) or a screenshot — see `tests/card_width.rs`, which asserts the
viewport width lands in a BAND, because the opposite failure (the card filling
the whole window) is equally real and a one-sided floor misses it.

### A `total > widget_h` OVERFLOW is not always an overfull page

Before hunting for a stale/overfull stored page, check whether `total` is
simply being MEASURED wrong. A prose row-fill page may straddle a paragraph at
either edge, and each straddling line contributes only its on-page part:

- `top_off=N` — the page starts N px INTO its first line.
- `bottom_head=Some(N)` — the page ends N px into its LAST line.

`exact_page_content_height` (scroll.rs) subtracts both. When it did not, page
113 of BH-Barrett measured 1175px against a 1098px widget and fired this
warning, while its TRUE occupied height was 1065 — comfortably inside
`usable=1071`. The stored page was fine all along; `validate_prose_pages`
accepted it correctly because its `page_px` already measured it this way.

**The decisive check:** compare the clip's `total` against `page_px` for the
same stored row. If `page_px` fits `usable` but `total` does not, the bug is in
the RENDER-side measurement, not the page table. Chasing the table instead
costs a session — that is exactly what happened here.

#### The same class again: BOX height vs INK (2026-08-11)

`prose_pages_keep_bottom_breathing_room` failed with "41 of 779 stored prose
pages are packed past the fill budget usable=1071px ... the reader sees text
touching the edge," and its own message prescribed fixing the boundary
decision in `prose_next_boundary`. **Both the diagnosis and the prescription
were wrong**, in the same way as the case above: the page was measured wrong,
not packed wrong.

The tells, all three cheap to check before touching any boundary code:

- **Every over-budget page had `end_off == 0`** (read it straight off the
  `PAGES_PROSE_DRIFT: over page N (a,b)..(c,d)` lines — `d` is `end_off`).
  `end_off == 0` means the page ends ON a paragraph boundary, so NO page held
  a partial row of ink. A genuine over-pack shows straddles.
- **Every overshoot was 1..=6px** — exactly one `pixels_below_lines` at
  `line_spacing` 6. An overshoot smaller than one text row (here 38px minimum)
  cannot be an extra row of text.
- **Production accepted every one of them.** `validate_prose_pages` tolerates
  `usable + fit_slack` (14px here) and passed. Only the census, comparing
  against bare `usable`, called them overfull.

Root cause: `log_generation_height_drift` charged the page its last line's
FULL box height, including the trailing `pixels_below_lines` that paints no
ink. `fit_slack`'s own doc already says the tolerated excess is
"trailing/leading spacing only, never glyph ink" — the census just wasn't
honouring it. Fix: `page_ink_charge` sheds that trailing gap when
`end_off == 0`, while the straddle branch still charges its on-page head in
full so real overflow still trips. Guarded by
`page_ink_charge_sheds_trailing_gap_but_never_ink`.

**Generalisation:** when a fit check fails by LESS than one row height, suspect
the measurement's treatment of spacing before suspecting the boundary. Moving
boundaries to satisfy a box-height measurement would have pulled a perfectly
fitting glyph row off 62 pages — a real regression, produced by "fixing" a
non-bug.

**Beware a failing test's prescription.** This test's assertion message named
the wrong subsystem with total confidence. An assertion message is a hypothesis
written when the test was authored, not a diagnosis of the run in front of you.

### A SILENT clip-class failure the tripwire cannot catch

`CLIP_WARN` only fires when clip MATH diverges. An off-grid landing (#20) has
perfectly consistent math — the reader simply isn't on a stored page, so
row-fill is the correct fallback and nothing warns. **An empty
`rg CLIP_WARN` does not clear the clip surfaces when the complaint is
"the page is framed wrong" rather than "a glyph is sliced."** For that class,
grep the engine instead:

```
rg "BOTTOM_CLIP_(EXACT|ROWFILL)|PAINT: first frame" linux-lit-dev.log | tail -20
```

A `ROWFILL` line following a run of `EXACT` lines on the same work marks the
moment the reader fell off the grid, and the preceding `PAINT` line names the
page top that did it.

## Verifying

**First, and after any clip fix, run the pure clip-invariant unit tests — they
are NOT run automatically (no CI / git hook here), so run them by hand:**

```bash
cargo test --bins
```

They compile into the binary (no display, no `cage`, ~seconds) and guard the
arithmetic invariants a clip fix must not regress — e.g.
`pinned_view_height_reserves_descender_room_at_zero_spacing` /
`descender_pad_scales_and_never_collapses` (checklist #14) assert the paginated
overlay always reserves descender room below the last line. They prove the
FORMULA is right, not that a specific glyph cleared on screen — that last mile is
still the pixel e2e + the real display below.

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

**Detector calibration (scripts/check_line_clipping.py):** three lessons from the
descender-allowance work. (1) Row segmentation merges runs separated by ≤2px —
a descender tip tapers so thin that its connecting rows fall under the 1%-width
ink threshold, and the detached tip read as a fake 1px "clipped row" the moment
the clip stopped covering real descender ink. (2) A short EDGE row counts as
clipped only if it is also shorter than every interior row — a complete
0.75-scale speaker label at the page top is legitimately under the body-text
median. (3) **A short EDGE row is a clip only if it sits within `--edge-tol`
(default 8px) of the pane edge.** A genuinely clipped line is short *because the
edge cut it*, so it is flush against the edge; a detached descender tip can land
far above it (a real case: a 1px sliver 35px above the bottom edge, gap 3px so
lesson (1)'s ≤2px merge did not fuse it, `median 22`, so it tripped `short_bottom`
as a fake `last_h=1` BOTTOM clip). The `min_margin` edge-touch rule still flags a
row with zero background margin regardless of height, so the guard cannot mask a
real edge clip. The decision now lives in the pure `decide_clip()` helper, unit-
tested by `scripts/test_check_line_clipping.py` (run `python3
scripts/test_check_line_clipping.py`). All three were detector false positives
that only surfaced once production rendered MORE of the glyphs, i.e. "when a clip
e2e fails, first ask whether the assertion is measuring the pre-fix rendering."

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
- `docs/superpowers/specs/2026-06-25-clip-prevention-design.md` — the unification
  design.
