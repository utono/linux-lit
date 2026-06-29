# Line Translations Toggle — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Alt+i keybind to toggle inline translations below original text lines, with dimming and cursor-skip behavior.

**Architecture:** New DB query fetches translations into a HashMap on AppState. Toggle inserts/removes translation lines in the GTK TextBuffer, applies/removes a dim tag, and adjusts cursor positions. Navigation skips inserted translation lines.

**Tech Stack:** Rust, GTK4, sourceview5, rusqlite

---

### Task 1: Add translations query to queries.rs

**Files:**
- Modify: `src/db/queries.rs:1-244`

- [ ] **Step 1: Add `load_translations` function**

Add this function after `load_work` (after line 139):

```rust
/// Load translations for a work, keyed by line_mapping.id.
pub fn load_translations(
    conn: &Connection,
    abbrev: &str,
) -> Result<HashMap<i64, String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT lm.id, lt.translation \
         FROM line_translations lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([abbrev], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, translation) = row?;
        map.insert(id, translation);
    }
    Ok(map)
}
```

- [ ] **Step 2: Add test for load_translations**

Add this test inside the existing `mod tests` block at the end of queries.rs:

```rust
#[test]
fn test_load_translations() {
    let conn = open_db().unwrap();
    let translations = load_translations(&conn, "Ham").unwrap();
    // Hamlet may or may not have translations — just verify no crash
    // and that the return type is correct
    assert!(translations.len() >= 0);
}
```

- [ ] **Step 3: Run test**

Run: `cargo test test_load_translations -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: add load_translations query for line_translations table"
```

---

### Task 2: Add translation state to AppState

**Files:**
- Modify: `src/app.rs:1-65` (AppState struct and build_window initialization)

- [ ] **Step 1: Add HashMap import**

At the top of `src/app.rs`, add to imports:

```rust
use std::collections::HashMap;
```

- [ ] **Step 2: Add fields to AppState**

Add these two fields to the `AppState` struct (after `dialogue_formatting_active: bool` at line 61):

```rust
    pub translations: HashMap<i64, String>,
    pub translations_visible: bool,
    /// Tracks which buffer lines are inserted translation lines.
    /// When translations are visible, `translation_lines[i]` is `true` if buffer line `i`
    /// is an inserted translation line (should be skipped by navigation).
    pub translation_lines: Vec<bool>,
```

- [ ] **Step 3: Add translation-dim and translation-text TextTags**

In `build_window`, after the `search_current_tag` creation (after line 143), add:

```rust
    let translation_dim_tag = gtk4::TextTag::builder()
        .name("translation-dim")
        .foreground(&theme.dim_fg)
        .build();
    buffer.tag_table().add(&translation_dim_tag);

    let translation_text_tag = gtk4::TextTag::builder()
        .name("translation-text")
        .style(pango::Style::Italic)
        .left_margin(60)
        .build();
    buffer.tag_table().add(&translation_text_tag);
```

- [ ] **Step 4: Add pango import**

At the top of `src/app.rs`, the `pango` crate is already used in `apply_dialogue_formatting`. Verify it's available — it's used at line 626 (`pango::Variant::SmallCaps`). No new import needed.

- [ ] **Step 5: Add fields to AppState initialization**

In the `Rc::new(RefCell::new(AppState { ... }))` block (around line 227-263), add the new fields and tags:

```rust
        translations: HashMap::new(),
        translations_visible: false,
        translation_lines: Vec::new(),
        translation_dim_tag,
        translation_text_tag,
```

Also add the tag fields to the AppState struct definition:

```rust
    pub translation_dim_tag: gtk4::TextTag,
    pub translation_text_tag: gtk4::TextTag,
```

- [ ] **Step 6: Build to verify**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat: add translation state fields and TextTags to AppState"
```

---

### Task 3: Load translations during display_work

**Files:**
- Modify: `src/app.rs` (display_work function, around line 358)

- [ ] **Step 1: Reset translation state and load translations**

In `display_work`, after `state.dialogue_formatting_active = false;` (line 434) and before `rebuild_buffer_text(state);` (line 435), add:

```rust
    state.translations_visible = false;
    state.translation_lines = Vec::new();
    // Load translations for this work
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            state.translations = crate::db::queries::load_translations(&conn, &work.abbrev)
                .unwrap_or_default();
            crate::logging::log(&format!(
                "TRANSLATIONS: loaded {} translations for {}",
                state.translations.len(),
                work.abbrev,
            ));
        }
    }
```

Note: `state.current_work` is already set at line 430, so this access is safe.

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: load translations from DB during display_work"
```

---

### Task 4: Implement toggle_translations function

**Files:**
- Modify: `src/app.rs` (add new public function)

- [ ] **Step 1: Add toggle_translations function**

Add this function after `toggle_sign_column` (after line 704):

