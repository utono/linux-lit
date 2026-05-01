# Keymap Cleanup (F3, F4, F7, F2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the last pre-dispatch bypasses in `handle_key`, simplify `dispatch_action`'s return type, and extract concordance/word-copy helpers out of `navigation.rs` so it contains only pure cursor verbs.

**Architecture:** Four independent refactors applied in order: (1) move the inline Escape handler into an `EscapeReaderMode` Action dispatched through the keymap, (2) move the inline gloss-toggle handler into a `ToggleGlossOverlay` Action, (3) change `dispatch_action` from `-> bool` to `-> ()`, (4) move concordance navigation and word-copy functions from `navigation.rs` to `actions/concordance.rs` and a new `actions/word_copy.rs`.

**Tech Stack:** Rust, GTK4, sourceview5

---

### Task 1: Extract Escape handler into `EscapeReaderMode` Action (F3)

**Files:**
- Modify: `src/input/actions/mod.rs` — add `EscapeReaderMode` variant to `Action` enum
- Modify: `src/input/keymap_config.rs` — bind `Escape` to `EscapeReaderMode` in `app_bindings()`
- Create: `src/input/actions/escape.rs` — verb implementing the precedence cascade
- Modify: `src/input/keymap.rs` — remove inline Escape block (lines 128-161), add dispatch arm

- [ ] **Step 1: Add `EscapeReaderMode` to Action enum**

In `src/input/actions/mod.rs`, add the variant to the `Action` enum:

```rust
    // App
    SaveAndQuit,
    ToggleDebugLogging,
    CopyLineMappingId,
    EscapeReaderMode,
```

Add it to the `category()` match (in the App section):

```rust
            | Action::EscapeReaderMode
```

Add it to the `name()` match:

```rust
            Action::EscapeReaderMode => "EscapeReaderMode",
```

- [ ] **Step 2: Add Escape keybind to `keymap_config.rs`**

In `src/input/keymap_config.rs`, add to `app_bindings()`:

```rust
        (KeyCombo::plain("Escape"), Action::EscapeReaderMode),
```

- [ ] **Step 3: Create `src/input/actions/escape.rs` with the verb**

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

pub(crate) fn escape_reader_mode(state: &Rc<RefCell<AppState>>) {
    // Concordance state takes priority
    {
        let has_conc = state.borrow().concordance_state.is_some();
        if has_conc {
            let mut s = state.borrow_mut();
            s.concordance_state = None;
            s.concordance_bar.hide();
            return;
        }
    }
    // AB loop
    {
        let is_ab_active = state.borrow().ab_repeat.loop_active;
        if is_ab_active {
            let mut s = state.borrow_mut();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
            s.ab_repeat.clear();
            s.ab_repeat.chunk_index = None;
            s.ab_a_line.set(None);
            s.ab_b_line.set(None);
            s.suppress_sync_until = None;
            if let Some(ref renderer) = s.gutter_renderer {
                renderer.queue_draw();
            }
            crate::app::remove_ab_dim(&s);
            crate::logging::log("CHUNK: AB loop cleared");
            drop(s);
            crate::input::navigation::update_highlight_and_center(&mut state.borrow_mut());
            return;
        }
    }
    // Search matches
    {
        let has_search = !state.borrow().search_matches.is_empty();
        if has_search {
            crate::input::search::clear_search(&mut state.borrow_mut());
        }
    }
}
```

- [ ] **Step 4: Register the module in `src/input/actions/mod.rs`**

Add at the top with the other module declarations:

```rust
pub mod escape;
```

- [ ] **Step 5: Add dispatch arm in `keymap.rs`**

In `dispatch_action`, add in the App section:

```rust
        EscapeReaderMode => { crate::input::actions::escape::escape_reader_mode(state); true }
```

- [ ] **Step 6: Remove the inline Escape block from `handle_key`**

Delete lines 126-161 in `keymap.rs` (the `// Escape: special multi-state handler` comment through the closing brace of the `if key_name == "Escape"` block).

