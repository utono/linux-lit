# Settings Restyle, InputMode Enum, and Per-Mode Handler Functions

Three sequential features that complete the keymap/dispatch cleanup. Each builds on the previous but produces working software independently.

## Part A: Settings Overlay Restyle

### Goal

Rewrite `settings_overlay.rs` to use the same widget tree and CSS classes as the library picker, eliminating the dark custom-drawn popup.

### Widget Tree

Match the library picker pattern exactly:

- `GtkBox.library-picker-scrim` — dark backdrop
- `GtkBox.library-picker` — popup container (width_request 500, centered)
  - `GtkBox.library-picker-header` — "SETTINGS" title left, "7 items" right
  - No search entry
  - `ScrolledWindow` → `ListBox` with 7 `ListBoxRow`s
  - `Label.library-picker-footer` — "↑↓ MOVE · ←→ ADJUST · r RESET · ↵ CONFIRM · ESC REVERT"

Each `ListBoxRow` contains a horizontal `GtkBox` with:
- Name `Label` (left, hexpand) — "Theme", "Line Spacing", etc.
- Value `Label` (right) — "◀ Gruvbox Light ▶", "◀ 6px ▶", etc.

### Selection

Use GTK-native `ListBox` selection (`list_box.select_row()`). Remove manual `selected: usize` tracking and `settings-row-selected` CSS class toggling. `move_selection` calls `list_box.select_row()` on the target row.

### Disabled Row (Transition when Navigation=Scroll)

Set opacity directly on the `ListBoxRow` widget (`row.set_opacity(0.35)`) instead of using a `settings-row-disabled` CSS class. Skip the row in `move_selection` when disabled (existing logic, just adapted to ListBox row indexing).

### CSS Cleanup

Remove from `theme.rs`:
- `.settings-overlay { ... }`
- `.settings-title { ... }`
- `.settings-row { ... }`
- `.settings-row-selected { ... }`
- `.settings-row-disabled { ... }`
- `.settings-footer { ... }`

The settings overlay now inherits all styling from the existing `.library-picker` rules.

### Public API Preserved

All existing public methods on `SettingsOverlay` keep their signatures:
- `new(themes, current_theme_name)` — constructor
- `show(line_spacing, column_width, text_margins, navigation_mode, transition_style, show_cursor_line)` — populate and show
- `hide()` / `is_visible()` / `attach(base)` — visibility management
- `move_selection(delta)` — now delegates to ListBox
- `adjust_value(delta, ...) -> SettingsChange` — unchanged logic
- `snapshot()` / `set_theme_index()` / `themes()` — unchanged
- `update_displayed_values(...)` — updates value labels

`SettingsChange` enum stays in `settings_overlay.rs`, unchanged.

### Callers

No changes needed in `keymap.rs` or `actions/settings.rs` — the public API is the same. The settings block in `handle_key` already uses `resolve_picker_key` (from F6) and calls `move_selection`, `adjust_value`, `hide`, etc. through the same interface.

---

## Part B: InputMode Enum (F2)

### Goal

Replace the 12 sequential `is_visible()` checks in `handle_key` with a single `match` on an `InputMode` enum stored in `AppState`.

