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
the ask card does **not** shrink the scrolled viewport (proven by runtime diag:
`page_size` stays constant across the open, sync and idle) — the ask card
overflows the fixed-height card container and overlaps the bottom of the scroll
area, so the lower text rows render *behind* it, fully visible-but-occluded.
There is no viewport resize to react to and no partial edge row to mask, so NO
clip recompute (path a/b/c) can fix it. **If text shows behind a card whose
opening did not change `page_size`, the bug is layout/occlusion, not clipping —
the fix is to make the overlapping widget claim real layout space so the scroll
viewport shrinks to end above it.**

### Investigation log: the journal/gloss ask-card occlusion (UNFINISHED — for a fresh session)

Status as of 2026-06-26: a WORKING mechanism for the OPEN path is found and
proven by runtime numbers; the CLOSE path and the generalization to a shared
`AskCardHost` + gloss are NOT done. Branch: `fix/ask-card-host` (off master).
Specs/plans: `docs/superpowers/specs/2026-06-26-ask-card-host-and-shrink-fix-design.md`,
`docs/superpowers/plans/2026-06-26-ask-card-host.md`.

**The widget tree (journal_overlay.rs `new`).** `overlay` →
`attach_overlay_panel` adds `scrim` + `container` as overlays with
`set_measure_overlay(container, false)`. `container` (`valign=Center`,
`set_size_request(card_width, card_height)` where `card_height =
content_hbox.height()` ≈ 1075, a *minimum*) is a vertical box: `title →
scroll_overlay(ScrolledWindow, vexpand=true) → footer → ask.container()`. The
gloss overlay is structurally identical (same shared `AskCard`, same
`card_height` source, `set_height_request` instead of `set_size_request` — same
effect), so gloss has the SAME latent bug.

**Why it occludes (root cause, proven).** Because the container is added
**non-measured**, the Overlay hands it the full window height; the `vexpand`
scroll fills it (`page_size` ≈ 1025). When `ask.open()` sets the ask card
visible, the box would need ~258px more; the container grows past its *minimum*
and (being `valign=Center`) the extra extends off-pane rather than shrinking the
scroll. **The scroll never yields — the ask card draws over the bottom ~258px of
unchanged text.** Diagnostic that proves it: log
`scrolled.vadjustment().page_size()` synchronously AND on `idle_add_local_once`
in `open_ask_card`; if both stay equal to the closed value, the viewport did not
shrink.

**Attempts (all run on real hardware; numbers are `page_size`):**

- **(c) value_changed clip recompute / the whole BottomClipGuard refactor.**
  WRONG TARGET — there is no resize to react to, so no clip recompute helps. (The
  BottomClipGuard work is independently good and merged as a refactor, but it does
  NOT fix this.) Confirmed: pressing A produced NO `value_changed` and `page_size`
  stayed 1025.
- **(2b) cap the scroll on open via `set_height_request(cur - ask_nat)`,
  `vexpand` left ON.** Shrank to 767 on FIRST open, but RACED: a later open
  logged `idle=1025` (cap lost the race against the overlay allocation). Reading
  `cur` from the live `scrolled.height()` is also stale on re-open. Intermittent
  clip on both open and close. REJECTED (timing-fragile).
- **(2c) `set_measure_overlay(container, true)` + `min_content_height(80)`.**
  INVERTED: with the container measured, the CLOSED state shrank
  (`close idle=817`) and the OPEN state stayed 1025 (still occluded). Broke the
  closed state. REJECTED.
- **(FIXED-SCROLL-HEIGHT — the one that WORKS on open).** Turn the scroll's
  `vexpand` OFF and set its height EXPLICITLY, so there is no vexpand-vs-container
  fight to race. In `size_card`: `scroll_h = card_height - title_pref - footer_pref`
  (closed reading height). In `open_ask_card`: `scroll_h = card_height -
  title_pref - ask_pref` (footer is hidden while asking, ask card takes its slot).
  Recompute the clip on open AND on an idle tick after the height lands. RESULT
  (proven): `open sync page_size=1025 set=817`, `open idle page_size=817` — the
  viewport shrinks deterministically and the on-screen text ends with a WHOLE line
  above the ask card (verified by screenshot, repeated open cycles consistent).
  **This is the mechanism to build on.**

**What's STILL BROKEN / TODO for the next session (higher effort):**

