# Spacebar Play-From-Cursor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `Space` replicate each surface's `a` bind — play from the cursor line's start (or the surface's notion of "play") — instead of being a global play/pause toggle, while leaving the gloss overlay's space (read-block) unchanged.

**Architecture:** The global space handler in `handle_key` (which today sends `TogglePause` for every mode) is narrowed to act only in Reader mode, where it calls `play_current_line` (the `a` path) with a no-timestamp toast fallback. Each overlay that has an `a`/play bind gets its own `"space"` arm mirroring that bind. The Ctrl+/ overlay's Space label is retargeted to the existing "play from ts" description.

**Tech Stack:** Rust, GTK4 (`gtk4`/`glib`), MPV IPC via `MpvCommand`.

---

## File Structure

- `src/input/keymap.rs` — owns the global space block, the per-overlay key handlers, and `dispatch_action`. All behavioral changes live here:
  - the global space block (currently `keymap.rs:64-81`),
  - `handle_translation_overlay_key`,
  - `handle_echoes_overlay_key`,
  - a new small toast helper (private fn).
- `src/ui/keybinds_overlay.rs` — the hand-maintained Ctrl+/ overlay mirror. One `KeyDef` label change for `Space`; the `describe()` arm it now points to already exists.

No new files. No `keymap.json` change (space is not in the keymap lookup table).

---

## Task 1: Add a no-timestamp toast helper

A 3-second bottom-center toast reusing the existing `chapter_toast` +
`glib::timeout_add_local_once` pattern already repeated in `keymap.rs` (e.g.
lines 1757-1762). Used when `play_current_line` returns `false`.

**Files:**
- Modify: `src/input/keymap.rs` (add a private fn near the other helpers, e.g. just below `toggle_playback_sync`, around line 920)

- [ ] **Step 1: Add the helper function**

Add this private function to `src/input/keymap.rs` (place it immediately after
the closing brace of `fn toggle_playback_sync`, around line 920):

```rust
/// Show a transient bottom-center toast (reuses `chapter_toast`, 3s auto-hide).
/// Used when a play attempt is a no-op because the line has no timestamp.
fn show_no_timestamp_toast(s: &AppState) {
    s.chapter_toast.set_text("No timestamp on this line");
    s.chapter_toast.set_visible(true);
    let toast = s.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds with a `dead_code` warning for `show_no_timestamp_toast`
(it is wired up in Tasks 2 and 3). The warning is expected at this step only.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(keymap): add no-timestamp toast helper for space play"
```

---

## Task 2: Retarget the global space block to Reader play-from-cursor

The block at `src/input/keymap.rs:64-81` currently sends `TogglePause` for any
non-editable, non-gloss mode. Change it so it only acts in Reader mode and calls
`play_current_line` (the `a` path) with the toast fallback. For other
non-editable modes it must NOT return — control falls through to mode dispatch
so each overlay's own `"space"` arm (Tasks 3) can run.

**Files:**
- Modify: `src/input/keymap.rs:64-81`

- [ ] **Step 1: Replace the global space block**

Find this exact block (currently lines 64-81):

```rust
    if key_name == "space" && !is_ctrl && !is_shift && !is_alt {
        let s = state.borrow();
        let gloss_open = s.input_mode == crate::app::InputMode::GlossOverlay;
        // The search bar (opened by /) is a text-input field; space must
        // type a literal space there, never toggle playback. Treat Search
        // mode as editable explicitly rather than relying on window focus.
        let focus_is_editable = s.input_mode == crate::app::InputMode::Search
            || gtk4::prelude::GtkWindowExt::focus(&s.window).is_some_and(|w| {
                w.is::<gtk4::Entry>()
                    || w.downcast_ref::<gtk4::TextView>()
                        .is_some_and(|tv| tv.is_editable())
            });
        drop(s);
        if !focus_is_editable && !gloss_open {
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
            return true;
        }
    }
```

Replace it with:

