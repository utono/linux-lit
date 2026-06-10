# Synopsis-as-gloss overlay + Shift+Space batch-synthesize — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the synopsis overlay render as cursor blocks with a left accent bar (navigated by j/k/gg/G like the gloss overlay), and add Shift+Space to batch-synthesize all prose blocks of the open gloss or synopsis to cached ElevenLabs MP3s with a "Synthesizing…" toast.

**Architecture:** Synopsis text in `lit.db` already uses `<p>` paragraph tags. A new pure `synopsis_blocks()` wraps each `<p>` as a `BlockKind::Explication` block, so the synopsis overlay can reuse the existing block/cursor/accent-bar machinery (`rebuild_block_ranges`/`mark_cursor_block`/`bar_ranges`/Cairo draw). Shift+Space loops those blocks, synthesizing+caching each (gloss → existing `gloss_audio` table; synopsis → new `synopsis_audio` table) with a fixed plain-prose voice, stopping on the first error.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite/SQLite, tokio, glib futures, ElevenLabs HTTP API.

**Reference spec:** `docs/superpowers/specs/2026-06-09-shift-space-batch-synthesize-design.md`

---

## File Structure

- `src/ui/gloss_overlay.rs` — add `synopsis_blocks()`; rework `show_synopsis` onto the block path. (Existing block machinery untouched.)
- `src/input/keymap.rs` — `handle_synopsis_overlay_key`: j/k → cursor blocks, arm/handle gg, G → last block; forward `is_shift` into both overlay handlers; Shift+Space dispatch.
- `src/db/queries.rs` — `SYNOPSIS_AUDIO_COLUMNS`, `ensure_synopsis_audio_table`, `find_synopsis_audio`, `save_synopsis_audio`.
- `src/app.rs` — `tts_batch_running: Cell<bool>` on `AppState` + initializer.
- `src/input/actions/gloss.rs` — `tts_batch_running` guard helpers, `synth_all_prose_blocks` (gloss), `synth_all_synopsis_blocks` (synopsis), `synopsis_audio_dir`.
- `src/ui/keybinds_overlay.rs` — Ctrl+/ overlay: Shift+Space cap + describe arm (done via the `update-cairo-keybinds-overlay` skill at the end).

Work order: Task 1 (synopsis_blocks) → Task 2 (show_synopsis block path) → Task 3 (synopsis key nav) → Task 4 (synopsis_audio DB) → Task 5 (AppState guard) → Task 6 (gloss batch) → Task 7 (synopsis batch) → Task 8 (keymap Shift+Space wiring) → Task 9 (Ctrl+/ overlay).

---

## Task 1: `synopsis_blocks()` — parse `<p>` paragraphs into Explication blocks

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add fn near `render_synopsis_with_labels` ~line 1547 and `gloss_blocks` ~line 1609)
- Test: same file, in the existing `#[cfg(test)] mod synopsis_label_tests` block (~line 2583) or a new `mod synopsis_blocks_tests`.

Context: `GlossBlock { kind, index, text, display }` and `BlockKind::{Source,Explication}` already exist (gloss_overlay.rs:1584-1603). `is_label_paragraph(p: &str) -> bool` exists (line 1539). `try_extract(after, "p")` extracts `<p>…</p>` content (used by `render_synopsis_with_labels`). Synopses are plain prose with no `/IPA/`, so `text == display` for synopsis blocks.

- [ ] **Step 1: Write the failing test**

Add to `src/ui/gloss_overlay.rs` test module:

```rust
#[cfg(test)]
mod synopsis_blocks_tests {
    use super::{synopsis_blocks, BlockKind};

    #[test]
    fn each_p_becomes_one_explication_block_skipping_labels() {
        let syn = "<p>First paragraph of action.</p>\
                   <p>Shakespearean parallels:</p>\
                   <p>Second paragraph continues.</p>";
        let blocks = synopsis_blocks(syn);
        // Label paragraph ("…parallels:") is skipped as a cursor stop.
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Explication));
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[1].index, 1);
        assert_eq!(blocks[0].text, "First paragraph of action.");
        assert_eq!(blocks[0].display, "First paragraph of action.");
        assert_eq!(blocks[1].text, "Second paragraph continues.");
    }

    #[test]
    fn legacy_plain_text_is_one_block() {
        let blocks = synopsis_blocks("Just plain text, no tags.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Explication);
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[0].text, "Just plain text, no tags.");
    }

    #[test]
    fn empty_yields_no_blocks() {
        assert_eq!(synopsis_blocks("").len(), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib synopsis_blocks_tests 2>&1 | tail -20`
Expected: FAIL — `cannot find function synopsis_blocks`.

- [ ] **Step 3: Write the implementation**

Add to `src/ui/gloss_overlay.rs` (e.g. directly after `render_synopsis_with_labels`, before `pub enum BlockKind` — or after `gloss_blocks`; placement is free since all are module-level):

```rust
/// Parse a `<p>`-tagged synopsis into cursor-stop blocks, one per paragraph,
/// each a `BlockKind::Explication` (synopses are prose, never verse). Label
/// paragraphs (`is_label_paragraph`, e.g. "Shakespearean parallels:") are shown
/// in the buffer but are NOT cursor stops, so they are skipped here — exactly
/// the paragraphs `render_synopsis_with_labels` marks for bolding. Synopsis text
/// carries no inline `/IPA/`, so `text == display`. Legacy untagged prose (no
/// `<p>`) is returned as a single block. Indices count the emitted (non-label)
/// blocks from 0, matching the cache `paragraph_index`.
pub fn synopsis_blocks(synopsis: &str) -> Vec<GlossBlock> {
    let mut paras: Vec<String> = Vec::new();
    let mut remaining = synopsis;
    while let Some(pos) = remaining.find("<p>") {
        let after = &remaining[pos..];
        if let Some((content, rest)) = try_extract(after, "p") {
            if !content.is_empty() {
                paras.push(content.to_string());
            }
            remaining = rest;
        } else {
            remaining = &remaining[pos + 3..];
        }
    }
    if paras.is_empty() {
        let t = synopsis.trim();
        if t.is_empty() {
            return Vec::new();
        }
        return vec![GlossBlock {
            kind: BlockKind::Explication,
            index: 0,
            text: t.to_string(),
            display: t.to_string(),
        }];
    }
    let mut blocks: Vec<GlossBlock> = Vec::new();
    let mut index = 0i32;
    for p in &paras {
        if is_label_paragraph(p) {
            continue;
        }
        blocks.push(GlossBlock {
            kind: BlockKind::Explication,
            index,
            text: p.clone(),
            display: p.clone(),
        });
        index += 1;
    }
    blocks
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib synopsis_blocks_tests 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): synopsis_blocks() parses <p> paragraphs into Explication blocks"
```

---

## Task 2: Render the synopsis overlay through the block/accent-bar machinery

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `show_synopsis` (lines 820-879).

Context: today `show_synopsis` clears the block machinery (line 856 `self.blocks.borrow_mut().clear()`, line 853 `bar_ranges = Vec::new()`) and never calls `rebuild_block_ranges`/`mark_cursor_block`, so the accent bar paints nothing and j/k pixel-scroll. `rebuild_block_ranges(&self, gloss: &str)` (line 1057) internally calls `gloss_blocks(gloss)` — it must instead use `synopsis_blocks`. The buffer is populated from `render_synopsis_with_labels(synopsis).0` (joined paragraphs with `\n\n`), whose first line of each paragraph is what `rebuild_block_ranges`' line matcher searches for — and `synopsis_blocks` `display` is the same paragraph text, so the matcher lines up. `bar_color` defaults to `(0.53, 0.62, 0.71)` (line 181); leave it as-is for the synopsis (same color as gloss). `bar_x` is set elsewhere to `card_width/4` — confirm it is set in `show_synopsis` (add if missing).

This is a rendered-pixel change; there is no pure unit test for it. It is verified at runtime (Task end / e2e).

