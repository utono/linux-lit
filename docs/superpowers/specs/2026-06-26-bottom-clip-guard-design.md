# BottomClipGuard: self-wiring clip lifecycle for free-scroll surfaces

**Date:** 2026-06-26
**Status:** Design approved

## Problem

The journal Q&A overlay clips a partial last text row behind the ask card / above
the footer. Root cause (confirmed by exhaustive gloss-vs-journal comparison):

The 2026-06-25 clip-prevention refactor shared the clip **math**
(`bottom_clip_height` / `display_rows` / `recompute_overlay_bottom_clip` in
`src/ui/mod.rs`) but left each surface to hand-wire **WHEN** to call it. The
gloss overlay (which clips correctly) wires FOUR mechanisms:

- (a-range) a `connect_changed` handler on the vadjustment (250ms after each
  open) for multi-pass open-time layout — inside `reset_scroll_top`
  (`gloss_overlay.rs`).
- (a-idle) an `idle_add_local_once` backstop, also in `reset_scroll_top`.
- (b) direct `recompute` calls in `scroll_gloss` / `_to_top` / `_to_bottom` /
  cursor-into-view.
- (c) **a persistent `vadjustment().connect_value_changed` handler that calls
  `recompute_overlay_bottom_clip`** (`gloss_overlay.rs:291-294`) — the catch-all
  for every scroll AND every layout-driven value/page_size change (including the
  ask card resizing the viewport).

The journal overlay wired only (b) plus a single open-time idle. Its
`connect_value_changed` handler (`journal_overlay.rs:147-150`) redraws ONLY the
selection bar — it never recomputes the clip. So when the ask card opens (which
shrinks the scrolled viewport) or any layout pass settles after the first show,
the clip keeps its stale height and the partial last row pokes through.

**The deeper problem:** clip-prevention is modular at the MATH level but not at
the LIFECYCLE level. A surface assembles 3-4 independent wiring steps by hand and
can silently drop one. The journal bug, the translation overlay's original
no-clip-box gap, and the journal's earlier descender bug are all the same class:
incomplete hand-wiring of a shared mechanism.

## Goal

Make clip-prevention modular at the LIFECYCLE level: a single self-wiring unit a
surface attaches once, which owns ALL recompute paths, so a surface **cannot**
assemble a partial set. Migrate gloss (the reference), journal (the bug), and
translation onto it, and extend the pixel clip-invariant test to the journal and
translation overlays so a dropped path is caught by CI.

## Design

### `BottomClipGuard` (new: `src/ui/bottom_clip_guard.rs`)

A small struct, constructed once per surface, that owns the clip Box and every
recompute path. The clip MATH stays in `mod.rs`; the guard owns the WIRING.

```rust
pub(crate) struct BottomClipGuard {
    view: gtk4::TextView,
    clip: gtk4::Box,
    scrolled: gtk4::ScrolledWindow,
    // a flag cell for the open-time range-pin, mirroring reset_scroll_top
    pinning: std::rc::Rc<std::cell::Cell<bool>>,
}

impl BottomClipGuard {
    /// Build the clip Box (valign=End, halign=Fill, vexpand=false,
    /// can_target=false, css "gloss-bottom-clip", height_request=0), add it to
    /// `scroll_overlay` with measure=false / clip=true, AND wire path (c): a
    /// persistent `scrolled.vadjustment().connect_value_changed` that calls
    /// `recompute_overlay_bottom_clip`. Returns the guard.
    ///
    /// After attach(), path (c) can never be forgotten — it is inside attach().
    pub fn attach(
        scroll_overlay: &gtk4::Overlay,
        view: &gtk4::TextView,
        scrolled: &gtk4::ScrolledWindow,
    ) -> Self { … }

    /// The clip Box, e.g. so the caller can add OTHER overlays (a selection bar)
    /// after it and control stacking order.
    pub fn clip(&self) -> &gtk4::Box { &self.clip }

    /// (b) Recompute now — called from the named scroll methods
    /// (scroll/_to_top/_to_bottom).
    pub fn recompute(&self) {
        crate::ui::recompute_overlay_bottom_clip(&self.view, &self.clip, &self.scrolled);
    }

    /// (a) Open-time coverage: snap-to-top pin + a one-shot `connect_changed`
    /// (range-change) handler that recomputes on each layout pass and
    /// self-disconnects after ~250ms, PLUS an `idle_add_local_once` backstop.
    /// Mirrors gloss's `reset_scroll_top`. Call from every show/open path.
    pub fn on_open(&self) { … }
}
```

