# Text File Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display Shakespeare works from raw Folger `.txt` files instead of database-parsed lines, with full timestamp/gutter/MPV sync via a bidirectional line map.

**Architecture:** Add a `text_file` column to `Work`. When present and file exists, read the file into the buffer and build a `LineMap` that maps buffer lines to work-line indices. All navigation, seek, gutter, search, and MPV sync code uses this map (or falls through to existing behavior when absent).

**Tech Stack:** Rust, GTK4, sourceview5, rusqlite

**Spec:** `docs/superpowers/specs/2026-03-26-text-file-display-design.md`

---

### Task 1: Create `text_file_map` module with `LineMap` and `build_line_map`

**Files:**
- Create: `src/text_file_map.rs`
- Modify: `src/main.rs:1` (add `mod text_file_map`)

- [ ] **Step 1: Write the test**

```rust
// In src/text_file_map.rs at bottom:
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{Line, TimeRange};

    fn make_line(id: i64, text: &str, normalized: &str) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: normalized.to_string(),
            speaker: None,
            is_dialogue: true,
            timestamp: None,
            div1: 1,
            div2: 1,
            line_in_div: id,
        }
    }

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("Who's there?"), "whos there");
        assert_eq!(normalize("Long live the King!"), "long live the king");
        assert_eq!(normalize("  He.  "), "he");
        assert_eq!(normalize("BARNARDO"), "barnardo");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn test_build_line_map_basic() {
        let file_lines: Vec<String> = vec![
            "ACT 1".into(),
            "=====".into(),
            "".into(),
            "BARNARDO".into(),
            "Who's there?".into(),
            "".into(),
            "FRANCISCO".into(),
            "Nay, answer me.".into(),
        ];
        let work_lines = vec![
            make_line(1, "Who's there?", "whos there"),
            make_line(2, "Nay, answer me.", "nay answer me"),
        ];

        let map = build_line_map(&file_lines, &work_lines);

        // Buffer line 4 ("Who's there?") -> work line 0
        assert_eq!(map.buffer_to_work[4], Some(0));
        // Buffer line 7 ("Nay, answer me.") -> work line 1
        assert_eq!(map.buffer_to_work[7], Some(1));
        // Unmapped lines
        assert_eq!(map.buffer_to_work[0], None); // "ACT 1"
        assert_eq!(map.buffer_to_work[2], None); // blank
        assert_eq!(map.buffer_to_work[3], None); // "BARNARDO"

        // Reverse: work line 0 -> buffer line 4
        assert_eq!(map.work_to_buffer[0], 4);
        assert_eq!(map.work_to_buffer[1], 7);

        // Dialogue lines precomputed
        assert_eq!(map.dialogue_buffer_lines, vec![4, 7]);
    }

    #[test]
    fn test_build_line_map_confirmation_check() {
        // "He." appears at file line 2 and file line 5.
        // DB has "He." at work index 1 (after "Who's there?" at index 0).
        // Without confirmation, file line 2 could falsely match work index 1.
        // With confirmation, it should skip file line 2 because the next file line
        // doesn't match the next DB row.
        let file_lines: Vec<String> = vec![
            "Who's there?".into(),
            "".into(),
            "He.".into(), // This is a stage direction "He." not the dialogue
            "".into(),
            "Who's there?".into(), // duplicate text
            "He.".into(),          // This is the real dialogue "He."
        ];
        let work_lines = vec![
            make_line(1, "Who's there?", "whos there"),
            make_line(2, "He.", "he"),
        ];

        let map = build_line_map(&file_lines, &work_lines);

        // First "Who's there?" at file line 0 matches work line 0
        assert_eq!(map.buffer_to_work[0], Some(0));
        assert_eq!(map.work_to_buffer[0], 0);
        // "He." at file line 5 matches work line 1 (not line 2)
        assert_eq!(map.buffer_to_work[5], Some(1));
        assert_eq!(map.work_to_buffer[1], 5);
    }
}
```

- [ ] **Step 2: Write the implementation**

