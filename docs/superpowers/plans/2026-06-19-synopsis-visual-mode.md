# Synopsis Visual Mode (Shift+V) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a vim-style visual mode to the synopsis overlay: `Shift+V` enters block-selection mode anchored at the current paragraph, `j`/`k`/`gg`/`G` extend the selection over paragraph blocks, `y` yanks the selected paragraphs' text to the system clipboard and exits, `Escape`/`Shift+V` exits without copying. The selection is shown by extending the overlay's existing left-edge cursor bar to span all selected blocks.

**Architecture:** The synopsis overlay (`src/ui/gloss_overlay.rs`) already navigates a paragraph-block cursor (`cursor_block: Cell<usize>` over `self.blocks: Vec<BlockRange>`) and draws a left bar for the cursor block via `mark_cursor_block` → `bar_ranges`. Visual mode adds an anchor (`synopsis_visual_anchor: Cell<Option<usize>>`); when set, the bar spans `anchor..=cursor` instead of one block. A new `InputMode::SynopsisVisual` routes keys to a new `handle_synopsis_visual_key`. Yank reuses `synopsis_blocks(synopsis)` (the same cursor-stop block list) and joins each selected block's `.display` text. The reader's existing line-based visual mode (`src/input/visual.rs`, `AppState.visual_selection`) is NOT reused — it operates on a different widget.

**Tech Stack:** Rust, GTK4 (`gtk4::TextView`, `Cell`/`RefCell`), Wayland clipboard via `wl-copy` (`std::process::Command`).

---

## Background the engineer needs

- **The synopsis overlay's text/cursor model.** In `src/ui/gloss_overlay.rs`:
  - `self.blocks: Rc<RefCell<Vec<BlockRange>>>` — the cursor-stop paragraph blocks. `struct BlockRange { kind: BlockKind, index: i32, start_line: i32, end_line: i32 }` (buffer line range of each paragraph). Label paragraphs (e.g. "Shakespearean parallels:") are already excluded from this list.
  - `self.cursor_block: Cell<usize>` — index into `self.blocks` (the moving cursor).
  - `mark_cursor_block(&self)` (~line 1352) — sets `*self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }]` for the cursor block and queues a redraw. `struct BarRange { start_line: i32, end_line: i32 }` (line 6).
  - `step_cursor(&self, delta)`, `cursor_to_end(&self, last: bool)` — move `cursor_block` (clamped), then `mark_cursor_block()` + `scroll_cursor_into_view()`. Public wrappers: `cursor_next_block`, `cursor_prev_block`, `cursor_first_block`, `cursor_last_block`.
  - `pub fn synopsis_blocks(synopsis: &str) -> Vec<GlossBlock>` (~line 1771) — parses `<p>` paragraphs into `GlossBlock { kind, index, text, display }`; `display` is the clean paragraph text (synopses carry no `/IPA/`, so `text == display`). Same cursor-stop list the bar uses (label paragraphs skipped).
  - The synopsis text currently displayed is stored — the overlay's `show_synopsis(label, synopsis, ...)` is the entry point. The plan stores the synopsis string so visual mode can re-extract it (see Task 3).

- **Key routing.** `src/input/keymap.rs`: the top-level `match input_mode` (~line 116) dispatches `SynopsisOverlay => handle_synopsis_overlay_key(...)`. `handle_synopsis_overlay_key` (~line 977) matches GTK `key_name` directly. On the user's RPD layout, Shift+v arrives as `key_name == "V"`, plain v as `"v"`. `"V"` is currently UNBOUND in the synopsis handler. The reader's visual handler `handle_visual_key` (~line 1605) is the pattern to mirror for the new `handle_synopsis_visual_key` (keys: `j`,`k`,`G`,`g`(chord),`y`,`Escape`|`V`, everything else consumed).

