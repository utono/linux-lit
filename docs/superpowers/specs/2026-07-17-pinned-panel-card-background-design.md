# Pinned chat panel shares the card background — design

## Problem

In two-column (float) layout the chat panel paints the card background
(`.chat-panel-float { background-color: {bg} }`) and its text uses the card ink
(`{fg}`), so it reads like an extension of the reading card. In single-column
(Pinned) layout the panel is transparent (`.chat-panel { background-color:
transparent }`) and its text uses `{chat_ink}` — a contrast color computed
against the ROOT (the blue wallpaper), because the panel was sitting directly on
the root. Result: pale, low-contrast text floating on bare blue instead of on a
card.

The user wants the Pinned panel to match float: share the main card's
background AND its font color.

## Root cause

`chat_ink = contrast_on(&theme.root_color)` — a near-white/near-black chosen to
contrast the wallpaper, correct only while the Pinned panel is transparent. The
float panel instead sits on `{bg}` and uses `{fg}` (the card's real ink).

## Changes (src/theme.rs, generate_css)

1. `.chat-panel` gains the card background and the same padding as the float
   panel, so the Pinned panel is a card surface too:
   `.chat-panel { background-color: {bg}; padding: 12px; }` (was
   `transparent`). No borders — the Pinned panel stands alone beside the card
   with its own outer margin, so no border is needed to separate it from a
   reading column (unlike float, which overlaps one).

2. `chat_ink` is redefined to the card foreground so every base `.chat-*` text
   rule contrasts against the card, matching the float overrides:
   `chat_ink = theme.text_fg` (was `contrast_on(&theme.root_color)`).

   All base rules that use `{chat_ink}` (`.chat-a`, `.chat-a-gloss`,
   `.chat-a-verse-flush`, `.chat-q`'s alpha, chip/error/saved, the flash wash)
   then paint the same ink the float panel already uses via `{fg}`. The float
   `.chat-panel-float .chat-* { color: {fg} }` overrides stay (still correct for
   the float placement) and now agree with the base rules by value.

## Out of scope

The internal dim hierarchy of the base rules (e.g. `.chat-q` at
`alpha(chat_ink, 0.70)`) is preserved — this design only re-bases the ink and
adds the card surface, it does not re-tune the per-row dimming to byte-match the
float `{dim}` rules.

## Verification

`cargo build`, then the user's live SIGUSR1 reload (or headless cage): open a
prose work, open the panel (`Tab` or `-` gloss), and confirm the panel now has
the cream card background with dark, readable text like the two-column float.