```rust
// src/text_file_map.rs

use crate::db::models::Line;

#[derive(Debug, Clone)]
pub struct LineMap {
    /// For each buffer line index, the corresponding index into work.lines (None if unmapped)
    pub buffer_to_work: Vec<Option<usize>>,
    /// For each work.lines index, the corresponding buffer line index
    pub work_to_buffer: Vec<usize>,
    /// Precomputed buffer line indices that map to dialogue lines, sorted ascending
    pub dialogue_buffer_lines: Vec<usize>,
}

/// Normalize text for matching: lowercase, strip non-alphanumeric, collapse whitespace.
/// Matches the normalized_text column in the line_mapping DB table.
pub fn normalize(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_lowercase();
    let stripped: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    // Collapse whitespace
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Build a bidirectional line map between file lines and work lines.
/// Both file lines and work lines are in document order, enabling O(n) matching
/// with a sliding window (ported from lit's citation_map.lua:build_core).
pub fn build_line_map(file_lines: &[String], work_lines: &[Line]) -> LineMap {
    let window = 50;
    let mut buffer_to_work: Vec<Option<usize>> = vec![None; file_lines.len()];
    let mut work_to_buffer: Vec<usize> = vec![0; work_lines.len()];
    let mut dialogue_buffer_lines: Vec<usize> = Vec::new();

    // Pre-normalize all file lines
    let normalized_file: Vec<String> = file_lines.iter().map(|l| normalize(l)).collect();

    let mut db_cursor: usize = 0;
    let mut matched: usize = 0;

    for (lnum, norm) in normalized_file.iter().enumerate() {
        if norm.is_empty() {
            continue;
        }

        let search_end = (db_cursor + window).min(work_lines.len());
        for j in db_cursor..search_end {
            if work_lines[j].normalized == *norm {
                // Confirmation check: when match is beyond cursor position,
                // verify the next non-empty file line matches the next DB row
                let mut skip = false;
                if j > db_cursor && j + 1 < work_lines.len() {
                    // Find next non-empty normalized file line
                    let mut next_norm: Option<&str> = None;
                    for k in (lnum + 1)..file_lines.len().min(lnum + 11) {
                        if !normalized_file[k].is_empty() {
                            next_norm = Some(&normalized_file[k]);
                            break;
                        }
                    }
                    if let Some(next) = next_norm {
                        if work_lines[j + 1].normalized != next {
                            skip = true;
                        }
                    }
                }

                if !skip {
                    buffer_to_work[lnum] = Some(j);
                    work_to_buffer[j] = lnum;
                    if work_lines[j].is_dialogue {
                        dialogue_buffer_lines.push(lnum);
                    }
                    db_cursor = j + 1;
                    matched += 1;
                    break;
                }
            }
        }
    }

    let match_pct = if work_lines.is_empty() {
        100.0
    } else {
        (matched as f64 / work_lines.len() as f64) * 100.0
    };
    crate::logging::log(&format!(
        "LINEMAP: matched {}/{} work lines ({:.1}%)",
        matched,
        work_lines.len(),
        match_pct
    ));
    if match_pct < 80.0 {
        crate::logging::log("LINEMAP: WARNING — less than 80% matched, text file may be stale or wrong");
    }

    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
    }
}
```

- [ ] **Step 3: Add `mod text_file_map` to `src/main.rs`**

Add after line 1 (`mod ab_repeat;`):

```rust
mod text_file_map;
```

- [ ] **Step 4: Run tests**

Run: `cargo test text_file_map`
Expected: all 3 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/text_file_map.rs src/main.rs
git commit -m "feat: add text_file_map module with LineMap and build_line_map"
```

---

### Task 2: Add `text_file` to `Work` model and `load_work` query

**Files:**
- Modify: `src/db/models.rs:3` (add field to `Work`)
- Modify: `src/db/queries.rs:32-36` (add `text_file` to SELECT)

- [ ] **Step 1: Add `text_file` field to `Work` struct**

In `src/db/models.rs`, add after `pub work_type: String,`:

```rust
pub text_file: Option<String>,
```

- [ ] **Step 2: Update `load_work` query to fetch `text_file`**

The `text_file` column may not exist in the DB yet (the migration is manual). Use a separate query with fallback so existing tests keep passing on un-migrated databases.

In `src/db/queries.rs`, leave the existing metadata query unchanged. After line 36 (after the metadata query), add:

```rust
    // text_file column may not exist yet (manual migration) — graceful fallback
    let text_file: Option<String> = conn.query_row(
        "SELECT text_file FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| row.get(0),
    ).unwrap_or(None);
