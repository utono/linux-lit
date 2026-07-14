# Shared Picker Key Dispatch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract duplicated key handling across 5 list pickers and the settings overlay into a shared `resolve_picker_key` function, remove j/k bindings from all pickers, and wire each picker to the shared resolver.

**Architecture:** New `src/input/picker_keys.rs` defines `PickerAction` enum and `resolve_picker_key()`. Each picker block in `keymap.rs` calls the resolver first, matches common actions, then falls through to picker-specific extras. The separate `if visible && is_ctrl` pre-dispatch blocks for Ctrl+n/Ctrl+p are removed since the resolver handles them.

**Tech Stack:** Rust, GTK4

---

### Task 1: Create picker_keys module with resolve_picker_key

**Files:**
- Create: `src/input/picker_keys.rs`
- Modify: `src/input/mod.rs`

- [ ] **Step 1: Write test**

Create `src/input/picker_keys.rs` with the enum, function, and tests:

```rust
pub(crate) enum PickerAction {
    Hide,
    Confirm,
    MoveDown,
    MoveUp,
    Unhandled,
}

pub(crate) fn resolve_picker_key(key_name: &str, is_ctrl: bool) -> PickerAction {
    if is_ctrl {
        match key_name {
            "n" => return PickerAction::MoveDown,
            "p" => return PickerAction::MoveUp,
            _ => {}
        }
    }
    match key_name {
        "Escape" => PickerAction::Hide,
        "Return" => PickerAction::Confirm,
        "Down" => PickerAction::MoveDown,
        "Up" => PickerAction::MoveUp,
        _ => PickerAction::Unhandled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_returns_hide() {
        assert!(matches!(resolve_picker_key("Escape", false), PickerAction::Hide));
    }

    #[test]
    fn return_returns_confirm() {
        assert!(matches!(resolve_picker_key("Return", false), PickerAction::Confirm));
    }

    #[test]
    fn down_and_ctrl_n_return_move_down() {
        assert!(matches!(resolve_picker_key("Down", false), PickerAction::MoveDown));
        assert!(matches!(resolve_picker_key("n", true), PickerAction::MoveDown));
    }

    #[test]
    fn up_and_ctrl_p_return_move_up() {
        assert!(matches!(resolve_picker_key("Up", false), PickerAction::MoveUp));
        assert!(matches!(resolve_picker_key("p", true), PickerAction::MoveUp));
    }

    #[test]
    fn other_keys_return_unhandled() {
        assert!(matches!(resolve_picker_key("j", false), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("k", false), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("a", false), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("n", false), PickerAction::Unhandled));
    }

    #[test]
    fn ctrl_with_non_nav_keys_returns_unhandled() {
        assert!(matches!(resolve_picker_key("a", true), PickerAction::Unhandled));
        assert!(matches!(resolve_picker_key("j", true), PickerAction::Unhandled));
    }
}
```

- [ ] **Step 2: Register module**

In `src/input/mod.rs`, add after the last `pub mod` line:

```rust
pub mod picker_keys;
```

- [ ] **Step 3: Run tests**

Run: `cargo test picker_keys::tests`
Expected: 6 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/input/picker_keys.rs src/input/mod.rs
git commit -m "Add picker_keys module with resolve_picker_key

PickerAction enum (Hide, Confirm, MoveDown, MoveUp, Unhandled) and
resolve_picker_key function that maps Escape, Return, Down, Up,
Ctrl+n, Ctrl+p to the shared actions. 6 tests."
```

---

### Task 2: Wire bookmark picker to shared dispatch

Replace the bookmark picker's two blocks (Ctrl+n/p pre-dispatch at lines 158-170 and main match at lines 172-226) with a single block using `resolve_picker_key`.

**Files:**
- Modify: `src/input/keymap.rs:155-226`

- [ ] **Step 1: Replace both bookmark picker blocks**

Find the two bookmark picker blocks. The first block is:
```rust
    if bookmark_picker_visible && is_ctrl {
        match key_name {
            "n" => { ... }
            "p" => { ... }
            _ => {}
        }
    }
