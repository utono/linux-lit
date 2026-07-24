# Inactive-card cue: drop the dim, hide the accent bar instead

**Date:** 2026-07-23
**Status:** Approved, ready to implement

Course-correction on the 2-col float focus cues (supersedes the
`.card-unfocused` dim added in the earlier ask-card work).

## Change

When the ask card and the doc card (gloss/journal) share the 2-col float and
one is inactive, DO NOT dim the inactive card. Each card gets a single, quieter
inactive cue instead:

- **Ask card inactive** → the frozen INSERT caret (already implemented via
  `AskCard::set_active`; kept) is the only cue.
- **Doc card (gloss/journal) inactive** → HIDE its accent bar (the left vertical
  selection/cursor line marking the current paragraph block). The page-marker
  glyph (⌄ / •) and line numbers stay visible.

Remove the `.card-unfocused` opacity dim from BOTH cards entirely.

## Implementation

1. **Remove the dim.** In each overlay's `set_ask_focus_dim` and
   `clear_focus_dim` (`gloss_overlay.rs`, `journal_overlay.rs`), delete the
   `add_css_class("card-unfocused")` / `remove_css_class("card-unfocused")`
   toggling on both `self.container` and the ask container. The `.card-unfocused`
   CSS rule in `theme.rs` may stay (harmless, now unused) or be removed.

2. **Hide the doc accent bar when inactive.** Each overlay's `bar_drawing`
   draw-func draws, in order: the page-marker glyph, the vim block cursor, then
   the accent-bar spans (gloss also draws line numbers). Add a shared
   `bar_active: Rc<Cell<bool>>` (default true) captured by the draw closure;
   when false, SKIP the vim block cursor and the accent-bar spans but still draw
   the page marker (and line numbers). Add a setter
   `set_doc_accent_active(bool)` that stores the flag and calls
   `bar_drawing.queue_draw()`.

3. **Drive both from the focus chokepoint.** In `set_ask_focus_dim(ask_focused)`
   (both overlays), replace the removed dim with:
   - `self.set_doc_accent_active(!ask_focused)` — bar visible only when the doc
     card is focused.
   - keep `self.ask_host.set_active(ask_focused)` (the caret freeze).
   `clear_focus_dim` (close/submit) restores `set_doc_accent_active(true)`.
   The initial-open call `set_ask_focus_dim(true)` hides the doc bar (ask card
   starts focused), which is correct.

## Non-goals

- No change to the caret-freeze behavior (kept as-is).
- No change to the page marker, line numbers, or the accent-bar color/geometry.
- No change to the 1-col stacked (journal) layout, which has no focus toggle.

## Testing

- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Headless cage, gloss + journal float (Ctrl+a to open the ask card):
  - On open (ask focused): the doc card shows NO accent bar; neither card is
    dimmed (pixel-sample the doc card body ≈ full `text_bg`, not 0.55).
  - Ctrl+Tab to the doc card: the accent bar reappears; the ask caret freezes.
  - Ctrl+Tab back: the doc accent bar hides again; the ask caret blinks.
