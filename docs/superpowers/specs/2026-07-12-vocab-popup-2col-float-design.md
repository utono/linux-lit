# Vocab popup: full-column float in two-column layout

2026-07-12. Approved in-session.

## Problem

The vocab popup (`r` tap / `Ctrl+r` vocab journal Q&A) anchors in the strip
right of the text card (`margin_start` = card right + 12, `valign End`).
A two-column card leaves almost no strip, so the popup lands squeezed
against the window edge, overlapping the right column.

## Decision

In `column_count() == 2`, the popup mirrors the Tab chat panel's float:
a full-column panel covering the reading column the cursor is NOT in,
re-evaluated automatically as the cursor moves (playback sync included).
Single-column behavior is unchanged.

- Shape: full column — width to the column divider, height = card height,
  `valign Center` (matches the card's vertical centering), `halign Start`
  with `margin_start` = column x from `compute_bounds(&window)`.
- Side: opposite the cursor's column via chat's `cursor_in_right_column`
  (page-table/`column_split` derived, never text inference); recomputed at
  every popup (re)show: `open_vocab_popup`, `refresh_vocab_popup`, the
  scene-synopsis call site. Word cycling alone does not reposition.
- Styling: new `.vocab-popup-float` class mirroring `.chat-panel-float`
  (card `{bg}` background, `1px alpha(fg, 0.25)` border, 8px radius) so the
  panel reads as floating over the card. The popup's `content_box` gets
  `vexpand` so the hint footer pins to the panel bottom (no effect at
  natural height in single-column).

## Components

- `src/app/vocab_popup.rs` — `position_vocab_popup(state)` replaces
  `update_vocab_popup_margin`: strip placement for 1-col, float placement
  for 2-col (column bounds + divider extension, same math as
  `chat::size_panel`).
- `src/ui/vocab_popup.rs` — `place_strip(margin_start)` /
  `place_float(x, w, h)` own the container mutations (margins, aligns,
  size requests, float class).
- `src/input/actions/chat.rs` — `cursor_in_right_column` becomes
  `pub(crate)` for reuse.
- `src/theme.rs` — `.vocab-popup-float` block.

## Out of scope / known edge cases

- Chat float and vocab float can target the same free column; chat
  (outer overlay) paints on top. Not handled.
- The panel flips sides whenever the spoken/cursor line crosses the column
  split. If that proves twitchy, damp later (flip only on re-open).
- No manual flip bind (chat's Ctrl+l stays chat-only).

## Verification

Headless cage: 2-col work (2H6-Arkangel), cursor in left column → popup
covers right column; cursor in right column → popup covers left. 1-col
(BH-Barrett) regression: popup unchanged right of the card. Final eyeball
on the real renderer by the user.