```

And in the `Ok(Work { ... })` block at line 119, add:

```rust
text_file,
```

- [ ] **Step 3: Run build and existing tests**

Run: `cargo build && cargo test`
Expected: compiles and all tests pass. The separate query returns `None` gracefully if the column doesn't exist.

- [ ] **Step 4: Commit**

```bash
git add src/db/models.rs src/db/queries.rs
git commit -m "feat: add text_file field to Work model and load_work query"
```

---

### Task 3: Add `line_map` to `AppState` and `effective_line_count` helper

**Files:**
- Modify: `src/app.rs:24-57` (add field to `AppState`)
- Modify: `src/app.rs` (add helper method)

- [ ] **Step 1: Add `line_map` field to `AppState`**

In `src/app.rs`, add after `pub ab_b_line: Rc<Cell<Option<usize>>>,`:

```rust
pub line_map: Option<crate::text_file_map::LineMap>,
```

- [ ] **Step 2: Add `effective_line_count` method**

Add after the `AppState` struct definition (after the closing `}`):

```rust
impl AppState {
    /// Returns the number of lines in the buffer for navigation bounds.
    /// When a text file is loaded (line_map present), this is the file line count.
    /// Otherwise, it's the work's DB line count.
    pub fn effective_line_count(&self) -> usize {
        if let Some(ref lm) = self.line_map {
            lm.buffer_to_work.len()
        } else {
            self.current_work.as_ref().map_or(0, |w| w.lines.len())
        }
    }

    /// Look up the work-line index for a buffer line. Returns None if unmapped
    /// (blank line, speaker name, header) or if no line_map is active.
    /// When no line_map, buffer line index equals work line index.
    pub fn work_line_for_buffer(&self, buffer_line: usize) -> Option<usize> {
        if let Some(ref lm) = self.line_map {
            lm.buffer_to_work.get(buffer_line).copied().flatten()
        } else {
            let count = self.current_work.as_ref().map_or(0, |w| w.lines.len());
            if buffer_line < count { Some(buffer_line) } else { None }
        }
    }
}
```

- [ ] **Step 3: Initialize `line_map` in `build_window`**

Find the `AppState` construction in `build_window` (search for `AppState {`) and add:

```rust
line_map: None,
```

- [ ] **Step 4: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: add line_map to AppState with effective_line_count helper"
```

---

### Task 4: Update `rebuild_buffer_text` to load text file and build line map

**Files:**
- Modify: `src/app.rs:429-442` (`rebuild_buffer_text` function)

- [ ] **Step 1: Replace `rebuild_buffer_text`**

Replace the existing function at lines 429-442:

```rust
/// Rebuild the buffer text from current_work.
/// If the work has a text_file and it exists, load from file and build a line map.
/// Otherwise, join work.lines as before.
fn rebuild_buffer_text(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    if let Some(ref path) = work.text_file {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let file_lines: Vec<String> = contents.lines().map(String::from).collect();
                let line_map = crate::text_file_map::build_line_map(&file_lines, &work.lines);
                state.buffer.set_text(&contents);
                state.line_map = Some(line_map);
                crate::logging::log(&format!(
                    "TEXT_FILE: loaded {} lines from {}",
                    file_lines.len(),
                    path
                ));
                return;
            }
            Err(e) => {
                crate::logging::log(&format!(
                    "TEXT_FILE: WARNING — failed to read {}: {}",
                    path, e
                ));
                // Fall through to DB-based display
            }
        }
    }

    // Default: join work.lines
    state.line_map = None;
    let text: String = work
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    state.buffer.set_text(&text);
}
```

- [ ] **Step 2: Clear line_map in `display_work` before rebuild**

In `display_work`, just before the `rebuild_buffer_text(state);` call (line 373), add:

```rust
state.line_map = None;
```

- [ ] **Step 3: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: rebuild_buffer_text loads text file when available"
```

---

### Task 5: Update navigation to use `effective_line_count`

**Files:**
- Modify: `src/input/navigation.rs`

All instances of `w.lines.len()` and `work.lines.len()` used for bounds checking must use `state.effective_line_count()` instead.

- [ ] **Step 1: Update `move_cursor` (line 21-24)**

Change:
```rust
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
```
to:
```rust
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
```

- [ ] **Step 2: Update `jump_to_end` (line 62-64)**

Change:
```rust
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
```
to:
```rust
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
```

- [ ] **Step 3: Update `page_forward` (line 78-80)**

Change:
```rust
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
```
to:
```rust
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
```

- [ ] **Step 4: Update `page_backward` (line 109-112)**

Change:
```rust
    let line_count = state
        .current_work
        .as_ref()
        .map_or(0, |w| w.lines.len());
```
to:
```rust
    let line_count = state.effective_line_count();
```

- [ ] **Step 5: Update `needs_page_turn_down` (line 239)**

Change:
```rust
    let line_count = state.current_work.as_ref().map_or(0, |w| w.lines.len());
```
to:
```rust
    let line_count = state.effective_line_count();
```

- [ ] **Step 6: Update `lines_per_page` (line 372)**

Change:
```rust
    let line_count = state.current_work.as_ref().map_or(0, |w| w.lines.len());
```
to:
```rust
    let line_count = state.effective_line_count();
```

- [ ] **Step 7: Update `seek_to_current_line` (line 268-277)**

Change the entire function:
```rust
fn seek_to_current_line(state: &AppState) {
    if let Some(ref work) = state.current_work {
        if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
            if let Some(ts) = &work.lines[work_idx].timestamp {
                let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
                let _ = state
                    .cmd_tx
                    .try_send(crate::mpv::MpvCommand::Seek(seek_time));
            }
        }
    }
}
```

- [ ] **Step 8: Update dialogue jumping — `jump_to_next_dialogue` (line 152-178)**

Replace the entire function:
```rust
/// Next dialogue line (`q` key).
/// Jump to next dialogue line. Page turn when target is not fully visible.
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        if let Some(ref lm) = state.line_map {
            // Text file mode: use precomputed dialogue buffer lines
            lm.dialogue_buffer_lines
                .iter()
                .find(|&&bl| bl > state.current_line)
                .copied()
        } else {
            let line_count = work.lines.len();
            let mut found = None;
            for i in (state.current_line + 1)..line_count {
                if work.lines[i].is_dialogue {
                    found = Some(i);
                    break;
                }
            }
            found
        }
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        if needs_page_turn_down(state, line_idx) {
            set_page(state, line_idx);
        }
        seek_to_current_line(state);
    }
}
```

- [ ] **Step 9: Update dialogue jumping — `jump_to_prev_dialogue` (line 122-148)**

Replace the entire function:
```rust
/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
/// Otherwise, jump to the previous dialogue line.
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if state.current_line == 0 {
            return;
        }

        if let Some(ref lm) = state.line_map {
            // Text file mode: use precomputed dialogue buffer lines
            lm.dialogue_buffer_lines
                .iter()
                .rev()
                .find(|&&bl| bl < state.current_line)
                .copied()
        } else {
            let mut found = None;
            for i in (0..state.current_line).rev() {
                if work.lines[i].is_dialogue {
                    found = Some(i);
                    break;
                }
            }
            found
        }
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        scroll_to_cursor(state);
        seek_to_current_line(state);
    }
}
```

- [ ] **Step 10: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 11: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat: navigation uses effective_line_count and line_map for text file mode"
```

