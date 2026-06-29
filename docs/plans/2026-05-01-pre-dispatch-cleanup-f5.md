# Pre-dispatch Interception Cleanup (F5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the remaining pre-dispatch interception blocks from `handle_key` so that Reader mode keys flow through one path: `keymap.lookup() → dispatch_action()`.

**Architecture:** The Ctrl+n/p picker navigation block (lines 31-44) and the vocab popup interception block (lines 151-165) fire before the keymap lookup, creating implicit priority. Move Ctrl+n/p into the per-mode handlers where it already exists via `PickerAction`, and move vocab popup key handling into the dispatch verbs. The Ctrl+p/Ctrl+Shift+P/Ctrl+Alt+p blocks are addressed in the F3 cleanup plan (Task 8) and are prerequisites to this plan.

**Tech Stack:** Rust, GTK4

**Prerequisites:** The F3 cleanup plan (Task 8: remove Ctrl+p pre-dispatch blocks) should be done first.

---

### Task 1: Remove Ctrl+n/p picker pre-dispatch block

**Files:**
- Modify: `src/input/keymap.rs:29-44` (remove pre-dispatch block)

The `Ctrl+n/p` block at lines 31-44 fires when `picker_visible` is true, before the `InputMode` match at line 102. But `handle_library_picker_key` already handles Ctrl+n/p via the GTK search entry's native keyboard navigation, and `handle_picker_key` handles it via `resolve_picker_key` which returns `MoveDown`/`MoveUp` for Ctrl+n/p. The pre-dispatch block is redundant.

- [ ] **Step 1: Remove the picker_visible Ctrl+n/p block**

Remove this block from `handle_key` (lines 29-44):

```rust
    let picker_visible = state.borrow().picker.is_visible();

    // Ctrl+n/Ctrl+p navigate picker list when visible
    if picker_visible && is_ctrl {
        match key_name {
            "n" => {
                state.borrow().picker.move_selection(1);
                return true;
            }
            "p" => {
                state.borrow().picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
    }
```

- [ ] **Step 2: Build and test**

Run: `cargo build && cargo test`

Verify that `Ctrl+n`/`Ctrl+p` still work in the library picker. They're handled by `handle_library_picker_key` at line 306 (the `is_ctrl` block is a comment saying "Ctrl+n/p already handled at top of handle_key" — update that comment to remove the stale reference).

- [ ] **Step 3: Update stale comment in handle_library_picker_key**

In `handle_library_picker_key`, the `_ =>` arm has a comment referencing the removed block:

```rust
            // Ctrl+n/p already handled at top of handle_key; let GTK route
            // remaining keys to the search entry.
```

Replace with:

```rust
            // Let GTK route remaining keys to the search entry.
```

- [ ] **Step 4: Add Ctrl+n/p handling to handle_library_picker_key**

The library picker needs explicit Ctrl+n/p handling since the removed pre-dispatch block was doing it. Add at the top of the function's match:

```rust
        "n" if is_ctrl => {
            state.borrow().picker.move_selection(1);
            true
        }
        "p" if is_ctrl => {
            state.borrow().picker.move_selection(-1);
            true
        }
```

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Move Ctrl+n/p picker navigation from pre-dispatch to handle_library_picker_key"
```

---

### Task 2: Move vocab popup interception into dispatch verbs

**Files:**
- Modify: `src/input/keymap.rs:151-165` (remove pre-dispatch block)
- Modify: `src/input/keymap.rs` dispatch_action arms for PendingG and TogglePlayback

The vocab popup intercepts `g` (toggle view) and `Tab` (toggle playback) before the keymap lookup at line 225. These should be handled by the dispatch verbs themselves checking popup visibility.

- [ ] **Step 1: Move vocab popup `g` handling into PendingG dispatch**

The `g` key when vocab popup is visible calls `vocab_popup_toggle_view`. In the `PendingG` dispatch arm, add a popup-visible check before starting the chord:

```rust
        PendingG => {
            if state.borrow().vocab_popup.is_visible() {
                crate::app::vocab_popup_toggle_view(&mut state.borrow_mut());
            } else {
                KeyState::start_chord(key_state, ChordState::PendingG);
            }
            true
        }
```

- [ ] **Step 2: Move vocab popup `Tab` handling**

The `Tab` key for vocab popup calls `toggle_playback`. The `TogglePlayback` action is already bound to `Tab` in `keymap_config.rs:241`. The pre-dispatch block is redundant — `toggle_playback` already works regardless of popup state. No change needed to the dispatch arm.

- [ ] **Step 3: Remove the vocab popup pre-dispatch block**

Remove the block at lines 151-165:

```rust
    // Other vocab popup keys (when popup is visible)
    if state.borrow().vocab_popup.is_visible() {
        match key_name {
            "g" => {
                crate::app::vocab_popup_toggle_view(&mut state.borrow_mut());
                return true;
            }
            "Tab" => {
                crate::input::search::toggle_playback(&mut state.borrow_mut());
                return true;
            }
            // Let h, j, k, Escape, and other keys fall through to normal handling
            _ => {}
        }
    }
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Move vocab popup key interceptions into dispatch verbs, remove pre-dispatch block"
```

---

### Task 3: Verify handle_key is clean

**Files:**
- Read: `src/input/keymap.rs` top of `handle_key`

- [ ] **Step 1: Verify no pre-dispatch blocks remain before mode dispatch**

After Tasks 1-2 (and the F3 plan's Task 8), `handle_key` should flow as:

1. Log the key event
2. `Ctrl+Alt+L` quit (global, not mode-specific)
3. Mode dispatch via `InputMode` match
4. Reader mode: Gloss toggle (`Shift+Tab`, `Ctrl+g`), Escape handler, keymap lookup → dispatch_action

The Gloss toggle and Escape blocks remain because they have multi-state preconditions that don't map cleanly to a single Action. This is acceptable — they're documented with comments explaining why they stay inline.

- [ ] **Step 2: Count lines in handle_key**

Run: `rg -n "^pub fn handle_key" src/input/keymap.rs` and `rg -n "^fn handle_library_picker_key" src/input/keymap.rs` to measure the reduction.

Expected: `handle_key` should be ~60-80 lines shorter than the original 230 lines (removed ~130 lines of pre-dispatch blocks).

- [ ] **Step 3: Build and run full test suite**

Run: `cargo build && cargo test && cargo clippy`
Expected: Clean.

- [ ] **Step 4: Commit (if any cleanup needed)**

```bash
git add src/input/keymap.rs
git commit -m "Final cleanup: verify handle_key has no remaining pre-dispatch interceptions"
```