1. **Close path restores the wrong height.** After Escape the diag shows
   `close idle page_size=817` (should be ~1025) — the scroll stays shrunk, wasting
   ~200px of reading area until the next `show_page`. The close handler recomputes
   `scroll_h = card_height - title_pref - footer_pref`, but at that instant the
   footer was just re-shown and its `preferred_size()` may read 0 (or the restore
   races the relayout). Fix the close restore to deterministically return to the
   full closed height (consider: store the closed `scroll_h` in `size_card` and
   reuse the stored value on close, rather than re-measuring chrome that was just
   toggled).
2. **`vexpand=false` side effects unverified.** Confirm a SHORT journal answer
   (content < scroll_h) still fills/positions correctly with vexpand off and an
   explicit height — no gap below the text, no mis-centering of the card.
3. **Generalize into the shared `AskCardHost`** (per the plan) so GLOSS gets the
   same fix — gloss has the identical latent occlusion. The host should own:
   `open`/`close` doing the explicit scroll-height set + clip recompute (+ footer
   hide/show if a footer is registered), composing the existing `BottomClipGuard`.
   Then route journal AND gloss through it and delete the per-overlay copies.
4. **Re-derive `ask_pref`/`title_pref`/`footer_pref` robustly.** `preferred_size()`
   on a just-toggled widget is timing-sensitive; prefer a stable reserved slot
   height (a const or a measured-once value) over per-call `preferred_size()`.
5. The e2e guard `tests/journal_clipping.rs` (already in tree, `#[ignore]`d)
   asserts no occluded row with the ask card open — it should PASS once the fix is
   complete; use it (via `./scripts/e2e-env.sh`) as the regression gate.

**The diagnostic to re-add when continuing** (it is the only reliable signal,
since the agent cannot run the GUI — have the user run `cargo run` and paste it):

```rust
// in open_ask_card / close_ask_card, after the height set:
let sc = self.scrolled.clone();
crate::logging::log(&format!("ASKFIX open sync: page_size={:.0} set={}",
    sc.vadjustment().page_size(), scroll_h));
glib::idle_add_local_once(move || {
    crate::logging::log(&format!("ASKFIX open idle: page_size={:.0} scrolled_h={}",
        sc.vadjustment().page_size(), sc.height()));
});
```

SUCCESS = ask-OPEN idle `page_size` is SMALLER than ask-CLOSED, CONSISTENTLY
across repeated open/close cycles (the earlier attempts passed once then raced).

(Contrast for the value_changed catch-all below: the gloss overlay sizes its card
with `container.set_height_request` (a minimum that can grow); the journal used
`set_size_request` — same minimum semantics, not the differentiator. The real
differentiator is the non-measured container + vexpand scroll, addressed by the
fixed-scroll-height mechanism above.)

  **The gloss overlay has (c)** (`gloss_overlay.rs`, connected in `new()` right
  after `bottom_clip` is created, calling `recompute_overlay_bottom_clip`). A
  surface that copies only (a) and (b) but not (c) WILL clip the moment its
  viewport changes outside a named method. (This was the journal Q&A bug: its
  `value_changed` handler only redrew the selection bar and never recomputed the
  clip, so opening the ask card — which shrinks the viewport — left a stale clip
  and a half-line showed behind the ask card.)

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
5. **A new surface reserves no real layout space for an element below it.** If a
   card opens below the scroll area (ask card, footer) and the scroll area is
   `vexpand` with no recompute on the resize, the overflow renders behind it.
   Path (c) covers this; if it still clips, confirm the resize actually fires a
   `value_changed`/`changed` the handler is connected to.

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
- `src/ui/journal_overlay.rs` — the journal Q&A overlay (must mirror all three
  paths; its `value_changed` historically redrew only the selection bar).
- `src/ui/translation_overlay.rs` — the Box-child variant.
- `src/input/scroll.rs` — `update_bottom_clip` (the MAIN card's *paginated*
  clip, NOT this algorithm), `scrolloff_bottom_clip_widgets` (scroll-mode, routed
  through the shared helper), `snap_value_to_line_top`.
- `src/theme.rs` — the `.gloss-bottom-clip` background CSS.
- `docs/troubleshooting/page-turning-mechanics.md` — the paged clip + pagination.
- `docs/superpowers/specs/2026-06-25-clip-prevention-design.md` — the unification
  design.
