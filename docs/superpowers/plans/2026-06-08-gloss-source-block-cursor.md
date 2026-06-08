# Gloss Source-Block Cursor + Media Playback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the gloss-overlay accent-bar cursor stop on every block (source passages + explication paragraphs); Space on a source block seeks the media to the first quoted line's start time and plays, or falls back to TTS of the verse text.

**Architecture:** Generalize the existing explication-only cursor (`ParaRange`/`explication_paragraphs`/`current_explication_para`) to block-level (`BlockRange`/`gloss_blocks`/`current_block`) covering both source `<speaker>`/`<verse>` runs and explication paragraphs. Add a `kind` column to the `gloss_audio` cache via a one-time table rebuild. Branch Space behavior: explication → TTS (unchanged); source → MPV `ResumeAndSeek` if timed + connected, else TTS fallback.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite (bundled SQLite), tokio, glib, rodio, ElevenLabs (reqwest).

---

## Reference facts (verified against current code on `master`)

- Spec: `docs/superpowers/specs/2026-06-08-gloss-source-block-cursor-design.md`.
- A gloss parses to `Vec<GlossElement>` (`Speaker(String) | Verse(String) | Gloss(String)`) via `parse_gloss_tags` in `src/ui/gloss_overlay.rs`. `split_echo(&text).is_none()` ⇒ a `Gloss` is an explication (not an echo bracket).
- Current cursor machinery in `src/ui/gloss_overlay.rs`: `struct ParaRange { paragraph_index, start_line, end_line }` (line ~17); field `explication_paras: Rc<RefCell<Vec<ParaRange>>>` (line ~70); `let` binding (~179); constructor field (~423); cleared in `show_echoes`/`show_synopsis`/`show_glossing`/`show_loading_message` (lines ~609/690/826/1261); `rebuild_explication_ranges` (~1014), `current_explication_para` (~1055), `mark_cursor_paragraph` (~1079). `mark_cursor_paragraph` called from `scroll_gloss`/`scroll_gloss_to_top`/`scroll_gloss_to_bottom` and end of `show_gloss_with_color`.
- `pub fn explication_paragraphs(gloss: &str) -> Vec<(i32, String)>` (~1437) is called by the action layer.
- Action: `read_current_paragraph` in `src/input/actions/gloss.rs` (~512); helpers `gloss_audio_dir` (~624), `show_tts_toast` (~631). Keybind arm `keymap.rs:734` calls `read_current_paragraph(state)`.
- Queries `src/db/queries.rs`: `find_gloss_audio(conn, gloss_id, paragraph_index: i64)`, `save_gloss_audio(conn, gloss_id, paragraph_index: i64, audio_path, voice_id, model_id)`, `delete_gloss_audio(conn, gloss_id)`, `ensure_gloss_audio_table(conn)`. Migration is called from the `BOOKMARKS_INIT` `Once` in `src/app.rs`.
- Timing: `Line.timestamp: Option<TimeRange>`, `TimeRange.start: f64`. Source verse line text matches the work line text verbatim (the gloss quotes verbatim). `s.current_work.lines: Vec<Line>`. `s.mpv_connected: bool`. `MpvCommand::ResumeAndSeek(f64)` seeks loaded media and resumes (same as `play_current_line`).
- Build only — do NOT run the app. `cargo build` / `cargo test --bins`. Audio/MPV/rendered-bar are manual-verify. Global rule: suppress verbose Bash output.
- This work goes on a NEW branch off `master` (see Task 0).

---

## Task 0: Branch

**Files:** none (git only)

- [ ] **Step 1: Create the feature branch**

Run:
```bash
cd /home/mlj/utono/linux-lit
git checkout master
git checkout -b feat/gloss-source-block-cursor
```
Expected: on `feat/gloss-source-block-cursor`.

---

## Task 1: `gloss_blocks` block parser

**Files:**
- Modify: `src/ui/gloss_overlay.rs`
- Test: inline `#[cfg(test)]` in `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Write the failing test**

Add a new test module to `src/ui/gloss_overlay.rs`:

```rust
#[cfg(test)]
mod block_tests {
    use super::*;

