# Clip-prevention: share the free-scroll covering math + close coverage gaps

**Date:** 2026-06-25
**Status:** Design approved

## Problem

Line-clipping bugs (the first/last line of a text surface cut off at an edge)
keep recurring because clipping prevention is spread across multiple
implementations and the test invariant covers only one surface. A clipping
audit (2026-06-25) mapped the surface and found:

- **Two genuinely different clipping STRATEGIES**, which must stay separate:
  - **Paginated** (main reading card, `text_view`/`right_view`): clip exactly at
    a computed boundary line by summing `line_yrange` heights from `page_top`,
    with descender-guard, `BASE_BOTTOM_MARGIN`, column-split (`exact_end`), and
    one-section-per-page logic (`scroll.rs::update_bottom_clip`). Correct as-is.
  - **Free-scroll** (overlays + scroll-mode j/k): mask whatever partial *visual*
    row straddles the viewport bottom (`ui::mod::bottom_clip_height`).
- The free-scroll algorithm is **duplicated verbatim**: `bottom_clip_height`
  (mod.rs:94-114) and `scrolloff_bottom_clip_widgets` (scroll.rs:1108-1138) are
  the same `last_full_bottom`/`any_full`/`effective_bottom`/single-row-guard
  block down to the `bottom_y + 0.5` epsilon. The only legitimate difference is
  the row source: `scrolloff` walks `line_yrange`/`forward_line` (logical lines),
  the overlays use `display_rows` (visual rows). A bug fixed in one is not fixed
  in the other.
- The **translation overlay scrolls but has no bottom-clip box** at all
  (`translation_overlay.rs` only sets a fixed `card_height`) — a clip bug waiting
  to happen when content exceeds the card with a straddling last row.
- The **clip-invariant test covers only the main card**. `tests/line_clipping.rs`
  explicitly scopes out overlays; `nav_test.rs::clip_violation` checks only the
  two main columns. Gloss/journal/synopsis/echoes/translation have zero clip
  tests, so even the shared "good" path can regress silently.

## Explicitly NOT in scope (investigation corrected the audit's premise)

- **Main-card unification.** The main card is paginated, not free-scroll; forcing
  it onto `bottom_clip_height` would replace boundary-clipping with
  scroll-clipping — a behavior change, not a dedup. Different coordinate spaces
  (`line_yrange` vs `iter_location+top_margin`) and pagination concerns
  (`exact_end`, sections) make them non-interchangeable. Left as-is.
- **Shared `snap_value_to_line`.** The gloss and journal overlay versions are
  DIFFERENT algorithms, not duplicates: gloss snaps to the greatest real
  `display_rows` row-top ≤ target; journal does `(value/row_step).round()*row_step`
  uniform-grid rounding (its own doc notes the grid approach is the weaker one).
  Converging them is a behavior change, not a safe dedup. Left as-is. (A separate
  "journal should adopt gloss's per-row snap" improvement could be filed, but it
  is behavior-changing and out of scope here.)
- The ~8 copies of `usable_height = widget_height - descender_guard -
  BASE_BOTTOM_MARGIN` — pagination-internal, threads through `visible_range`/
  `trim`/`last_*_visible_line`; out of scope for this clip-prevention pass.

## Part A — Share the free-scroll covering math

Extract a logical-line row producer and route `scrolloff_bottom_clip_widgets`
through the existing pure `bottom_clip_height`, so scroll-mode and the overlays
run ONE covering algorithm.

- **New (src/ui/mod.rs):** `line_yrange_rows(view: &gtk4::TextView, top_val: f64)
  -> Vec<(f64, f64)>` — the logical-line analog of `display_rows`: walk from the
  line at `top_val` via `line_at_y` + `forward_line`, emit each line's
  `(row_top, row_bottom)` from `line_yrange` in vadjustment space, stopping once
  `row_top >= top_val + viewport`. (Producer mirrors the existing loop at
  scroll.rs:1110-1125; it must reproduce the same rows that loop visited.)
- **Change (src/input/scroll.rs):** `scrolloff_bottom_clip_widgets` keeps its
  signature and its early `viewport_h <= 0` guard, then computes
  `let rows = crate::ui::line_yrange_rows(text_view, top_val);` and
  `let clip_h = crate::ui::bottom_clip_height(&rows, top_val, viewport_h, adj.upper());`
  and sets `bottom_clip.set_height_request(clip_h)` (guarded by the existing
  `!=` check). The inline `last_full_bottom`/`any_full`/guard block is deleted.
- **Behavior-preserving:** `bottom_clip_height` already encodes the identical
  algorithm; feeding it `line_yrange`-derived rows reproduces the current result.

