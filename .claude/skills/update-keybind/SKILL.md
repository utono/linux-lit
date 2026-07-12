---
name: update-keybind
description: Use when adding, removing, or changing a keybind in linux-lit — key routing in keymap.rs, action handlers in navigation.rs or other input modules, state machine sequences like gg
---

# Update Keybind

Add, remove, or modify keybinds in the linux-lit GTK4 reader.

## Architecture

Keybind dispatch is layered in `src/input/keymap.rs`. Each layer short-circuits before falling through:

1. **Library picker visible** — picker navigation keys
2. **Media picker visible** — media picker keys
3. **Settings overlay visible** — overlay navigation keys
4. **Search bar visible** — search interaction keys
5. **Normal mode: multi-key sequences** — `gg` state machine (`KeyState.pending_g`)
6. **Normal mode: modifier combos** — Ctrl/Alt/Shift combinations
7. **Normal mode: single keys** — all remaining keybinds

Action handlers live in:
- `src/input/navigation.rs` — cursor movement, page turns, jumping
- `src/input/timestamps.rs` — timestamp editing
- `src/input/search.rs` — search operations
- `src/app.rs` — font, theme, display toggling

## Steps

### 0. Resolve the emitted keysym on the RPD layout (symbol/non-letter keys)

The layout is Real Programmers Dvorak. The GTK key name a physical cap emits is
NOT what QWERTY shows. Before binding any symbol, digit, or punctuation key,
read the authoritative xkb symbols file:

```
~/utono/rpd/xkb/usr/share/X11/xkb/symbols/real_prog_dvorak
```

Each `key <CODE> { [ level1, level2, ... ] }` row gives the keysym per shift
level; `<CODE>` is the QWERTY position (`<TLDE>` = QWERTY `` `/~ `` cap). Rules:

- Bind the **keysym name**, not the glyph (`$` → `"dollar"`, `~` →
  `"asciitilde"`, `(` → `"parenleft"`).
- **Level 1 = unshifted.** A level-1 symbol supports both a Ctrl and a distinct
  Ctrl+Shift chord (e.g. `$` on `<TLDE>` → `ctrl("dollar")` AND
  `ctrl_shift("dollar")`). A symbol that lives only on level 2 (e.g. shift+minus
  → `underscore`) can only be a shifted chord — you can't split forward/back
  across one cap. Confirm the level before promising two directions.
- Shifted ASCII letters arrive as the capital name + `shift=true` (handled by
  `lookup`'s `effective_shift`); shifted symbols arrive as the shifted glyph
  name (e.g. Shift+comma → `less`). When unsure how a specific chord is
  reported, check existing dual binds in `keymap_config.rs` (slash/question,
  minus/underscore) or the debug log.

### 1. Read current state

Read the dispatch layer where the keybind lives (or will live):
- `src/input/keymap.rs` — find the exact match arm
- The action handler file — find the function signature and logic

### 2. Identify the line type context

If the keybind navigates to specific line types, check `src/db/line_types.rs` for classification predicates (`is_dialogue`, `is_speaker`, `is_stage_direction`, etc.) and `src/db/models.rs` for `Line` struct fields (`is_dialogue`, `speaker`, `timestamp`).

### 3. Handle line_map vs direct access

Buffer lines and work lines may differ when a text file is loaded:
- **With `line_map`**: use `lm.dialogue_buffer_lines` or `lm.buffer_to_work` for index conversion
- **Without `line_map`**: access `work.lines[i]` directly (buffer index = work index)

Both paths must be handled. See `jump_to_next_dialogue` / `jump_to_prev_dialogue` for the canonical pattern.

### 4. Make the change

- **Add**: Write the action handler function, then add the match arm in the correct layer of `keymap.rs`
- **Modify**: Update the action handler; the match arm in `keymap.rs` usually stays the same
- **Delete**: Remove the match arm from `keymap.rs` and delete the handler if unused
- **Multi-key sequence**: Follow the `pending_g` pattern — set a flag, arm a timeout, check flag on next keypress

### 5. Build

```bash
cargo build
```

Do not run the app. The user will test with `cargo run`.
