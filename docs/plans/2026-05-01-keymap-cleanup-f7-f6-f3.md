# Keymap Cleanup (F7 + F6 + F3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove dead code from PageChangeReason, consolidate chord state management, and clean dispatch_action so every arm is a single verb call.

**Architecture:** Three independent cleanups touching `navigation.rs`, `keymap.rs`, and `actions/`. F7 removes a dead method. F6 replaces three boolean flags with one enum. F3 moves inline logic from dispatch_action arms into action verb modules.

**Tech Stack:** Rust, GTK4 (glib timeouts)

---

### Task 1: Remove dead `should_update_label` method (F7)

**Files:**
- Modify: `src/input/navigation.rs:1208-1212` (remove method)
- Modify: `src/input/navigation.rs:3604-3619` (remove tests)

- [ ] **Step 1: Delete `should_update_label` method**

In `src/input/navigation.rs`, remove these lines (the method and its doc comment):

```rust
    /// Whether to refresh the page label. True for everything except WorkLoad
    /// (display_work handles label setup itself).
    pub(crate) fn should_update_label(self) -> bool {
        !matches!(self, Self::WorkLoad)
    }
```

- [ ] **Step 2: Delete the test for `should_update_label`**

In `src/input/navigation.rs`, remove the entire test function `reason_always_updates_label_except_workload`:

```rust
    #[test]
    fn reason_always_updates_label_except_workload() {
        assert!(PageChangeReason::Forward.should_update_label());
        assert!(PageChangeReason::MpvSync.should_update_label());
        assert!(PageChangeReason::Resnap.should_update_label());
        assert!(!PageChangeReason::WorkLoad.should_update_label());
    }
```

Also remove the `should_update_label()` assertion from `reason_skips_seek_for_cursor_only_navigation`:

```rust
        assert!(PageChangeReason::Cursor.should_update_label());
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`
Expected: Clean build (possibly with warnings), all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Remove dead should_update_label from PageChangeReason"
```

---

### Task 2: Consolidate chord state into an enum (F6)

**Files:**
- Modify: `src/input/keymap.rs:11-16` (KeyState struct)
- Modify: `src/input/keymap.rs:126-149` (Reader mode chord checks)
- Modify: `src/input/keymap.rs:536-549` (Gloss mode chord check)
- Modify: `src/input/keymap.rs:877-911` (Keybinds mode chord check)
- Modify: `src/input/keymap.rs:958-1001` (Visual mode chord check)
- Modify: `src/input/keymap.rs:1328-1343` (PendingG/PendingZ dispatch arms)
- Modify: `src/input/keymap.rs:1102-1124` (OpenKeybindsOverlay dispatch arm)

- [ ] **Step 1: Replace KeyState booleans with a ChordState enum**

Replace the `KeyState` struct in `src/input/keymap.rs`:

```rust
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ChordState {
    #[default]
    None,
    PendingG,
    PendingZ,
    PendingCtrlSlash,
}

