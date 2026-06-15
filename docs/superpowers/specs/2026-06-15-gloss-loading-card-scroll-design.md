# Gloss loading-card scrolling

## Problem

When the user glosses a passage, linux-lit shows a "Glossing…" loading card
(`GlossOverlay::show_glossing`) that renders the full source passage while the
LLM generates the gloss. For a long passage — e.g. York's speech in 2H6
(2.1) — the passage overflows the visible card and the lower lines are
unreachable: there is no way to scroll the loading card.

The result gloss (once it arrives) is navigable via the block cursor
(`j`/`k`/`gg`/`G` step between source/explication blocks, scrolling each into
view). The loading card has no blocks, so those keys no-op and the card stays
pinned at the top.

## Root cause

`show_glossing` renders the passage into the existing scrolling viewport
(`gloss_scrolled` ScrolledWindow + `gloss_view` TextView, inside
`gloss_scroll_overlay`) — the same viewport the result gloss uses — but clears
`self.blocks`. In `handle_gloss_key` (`src/input/keymap.rs`), `j`/`k` call
`cursor_next_block`/`cursor_prev_block`, which return early on an empty block
list. The page keys (`x`/`y`) and `gg`/`G`-to-extremes are not routed to a
plain viewport scroll in the loading sub-state.

## Key insight: the scroll machinery already exists

`GlossOverlay` already exposes direct-viewport scroll methods with correct
visual-row snapping and bottom-clip handling:

- `scroll_gloss(delta: i32)` — scroll ~3 line-heights per press, snapping the
  viewport top to a whole visual row and re-sizing the bottom clip.
- `scroll_gloss_to_top()` / `scroll_gloss_to_bottom()` — jump to the extremes.

These are **already used by the echoes overlay** (`handle_echoes_overlay_key`
routes `j`/`k` to `scroll_gloss(±1)`). The gloss loading card uses the same
viewport widgets, so the same methods apply with no new scroll code.

## Design

Gate the gloss-overlay navigation keys on whether the card has blocks, in
`handle_gloss_key` only. No changes to `gloss_overlay.rs`, `keymap.json`, or any
other handler.

**Loading state — `current_block()` is `None` (empty `blocks`):**
route navigation to plain viewport scrolling:

- `j` → `scroll_gloss(1)`
- `k` → `scroll_gloss(-1)`
- `x` (PageForward) → page-sized scroll forward
- `y` (PageBackward) → page-sized scroll backward
- `gg` → `scroll_gloss_to_top()`
- `G` → `scroll_gloss_to_bottom()`

**Result gloss — `current_block()` is `Some` (blocks present):**
behavior is unchanged. `j`/`k`/`gg`/`G` keep stepping the block cursor exactly
as today; `x`/`y` keep their current (no-op) gloss-overlay behavior.

### Why the empty-blocks gate

The loading card is exactly the state where `blocks` is empty and the scroll
overlay is visible (`show_glossing` and `show_loading_message` are the only
paths that clear `blocks` while the overlay is shown; `show_loading_message`
hides the scroll overlay entirely, so a scroll call there is a harmless no-op on
a zero-range adjustment). Gating on `current_block().is_none()` therefore:

- leaves the result-gloss block navigation completely untouched,
- needs no new state field to keep in sync,
- matches how the code already discriminates loading vs. result (every
  block-consuming action already checks `current_block()`).

### Page-scroll step

`scroll_gloss(delta)` steps ~3 line-heights. A page scroll needs a larger jump.
Two acceptable implementations (decide at implementation time, prefer the
smaller change):

1. Call `scroll_gloss` with a larger multiplier if a parameterized step is
   trivial to add, **or**
2. Add a thin `scroll_gloss_page(delta)` on `GlossOverlay` that steps by the
   viewport `page_size()` (snapped to a row, bottom-clip updated) — mirroring
   `scroll_gloss`'s snap/clip logic. This is a small, self-contained addition if
   reused; it does not constitute a refactor of the module.

Either way the top must remain row-snapped (no fractional top line clipped under
the title rule) and the bottom clip must be recomputed, exactly as
`scroll_gloss` already does.

## Keys (loading card)

| Key   | GTK name        | Action                    |
|-------|-----------------|---------------------------|
| j     | `j`             | scroll down ~3 lines      |
| k     | `k`             | scroll up ~3 lines        |
| x     | `x`             | page forward              |
| y     | `y`             | page backward             |
| gg    | `g` `g` (chord) | scroll to top             |
| G     | `G`             | scroll to bottom          |
| Esc/n | `Escape`/`n`    | cancel glossing (existing)|

(Table kept because it is the clearest form for a key→action mapping and fits
well under 80 columns.)

## Out of scope

- **Refactoring `gloss_overlay.rs` (3366 lines) or `actions/gloss.rs` (2074
  lines).** Both are large and `gloss_overlay.rs` is a legitimate
  split candidate (the four card modes + the bar-draw / block / buffer helpers
  are natural module boundaries), but that is a separate, dedicated effort. This
  change stays focused on the scrolling fix per the brainstorming guidance
  ("don't propose unrelated refactoring").
- Changing any keybinding's identity (no `keymap.json` or Ctrl+/ overlay change
  — `j`/`k`/`x`/`y`/`gg`/`G` keep their gloss-overlay meaning; they only gain a
  scroll behavior in the loading sub-state).

## Testing

- `cargo build` / `cargo test --bins` — pure-logic suite stays green (this
  change adds no pure-logic helpers; the gate is a key-routing branch).
- Runtime verification is **visual** ("the loading card scrolls"), so per the
  project's headless-test rule it must be confirmed by launching the app. The
  agent cannot reliably launch cage from the live dwl session, so the user will
  run the app, open a long-passage gloss, and confirm `j`/`k`/`x`/`y`/`gg`/`G`
  scroll the "Glossing…" card.