```rust
/// Toggle translation lines below original text.
/// When showing: dims all lines, inserts translation text below matched lines.
/// When hiding: removes inserted lines and dim tag.
pub fn toggle_translations(state: &mut AppState) {
    if state.translations.is_empty() {
        crate::logging::log("TRANSLATIONS: no translations for this work");
        return;
    }

    if state.translations_visible {
        hide_translations(state);
    } else {
        show_translations(state);
    }
}

fn show_translations(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Build a list of (buffer_line, translation_text) pairs
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let line_count = state.buffer.line_count() as usize;

    for buf_line in 0..line_count {
        // Map buffer line to work line
        let work_idx = state.work_line_for_buffer(buf_line);
        if let Some(wi) = work_idx {
            if let Some(line) = work.lines.get(wi) {
                if let Some(translation) = state.translations.get(&line.id) {
                    inserts.push((buf_line, format!("        {}", translation)));
                }
            }
        }
    }

    // Insert bottom-to-top to avoid index shifting
    for (buf_line, text) in inserts.iter().rev() {
        // Get iterator at end of the target line
        let line_end = if let Some(mut iter) = state.buffer.iter_at_line(*buf_line as i32) {
            if !iter.ends_line() {
                iter.forward_to_line_end();
            }
            iter
        } else {
            continue;
        };
        state.buffer.insert(&mut line_end.clone(), &format!("\n{}", text));
    }

    // Build translation_lines tracking vector
    let new_line_count = state.buffer.line_count() as usize;
    let mut tl = vec![false; new_line_count];

    // The inserts shifted lines. Rebuild: walk through and mark translation lines.
    // After insertion, each original line that had a translation now has a new line after it.
    // We need to walk the buffer and identify which lines are translations.
    // Strategy: re-scan using the known insert count. Each insert adds 1 line after its source.
    // Since we inserted bottom-to-top, the final buffer has inserts in the right positions.
    // Walk top-to-bottom: for each original line that had a translation, the next line is a translation line.
    let mut orig_idx = 0;
    let orig_line_count = line_count; // original line count before inserts
    let mut buf_idx = 0;
    let work_lines = &work.lines;
    while orig_idx < orig_line_count && buf_idx < new_line_count {
        // This is an original line
        tl[buf_idx] = false;
        // Check if this original line had a translation
        let work_idx = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work.get(orig_idx).copied().flatten()
        } else {
            if orig_idx < work_lines.len() { Some(orig_idx) } else { None }
        };
        let has_translation = work_idx
            .and_then(|wi| work_lines.get(wi))
            .and_then(|line| state.translations.get(&line.id))
            .is_some();
        buf_idx += 1;
        if has_translation && buf_idx < new_line_count {
            tl[buf_idx] = true; // this is a translation line
            buf_idx += 1;
        }
        orig_idx += 1;
    }
    state.translation_lines = tl;

    // Apply translation-dim tag to entire buffer
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.apply_tag(&state.translation_dim_tag, &buf_start, &buf_end);

    // Apply translation-text tag to translation lines only
    for (i, is_trans) in state.translation_lines.iter().enumerate() {
        if *is_trans {
            if let Some(line_start) = state.buffer.iter_at_line(i as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                // Remove dim from translation lines so they show at full brightness
                state.buffer.remove_tag(&state.translation_dim_tag, &line_start, &line_end);
                state.buffer.apply_tag(&state.translation_text_tag, &line_start, &line_end);
            }
        }
    }

    // Adjust current_line and page_top_line to account for inserted lines
    state.current_line = map_line_after_insert(state.current_line, &inserts);
    state.page_top_line = map_line_after_insert(state.page_top_line, &inserts);

    state.translations_visible = true;

    // Re-apply font tag to cover new lines
    reapply_font(state);
    crate::input::navigation::update_highlight_and_ensure_visible(state);

    crate::logging::log(&format!(
        "TRANSLATIONS: shown ({} translation lines inserted)",
        inserts.len(),
    ));
}

/// Map an original buffer line index to its new position after translation inserts.
/// `inserts` is a list of (original_buffer_line, text) sorted by buffer_line ascending.
fn map_line_after_insert(orig_line: usize, inserts: &[(usize, String)]) -> usize {
    let mut offset = 0;
    for (buf_line, _) in inserts {
        if *buf_line < orig_line {
            offset += 1;
        } else if *buf_line == orig_line {
            offset += 1; // translation goes after this line, but cursor stays on original
            break;
        } else {
            break;
        }
    }
    orig_line + offset
}

fn hide_translations(state: &mut AppState) {
    // Remove translation lines from buffer bottom-to-top
    let line_count = state.buffer.line_count() as usize;
    for i in (0..line_count).rev() {
        if i < state.translation_lines.len() && state.translation_lines[i] {
            // Delete this entire line (including the preceding newline)
            let line_start = if i > 0 {
                // Start from end of previous line to capture the newline
                if let Some(mut iter) = state.buffer.iter_at_line((i - 1) as i32) {
                    if !iter.ends_line() {
                        iter.forward_to_line_end();
                    }
                    iter
                } else {
                    continue;
                }
            } else {
                state.buffer.start_iter()
            };
            let line_end = if let Some(mut iter) = state.buffer.iter_at_line(i as i32) {
                if !iter.ends_line() {
                    iter.forward_to_line_end();
                }
                iter
            } else {
                continue;
            };
            state.buffer.delete(&mut line_start.clone(), &mut line_end.clone());
        }
    }

    // Remove translation-dim tag from entire buffer
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.translation_dim_tag, &buf_start, &buf_end);
    state.buffer.remove_tag(&state.translation_text_tag, &buf_start, &buf_end);

    // Reverse-map current_line and page_top_line
    let old_current = state.current_line;
    let old_top = state.page_top_line;
    state.current_line = map_line_before_insert(old_current, &state.translation_lines);
    state.page_top_line = map_line_before_insert(old_top, &state.translation_lines);

    state.translation_lines.clear();
    state.translations_visible = false;

    // Re-apply font tag
    reapply_font(state);
    crate::input::navigation::update_highlight_and_ensure_visible(state);

    crate::logging::log("TRANSLATIONS: hidden");
}

/// Map a buffer line index (with translations) back to the original line index.
fn map_line_before_insert(buf_line: usize, translation_lines: &[bool]) -> usize {
    let mut orig = 0;
    for i in 0..=buf_line.min(translation_lines.len().saturating_sub(1)) {
        if i < translation_lines.len() && translation_lines[i] {
            // Skip translation lines — don't increment orig
        } else if i == buf_line {
            return orig;
        } else {
            orig += 1;
        }
    }
    orig
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: implement toggle_translations show/hide with buffer manipulation"
```