- **gg chord.** `KeyState::start_chord(key_state, ChordState::PendingG)` arms it; the synopsis handler already checks `key_state.borrow().chord == ChordState::PendingG` near its top to complete `gg`. Mirror that in the visual handler.

- **Clipboard.** Pattern from `copy_gloss_id` (`src/input/actions/gloss.rs:184`): `std::process::Command::new("wl-copy").arg(text).spawn()`. Failure is non-fatal/logged.

- **Toast.** The overlay uses `state.chapter_toast` (a `Label`): `set_text`, `set_visible(true)`, then `glib::timeout_add_local_once(Duration::from_secs(2), move || toast.set_visible(false))`. See `undo_amend` in `src/input/actions/synopsis.rs` for the exact idiom.

- **Project rules:** Do NOT run `cargo run` (user runs the app). Use `cargo build` and `cargo test --bins`. Branch off `master` first (the executor handles branching). No `keymap.json` change (synopsis handlers match key_name directly). The Ctrl+/ overlay and footer hints MUST be updated (Tasks 6–7).

## File structure

- **`src/app.rs`** — add `InputMode::SynopsisVisual` variant. (Enum only.)
- **`src/ui/gloss_overlay.rs`** — `synopsis_visual_anchor` field + init; the pure `visual_block_range` fn + tests; `selected_synopsis_text` + tests; store the current synopsis string; bar-span path; `enter_synopsis_visual`/`exit_synopsis_visual`/`extend_*` methods; footer-hint strings. (Most of the feature.)
- **`src/input/keymap.rs`** — `SynopsisVisual` dispatch arm; `handle_synopsis_visual_key`; `"V"` enter arm in `handle_synopsis_overlay_key`. (Routing.)
- **`src/ui/keybinds_overlay.rs`** — Ctrl+/ documentation. (Docs.)

---

## Task 1: Add the pure `visual_block_range` helper + tests

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add the free fn near `synopsis_blocks`, ~line 1771; add a test module near the other `#[cfg(test)]` modules at the end of the file)

- [ ] **Step 1: Write the failing test.** Append this test module at the END of `src/ui/gloss_overlay.rs` (after the last existing `#[cfg(test)] mod ...`):

```rust
#[cfg(test)]
mod visual_range_tests {
    use super::*;

    #[test]
    fn range_is_direction_independent() {
        assert_eq!(visual_block_range(2, 5), (2, 5));
        assert_eq!(visual_block_range(5, 2), (2, 5));
    }

    #[test]
    fn single_block_range() {
        assert_eq!(visual_block_range(3, 3), (3, 3));
    }

    #[test]
    fn range_from_zero() {
        assert_eq!(visual_block_range(0, 4), (0, 4));
        assert_eq!(visual_block_range(4, 0), (0, 4));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails.**

Run: `cargo test --bins visual_range_tests 2>&1 | tail -15`
Expected: compile error — `cannot find function 'visual_block_range'`.

- [ ] **Step 3: Write the implementation.** Add this free function immediately above `pub fn synopsis_blocks` (~line 1771) in `src/ui/gloss_overlay.rs`:

```rust
/// Inclusive block range for a visual selection given the anchor and cursor
/// block indices. Direction-independent: the smaller index is the start.
pub fn visual_block_range(anchor: usize, cursor: usize) -> (usize, usize) {
    (anchor.min(cursor), anchor.max(cursor))
}
```

- [ ] **Step 4: Run the test to verify it passes.**

Run: `cargo test --bins visual_range_tests 2>&1 | tail -8`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): pure visual_block_range helper

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Add `selected_synopsis_text` extraction (pure) + tests

This is the yank text builder, written as a free function over a synopsis string so it is unit-testable without GTK.

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add the free fn next to `visual_block_range`; extend the test module from Task 1)

- [ ] **Step 1: Write the failing test.** Add these tests inside the `visual_range_tests` module from Task 1 (before its closing `}`):

```rust
    #[test]
    fn selects_paragraph_range_blank_line_joined() {
        let syn = "<p>One.</p><p>Two.</p><p>Three.</p>";
        // blocks 0..=1 -> first two paragraphs
        assert_eq!(selected_blocks_text(syn, 0, 1), "One.\n\nTwo.");
    }

    #[test]
    fn selects_single_paragraph() {
        let syn = "<p>One.</p><p>Two.</p>";
        assert_eq!(selected_blocks_text(syn, 1, 1), "Two.");
    }

    #[test]
    fn selection_skips_label_paragraph_like_the_cursor() {
        // synopsis_blocks excludes label paragraphs, so block indices count only
        // cursor-stop paragraphs. "Shakespearean parallels:" is a label.
        let syn = "<p>Plot.</p><p>Shakespearean parallels:</p><p>The parallel.</p>";
        // cursor-stop blocks are [Plot., The parallel.] -> indices 0,1
        assert_eq!(selected_blocks_text(syn, 0, 1), "Plot.\n\nThe parallel.");
    }

    #[test]
    fn plain_untagged_synopsis_is_one_block() {
        let syn = "Just one paragraph, no tags.";
        assert_eq!(selected_blocks_text(syn, 0, 0), "Just one paragraph, no tags.");
    }