    #[test]
    fn blocks_in_document_order_with_kinds() {
        let gloss = "<speaker>CRANMER</speaker>\n\
                     <verse>Ah, my good Lord of Winchester, I thank you.</verse>\n\
                     <verse>You are always my good friend.</verse>\n\
                     <gloss>Cranmer opens with cutting irony.</gloss>\n\
                     <speaker>CRANMER</speaker>\n\
                     <verse>'Tis my undoing. Love and meekness, lord,</verse>\n\
                     <gloss>The tone shifts from irony to sincere counsel.</gloss>\n\
                     <gloss>[\"a quote\" — Macbeth 1.1]</gloss>";
        let blocks = gloss_blocks(gloss);
        assert_eq!(blocks.len(), 4); // source, explication, source, explication (echo excluded)

        assert_eq!(blocks[0].kind, BlockKind::Source);
        assert_eq!(blocks[0].index, 0);
        assert_eq!(
            blocks[0].text,
            "Ah, my good Lord of Winchester, I thank you.\nYou are always my good friend."
        );

        assert_eq!(blocks[1].kind, BlockKind::Explication);
        assert_eq!(blocks[1].index, 0);
        assert_eq!(blocks[1].text, "Cranmer opens with cutting irony.");

        assert_eq!(blocks[2].kind, BlockKind::Source);
        assert_eq!(blocks[2].index, 1);
        assert_eq!(blocks[2].text, "'Tis my undoing. Love and meekness, lord,");

        assert_eq!(blocks[3].kind, BlockKind::Explication);
        assert_eq!(blocks[3].index, 1);
        assert_eq!(blocks[3].text, "The tone shifts from irony to sincere counsel.");
    }

    #[test]
    fn all_echo_gloss_has_only_source_block() {
        let gloss = "<speaker>HAMLET</speaker>\n\
                     <verse>To be, or not to be</verse>\n\
                     <gloss>[\"q\" — Lr 1.1]</gloss>";
        let blocks = gloss_blocks(gloss);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Source);
        assert_eq!(blocks[0].text, "To be, or not to be");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins blocks_in_document_order_with_kinds`
Expected: FAIL — `cannot find type BlockKind` / `cannot find function gloss_blocks`.

- [ ] **Step 3: Write minimal implementation**

Add near `explication_paragraphs` in `src/ui/gloss_overlay.rs`:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Source,
    Explication,
}

/// One cursor stop in the gloss, in document order.
pub struct GlossBlock {
    pub kind: BlockKind,
    /// 0-based index WITHIN its kind (source blocks numbered separately from
    /// explication paragraphs).
    pub index: i32,
    /// For Source: the joined verse-line text (speaker labels excluded).
    /// For Explication: the paragraph prose.
    pub text: String,
}

/// Parse a gloss into ordered cursor-stop blocks: each contiguous
/// `<speaker>`/`<verse>` run is one Source block; each non-echo `<gloss>` is one
/// Explication block. Echo `<gloss>` brackets are excluded. Source and
/// explication indices increment independently.
pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock> {
    let mut blocks = Vec::new();
    let mut source_idx = 0i32;
    let mut expl_idx = 0i32;
    let mut pending_verses: Vec<String> = Vec::new();

    let flush_source =
        |blocks: &mut Vec<GlossBlock>, source_idx: &mut i32, pending: &mut Vec<String>| {
            if !pending.is_empty() {
                blocks.push(GlossBlock {
                    kind: BlockKind::Source,
                    index: *source_idx,
                    text: pending.join("\n"),
                });
                *source_idx += 1;
                pending.clear();
            }
        };

    for el in parse_gloss_tags(gloss) {
        match el {
            GlossElement::Speaker(_) => { /* drop speaker labels from source text */ }
            GlossElement::Verse(text) => pending_verses.push(text.trim().to_string()),
            GlossElement::Gloss(text) => {
                if split_echo(&text).is_some() {
                    continue; // echo bracket: not a cursor stop
                }
                // A real explication paragraph ends the current source run.
                flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
                blocks.push(GlossBlock {
                    kind: BlockKind::Explication,
                    index: expl_idx,
                    text: text.trim().to_string(),
                });
                expl_idx += 1;
            }
        }
    }
    // Trailing source run (gloss that ends on verse).
    flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
    blocks
}
```

Note: an echo `<gloss>` does NOT flush the pending source run (it is invisible to the cursor), so verses straddling an echo stay in one source block. This matches "echo brackets are excluded".

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins block_tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): gloss_blocks — source + explication cursor stops"
```

---

## Task 2: Generalize the cursor ranges to blocks

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

No unit test (GTK-coupled). Verify by COMPILING.

This replaces `ParaRange`/`explication_paras`/`rebuild_explication_ranges`/`current_explication_para`/`mark_cursor_paragraph` with block-level equivalents. `explication_paragraphs` (the pub fn used by the action layer) is REMOVED here and the action layer is updated in Task 5; to keep the tree compiling between tasks, this task keeps `explication_paragraphs` in place (it's still pub and harmless) and only adds the block machinery + repoints the internal cursor to it. Task 5 removes the now-unused `explication_paragraphs` if the action layer no longer calls it.

- [ ] **Step 1: Replace `ParaRange` with `BlockRange`**

In `src/ui/gloss_overlay.rs`, replace the `struct ParaRange { ... }` definition (~line 17) with:

```rust
/// Buffer-line span of one cursor-stop block (source or explication).
struct BlockRange {
    kind: BlockKind,
    index: i32,
    start_line: i32,
    end_line: i32,
}
```

- [ ] **Step 2: Rename the field**

Change the struct field (~line 70):

```rust
    blocks: Rc<RefCell<Vec<BlockRange>>>,
