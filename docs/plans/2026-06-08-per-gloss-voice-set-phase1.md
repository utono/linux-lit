# Per-gloss voice set (Phase 1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a gloss carry a set of associated ElevenLabs voices (toggle membership with `v`, cycle the active one with `V`); play the active associated voice if any, else the existing gender default; cache each voice's audio separately.

**Architecture:** A new `gloss_voices(gloss_id, voice_id, model_id, position)` table holds the per-gloss set. `gloss_audio`'s UNIQUE key gains `voice_id` (migration) so each voice's block audio caches separately. `play_block_tts` resolves the voice as "active associated voice, else `voice_for(gender)`". The existing `VoicePicker` is reused via a `voice_picker_origin` flag so its confirm routes back to the gloss overlay and writes to `gloss_voices` instead of config.

**Tech Stack:** Rust (rusqlite, GTK4, `cargo test --bins` — binary-only crate; the rare parallel-test flake means use `--test-threads=1`).

**Spec:** `docs/superpowers/specs/2026-06-08-per-gloss-voice-set-design.md` (Phase 1 only; Phase 2 = age-aware default is a separate plan).

---

## Task 1: `gloss_voices` table + helpers

**Files:**
- Modify: `src/db/queries.rs` (add `ensure_gloss_voices_table`, `get_gloss_voices`, `toggle_gloss_voice`; tests)
- Modify: `src/app.rs` (wire `ensure_gloss_voices_table` into `BOOKMARKS_INIT.call_once`, ~line 2455)

- [ ] **Step 1: Write failing tests** — add a test module at the bottom of `src/db/queries.rs` (or into the existing `mod character_tests`/`mod tests`; whichever has `use super::*;`):

```rust
    #[test]
    fn gloss_voices_toggle_add_remove_and_order() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_gloss_voices_table(&conn).unwrap();
        // add two voices -> both present, in insertion order
        assert!(toggle_gloss_voice(&conn, 1, "vA", "m1"));   // true = added
        assert!(toggle_gloss_voice(&conn, 1, "vB", "m2"));
        assert_eq!(
            get_gloss_voices(&conn, 1),
            vec![("vA".to_string(), "m1".to_string()), ("vB".to_string(), "m2".to_string())]
        );
        // toggling vA again removes it
        assert!(!toggle_gloss_voice(&conn, 1, "vA", "m1"));  // false = removed
        assert_eq!(get_gloss_voices(&conn, 1), vec![("vB".to_string(), "m2".to_string())]);
        // a different gloss has its own (empty) set
        assert!(get_gloss_voices(&conn, 2).is_empty());
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins gloss_voices_toggle`
Expected: FAIL — `cannot find function ensure_gloss_voices_table`.

- [ ] **Step 3: Implement the table + helpers** — add to `src/db/queries.rs` after `ensure_characters_table` (~line 502):

```rust
/// Ensure the per-gloss voice-set table exists. A gloss can be associated with
/// zero, one, or more voices; `position` gives a stable cycle order. Rows are
/// added/removed via `toggle_gloss_voice`. See the per-gloss-voice-set spec.
pub fn ensure_gloss_voices_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gloss_voices (
            gloss_id  INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
            voice_id  TEXT NOT NULL,
            model_id  TEXT NOT NULL,
            position  INTEGER NOT NULL,
            PRIMARY KEY (gloss_id, voice_id)
        );"
    )?;
    Ok(())
}

/// The voices associated with a gloss, ordered by `position` (cycle order).
pub fn get_gloss_voices(conn: &Connection, gloss_id: i64) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT voice_id, model_id FROM gloss_voices WHERE gloss_id = ?1 ORDER BY position",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![gloss_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}

/// Toggle a voice's membership in a gloss's set. Returns `true` if it was ADDED
/// (appended at the next position), `false` if it was REMOVED.
pub fn toggle_gloss_voice(
    conn: &Connection,
    gloss_id: i64,
    voice_id: &str,
    model_id: &str,
) -> bool {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM gloss_voices WHERE gloss_id = ?1 AND voice_id = ?2",
            rusqlite::params![gloss_id, voice_id],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        let _ = conn.execute(
            "DELETE FROM gloss_voices WHERE gloss_id = ?1 AND voice_id = ?2",
            rusqlite::params![gloss_id, voice_id],
        );
        false
    } else {
        let next_pos: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM gloss_voices WHERE gloss_id = ?1",
                rusqlite::params![gloss_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "INSERT INTO gloss_voices (gloss_id, voice_id, model_id, position) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![gloss_id, voice_id, model_id, next_pos],
        );
        true
    }
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins gloss_voices_toggle`
Expected: PASS.

