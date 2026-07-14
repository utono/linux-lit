# Dialogue Formatting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add dialogue indentation and tight spacing to text-file mode when speaker lines are detected — dialogue lines indented 60px, zero gap between dialogue lines, larger gap before speaker names.

**Architecture:** Three new GtkTextTags (`dialogue-indent`, `speaker-gap`, `stage-direction-gap`) applied per-line after buffer population. A `dialogue_formatting_active` flag on AppState controls whether the settings overlay's line_spacing adjusts global spacing or the speaker-gap tag.

**Tech Stack:** Rust, GTK4, sourceview5

**Spec:** `docs/superpowers/specs/2026-03-26-dialogue-formatting-design.md`

---

### Task 1: Add `dialogue_formatting_active` flag to AppState

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the field to AppState**

In `src/app.rs`, add to the `AppState` struct (after `settings_overlay` field on line 58):

```rust
    pub dialogue_formatting_active: bool,
```

- [ ] **Step 2: Initialize the field in build_window**

In the `AppState` initialization block (around line 217), add after `settings_overlay,`:

```rust
        dialogue_formatting_active: false,
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: add dialogue_formatting_active flag to AppState"
```

---

### Task 2: Create `apply_dialogue_formatting` function

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the function**

In `src/app.rs`, add this function after `rebuild_buffer_text` (after line 515):

```rust
/// Apply dialogue indentation and tight spacing for text-file mode.
/// Scans buffer lines for speaker patterns. If speakers found:
/// - Sets global line spacing to 0
/// - Applies "dialogue-indent" tag (extra left margin) to dialogue lines
/// - Applies "speaker-gap" tag (extra pixels above) to speaker lines
/// - Applies "stage-direction-gap" tag to stage directions
fn apply_dialogue_formatting(state: &mut AppState) {
    use crate::db::line_types;

    // Only in text-file mode
    if state.line_map.is_none() {
        state.dialogue_formatting_active = false;
        return;
    }

    // Scan first 200 lines for any speaker
    let line_count = state.buffer.line_count() as usize;
    let scan_limit = line_count.min(200);
    let mut has_speakers = false;
    for i in 0..scan_limit {
        let iter = state.buffer.iter_at_line(i as i32).unwrap();
        let end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap()
        } else {
            state.buffer.end_iter()
        };
        let text = state.buffer.text(&iter, &end, false);
        let text = text.trim_end_matches('\n');
        if line_types::is_speaker(text) {
            has_speakers = true;
            break;
        }
    }

    if !has_speakers {
        state.dialogue_formatting_active = false;
        return;
    }

    state.dialogue_formatting_active = true;

    // Set global spacing to 0
    state.text_view.set_pixels_above_lines(0);
    state.text_view.set_pixels_below_lines(0);

    // Remove old formatting tags if they exist
    let tag_table = state.buffer.tag_table();
    for name in &["dialogue-indent", "speaker-gap", "stage-direction-gap"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    // Create tags
    let base_margin = state.text_view.left_margin();
    let speaker_gap = state.config.line_spacing.max(1) as i32;

    let indent_tag = gtk4::TextTag::builder()
        .name("dialogue-indent")
        .left_margin(base_margin + 60)
        .build();

    let speaker_gap_tag = gtk4::TextTag::builder()
        .name("speaker-gap")
        .pixels_above_lines(speaker_gap * 5)
        .build();

    let stage_gap_tag = gtk4::TextTag::builder()
        .name("stage-direction-gap")
        .pixels_above_lines(10)
        .build();

    tag_table.add(&indent_tag);
    tag_table.add(&speaker_gap_tag);
    tag_table.add(&stage_gap_tag);

    // Apply tags per line
    for i in 0..line_count {
        let line_start = match state.buffer.iter_at_line(i as i32) {
            Some(iter) => iter,
            None => continue,
        };
        let line_end = if i + 1 < line_count {
            match state.buffer.iter_at_line((i + 1) as i32) {
                Some(iter) => iter,
                None => state.buffer.end_iter(),
            }
        } else {
            state.buffer.end_iter()
        };

        let text = state.buffer.text(&line_start, &line_end, false);
        let text = text.trim_end_matches('\n');

        if line_types::is_blank(text) {
            continue;
        } else if line_types::is_speaker(text) {
            state.buffer.apply_tag(&speaker_gap_tag, &line_start, &line_end);
        } else if line_types::is_stage_direction(text) {
            state.buffer.apply_tag(&stage_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
        } else if line_types::is_act_scene_marker(text) || line_types::is_separator(text) {
            // Flush left, no extra spacing
            continue;
        } else {
            // Dialogue line — indent
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
        }
    }

    crate::logging::log(&format!(
        "FORMATTING: applied dialogue formatting ({} lines)",
        line_count
    ));
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: compiles (warning about unused function OK)

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: add apply_dialogue_formatting function"
```