```

The `let` binding (~line 179):

```rust
        let blocks: Rc<RefCell<Vec<BlockRange>>> = Rc::new(RefCell::new(Vec::new()));
```

The constructor literal field (~line 423): change `explication_paras,` to `blocks,`.

The four clear sites (`show_echoes` ~609, `show_glossing` ~690, `show_synopsis` ~826, `show_loading_message` ~1261): change each `self.explication_paras.borrow_mut().clear();` to `self.blocks.borrow_mut().clear();`. (Confirm exact line numbers by grepping `explication_paras` — replace every remaining reference.)

- [ ] **Step 3: Replace `rebuild_explication_ranges` with `rebuild_block_ranges`**

Replace the whole `fn rebuild_explication_ranges` (~1011–1050) with:

```rust
    /// Recompute `blocks` line spans from the current buffer + gloss text. Each
    /// block is located by scanning buffer lines for its first text line; a
    /// source block extends to its last verse line.
    fn rebuild_block_ranges(&self, gloss: &str) {
        let blocks = gloss_blocks(gloss);
        let buffer = self.gloss_view.buffer();
        let line_count = buffer.line_count();
        let mut ranges: Vec<BlockRange> = Vec::new();
        let mut search_from = 0i32;

        // Find the first buffer line at or after `from` whose trimmed text
        // starts with `needle`. Returns the line index, or None.
        let find_line = |needle: &str, from: i32| -> Option<i32> {
            if needle.is_empty() {
                return None;
            }
            for line in from..line_count {
                if let Some(start) = buffer.iter_at_line(line) {
                    let mut end = start.clone();
                    if !end.ends_line() {
                        end.forward_to_line_end();
                    }
                    let line_text = buffer.text(&start, &end, false);
                    if line_text.as_str().trim().starts_with(needle) {
                        return Some(line);
                    }
                }
            }
            None
        };

        for b in blocks {
            let lines: Vec<&str> = b.text.lines().collect();
            let first_needle = lines.first().map(|s| s.trim()).unwrap_or("");
            let start_line = match find_line(first_needle, search_from) {
                Some(l) => l,
                None => continue,
            };
            // Explication = single logical buffer line. Source = span to its
            // last verse line.
            let end_line = if b.kind == BlockKind::Source && lines.len() > 1 {
                let last_needle = lines.last().map(|s| s.trim()).unwrap_or("");
                find_line(last_needle, start_line).unwrap_or(start_line)
            } else {
                start_line
            };
            ranges.push(BlockRange {
                kind: b.kind,
                index: b.index,
                start_line,
                end_line,
            });
            search_from = end_line + 1;
        }
        *self.blocks.borrow_mut() = ranges;
    }
```

- [ ] **Step 4: Replace `current_explication_para` with `current_block`**

Replace `fn current_explication_para` (~1055) with:

```rust
    /// The cursor-stop block nearest the viewport vertical center, as
    /// `(kind, index)`. None when the current card has no blocks
    /// (echoes/synopsis/empty gloss).
    pub fn current_block(&self) -> Option<(BlockKind, i32)> {
        let ranges = self.blocks.borrow();
        if ranges.is_empty() {
            return None;
        }
        let adj = self.gloss_scrolled.vadjustment();
        let center_y = adj.value() + adj.page_size() / 2.0;
        let buffer = self.gloss_view.buffer();
        let mut best: Option<((BlockKind, i32), f64)> = None;
        for r in ranges.iter() {
            if let Some(iter) = buffer.iter_at_line(r.start_line) {
                let (y, h) = self.gloss_view.line_yrange(&iter);
                let mid = (y + self.gloss_view.top_margin()) as f64 + h as f64 / 2.0;
                let dist = (mid - center_y).abs();
                if best.map(|(_, d)| dist < d).unwrap_or(true) {
                    best = Some(((r.kind, r.index), dist));
                }
            }
        }
        best.map(|(k, _)| k)
    }