- [ ] **Step 5: Wire into startup** — in `src/app.rs`, the `BOOKMARKS_INIT.call_once` block (~line 2455), add after the `ensure_characters_table` line:

```rust
            let _ = crate::db::queries::ensure_gloss_voices_table(&conn);
```

- [ ] **Step 6: Build + full tests**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: clean; all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/db/queries.rs src/app.rs
git commit -m "feat(db): gloss_voices table + get_gloss_voices / toggle_gloss_voice"
```

---

## Task 2: `gloss_audio` per-voice cache (migration + signatures)

**Files:**
- Modify: `src/db/queries.rs` (`GLOSS_AUDIO_COLUMNS`, `ensure_gloss_audio_table` migration, `find_gloss_audio` + `save_gloss_audio` signatures; tests)

The cache must hold a distinct row per voice for the same block. Add `voice_id` to the UNIQUE key. The existing migration detector (`pragma_table_info … 'kind'`) won't work because `voice_id` already exists as a column — detect the OLD UNIQUE shape via `sqlite_master.sql`.

- [ ] **Step 1: Write failing test** — add to the queries test module:

```rust
    #[test]
    fn gloss_audio_caches_per_voice() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        // two voices for the SAME (gloss, kind, index) coexist as separate rows
        save_gloss_audio(&conn, 1, "source", 0, "/a.mp3", "vA", "m1").unwrap();
        save_gloss_audio(&conn, 1, "source", 0, "/b.mp3", "vB", "m2").unwrap();
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vA").unwrap(), Some("/a.mp3".into()));
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vB").unwrap(), Some("/b.mp3".into()));
        // re-saving the same (gloss,kind,index,voice) overwrites just that one
        save_gloss_audio(&conn, 1, "source", 0, "/a2.mp3", "vA", "m1").unwrap();
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vA").unwrap(), Some("/a2.mp3".into()));
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vB").unwrap(), Some("/b.mp3".into()));
        // a voice with no cached row -> None
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vZ").unwrap(), None);
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins gloss_audio_caches_per_voice`
Expected: FAIL — `find_gloss_audio` takes 4 args not 5 (arity mismatch / wrong signature).

- [ ] **Step 3: Update `GLOSS_AUDIO_COLUMNS`** — change the UNIQUE line (in `src/db/queries.rs` ~line 514):

```rust
    UNIQUE(gloss_id, kind, paragraph_index, voice_id)