### Enum

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Reader,
    LibraryPicker,
    BookmarkPicker,
    MediaPicker,
    Settings,
    Search,
    GlossOverlay,
    GamepadOverlay,
    KeybindsOverlay,
    ConcordancePicker,
    ConcordanceWordPicker,
    ConcordanceListPicker,
    ActionPopup,
    Visual,
}
```

### AppState Field

`pub input_mode: InputMode` — initialized to `InputMode::Reader`.

### Show/Hide Contracts

Every overlay's `show` path sets `state.input_mode` to the corresponding variant. Every `hide` path sets it back to `InputMode::Reader`.

Affected sites (each gains one `input_mode` assignment):
- Library picker: `show_prepare`/`show_finish` → LibraryPicker, `hide` → Reader
- Bookmark picker: `show` → BookmarkPicker, `hide` → Reader
- Media picker: `show` → MediaPicker, `hide` → Reader
- Settings overlay: `show` → Settings, `hide` → Reader (+ revert_to_snapshot)
- Search bar: `show` → Search, `hide` → Reader
- Correction overlay: `show`/`show_loading` → GlossOverlay, `hide` → Reader
- Gamepad overlay: `show` → GamepadOverlay, `hide` → Reader
- Keybinds overlay: `show` → KeybindsOverlay, `hide` → Reader
- Concordance picker: `show` → ConcordancePicker, `hide` → Reader
- Concordance word picker: `show` → ConcordanceWordPicker, `hide` → Reader
- Concordance list picker: `show` → ConcordanceListPicker, `hide` → Reader
- Action popup: `open_action_popup` → ActionPopup, `close_action_popup` → Reader
- Visual mode: `enter_visual_mode` → Visual, `exit_visual_mode` → Reader

### handle_key Change

The top of `handle_key` changes from:

```rust
let picker_visible = state.borrow().picker.is_visible();
if picker_visible && is_ctrl { ... }
if picker_visible { ... }
let bookmark_picker_visible = state.borrow().bookmark_picker.is_visible();
if bookmark_picker_visible { ... }
// ... 10 more blocks
```

To:

```rust
let mode = state.borrow().input_mode;
match mode {
    InputMode::Reader => {} // fall through to keymap dispatch below
    InputMode::LibraryPicker => { ... existing library picker block ... }
    InputMode::BookmarkPicker => { ... existing bookmark picker block ... }
    // ... etc
}
// Reader-mode: keymap dispatch
```

The block contents don't change — they move from sequential if/else into match arms.

### Behavioral Invariant

`input_mode` and widget visibility must stay in sync. If they diverge (bug), the widget's `is_visible()` is the ground truth. The `input_mode` field is an optimization for dispatch, not a replacement for the widget state.

---

## Part C: Per-Mode Handler Functions (F1)

### Goal

Extract each mode's match arm from `handle_key` into a standalone function, reducing `handle_key` to ~30 lines.

### Functions

Each function takes the same signature as needed by its block (`state`, `key_state`, `key_name`, `is_ctrl`, `is_shift`, `is_alt`, `tokio_handle` — only the parameters it actually uses). Returns `bool` (key consumed).

New functions in `src/input/keymap.rs` (or a new `src/input/mode_handlers.rs` if keymap.rs is still too large):

- `handle_library_picker_key(state, key_state, key_name, is_ctrl, tokio_handle) -> bool` — the library picker block (hierarchical, standalone)
- `handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode) -> bool` — shared handler for BookmarkPicker, MediaPicker, ConcordancePicker, ConcordanceWordPicker, ConcordanceListPicker. Uses `resolve_picker_key` + per-picker extras selected by `mode`.
- `handle_settings_key(state, key_name, is_ctrl) -> bool` — settings overlay block
- `handle_search_key(state, key_name) -> bool` — search bar block
- `handle_gloss_key(state, key_name) -> bool` — gloss overlay block
- `handle_gamepad_key(state, key_name) -> bool` — gamepad overlay (just Escape)
- `handle_keybinds_key(state, key_state, key_name) -> bool` — keybinds overlay block
- `handle_action_popup_key(state, key_name, is_ctrl, tokio_handle) -> bool` — action popup block
- `handle_visual_key(state, key_state, key_name) -> bool` — visual mode block

### handle_key After Extraction

```rust
pub fn handle_key(...) -> bool {
    crate::logging::log(&format!("KEY: ..."));
    let mode = state.borrow().input_mode;
    match mode {
        InputMode::Reader => {}
        InputMode::LibraryPicker => return handle_library_picker_key(state, key_state, key_name, is_ctrl, tokio_handle),
        InputMode::BookmarkPicker => return handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
        InputMode::MediaPicker => return handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
        InputMode::Settings => return handle_settings_key(state, key_name, is_ctrl),
        InputMode::Search => return handle_search_key(state, key_name),
        InputMode::GlossOverlay => return handle_gloss_key(state, key_name),
        InputMode::GamepadOverlay => return handle_gamepad_key(state, key_name),
        InputMode::KeybindsOverlay => return handle_keybinds_key(state, key_state, key_name),
        InputMode::ConcordancePicker => return handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
        InputMode::ConcordanceWordPicker => return handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
        InputMode::ConcordanceListPicker => return handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
        InputMode::ActionPopup => return handle_action_popup_key(state, key_name, is_ctrl, tokio_handle),
        InputMode::Visual => return handle_visual_key(state, key_state, key_name),
    }
    // Reader mode: Escape handler, then keymap dispatch
    ...
}
```

### Shared Picker Handler

`handle_picker_key` uses `InputMode` to select picker-specific behavior:

```rust
fn handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode) -> bool {
    let action = resolve_picker_key(key_name, is_ctrl);
    match action {
        Hide => { /* mode-specific hide */ }
        Confirm => { /* mode-specific confirm */ }
        MoveDown => { /* mode-specific move_selection(1) */ }
        MoveUp => { /* mode-specific move_selection(-1) */ }
        Unhandled => { /* mode-specific extras */ }
    }
}
```

The per-mode dispatch inside `handle_picker_key` uses a match on `mode` for each action — same code as the current per-picker blocks, just consolidated.

### What Doesn't Change

- `Keymap`/`Action`/`dispatch_action` — stays in `keymap.rs`, handles Reader mode only
- `resolve_picker_key` — stays in `picker_keys.rs`
- `SettingsChange` / `adjust_value` — stays in `settings_overlay.rs`
- Overlay widget code — each overlay's `.rs` file is unchanged
