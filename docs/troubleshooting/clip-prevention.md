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
  overlay whose scrolled child is a widget **Box**, not a TextView (the
  translation overlay's column stack). A Box never splits a child across the
  edge, so there is no partial wrapped row — it only covers trailing slack when
  the content ends inside the viewport.

**Per-row geometry is mandatory for prose, never a uniform row-step.** The
synopsis/gloss/journal buffers join paragraphs into single multi-row buffer
lines with per-tag `pixels_above_lines`/`scale`, so rows are NOT uniform.
`line_yrange` (logical-line granular) collapses a wrapped paragraph to one
paragraph-tall "row" and clips the wrong amount; a uniform `step` estimate cuts
the last line's descenders. The journal overlay's original descender bug was
exactly this — it used a `line_yrange` row-step before the unification.

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
wire path (c) in one call; `on_open()` is path (a); `recompute()` is path (b).
The gloss, journal, and translation overlays all attach a guard. When adding a
new free-scroll surface, attach a guard — do not hand-wire the handlers. Failure
checklist item #1 below becomes "confirm the surface attaches a BottomClipGuard."

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

### The fix (DONE): `AskCardHost` + fixed-scroll-height

Status: **fixed** on `fix/ask-card-host` (Tasks 1-5). Both the journal Q&A and
gloss synopsis/add-edit ask cards now shrink the scroll viewport on open so the
reading text ends ABOVE the card. The mechanism and the shared host:

**Fixed-scroll-height.** Turn the scroll's `vexpand` **OFF** and set its height
EXPLICITLY. With vexpand off there is no vexpand-vs-container fight to race
(the earlier `set_height_request(cur - ask_nat)` attempts raced and were
rejected). The closed height is `card_height − fixed_chrome − footer`; on open
it becomes `card_height − fixed_chrome − ask`, so the scroll deterministically
yields the ask card's slot. Proven on hardware: ask-open `page_size` (~817) is
smaller than ask-closed (~1025), consistently across repeated open/close.

**`AskCardHost` (`src/ui/ask_card.rs`)** owns the lifecycle so neither overlay
hand-wires it: `size(card_width, card_height, fixed_chrome_h, footer_h)` records
the geometry and sets the closed scroll height; `open(title, hint)` shrinks the
scroll + hides the toggled footer (if any) + recomputes the clip (sync + idle);
`close()` restores the STORED closed height (not a re-measure — the footer's
`preferred_size()` reads 0 right after it is re-shown) + shows the footer +
recomputes. It composes the existing `BottomClipGuard` via a boxed recompute
closure (the guard isn't `Clone`).

- `fixed_chrome_h` = all non-scroll, non-ask chrome that STAYS visible while the
  ask card is open. Journal: just the title (its nav footer IS the toggled
  `footer`, so not counted here). Gloss has NO toggled footer (its hint row
  stays put), so it folds the footer into `fixed_chrome_h` and passes
  `footer_h = 0`; the chrome varies by gloss show mode (synopsis/result =
  title + footer; echoes = source header + rule + footer; glossing-loading =
  title only).
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
- **Box-slack guard** (`recompute_overlay_bottom_clip_box`) — the translation
  column stack. No wrapped partial row; covers only trailing slack.

Likewise the top-snap algorithms differ (`snap_value_to_line` per-`display_rows`
row vs scroll-mode's `snap_value_to_line_top` via `line_at_y` vs uniform
`row_step` rounding) — not duplicates. See
`docs/superpowers/specs/2026-06-25-clip-prevention-design.md`.

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

## Verifying

Real GTK pixel layout is what matters; the headless `cage` + `grim` flow lays
out fonts/metrics differently and can confirm the mechanism RUNS and roughly
looks right but cannot prove pixel-exact edges. Confirm on the real display:
open the surface, scroll to the bottom, and check the bottom edge shows only a
whole line. The pixel-level e2e invariants are `tests/line_clipping.rs` (main
card) and `tests/overlay_clipping.rs` (synopsis overlay), both `#[ignore]`d and
run via `./scripts/e2e-env.sh cargo test --test line_clipping --test
overlay_clipping -- --ignored --nocapture`.

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
  clip, NOT this algorithm), `scrolloff_bottom_clip_widgets` (scroll-mode, routed
  through the shared helper), `snap_value_to_line_top`.
- `src/theme.rs` — the `.gloss-bottom-clip` background CSS.
- `docs/troubleshooting/page-turning-mechanics.md` — the paged clip + pagination.
- `docs/superpowers/specs/2026-06-25-clip-prevention-design.md` — the unification
  design.