```

- [ ] **Step 5: Replace `mark_cursor_paragraph` with `mark_cursor_block`**

Replace `fn mark_cursor_paragraph` (~1079) with:

```rust
    /// Move the left accent bar to the current cursor block and repaint. No-op
    /// when there are no blocks.
    fn mark_cursor_block(&self) {
        let (kind, index) = match self.current_block() {
            Some(t) => t,
            None => return,
        };
        let span = self
            .blocks
            .borrow()
            .iter()
            .find(|r| r.kind == kind && r.index == index)
            .map(|r| (r.start_line, r.end_line));
        if let Some((start_line, end_line)) = span {
            *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
            self.bar_drawing.queue_draw();
        }
    }
```

- [ ] **Step 6: Repoint the call sites**

- In `show_gloss_with_color`: change `self.rebuild_explication_ranges(gloss);` to `self.rebuild_block_ranges(gloss);` and the trailing `self.mark_cursor_paragraph();` to `self.mark_cursor_block();`.
- In `scroll_gloss`, `scroll_gloss_to_top`, `scroll_gloss_to_bottom`: change each `self.mark_cursor_paragraph();` to `self.mark_cursor_block();`.

Grep `mark_cursor_paragraph` and `rebuild_explication_ranges` to confirm zero remaining references.

- [ ] **Step 7: Verify it compiles**

Run: `cargo build`
Expected: clean. `current_block`/`gloss_blocks` warn dead_code only where not yet called by the action layer — `current_block` is `pub` and called in Task 5, so a dead_code warning is acceptable here. (`explication_paragraphs` is still present/pub from Task 1's file; it may warn dead_code if the action layer hasn't switched yet — that's fine until Task 5.)

- [ ] **Step 8: Run the block tests + full bins suite**

Run: `cargo test --bins`
Expected: PASS (existing suite + Task 1's `block_tests`).

- [ ] **Step 9: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): generalize cursor to blocks (source + explication)"
```

---

## Task 3: Add `kind` to the audio cache (schema + migration)

**Files:**
- Modify: `src/db/queries.rs`
- Test: inline `#[cfg(test)]` in `src/db/queries.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` containing `gloss_audio_roundtrip_and_upsert`:

```rust
#[test]
fn gloss_audio_kind_distinct_rows() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
         INSERT INTO glosses (id) VALUES (9);",
    )
    .unwrap();
    ensure_gloss_audio_table(&conn).unwrap();

    // Same gloss_id + same index, different kind -> two distinct rows.
    save_gloss_audio(&conn, 9, "explication", 0, "/e0.mp3", "v", "m").unwrap();
    save_gloss_audio(&conn, 9, "source", 0, "/s0.mp3", "v", "m").unwrap();
    assert_eq!(find_gloss_audio(&conn, 9, "explication", 0).unwrap(), Some("/e0.mp3".to_string()));
    assert_eq!(find_gloss_audio(&conn, 9, "source", 0).unwrap(), Some("/s0.mp3".to_string()));

    // Upsert respects the (gloss_id, kind, index) key.
    save_gloss_audio(&conn, 9, "source", 0, "/s0b.mp3", "v", "m").unwrap();
    assert_eq!(find_gloss_audio(&conn, 9, "source", 0).unwrap(), Some("/s0b.mp3".to_string()));
    assert_eq!(find_gloss_audio(&conn, 9, "explication", 0).unwrap(), Some("/e0.mp3".to_string()));
}

#[test]
fn gloss_audio_migrates_legacy_table() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
         INSERT INTO glosses (id) VALUES (3);",
    )
    .unwrap();
    // Legacy table shape (no `kind` column), with one row.
    conn.execute_batch(
        "CREATE TABLE gloss_audio (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            gloss_id INTEGER NOT NULL,
            paragraph_index INTEGER NOT NULL,
            audio_path TEXT NOT NULL,
            voice_id TEXT NOT NULL,
            model_id TEXT NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(gloss_id, paragraph_index)
        );
        INSERT INTO gloss_audio (gloss_id, paragraph_index, audio_path, voice_id, model_id)
            VALUES (3, 0, '/legacy0.mp3', 'v', 'm');",
    )
    .unwrap();

    // Migration: legacy row preserved and labeled 'explication'.
    ensure_gloss_audio_table(&conn).unwrap();
    assert_eq!(find_gloss_audio(&conn, 3, "explication", 0).unwrap(), Some("/legacy0.mp3".to_string()));
    // And a source row can now coexist at the same index.
    save_gloss_audio(&conn, 3, "source", 0, "/s.mp3", "v", "m").unwrap();
    assert_eq!(find_gloss_audio(&conn, 3, "source", 0).unwrap(), Some("/s.mp3".to_string()));
}
```