- [ ] **Step 1: Add a synopsis-aware block-range rebuild**

`rebuild_block_ranges` hardcodes `gloss_blocks`. Add a sibling that takes pre-built blocks so both gloss and synopsis can share the line-matching body. In `src/ui/gloss_overlay.rs`, refactor: rename the body to take blocks, keep `rebuild_block_ranges` as a thin caller.

Replace the signature/first line of `rebuild_block_ranges` (line 1057):

```rust
    fn rebuild_block_ranges(&self, gloss: &str) {
        let blocks = gloss_blocks(gloss);
        self.rebuild_block_ranges_from(blocks);
    }

    /// Map a pre-built block list to buffer-line spans (shared by the gloss path,
    /// which builds blocks with `gloss_blocks`, and the synopsis path, which uses
    /// `synopsis_blocks`). Matches each block's first `display` line against
    /// buffer lines, stores `self.blocks`, resets the cursor to block 0.
    fn rebuild_block_ranges_from(&self, blocks: Vec<GlossBlock>) {
```

Everything from the original `let buffer = self.gloss_view.buffer();` (line 1059) through the end of the function (line 1107) stays as the body of `rebuild_block_ranges_from` (it already iterates `for b in blocks`).

- [ ] **Step 2: Rework `show_synopsis` to populate blocks + bar**

In `show_synopsis` (lines 820-879), make these edits:

Remove the block-clearing line. Change (line 856):

```rust
        self.blocks.borrow_mut().clear();
```
to (keep bar_ranges/line_numbers/echo_lines clears above it, but NOT blocks):
```rust
        *self.line_numbers.borrow_mut() = Vec::new();
        *self.echo_lines.borrow_mut() = Vec::new();
```
(i.e. delete the `self.blocks.borrow_mut().clear();` line and the now-duplicate lines; ensure `*self.bar_ranges.borrow_mut() = Vec::new();` at line 853 stays — `mark_cursor_block` repopulates it.)

After the buffer is set and labels bolded (after line 872 `self.apply_synopsis_label_bold();`), and BEFORE `self.bar_drawing.queue_draw();` (line 873), insert:

```rust
        // Block cursor + left accent bar, exactly like the gloss overlay. Each
        // <p> paragraph (non-label) is one Explication cursor stop; j/k move the
        // bar between them (see handle_synopsis_overlay_key).
        *self.bar_x.borrow_mut() = left;
        self.rebuild_block_ranges_from(crate::ui::gloss_overlay::synopsis_blocks(synopsis));
        self.mark_cursor_block();
```

Note: `synopsis_blocks` is in this same module, so call it as `synopsis_blocks(synopsis)` (no path prefix). Use:

```rust
        *self.bar_x.borrow_mut() = left;
        self.rebuild_block_ranges_from(synopsis_blocks(synopsis));
        self.mark_cursor_block();
```

- [ ] **Step 3: Update the synopsis hint text**

Change the hint (line 876) from:
```rust
        self.hint.set_text("Esc close · j/k scroll · n/p scene · Ctrl+g glosses · A ask · U undo");
```
to:
```rust
        self.hint.set_text("Esc close · j/k block · n/p scene · ⇧Space synth · Ctrl+g glosses · A ask · U undo");
```

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no `error` lines (warnings ok). If `bar_x` field name differs, grep `bar_x` in the file and match it.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): render overlay as cursor blocks with left accent bar"
```

---

## Task 3: Wire j/k/gg/G to block navigation in the synopsis overlay

**Files:**
- Modify: `src/input/keymap.rs` — `handle_synopsis_overlay_key` (lines 965-1081); it currently lacks `key_state` (needed for the gg chord).

Context: gloss overlay nav (lines 698-705, 788-794, 805-813): `gg` is a chord checked via `key_state.borrow().chord == ChordState::PendingG`; `g` arms it with `KeyState::start_chord(key_state, ChordState::PendingG)`; `G` → `cursor_last_block()`; `j`/`k` → `cursor_next_block()`/`cursor_prev_block()`. The synopsis handler is called at keymap.rs:116 as `handle_synopsis_overlay_key(state, key_name, is_ctrl)` — no `key_state`. Add `key_state` to its signature and the call site.

- [ ] **Step 1: Add `key_state` to the synopsis handler signature**

`src/input/keymap.rs` line 965-969, change:
```rust
fn handle_synopsis_overlay_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
```
to:
```rust
fn handle_synopsis_overlay_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
```

And the call site (line 116):
```rust
            crate::app::InputMode::SynopsisOverlay => handle_synopsis_overlay_key(state, key_state, key_name, is_ctrl),