```

- [ ] **Step 2: Run the test to verify it fails.**

Run: `cargo test --bins visual_range_tests 2>&1 | tail -15`
Expected: compile error — `cannot find function 'selected_blocks_text'`.

- [ ] **Step 3: Write the implementation.** Add this free function immediately below `visual_block_range` in `src/ui/gloss_overlay.rs`:

```rust
/// Build the yank text for a synopsis visual selection: the `display` (clean,
/// `<p>`-stripped) text of cursor-stop blocks `start..=end`, joined by a blank
/// line. Uses `synopsis_blocks` so the indices match the on-screen cursor
/// stops exactly (label paragraphs already excluded). Out-of-range indices are
/// clamped; an empty synopsis yields an empty string.
pub fn selected_blocks_text(synopsis: &str, start: usize, end: usize) -> String {
    let blocks = synopsis_blocks(synopsis);
    if blocks.is_empty() {
        return String::new();
    }
    let last = blocks.len() - 1;
    let (s, e) = (start.min(last), end.min(last));
    let (s, e) = (s.min(e), s.max(e));
    blocks[s..=e]
        .iter()
        .map(|b| b.display.clone())
        .collect::<Vec<_>>()
        .join("\n\n")
}
```

- [ ] **Step 4: Run the test to verify it passes.**

Run: `cargo test --bins visual_range_tests 2>&1 | tail -8`
Expected: `test result: ok. 7 passed`.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): selected_blocks_text yank-text builder

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Add overlay state — anchor + stored synopsis string

Visual mode needs (a) the anchor cell and (b) the current synopsis text to feed `selected_blocks_text`. The overlay does not currently retain the synopsis string after `show_synopsis`, so store it.

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (struct fields near `cursor_block: Cell<usize>`, ~line 76; the constructor initializer near `cursor_block: Cell::new(0)`, ~line 445; and `show_synopsis` to record the synopsis string)

- [ ] **Step 1: Add the fields.** In the `GlossOverlay` struct, immediately after the `cursor_block: Cell<usize>,` field (~line 76), add:

```rust
    /// `Some(block_index)` while synopsis visual mode is active — the anchor end
    /// of the selection. The cursor end is `cursor_block`. `None` in normal
    /// synopsis navigation. Selected range: `visual_block_range(anchor, cursor)`.
    synopsis_visual_anchor: Cell<Option<usize>>,
    /// The synopsis string currently shown (raw, `<p>`-tagged), retained so
    /// visual-mode yank can rebuild the selected paragraphs via
    /// `selected_blocks_text`. Set by `show_synopsis`.
    current_synopsis: RefCell<String>,
