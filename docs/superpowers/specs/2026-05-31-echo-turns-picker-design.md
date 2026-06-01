# Echo Turns Picker (Ctrl+Shift+G)

## Goal

A picker that lists every turn in the **current work** that has at least one
echo link. Selecting a turn jumps the reader cursor to that turn's line and
opens its echoes overlay — the same view the `i` keybind produces.

This complements the existing `i` (show echoes for cursor line) by giving a
work-wide index of where echoes exist, so the user can browse and jump between
annotated turns without scrolling the text looking for them.

## Behavior summary

- **Ctrl+Shift+G** in Reader mode opens the picker.
- The list shows turns in **reading order** (`div1, div2, start_line`).
- **Scope:** current work only; any turn with ≥1 echo link (curated or cached).
- **j/k** or arrows move the selection, **Enter** selects, **Esc** cancels.
- **On select:** jump the cursor to the turn's first line, then open that
  turn's echoes overlay (cache hit — no API call).
- **Empty work** (no echo turns): show a toast ("No echoes in this work") and
  stay in Reader; do not open an empty picker.
- Rows show speaker + location + the turn's first line. **No echo count** —
  kept clean.

## Data model (existing)

- `echo_turns` — a source turn in a work: `work_abbrev, div1, div2,
  start_line, end_line, speaker, turn_text`.
- `echo_links` — echoes attached to a turn (`turn_id` FK, plus `curated`,
  `rank`, etc.).

"Turns in the current work that have echoes" = `echo_turns` rows for the
current `work_abbrev` that have at least one matching `echo_links` row.

## Components

### 1. Data layer — `src/db/queries.rs`

New summary struct and query:

```rust
pub struct EchoTurnSummary {
    pub turn_id: i64,
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64,   // line_in_div, used to locate the line to jump to
    pub speaker: String,
    pub turn_text: String,
}

pub fn list_echo_turns_for_work(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<EchoTurnSummary>, rusqlite::Error>
```

Query shape:

```sql
SELECT t.id, t.div1, t.div2, t.start_line, t.speaker, t.turn_text
FROM echo_turns t
JOIN echo_links l ON l.turn_id = t.id
WHERE t.work_abbrev = ?1
GROUP BY t.id
ORDER BY t.div1, t.div2, t.start_line
```

The `JOIN` + `GROUP BY` guarantees only turns with ≥1 link appear.

### 2. UI layer — `src/ui/echo_turns_picker.rs` (new)

Modeled on `src/ui/echo_picker.rs`:

- Cream card (`library-picker` CSS), scrim, header "ECHOES IN THIS WORK".
- Footer hint: `j/k navigate  ·  Enter select  ·  Esc cancel`.
- Each row: detail line `{speaker}  ·  {title} {div1}.{div2}` and a body label
  with the turn's first line (ellipsized). No count badge.
- `set_items(Vec<EchoTurnSummary>)`, `set_titles(HashMap)` (to resolve
  `work_abbrev` → readable title, same as `echo_picker`), `show`, `hide`,
  `populate_list`, `selected_index`, `move_selection` — same API surface as the
  other pickers.

**Attachment:** add as an `add_overlay` panel onto the outer overlay (the way
`echo_line_picker.picker_box` is added at `app.rs:792`), **not** wrapped into
the reader's size-bearing widget chain. Wrapping pickers into the chain has
previously collapsed the reader layout (sw_h stuck at 0) — see the comment at
`app.rs:787-790` and the project memory note "Pickers: overlay, not chain
link".

### 3. Input layer

- `src/app.rs`: new `InputMode::EchoTurnsPicker` variant; new
  `echo_turns_picker` field in `AppState`.
- `src/input/actions/mod.rs`: new `Action::ShowEchoTurns`.
- `src/input/keymap_config.rs`: bind `KeyCombo::ctrl_shift("G")` →
  `Action::ShowEchoTurns` (reuse the same `ctrl_shift("G")` form already used by
  `ToggleNavTest`). Verify the GTK key name for `G` on RPD against `~/utono/rpd`
  before finalizing.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`: add the same
  binding, or the JSON override silently shadows the compiled default. (Both
  files must change — project convention.)
- `src/input/keymap.rs`:
  - Route `InputMode::EchoTurnsPicker` to a new
    `handle_echo_turns_picker_key` handler (j/k/arrows → move_selection,
    Enter → confirm, Esc → cancel back to Reader). Add the mode to the
    early-return picker-mode list (around `keymap.rs:86`) as appropriate.
  - Dispatch `Action::ShowEchoTurns` →
    `crate::input::actions::echoes::open_echo_turns_picker`.

### 4. Open + confirm flow — `src/input/actions/echoes.rs`

`open_echo_turns_picker(state_rc)`:
1. Read `current_work.abbrev` (return if none).
2. `list_echo_turns_for_work`; load work titles (`load_work_titles`).
3. If empty → toast "No echoes in this work", stay in Reader, return.
4. Populate the picker, `set_titles`, `show`, set
   `InputMode::EchoTurnsPicker`.

`confirm_echo_turns_pick(state_rc, tokio_handle)`:
1. Take the selected `EchoTurnSummary`.
2. Resolve `(div1, div2, start_line)` to the work line and set the cursor
   there: find the line in `current_work.lines` whose `div1/div2/line_in_div`
   match, map work-index → buffer-index via `line_map`, set `current_line`,
   call `update_highlight_and_center` (mirrors `jump_to_line_mapping_id` in
   `pickers.rs:181`).
3. Hide the picker, set `InputMode::Reader`.
4. Call `show_echoes_for_cursor_line(state_rc, tokio_handle)` — it rebuilds the
   same `EchoTurnKey` from the cursor turn, hits the cache, and opens the
   echoes overlay. No duplicated overlay logic; identical to pressing `i`.

## Data flow

```
Ctrl+Shift+G
  → Action::ShowEchoTurns
  → open_echo_turns_picker
      → list_echo_turns_for_work(current work)
      → (empty? toast + stay) | populate picker + show
  → [user j/k] move_selection
  → Enter
  → confirm_echo_turns_pick
      → jump cursor to turn line (update_highlight_and_center)
      → hide picker, InputMode::Reader
      → show_echoes_for_cursor_line  → cache hit → EchoesOverlay
```

## Error handling

- No current work → no-op return.
- DB error in `list_echo_turns_for_work` → log + treat as empty (toast).
- Selected turn's line not found in the loaded work (stale row) → log + hide
  picker, return to Reader without opening the overlay.
- `show_echoes_for_cursor_line` already handles "cursor line has no speaker
  turn" gracefully (logs and returns); the jump should always land on a valid
  turn line, but this is the safety net.

## Testing

- Unit test `list_echo_turns_for_work` in `queries.rs` (mirrors existing
  `echo_links` tests): in-memory DB, seed two turns with links and one turn
  with no links; assert only the two linked turns return, in reading order.
- `cargo build`, `cargo clippy`, `cargo test`.
- Manual (user runs): Ctrl+Shift+G opens the picker; rows list the work's
  echo turns in reading order; Enter jumps the cursor and opens the echoes
  overlay; Esc cancels; a work with no echoes shows the toast.

## Out of scope (YAGNI)

- Fuzzy text filter in the picker (the other concordance pickers have one;
  not requested here — add later if the list gets long).
- Cross-work / all-author listing (request is explicitly "current work").
- Echo count badges, curated-only filtering, alternate sort orders.