- [ ] **Step 7: Build and verify**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/mod.rs src/input/actions/escape.rs src/input/keymap.rs src/input/keymap_config.rs
git commit -m "Move Escape handler into EscapeReaderMode Action dispatched through keymap"
```

---

### Task 2: Extract gloss toggle into `ToggleGlossOverlay` Action (F4)

**Files:**
- Modify: `src/input/actions/mod.rs` — add `ToggleGlossOverlay` variant
- Modify: `src/input/keymap_config.rs` — bind `ISO_Left_Tab` and `Ctrl+g` to it
- Modify: `src/input/actions/gloss.rs` — add `toggle_overlay()` function
- Modify: `src/input/keymap.rs` — remove inline gloss-toggle block, add dispatch arm

- [ ] **Step 1: Add `ToggleGlossOverlay` to Action enum**

In `src/input/actions/mod.rs`, add the variant:

```rust
    ToggleGlossOverlay,
```

Add it to the `category()` match (in the Vocab section, after `ToggleVocabHighlight`):

```rust
            | Action::ToggleGlossOverlay
```

Add it to the `name()` match:

```rust
            Action::ToggleGlossOverlay => "ToggleGlossOverlay",
```

- [ ] **Step 2: Add keybinds in `keymap_config.rs`**

In `vocab_bindings()`, add:

```rust
        (KeyCombo::plain("ISO_Left_Tab"), Action::ToggleGlossOverlay),
        (KeyCombo::ctrl("g"), Action::ToggleGlossOverlay),
```

- [ ] **Step 3: Add `toggle_overlay()` to `actions/gloss.rs`**

Add at the end of the file:

```rust
pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    let has_gloss = !state.borrow().gloss_list.is_empty();
    if has_gloss {
        let s = state.borrow();
        let idx = s.gloss_index;
        let gloss = &s.gloss_list[idx];
        let ctx = s.gloss_context.as_ref().unwrap();
        let h = s.scrolled_window.height();
        let pairs = ctx.source_line_pairs();
        s.gloss_overlay.show_gloss_with_color(
            &ctx.source_text, &gloss.gloss_text, h,
            Some(&s.theme.root_color), &pairs,
        );
        s.gloss_overlay.set_position(idx, s.gloss_list.len());
        drop(s);
        state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
    }
}
```

- [ ] **Step 4: Add dispatch arm in `keymap.rs`**

In `dispatch_action`, add in the Vocab section:

```rust
        ToggleGlossOverlay => { crate::input::actions::gloss::toggle_overlay(state); true }
```

- [ ] **Step 5: Remove the inline gloss-toggle block from `handle_key`**

Delete the `// Shift+Tab or Ctrl+g: toggle gloss overlay` comment and the entire `if key_name == "ISO_Left_Tab" || (is_ctrl && key_name == "g")` block (lines 107-124 in the current file, which will have shifted after Task 1's deletions).

- [ ] **Step 6: Build and verify**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/input/actions/gloss.rs src/input/keymap.rs src/input/keymap_config.rs
git commit -m "Move gloss overlay toggle into ToggleGlossOverlay Action dispatched through keymap"
```

---

### Task 3: Change `dispatch_action` to return `()` (F7)

**Files:**
- Modify: `src/input/keymap.rs` — change return type, simplify all arms

- [ ] **Step 1: Change `dispatch_action` signature and call site**

In `src/input/keymap.rs`, change the call site in `handle_key` from:

```rust
    if let Some(action) = action {
        return dispatch_action(state, action, key_state, tokio_handle);
    }
```

to:

```rust
    if let Some(action) = action {
        dispatch_action(state, action, key_state, tokio_handle);
        return true;
    }