```

- [ ] **Step 4: Update the migration in `ensure_gloss_audio_table`** — replace the legacy-rebuild block. The existing fn first does the `IF NOT EXISTS` create (leave that), then the `kind`-column migration (leave that — old installs need it first). ADD a SECOND migration that detects the OLD unique shape and rebuilds. Replace the function body with:

```rust
pub fn ensure_gloss_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Fresh installs get the new shape directly.
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS gloss_audio ({GLOSS_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);"
    ))?;

    // Legacy migration 1: a table with no `kind` column.
    let has_kind: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('gloss_audio') WHERE name = 'kind'")?
        .exists([])?;
    if !has_kind {
        conn.execute_batch(&format!(
            "BEGIN;
             ALTER TABLE gloss_audio RENAME TO gloss_audio_old;
             CREATE TABLE gloss_audio ({GLOSS_AUDIO_COLUMNS});
             INSERT INTO gloss_audio (id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp)
                SELECT id, gloss_id, 'explication', paragraph_index, audio_path, voice_id, model_id, timestamp
                FROM gloss_audio_old;
             DROP TABLE gloss_audio_old;
             CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
             COMMIT;"
        ))?;
    }

    // Legacy migration 2: the UNIQUE key omits voice_id (pre per-voice cache).
    // Detect by the table's stored DDL still naming the 3-column UNIQUE.
    let old_unique: bool = conn
        .prepare(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='gloss_audio' \
             AND sql LIKE '%UNIQUE(gloss_id, kind, paragraph_index)%' \
             AND sql NOT LIKE '%voice_id)%'",
        )?
        .exists([])?;
    if old_unique {
        conn.execute_batch(&format!(
            "BEGIN;
             ALTER TABLE gloss_audio RENAME TO gloss_audio_old;
             CREATE TABLE gloss_audio ({GLOSS_AUDIO_COLUMNS});
             INSERT INTO gloss_audio (id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp)
                SELECT id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp
                FROM gloss_audio_old;
             DROP TABLE gloss_audio_old;
             CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
             COMMIT;"
        ))?;
    }
    Ok(())
}
```

(The `sql LIKE '%UNIQUE(gloss_id, kind, paragraph_index)%' AND sql NOT LIKE '%voice_id)%'` distinguishes the old 3-column UNIQUE from the new 4-column one, since the new DDL renders `…paragraph_index, voice_id)`.)

- [ ] **Step 5: Update `find_gloss_audio` + `save_gloss_audio`** — add `voice_id` to both. Replace both functions:

```rust
/// Return the cached audio path for a gloss block in a SPECIFIC voice, if any.
pub fn find_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM gloss_audio
         WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3 AND voice_id = ?4",
        rusqlite::params![gloss_id, kind, index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Insert or replace the audio path for a gloss block in a specific voice.
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
         ON CONFLICT(gloss_id, kind, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![gloss_id, kind, index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}
```

- [ ] **Step 6: Run the new test, verify PASS**

Run: `cargo test --bins gloss_audio_caches_per_voice`
Expected: PASS.

- [ ] **Step 7: Fix the now-broken caller.** Adding `voice_id` to `find_gloss_audio` changes its arity, so `src/input/actions/gloss.rs::play_block_tts` won't compile. Make it compile by threading the captured voice through (Task 3 rewrites this same call site, but it must build now):
  - **Lookup** (~line 675): add `&voice_id` as the new 5th arg:
    `find_gloss_audio(&conn, gloss_id, kind_str, index as i64, &voice_id)`
  - **Save** (~line 734): `save_gloss_audio`'s signature is unchanged (voice_id/model_id were already its last two args), so the existing call
    `save_gloss_audio(&conn, gloss_id, kind_str, index as i64, &path.to_string_lossy(), &used_voice, &used_model)`
    already compiles — no edit needed.

- [ ] **Step 8: Build + full tests**

Run: `cargo build 2>&1 | rg "^error" || echo OK` then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"`
Expected: OK; all PASS.

- [ ] **Step 9: Commit**

```bash
git add src/db/queries.rs src/input/actions/gloss.rs
git commit -m "feat(db): per-voice gloss_audio cache (voice_id in UNIQUE key + lookup) + migration"
```

---

## Task 3: Per-voice filename + active-voice resolution in `play_block_tts`

**Files:**
- Modify: `src/input/actions/gloss.rs` (`play_block_tts` voice resolution + filename stem)
- Modify: `src/app.rs` (add `gloss_active_voice: usize` field + initializer)

This makes the cached FILE per-voice (so two voices' files don't collide) and resolves the play voice as "active associated voice, else gender default".

- [ ] **Step 1: Add the `gloss_active_voice` field.** In `src/app.rs`, near the gloss fields (~line 245, after `gloss_picker_from_overlay`), add the declaration:

```rust
    /// Index into the current gloss's associated voice set (gloss_voices,
    /// position order) — which voice plays next. Session-only; reset to 0 on
    /// gloss change. With no associated voices, the gender default is used.
    pub gloss_active_voice: usize,
```

And the initializer (~line 1607, after `gloss_picker_from_overlay: false,`):

```rust
        gloss_active_voice: 0,
```

- [ ] **Step 2: Build (field added, unused)**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK` (a dead_code warning on `gloss_active_voice` is fine until Step 3 reads it).

- [ ] **Step 3: Resolve the voice from the associated set, else gender.** In `play_block_tts`, replace the gender-only voice block (the `let gender = …; let is_verse = …; let (vid, mid) = voice_for(…)` section) with this — it reads the gloss's associated voices first:

```rust
        let is_verse = kind == BlockKind::Source;
        // Per-gloss voice override: if the gloss has associated voices, play the
        // active one (gloss_active_voice index, clamped). Else fall back to the
        // character-gender default (verse->OP, prose->plain).
        let (vid, mid): (String, String) = match crate::db::queries::open_db() {
            Ok(conn) => {
                let voices = crate::db::queries::get_gloss_voices(&conn, gloss_id);
                if !voices.is_empty() {
                    let i = s.gloss_active_voice.min(voices.len() - 1);
                    (voices[i].0.clone(), voices[i].1.clone())
                } else {
                    let gender =
                        crate::db::queries::get_character_gender(&conn, &work_abbrev, &speaker);
                    let (v, m) = crate::elevenlabs::voice_for(gender, is_verse);
                    (v.to_string(), m.to_string())
                }
            }
            Err(_) => {
                let (v, m) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, is_verse);
                (v.to_string(), m.to_string())
            }
        };
        crate::log_fmt!(
            "TTS: voice -> {} (gloss {}, {})",
            vid, gloss_id, if is_verse { "verse" } else { "prose" }
        );
        (
            gloss_id,
            work_abbrev,
            text,
            vid,
            mid,
            s.tokio_handle.clone(),
        )
```

(Note `vid`/`mid` are now `String`, so the tuple uses `vid, mid` directly, not `vid.to_string()`.)

- [ ] **Step 4: Make the cached filename per-voice.** The stem currently is `source-<index>` / `<index>`, which collides across voices. Append a short, filesystem-safe slice of the voice id. Replace the stem construction (~line 666):

```rust
    // Filename stem: include a short voice tag so each voice's audio for a block
    // is a distinct file (voice ids are alphanumeric, filesystem-safe).
    let voice_tag: String = voice_id.chars().take(12).collect();
    let stem = match kind {
        BlockKind::Source => format!("source-{}-{}", index, voice_tag),
        BlockKind::Explication => format!("{}-{}", index, voice_tag),
    };
```

(`voice_id` is the captured selected voice; first 12 chars of an ElevenLabs id are unique enough per block and keep the filename short. The cache lookup already keys on the full voice_id in the DB, Task 2 — the filename only needs to avoid file collisions.)

- [ ] **Step 5: Build + tests**

Run: `cargo build 2>&1 | rg "^error" || echo OK` then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"`
Expected: `OK`; all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/gloss.rs src/app.rs
git commit -m "feat(tts): play active associated voice (else gender default); per-voice cache filename"
```

---

## Task 4: Reset `gloss_active_voice` on gloss change

**Files:**
- Modify: `src/input/actions/gloss.rs` (`navigate_gloss`, `navigate_gloss_passage`, `delete_current_gloss`)

The active-voice index is per-gloss; reset it to 0 wherever the current gloss changes.

- [ ] **Step 1: Reset in `navigate_gloss`** (Ctrl+n/p, ~line 165 where `s.gloss_index = new_idx;`). Add right after that line:

```rust
    s.gloss_active_voice = 0;
```

- [ ] **Step 2: Reset in `navigate_gloss_passage`** (Alt+n/p, ~line 151 where `s.gloss_index = 0;` after rebuilding `s.gloss_list`). Add right after the `s.gloss_index = 0;` line:

```rust
    s.gloss_active_voice = 0;
```

- [ ] **Step 3: Reset in `delete_current_gloss`** (~line 211 where `gloss_index` changes). Add after the index reassignment:

```rust
    s.gloss_active_voice = 0;
```

(If any of these sites doesn't hold a `&mut s` / `s` binding at that exact line, set it on the borrowed state the same way the surrounding code mutates `s.gloss_index` — match the local binding name.)

- [ ] **Step 4: Build + tests**

Run: `cargo build 2>&1 | rg "^error" || echo OK` then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"`
Expected: `OK`; all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(tts): reset active voice index on gloss/passage change"
```

---

## Task 5: `cycle_active_voice` + the `V` key

**Files:**
- Modify: `src/input/actions/gloss.rs` (add `cycle_active_voice`)
- Modify: `src/input/keymap.rs` (`handle_gloss_key`: add the `V` arm)

- [ ] **Step 1: Implement `cycle_active_voice`.** Add to `src/input/actions/gloss.rs` (near `play_block_tts` / the other pub(crate) gloss actions):

```rust
/// Cycle which associated voice is active for the current gloss (wraps). Toasts
/// the now-active voice id; no-op toast if the gloss has no associated voices.
/// Does NOT auto-play — the next Space uses the new active voice.
pub(crate) fn cycle_active_voice(state_rc: &Rc<RefCell<AppState>>) {
    let gloss_id = {
        let s = state_rc.borrow();
        match s.gloss_list.get(s.gloss_index) {
            Some(g) => g.gloss_id,
            None => return,
        }
    };
    let voices = match crate::db::queries::open_db() {
        Ok(conn) => crate::db::queries::get_gloss_voices(&conn, gloss_id),
        Err(_) => Vec::new(),
    };
    if voices.is_empty() {
        show_tts_toast(state_rc, "No voices associated — default in use");
        return;
    }
    let next = {
        let mut s = state_rc.borrow_mut();
        s.gloss_active_voice = (s.gloss_active_voice + 1) % voices.len();
        s.gloss_active_voice
    };
    show_tts_toast(state_rc, &format!("Voice: {}", voices[next].0));
}
```

- [ ] **Step 2: Add the `V` arm to `handle_gloss_key`** — in `src/input/keymap.rs`, the plain-key `match key_name` (~line 751), add before `_ => true`:

```rust
        "V" => {
            crate::input::actions::gloss::cycle_active_voice(state);
            true
        }
```

- [ ] **Step 3: Build + tests**

Run: `cargo build 2>&1 | rg "^error" || echo OK` then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"`
Expected: `OK`; all PASS.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs src/input/keymap.rs
git commit -m "feat(gloss): V cycles the active associated voice"
```

- [ ] **Step 5: User runtime check.** Ask the user to `cargo run`, open a gloss, press `V` → toast "No voices associated" (until Task 6 adds voices). After Task 6, `V` cycles and toasts the voice id.

---

## Task 6: `v` key → voice picker → toggle membership (picker reuse)

**Files:**
- Modify: `src/app.rs` (add `voice_picker_origin: VoicePickerOrigin` field + enum + initializer)
- Modify: `src/ui/voice_picker.rs` (associated-badge: `set_associated` + show ✓ in `populate_list`)
- Modify: `src/input/actions/settings.rs` (`open_voice_picker` takes origin; `confirm_voice_picker`/`cancel_voice_picker` branch on origin)
- Modify: `src/input/keymap.rs` (`handle_gloss_key`: `v` arm; `handle_voice_picker_key` unchanged — it calls confirm/cancel which now branch)

- [ ] **Step 1: Add the origin enum + field.** In `src/app.rs`, define the enum near `InputMode` (~line 36) and add the field. Enum:

```rust
/// Where the voice picker was opened from, so confirm/cancel route back
/// correctly and write the right target.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VoicePickerOrigin {
    Settings,
    GlossOverlay,
}
```

Field declaration (near `gloss_active_voice`, ~line 246):

```rust
    pub voice_picker_origin: VoicePickerOrigin,