Also UPDATE the existing `gloss_audio_roundtrip_and_upsert` and `delete_gloss_audio_removes_rows` tests to the new signatures: every `find_gloss_audio(&conn, id, idx)` becomes `find_gloss_audio(&conn, id, "explication", idx)`, and every `save_gloss_audio(&conn, id, idx, path, v, m)` becomes `save_gloss_audio(&conn, id, "explication", idx, path, v, m)`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins gloss_audio`
Expected: FAIL — arity mismatch / new tests don't compile against old signatures.

- [ ] **Step 3: Rewrite `ensure_gloss_audio_table` (new shape + legacy rebuild)**

Replace the body of `ensure_gloss_audio_table` in `src/db/queries.rs` with:

```rust
pub fn ensure_gloss_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Fresh installs get the new shape directly.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gloss_audio (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            gloss_id        INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
            kind            TEXT NOT NULL DEFAULT 'explication',
            paragraph_index INTEGER NOT NULL,
            audio_path      TEXT NOT NULL,
            voice_id        TEXT NOT NULL,
            model_id        TEXT NOT NULL,
            timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(gloss_id, kind, paragraph_index)
        );
        CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);",
    )?;

    // Upgrade a legacy table (no `kind` column) by rebuilding to the new shape.
    let has_kind: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('gloss_audio') WHERE name = 'kind'")?
        .exists([])?;
    if !has_kind {
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE gloss_audio RENAME TO gloss_audio_old;
             CREATE TABLE gloss_audio (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gloss_id INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
                kind TEXT NOT NULL DEFAULT 'explication',
                paragraph_index INTEGER NOT NULL,
                audio_path TEXT NOT NULL,
                voice_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(gloss_id, kind, paragraph_index)
             );
             INSERT INTO gloss_audio (id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp)
                SELECT id, gloss_id, 'explication', paragraph_index, audio_path, voice_id, model_id, timestamp
                FROM gloss_audio_old;
             DROP TABLE gloss_audio_old;
             CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
             COMMIT;",
        )?;
    }
    Ok(())
}
```

Note: on a fresh DB, `CREATE TABLE IF NOT EXISTS` already made the new-shape table, so `has_kind` is true and the rebuild is skipped. On a legacy DB, the `CREATE TABLE IF NOT EXISTS` is a no-op (table exists), `has_kind` is false, and the rebuild runs once.

- [ ] **Step 4: Update `find_gloss_audio` / `save_gloss_audio` signatures**

Replace both functions:

```rust
/// Return the cached audio path for a gloss block, if any.
pub fn find_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM gloss_audio
         WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        rusqlite::params![gloss_id, kind, index],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Insert or replace the audio path for a gloss block.
pub fn save_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(gloss_id, kind, paragraph_index)
         DO UPDATE SET audio_path = excluded.audio_path,
                       voice_id   = excluded.voice_id,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![gloss_id, kind, index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}
```

`delete_gloss_audio` is unchanged.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins gloss_audio`
Expected: PASS (roundtrip, kind-distinct, legacy-migrate, delete).

This will FAIL TO COMPILE if the action layer call sites in `read_current_paragraph` still use the old arity — that is expected and fixed in Task 5. To keep this task's commit green, do Step 6 now.

- [ ] **Step 6: Fix the existing explication call sites to the new arity (minimal)**

In `src/input/actions/gloss.rs`, the current `read_current_paragraph` calls `find_gloss_audio(&conn, gloss_id, para_index as i64)` and `save_gloss_audio(&conn, gloss_id, para_index as i64, ...)`. Insert the `kind` argument as `"explication"` at both call sites so the crate compiles:

- `find_gloss_audio(&conn, gloss_id, "explication", para_index as i64)`
- `save_gloss_audio(&conn, gloss_id, "explication", para_index as i64, &path.to_string_lossy(), &voice_id, &model_id)`

(Task 5 restructures this function; this is only to keep the build green now.)

- [ ] **Step 7: Build + full suite**

Run: `cargo build && cargo test --bins`
Expected: clean build, all pass.

- [ ] **Step 8: Commit**