```

- [ ] **Step 2: Change `dispatch_action` return type and remove `true` from all arms**

Change the function signature from:

```rust
fn dispatch_action(
    state: &Rc<RefCell<AppState>>,
    action: crate::input::actions::Action,
    key_state: &Rc<RefCell<KeyState>>,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
```

to:

```rust
fn dispatch_action(
    state: &Rc<RefCell<AppState>>,
    action: crate::input::actions::Action,
    key_state: &Rc<RefCell<KeyState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
```

Then remove `; true` or `true` from every match arm. Arms that were `{ verb(); true }` become `{ verb(); }`. Arms that were just `true` after a verb become `verb()`. The `SearchNextMatch` and `SearchPrevMatch` arms that conditionally returned `false` simply call the verb unconditionally:

```rust
        SearchNextMatch => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::next_match(&mut state.borrow_mut());
            }
        }
        SearchPrevMatch => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::prev_match(&mut state.borrow_mut());
            }
        }
```

Also change `SetEndTime`, `SetChapter`, `DeleteTimestamp`, `NudgeStartBackward`, `NudgeStartForward`, `UndoTimestamp` which currently return the bool from the timestamp function directly — wrap them:

```rust
        SetEndTime => { crate::input::timestamps::set_end_time(&mut state.borrow_mut()); }
        SetChapter => { crate::input::timestamps::set_chapter(&mut state.borrow_mut()); }
        DeleteTimestamp => { crate::input::timestamps::delete_timestamp(&mut state.borrow_mut()); }
        NudgeStartBackward => { crate::input::timestamps::nudge_start_backward(&mut state.borrow_mut()); }
        NudgeStartForward => { crate::input::timestamps::nudge_start_forward(&mut state.borrow_mut()); }
        UndoTimestamp => { crate::input::timestamps::undo_timestamp(&mut state.borrow_mut()); }
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: compiles cleanly (timestamp functions return `bool` but we discard it — Rust allows this)

- [ ] **Step 4: Run clippy to check for warnings**

