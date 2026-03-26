# GtkSourceView 5 Migration + AB Repeat & Chunks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate from plain `gtk4::TextView` to `sourceview5::View` to gain per-line gutter renderers, then implement AB repeat looping and chunk visualization in the gutter — matching the Neovim `lit` plugin's sign column and chunk bar behavior.

**Architecture:** The `sourceview5::View` is a subclass of `gtk4::TextView`, so all existing buffer/tag/cursor code continues to work. The gutter gains structured per-line renderers replacing the current manual Cairo `DrawingArea`. AB repeat state (A/B points) and chunk data (loaded from the `chunks` DB table) drive gutter marks and dim-highlight overlays. MPV receives `ab-loop-a`/`ab-loop-b` properties for hardware looping.

**Tech Stack:** sourceview5 (crate `sourceview5 = { version = "0.9", features = ["gtk_v4_12"] }`), gtk4, rusqlite, existing MPV IPC bridge

---

## Background: What We're Building

### From Neovim `lit`

The Neovim plugin has two gutter features for audio-synced reading:

1. **Sign column** — per-line icons showing timestamp status and A/B point markers:
   - `◆` / `⋅` = has timestamp (chapter / non-chapter)
   - `◐` / `◑` = A-point / B-point
   - `●` / `■` = line is inside active loop

2. **Chunk bar** — a vertical bar in the leftmost column showing chunk boundaries:
   - `╷` top boundary, `│` interior, `╵` bottom boundary
   - Stored chunks render brighter than algorithm-computed chunks
   - Chunks are contiguous ranges of lines with audio times, stored in the `chunks` DB table

### AB Repeat

An audio loop between two text lines (A = start, B = end). Setting A and B sends `ab-loop-a` / `ab-loop-b` to MPV. Text outside A–B is dimmed. The user navigates chunks (pre-computed AB regions) with forward/backward keys.

### Database

Chunks live in `~/utono/litdb/data/lit.db`:

```sql
CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    work_abbrev TEXT NOT NULL,
    media_id INTEGER NOT NULL,
    div1 INTEGER NOT NULL,
    div2 INTEGER,
    a_line INTEGER NOT NULL,   -- first line (line_in_div)
    b_line INTEGER NOT NULL,   -- last line (inclusive)
    a_time REAL,               -- audio start (seconds)
    b_time REAL,               -- audio end (seconds)
    a_mid INTEGER DEFAULT 0,   -- starts mid-line
    b_mid INTEGER DEFAULT 0,   -- ends mid-line
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(work_abbrev, media_id, div1, div2, a_line)
);
```

Lines already have `line_mapping.div1`, `div2`, `line_in_div` in their citations (format: `abbrev.div1.div2.line_in_div`).

---

## File Structure

**New files:**
- `src/ab_repeat.rs` — AB repeat state, chunk loading, chunk navigation logic
- `src/db/chunks.rs` — DB queries for loading chunks
- `src/gutter.rs` — GtkSourceView 5 gutter renderer setup (replaces current `setup_sign_gutter_draw`)

**Modified files:**
- `Cargo.toml` — add `sourceview5` dependency
- `src/app.rs` — replace `gtk4::TextView`/`TextBuffer` with `sourceview5::View`/`Buffer`, remove `DrawingArea` gutter, wire new gutter renderers
- `src/db/mod.rs` — add `pub mod chunks;`
- `src/db/models.rs` — add `Chunk` struct, add `div1`/`div2`/`line_in_div` fields to `Line`
- `src/db/queries.rs` — preserve `div1`/`div2`/`line_in_div` as `Line` fields (already fetched, currently discarded)
- `src/main.rs` — add `mod ab_repeat; mod gutter;`, handle new MPV events for AB loop
- `src/mpv/commands.rs` — add `SetAbLoop` / `ClearAbLoop` commands
- `src/mpv/client.rs` — handle `SetAbLoop` / `ClearAbLoop` using existing `send_command`/`writer` pattern
- `src/input/keymap.rs` — add AB repeat and chunk navigation keybindings

---

## Task 1: Add sourceview5 Dependency and Verify Build

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add sourceview5 to Cargo.toml**

