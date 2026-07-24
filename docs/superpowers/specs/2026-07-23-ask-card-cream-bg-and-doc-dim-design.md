# Ask input card: cream background + dim the doc card when active

**Date:** 2026-07-23
**Status:** Approved, ready for plan

## Problem

When an ask input card is opened inside a gloss/synopsis/journal overlay
(Ctrl+a → the right-side floated panel), two things look wrong:

1. **Ask card background is blue-grey, not cream.** The `.ask-card` base CSS
   rule paints `alpha({gloss_bg}, 0.82)` — 18% transparent. The
   `.gloss-ask-float` override sets opaque `{bg}` (cream), but the
   semi-transparent base composites against the dark-teal `scrim_bg` behind the
   float, so the panel reads blue-grey instead of matching the overlay's cream
   card. (`gloss_bg` and `bg` both resolve to `theme.text_bg` — the same cream —
   so the ONLY thing making the panel blue-grey is the 0.82 alpha letting the
   scrim bleed through.)

2. **The left doc card is not dimmed** while the ask card is active. The
   dimming mechanism (`set_ask_focus_dim`, `.card-unfocused` → `opacity: 0.55`)
   already exists and works, but it is only wired to the Ctrl+Tab and Escape
   focus-toggle handlers in `keymap.rs` — never to the *initial open*. So a
   freshly opened ask card leaves the doc card at full brightness; the dim only
   appears after a Ctrl+Tab round-trip.

## Desired behavior

- The ask input card's background matches the other overlays' cream card
  background (`theme.text_bg`), on both the gloss/synopsis and journal surfaces.
- When the ask input card is active, the left doc card (gloss / synopsis /
  journal) is dimmed at `opacity: 0.55` — the existing `.card-unfocused`
  pattern, applied from the moment the ask card opens.

## Fix

Two small, surgical changes.

### (A) Ask card background → opaque cream

In `src/theme.rs` `generate_css`, change `.ask-card`'s `background-color` from
`alpha({gloss_bg}, 0.82)` to opaque `{gloss_bg}` (the line currently at ~1333).
This makes both the stacked and floated ask cards paint the same cream as the
overlay card. `.gloss-ask-float`'s `{bg}` override (~1441) becomes
redundant-but-harmless (identical cream) and is left in place. Touching the one
shared `.ask-card` class fixes gloss, synopsis, and journal ask cards at once.

### (B) Dim the doc card on open

Apply the dim inside the overlay's own open methods rather than at the call
sites:

- `GlossOverlay::open_ask_card_with` (gloss + synopsis) →
  `self.set_ask_focus_dim(true)`.
- `JournalOverlay::open_ask_card` → `self.set_ask_focus_dim(true)`.

A fresh ask card always starts focused (`ask_card_focus = true` is set by every
open path), so the dim direction is always "dim the doc card, un-dim the ask
float." The existing Ctrl+Tab / Escape toggles and the `close` /
`clear_focus_dim` paths already handle the rest of the lifecycle correctly, so
no call-site changes are needed and the dim can no longer be forgotten.

## Non-goals

- No change to the scrim depth or to the ask-card border/focus styling.
- No change to the Ctrl+Tab focus-toggle or close/submit lifecycle.
- No new CSS classes.

## Testing

- `cargo build` + `cargo clippy` clean.
- Headless cage render of the Ctrl+a-in-gloss state; pixel-sample:
  - the ask panel background reads cream (≈ `theme.text_bg`), not blue-grey;
  - the left doc card is visibly dimmed relative to the ask card.
- Verify on the real GL renderer / user screenshot, since cage is software
  rendering and can disagree on compositing.