---

### Task 3: Call `apply_dialogue_formatting` from `display_work`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Reset flag and call formatting after buffer rebuild**

In `src/app.rs`, in `display_work`, find this section (around line 404-406):

```rust
    // Build buffer text (with or without sign column)
    state.line_map = None;
    rebuild_buffer_text(state);
```

Change it to:

```rust
    // Build buffer text (with or without sign column)
    state.line_map = None;
    state.dialogue_formatting_active = false;
    rebuild_buffer_text(state);
    apply_dialogue_formatting(state);
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: call apply_dialogue_formatting after buffer rebuild"
```

---

### Task 4: Update settings overlay to use speaker-gap when formatting is active

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Update `apply_settings_change` for LineSpacing**

In `src/input/keymap.rs`, find the `apply_settings_change` function (around line 509). Replace the `LineSpacing` arm:

```rust
        SettingsChange::LineSpacing(val) => {
            s.text_view.set_pixels_above_lines(val as i32);
            s.text_view.set_pixels_below_lines(val as i32);
            s.config.line_spacing = val;
        }
```

With:

```rust
        SettingsChange::LineSpacing(val) => {
            if s.dialogue_formatting_active {
                // Adjust speaker-gap tag instead of global spacing
                let tag_table = s.buffer.tag_table();
                if let Some(tag) = tag_table.lookup("speaker-gap") {
                    tag.set_property("pixels-above-lines", val.max(1) as i32 * 5);
                }
            } else {
                s.text_view.set_pixels_above_lines(val as i32);
                s.text_view.set_pixels_below_lines(val as i32);
            }
            s.config.line_spacing = val;
        }
```

- [ ] **Step 2: Update the Escape revert logic for LineSpacing**

In the same file, find the Escape handler for settings overlay (around line 123-144). The revert code sets global spacing unconditionally. Update the spacing revert section:

Replace:

```rust
                    s.text_view.set_pixels_above_lines(snap_ls as i32);
                    s.text_view.set_pixels_below_lines(snap_ls as i32);
```

With:

```rust
                    if s.dialogue_formatting_active {
                        let tag_table = s.buffer.tag_table();
                        if let Some(tag) = tag_table.lookup("speaker-gap") {
                            tag.set_property("pixels-above-lines", snap_ls.max(1) as i32 * 5);
                        }
                    } else {
                        s.text_view.set_pixels_above_lines(snap_ls as i32);
                        s.text_view.set_pixels_below_lines(snap_ls as i32);
                    }
```

- [ ] **Step 3: Update the reset-to-defaults "r" key handler**

Find the "r" key handler in the settings overlay block. Update the spacing reset:

Replace:

```rust
                let mut s = state.borrow_mut();
                let ls = crate::config::DEFAULT_LINE_SPACING;
                let cw = crate::config::DEFAULT_COLUMN_WIDTH;
                let tm = crate::config::DEFAULT_TEXT_MARGINS;
                s.text_view.set_pixels_above_lines(ls as i32);
                s.text_view.set_pixels_below_lines(ls as i32);
```

With:

```rust
                let mut s = state.borrow_mut();
                let ls = crate::config::DEFAULT_LINE_SPACING;
                let cw = crate::config::DEFAULT_COLUMN_WIDTH;
                let tm = crate::config::DEFAULT_TEXT_MARGINS;
                if s.dialogue_formatting_active {
                    let tag_table = s.buffer.tag_table();
                    if let Some(tag) = tag_table.lookup("speaker-gap") {
                        tag.set_property("pixels-above-lines", ls.max(1) as i32 * 5);
                    }
                } else {
                    s.text_view.set_pixels_above_lines(ls as i32);
                    s.text_view.set_pixels_below_lines(ls as i32);
                }
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: settings overlay uses speaker-gap when dialogue formatting active"
```

---

### Task 5: Polish — clippy, tests, final build

**Files:**
- Possibly: `src/app.rs`, `src/input/keymap.rs`

- [ ] **Step 1: Build**

Run: `cargo build`
Expected: clean build

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no new warnings from our changes

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit any fixes**

If clippy or tests required changes:

```bash
git add -u
git commit -m "fix: address clippy warnings from dialogue formatting"
```