```

- [ ] **Step 2: Initialize the fields.** In the constructor's struct literal, immediately after `cursor_block: Cell::new(0),` (~line 445), add:

```rust
            synopsis_visual_anchor: Cell::new(None),
            current_synopsis: RefCell::new(String::new()),
```

- [ ] **Step 3: Record the synopsis in `show_synopsis`.** Find the `pub fn show_synopsis(` method (it sets the footer hint at line ~978 and calls `rebuild_block_ranges_from(synopsis_blocks(synopsis))` at ~973). Add, right after the function's opening (use the actual parameter name for the synopsis text — it is the `&str` passed as the synopsis body; confirm by reading the signature):

```rust
        *self.current_synopsis.borrow_mut() = synopsis.to_string();
```

Place this near the top of `show_synopsis`, before the early geometry work, so it is always recorded when the card is shown.

- [ ] **Step 4: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished`. New fields may warn "never read" until Task 4 — expected.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): overlay state for visual anchor + current synopsis

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Bar span + enter/exit/extend methods on `GlossOverlay`

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add methods in the same `impl GlossOverlay` block as `mark_cursor_block`/`step_cursor`, ~line 1233–1352)

- [ ] **Step 1: Add a span-aware bar refresh.** Add this method next to `mark_cursor_block` (~line 1352). It draws the bar across the selected block range when an anchor is set, else falls back to the single-block `mark_cursor_block`:

```rust
    /// Redraw the left bar. In visual mode (anchor set) the bar spans every
    /// selected block (`anchor..=cursor`); otherwise it marks the single cursor
    /// block. Safe to call in both modes.
    fn refresh_selection_bar(&self) {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => {
                self.mark_cursor_block();
                return;
            }
        };
        let blocks = self.blocks.borrow();
        if blocks.is_empty() {
            return;
        }
        let last = blocks.len() - 1;
        let cursor = self.cursor_block.get().min(last);
        let (s, e) = visual_block_range(anchor.min(last), cursor);
        let start_line = blocks[s].start_line;
        let end_line = blocks[e].end_line;
        *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
        self.bar_drawing.queue_draw();
    }
```

- [ ] **Step 2: Add enter/exit and extend methods.** Add these public methods in the same `impl` block (e.g. right after `cursor_last_block`, ~line 1245):

```rust
    /// Enter synopsis visual mode: anchor at the current block. No-op if there
    /// are no blocks. Returns true if mode was entered.
    pub fn enter_visual(&self) -> bool {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return false;
        }
        let cur = self.cursor_block.get().min(len - 1);
        self.synopsis_visual_anchor.set(Some(cur));
        self.refresh_selection_bar();
        true
    }

    /// Exit synopsis visual mode: clear the anchor and redraw the bar as the
    /// single cursor block.
    pub fn exit_visual(&self) {
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Move the cursor end of the selection by `delta` blocks (clamped) and
    /// re-span the bar. Used by j/k while in visual mode.
    pub fn visual_step(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
        self.refresh_selection_bar();
        self.scroll_cursor_into_view();
    }

    /// Move the cursor end of the selection to the first (`false`) or last
    /// (`true`) block and re-span the bar. Used by gg/G while in visual mode.
    pub fn visual_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.refresh_selection_bar();
        self.scroll_cursor_into_view();
    }

    /// The currently-selected paragraphs' text (blank-line joined), for yank.
    pub fn visual_selection_text(&self) -> String {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => return String::new(),
        };
        let cursor = self.cursor_block.get();
        let syn = self.current_synopsis.borrow();
        selected_blocks_text(&syn, anchor, cursor)
    }

    /// Number of blocks currently selected (for the log line).
    pub fn visual_selection_len(&self) -> usize {
        match self.synopsis_visual_anchor.get() {
            Some(a) => {
                let (s, e) = visual_block_range(a, self.cursor_block.get());
                e - s + 1
            }
            None => 0,
        }
    }
```