```bash
git add src/db/queries.rs src/input/actions/gloss.rs
git commit -m "feat(db): gloss_audio kind column + legacy rebuild migration"
```

---

## Task 4: Source-block timing resolver (pure helper)

**Files:**
- Modify: `src/input/actions/gloss.rs`
- Test: inline `#[cfg(test)]` in `src/input/actions/gloss.rs`

A pure helper that, given a source block's verse-line texts and the work's lines (as `(text, Option<start>)` pairs), returns the first start time among matching lines.

- [ ] **Step 1: Write the failing test**

Add to `src/input/actions/gloss.rs`:

```rust
#[cfg(test)]
mod source_timing_tests {
    use super::*;

    #[test]
    fn first_timed_matching_line_wins() {
        // Work lines as (text, Option<start_seconds>).
        let work: Vec<(String, Option<f64>)> = vec![
            ("Ah, my good Lord of Winchester, I thank you.".into(), None),
            ("You are always my good friend.".into(), Some(12.5)),
            ("I shall both find your Lordship judge and juror,".into(), Some(15.0)),
        ];
        // Block verses: first has no timing, second does -> 12.5 wins.
        let verses = "Ah, my good Lord of Winchester, I thank you.\nYou are always my good friend.";
        assert_eq!(first_source_start_time(verses, &work), Some(12.5));
    }

    #[test]
    fn none_when_no_match_has_timing() {
        let work: Vec<(String, Option<f64>)> = vec![
            ("Ah, my good Lord of Winchester, I thank you.".into(), None),
        ];
        let verses = "Ah, my good Lord of Winchester, I thank you.";
        assert_eq!(first_source_start_time(verses, &work), None);
    }

    #[test]
    fn none_when_no_text_match() {
        let work: Vec<(String, Option<f64>)> = vec![("Unrelated line.".into(), Some(1.0))];
        let verses = "Ah, my good Lord of Winchester, I thank you.";
        assert_eq!(first_source_start_time(verses, &work), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins first_timed_matching_line_wins`
Expected: FAIL — `cannot find function first_source_start_time`.

- [ ] **Step 3: Write the helper**

Add to `src/input/actions/gloss.rs` (module-level, not inside a fn):

```rust
/// Given a source block's verse text (one quoted line per `\n`) and the work's
/// lines as `(text, Option<start_seconds>)`, return the start time of the FIRST
/// verse line (in block order) that matches a work line carrying a timestamp.
/// Matching is exact on trimmed text. None if no matched line has timing.
fn first_source_start_time(verses: &str, work: &[(String, Option<f64>)]) -> Option<f64> {
    for verse in verses.lines() {
        let needle = verse.trim();
        if needle.is_empty() {
            continue;
        }
        for (text, start) in work {
            if text.trim() == needle {
                if let Some(s) = start {
                    return Some(*s);
                }
            }
        }
    }
    None
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins source_timing_tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): first_source_start_time resolver for source-block media seek"
```

---

## Task 5: Branch Space behavior (source vs explication)

**Files:**
- Modify: `src/input/actions/gloss.rs`
- Modify: `src/input/keymap.rs` (rename the entry-point call)
- Modify: `src/ui/gloss_overlay.rs` (remove now-unused `explication_paragraphs` if unreferenced)

No unit test (GTK/MPV/audio). Verify by COMPILING + manual.

- [ ] **Step 1: Rewrite `read_current_paragraph` as `read_current_block`**

Replace the entire `read_current_paragraph` function in `src/input/actions/gloss.rs` with:

```rust
/// Space in the gloss overlay: act on the cursor's current block.
/// - Explication block -> read the paragraph aloud via TTS (cached).
/// - Source block -> seek media to the first quoted line's start time and play
///   (when MPV is connected and the line is timestamped); otherwise fall back
///   to TTS of the verse text (cached).
pub(crate) fn read_current_block(state_rc: &Rc<RefCell<AppState>>) {
    use crate::ui::gloss_overlay::BlockKind;

    {
        let s = state_rc.borrow();
        if s.tts.is_playing() {
            s.tts.stop();
            return;
        }
    }

    // Resolve current block once; drop the Ref before any toast call.
    let block_opt = state_rc.borrow().gloss_overlay.current_block();
    let (kind, index) = match block_opt {
        Some(t) => t,
        None => {
            show_tts_toast(state_rc, "Nothing to read");
            return;
        }
    };

    // For a source block, try media playback first.
    if kind == BlockKind::Source {
        let seek = {
            let s = state_rc.borrow();
            if !s.mpv_connected {
                None
            } else {
                source_block_seek_time(&s, index)
            }
        };
        if let Some(start) = seek {
            let _ = state_rc
                .borrow()
                .cmd_tx
                .try_send(crate::mpv::MpvCommand::ResumeAndSeek(start));
            crate::log_fmt!("TTS: source block {} -> media seek {}", index, start);
            return;
        }
        // else: fall through to TTS fallback below.
    }

    // Resolve (gloss_id, work_abbrev, text, voice, model, handle) and the cache
    // kind string for this block.
    let kind_str = match kind {
        BlockKind::Source => "source",
        BlockKind::Explication => "explication",
    };
    let (gloss_id, work_abbrev, text, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let gloss = match s.gloss_list.get(s.gloss_index) {
            Some(g) => g,
            None => return,
        };
        let gloss_id = gloss.gloss_id;
        let work_abbrev = match &s.gloss_context {
            Some(ctx) => ctx.work_abbrev.clone(),
            None => return,
        };
        let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text);
        let text = match blocks.iter().find(|b| b.kind == kind && b.index == index) {
            Some(b) => b.text.clone(),
            None => return,
        };
        (
            gloss_id,
            work_abbrev,
            text,
            s.config.elevenlabs_voice_id.clone(),
            s.config.elevenlabs_model_id.clone(),
            s.tokio_handle.clone(),
        )
    };

    // Filename stem: explication uses "<index>", source uses "source-<index>".
    let stem = match kind {
        BlockKind::Source => format!("source-{}", index),
        BlockKind::Explication => format!("{}", index),
    };

    // Cache hit?
    if let Ok(conn) = crate::db::queries::open_db() {
        if let Ok(Some(path)) =
            crate::db::queries::find_gloss_audio(&conn, gloss_id, kind_str, index as i64)
        {
            if std::path::Path::new(&path).exists() {
                state_rc.borrow().tts.play_file(std::path::Path::new(&path));
                return;
            }
        }
    }

    // Miss: synthesize asynchronously.
    show_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    let kind_owned = kind_str.to_string();
    glib::spawn_future_local(async move {
        let voice = voice_id.clone();
        let model = model_id.clone();
        let result = tokio_handle
            .spawn(async move { crate::elevenlabs::synthesize(&text, &voice, &model).await })
            .await;

        match result {
            Ok(Ok(bytes)) => {
                let dir = gloss_audio_dir(&work_abbrev, gloss_id);
                let path = dir.join(format!("{}.mp3", stem));
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    crate::log_fmt!("TTS: mkdir {} failed: {}", dir.display(), e);
                    show_tts_toast(&state_for_result, "Could not save audio");
                    return;
                }
                if let Err(e) = std::fs::write(&path, &bytes) {
                    crate::log_fmt!("TTS: write {} failed: {}", path.display(), e);
                    show_tts_toast(&state_for_result, "Could not save audio");
                    return;
                }
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    if let Err(e) = crate::db::queries::save_gloss_audio(
                        &conn,
                        gloss_id,
                        &kind_owned,
                        index as i64,
                        &path.to_string_lossy(),
                        &voice_id,
                        &model_id,
                    ) {
                        crate::log_fmt!("TTS: save_gloss_audio failed: {}", e);
                    }
                }
                state_for_result.borrow().tts.play_file(&path);
                crate::log_fmt!("TTS: synthesized gloss {} {} {}", gloss_id, kind_owned, index);
            }
            Ok(Err(e)) => {
                crate::log_fmt!("TTS: synth error: {}", e);
                show_tts_toast(&state_for_result, &e.to_string());
            }
            Err(e) => {
                crate::log_fmt!("TTS: tokio join error: {}", e);
            }
        }
    });
}

/// Resolve a source block's first-line start time from the current work's line
/// timestamps. Returns None if no current work, no matching block, or no
/// matched verse line carries a timestamp.
fn source_block_seek_time(s: &AppState, index: i32) -> Option<f64> {
    let gloss = s.gloss_list.get(s.gloss_index)?;
    let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text);
    let block = blocks
        .iter()
        .find(|b| b.kind == crate::ui::gloss_overlay::BlockKind::Source && b.index == index)?;
    let work = s.current_work.as_ref()?;
    let work_pairs: Vec<(String, Option<f64>)> = work
        .lines
        .iter()
        .map(|l| (l.text.clone(), l.timestamp.map(|t| t.start)))
        .collect();
    first_source_start_time(&block.text, &work_pairs)
}
```

- [ ] **Step 2: Update the keybind entry point**