```

Initializer (~line 1608):

```rust
        voice_picker_origin: VoicePickerOrigin::Settings,
```

- [ ] **Step 2: Add the associated-badge support to `VoicePicker`.** In `src/ui/voice_picker.rs`, add an `associated: Vec<String>` field to the struct (after `voices`), initialize it `Vec::new()` in `new()`, add a setter, and show a ✓ badge in `populate_list`. Struct field + setter:

```rust
    // in the struct:
    associated: Vec<String>,
```
```rust
    /// Mark which voice ids are already in the current gloss's set (✓ badge).
    pub fn set_associated(&mut self, ids: Vec<String>) {
        self.associated = ids;
        if self.is_visible() {
            let filter = self.search_entry.text().to_string();
            self.populate_list(&filter);
        }
    }
```
In `populate_list`, after the free/paid `badge` append, add an associated marker:

```rust
            if self.associated.iter().any(|id| id == &voice.voice_id) {
                let assoc = Label::new(Some("\u{2713}")); // ✓
                assoc.set_halign(Align::End);
                assoc.add_css_class("picker-item-detail");
                row_box.append(&assoc);
            }
```
(Set `associated: Vec::new()` in `new()`'s struct literal.)

- [ ] **Step 3: Make `open_voice_picker` take an origin + seed the associated set.** In `src/input/actions/settings.rs`, change `open_voice_picker` to accept origin and (for gloss origin) pass the current gloss's associated ids. Replace its signature + opening:

```rust
pub(crate) fn open_voice_picker(
    state: &Rc<RefCell<crate::app::AppState>>,
    origin: crate::app::VoicePickerOrigin,
) {
    {
        let mut s = state.borrow_mut();
        s.voice_picker_origin = origin;
        // Seed the ✓ badges with the current gloss's associated voices.
        let assoc: Vec<String> = if origin == crate::app::VoicePickerOrigin::GlossOverlay {
            s.gloss_list.get(s.gloss_index).map(|g| g.gloss_id).map_or(Vec::new(), |gid| {
                match crate::db::queries::open_db() {
                    Ok(conn) => crate::db::queries::get_gloss_voices(&conn, gid)
                        .into_iter().map(|(vid, _)| vid).collect(),
                    Err(_) => Vec::new(),
                }
            })
        } else {
            Vec::new()
        };
        s.voice_picker.set_associated(assoc);
        s.voice_picker.set_status("Loading voices\u{2026}");
        s.voice_picker.show();
    }
    state.borrow_mut().input_mode = crate::app::InputMode::VoicePicker;
    // (the async list_voices() fetch block below is UNCHANGED)
```
(Keep the rest of `open_voice_picker`'s async fetch body exactly as-is. The existing caller `OpenVoicePicker` action in keymap.rs that calls `open_voice_picker(state)` must now pass `VoicePickerOrigin::Settings` — update that call site too.)

- [ ] **Step 4: Branch confirm/cancel on origin.** Replace `confirm_voice_picker` and `cancel_voice_picker`:

```rust
pub(crate) fn confirm_voice_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let selected = state.borrow().voice_picker.selected_voice();
    let origin = state.borrow().voice_picker_origin;
    match origin {
        crate::app::VoicePickerOrigin::Settings => {
            let mut s = state.borrow_mut();
            s.voice_picker.hide();
            if let Some((voice_id, name, _free)) = selected {
                s.config.elevenlabs_voice_id = voice_id.clone();
                crate::config::save(&s.config);
                s.settings_overlay.set_voice_label(&name);
                crate::log_fmt!("VOICE: preferred voice set to {} ({})", name, voice_id);
            }
            s.input_mode = crate::app::InputMode::Settings;
        }
        crate::app::VoicePickerOrigin::GlossOverlay => {
            // Toggle the selected voice in the current gloss's set.
            let gloss_id = {
                let s = state.borrow();
                s.gloss_list.get(s.gloss_index).map(|g| g.gloss_id)
            };
            state.borrow().voice_picker.hide();
            if let (Some((voice_id, name, _free)), Some(gid)) = (selected, gloss_id) {
                // Most voices use the default model; the four custom narration
                // voices use eleven_v3. Use eleven_v3 for any voice picked here
                // (the synth path + 402 fallback handle model compatibility).
                let model = crate::elevenlabs::OP_MODEL_ID.to_string();
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let added = crate::db::queries::toggle_gloss_voice(&conn, gid, &voice_id, &model);
                    crate::input::actions::gloss::voice_picker_toast(
                        state, if added { "Associated" } else { "Removed" }, &name,
                    );
                }
            }
            state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
        }
    }
}