- [ ] **Step 3: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished`. These methods may warn "never used" until Task 5 — expected.

- [ ] **Step 4: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): bar span + enter/exit/extend visual methods

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Add `InputMode::SynopsisVisual` + key routing

**Files:**
- Modify: `src/app.rs` (the `InputMode` enum, ~line 49–57)
- Modify: `src/input/keymap.rs` (top-level dispatch ~line 116; new `handle_synopsis_visual_key`; `"V"` arm in `handle_synopsis_overlay_key`)

- [ ] **Step 1: Add the enum variant.** In `src/app.rs`, in `pub enum InputMode`, immediately after `SynopsisOverlay,` (~line 57), add:

```rust
    SynopsisVisual,
```

- [ ] **Step 2: Add the dispatch arm.** In `src/input/keymap.rs`, in the top-level `match` (the line with `SynopsisOverlay => handle_synopsis_overlay_key(...)`, ~line 116), add immediately after it:

```rust
            crate::app::InputMode::SynopsisVisual => handle_synopsis_visual_key(state, key_state, key_name),
```

- [ ] **Step 3: Add the `"V"` enter arm to the synopsis handler.** In `handle_synopsis_overlay_key`, find the `"E"` arm (added previously):

```rust
        "E" => {
            crate::input::actions::synopsis::show_edit_prompt(state);
            true
        }
```

Add immediately after it:

```rust
        "V" => {
            let entered = state.borrow().gloss_overlay.enter_visual();
            if entered {
                let mut s = state.borrow_mut();
                s.input_mode = crate::app::InputMode::SynopsisVisual;
                s.gloss_overlay.set_synopsis_visual_hint();
            }
            true
        }
```

- [ ] **Step 4: Add the visual-mode handler.** Add this new function immediately after `handle_synopsis_overlay_key` ends (after its closing `}`):

```rust
/// Key handling for synopsis visual mode (Shift+V from the synopsis overlay).
/// Mirrors the reader's `handle_visual_key`: j/k extend the block selection,
/// gg/G jump the cursor end, y yanks the selected paragraphs and exits, Esc/V
/// exits without copying. All other keys are consumed.
fn handle_synopsis_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    // gg: extend to the first block.
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.visual_to_end(false);
        }
        return true;
    }

    match key_name {
        "j" => {
            state.borrow().gloss_overlay.visual_step(1);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.visual_step(-1);
            true
        }
        "G" => {
            state.borrow().gloss_overlay.visual_to_end(true);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "y" => {
            let (text, n) = {
                let s = state.borrow();
                (s.gloss_overlay.visual_selection_text(), s.gloss_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("SYNOPSIS: copied {} blocks", n));
            }
            // Exit visual mode, back to the synopsis overlay.
            {
                let mut s = state.borrow_mut();
                s.gloss_overlay.exit_visual();
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
                s.gloss_overlay.set_synopsis_hint();
                // "Copied" toast (2s), matching undo_amend's toast idiom.
                s.chapter_toast.set_text("Copied");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    toast.set_visible(false);
                });
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            s.gloss_overlay.exit_visual();
            s.input_mode = crate::app::InputMode::SynopsisOverlay;
            s.gloss_overlay.set_synopsis_hint();
            true
        }
        _ => true,
    }
}
```

- [ ] **Step 5: Confirm imports.** `handle_synopsis_visual_key` uses `Rc`, `RefCell`, `AppState`, `KeyState`, `ChordState`, `glib`. These are already used throughout `keymap.rs` (the file has `handle_visual_key` and `handle_synopsis_overlay_key` using the same). Confirm with `cargo build`; no new `use` is expected. If a `glib` path error appears, match how other handlers in this file reference it (search `glib::timeout_add_local_once` in `keymap.rs`).

- [ ] **Step 6: Build.** Note Task 5 references `set_synopsis_visual_hint()` and `set_synopsis_hint()`, which Task 6 adds. To keep this task compiling on its own, add the two hint methods now as part of Task 6's scope OR add temporary stubs. To avoid stubs, **do Task 6 before building**: implement Task 6 Step 1–2 (the two hint methods) now, then build here.

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` once Task 6's hint methods exist.

