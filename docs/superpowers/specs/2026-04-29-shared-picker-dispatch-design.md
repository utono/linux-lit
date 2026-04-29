# Shared Picker Key Dispatch (F6)

## Goal

Extract the duplicated key handling across 5 list pickers and the settings overlay into a shared `resolve_picker_key` function, removing ~150 lines of boilerplate from `handle_key` in `keymap.rs`.

## Background

Six overlays in `handle_key` (bookmark picker, media picker, concordance picker, concordance word picker, concordance list picker, settings overlay) all handle the same core keys: Escape→hide, Return→confirm, Ctrl+n/Down→move down, Ctrl+p/Up→move up. Each currently implements this as its own 20-40 line match block. The library picker is excluded — its hierarchical level-dependent Escape/Return/BackSpace behavior is too different.

## Design

### PickerAction enum and resolve_picker_key function

New file `src/input/picker_keys.rs`:

```rust
pub(crate) enum PickerAction {
    Hide,
    Confirm,
    MoveDown,
    MoveUp,
    Unhandled,
}

pub(crate) fn resolve_picker_key(key_name: &str, is_ctrl: bool) -> PickerAction
```

Maps:
- `Escape` → Hide
- `Return` → Confirm
- `Ctrl+n`, `Down` → MoveDown
- `Ctrl+p`, `Up` → MoveUp
- Everything else → Unhandled

### j/k removal

Remove j/k bindings from all pickers:
- **Bookmark picker**: currently has `"Down" | "j"` and `"Up" | "k"` with search-focus gating. Remove `"j"` and `"k"` alternatives — only `Down`/`Up`/`Ctrl+n`/`Ctrl+p` navigate.
- **Media picker**: same pattern, remove j/k alternatives.
- **Concordance list picker**: currently has `"j" | "n"` for down and `"k" | "p"` for up. Remove all four — only `Down`/`Up`/`Ctrl+n`/`Ctrl+p` navigate.
- **Settings overlay**: currently has `"j" | "Down"` and `"k" | "Up"`. Remove j/k alternatives.

After removal, search-focus gating for Down/Up is no longer needed in bookmark and media pickers — arrow keys don't conflict with search entry text input.

### Per-picker wiring

Each picker block in `handle_key` becomes:

```
let action = resolve_picker_key(key_name, is_ctrl);
match action {
    Hide => { /* picker-specific hide */ },
    Confirm => { /* picker-specific confirm */ },
    MoveDown => { /* widget.move_selection(1) */ },
    MoveUp => { /* widget.move_selection(-1) */ },
    Unhandled => { /* picker-specific extras or return false */ },
}
```

Picker-specific extras that survive:
- **Bookmark picker**: `Delete` → delete_bookmark
- **Media picker**: `"p"` (when search not focused) → set_media_default
- **Settings overlay**: `"h"/"Left"` → adjust(-1), `"l"/"Right"` → adjust(1), `"r"` → reset, `_ => return true` (consume all)
- **Concordance picker**: `_ => {}; return false` (let GTK handle text input)
- **Concordance word picker**: `_ => return false` (let GTK handle text input)
- **Concordance list picker**: `_ => return false`

### Ctrl+n/Ctrl+p pre-dispatch removal

Currently each picker has a separate `if picker_visible && is_ctrl { match "n"/"p" ... }` block before the main match. Since `resolve_picker_key` handles `Ctrl+n`/`Ctrl+p` → MoveDown/MoveUp, these pre-dispatch blocks are removed. This applies to: bookmark picker, media picker, settings overlay, concordance picker (inline `if is_ctrl`), concordance word picker (inline `if is_ctrl`), concordance list picker (inline `if is_ctrl`).

### Settings footer text update

Change the footer label in `settings_overlay.rs` from:
```
"j/k navigate · h/l adjust · r reset · Enter confirm · Esc revert"
```
to:
```
"↑↓ navigate · h/l adjust · r reset · Enter confirm · Esc revert"
```

### Module registration

Add `pub mod picker_keys;` to `src/input/mod.rs`.

## What doesn't change

- Library picker block (hierarchical, stays standalone)
- Search bar, gloss overlay, gamepad overlay, keybinds overlay (not list pickers)
- Visual mode, action popup
- The Keymap/Action/dispatch_action pipeline
- Settings overlay visual style (deferred to a separate task)