---

### Task 6: Update gutter to support text file line counts

**Files:**
- Modify: `src/app.rs:379-392` (gutter setup in `display_work`)
- Modify: `src/app.rs:407-420` (chunk gutter setup in `display_work`)

- [ ] **Step 1: Update timestamp gutter mark building in `display_work`**

Replace lines 379-384 in `display_work` (the `has_timestamp` building):

```rust
    let has_timestamp: Vec<bool> = if let Some(ref lm) = state.line_map {
        // Text file mode: one entry per buffer line, mapped through line_map
        lm.buffer_to_work
            .iter()
            .map(|opt_idx| {
                opt_idx
                    .and_then(|idx| state.current_work.as_ref()?.lines.get(idx)?.timestamp.as_ref())
                    .is_some()
            })
            .collect()
    } else {
        state
            .current_work
            .as_ref()
            .map(|w| w.lines.iter().map(|l| l.timestamp.is_some()).collect())
            .unwrap_or_default()
    };
```

- [ ] **Step 2: Update chunk gutter setup in `display_work`**

Replace lines 410-419 (chunk gutter setup). The key change: when `line_map` is present, pass `buffer_line_count` and translate chunk positions through the map.

In `src/gutter.rs`, update `build_chunk_positions` signature and `setup_chunk_gutter` to accept an optional `LineMap`:

Change `build_chunk_positions`:
```rust
fn build_chunk_positions(
    chunks: &[crate::db::models::Chunk],
    lines: &[crate::db::models::Line],
    line_map: Option<&crate::text_file_map::LineMap>,
) -> Vec<ChunkPos> {
    let buf_len = line_map.map_or(lines.len(), |lm| lm.buffer_to_work.len());
    let mut positions = vec![ChunkPos::None; buf_len];

    for chunk in chunks {
        let mut a_idx = None;
        let mut b_idx = None;
        for (i, line) in lines.iter().enumerate() {
            if line.div1 == chunk.div1
                && Some(line.div2) == chunk.div2
                && line.line_in_div == chunk.a_line
            {
                a_idx = Some(i);
            }
            if line.div1 == chunk.div1
                && Some(line.div2) == chunk.div2
                && line.line_in_div == chunk.b_line
            {
                b_idx = Some(i);
            }
        }

        // Translate work-line indices to buffer-line indices if line_map present
        if let Some(lm) = line_map {
            a_idx = a_idx.map(|i| lm.work_to_buffer[i]);
            b_idx = b_idx.map(|i| lm.work_to_buffer[i]);
        }

        if let (Some(a), Some(b)) = (a_idx, b_idx) {
            if a == b {
                positions[a] = ChunkPos::Single;
            } else {
                positions[a] = ChunkPos::Start;
                for i in (a + 1)..b {
                    positions[i] = ChunkPos::Interior;
                }
                positions[b] = ChunkPos::End;
            }
        }
    }

    positions
}
```

Update `setup_chunk_gutter` signature:
```rust
pub fn setup_chunk_gutter(
    view: &View,
    visible: Rc<Cell<bool>>,
    chunks: &[crate::db::models::Chunk],
    lines: &[crate::db::models::Line],
    line_map: Option<&crate::text_file_map::LineMap>,
) -> sourceview5::GutterRendererText {
    let positions = build_chunk_positions(chunks, lines, line_map);
    // ... rest unchanged
```

- [ ] **Step 3: Update `setup_chunk_gutter` call in `display_work`**

In `src/app.rs`, update the call at line 412-418:
```rust
            let renderer = crate::gutter::setup_chunk_gutter(
                &state.text_view,
                state.sign_column_visible.clone(),
                &state.ab_repeat.chunks,
                &work.lines,
                state.line_map.as_ref(),
            );
```

- [ ] **Step 4: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/gutter.rs
git commit -m "feat: gutter marks support text file line mapping"
```

---

### Task 7: Update MPV CursorSync to translate through line_map

**Files:**
- Modify: `src/main.rs:58-72` (CursorSync handler)

- [ ] **Step 1: Update CursorSync event handler**

Replace lines 58-72 in `src/main.rs`:

```rust
                    MpvEvent::CursorSync(line_idx) => {
                        let mut s = state_for_events.borrow_mut();
                        // Don't let MPV sync override cursor when search matches are active
                        if !s.search_matches.is_empty() {
                            continue;
                        }
                        // Translate work-line index to buffer-line index if line_map present
                        let buffer_line = if let Some(ref lm) = s.line_map {
                            if line_idx < lm.work_to_buffer.len() {
                                lm.work_to_buffer[line_idx]
                            } else {
                                continue;
                            }
                        } else {
                            line_idx
                        };
                        if s.current_line != buffer_line {
                            s.current_line = buffer_line;
                            crate::input::navigation::update_highlight_and_ensure_visible(
                                &mut s,
                            );
                            // Continuously save position so MRU restores to playback point
                            s.config.last_line = buffer_line;
                            crate::config::save(&s.config);
                        }
                    }
```

- [ ] **Step 2: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: MPV CursorSync translates through line_map"
```

---

### Task 8: Update cursor restore to use effective_line_count

**Files:**
- Modify: `src/app.rs:274-279` (startup cursor clamping)

- [ ] **Step 1: Update cursor clamping in startup**

Replace lines 274-279:
```rust
                        s.current_line = last_line.min(
                            s.current_work
                                .as_ref()
                                .map_or(0, |w| w.lines.len().saturating_sub(1)),
                        );
```
with:
```rust
                        s.current_line = last_line.min(
                            s.effective_line_count().saturating_sub(1),
                        );
```

Note: `display_work` calls `rebuild_buffer_text` which sets `line_map` before this code runs, so `effective_line_count()` will return the correct buffer line count.