- [ ] **Step 7: Commit (after Task 6's hint methods exist).**

```bash
git add src/app.rs src/input/keymap.rs
git commit -m "feat(synopsis): SynopsisVisual mode + Shift+V/j/k/gg/G/y/Esc routing

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

> **Ordering note for the executor:** Task 5 and Task 6 are interdependent (Task 5's handler calls Task 6's hint methods). Implement Task 6 Steps 1–2 (the two hint methods on `GlossOverlay`) before building/committing Task 5, then commit Task 5 and Task 6 in that order. Each commit's tree must build.

---

## Task 6: Footer hints (overlay + visual mode)

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (refactor the synopsis footer string at line ~978 into a method; add a visual-mode hint method)

- [ ] **Step 1: Add the normal synopsis hint method.** In the `impl GlossOverlay` block, add:

```rust
    /// Set the synopsis-overlay footer hint (normal navigation).
    pub fn set_synopsis_hint(&self) {
        self.hint.set_text("Esc close · j/k block · Space play · n/p scene · Shift+Space synth · Ctrl+g glosses · A ask · E edit · U undo · \u{21e7}V select");
    }
```

- [ ] **Step 2: Add the visual-mode hint method.** Add immediately after it:

```rust
    /// Set the footer hint shown while synopsis visual mode is active.
    pub fn set_synopsis_visual_hint(&self) {
        self.hint.set_text("\u{21e7}V/Esc exit · j/k extend · gg/G ends · y yank");
    }
```

- [ ] **Step 3: Use the method in `show_synopsis`.** Replace the existing inline footer line in `show_synopsis` (line ~978):

```rust
        self.hint.set_text("Esc close · j/k block · Space play · n/p scene · Shift+Space synth · Ctrl+g glosses · A ask · E edit · U undo");
```

with:

```rust
        self.set_synopsis_hint();
```

(This both adds `\u{21e7}V select` to the normal footer and DRYs the string into the method the handlers call.)

- [ ] **Step 4: Build.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished`, no errors. (With Task 5's handler in place, the "never used" warnings for the visual methods are now gone.)

- [ ] **Step 5: Run the full pure-logic suite.**

Run: `cargo test --bins 2>&1 | tail -5`
Expected: `test result: ok` (the 7 new visual tests plus the existing suite; total = prior 380 + 7 = 387 passed).

- [ ] **Step 6: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): footer hints for visual mode (⇧V select / yank)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Document Shift+V / yank in the Ctrl+/ keybinds overlay

The Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`) is a hand-maintained Cairo mirror with no compile-time enforcement. Per CLAUDE.md, any keybind change updates both the keycap strip and the per-key `describe()` detail panel.

**Files:**
- Modify: `src/ui/keybinds_overlay.rs`

- [ ] **Step 1: Invoke the skill.** Use the `update-cairo-keybinds-overlay` skill. Add the synopsis-context bindings:
  - On the `v`/`V` key def, add a context entry `("V", "synopsis select")` (the `V` cap already shows reader/gloss `V` = cycle active voice; this adds the synopsis-overlay meaning).
  - On the `y` key def, add `("y", "synopsis yank")` for the visual-mode yank.
  - Add `describe()` arms:
    - `"synopsis select"` => "While the synopsis overlay is open (h), Shift+V enters visual mode: j/k (and gg/G) extend a paragraph-block selection; y yanks it, Esc or Shift+V exits. -> handle_synopsis_visual_key / gloss_overlay::enter_visual — src/input/keymap.rs, src/ui/gloss_overlay.rs"
    - `"synopsis yank"` => "In synopsis visual mode, copy the selected paragraphs (blank-line joined) to the clipboard via wl-copy, then exit visual mode. -> handle_synopsis_visual_key (y arm) — src/input/keymap.rs"
  Follow the skill's three-pass check (no blank slot hides a real binding; no label names the wrong action; every label has a `describe()` arm).

- [ ] **Step 2: Build.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished`, no errors.

- [ ] **Step 3: Commit.**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): document synopsis Shift+V select / y yank in Ctrl+/

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Verify and finish the branch