`attach()` is lifted VERBATIM (in effect) from gloss's current inline wiring —
gloss is the known-good reference, so the guard reproduces its four mechanisms
exactly. The only behavior change anywhere is that journal and translation GAIN
the paths they were missing.

### Box-child variant

The translation overlay's scrolled child is a widget **Box**, not a TextView, so
it uses `recompute_overlay_bottom_clip_box` (slack-only, no wrapped partial row).
Provide a parallel `BottomClipGuard::attach_box(scroll_overlay, scrolled)` that
wires path (c) to call `recompute_overlay_bottom_clip_box`, with `recompute()` /
`on_open()` routed to the box variant. (Same lifecycle, different math fn.)

### Migration

1. **Build the guard from gloss's wiring**, then **migrate gloss first** — replace
   its inline clip-box creation, the value_changed→recompute handler, and the
   `reset_scroll_top` range/idle handlers with `attach()` + `on_open()` +
   `recompute()`. This MUST be behavior-preserving (gloss is the reference); verify
   on the real display it still clips top+bottom correctly.
2. **Migrate journal:** replace the hand-wired clip box, the bar-only
   value_changed handler's clip responsibility (the bar redraw STAYS — it's a
   separate concern; only the missing clip recompute is added via the guard's
   path c), the `update_bottom_clip()` calls, and `schedule_bottom_clip_recompute`
   with the guard. Journal gains path (c) + on_open — **this fixes the bug.** The
   selection-bar `queue_draw` handler remains as its own `connect_value_changed`
   (two handlers on the same signal is fine).
3. **Migrate translation** to `attach_box` + the box recompute.

### Test (defense-in-depth)

Extend the pixel clip-invariant e2e tests (currently `tests/line_clipping.rs`
main card + `tests/overlay_clipping.rs` synopsis) to the **journal** and
**translation** overlays: open each, scroll to the bottom, assert no partial
last row. The journal test must open the ask card (the resize that exposed the
bug) and assert no clip with it open. This way a future dropped path fails CI,
not a user screenshot.

## What stays separate (do NOT fold into the guard)

- The MAIN reading card's **paginated** clip (`scroll.rs::update_bottom_clip`) —
  a different strategy (clips at a computed page boundary, not the live partial
  row). Untouched.
- The clip **math** in `mod.rs` — the guard CALLS it; it does not move into the
  guard. Keep `bottom_clip_height` pure and unit-tested where it is.
- The **selection bar** `value_changed` handler in journal — a separate concern
  (redraws the visual bar); it stays, alongside the guard's clip handler.
- The **top-edge line-snapping** (`snap_value_to_line` etc.) — a separate
  mechanism (no mask); not part of this guard. (A future guard could own it too,
  but that is out of scope here — keep this change to the bottom clip.)

## Acceptance / verification

- Journal Q&A: open Cromwell, open the journal, press A — the answer text stops
  cleanly above the ask card (no partial row behind it). Escape — no partial row
  above the footer. (The reported bug, gone.)
- Gloss synopsis (`h`): scroll with `j`/`k` — both edges show whole lines
  (unchanged from today — gloss is the reference, migration is behavior-preserving).
- Translation (`i`): page through — no clipped partial row.
- `cargo test --bins` green; the extended e2e clip tests pass under
  `./scripts/e2e-env.sh`.
- A grep shows journal/translation no longer hand-wire a clip box or a bare
  value_changed-without-recompute — they go through `BottomClipGuard`.

## Files

- **New:** `src/ui/bottom_clip_guard.rs` — `BottomClipGuard` (attach / attach_box
  / clip / recompute / on_open).
- `src/ui/mod.rs` — `pub mod bottom_clip_guard;` (math helpers unchanged).
- `src/ui/gloss_overlay.rs` — migrate to the guard (behavior-preserving reference).
- `src/ui/journal_overlay.rs` — migrate to the guard (gains path c + on_open →
  fixes the bug); keep the selection-bar redraw handler.
- `src/ui/translation_overlay.rs` — migrate to `attach_box`.
- `tests/` — extend the clip-invariant e2e to journal + translation (ask card open).
- `docs/troubleshooting/clip-prevention.md` — add a "BottomClipGuard owns the
  three paths" note pointing the failure checklist at the guard.