pub(crate) fn cancel_voice_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let origin = state.borrow().voice_picker_origin;
    let mut s = state.borrow_mut();
    s.voice_picker.hide();
    s.input_mode = match origin {
        crate::app::VoicePickerOrigin::Settings => crate::app::InputMode::Settings,
        crate::app::VoicePickerOrigin::GlossOverlay => crate::app::InputMode::GlossOverlay,
    };
}
```

- [ ] **Step 5: Add a public toast shim in gloss.rs** (so settings.rs can toast via the gloss overlay's toast). Add to `src/input/actions/gloss.rs`:

```rust
/// Toast helper exposed for the voice-picker confirm path (settings.rs) to
/// report gloss-voice association from the gloss overlay.
pub(crate) fn voice_picker_toast(state_rc: &Rc<RefCell<AppState>>, verb: &str, name: &str) {
    show_tts_toast(state_rc, &format!("{}: {}", verb, name));
}
```

- [ ] **Step 6: Add the `v` arm to `handle_gloss_key`** — in `src/input/keymap.rs` (~line 751), before `_ => true`. Note `open_voice_picker` takes a `VoicePickerOrigin` (not an `InputMode`):

```rust
        "v" => {
            crate::input::actions::settings::open_voice_picker(
                state,
                crate::app::VoicePickerOrigin::GlossOverlay,
            );
            true
        }