Run: `cargo clippy`
Expected: no new warnings (discarding `bool` return values is fine; if clippy warns about `#[must_use]` on timestamp functions, that's a pre-existing issue, not from this change)

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Change dispatch_action to return () — keymap match always consumes the key"
```

---

### Task 4: Move concordance navigation from `navigation.rs` to `actions/concordance.rs` (F2 part 1)

**Files:**
- Modify: `src/input/navigation.rs` — remove concordance functions (lines 875-1035)
- Modify: `src/input/actions/concordance.rs` — receive the moved functions
- Modify: `src/input/keymap.rs` — update call site for `concordance_jump_to_current`
- Modify: `src/app.rs` — update call site for `concordance_jump_to_current`

- [ ] **Step 1: Move concordance functions to `actions/concordance.rs`**

Cut these functions from `navigation.rs` (the entire `// Cross-work concordance navigation` section through `concordance_update_bar`):
- `concordance_jump_to_current` (pub)
- `concordance_resolve_indices` (private)
- `concordance_seek` (private)
- `concordance_position_cursor` (private)
- `find_sentence_start_by_timestamp` (private)
- `concordance_update_bar` (private)

Paste them into `actions/concordance.rs` after the existing functions. Update their imports — they need:

```rust
use crate::input::navigation::SEEK_PREROLL;
use crate::input::scroll::center_cursor;
use crate::input::highlight::update_highlight;
```

Replace uses of `update_highlight(state)` with the imported function, and `center_cursor(state)` with the imported function. These were previously called via the re-export in `navigation.rs` but will now be called directly from the sibling modules. Note: `center_cursor` and `update_highlight` are `pub(crate)` in their modules, so `actions/concordance.rs` (within the same crate) can import them directly.

- [ ] **Step 2: Remove the re-export from `navigation.rs` if present**

`concordance_jump_to_current` is public. After moving it, remove it from `navigation.rs`. The callers will update in the next steps.

- [ ] **Step 3: Update callers to use the new path**

In `src/input/keymap.rs`, change:

```rust
navigation::concordance_jump_to_current(state, tokio_handle);
```

to:

```rust
crate::input::actions::concordance::concordance_jump_to_current(state, tokio_handle);
```

In `src/app.rs`, change:

```rust
crate::input::navigation::concordance_jump_to_current(
```

to:

```rust
crate::input::actions::concordance::concordance_jump_to_current(
```

In `src/input/actions/concordance.rs`, change the three existing calls from:

```rust
navigation::concordance_jump_to_current(&state_clone, &handle);
navigation::concordance_jump_to_current(state, tokio_handle);
```

to local calls (since the function is now in the same file):

```rust
concordance_jump_to_current(&state_clone, &handle);
concordance_jump_to_current(state, tokio_handle);
```

Remove the `use crate::input::navigation;` import from `actions/concordance.rs` if it is no longer needed (check if `navigation::jump_to_next_vocab` / `navigation::jump_to_prev_vocab` are still called — they are, so keep the import but note it now only serves the vocab jump functions).

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all tests pass (concordance functions have no unit tests; their callers are the same)

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs src/input/actions/concordance.rs src/input/keymap.rs src/app.rs
git commit -m "Move concordance navigation functions from navigation.rs to actions/concordance.rs"
```

---

### Task 5: Move word-copy functions from `navigation.rs` to `actions/word_copy.rs` (F2 part 2)

**Files:**
- Create: `src/input/actions/word_copy.rs` — receives word_cycle_copy, word_collect_copy, helpers
- Modify: `src/input/actions/mod.rs` — add `pub mod word_copy;`
- Modify: `src/input/navigation.rs` — remove word-copy functions (lines ~1040-1205)
- Modify: `src/input/keymap.rs` — update dispatch arms for `WordCycleCopy` and `WordCollectCopy`

- [ ] **Step 1: Create `src/input/actions/word_copy.rs`**

Cut these functions from `navigation.rs` (the entire `// Word copy` section):
- `word_cycle_copy` (pub)
- `word_collect_copy` (pub)
- `extract_buffer_line_words` (private)
- `apply_word_underline` (private)

Create `src/input/actions/word_copy.rs` with the moved functions. Update the imports at the top:

```rust
use gtk4::prelude::*;

use crate::app::AppState;
use crate::log_fmt;
```

The functions use `glib::timeout_add_local_once` which comes from `gtk4::prelude::*`. They also use `std::io::Write` and `std::process::{Command, Stdio}` locally within function bodies — these stay as-is.

- [ ] **Step 2: Register the module**

In `src/input/actions/mod.rs`, add:

```rust
pub mod word_copy;
```

- [ ] **Step 3: Update dispatch arms in `keymap.rs`**

Change:

```rust
        WordCycleCopy => { navigation::word_cycle_copy(&mut state.borrow_mut()); true }
        WordCollectCopy => { navigation::word_collect_copy(&mut state.borrow_mut()); true }
```

to (after Task 3, `true` is removed):

```rust
        WordCycleCopy => { crate::input::actions::word_copy::word_cycle_copy(&mut state.borrow_mut()); }
        WordCollectCopy => { crate::input::actions::word_copy::word_collect_copy(&mut state.borrow_mut()); }
```

- [ ] **Step 4: Remove unused imports from `navigation.rs`**

After removing the word-copy section, check if `use crate::log_fmt;` is still needed in navigation.rs. If no other function in navigation.rs uses it, remove the import. Also check `use gtk4::prelude::*;` — the remaining navigation functions likely still need it for TextBuffer/TextIter operations.

- [ ] **Step 5: Build and verify**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 7: Run clippy**

Run: `cargo clippy`
Expected: no new warnings

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/mod.rs src/input/actions/word_copy.rs src/input/navigation.rs src/input/keymap.rs
git commit -m "Move word-copy functions from navigation.rs to actions/word_copy.rs"
```