- [ ] **Step 1: Full build + pure-logic tests.**

Run: `cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -5`
Expected: `Finished`; `test result: ok` (387 passed).

- [ ] **Step 2: Ask the user to verify on screen.** The bar-span rendering, clipboard, and toast are visual/side-effecting; the agent cannot reliably launch cage in the live dwl session. Request user verification with these steps:
  1. `cargo run`, open a play, navigate to a scene with a multi-paragraph synopsis, press `h`.
  2. Confirm the footer shows `… U undo · ⇧V select`.
  3. Press `Shift+V`; confirm the footer switches to `⇧V/Esc exit · j/k extend · gg/G ends · y yank` and the left bar marks the current paragraph.
  4. Press `j`/`k`; confirm the bar grows/shrinks to span the selected paragraphs. Try `G` (extend to last) and `gg` (extend to first).
  5. Press `y`; confirm a "Copied" toast, return to normal synopsis navigation (bar back to one block), and paste elsewhere (e.g. `wl-paste`) to confirm the selected paragraphs were copied, blank-line separated.
  6. Re-enter with `Shift+V`, press `Escape`; confirm it exits without changing the clipboard.

- [ ] **Step 3: Finish the branch per project rule.** Once the user confirms (and only then), follow `~/CLAUDE.md` "Finishing a Branch": verify tree clean, `git checkout master`, `git merge --no-ff <branch>`, re-verify build/tests, `git push origin master`, delete the feature branch.

---

## Testing notes

- **Pure unit tests** (Tasks 1–2): `visual_block_range` (range math) and `selected_blocks_text` (paragraph extraction + blank-line join, including the label-skip and untagged-fallback cases). These run in `cargo test --bins`.
- **Visual / side-effecting** (bar span, `wl-copy`, toast, mode transitions): user-verified on screen (Task 8 Step 2). No GTK measurement is unit-testable here.

## Self-review

- **Spec coverage:** anchor state + stored synopsis (Task 3); `InputMode::SynopsisVisual` + routing + `handle_synopsis_visual_key` (Task 5); Shift+V enter (Task 5); j/k/gg/G extend (Tasks 4–5); y yank → wl-copy → toast → exit (Tasks 4–5); Esc/Shift+V exit (Task 5); bar span via `refresh_selection_bar` (Task 4); pure `visual_block_range` + `selected_blocks_text` with tests (Tasks 1–2); footer hints both modes (Task 6); Ctrl+/ overlay (Task 7); no keymap.json change (stated in Background). All spec sections covered.
- **Placeholder scan:** none — every code step has complete code; Task 7's skill step names exact label + describe() text.
- **Type/name consistency:** `synopsis_visual_anchor: Cell<Option<usize>>`, `current_synopsis: RefCell<String>`, `visual_block_range`, `selected_blocks_text`, `refresh_selection_bar`, `enter_visual`/`exit_visual`/`visual_step`/`visual_to_end`/`visual_selection_text`/`visual_selection_len`, `set_synopsis_hint`/`set_synopsis_visual_hint`, `handle_synopsis_visual_key`, `InputMode::SynopsisVisual` are used identically across tasks. `BlockRange.start_line/end_line` and `BarRange.start_line/end_line` are `i32` (matches existing code). The yank handler reads both `visual_selection_text` and `visual_selection_len` under one borrow.
- **Interdependency:** Task 5 ↔ Task 6 ordering called out explicitly (hint methods before Task 5 build/commit).