```

- [ ] **Step 2: Add the gg chord check at the top of the handler**

Immediately after the `use crate::ui::gloss_overlay::AskFocus;` line (line 970) and the `ask_open`/`ask_focus` read (lines 972-975), but BEFORE the Tab handling, insert the chord resolution (mirrors the gloss handler lines 698-705):

```rust
    // gg: jump to the first block (only when no ask card is capturing input).
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.cursor_first_block();
        }
        return true;
    }
```

- [ ] **Step 3: Replace the j/k scroll with block navigation and add g/G**

Find the j/k arms (lines 1071-1078):
```rust
        "j" => {
            state.borrow().gloss_overlay.scroll_gloss(1);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.scroll_gloss(-1);
            true
        }
        _ => true,
```
Replace with:
```rust
        "j" => {
            state.borrow().gloss_overlay.cursor_next_block();
            true
        }
        "k" => {
            state.borrow().gloss_overlay.cursor_prev_block();
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            state.borrow().gloss_overlay.cursor_last_block();
            true
        }
        _ => true,
```

(`G` arrives as key_name `"G"`; confirm against the gloss handler's `G` arm — it uses `"G"` at line ~793. If the gloss handler matches a different token for shift+g, mirror that token here.)

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines. (`ChordState`, `KeyState` are already imported in this file — confirmed by the gloss handler using them.)

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(synopsis): j/k move cursor block, gg/G jump first/last"
```

---

## Task 4: `synopsis_audio` cache table + find/save helpers

**Files:**
- Modify: `src/db/queries.rs` — add near `gloss_audio` (const ~637, `ensure_gloss_audio_table` ~650, `find_gloss_audio` ~701, `save_gloss_audio` ~827).
- Test: a new `#[cfg(test)]` test in `src/db/queries.rs` using an in-memory connection.

Context: `gloss_audio` uses a lazy `CREATE TABLE IF NOT EXISTS` (no `user_version` migration). Mirror that. Synopses have no `glosses` FK; key on `(work_abbrev, div1, div2, paragraph_index, voice_id)`. `open_db_rw()` exists for writes; `open_db()` for reads. `use rusqlite::OptionalExtension` is already in scope (used by `find_gloss_audio` `.optional()`).

- [ ] **Step 1: Write the failing round-trip test**

Add to `src/db/queries.rs` (in or alongside existing tests; if none, add a `#[cfg(test)] mod synopsis_audio_tests`):

```rust
#[cfg(test)]
mod synopsis_audio_tests {
    use super::*;

    #[test]
    fn synopsis_audio_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_synopsis_audio_table(&conn).unwrap();

        // Miss before save.
        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit, None);

        save_synopsis_audio(
            &conn, "KingJohn", 4, 2, 0, "/tmp/a.mp3", "voice123", "eleven_v3",
        )
        .unwrap();

        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/a.mp3"));

        // Different voice is a separate cache entry → miss.
        let other = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voiceXYZ").unwrap();
        assert_eq!(other, None);

        // Upsert updates the path in place.
        save_synopsis_audio(
            &conn, "KingJohn", 4, 2, 0, "/tmp/b.mp3", "voice123", "eleven_v3",
        )
        .unwrap();
        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/b.mp3"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib synopsis_audio_round_trip 2>&1 | tail -20`
Expected: FAIL — `cannot find function ensure_synopsis_audio_table`.

- [ ] **Step 3: Implement the table + helpers**

