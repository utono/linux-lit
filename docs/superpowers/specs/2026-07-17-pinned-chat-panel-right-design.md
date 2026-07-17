# Pinned chat panel on the right — design

## Goal

For prose works / single-column layouts, the pinned chat panel currently sits
in the freed space on the **left**, with the reading card pinned flush
**right**. Mirror this: pin the card flush **left**, and fill the freed space
on the **right** with the chat panel.

Two-column (`FloatLeft`/`FloatRight`) placements are unchanged — they already
float over whichever column the cursor is NOT in, so they can already land on
the right. This change touches only the single-column `Pinned` placement.

## Scope decision

Layout mirror only. No keybind, toast, free-space-gate, or float-side-default
changes. The `ChatPlacement` enum, `CHAT_MIN_PANEL_W` gate, and all float
branches stay as they are.

## Change 1 — `apply_card_sizing` (`src/app/layout.rs`, chat-open branch)

Today all slack goes to `margin_start`, pinning the card right:

```
end   = clamp(slack/2, 0, CARD_OUTER_MARGIN)   // normal right margin
start = slack - end                            // all remaining slack → left
```

Mirror it — keep the normal margin on the **left**, push all remaining slack to
the **right**:

```
start = clamp(slack/2, 0, CARD_OUTER_MARGIN)   // normal left margin
end   = slack - start                          // all remaining slack → right
```

## Change 2 — `size_panel` (`src/input/actions/chat.rs`, `Pinned` branch)

Panel width is unchanged (`ww - card_w - CARD_OUTER_MARGIN - 24 - 16`), but the
panel must now sit to the right of the card. Layout left→right:
`CARD_OUTER_MARGIN` (card left margin) + `card_w` + 16px gap + panel + 24px
right outer margin. So:

```
margin_start = CARD_OUTER_MARGIN + card_w + 16   // just right of the card
```

Everything else in the branch — top margin 0, `valign: Center`, height
`card_h`, removing the `chat-panel-float` CSS class — stays.

## Also — `close_chat_layout`

Resets `margin_start(24)` on the (hidden) panel container. Cosmetic only since
the panel is hidden, but kept consistent so a re-open starts from a clean
margin.

## Verification

1. `cargo build`.
2. Headless cage e2e: open a prose work, press `Tab` to open the panel,
   screenshot, and confirm the card is on the left and the panel on the right
   with no overlap or clipping.