```

- [ ] **Step 7: Update the existing Settings call site.** Find where `OpenVoicePicker` dispatches `open_voice_picker(state)` (in keymap.rs / the settings Voice-row handler) and change it to `open_voice_picker(state, crate::app::VoicePickerOrigin::Settings)`.

Run: `rg -n "open_voice_picker\(" src/` to find all call sites; update each.

- [ ] **Step 8: Build + tests**

Run: `cargo build 2>&1 | rg "^error" || echo OK` then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"`
Expected: `OK`; all PASS.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs src/ui/voice_picker.rs src/input/actions/settings.rs src/input/actions/gloss.rs src/input/keymap.rs
git commit -m "feat(gloss): v opens voice picker to toggle gloss voice membership (✓ badge); origin-aware confirm"
```

- [ ] **Step 10: User runtime check.** `cargo run`, open a gloss, press `v` → picker opens; select a voice → toast "Associated: <name>", returns to gloss overlay. Press `v` again, select the same voice → "Removed". With one+ associated, `V` cycles them; Space plays the active voice (and the settings Ctrl+, voice picker still works → "preferred voice set").

---

## Task 7: Ctrl+/ keybinds overlay — document `v`/`V`

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the gloss-overlay key list + `describe()` arms)

Per CLAUDE.md, any new keybind must be reflected in the Ctrl+/ overlay. `v`/`V` are gloss-overlay binds.

- [ ] **Step 1: Use the skill.** Read and follow `update-cairo-keybinds-overlay` (it carries the mandatory exhaustive cross-reference). Add `v` ("voice: add/remove") and `V` ("voice: cycle active") to the appropriate row/detail in `keybinds_overlay.rs`, with `describe()` arms:
  - `v` → "Open the voice picker to add/remove a voice for this gloss. -> open_voice_picker(GlossOverlay) — src/input/actions/settings.rs"
  - `V` → "Cycle which associated voice plays for this gloss. -> cycle_active_voice — src/input/actions/gloss.rs"

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 3: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): v/V gloss voice add-remove + cycle in the Ctrl+/ overlay"
```