Add to `src/db/queries.rs` (near the gloss_audio block):

```rust
/// Column body of the synopsis_audio table (per-paragraph synopsis TTS cache).
/// Keyed by scene + paragraph + voice (synopses have no glosses FK).
const SYNOPSIS_AUDIO_COLUMNS: &str = "
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    work_abbrev     TEXT NOT NULL,
    div1            INTEGER NOT NULL,
    div2            INTEGER NOT NULL,
    paragraph_index INTEGER NOT NULL,
    audio_path      TEXT NOT NULL,
    voice_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(work_abbrev, div1, div2, paragraph_index, voice_id)
";

/// Ensure the synopsis_audio table exists (lazy CREATE, like gloss_audio — no
/// user_version migration, no SNAPSHOT bump; this is not a LineMap change).
pub fn ensure_synopsis_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS synopsis_audio ({SYNOPSIS_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_synopsis_audio_scene
             ON synopsis_audio(work_abbrev, div1, div2);"
    ))
}

/// Cached MP3 path for a synopsis paragraph in a specific voice, if any.
pub fn find_synopsis_audio(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    paragraph_index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM synopsis_audio
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3
           AND paragraph_index = ?4 AND voice_id = ?5",
        rusqlite::params![work_abbrev, div1, div2, paragraph_index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Upsert a cached synopsis-paragraph MP3 path.
pub fn save_synopsis_audio(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    paragraph_index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO synopsis_audio
            (work_abbrev, div1, div2, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(work_abbrev, div1, div2, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![work_abbrev, div1, div2, paragraph_index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib synopsis_audio_round_trip 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): synopsis_audio cache table + find/save helpers"
```

---

## Task 5: `tts_batch_running` re-entrancy guard on AppState

**Files:**
- Modify: `src/app.rs` — `AppState` struct (field near `tts` ~line 247) + the initializer (~line 1624 region where other fields default).

Context: `AppState` is a plain struct built in a big literal. `std::cell::Cell` is already used across the codebase. Add a `Cell<bool>` defaulting to `false`.

- [ ] **Step 1: Add the field**

Near the `pub tts: crate::tts::TtsPlayer,` field (line 247) add:
```rust
    /// True while a Shift+Space batch synthesis is running, so a second press is
    /// a no-op rather than launching a concurrent batch.
    pub tts_batch_running: std::cell::Cell<bool>,
```

- [ ] **Step 2: Initialize it**

In the `AppState { … }` construction literal (the block around lines 1620-1730 where fields like `settings_return_mode: InputMode::Reader,` and `input_mode: InputMode::Reader,` are set), add:
```rust
        tts_batch_running: std::cell::Cell::new(false),
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines. (If the build complains about a missing field in the literal, that confirms you put it in the struct but not the literal — add it.)

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: tts_batch_running guard flag on AppState"
```

---

## Task 6: Gloss-overlay batch synth — `synth_all_prose_blocks`

**Files:**
- Modify: `src/input/actions/gloss.rs` — add the fn + a small shared guard helper near `play_block_tts` (line 981) and the toast helpers (1283-1310).

Context: reuse `synth_via` (line 1222), `show_persistent_tts_toast`/`show_tts_toast`/`hide_tts_toast` (1283-1310), `gloss_audio_dir` (1268), `find_gloss_audio`/`save_gloss_audio`, `ipa_for_tts`, `gloss_blocks`, `BlockKind`, `voice_for`. Fixed voice: `crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false)` → `(&'static str voice_id, &'static str model_id)`. The current gloss + abbrev come from `s.gloss_list[s.gloss_index]` and `s.gloss_context` (see `play_block_tts` lines 986-996). Sequential await loop; stop on first error.

This task's network path is runtime-only; no unit test. Build-only verification here.

- [ ] **Step 1: Add the fn**

Add to `src/input/actions/gloss.rs` (e.g. after `play_block_tts`, before `play_source_tts_pausing_mpv`):