```

The second block is:
```rust
    if bookmark_picker_visible {
        match key_name {
            "Escape" => { ... }
            "Return" => { ... }
            "Delete" | "d" => { ... }
            "Down" | "j" => { ... }
            "Up" | "k" => { ... }
            _ => {}
        }
        return false;
    }
```

Delete BOTH blocks (the `if bookmark_picker_visible && is_ctrl` block AND the `if bookmark_picker_visible` block) and replace with a single block:

```rust
    if bookmark_picker_visible {
        use crate::input::picker_keys::{resolve_picker_key, PickerAction};
        match resolve_picker_key(key_name, is_ctrl) {
            PickerAction::Hide => {
                state.borrow().bookmark_picker.hide();
                return true;
            }
            PickerAction::Confirm => {
                let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
                if let Some(lm_id) = selected_id {
                    {
                        let s = state.borrow();
                        s.bookmark_picker.hide();
                    }
                    let mut s = state.borrow_mut();
                    let buffer_line = if let Some(ref lm) = s.line_map {
                        s.current_work.as_ref().and_then(|w| {
                            let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                            Some(lm.work_to_buffer[work_idx])
                        })
                    } else {
                        s.current_work.as_ref().and_then(|w| {
                            w.lines.iter().position(|l| l.id == lm_id)
                        })
                    };
                    if let Some(bl) = buffer_line {
                        navigation::jump_to_line(&mut s, bl);
                    }
                }
                return true;
            }
            PickerAction::MoveDown => {
                state.borrow().bookmark_picker.move_selection(1);
                return true;
            }
            PickerAction::MoveUp => {
                state.borrow().bookmark_picker.move_selection(-1);
                return true;
            }
            PickerAction::Unhandled => {
                if key_name == "Delete" || key_name == "d" {
                    crate::input::actions::pickers::delete_bookmark(state, tokio_handle);
                    return true;
                }
            }
        }
        return false;
    }
```

Note: j/k removed. Delete/d no longer has search-focus gating (it's always available). Down/Up no longer have search-focus gating (arrow keys don't conflict with search text input).

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire bookmark picker to shared picker dispatch

Replace two separate blocks (Ctrl+n/p pre-dispatch + main match)
with a single resolve_picker_key call. Remove j/k bindings.
Delete/d always available (no search-focus gating needed)."
```

---

### Task 3: Wire media picker to shared dispatch

Replace the media picker's two blocks (Ctrl+n/p pre-dispatch at lines 232-244 and main match at lines 246-280) with a single block.

**Files:**
- Modify: `src/input/keymap.rs` (media picker blocks — line numbers will have shifted after Task 2, so find by the `let media_picker_visible` line)

- [ ] **Step 1: Replace both media picker blocks**

Find the `let media_picker_visible` line and the two blocks that follow. Delete BOTH the `if media_picker_visible && is_ctrl` block AND the `if media_picker_visible` block. Replace with:

```rust
    if media_picker_visible {
        use crate::input::picker_keys::{resolve_picker_key, PickerAction};
        match resolve_picker_key(key_name, is_ctrl) {
            PickerAction::Hide => {
                state.borrow().media_picker.hide();
                return true;
            }
            PickerAction::Confirm => {
                crate::input::actions::pickers::confirm_media_selection(state, tokio_handle);
                return true;
            }
            PickerAction::MoveDown => {
                state.borrow().media_picker.move_selection(1);
                return true;
            }
            PickerAction::MoveUp => {
                state.borrow().media_picker.move_selection(-1);
                return true;
            }
            PickerAction::Unhandled => {
                if key_name == "p" {
                    let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
                    if !is_search_focused {
                        crate::input::actions::pickers::set_media_default(state, tokio_handle);
                        return true;
                    }
                }
            }
        }
        return false;
    }
```

Note: j/k removed. The `"p"` extra retains search-focus gating because `p` is a typeable character that conflicts with the search entry.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire media picker to shared picker dispatch