```rust
    // Spacebar (no modifiers) replicates the `a` bind on each surface:
    // begin playback from the cursor line's start time. This block handles the
    // Reader (main card) case; overlays handle space in their own arms so each
    // surface's space matches that surface's `a`. Guards stay here because they
    // gate Search and Gloss before their handlers run:
    //  - editable widget focus (Entry / editable TextView / Search) → space
    //    must type a literal space, so let GTK route it (return false);
    //  - GlossOverlay → its handler owns space (read-block), so skip here.
    // For any other non-editable mode, fall through to mode dispatch.
    if key_name == "space" && !is_ctrl && !is_shift && !is_alt {
        let s = state.borrow();
        let mode = s.input_mode;
        let gloss_open = mode == crate::app::InputMode::GlossOverlay;
        let focus_is_editable = mode == crate::app::InputMode::Search
            || gtk4::prelude::GtkWindowExt::focus(&s.window).is_some_and(|w| {
                w.is::<gtk4::Entry>()
                    || w.downcast_ref::<gtk4::TextView>()
                        .is_some_and(|tv| tv.is_editable())
            });
        drop(s);
        if focus_is_editable {
            return false; // type a literal space in the text field
        }
        if !gloss_open && mode == crate::app::InputMode::Reader {
            let mut s = state.borrow_mut();
            if !crate::input::timestamps::play_current_line(&mut s) {
                show_no_timestamp_toast(&s);
            }
            return true;
        }
        // Non-editable, non-Reader, non-gloss (e.g. an overlay): fall through
        // to mode dispatch so the overlay's own space arm runs.
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds. `show_no_timestamp_toast` is now used, so its dead_code
warning from Task 1 is gone. `play_current_line`'s import path
(`crate::input::timestamps::play_current_line`) matches its dispatch use at
`keymap.rs:1663`.

- [ ] **Step 3: Confirm Reader-mode behavior in source**

Run: `rg -n "TogglePause" src/input/keymap.rs`
Expected: no matches (the global space toggle is gone). `Tab`'s play/pause via
`search::toggle_playback` is unaffected — it was never `TogglePause`.

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(keymap): space plays from cursor line in Reader (was play/pause)"
```

---

## Task 3: Add space arms to the translation and echoes overlays

Each overlay gets a `"space"` arm identical to its existing `"a"` arm.

**Files:**
- Modify: `src/input/keymap.rs` — `handle_translation_overlay_key` (the `"a"` arm at ~881), `handle_echoes_overlay_key` (the `"a"` arm at ~1189)

- [ ] **Step 1: Add the translation-overlay space arm**

In `handle_translation_overlay_key`, find this arm (around line 881):

```rust
        "a" => {
            crate::input::timestamps::play_current_line(&mut state.borrow_mut());
            true
        }
```

Replace it with (adds `"space"` as an alias and the toast fallback so it
matches the main card exactly):

```rust
        "a" | "space" => {
            let mut s = state.borrow_mut();
            if !crate::input::timestamps::play_current_line(&mut s) {
                show_no_timestamp_toast(&s);
            }
            true
        }
```

- [ ] **Step 2: Add the echoes-overlay space arm**

In `handle_echoes_overlay_key`, find this arm (around line 1189):

```rust
        "a" => {
            crate::input::actions::echoes::play_selected_echo(state, tokio_handle);
            true
        }
```

Replace it with:

```rust
        "a" | "space" => {
            crate::input::actions::echoes::play_selected_echo(state, tokio_handle);
            true
        }
```

(No toast here: `play_selected_echo` has its own feedback and is out of scope
for the no-timestamp toast.)

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Confirm gloss space is untouched**

Run: `rg -n '"space"' src/input/keymap.rs`
Expected: three matches — the global block guard (Task 2), and the gloss
overlay's `read_current_block` arm (~800) unchanged. The translation/echoes
arms use `"a" | "space"` so they show under the `"a"` search instead; confirm:

Run: `rg -n '"a" \| "space"' src/input/keymap.rs`
Expected: two matches (translation overlay, echoes overlay).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(keymap): space mirrors the a bind in translation and echoes overlays"
```

---

## Task 4: Update the Ctrl+/ keybinds overlay

`Space` (line 100) and `Tab` (line 66) both currently use the label
`"play/pause"`. Retarget only `Space` to the existing `"play from ts"` label —
which already has a correct `describe()` arm (line 455, "Seek MPV to the current
line's saved start timestamp and play from there"). `Tab` keeps `"play/pause"`.
The `a` key (line 69) also uses `"play from ts"`, which is now correct since
space mirrors `a`.

**Files:**
- Modify: `src/ui/keybinds_overlay.rs:100`

- [ ] **Step 1: Change the Space KeyDef label**

In `src/ui/keybinds_overlay.rs`, find (line 100):

```rust
    bare("Space", "", "play/pause"),
```

Replace with:

```rust
    bare("Space", "", "play from ts"),
```

- [ ] **Step 2: Verify the describe() arm exists and is correct**

Run: `rg -n '"play from ts" =>' src/ui/keybinds_overlay.rs`
Expected: one match (line ~455): the arm
`"play from ts" => "Seek MPV to the current line's saved start timestamp and
play from there. -> timestamps::play_current_line — src/input/timestamps.rs"`.
No new arm needed — Space now shares `a`'s description, which is accurate.

Also confirm the `MOD_SEQ_ROW` "page ↓/↑" describe arms (lines ~225-227) still
say "(Space)" / "(Shift+Space)" — those refer to the OLD page-turn binding and
are stale text, but they are out of scope for this change (they describe
`page ↓`/`page ↑` labels, not Space). Do not edit them in this task.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Run the overlay cross-reference skill**

Invoke the `update-cairo-keybinds-overlay` skill and run its three-pass
exhaustive cross-reference (no blank slot hides a real binding; no label names
the wrong action; every label has a `describe()` arm). Confirm `Space` →
"play from ts" → real description, and `Tab` → "play/pause" → real description
are both consistent.

- [ ] **Step 5: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): Ctrl+/ Space now documents play-from-cursor"
```

---

## Task 5: Verify the full suite and request runtime check

**Files:** none (verification only)

- [ ] **Step 1: Build**

Run: `cargo build`
Expected: clean build, no warnings introduced by this change.

- [ ] **Step 2: Pure-logic tests**

Run: `cargo test --bins`
Expected: PASS (this change touches GTK key routing only; no pure-logic test
asserts on space, and the keymap unit tests for `a`/`Ctrl+a` at
`keymap_config.rs:399-400` are unaffected — space is not in the keymap table).

- [ ] **Step 3: Request user runtime verification**

The acceptance criterion is "space plays from the right line on screen", which
needs the e2e harness; the agent generally cannot launch cage (the live dwl
owns the seat). State that runtime verification is blocked for the agent and
ask the user to confirm, in a running `cargo run`:

  - press `a`, note the seek; press `Space` on the same line → identical seek;
  - translation overlay: `Space` matches `a`;
  - echoes overlay: `Space` plays the selected echo;
  - gloss overlay: `Space` still reads the block aloud (unchanged);
  - a line with no timestamp: `Space` shows the "No timestamp on this line"
    toast;
  - while a text field is focused (Search `/`): `Space` types a literal space.

- [ ] **Step 4: Commit (if any final touch-ups)**

```bash
git add -A
git commit -m "chore: spacebar play-from-cursor verified"
```

(Only if Step 3 surfaces a fix; otherwise skip.)

---

## Self-Review Notes

- **Spec coverage:** Reader (Task 2), translation overlay + echoes overlay
  (Task 3), gloss unchanged (verified Task 3 Step 4), no-timestamp toast (Tasks
  1-3), keymap.json no-change (noted, no task needed), Ctrl+/ overlay (Task 4).
  All spec sections mapped.
- **Type consistency:** `play_current_line(&mut AppState) -> bool`,
  `show_no_timestamp_toast(&AppState)`, `chapter_toast` field, and
  `echoes::play_selected_echo(state, tokio_handle)` all match their definitions
  in the codebase.
- **No placeholders:** every code step shows the exact replacement.
