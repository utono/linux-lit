# Clean dispatch_action (F7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move all inline side-effects from `dispatch_action` match arms into verb functions so every arm is a single function call — matching bk's `on_key` dispatch pattern.

**Architecture:** Four dispatch arms in `keymap.rs:dispatch_action` contain logic beyond calling a verb: `JumpToNextChapter`/`JumpToPrevChapter` (translation toggle), `JumpToNextScene`/`JumpToPrevScene` (work_type branch), and `SetStartTime` (conditional cursor advance). Each arm's logic moves into the verb it calls, then the dispatch arm reduces to one call. The scene verbs (`JumpToNextScene`/`JumpToPrevScene`) additionally fold into a unified function that handles the play-vs-prose routing internally.

**Tech Stack:** Rust, GTK4, sourceview5

---

### Task 1: Absorb translation toggle into chapter jump verbs

The `JumpToNextChapter` and `JumpToPrevChapter` arms in `dispatch_action` both check `state.translations_visible` and call `toggle_translations` before the jump. This pre-condition belongs in the verb.

**Files:**
- Modify: `src/input/navigation.rs:750-791` (`jump_to_next_chapter`, `jump_to_prev_chapter`)
- Modify: `src/input/keymap.rs:779-794` (`dispatch_action` arms)

- [ ] **Step 1: Add translation dismiss to `jump_to_next_chapter`**

In `src/input/navigation.rs`, add the translation toggle at the top of `jump_to_next_chapter`, before the target search:

```rust
/// Next chapter line.
pub fn jump_to_next_chapter(state: &mut AppState) {
    if state.translations_visible {
        crate::app::toggle_translations(state);
    }
    let line_count = state.effective_line_count();
```

The rest of the function body stays unchanged.

- [ ] **Step 2: Add translation dismiss to `jump_to_prev_chapter`**

In `src/input/navigation.rs`, add the same translation toggle at the top of `jump_to_prev_chapter`:

```rust
/// Previous chapter line (`[` key).
pub fn jump_to_prev_chapter(state: &mut AppState) {
    if state.translations_visible {
        crate::app::toggle_translations(state);
    }
    let target = {
```

The rest of the function body stays unchanged.

- [ ] **Step 3: Simplify dispatch arms to single calls**

In `src/input/keymap.rs`, replace the multi-line `JumpToNextChapter` and `JumpToPrevChapter` arms:

```rust
        JumpToNextChapter => { navigation::jump_to_next_chapter(&mut state.borrow_mut()); true }
        JumpToPrevChapter => { navigation::jump_to_prev_chapter(&mut state.borrow_mut()); true }
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/navigation.rs
git commit -m "Move translation toggle into chapter jump verbs

dispatch_action arms for JumpToNextChapter and JumpToPrevChapter
previously checked translations_visible and called toggle_translations
before the verb. That logic now lives inside the verb functions,
making each dispatch arm a single call."
```

---

### Task 2: Absorb work_type routing into scene jump verbs

The `JumpToNextScene` and `JumpToPrevScene` arms in `dispatch_action` check `work_type == "play"` and branch between scene vs chapter jump. This routing belongs in the verb. Rather than adding the branch to both scene verbs (duplicating the chapter fallback), create a single `jump_to_next_section` / `jump_to_prev_section` pair that encapsulates the routing.

**Files:**
- Modify: `src/input/navigation.rs:793-879` (add section-jump wrappers)
- Modify: `src/input/keymap.rs:795-814` (`dispatch_action` arms)

- [ ] **Step 1: Add `jump_to_next_section` wrapper**

In `src/input/navigation.rs`, add after `jump_to_next_scene` (after line 879):

```rust
/// Jump to the next structural section: scene marker for plays, chapter
/// for prose. Encapsulates the work_type routing so the dispatch table
/// stays clean.
pub fn jump_to_next_section(state: &mut AppState) {
    let is_play = state.current_work.as_ref()
        .map(|w| w.work_type == "play")
        .unwrap_or(false);
    if is_play {
        jump_to_next_scene(state);
    } else {
        jump_to_next_chapter(state);
    }
}
```

- [ ] **Step 2: Add `jump_to_prev_section` wrapper**

In `src/input/navigation.rs`, add immediately after `jump_to_next_section`:

```rust
/// Jump to the previous structural section: scene marker for plays,
/// chapter for prose.
pub fn jump_to_prev_section(state: &mut AppState) {
    let is_play = state.current_work.as_ref()
        .map(|w| w.work_type == "play")
        .unwrap_or(false);
    if is_play {
        jump_to_prev_scene(state);
    } else {
        jump_to_prev_chapter(state);
    }
}
```

- [ ] **Step 3: Simplify dispatch arms to single calls**

In `src/input/keymap.rs`, replace the multi-line `JumpToNextScene` and `JumpToPrevScene` arms:

```rust
        JumpToNextScene => { navigation::jump_to_next_section(&mut state.borrow_mut()); true }
        JumpToPrevScene => { navigation::jump_to_prev_section(&mut state.borrow_mut()); true }
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/navigation.rs
git commit -m "Move work_type routing into section jump verbs

JumpToNextScene/JumpToPrevScene dispatch arms previously checked
work_type == play and branched between scene and chapter jumps.
New jump_to_next_section/jump_to_prev_section wrappers encapsulate
this routing, reducing each dispatch arm to a single call."
```

---

### Task 3: Absorb cursor advance into `set_start_time`

The `SetStartTime` arm in `dispatch_action` calls `set_start_time`, then conditionally calls `cursor_next_dialogue` on success. The cursor advance is part of the timestamp-setting workflow and belongs in the verb.

**Files:**
- Modify: `src/input/timestamps.rs:64-141` (`set_start_time`)
- Modify: `src/input/keymap.rs:1046-1052` (`dispatch_action` arm)

- [ ] **Step 1: Add cursor advance to `set_start_time`**

In `src/input/timestamps.rs`, at the end of `set_start_time`, after the gutter renderer queue_draw block (after line 138), add the cursor advance before the final `true`:

```rust
    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }

    crate::input::navigation::cursor_next_dialogue(state);

    true
}
```

- [ ] **Step 2: Simplify dispatch arm to single call**

In `src/input/keymap.rs`, replace the `SetStartTime` arm:

```rust
        SetStartTime => { crate::input::timestamps::set_start_time(&mut state.borrow_mut()); true }
```

Note: the old arm returned `ok` (the bool from `set_start_time`) to signal whether the key was consumed. The verb now always advances the cursor on success internally, and we always return `true` from the dispatch arm (the key was consumed regardless of whether the timestamp write succeeded — the user pressed the key, we handled it).

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs src/input/timestamps.rs
git commit -m "Move cursor advance into set_start_time verb

The SetStartTime dispatch arm previously called set_start_time,
then conditionally called cursor_next_dialogue on success. The
cursor advance is part of the timestamp workflow and now lives
inside the verb. Dispatch arm is a single call."
```

---

### Task 4: Verify all dispatch arms are single calls

After Tasks 1-3, audit the full `dispatch_action` to confirm no remaining multi-statement arms contain action-level logic (as opposed to simple borrow/drop patterns needed by Rc<RefCell>).

**Files:**
- Read: `src/input/keymap.rs:758-1133` (`dispatch_action`)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy`
Expected: no new warnings in `keymap.rs` or `navigation.rs` or `timestamps.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 3: Visual audit of dispatch_action**

Scan `dispatch_action` and confirm every arm follows one of two patterns:

1. `ActionName => { verb(&mut state.borrow_mut()); true }` — single verb call
2. `ActionName => { /* Rc<RefCell> borrow dance */ verb(…); true }` — borrow management only, no domain logic

The remaining multi-line arms (e.g., `OpenLibraryPicker`, `OpenSettingsOverlay`, `OpenKeybindsOverlay`) contain borrow juggling and widget visibility checks, **not** action-level side-effects. These are candidates for extraction into `actions::pickers` or `actions::settings` verbs in a future cleanup, but are not in scope for F7.

- [ ] **Step 4: Commit verification note (optional)**

No code change needed. If all checks pass, the F7 refactor is complete.