- [ ] **Step 2: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: cursor restore uses effective_line_count for text file mode"
```

---

### Task 9: Update search to work in text file mode

**Files:**
- Modify: `src/input/search.rs`

Search currently iterates `work.lines` and stores work-line indices. In text file mode, the buffer contains different text, so search must search the buffer directly.

- [ ] **Step 1: Update `execute_search` to handle text file mode**

Replace lines 23-65 (the search loop) in `execute_search`:

```rust
    if let Some(ref lm) = state.line_map {
        // Text file mode: search the buffer text directly
        let text = state.buffer.text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        let buf_lines: Vec<&str> = text.as_str().lines().collect();

        for (line_idx, line_text) in buf_lines.iter().enumerate() {
            if case_sensitive {
                let mut search_start = 0;
                while let Some(pos) = line_text[search_start..].find(&*query) {
                    let byte_start = search_start + pos;
                    let byte_end = byte_start + query.len();
                    new_matches.push(SearchMatch {
                        line_index: line_idx,
                        byte_start,
                        byte_end,
                    });
                    search_start = byte_end;
                }
            } else {
                let text_lower = line_text.to_lowercase();
                let query_lower = query.to_lowercase();
                let mut search_start = 0;
                while let Some(pos) = text_lower[search_start..].find(&*query_lower) {
                    let byte_start = search_start + pos;
                    let byte_end = byte_start + query_lower.len();
                    new_matches.push(SearchMatch {
                        line_index: line_idx,
                        byte_start,
                        byte_end,
                    });
                    search_start = byte_end;
                }
            }
        }
    } else {
        for (line_idx, line) in work.lines.iter().enumerate() {
            if case_sensitive {
                let mut search_start = 0;
                while let Some(pos) = line.text[search_start..].find(&*query) {
                    let byte_start = search_start + pos;
                    let byte_end = byte_start + query.len();
                    new_matches.push(SearchMatch {
                        line_index: line_idx,
                        byte_start,
                        byte_end,
                    });
                    search_start = byte_end;
                }
            } else {
                let text_lower = line.text.to_lowercase();
                let query_lower = query.to_lowercase();
                let mut search_start = 0;
                while let Some(pos) = text_lower[search_start..].find(&*query_lower) {
                    let byte_start = search_start + pos;
                    let byte_end = byte_start + query_lower.len();
                    new_matches.push(SearchMatch {
                        line_index: line_idx,
                        byte_start,
                        byte_end,
                    });
                    search_start = byte_end;
                }
            }
        }
    }
```

- [ ] **Step 2: Update `apply_highlights` to handle text file mode**

Replace `apply_highlights` (lines 164-183). The issue is that `work.lines[m.line_index].text` won't be valid in text file mode since `line_index` is a buffer line, not a work line:

```rust
fn apply_highlights(state: &AppState) {
    for m in &state.search_matches {
        let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
            continue;
        };
        // Get line text from buffer directly (works in both modes)
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let line_text = state.buffer.text(&line_start, &line_end, false);
        let char_start = line_text[..m.byte_start.min(line_text.len())].chars().count() as i32;
        let char_end = line_text[..m.byte_end.min(line_text.len())].chars().count() as i32;
        let mut match_start = line_start;
        match_start.forward_chars(char_start);
        let mut match_end = line_start;
        match_end.forward_chars(char_end);
        state
            .buffer
            .apply_tag(&state.search_tag, &match_start, &match_end);
    }
}
```

- [ ] **Step 3: Update `apply_current_highlight` and `remove_current_highlight` similarly**

Replace `apply_current_highlight` (lines 185-203):
```rust
fn apply_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() {
        return;
    }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
        return;
    };
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line_text = state.buffer.text(&line_start, &line_end, false);
    let char_start = line_text[..m.byte_start.min(line_text.len())].chars().count() as i32;
    let char_end = line_text[..m.byte_end.min(line_text.len())].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state
        .buffer
        .apply_tag(&state.search_current_tag, &match_start, &match_end);
}
```

Replace `remove_current_highlight` (lines 205-223):
```rust
fn remove_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() {
        return;
    }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
        return;
    };
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line_text = state.buffer.text(&line_start, &line_end, false);
    let char_start = line_text[..m.byte_start.min(line_text.len())].chars().count() as i32;
    let char_end = line_text[..m.byte_end.min(line_text.len())].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state
        .buffer
        .remove_tag(&state.search_current_tag, &match_start, &match_end);
}
```

- [ ] **Step 4: Update `toggle_playback` and `seek_and_resume` to use `work_line_for_buffer`**

Replace `toggle_playback` (lines 93-103):
```rust
pub fn toggle_playback(state: &AppState) {
    if let Some(ref work) = state.current_work {
        if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
            if let Some(ts) = &work.lines[work_idx].timestamp {
                let seek_time = (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0);
                let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
            }
        }
    }
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
}
```

Replace `seek_and_resume` (lines 138-145):
```rust
fn seek_and_resume(state: &AppState) {
    if let Some(ref work) = state.current_work {
        if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
            if let Some(ts) = &work.lines[work_idx].timestamp {
                let seek_time = (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0);
                let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(seek_time));
            }
        }
    }
}
```

- [ ] **Step 5: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 6: Commit**

```bash
git add src/input/search.rs
git commit -m "feat: search works in text file mode by reading buffer directly"
```

---

### Task 10: Update chunk resolution in keymap to translate through line_map

**Files:**
- Modify: `src/input/keymap.rs:299-317,346-363` (chunk line resolution)

- [ ] **Step 1: Update chunk line resolution for next chunk (u key)**

In `src/input/keymap.rs`, the block at lines 299-317 resolves chunk `a_line`/`b_line` from `work.lines`. When `line_map` is present, translate the work-line index to a buffer-line index.

Replace lines 299-317:
```rust
                                if let Some(ref work) = s.current_work {
                                    let mut a_buf = None;
                                    let mut b_buf = None;
                                    for (i, line) in work.lines.iter().enumerate() {
                                        if line.div1 == chunk.div1 && Some(line.div2) == chunk.div2 {
                                            if line.line_in_div == chunk.a_line {
                                                a_buf = Some(i);
                                            }
                                            if line.line_in_div == chunk.b_line {
                                                b_buf = Some(i);
                                            }
                                        }
                                    }
                                    // Translate to buffer indices if line_map present
                                    if let Some(ref lm) = s.line_map {
                                        a_buf = a_buf.map(|i| lm.work_to_buffer[i]);
                                        b_buf = b_buf.map(|i| lm.work_to_buffer[i]);
                                    }
                                    s.ab_repeat.a_line = a_buf;
                                    s.ab_repeat.b_line = b_buf;
                                    s.ab_a_line.set(a_buf);
                                    s.ab_b_line.set(b_buf);
                                }