Replace two separate blocks with a single resolve_picker_key call.
Remove j/k bindings. p-for-default retains search-focus gating."
```

---

### Task 4: Wire settings overlay to shared dispatch

Replace the settings overlay's two blocks (Ctrl+n/p pre-dispatch at lines 286-298 and main match at lines 301-348) with a single block.

**Files:**
- Modify: `src/input/keymap.rs` (settings overlay blocks — find by the `let settings_visible` line)

- [ ] **Step 1: Replace both settings overlay blocks**

Find the `let settings_visible` line and the two blocks that follow. Delete BOTH the `if settings_visible && is_ctrl` block AND the `if settings_visible` block. Replace with:

```rust
    if settings_visible {
        use crate::input::picker_keys::{resolve_picker_key, PickerAction};
        match resolve_picker_key(key_name, is_ctrl) {
            PickerAction::Hide => {
                crate::input::actions::settings::revert_to_snapshot(state);
                return true;
            }
            PickerAction::Confirm => {
                {
                    let s = state.borrow_mut();
                    crate::config::save(&s.config);
                    s.settings_overlay.hide();
                }
                return true;
            }
            PickerAction::MoveDown => {
                state.borrow_mut().settings_overlay.move_selection(1);
                return true;
            }
            PickerAction::MoveUp => {
                state.borrow_mut().settings_overlay.move_selection(-1);
                return true;
            }
            PickerAction::Unhandled => {
                match key_name {
                    "h" | "Left" => {
                        let (ls, cw, tm, nm, ts, cl) = {
                            let s = state.borrow();
                            (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style, s.config.show_cursor_line)
                        };
                        let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm, nm, ts, cl);
                        crate::input::actions::settings::apply_settings_change(state, change);
                        return true;
                    }
                    "l" | "Right" => {
                        let (ls, cw, tm, nm, ts, cl) = {
                            let s = state.borrow();
                            (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style, s.config.show_cursor_line)
                        };
                        let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm, nm, ts, cl);
                        crate::input::actions::settings::apply_settings_change(state, change);
                        return true;
                    }
                    "r" => {
                        crate::input::actions::settings::reset_to_defaults(state);
                        return true;
                    }
                    _ => return true, // consume all other keys when settings visible
                }
            }
        }
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire settings overlay to shared picker dispatch