```rust
/// Shift+Space (gloss overlay): synthesize ALL prose (Explication) blocks of the
/// open gloss to cached MP3s in the fixed plain-prose voice. Cache-only (no
/// playback). Shows a persistent "Synthesizing…" toast; stops on the first
/// error and shows it. Skips blocks already cached. Re-entrant-safe via
/// AppState.tts_batch_running.
pub(crate) fn synth_all_prose_blocks(state_rc: &Rc<RefCell<AppState>>) {
    if state_rc.borrow().tts_batch_running.get() {
        return;
    }
    let (gloss_id, work_abbrev, blocks, voice_id, model_id, tokio_handle) = {
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
        let prose: Vec<(i32, String)> =
            crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text)
                .into_iter()
                .filter(|b| b.kind == BlockKind::Explication)
                .map(|b| (b.index, b.text))
                .collect();
        if prose.is_empty() {
            return;
        }
        let (vid, mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
        (gloss_id, work_abbrev, prose, vid.to_string(), mid.to_string(), s.tokio_handle.clone())
    };

    state_rc.borrow().tts_batch_running.set(true);
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        for (index, raw) in &blocks {
            // Skip if already cached for this voice.
            if let Ok(conn) = crate::db::queries::open_db() {
                if let Ok(Some(path)) = crate::db::queries::find_gloss_audio(
                    &conn, gloss_id, "explication", *index as i64, &voice_id,
                ) {
                    if std::path::Path::new(&path).exists() {
                        continue;
                    }
                }
            }
            let tts_text = crate::ui::gloss_overlay::ipa_for_tts(raw);
            let bytes = match synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await {
                Ok(b) => b,
                Err(e) => {
                    crate::log_fmt!("BATCH: gloss synth error at block {}: {}", index, e);
                    show_tts_toast(&state_for_result, &format!("Synthesis failed: {}", e));
                    state_for_result.borrow().tts_batch_running.set(false);
                    return;
                }
            };
            let dir = gloss_audio_dir(&work_abbrev, gloss_id);
            let voice_tag: String = voice_id.chars().take(12).collect();
            let path = dir.join(format!("{}-{}.mp3", index, voice_tag));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                crate::log_fmt!("BATCH: mkdir {} failed: {}", dir.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                crate::log_fmt!("BATCH: write {} failed: {}", path.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::queries::save_gloss_audio(
                    &conn, gloss_id, "explication", *index as i64,
                    &path.to_string_lossy(), &voice_id, &model_id,
                );
            }
        }
        hide_tts_toast(&state_for_result);
        state_for_result.borrow().tts_batch_running.set(false);
        crate::log_fmt!("BATCH: synthesized {} gloss prose blocks", blocks.len());
    });
}
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines. If `gloss.gloss_id` / `gloss.gloss_text` field names differ, match `play_block_tts` (it uses `gloss.gloss_id` and `gloss.gloss_text`).

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): Shift+Space batch-synthesize all prose blocks (cache-only)"
```

---

## Task 7: Synopsis-overlay batch synth — `synth_all_synopsis_blocks`

**Files:**
- Modify: `src/input/actions/gloss.rs` — add the fn + `synopsis_audio_dir`.

Context: synopsis text is `s.synopsis_cache[&s.synopsis_overlay_scene]`; abbrev is `s.current_work.as_ref().abbrev`. Blocks come from `crate::ui::gloss_overlay::synopsis_blocks(&synopsis)` (same blocks the cursor navigates, so cache index == on-screen block). Use the new `synopsis_audio` helpers and a `~/Music/synopses/<abbrev>/<div1>-<div2>/` dir. `ensure_synopsis_audio_table` must be called before find/save (lazy create).

- [ ] **Step 1: Add `synopsis_audio_dir`**

Add near `gloss_audio_dir` (line 1268):
```rust
/// `~/Music/synopses/<work-abbrev>/<div1>-<div2>/`
fn synopsis_audio_dir(work_abbrev: &str, div1: i64, div2: i64) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join("Music")
        .join("synopses")
        .join(work_abbrev)
        .join(format!("{}-{}", div1, div2))
}
```

