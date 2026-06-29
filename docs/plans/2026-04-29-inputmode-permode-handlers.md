# InputMode Enum + Per-Mode Handler Functions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `InputMode` enum to `AppState` to replace the 12 sequential `is_visible()` checks in `handle_key`, then extract each mode's block into a standalone handler function, reducing `handle_key` to ~40 lines.

**Architecture:** Part B adds the `InputMode` enum and wires show/hide calls to set it. Part C extracts handler functions and restructures `handle_key` as a match on `input_mode`. These are done sequentially — B first (compile-verified), then C (compile-verified).

**Tech Stack:** Rust, GTK4

---

### Task 1: Add InputMode enum and field to AppState

**Files:**
- Modify: `src/app.rs` (add enum + field)

- [ ] **Step 1: Add the InputMode enum**

At the top of `src/app.rs`, after the existing imports and before the `AppState` struct, add:

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

- [ ] **Step 2: Add the field to AppState**

In the `AppState` struct definition, add after the last field:

```rust
    pub input_mode: InputMode,
```

- [ ] **Step 3: Initialize in AppState construction**

Find where `AppState` is constructed (in `build_window` or similar). Add `input_mode: InputMode::Reader,` to the struct literal.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles. The field exists but nothing reads or writes it yet.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "Add InputMode enum and field to AppState

14 variants covering every overlay mode plus Reader. Initialized
to Reader. No consumers yet — wired in the next task."
```

---

### Task 2: Wire show/hide calls to set input_mode

Every overlay show/hide path sets `input_mode`. This is a mechanical change — add one line at each site.

**Files:**
- Modify: `src/input/keymap.rs` (show/hide calls in handle_key and dispatch_action)
- Modify: `src/input/actions/pickers.rs` (show calls in open_bookmark_picker, open_media_picker, confirm_media_selection hide, set_media_default, delete_bookmark)
- Modify: `src/input/actions/settings.rs` (revert_to_snapshot hide)
- Modify: `src/input/actions/concordance.rs` (concordance_picker show)
- Modify: `src/input/visual.rs` (enter_visual_mode, exit_visual_mode, open_action_popup, close_action_popup, gloss show)

The pattern is: immediately after every `.set_visible(true)` / `.show()` call, add `state.borrow_mut().input_mode = InputMode::X;`. After every `.hide()` / `.set_visible(false)`, add `state.borrow_mut().input_mode = InputMode::Reader;`.

**Important edge cases:**
- When switching between overlays (e.g., keybinds → gamepad), the hide+show sets mode to the new overlay, not Reader.
- When `close_action_popup` is called, mode goes back to Visual (not Reader) if visual selection is still active. Check `visual_selection.is_some()` to decide.
- Library picker uses `show_prepare` (mutable) + `show_finish` (immutable, does the widget show). Set mode in `show_finish` path since that's where the widget becomes visible.

- [ ] **Step 1: Add InputMode assignments to keymap.rs**

This is the largest set of changes. At each show/hide site in `handle_key` and `dispatch_action`, add the mode assignment. The subagent implementing this task should:

1. Read `src/input/keymap.rs` fully
2. Find every `.hide()`, `.show()`, `.set_visible(true/false)`, `show_prepare`/`show_finish` call
3. Add `state.borrow_mut().input_mode = InputMode::X;` (or set it on an existing `&mut s` borrow) at each site
4. Use `crate::app::InputMode` as the type

Key sites in keymap.rs (non-exhaustive — the subagent must find all):
- Library picker show_finish → LibraryPicker, hide → Reader
- Bookmark picker hide → Reader (show is in actions/pickers.rs)
- Media picker hide → Reader (show is in actions/pickers.rs)
- Settings overlay show → Settings, hide → Reader
- Search bar show → Search, hide → Reader
- Correction overlay hide → Reader (show is in visual.rs)
- Gamepad overlay hide → Reader, show → GamepadOverlay
- Keybinds overlay show → KeybindsOverlay, hide → Reader
- Concordance picker hide → Reader (show is in actions/concordance.rs)
- Concordance word picker show → ConcordanceWordPicker, hide → Reader
- Concordance list picker hide → Reader
- Visual mode exit → Reader (enter is in dispatch_action)
- Action popup close → Visual (not Reader)

- [ ] **Step 2: Add InputMode assignments to action modules**

In `src/input/actions/pickers.rs`:
- `open_bookmark_picker`: after `state_clone.borrow().bookmark_picker.show();` → set BookmarkPicker
- `open_media_picker`: after `state_clone.borrow().media_picker.show();` → set MediaPicker
- `confirm_media_selection`: after `s.media_picker.hide();` → set Reader
- `delete_bookmark`: after `s.bookmark_picker.hide();` → set Reader

In `src/input/actions/settings.rs`:
- `revert_to_snapshot`: after `s.settings_overlay.hide();` → set Reader

In `src/input/actions/concordance.rs`:
- After `s.concordance_picker.show();` → set ConcordancePicker

In `src/input/visual.rs`:
- `enter_visual_mode`: set Visual
- `exit_visual_mode`: set Reader
- `open_action_popup`: set ActionPopup
- `close_action_popup`: set Visual (visual selection still active)
- After `state.correction_overlay.show(...)`: set GlossOverlay

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs src/input/actions/pickers.rs src/input/actions/settings.rs src/input/actions/concordance.rs src/input/visual.rs
git commit -m "Wire show/hide calls to set input_mode

Every overlay show sets input_mode to the corresponding variant.
Every hide sets it back to Reader (except close_action_popup
which returns to Visual)."
```