In `src/input/keymap.rs` (the `"space"` arm, ~line 734), change:

```rust
            crate::input::actions::gloss::read_current_paragraph(state);
```
to:
```rust
            crate::input::actions::gloss::read_current_block(state);
```

- [ ] **Step 3: Remove the now-unused `explication_paragraphs`**

Grep `explication_paragraphs` across `src/`. If the only remaining references are its definition + its old tests, the action layer no longer uses it. Remove the `pub fn explication_paragraphs` from `src/ui/gloss_overlay.rs` and its `explication_tests` module (the block-level `gloss_blocks` + `block_tests` supersede them). If anything else still references it, leave it and note so.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: clean. `read_current_block` no longer dead_code (called by keymap); `current_block`/`gloss_blocks` now used.

- [ ] **Step 5: Run the full pure-logic suite**

Run: `cargo test --bins`
Expected: PASS (all existing + new block/timing/kind tests).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/gloss.rs src/input/keymap.rs src/ui/gloss_overlay.rs
git commit -m "feat(gloss): Space on source block seeks media or TTS-falls-back"
```

---

## Task 6: Footer hint + Ctrl+/ overlay + final verification

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (footer hint wording)
- Modify: `src/ui/keybinds_overlay.rs` (via the update-cairo-keybinds-overlay skill, if applicable)

- [ ] **Step 1: Update the gloss footer hint**

In `show_gloss_with_color`, the hint currently reads:

```
"Esc close · Space read aloud · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage"
```

Change `Space read aloud` to reflect the dual behavior:

```rust
        self.hint.set_text("Esc close · Space play/read · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage");
```

- [ ] **Step 2: Ctrl+/ overlay**

The Space gloss-overlay bind is already represented (or determined reader-mode-only) from the prior TTS feature. Invoke the `update-cairo-keybinds-overlay` skill to confirm the Space describe() text reflects "play media or read aloud (gloss overlay)" and run its three-pass check. If the gloss-overlay binds are not individually represented in the overlay (established pattern from the prior feature), this is a no-op beyond confirming consistency — report which.

- [ ] **Step 3: Full build + test + clippy**

Run:
```bash
cargo build
cargo test --bins
cargo clippy
```
Expected: clean build, all pass, no new clippy errors.

- [ ] **Step 4: Manual verification handoff**

Ask the user to launch and verify (audio/MPV/rendered bar can't be tested headlessly):
1. `source ~/.config/shell/secrets && cargo run`
2. Open a **verse** gloss (e.g. H8 Cranmer). Scroll so the accent bar sits on the **source verse block** → press **Space** → media should seek to that passage's start and play.
3. Scroll so the bar sits on an **explication paragraph** → **Space** → TTS reads it (unchanged).
4. Open a **prose** gloss (e.g. Bleak House) where the source has no per-line timing → bar on the source block → **Space** → TTS speaks the quoted prose.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): gloss Space play/read footer + Ctrl+/ overlay"
```

---

## Self-review notes

- **Spec coverage:** block cursor (Tasks 1–2), source block = contiguous speaker/verse run with separate index (Task 1 `gloss_blocks`), accent bar follows scroll onto source + explication (Task 2 `mark_cursor_block`), Space branch source→media-seek/TTS-fallback (Task 5), first-line-start resolution verse-lines-only (Task 4 `first_source_start_time`), TTS text = verse lines only (Task 1 joins verse text, speaker dropped), `kind` cache column + legacy rebuild (Task 3), `source-<n>.mp3` filename (Task 5 `stem`), cross-work / no-timing / MPV-off all fall to TTS (Task 5 + `source_block_seek_time` returning None), footer + overlay (Task 6). All spec sections map to a task.
- **Type consistency:** `BlockKind`/`GlossBlock`/`gloss_blocks` defined in Task 1, used in Tasks 2 & 5; `BlockRange`/`current_block(&self) -> Option<(BlockKind, i32)>`/`mark_cursor_block` consistent across Task 2 & 5; `find_gloss_audio(conn, id, kind: &str, index: i64)` / `save_gloss_audio(conn, id, kind: &str, index, path, v, m)` consistent in Tasks 3 & 5; `first_source_start_time(&str, &[(String, Option<f64>)])` defined Task 4, used Task 5 via `source_block_seek_time`; `read_current_block` defined Task 5, called from keymap Task 5.
- **No placeholders:** every code step is complete; the inter-task compile-green concern (signature change in Task 3 vs caller in Task 5) is handled by Task 3 Step 6's minimal `"explication"` patch.
