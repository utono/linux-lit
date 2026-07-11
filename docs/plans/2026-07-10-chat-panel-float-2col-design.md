# Chat panel floats over 2-column layout — design

Date: 2026-07-10
Status: approved (brainstormed with user; see decisions inline)

## Problem

On two-column works the chat layout currently has no room: opening the panel
pins the card right and needs >= 500px of freed left space, which a 2-col card
never leaves — the panel closes with a "No room for chat panel at this layout"
toast. Work switches from a 1-col work with the panel open hit the same gate
(`regate_panel` close path).

## Decision summary (user-approved)

- Two-column works: the panel FLOATS ON TOP of the card, overlapping one
  column. The card's margins/geometry are untouched.
- Single-column works: current behavior unchanged (card pins right, panel in
  the freed left space, `CHAT_MIN_PANEL_W` gate, toast on no-room).
- Side toggle: a SINGLE key, `Ctrl+l` (currently unbound), flips the panel to
  the other column. No directional Ctrl+h/Ctrl+l pair — Ctrl+h stays
  `ToggleSynopsis` (`keymap_config.rs:351`).
- Default side at open: the column the cursor is NOT in at open time.
- No auto-follow: after open, the panel stays put as the cursor moves between
  columns; flipping is always manual (Ctrl+l). Side is session-only, not
  persisted to config.

## Architecture

The panel container is ALREADY an overlay child (`outer_overlay.add_overlay`
at `src/app/mod.rs:1615`, halign Start, valign Center) — the "pin" look is
purely `apply_card_sizing(chat_open=true)` pushing the card right. Float mode
repositions the same widget; nothing enters the size-bearing widget chain
(house rule: pickers/panels are overlays, never chain links).

### State

```rust
pub enum ChatPlacement { Pinned, FloatLeft, FloatRight }
// AppState field, session-only:
pub chat_placement: ChatPlacement,
```

Set at open and at regate. Three valid states instead of flag soup
(`chat_layout_open` remains the open/closed source of truth).

### Placement selection

- `toggle_chat_layout`: `column_count() == 2` → float (no free-space gate; a
  2-col card always has column-width room by construction). Side = opposite
  the cursor's column. Else → `Pinned` with today's gate.
- Cursor-side detection reads the active spread's right-column start line
  (page table / `column_split` boundary — the same authoritative source nav
  uses; never re-inferred from buffer text).

### Geometry and visuals

- Float width: `MIN_TWO_COLUMN_COLUMN_WIDTH`. Height: card height (same
  vertical span as pinned mode).
- Position: card x + centered-columns-block offset + (0 for left column, or
  `col_w + divider` for right), applied via the overlay child's
  margin_start/width_request.
- New `.chat-panel-float` CSS class: OPAQUE root-color background + hairline
  border. The current "bare on root" panel is transparent and would blend
  with card text underneath; opacity is required in float mode only.
- Occlusion, not clipping: the column under the panel keeps rendering and
  sync/nav keep running beneath it.

### Keybind

New `Action::ChatPanelFlipSide` bound to `Ctrl+l`:

- live from reader focus (global table), AND consumed in the chat prompt /
  transcript modal handlers so it works while typing;
- no-op unless `chat_layout_open` and placement is Float*.

Update all four keybind surfaces: `keymap_config.rs`, stow
`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, Ctrl+/ overlay
KeyDef + `describe()` arm (run the update-cairo-keybinds-overlay skill).

### Work-switch regate

`regate_panel` (the deferred, settled-geometry hook from commit d373c48;
`chat_regate_pending` mechanism reused as-is) gains placement logic:

- target work 2-col → convert to float (side from cursor), panel STAYS open —
  the close-with-toast path disappears for 2-col targets;
- target 1-col → pin if free >= `CHAT_MIN_PANEL_W`, else close + toast
  (today's behavior).

## Testing

- Headless cage e2e: open on Hamlet (2-col) → screenshot shows panel over one
  column, other column fully readable; Ctrl+l → other side; BH→Ham switch
  with panel open → pin converts to float (no toast-close); Ham→BH → float
  converts to pin.
- `cargo test --bins`: pure placement-choice logic (cursor column → default
  side), no GTK needed.

## Out of scope

- Persisting the side choice to config (add later if session-flipping
  annoys).
- Auto-follow / smart re-side on cursor column crossings.
- Any change to single-column chat layout behavior.