---

### Task 3: Replace is_visible cascade with match on input_mode

Restructure `handle_key` to dispatch on `state.borrow().input_mode` instead of sequential `is_visible()` checks.

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Replace the overlay blocks with a match**

At the top of `handle_key`, after the KEY log line, replace everything from `let picker_visible = ...` through the end of the concordance list picker block with:

```rust
    let mode = state.borrow().input_mode;
    if mode != crate::app::InputMode::Reader {
        match mode {
            crate::app::InputMode::LibraryPicker => {
                // ... existing library picker block (Ctrl+n/p + main match) ...
                // Keep the ENTIRE current library picker block here unchanged.
            }
            crate::app::InputMode::BookmarkPicker => {
                // ... existing bookmark picker block ...
            }
            crate::app::InputMode::MediaPicker => {
                // ... existing media picker block ...
            }
            crate::app::InputMode::Settings => {
                // ... existing settings block ...
            }
            crate::app::InputMode::Search => {
                // ... existing search block ...
            }
            crate::app::InputMode::GlossOverlay => {
                // ... existing gloss block ...
            }
            crate::app::InputMode::GamepadOverlay => {
                // ... existing gamepad block ...
            }
            crate::app::InputMode::KeybindsOverlay => {
                // ... existing keybinds block ...
            }
            crate::app::InputMode::ConcordancePicker => {
                // ... existing concordance picker block ...
            }
            crate::app::InputMode::ConcordanceWordPicker => {
                // ... existing concordance word picker block ...
            }
            crate::app::InputMode::ConcordanceListPicker => {
                // ... existing concordance list picker block ...
            }
            crate::app::InputMode::ActionPopup => {
                // ... existing action popup block ...
            }
            crate::app::InputMode::Visual => {
                // ... existing visual mode block ...
            }
            crate::app::InputMode::Reader => unreachable!(),
        }
    }
```

Each arm contains the exact code from the current if-block for that overlay, minus the `is_visible()` check (the match already proved we're in that mode). The `let X_visible = state.borrow().X.is_visible();` lines are removed.

Keep the Ctrl+Shift+p / Ctrl+Alt+p / Ctrl+p blocks at the very top (before the match) since those open pickers from Reader mode and need to run regardless.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Replace is_visible cascade with match on input_mode

handle_key now dispatches to overlay blocks via a single match on
state.input_mode instead of 12 sequential is_visible() checks.
Block contents unchanged — pure structural refactor."
```

---

### Task 4: Extract per-mode handler functions

Extract each match arm into a standalone function. `handle_key` becomes ~40 lines.

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Extract handler functions**

For each mode, cut the match arm body into a new function at the bottom of keymap.rs (before `dispatch_action`). Each function takes only the parameters it uses and returns `bool`.

Functions to create:

```rust
fn handle_library_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool { /* library picker block */ }

fn handle_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
    mode: crate::app::InputMode,
) -> bool {
    use crate::input::picker_keys::{resolve_picker_key, PickerAction};
    match resolve_picker_key(key_name, is_ctrl) {
        PickerAction::Hide => { /* dispatch hide by mode */ }
        PickerAction::Confirm => { /* dispatch confirm by mode */ }
        PickerAction::MoveDown => { /* dispatch move(1) by mode */ }
        PickerAction::MoveUp => { /* dispatch move(-1) by mode */ }
        PickerAction::Unhandled => { /* dispatch extras by mode */ }
    }
    /* Each PickerAction arm contains a match on `mode` for the 5 picker variants */
}

fn handle_settings_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool { /* settings block */ }

fn handle_search_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool { /* search block */ }

fn handle_gloss_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool { /* gloss block */ }

fn handle_gamepad_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool { /* gamepad block — just Escape */ }

fn handle_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool { /* keybinds block */ }

fn handle_action_popup_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool { /* action popup block */ }

fn handle_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool { /* visual mode block */ }
```

The `handle_picker_key` function consolidates the 5 picker modes (BookmarkPicker, MediaPicker, ConcordancePicker, ConcordanceWordPicker, ConcordanceListPicker) since they all go through `resolve_picker_key` with mode-specific extras.

- [ ] **Step 2: Update handle_key to call handlers**

Replace the match arms with handler calls:

```rust
    let mode = state.borrow().input_mode;
    if mode != crate::app::InputMode::Reader {
        return match mode {
            crate::app::InputMode::LibraryPicker => handle_library_picker_key(state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::BookmarkPicker
            | crate::app::InputMode::MediaPicker
            | crate::app::InputMode::ConcordancePicker
            | crate::app::InputMode::ConcordanceWordPicker
            | crate::app::InputMode::ConcordanceListPicker => handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
            crate::app::InputMode::Settings => handle_settings_key(state, key_name, is_ctrl),
            crate::app::InputMode::Search => handle_search_key(state, key_name),
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_name),
            crate::app::InputMode::GamepadOverlay => handle_gamepad_key(state, key_name),
            crate::app::InputMode::KeybindsOverlay => handle_keybinds_key(state, key_state, key_name),
            crate::app::InputMode::ActionPopup => handle_action_popup_key(state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name),
            crate::app::InputMode::Reader => unreachable!(),
        };
    }
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all tests pass (existing keymap_config and picker_keys tests).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Extract per-mode handler functions from handle_key

handle_key is now ~40 lines: log, match on input_mode, delegate to
handler function, then Reader-mode keymap dispatch. 9 handler
functions extracted. handle_picker_key consolidates 5 picker modes
via resolve_picker_key with mode-specific extras."
```