Replace two separate blocks with a single resolve_picker_key call.
Remove j/k bindings. Settings-specific h/l/r keys handled in
Unhandled fallback."
```

---

### Task 5: Wire concordance picker to shared dispatch

Replace the concordance picker block (lines starting at `if state.borrow().concordance_picker.is_visible()`) with shared dispatch.

**Files:**
- Modify: `src/input/keymap.rs` (concordance picker block — find by `concordance_picker.is_visible()`)

- [ ] **Step 1: Replace concordance picker block**

Find the block starting with `if state.borrow().concordance_picker.is_visible() {`. It currently has inline `if is_ctrl` checks for Ctrl+n/Ctrl+p, then a match for Down/Up/Return/Escape. Replace the entire block (from the `if` through its closing `}`) with:

```rust
    if state.borrow().concordance_picker.is_visible() {
        use crate::input::picker_keys::{resolve_picker_key, PickerAction};
        match resolve_picker_key(key_name, is_ctrl) {
            PickerAction::Hide => {
                state.borrow().concordance_picker.hide();
                return true;
            }
            PickerAction::Confirm => {
                let selected = state.borrow().concordance_picker.selected_word();
                state.borrow().concordance_picker.hide();
                if let Some(word) = selected {
                    crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
                }
                return true;
            }
            PickerAction::MoveDown => {
                state.borrow().concordance_picker.move_selection(1);
                return true;
            }
            PickerAction::MoveUp => {
                state.borrow().concordance_picker.move_selection(-1);
                return true;
            }
            PickerAction::Unhandled => {}
        }
        return false;
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire concordance picker to shared picker dispatch

Replace inline Ctrl+n/p checks and match block with a single
resolve_picker_key call."
```

---

### Task 6: Wire concordance word picker to shared dispatch

Replace the concordance word picker block (find by `conc_word_picker_visible`).

**Files:**
- Modify: `src/input/keymap.rs` (concordance word picker block)

- [ ] **Step 1: Replace concordance word picker block**

Find the `let conc_word_picker_visible` line and the `if conc_word_picker_visible {` block. Replace the entire block with:

```rust
    if conc_word_picker_visible {
        use crate::input::picker_keys::{resolve_picker_key, PickerAction};
        match resolve_picker_key(key_name, is_ctrl) {
            PickerAction::Hide => {
                state.borrow().concordance_word_picker.hide();
                return true;
            }
            PickerAction::Confirm => {
                let selected = state.borrow().concordance_word_picker.selected_word();
                state.borrow().concordance_word_picker.hide();
                if let Some(word) = selected {
                    crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
                }
                return true;
            }
            PickerAction::MoveDown => {
                state.borrow().concordance_word_picker.move_selection(1);
                return true;
            }
            PickerAction::MoveUp => {
                state.borrow().concordance_word_picker.move_selection(-1);
                return true;
            }
            PickerAction::Unhandled => {}
        }
        return false;
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire concordance word picker to shared picker dispatch

Replace inline Ctrl+n/p checks and match block with a single
resolve_picker_key call."
```

---

### Task 7: Wire concordance list picker to shared dispatch

Replace the concordance list picker block (find by `conc_list_picker_visible`).

**Files:**
- Modify: `src/input/keymap.rs` (concordance list picker block)

- [ ] **Step 1: Replace concordance list picker block**

Find the `let conc_list_picker_visible` line and the `if conc_list_picker_visible {` block. Replace the entire block with:

```rust
    if conc_list_picker_visible {
        use crate::input::picker_keys::{resolve_picker_key, PickerAction};
        match resolve_picker_key(key_name, is_ctrl) {
            PickerAction::Hide => {
                state.borrow().concordance_list_picker.hide();
                return true;
            }
            PickerAction::Confirm => {
                let selected = state.borrow().concordance_list_picker.selected_index();
                state.borrow().concordance_list_picker.hide();
                if let Some(idx) = selected {
                    {
                        let mut s = state.borrow_mut();
                        if let Some(conc) = &mut s.concordance_state {
                            conc.current_index = idx;
                        }
                    }
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
                return true;
            }
            PickerAction::MoveDown => {
                state.borrow().concordance_list_picker.move_selection(1);
                return true;
            }
            PickerAction::MoveUp => {
                state.borrow().concordance_list_picker.move_selection(-1);
                return true;
            }
            PickerAction::Unhandled => {}
        }
        return false;
    }
```

Note: j/n and k/p movement bindings removed. Only Down/Up/Ctrl+n/Ctrl+p navigate.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire concordance list picker to shared picker dispatch

Replace j/n/k/p movement and inline Ctrl+n/p checks with a single
resolve_picker_key call. Only Down/Up/Ctrl+n/Ctrl+p navigate."
```

---

### Task 8: Update settings footer text and verify

**Files:**
- Modify: `src/ui/settings_overlay.rs:86`

- [ ] **Step 1: Update footer label**

In `src/ui/settings_overlay.rs`, find line 86:

```rust
            .label("j/k navigate · h/l adjust · r reset · Enter confirm · Esc revert")
```

Replace with:

```rust
            .label("↑↓ navigate · h/l adjust · r reset · Enter confirm · Esc revert")
```

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | grep -E "^error" | head -5`
Expected: no errors.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass (including the 6 new picker_keys tests and all existing keymap_config tests).

- [ ] **Step 4: Commit**

```bash
git add src/ui/settings_overlay.rs
git commit -m "Update settings footer to show arrow key navigation

Replace 'j/k navigate' with '↑↓ navigate' to match the removal of
j/k bindings from the settings overlay."
```