- [ ] **Step 2: Add the batch fn**

Add after `synth_all_prose_blocks`:
```rust
/// Shift+Space (synopsis overlay): synthesize ALL synopsis paragraphs of the
/// open scene to cached MP3s in the fixed plain-prose voice. Cache-only.
/// Persistent toast; stop on first error. Re-entrant-safe via tts_batch_running.
pub(crate) fn synth_all_synopsis_blocks(state_rc: &Rc<RefCell<AppState>>) {
    if state_rc.borrow().tts_batch_running.get() {
        return;
    }
    let (work_abbrev, div1, div2, blocks, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.abbrev.clone(),
            None => return,
        };
        let prose: Vec<(i32, String)> = crate::ui::gloss_overlay::synopsis_blocks(&synopsis)
            .into_iter()
            .map(|b| (b.index, b.text))
            .collect();
        if prose.is_empty() {
            return;
        }
        let (vid, mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
        (work_abbrev, div1, div2, prose, vid.to_string(), mid.to_string(), s.tokio_handle.clone())
    };

    state_rc.borrow().tts_batch_running.set(true);
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        for (index, raw) in &blocks {
            if let Ok(conn) = crate::db::queries::open_db() {
                let _ = crate::db::queries::ensure_synopsis_audio_table(&conn);
                if let Ok(Some(path)) = crate::db::queries::find_synopsis_audio(
                    &conn, &work_abbrev, div1, div2, *index as i64, &voice_id,
                ) {
                    if std::path::Path::new(&path).exists() {
                        continue;
                    }
                }
            }
            let tts_text = crate::ui::gloss_overlay::ipa_for_tts(raw);
            let bytes = match synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await {
                Ok(b) => b,
                Err(e) => {
                    crate::log_fmt!("BATCH: synopsis synth error at para {}: {}", index, e);
                    show_tts_toast(&state_for_result, &format!("Synthesis failed: {}", e));
                    state_for_result.borrow().tts_batch_running.set(false);
                    return;
                }
            };
            let dir = synopsis_audio_dir(&work_abbrev, div1, div2);
            let voice_tag: String = voice_id.chars().take(12).collect();
            let path = dir.join(format!("{}-{}.mp3", index, voice_tag));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                crate::log_fmt!("BATCH: mkdir {} failed: {}", dir.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                crate::log_fmt!("BATCH: write {} failed: {}", path.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::queries::ensure_synopsis_audio_table(&conn);
                let _ = crate::db::queries::save_synopsis_audio(
                    &conn, &work_abbrev, div1, div2, *index as i64,
                    &path.to_string_lossy(), &voice_id, &model_id,
                );
            }
        }
        hide_tts_toast(&state_for_result);
        state_for_result.borrow().tts_batch_running.set(false);
        crate::log_fmt!("BATCH: synthesized {} synopsis paragraphs", blocks.len());
    });
}
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines. If `w.abbrev` field name differs, grep the `Work` struct in `src/db/queries.rs` (field is `abbrev` per load_work line 174).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(synopsis): Shift+Space batch-synthesize all synopsis paragraphs (cache-only)"
```

---

## Task 8: Route Shift+Space in both overlay handlers

**Files:**
- Modify: `src/input/keymap.rs` — forward `is_shift` into `handle_gloss_key` and `handle_synopsis_overlay_key`; add the Shift+Space pre-`match` dispatch in each.

Context: `handle_key` has `is_shift` (line 42). The plain-space guard at line 73 already excludes `is_shift`, so `Shift+Space` reaches mode dispatch. `handle_gloss_key` (line 660) and `handle_synopsis_overlay_key` (now `key_state, key_name, is_ctrl` from Task 3) must learn `is_shift`. GTK reports Shift+Space as `key_name == "space"`, `is_shift == true`.

- [ ] **Step 1: Add `is_shift` to `handle_gloss_key`**

Signature (lines 660-667), add `is_shift: bool` (e.g. after `is_ctrl`):
```rust
fn handle_gloss_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
```
Call site (line 115):
```rust
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_state, key_name, is_ctrl, is_shift, is_alt, tokio_handle),
```