#[derive(Default)]
pub struct KeyState {
    pub chord: ChordState,
}
```

- [ ] **Step 2: Add a helper to set chord with timeout**

Add after the `KeyState` struct:

```rust
impl KeyState {
    pub fn start_chord(key_state: &Rc<RefCell<KeyState>>, chord: ChordState) {
        key_state.borrow_mut().chord = chord;
        let ks = Rc::clone(key_state);
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if ks.borrow().chord == chord {
                ks.borrow_mut().chord = ChordState::None;
            }
        });
    }
}
```

- [ ] **Step 3: Update Reader mode chord checks**

Replace the `pending_g` check block (around line 126):

```rust
    // gg sequence check
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
```

Replace the `pending_z` check block (around line 143):

```rust
    // zt sequence check
    if key_state.borrow().chord == ChordState::PendingZ {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "t" {
```

- [ ] **Step 4: Update PendingG and PendingZ dispatch arms**

Replace the `PendingG` arm in `dispatch_action` (around line 1328):

```rust
        PendingG => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        PendingZ => {
            KeyState::start_chord(key_state, ChordState::PendingZ);
            true
        }
```

- [ ] **Step 5: Update OpenKeybindsOverlay dispatch arm**

Replace the `pending_ctrl_slash` setup in the `OpenKeybindsOverlay` arm (around line 1119):

```rust
            KeyState::start_chord(key_state, ChordState::PendingCtrlSlash);
```

- [ ] **Step 6: Update handle_gloss_key chord check**

Replace the `pending_g` check in `handle_gloss_key` (around line 543):

```rust
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
```

And the `g` arm in the gloss match that starts the chord (around line 589):

```rust
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
```

- [ ] **Step 7: Update handle_keybinds_key chord check**

Replace the `pending_ctrl_slash` check (around line 888):

```rust
        "g" if key_state.borrow().chord == ChordState::PendingCtrlSlash => {
            key_state.borrow_mut().chord = ChordState::None;
```

- [ ] **Step 8: Update handle_visual_key chord**

Replace the visual mode `g` arm (around line 977):

```rust
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
```

- [ ] **Step 9: Build and test**

Run: `cargo build && cargo test`
Expected: Clean build, all tests pass. Behavior is identical — same 500ms timeouts, same chord resolution.

- [ ] **Step 10: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Consolidate chord state into ChordState enum, eliminating three boolean flags"
```

---

### Task 3: Clean dispatch_action — OpenLibraryPicker (F3a)

**Files:**
- Modify: `src/input/actions/pickers.rs` (add open_library_picker verb)
- Modify: `src/input/keymap.rs:1042-1060` (simplify dispatch arm)

- [ ] **Step 1: Add `open_library_picker_from_reader` to `actions/pickers.rs`**

Add at the end of `src/input/actions/pickers.rs`:

```rust
pub fn open_library_picker_from_reader(state: &Rc<RefCell<crate::app::AppState>>) {
    let s = state.borrow();
    if s.picker.is_visible()
        || s.bookmark_picker.is_visible()
        || s.media_picker.is_visible()
        || s.settings_overlay.is_visible()
    {
        return;
    }
    drop(s);
    {
        let mut sm = state.borrow_mut();
        sm.concordance_state = None;
        sm.concordance_bar.hide();
    }
    state.borrow().gloss_overlay.hide();
    state.borrow_mut().picker.show_prepare();
    state.borrow().picker.show_finish();
    state.borrow_mut().input_mode = crate::app::InputMode::LibraryPicker;
}
```

- [ ] **Step 2: Replace dispatch arm**

Replace the `OpenLibraryPicker` arm in `dispatch_action`:

```rust
        OpenLibraryPicker => { crate::input::actions::pickers::open_library_picker_from_reader(state); true }
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/pickers.rs src/input/keymap.rs
git commit -m "Move OpenLibraryPicker logic from dispatch_action to actions::pickers verb"
```

---

### Task 4: Clean dispatch_action — OpenSettingsOverlay (F3b)

**Files:**
- Modify: `src/input/actions/settings.rs` (add open_settings verb)
- Modify: `src/input/keymap.rs:1086-1100` (simplify dispatch arm)

- [ ] **Step 1: Add `open_settings` to `actions/settings.rs`**

Add at the end of `src/input/actions/settings.rs`:

```rust
pub fn open_settings(state: &Rc<RefCell<crate::app::AppState>>) {
    let s = state.borrow();
    if s.settings_overlay.is_visible() || s.picker.is_visible() {
        return;
    }
    s.gloss_overlay.hide();
    let ls = s.config.line_spacing;
    let cw = s.config.column_width;
    let tm = s.config.text_margins;
    let nm = s.config.navigation_mode;
    let ts = s.config.transition_style;
    let cl = s.config.show_cursor_line;
    drop(s);
    state.borrow_mut().settings_overlay.show(ls, cw, tm, nm, ts, cl);
    state.borrow_mut().input_mode = crate::app::InputMode::Settings;
}
```

- [ ] **Step 2: Replace dispatch arm**

Replace the `OpenSettingsOverlay` arm in `dispatch_action`:

```rust
        OpenSettingsOverlay => { crate::input::actions::settings::open_settings(state); true }
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/settings.rs src/input/keymap.rs
git commit -m "Move OpenSettingsOverlay logic from dispatch_action to actions::settings verb"
```

---

### Task 5: Clean dispatch_action — OpenKeybindsOverlay (F3c)

**Files:**
- Modify: `src/input/actions/pickers.rs` (add open_keybinds verb)
- Modify: `src/input/keymap.rs:1102-1125` (simplify dispatch arm)

- [ ] **Step 1: Add `open_keybinds_overlay` to `actions/pickers.rs`**

Add at the end of `src/input/actions/pickers.rs`:

```rust
pub fn open_keybinds_overlay(state: &Rc<RefCell<crate::app::AppState>>) {
    let s = state.borrow();
    if s.keybinds_overlay.is_visible() || s.gamepad_overlay.is_visible() {
        s.keybinds_overlay.hide();
        s.gamepad_overlay.hide();
        drop(s);
        state.borrow_mut().input_mode = crate::app::InputMode::Reader;
    } else {
        s.picker.hide();
        s.media_picker.hide();
        s.settings_overlay.hide();
        s.search_bar.hide();
        s.gloss_overlay.hide();
        s.keybinds_overlay.show();
        drop(s);
        state.borrow_mut().input_mode = crate::app::InputMode::KeybindsOverlay;
    }
}
```

- [ ] **Step 2: Replace dispatch arm**

Replace the `OpenKeybindsOverlay` arm. Note the chord-start call stays in `dispatch_action` because it touches `key_state`, which the verb doesn't need to know about:

```rust
        OpenKeybindsOverlay => {
            crate::input::actions::pickers::open_keybinds_overlay(state);
            KeyState::start_chord(key_state, ChordState::PendingCtrlSlash);
            true
        }
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/pickers.rs src/input/keymap.rs
git commit -m "Move OpenKeybindsOverlay logic from dispatch_action to actions::pickers verb"
```

---

### Task 6: Clean dispatch_action — JumpToNextVocab / JumpToPrevVocab (F3d)

**Files:**
- Modify: `src/input/actions/concordance.rs` (add jump_to_next/prev_vocab verbs)
- Modify: `src/input/keymap.rs:1180-1215` (simplify dispatch arms)

- [ ] **Step 1: Add concordance-aware vocab jump verbs to `actions/concordance.rs`**

Add at the end of `src/input/actions/concordance.rs`:

```rust
pub fn jump_to_next_vocab(
    state: &Rc<RefCell<crate::app::AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
        let advanced = {
            let mut s = state.borrow_mut();
            if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                conc.advance_within_work(abbrev)
            } else { false }
        };
        if advanced {
            crate::input::navigation::concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        crate::input::navigation::jump_to_next_vocab(&mut state.borrow_mut());
    }
}

pub fn jump_to_prev_vocab(
    state: &Rc<RefCell<crate::app::AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
        let retreated = {
            let mut s = state.borrow_mut();
            if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                conc.retreat_within_work(abbrev)
            } else { false }
        };
        if retreated {
            crate::input::navigation::concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        crate::input::navigation::jump_to_prev_vocab(&mut state.borrow_mut());
    }
}
```

- [ ] **Step 2: Replace dispatch arms**

Replace both arms in `dispatch_action`:

```rust
        JumpToNextVocab => { crate::input::actions::concordance::jump_to_next_vocab(state, tokio_handle); true }
        JumpToPrevVocab => { crate::input::actions::concordance::jump_to_prev_vocab(state, tokio_handle); true }
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/concordance.rs src/input/keymap.rs
git commit -m "Move JumpToNextVocab/JumpToPrevVocab concordance logic to actions::concordance verbs"
```

---

### Task 7: Clean dispatch_action — remaining multi-line arms (F3e)

**Files:**
- Modify: `src/input/actions/pickers.rs` (add open_concordance_word/list verbs)
- Modify: `src/input/keymap.rs` (simplify remaining multi-line dispatch arms)

- [ ] **Step 1: Add `open_concordance_word_picker` to `actions/pickers.rs`**

```rust
pub fn open_concordance_word_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let words: Vec<(String, usize)> = {
        let s = state.borrow();
        let mut seen = std::collections::BTreeSet::new();
        for m in &s.vocab_matches {
            seen.insert(m.word.clone());
        }
        seen.into_iter().map(|w| (w, 0)).collect()
    };
    state.borrow_mut().concordance_word_picker.set_words(words);
    state.borrow().concordance_word_picker.show();
    state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceWordPicker;
}
```

- [ ] **Step 2: Add `open_concordance_list_picker` to `actions/pickers.rs`**

```rust
pub fn open_concordance_list_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let s = state.borrow();
    if let Some(conc) = &s.concordance_state {
        s.concordance_list_picker.show(&conc.occurrences, conc.current_index);
    }
    drop(s);
    state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceListPicker;
}
```

- [ ] **Step 3: Replace dispatch arms**

```rust
        OpenConcordanceWordPicker => { crate::input::actions::pickers::open_concordance_word_picker(state); true }
        OpenConcordanceListPicker => { crate::input::actions::pickers::open_concordance_list_picker(state); true }
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/pickers.rs src/input/keymap.rs
git commit -m "Move remaining multi-line dispatch arms to action verb modules"
```

---

### Task 8: Remove duplicate Ctrl+p / Ctrl+Shift+P pre-dispatch blocks from handle_key (partial F5)

**Files:**
- Modify: `src/input/keymap.rs:46-91` (remove redundant pre-dispatch blocks)

The `Ctrl+Shift+P` block at lines 46-60 and `Ctrl+p` block at lines 74-91 duplicate logic that already exists in `dispatch_action` via the `OpenConcordanceWordPicker` and `OpenLibraryPicker` Action arms (and their keymap bindings in `keymap_config.rs`). The pre-dispatch blocks fire before mode dispatch, shadowing the keymap system.

- [ ] **Step 1: Remove the Ctrl+Shift+P pre-dispatch block**

Remove lines 46-60 of `keymap.rs` (the `Ctrl+Shift+p` concordance word picker block). The `OpenConcordanceWordPicker` action at `keymap_config.rs:262` already binds `Ctrl+Shift+P` and the verb was just moved to `actions/pickers.rs` in Task 7.

- [ ] **Step 2: Remove the Ctrl+p pre-dispatch block**

Remove lines 74-91 (the `Ctrl+p` library picker block). The `OpenLibraryPicker` action at `keymap_config.rs:309` already binds `Ctrl+p` and the verb was moved to `actions/pickers.rs` in Task 3.

- [ ] **Step 3: Remove the Ctrl+Alt+p pre-dispatch block**

Remove lines 63-72 (the `Ctrl+Alt+p` concordance list picker block). The `OpenConcordanceListPicker` action at `keymap_config.rs:263` already binds `Ctrl+Alt+p` and the verb was moved in Task 7.

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`

Verify that `Ctrl+p` still opens the library picker by checking the keymap default bindings contain the entry.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Remove pre-dispatch Ctrl+p/Ctrl+Shift+P/Ctrl+Alt+p blocks that shadowed keymap dispatch"
```