- [ ] **Step 4: User visual check.** `cargo run`, open the gloss overlay, press Ctrl+/ → confirm `v`/`V` appear with correct descriptions.

---

## Self-review notes

- **Spec coverage:** §1.1 gloss_voices → Task 1; §1.1 gloss_audio migration → Task 2; §1.2 helpers → Task 1; §1.3 active state → Task 3 (field) + Task 4 (reset); §1.4 playback resolution → Task 3; §1.5 picker reuse + v/V keys → Tasks 5,6,7; §1.6 testability → Tasks 1,2 (DB unit tests) + user checks (Tasks 5,6,7). Phase 2 (age) explicitly out of scope.
- **Out of scope (correct):** voice_catalog, characters.age, resolve_default_voice, curate_characters.py — all Phase 2 (separate plan). The default branch here stays `voice_for(gender)`.
- **Type consistency:** `get_gloss_voices(&Connection, i64) -> Vec<(String,String)>` (Tasks 1,3,5,6). `toggle_gloss_voice(&Connection, i64, &str, &str) -> bool` (Tasks 1,6). `find_gloss_audio(…, voice_id: &str)` / `save_gloss_audio` 7-arg (Task 2,3). `VoicePickerOrigin { Settings, GlossOverlay }` (Tasks 6). `gloss_active_voice: usize` (Tasks 3,4,5). `cycle_active_voice(&Rc<RefCell<AppState>>)` (Tasks 5). `voice_picker_toast(state, verb, name)` (Task 6).
- **Migration risk (the key subtlety):** the gloss_audio UNIQUE change detects the OLD shape via `sqlite_master.sql LIKE` (NOT pragma_table_info, since voice_id already exists). On the dev DB, the first run rebuilds; old rows keep their voice_id as the new key component (no loss). Verify on a real DB run that the table ends with the 4-col UNIQUE.
- **Model id for picked voices (Task 6):** an arbitrary picked voice is associated with model `eleven_v3` (OP_MODEL_ID). Most voices accept it; if a voice 402s, the existing Alice fallback covers it. (Phase 2's voice_catalog could store per-voice models; out of scope here.)
- **RPD note:** `v`/`V` — verify the physical key + that `V` arrives as key_name "V" (the `A`/`G` arms confirm uppercase arrives capitalized). Check `~/utono/rpd` if `v` misbehaves.