- [ ] **Step 2: Add the gloss Shift+Space dispatch**

In `handle_gloss_key`, after the ask-card block (after line 696, before the `PendingG` chord check at line 698), insert:
```rust
    // Shift+Space: batch-synthesize all prose blocks (cache-only).
    if key_name == "space" && is_shift {
        crate::input::actions::gloss::synth_all_prose_blocks(state);
        return true;
    }
```

- [ ] **Step 3: Add `is_shift` to `handle_synopsis_overlay_key`**

Signature (from Task 3 it is `state, key_state, key_name, is_ctrl`); add `is_shift`:
```rust
fn handle_synopsis_overlay_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    is_shift: bool,
) -> bool {
```
Call site (line 116):
```rust
            crate::app::InputMode::SynopsisOverlay => handle_synopsis_overlay_key(state, key_state, key_name, is_ctrl, is_shift),
```

- [ ] **Step 4: Add the synopsis Shift+Space dispatch**

In `handle_synopsis_overlay_key`, after the gg chord check added in Task 3 (and before the Tab handling), insert:
```rust
    // Shift+Space: batch-synthesize all synopsis paragraphs (cache-only).
    if key_name == "space" && is_shift {
        crate::input::actions::gloss::synth_all_synopsis_blocks(state);
        return true;
    }
```

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines.

- [ ] **Step 6: Full test + clippy**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS (including the Task 1 and Task 4 tests).
Run: `cargo clippy 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines.

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(keymap): route Shift+Space to batch synth in gloss + synopsis overlays"
```

---

## Task 9: Ctrl+/ keybinds overlay — Shift+Space + synopsis block nav

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (cap + `describe()` arm).

Per CLAUDE.md, any keybind change must update the Ctrl+/ overlay. Do NOT hand-edit blindly.

- [ ] **Step 1: Invoke the overlay-update skill**

Use the `update-cairo-keybinds-overlay` skill. Add/representations:
- `Shift+Space` (in the gloss/synopsis overlay context) → "Synthesize all prose blocks (cache only) — `synth_all_prose_blocks` / `synth_all_synopsis_blocks`, src/input/actions/gloss.rs".
- Note the synopsis overlay's `j`/`k` now move the cursor block (and `gg`/`G` first/last), matching the gloss overlay, if the overlay distinguishes synopsis-mode descriptions.

Follow the skill's mandatory three-pass cross-reference (no blank slot hides a real binding; no label names the wrong action; every label has a `describe()` arm).

- [ ] **Step 2: Build + commit**

Run: `cargo build 2>&1 | grep -E "^error" ; echo done`
Expected: no error lines.
```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): Shift+Space synth + synopsis block-nav in Ctrl+/ overlay"
```

---

## Runtime verification (ask the user — agent does not run the app)

Per CLAUDE.md, the agent builds but does not run the app; these are rendered/network behaviors:

1. **Synopsis accent bar + block nav (rendered-pixel):** open the synopsis (`h`), confirm a left accent bar sits beside the first paragraph; `j`/`k` move it between paragraphs; `gg`/`G` jump first/last. Headless option:
   ```bash
   ./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
   ```
   (clipping suite covers the main card, not the overlay bar — the bar itself needs eyes or a screenshot via the headless-test skill).
2. **Shift+Space batch synth (network):** with a valid `ELEVENLABS_API_KEY`, press `Shift+Space` in each overlay. Confirm the "Synthesizing…" toast appears then dismisses, MP3s land under `~/Music/glosses/<abbrev>/<gloss_id>/<index>-<voice>.mp3` and `~/Music/synopses/<abbrev>/<div1>-<div2>/<index>-<voice>.mp3`, and a forced failure (e.g. unset the key) shows a "Synthesis failed: …" toast and stops the batch.
   ```bash
   ls -R ~/Music/synopses ~/Music/glosses 2>/dev/null | head
   grep BATCH ~/utono/linux-lit/linux-lit-dev.log | tail
   ```