### Testing A
- `bottom_clip_height` is already thoroughly unit-tested (mod.rs `bottom_clip_tests`).
- Add a unit test for `line_yrange_rows` is NOT possible without a realized
  TextView (it needs GTK layout) — so the parity is verified by the existing
  overlay clip behavior plus the Part C harness. State this; do not fake a unit
  test that asserts nothing.
- `cargo test --bins` stays green; the change is a pure refactor of an internal
  helper.

## Part B — Translation overlay bottom-clip guard

Give the translation overlay the same free-scroll clip the gloss/journal
overlays have.

- **Change (src/ui/translation_overlay.rs):** add a `bottom_clip: gtk4::Box`
  field, append it as an overlay/clip box over the scrolled content the same way
  gloss/journal do (a zero-height box pinned to the bottom that grows to mask the
  partial row), and connect the scrolled window's `vadjustment` `value_changed`
  to call `crate::ui::recompute_overlay_bottom_clip(&view, &bottom_clip,
  &scrolled)`. Also recompute once on reveal (after content + size settle).
- **Mirror the open-time/scroll-time rule** (page-turning-mechanics.md:871-880):
  recompute on every `value_changed`, not only on named scroll methods, so the
  clip can't keep a stale open-time height.
- **Behavior-adding, not preserving:** this introduces a clip where there was
  none. **Render check required** (see Verification).

### Testing B
- No pure unit test (geometry-only). Covered by the Part C harness once the
  translation overlay is reachable in the test, and by the manual render check.

## Part C — Extend the clip-invariant test to overlays

Make the no-clip invariant ENFORCED on at least one overlay, not just the main
card — this is the structural prevention against silent regression.

- **New emission (src/input/scroll.rs or the overlay module):**
  `emit_test_overlay_viewport_rect(...)` mirroring `emit_test_viewport_rect`
  (scroll.rs:815) — under `LIT_HEADLESS_TEST`, log `TEST_OVERLAY_VIEWPORT_RECT x
  y w h` for the open overlay's scrolled viewport (window == screenshot coords).
- **New test (tests/overlay_clipping.rs):** modeled on `tests/line_clipping.rs` +
  `tests/harness/mod.rs`. Launch headless (cage), open the synopsis overlay with
  `h` (per the headless-verification notes: advance into a chapter first so a
  synopsis exists), wait for `TEST_OVERLAY_VIEWPORT_RECT`, scroll to the bottom
  with `j`, and run `assert_no_line_clipping` against the overlay region. `#[ignore]`d
  like the other e2e tests so a bare `cargo test` stays green.
- **Harness (tests/harness/mod.rs):** parse the new rect line alongside the
  existing one; reuse `assert_no_line_clipping`/`check_line_clipping.py` unchanged
  (the pixel detector is already region-parameterized).

### Testing C
- This IS a test. It runs via `./scripts/e2e-env.sh cargo test --test
  overlay_clipping -- --ignored --nocapture`. Per CLAUDE.md the agent often
  cannot launch cage (seat owned by the live dwl session); the USER runs the e2e
  command and pastes the result/screenshot.

## Verification

- `cargo build` + `cargo test --bins` green after A (pure refactor) and the
  non-GUI parts of B/C; clippy warning count unchanged.
- **Render checks the agent cannot do (flagged):**
  - Part A: scroll-mode (j/k) on a long page and the translation-follow path —
    confirm the bottom partial row is still masked identically (no regression).
  - Part B: turn translations on, scroll a column to its last line — confirm no
    descender clip AND that nothing previously visible is now hidden by the new
    clip box.
  - Part C: `./scripts/e2e-env.sh cargo test --test overlay_clipping -- --ignored
    --nocapture` — the user runs it; a green run is the enforced invariant.
- After any e2e run, open every PNG in `target/ui/` and report on-screen text +
  any clipping by eye (UI review protocol, CLAUDE.md).

## Files

- `src/ui/mod.rs` — Part A `line_yrange_rows`.
- `src/input/scroll.rs` — Part A reroute `scrolloff_bottom_clip_widgets`; Part C
  `emit_test_overlay_viewport_rect`.
- `src/ui/translation_overlay.rs` — Part B clip box + `value_changed` recompute.
- `tests/overlay_clipping.rs` (new), `tests/harness/mod.rs` — Part C.

## Why this prevents the bug class

After A, the free-scroll covering math exists in exactly ONE place
(`bottom_clip_height`) — scroll-mode can no longer drift from the overlays. After
B, every scrolling surface has a clip guard (no silent gap). After C, the no-clip
invariant is enforced on an overlay, so the shared path can't regress unnoticed.
The paginated main card stays on its own correct algorithm — unifying it was the
unsound part of the original idea, and is deliberately excluded.
