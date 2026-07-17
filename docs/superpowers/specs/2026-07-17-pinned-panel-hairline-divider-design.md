# Pinned panel: hairline seam with the card — design

## Goal

In single-column (Pinned) layout the card and panel currently sit as two cream
surfaces separated by a wide band of blue root (a 16px gap plus all the card's
right-margin slack). The user wants them to read as ONE cream surface split only
by a 1px hairline: card and panel abut, rounded OUTER corners, a square hairline
seam down the meeting edge. (Chosen over "two rounded cards with a thin gap".)

## Layout — card and panel abut

The card+panel become one block spanning `[CARD_OUTER_MARGIN .. ww -
CARD_OUTER_MARGIN]`, with a 1px seam between them:

1. `apply_card_sizing` (chat branch, `src/app/layout.rs`): card stays flush-left
   as before — `content_hbox` is `halign:Center`, so `margin_end` must RESERVE
   the panel region to keep the fixed-width card pinned left (setting it to 0
   floats the centered card into the middle and the panel overlaps/clips it).
   The blue gap was never from `margin_end`; it was the panel's `+16` offset,
   removed in step 2.

2. `size_panel` (Pinned branch, `src/input/actions/chat.rs`): panel starts 1px
   right of the card's edge (`CARD_OUTER_MARGIN + card_w + PINNED_DIVIDER_W`),
   and its width fills to the right outer margin
   (`ww - CARD_OUTER_MARGIN - start`). The 1px gap shows a hairline of root; the
   panel's `border-left` (below) paints on top of it for a crisp line. (Was:
   `+16` gap and a width that subtracted the 16.)

`PINNED_DIVIDER_W = 1`.

## Styling — hairline + outer-only rounding

A new `.chat-panel-pinned` class (added in the Pinned branch, removed on
close/float, mirroring how `.chat-panel-float` is toggled):

- `border-left: 1px solid alpha({fg}, 0.25)` — the hairline seam (same stroke
  the float panel already uses on its left edge).
- `border-radius: 0 12px 12px 0` — rounded on the RIGHT (outer) corners only;
  the left (seam) corners stay square so the panel meets the card flush. 12px
  matches the card's own `.page-turn-overlay` radius.

The base `.chat-panel` keeps the card `{bg}` + padding from the previous change;
this class only adds the seam border and the outer rounding.

## Files

- `src/app/layout.rs` — `apply_card_sizing` chat branch: `margin_end = 0`.
- `src/input/actions/chat.rs` — `size_panel` Pinned branch: new start/width math
  + `add_css_class("chat-panel-pinned")`; `close_chat_layout` and the float
  paths / `regate` remove it.
- `src/theme.rs` — `.chat-panel-pinned` rule.

## Verification

`cargo build`, then headless cage: open a prose work, `Tab` to open the Pinned
panel, screenshot — card and panel should form one cream block split by a single
hairline, rounded outer corners, no blue gap between them.