Add to `[dependencies]`:
```toml
sourceview5 = { version = "0.9", features = ["gtk_v4_12"] }
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: compiles (sourceview5 system lib `gtksourceview-5` is already installed)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add sourceview5 dependency for gutter infrastructure"
```

---

## Task 2: Add Line Coordinate Fields to Data Model

Chunks use `div1/div2/line_in_div` coordinates. The `Line` struct currently discards these after formatting the citation string. Add them as fields so chunk navigation can map between buffer line indices and chunk coordinates.

**Files:**
- Modify: `src/db/models.rs`
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Add fields to Line struct**

In `src/db/models.rs`, add to `Line`:
```rust
pub div1: i64,
pub div2: i64,
pub line_in_div: i64,
```

- [ ] **Step 2: Populate fields in load_work query**

In `src/db/queries.rs`, the `load_work` function already fetches `div1`, `div2`, `line_in_div` (lines 51-53) but only uses them for the citation format string. Add them to the `Line` construction:
```rust
Ok(Line {
    id: row.get(0)?,
    citation,
    is_dialogue: line_types::is_dialogue(&text, is_prose),
    text,
    normalized,
    speaker,
    timestamp: None,
    div1,
    div2,
    line_in_div,
})
```

- [ ] **Step 3: Verify tests pass**

Run: `cargo test`
Expected: all existing tests pass

- [ ] **Step 4: Commit**

```bash
git add src/db/models.rs src/db/queries.rs
git commit -m "feat: preserve div1/div2/line_in_div on Line struct for chunk mapping"
```

---

## Task 3: Add Chunk Data Model and DB Queries

**Files:**
- Create: `src/db/chunks.rs`
- Modify: `src/db/models.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Add Chunk model to models.rs**

```rust
#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: i64,
    pub a_line: i64,    // line_in_div of first line
    pub b_line: i64,    // line_in_div of last line (inclusive)
    pub a_time: Option<f64>,
    pub b_time: Option<f64>,
    pub a_mid: bool,
    pub b_mid: bool,
    pub div1: i64,
    pub div2: Option<i64>,
}
```

- [ ] **Step 2: Create src/db/chunks.rs**

```rust
use rusqlite::Connection;
use super::models::Chunk;