```

- [ ] **Step 2: Same update for prev chunk (y key)**

Apply the identical change to lines 346-363 (the `y` key handler). Add the same translation block after the for loop:
```rust
                                    // Translate to buffer indices if line_map present
                                    if let Some(ref lm) = s.line_map {
                                        a_buf = a_buf.map(|i| lm.work_to_buffer[i]);
                                        b_buf = b_buf.map(|i| lm.work_to_buffer[i]);
                                    }
```

- [ ] **Step 3: Update `find_chunk_at_line` usage**

In `src/input/keymap.rs` line 286:
```rust
s.ab_repeat.chunk_index = s.ab_repeat.find_chunk_at_line(s.current_line, lines);
```

When `line_map` is present, `current_line` is a buffer line but `find_chunk_at_line` expects a work-line index. Translate:

```rust
                        let work_idx = s.work_line_for_buffer(s.current_line);
                        s.ab_repeat.chunk_index = work_idx.and_then(|idx| {
                            s.ab_repeat.find_chunk_at_line(idx, lines)
                        });
```

- [ ] **Step 4: Run build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: chunk resolution translates through line_map"
```

---

### Task 11: Run full test suite and verify build

**Files:** None (verification only)

- [ ] **Step 1: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no errors

- [ ] **Step 3: Populate DB column for testing**

To test, add the `text_file` column and populate one entry:

```sql
ALTER TABLE works ADD COLUMN text_file TEXT;
UPDATE works SET text_file = '/home/mlj/utono/literature/shakespeare-william/folger-txt/hamlet_TXT_FolgerShakespeare.txt' WHERE abbrev = 'Ham';
```

Run: `sqlite3 ~/utono/litdb/data/lit.db` with the above SQL.

- [ ] **Step 4: Manual test**

The user runs `cargo run`, opens Hamlet, and verifies:
- Text displays with blank lines, speaker names, act/scene headers
- Cursor moves through all lines including blanks
- Timestamp gutter marks appear on the correct lines
- MPV sync highlights the correct line during playback
- Search finds text in the full file content
- Dialogue jumping (q/,) skips to dialogue lines

- [ ] **Step 5: Commit all remaining changes**

```bash
git add -A
git commit -m "feat: text file display for Shakespeare works with full feature mapping"
```