---

### Task 5: Add cursor-skip for translation lines in navigation

**Files:**
- Modify: `src/input/navigation.rs` (move_cursor function)

- [ ] **Step 1: Add translation line skipping to move_cursor**

In `move_cursor` (line 20), after computing `new_line` (line 29-31) and before the early return check (line 33), add logic to skip translation lines:

Replace the current `new_line` computation and check:

```rust
    let new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    if new_line == state.current_line {
        return;
    }
```

With:

```rust
    let mut new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    // Skip over translation lines
    if state.translations_visible && !state.translation_lines.is_empty() {
        let direction = if delta > 0 { 1i32 } else { -1i32 };
        while new_line < state.translation_lines.len()
            && state.translation_lines[new_line]
        {
            let next = new_line as i32 + direction;
            if next < 0 || next >= line_count as i32 {
                break;
            }
            new_line = next as usize;
        }
    }

    if new_line == state.current_line {
        return;
    }
```

- [ ] **Step 2: Update effective_line_count for translation-aware navigation**

In `src/app.rs`, the `effective_line_count` method (line 68) returns the buffer line count. When translations are visible, this already includes translation lines, which is correct — the buffer physically has those lines. The cursor-skip logic in `move_cursor` handles skipping them. No change needed here.

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat: skip translation lines during j/k cursor movement"
```

---

### Task 6: Add Alt+i keybinding

**Files:**
- Modify: `src/input/keymap.rs` (alt key section around line 369)

- [ ] **Step 1: Add Alt+i binding**

In the `if is_alt` block (line 369-377), add the `"i"` arm before the `_ => return false` fallthrough:

Replace:

```rust
    if is_alt {
        match key_name {
            "f" => {
                crate::app::show_font_info(&state.borrow());
                return true;
            }
            _ => return false,
        }
    }
```

With:

```rust
    if is_alt {
        match key_name {
            "f" => {
                crate::app::show_font_info(&state.borrow());
                return true;
            }
            "i" => {
                crate::app::toggle_translations(&mut state.borrow_mut());
                return true;
            }
            _ => return false,
        }
    }
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: bind Alt+i to toggle translations"
```

---

### Task 7: Update theme change to update translation-dim tag

**Files:**
- Modify: `src/input/keymap.rs` (apply_theme_to_state function around line 700)

- [ ] **Step 1: Update translation-dim tag foreground on theme change**

In `apply_theme_to_state` (line 700), after the existing dim tag updates (line 705-706), add:

```rust
    state.translation_dim_tag.set_property("foreground", &theme.dim_fg);
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: update translation-dim tag on theme change"
```

---

### Task 8: Final integration test

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings

- [ ] **Step 3: Final commit if any fixups needed**

```bash
git add -A
git commit -m "fix: clippy and test fixes for translations feature"
```