pub fn load_chunks(
    conn: &Connection,
    work_abbrev: &str,
    media_id: i64,
) -> Result<Vec<Chunk>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, a_line, b_line, a_time, b_time, a_mid, b_mid, div1, div2 \
         FROM chunks \
         WHERE work_abbrev = ?1 AND media_id = ?2 \
         ORDER BY div1, div2, a_line",
    )?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, media_id], |row| {
        Ok(Chunk {
            id: row.get(0)?,
            a_line: row.get(1)?,
            b_line: row.get(2)?,
            a_time: row.get(3)?,
            b_time: row.get(4)?,
            a_mid: row.get::<_, i64>(5)? != 0,
            b_mid: row.get::<_, i64>(6)? != 0,
            div1: row.get(7)?,
            div2: row.get(8)?,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 3: Add module to db/mod.rs**

```rust
pub mod chunks;
```

- [ ] **Step 4: Write test for chunk loading**

Add to `src/db/chunks.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries::open_db;

    #[test]
    fn test_load_chunks_no_error() {
        let conn = open_db().unwrap();
        // Should not error even if no chunks exist for this work/media combo
        let result = load_chunks(&conn, "Ref", 1);
        assert!(result.is_ok());
    }
}
```

Note: works without media (`Work.media_id == None`) will have no chunks loaded — this is expected.

- [ ] **Step 5: Run tests**

Run: `cargo test db::chunks`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/db/chunks.rs src/db/models.rs src/db/mod.rs
git commit -m "feat: add Chunk model and load_chunks query"
```

---

## Task 4: Migrate TextView to sourceview5::View

Replace `gtk4::TextView` with `sourceview5::View` and `gtk4::TextBuffer` with `sourceview5::Buffer` in `app.rs`. Since these are subclasses, all existing buffer/tag/cursor operations continue to work.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Update imports**

Add sourceview5 imports and remove unused gtk4 imports:
```rust
use sourceview5::prelude::*;  // Required for ViewExt methods
use sourceview5::View;
```

Remove `DrawingArea` and `TextView` from the `gtk4` import block. Keep `gtk4::TextBuffer` references — they will be replaced with `sourceview5::Buffer` in step 2.

- [ ] **Step 2: Switch buffer to sourceview5::Buffer**

Replace buffer construction (currently `TextBuffer::new(None)` or similar) with:
```rust
let buffer = sourceview5::Buffer::new(None);
```

`sourceview5::Buffer` is a subclass of `gtk4::TextBuffer`. All existing `TextTag`, `apply_tag`, `create_mark` operations remain valid. The `AppState.buffer` field type should change to `sourceview5::Buffer` (or remain `gtk4::TextBuffer` if you upcast — but using `sourceview5::Buffer` directly is cleaner for later `create_source_mark` calls).

- [ ] **Step 3: Update View construction**

Replace `TextView::builder()` with `View::builder()`. The builder API is compatible — `.buffer()`, `.wrap_mode()`, `.editable()`, `.cursor_visible()` all exist on `sourceview5::View`.

Add after construction:
```rust
text_view.set_show_line_numbers(false);
text_view.set_highlight_current_line(false);
```

Note: `set_show_line_numbers` and `set_highlight_current_line` require `sourceview5::prelude::*` in scope (methods live on `ViewExt`).

- [ ] **Step 4: Remove the DrawingArea sign gutter**

Remove:
- `DrawingArea` creation (lines ~104-108)
- `sign_gutter.add_css_class(...)` line
- `text_view.set_gutter(TextWindowType::Left, ...)` call
- `sign_gutter` field from `AppState` struct
- `setup_sign_gutter_draw()` function entirely
- `parse_hex_color()` helper (if only used by gutter)
- `sign_gutter` scroll sync in `connect_value_changed`
- `DrawingArea` from the `gtk4` use statement

Keep `sign_column_visible: Rc<Cell<bool>>` — it will control the new gutter renderers.

Important: after migration, the left gutter is accessed via `view.gutter(TextWindowType::Left)` which returns a `sourceview5::Gutter` object (for inserting renderers). This is **different** from `gtk4::TextView::set_gutter()` (for placing arbitrary widgets in border windows). Do not confuse the two.

- [ ] **Step 5: Update AppState struct**

Remove `sign_gutter: DrawingArea`. Change `text_view` field type if needed (now `sourceview5::View`). Change `buffer` field type to `sourceview5::Buffer`.

- [ ] **Step 6: Update toggle_sign_column**

Replace `state.sign_gutter.queue_draw()` with a call that shows/hides the sourceview gutter (to be wired in Task 5).

- [ ] **Step 7: Verify build**

Run: `cargo build`
Expected: compiles with warnings about unused `sign_column_visible` (gutter renderers not yet wired)

- [ ] **Step 8: Verify app runs**

Run the app manually, confirm text displays correctly with the new sourceview5::View.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "refactor: migrate from gtk4::TextView to sourceview5::View and Buffer"
```

---

## Task 5: Implement GtkSourceView 5 Gutter with Timestamp Marks

Replace the manual Cairo drawing with sourceview5's gutter renderer infrastructure. Use `sourceview5::Mark` categories to mark timestamped lines, and `connect_query_data` to conditionally render per-line content.

**Files:**
- Create: `src/gutter.rs`
- Modify: `src/app.rs`
- Modify: `src/main.rs` — add `mod gutter;`

- [ ] **Step 1: Create src/gutter.rs with mark setup**

```rust
use std::cell::Cell;
use std::rc::Rc;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use sourceview5::{Buffer as SrcBuffer, View};

const MARK_TIMESTAMP: &str = "timestamp";

/// Place sourceview5::Mark on each timestamped line.
pub fn place_timestamp_marks(buffer: &SrcBuffer, has_timestamp: &[bool]) {
    // Clear old timestamp marks
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    buffer.remove_source_marks(&start, &end, Some(MARK_TIMESTAMP));

    for (i, &has_ts) in has_timestamp.iter().enumerate() {
        if !has_ts {
            continue;
        }
        if let Some(iter) = buffer.iter_at_line(i as i32) {
            let mark_name = format!("ts-{}", i);
            buffer.create_source_mark(
                Some(&mark_name),
                MARK_TIMESTAMP,
                &iter,
            );
        }
    }
}
```

- [ ] **Step 2: Set up gutter renderer with per-line query_data**

In `gutter.rs`, add a function that creates a `GutterRendererText` and connects `query_data` to conditionally show content:

```rust
pub fn setup_timestamp_gutter(view: &View, visible: Rc<Cell<bool>>, has_timestamp: Vec<bool>) {
    let gutter = view.gutter(gtk4::TextWindowType::Left);
    let renderer = sourceview5::GutterRendererText::new();
    gutter.insert(&renderer, 0);

    renderer.connect_query_data(move |renderer, _object, line| {
        let renderer = renderer.downcast_ref::<sourceview5::GutterRendererText>().unwrap();
        if !visible.get() {
            renderer.set_text("");
            return;
        }
        let idx = line as usize;
        if idx < has_timestamp.len() && has_timestamp[idx] {
            renderer.set_text("│");
        } else {
            renderer.set_text("");
        }
    });
}
```

Note: the `connect_query_data` signal fires for each visible line during gutter rendering. This is the core mechanism for conditional per-line gutter content.

- [ ] **Step 3: Wire gutter setup into app.rs**

After `display_work` populates the buffer, build the timestamp vec and call:
```rust
let has_timestamp: Vec<bool> = work.lines.iter().map(|l| l.timestamp.is_some()).collect();
gutter::place_timestamp_marks(&state.buffer, &has_timestamp);
gutter::setup_timestamp_gutter(&state.text_view, state.sign_column_visible.clone(), has_timestamp);
```

- [ ] **Step 4: Add mod declaration to main.rs**

```rust
mod gutter;
```

- [ ] **Step 5: Verify build and test visually**

Run: `cargo build` then run app
Expected: vertical bars appear in gutter for timestamped lines (when sign column toggled on)

- [ ] **Step 6: Commit**

```bash
git add src/gutter.rs src/app.rs src/main.rs
git commit -m "feat: sourceview5 gutter with timestamp marks and query_data renderer"
```

---

## Task 6: AB Repeat State and MPV Loop Commands

**Files:**
- Create: `src/ab_repeat.rs`
- Modify: `src/mpv/commands.rs`
- Modify: `src/mpv/client.rs` — use existing `send_command`/`writer` pattern
- Modify: `src/main.rs`

- [ ] **Step 1: Add MPV commands for AB loop**

In `src/mpv/commands.rs`, add to `MpvCommand`:
```rust
SetAbLoop { a: f64, b: f64 },
ClearAbLoop,
```

- [ ] **Step 2: Handle AB loop commands in client.rs**

In the command match in `src/mpv/client.rs`, use the existing `send_command` helper and `writer` parameter (not a raw `stream`):
```rust
MpvCommand::SetAbLoop { a, b } => {
    if let Some(w) = writer.as_mut() {
        let cmd_a = format!(r#"{{"command":["set_property","ab-loop-a",{}]}}"#, a);
        let cmd_b = format!(r#"{{"command":["set_property","ab-loop-b",{}]}}"#, b);
        let seek = format!(r#"{{"command":["seek",{},"absolute"]}}"#, a);
        let _ = send_command(w, &cmd_a).await;
        let _ = send_command(w, &cmd_b).await;
        let _ = send_command(w, &seek).await;
    }
}
MpvCommand::ClearAbLoop => {
    if let Some(w) = writer.as_mut() {
        let cmd_a = r#"{"command":["set_property","ab-loop-a","no"]}"#;
        let cmd_b = r#"{"command":["set_property","ab-loop-b","no"]}"#;
        let _ = send_command(w, cmd_a).await;
        let _ = send_command(w, cmd_b).await;
    }
}
```

- [ ] **Step 3: Create src/ab_repeat.rs with state**

```rust
use crate::db::models::Chunk;

#[derive(Debug, Clone, Default)]
pub struct AbRepeatState {
    pub a_line: Option<usize>,  // buffer line index
    pub b_line: Option<usize>,  // buffer line index
    pub a_time: Option<f64>,
    pub b_time: Option<f64>,
    pub loop_active: bool,
    pub chunks: Vec<Chunk>,
    pub chunk_index: Option<usize>,
}

impl AbRepeatState {
    pub fn set_a(&mut self, line: usize, time: f64) {
        self.a_line = Some(line);
        self.a_time = Some(time);
        // If A >= B, clear B
        if let Some(b) = self.b_time {
            if time >= b {
                self.b_line = None;
                self.b_time = None;
            }
        }
        self.loop_active = false;
    }

    pub fn set_b(&mut self, line: usize, time: f64) {
        self.b_line = Some(line);
        self.b_time = Some(time);
        self.loop_active = false;
    }

    pub fn clear(&mut self) {
        self.a_line = None;
        self.b_line = None;
        self.a_time = None;
        self.b_time = None;
        self.loop_active = false;
    }

    pub fn can_loop(&self) -> bool {
        matches!((&self.a_time, &self.b_time), (Some(a), Some(b)) if a < b)
    }

    /// Find the chunk containing the given buffer line index.
    pub fn find_chunk_at_line(&self, line: usize, lines: &[crate::db::models::Line]) -> Option<usize> {
        if line >= lines.len() { return None; }
        let l = &lines[line];
        self.chunks.iter().position(|c| {
            c.div1 == l.div1
                && c.div2 == Some(l.div2)
                && l.line_in_div >= c.a_line
                && l.line_in_div <= c.b_line
        })
    }

    pub fn next_chunk(&mut self) -> Option<&Chunk> {
        let idx = self.chunk_index.map(|i| i + 1).unwrap_or(0);
        if idx < self.chunks.len() {
            self.chunk_index = Some(idx);
            Some(&self.chunks[idx])
        } else {
            None
        }
    }

    pub fn prev_chunk(&mut self) -> Option<&Chunk> {
        let idx = self.chunk_index.and_then(|i| i.checked_sub(1))?;
        self.chunk_index = Some(idx);
        Some(&self.chunks[idx])
    }
}
```

- [ ] **Step 4: Add ab_repeat field to AppState**

In `src/app.rs`:
```rust
pub ab_repeat: AbRepeatState,
```

Initialize with `AbRepeatState::default()`.

- [ ] **Step 5: Add mod declaration**

In `src/main.rs`:
```rust
mod ab_repeat;
```

- [ ] **Step 6: Verify build**

Run: `cargo build`
Expected: compiles with warnings about unused fields

- [ ] **Step 7: Commit**

```bash
git add src/ab_repeat.rs src/mpv/commands.rs src/mpv/client.rs src/app.rs src/main.rs
git commit -m "feat: add AB repeat state and MPV ab-loop commands"
```

---

## Task 7: Chunk Navigation Keybindings

Wire keybindings to load chunks on work open, navigate between them, and activate AB loops.

**Files:**
- Modify: `src/input/keymap.rs`
- Modify: `src/app.rs` — load chunks on work open

- [ ] **Step 1: Load chunks when opening a work**

In the `display_work` function in `app.rs`, after loading the work, load chunks:
```rust
if let Some(media_id) = state.current_work.as_ref().and_then(|w| w.media_id) {
    if let Ok(conn) = crate::db::queries::open_db() {
        let abbrev = &state.current_work.as_ref().unwrap().abbrev;
        if let Ok(chunks) = crate::db::chunks::load_chunks(&conn, abbrev, media_id) {
            state.ab_repeat.chunks = chunks;
        }
    }
}
```

- [ ] **Step 2: Add keybindings**

In `src/input/keymap.rs`:

| Key | Action |
|-----|--------|
| `x` | Loop current/next chunk forward |
| `y` | Loop current/previous chunk backward |
| `Escape` | Clear AB loop |

Note: these match the Neovim lit keybindings. Verify no conflicts with existing bindings.

- [ ] **Step 3: Verify build**

Run: `cargo build`

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs src/app.rs
git commit -m "feat: chunk navigation keybindings (x/y/Escape)"
```

---

## Task 8: Chunk Bar in Gutter

Add a second gutter renderer column showing chunk boundaries — vertical bars with `╷` / `│` / `╵` characters, matching the Neovim lit chunk bar.

**Files:**
- Modify: `src/gutter.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add chunk mark categories**

In `gutter.rs`:
```rust
const MARK_CHUNK_START: &str = "chunk-start";
const MARK_CHUNK_INTERIOR: &str = "chunk-interior";
const MARK_CHUNK_END: &str = "chunk-end";
const MARK_CHUNK_SINGLE: &str = "chunk-single"; // single-line chunk
```

- [ ] **Step 2: Add place_chunk_marks function**

Place sourceview5 marks for each chunk's line range, categorized by position (start/interior/end). Requires resolving chunk `div1/div2/line_in_div` to buffer line indices using the `Line` struct's coordinate fields (added in Task 2).

- [ ] **Step 3: Add chunk bar gutter renderer**

A second `GutterRendererText` with `connect_query_data` that renders `╷`, `│`, `╵` based on the mark category at each line. Insert at position -1 (before the timestamp renderer) so it appears as the leftmost column.

- [ ] **Step 4: Wire into app.rs after chunk load**

- [ ] **Step 5: Verify visually**

- [ ] **Step 6: Commit**

```bash
git add src/gutter.rs src/app.rs
git commit -m "feat: chunk bar visualization in sourceview5 gutter"
```

---

## Task 9: AB Loop Dim Highlighting

When an AB loop is active, dim all text outside the A–B range (matching Neovim lit's `LitLoopDim` behavior).

**Files:**
- Modify: `src/app.rs` — add dim tag application for AB range
- Modify: `src/ab_repeat.rs` — add highlight range calculation

- [ ] **Step 1: Create a dim tag for AB inactive regions**

Reuse existing `dim_tag` pattern — apply to lines before A and after B.

- [ ] **Step 2: Apply/remove dim on loop activate/deactivate**

- [ ] **Step 3: Verify visually**

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/ab_repeat.rs
git commit -m "feat: dim text outside active AB loop range"
```

---

## Task 10: A/B Point Gutter Signs

Show A-point and B-point indicators in the gutter (matching Neovim's `◐`/`◑` signs).

**Files:**
- Modify: `src/gutter.rs`

- [ ] **Step 1: Add mark categories for A/B points**

```rust
const MARK_A_POINT: &str = "ab-a";
const MARK_B_POINT: &str = "ab-b";
```

- [ ] **Step 2: Update gutter renderer to show A/B indicators**

- [ ] **Step 3: Wire mark placement on A/B set/clear**

- [ ] **Step 4: Commit**

```bash
git add src/gutter.rs src/ab_repeat.rs
git commit -m "feat: A/B point gutter signs"
```

---

## Notes

- **Buffer type:** Task 4 changes `gtk4::TextBuffer` to `sourceview5::Buffer` (subclass). All existing `TextTag` operations remain compatible. `create_source_mark` and `remove_source_marks` are available in sourceview5 0.9.1.
- **Line index mapping:** Chunks use `div1/div2/line_in_div` coordinates. Task 2 adds these fields to the `Line` struct. Chunk-to-buffer-line resolution uses these fields directly — no separate mapping vec needed.
- **Existing features preserved:** search highlighting, cursor sync, font cycling, library picker — all use `TextBuffer`/`TextTag` APIs that `sourceview5::Buffer` inherits.
- **sourceview5 crate version:** Using `0.9` with `gtk_v4_12` feature to match `gtk4 = { version = "0.9", features = ["v4_12"] }`.
- **Gutter API distinction:** `sourceview5::ViewExt::gutter()` returns a `sourceview5::Gutter` (for inserting renderers). This is different from `gtk4::TextView::set_gutter()` (for placing arbitrary widgets in border windows). After migration, use the former.
- **Works without media:** `Work.media_id` is `Option<i64>`. Works without media will have no chunks loaded — this is expected behavior.
