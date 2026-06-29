# Echo / Source Playback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the echoes overlay, bind `a` to play the selected echo's audio without opening its work (pause/resume toggle), and `Tab` to reload the source-turn media and re-arm the source AB-loop from the turn's first line.

**Architecture:** Add a `line_start_time` DB query and an `echo_playing_link: Option<i64>` AppState field. Add `play_selected_echo` (the `a` action) and rewrite `toggle_echo_playback` into `play_source_turn` (the `Tab` action) in `src/input/actions/echoes.rs`. Wire both into `handle_echoes_overlay_key`. The reader display never changes; only MPV playback does.

**Tech Stack:** Rust, GTK4, rusqlite (SQLite), MPV via IPC command channel (`cmd_tx` → `MpvCommand`).

---

## Reference facts (verified in source)

- `handle_echoes_overlay_key` (`src/input/keymap.rs`): `Tab` arm calls `toggle_echo_playback(state)`; `Escape` arm hides/clears and returns to Reader; `n`/`p` move selection; `a` is unbound. The fn's 4th param is `_is_ctrl: bool`.
- `MpvCommand` (`src/mpv/commands.rs`): `LoadFileAndSeek(String, f64)`, `LoadFileSeekPaused(String, f64)`, `SetAbLoop { a: f64, b: f64 }`, `ClearAbLoop`, `Seek(f64)`, `TogglePause`.
- MPV client (`src/mpv/client.rs:148-184`): `SetAbLoop` sets `ab-loop-a`/`ab-loop-b` **and** issues an absolute seek to `a`. `LoadFileAndSeek` issues `loadfile … replace` and defers the seek (resume=true) to `pending_seek_after_load`, firing on `file-loaded`.
- `StoredEchoLink` (`src/db/queries.rs:1052`): `link_id: i64`, `echo_work_abbrev: String`, `echo_div1: i64`, `echo_div2: i64`, `echo_start_line: i64`, `echo_text: String`, `similarity`, `curated`, `rank`. No timestamp.
- `line_id_for_location(conn, work_abbrev, div1, div2, line_in_div) -> Option<i64>` (`queries.rs:1195`).
- `list_media_for_work(conn, abbrev) -> Result<Vec<MediaItem>, _>` (`queries.rs:391`). `MediaItem { media_id: i64, path: String, display_name: Option<String>, priority: i64 }` (`src/db/models.rs:80`).
- `line_timestamps` columns include `line_mapping_id`, `media_id`, `start_time` (`queries.rs:549`).
- `navigation::SEEK_PREROLL = 0.2` (`src/input/navigation.rs:56`). `TURN_PREROLL = 0.5` const in `echoes.rs`.
- AppState fields: `ab_repeat: AbRepeatState` (fields `a_time`, `b_time`, `loop_active`), `mpv_playing: bool`, `suppress_sync_until: Option<Instant>`, `cmd_tx`, `current_work: Option<Work>`, `echo_session: Option<EchoSession>`, `echo_overlay_links: Vec<StoredEchoLink>`, `echo_overlay_index: usize`, `gloss_overlay`. Struct decl near `src/app.rs:237`; constructor inits near `src/app.rs:1035`.
- Arkangel media pick pattern (`switch_mpv_to_current_line`, `echoes.rs:998`): `current_work.media_paths.iter().zip(media_ids).find(|(p,_)| p.contains("/aax-Arkangel/"))`.
- `current_work` during the overlay is the **source work** (overlay opens from the displayed work; no `display_work` runs).

---

## File Structure

- **Modify** `src/db/queries.rs` — add `line_start_time` query + an in-memory unit test.
- **Modify** `src/app.rs` — add `echo_playing_link: Option<i64>` field + constructor init.
- **Modify** `src/input/actions/echoes.rs` — add `play_selected_echo`; rewrite `toggle_echo_playback` → `play_source_turn`.
- **Modify** `src/input/keymap.rs` — add `"a"` arm; point `"Tab"` at `play_source_turn`; reset `echo_playing_link` in `Escape`.
- **Modify** `src/ui/gloss_overlay.rs` — update the echoes footer hint string.

---

## Task 1: `line_start_time` DB query (TDD)

**Files:**
- Modify: `src/db/queries.rs`
- Test: `src/db/queries.rs` (in the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add inside `src/db/queries.rs`'s `#[cfg(test)] mod tests { … }` (the module exists near the bottom of the file):

```rust
#[test]
fn line_start_time_reads_stored_value() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE line_timestamps (
            line_mapping_id INTEGER, media_id INTEGER, start_time REAL
         );
         INSERT INTO line_timestamps (line_mapping_id, media_id, start_time)
            VALUES (42, 7, 123.5);",
    )
    .unwrap();
    assert_eq!(line_start_time(&conn, 42, 7), Some(123.5));
    // Wrong media or missing line -> None.
    assert_eq!(line_start_time(&conn, 42, 99), None);
    assert_eq!(line_start_time(&conn, 1, 7), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test line_start_time 2>&1 | tail -15`
Expected: FAIL — `cannot find function line_start_time`.

- [ ] **Step 3: Write the query**

Add this `pub fn` to `src/db/queries.rs` (place it next to `line_id_for_location`, around line 1195):

```rust
/// Look up a single line's start time for a given media file. Returns None when
/// no timestamp row exists for that (line, media) pair.
pub fn line_start_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64> {
    conn.query_row(
        "SELECT start_time FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_id, media_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test line_start_time 2>&1 | tail -15`
Expected: PASS — 1 test. (A `dead_code` warning on `line_start_time` is expected until Task 3; do not suppress it.)

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add line_start_time query for single-line timestamp lookup

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `echo_playing_link` AppState field

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the struct field**

In `src/app.rs`, find the field declaration `pub mpv_playing: bool,` (around line 237) and add a new field immediately after it:

```rust
    pub mpv_playing: bool,
    /// link_id of the echo currently playing via `a` in the echoes overlay,
    /// for the pause/resume toggle. None when no echo is being auditioned.
    pub echo_playing_link: Option<i64>,
```

- [ ] **Step 2: Add the constructor init**

In `src/app.rs`, find the constructor initializer `mpv_playing: false,` (around line 1035) and add immediately after it:

```rust
        mpv_playing: false,
        echo_playing_link: None,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean (a `dead_code`/`field never read` note on `echo_playing_link` is acceptable until Task 3).

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "Add echo_playing_link AppState field for echo play/pause toggle

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `play_selected_echo` (`a` action)

**Files:**
- Modify: `src/input/actions/echoes.rs`

Add a new `pub(crate) fn play_selected_echo`. Play the selected echo's Arkangel media at the echo line's start; if the same echo is already playing, toggle pause. Do NOT touch the displayed work or the remembered source-turn loop range.

- [ ] **Step 1: Add the function**

Add to `src/input/actions/echoes.rs` (place it immediately after `jump_to_selected_echo`):

```rust
/// `a` in the echoes overlay: play the selected echo's media in the existing
/// MPV instance without opening its work. Re-pressing `a` on the same echo
/// toggles pause/resume. The source-turn loop range is preserved so `Tab` can
/// restore it; the reader display is untouched.
pub(crate) fn play_selected_echo(
    state_rc: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    let link = {
        let s = state_rc.borrow();
        match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => l.clone(),
            None => return,
        }
    };

    // Pause/resume toggle when re-pressing `a` on the echo already playing.
    if state_rc.borrow().echo_playing_link == Some(link.link_id) {
        let _ = state_rc.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
        crate::logging::log("ECHOES: toggled echo playback");
        return;
    }

    // Resolve the echo line, its Arkangel media, and its start time.
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    let line_id = match crate::db::queries::line_id_for_location(
        &conn, &link.echo_work_abbrev, link.echo_div1, link.echo_div2, link.echo_start_line,
    ) {
        Some(id) => id,
        None => {
            state_rc.borrow().gloss_overlay.show("Could not locate the echoed line.", "");
            crate::logging::log("ECHOES: could not resolve echo line for playback");
            return;
        }
    };
    let media = match crate::db::queries::list_media_for_work(&conn, &link.echo_work_abbrev) {
        Ok(items) if !items.is_empty() => {
            // Prefer Arkangel; fall back to the highest-priority media (first).
            items.iter().find(|m| m.path.contains("/aax-Arkangel/"))
                .cloned()
                .unwrap_or_else(|| items[0].clone())
        }
        _ => {
            state_rc.borrow().gloss_overlay.show("No media for this echo's work.", "");
            crate::logging::log("ECHOES: no media for echo work");
            return;
        }
    };
    let start = match crate::db::queries::line_start_time(&conn, line_id, media.media_id) {
        Some(t) => t,
        None => {
            state_rc.borrow().gloss_overlay.show("No timestamp for the echoed line.", "");
            crate::logging::log("ECHOES: no timestamp for echo line");
            return;
        }
    };
    let seek = (start - crate::input::navigation::SEEK_PREROLL).max(0.0);

    let mut s = state_rc.borrow_mut();
    // Don't loop the source turn while auditioning the echo; keep the remembered
    // (a_time, b_time) so `Tab` can re-arm it.
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileAndSeek(media.path.clone(), seek));
    s.ab_repeat.loop_active = false;
    s.echo_playing_link = Some(link.link_id);
    s.suppress_sync_until =
        Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    crate::logging::log(&format!(
        "ECHOES: playing echo {} line_id={} @{:.1}", link.echo_work_abbrev, line_id, seek
    ));
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -15`
Expected: builds clean. The Task-1 `line_start_time` and Task-2 `echo_playing_link` warnings should now be gone (this fn uses both). A `dead_code` warning on `play_selected_echo` is expected until Task 5. (`MediaItem` and `StoredEchoLink` both already `#[derive(Clone)]`, so the `.clone()` calls compile.)

- [ ] **Step 3: Run clippy**

Run: `cargo clippy 2>&1 | rg 'echoes.rs' | head`
Expected: no new clippy warning attributable to `play_selected_echo` (ignore pre-existing warnings and the expected dead_code).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Add play_selected_echo for a-key echo audition

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Rewrite `toggle_echo_playback` → `play_source_turn` (`Tab` action)

**Files:**
- Modify: `src/input/actions/echoes.rs`

Replace `toggle_echo_playback` with `play_source_turn`. Always restore the source turn: reload the source media (because `a` may have swapped MPV to an echo's file), re-arm the AB-loop, and play from the turn's first line.

- [ ] **Step 1: Replace the function**

In `src/input/actions/echoes.rs`, replace the entire existing `pub(crate) fn toggle_echo_playback(state_rc: &Rc<RefCell<AppState>>) { … }` with:

```rust
/// `Tab` in the echoes overlay: reload the source-turn media, re-arm the source
/// AB-loop, and play from the source turn's first line. The displayed work is
/// the source work, so its Arkangel media is used.
pub(crate) fn play_source_turn(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();

    // Resolve the turn's (a, b) timestamps from the session key against the
    // currently displayed (source) work.
    let range = s.echo_session.as_ref().and_then(|sess| {
        let key = &sess.turn_key;
        let work = s.current_work.as_ref()?;
        if work.abbrev != key.work_abbrev {
            return None;
        }
        let first = work.lines.iter().find(|l| {
            l.div1 == key.div1 && l.div2 == key.div2 && l.line_in_div == key.start_line
        })?;
        let last = work.lines.iter().find(|l| {
            l.div1 == key.div1 && l.div2 == key.div2 && l.line_in_div == key.end_line
        })?;
        let a = first.timestamp.as_ref()?.start;
        let b = last.timestamp.as_ref()?.end;
        Some((a, b))
    });

    let (a, b) = match range {
        Some(r) => r,
        None => {
            // No resolvable turn range — just toggle whatever is loaded.
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
            crate::logging::log("ECHOES: toggled playback (no turn range)");
            return;
        }
    };

    // The source work's Arkangel media (fall back to first media path).
    let source_media = s.current_work.as_ref().and_then(|w| {
        w.media_paths.iter()
            .find(|p| p.contains("/aax-Arkangel/"))
            .or_else(|| w.media_paths.first())
            .cloned()
    });

    let loop_a = (a - TURN_PREROLL).max(0.0);
    // Reload the source media (a may have swapped MPV to an echo file), then set
    // the loop. LoadFileAndSeek resumes playback on file-loaded.
    if let Some(path) = source_media {
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileAndSeek(path, loop_a));
    }
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetAbLoop { a: loop_a, b });
    s.ab_repeat.a_time = Some(a);
    s.ab_repeat.b_time = Some(b);
    s.ab_repeat.loop_active = true;
    s.echo_playing_link = None;
    s.suppress_sync_until =
        Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    crate::logging::log(&format!("ECHOES: re-armed source turn loop [{:.1}, {:.1}]", loop_a, b));
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -15`
Expected: a compile ERROR at the `Tab` call site in `keymap.rs` (`cannot find function toggle_echo_playback`). That is expected and fixed in Task 5. The `echoes.rs` file itself must compile its new fn — confirm the only error is the `keymap.rs` call site, not anything inside `play_source_turn`.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Rewrite toggle_echo_playback into play_source_turn

Tab now reloads the source media and re-arms the turn loop from the
first line, rather than pausing/toggling, since a may have swapped MPV
to an echo's media.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Wire `a` / `Tab` / `Escape` in the echoes overlay handler

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Point `Tab` at `play_source_turn`**

In `src/input/keymap.rs`, in `handle_echoes_overlay_key`, the `"Tab"` arm currently reads:

```rust
        "Tab" => {
            crate::input::actions::echoes::toggle_echo_playback(state);
            true
        }
```

Change the call to:

```rust
        "Tab" => {
            crate::input::actions::echoes::play_source_turn(state);
            true
        }
```

- [ ] **Step 2: Add the `"a"` arm**

In the same `match key_name { … }`, add this arm immediately after the `"Tab"` arm:

```rust
        "a" => {
            crate::input::actions::echoes::play_selected_echo(state, tokio_handle);
            true
        }
```

- [ ] **Step 3: Reset `echo_playing_link` on Escape**

In the same handler, the `"Escape"` arm sets several fields. Add the reset inside its `borrow_mut()` block, immediately after `s.echo_overlay_turn_key = None;`:

```rust
            s.echo_overlay_turn_key = None;
            s.echo_playing_link = None;
```

- [ ] **Step 4: Verify it compiles + clippy + tests**

Run: `cargo build 2>&1 | tail -5 && cargo clippy 2>&1 | rg 'keymap.rs|echoes.rs' | head && cargo test 2>&1 | tail -6`
Expected: builds clean (no `toggle_echo_playback` error; no `dead_code` on `play_selected_echo`/`play_source_turn`/`line_start_time`/`echo_playing_link`). Clippy: no new warnings in the touched code. Tests: the Task-1 `line_start_time` test passes; the only failures are the two pre-existing `input::viewport::block_atom_tests` (`block_start_stops_at_blank`, `block_start_in_verse_stanza_bounded_above_by_stage_direction`).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Bind a (play echo) and Tab (play source turn) in echoes overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Update the echoes footer hint

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Edit the hint string**

In `src/ui/gloss_overlay.rs`, in `show_echoes`, the hint is currently:

```rust
        self.hint.set_text("Esc close · Tab loop turn · n/p select · Enter open work · c copy · s curate · R refresh");
```

Change it to:

```rust
        self.hint.set_text("Esc close · a play echo · Tab play turn · n/p select · Enter open work · c copy · s curate · R refresh");
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -3`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Update echoes footer hint for a/Tab playback binds

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Manual verification (user runs the app)

The MPV/GTK behavior cannot be exercised in `cargo test`. The author must NOT run `cargo run` (project rule). Hand these to the user.

- [ ] **Step 1: Build for the user**

Run: `cargo build 2>&1 | tail -3`
Expected: clean build.

- [ ] **Step 2: User reproduction script**

Ask the user to `cargo run`, open a work, make a selection, press `i` (or be on a turn) to open the echoes overlay, then:
1. `n`/`p` to select an echo; press `a`.
   - Expect: the echo's media loads and plays from the echo line; the reader still shows the source turn; the overlay stays open. Log shows `ECHOES: playing echo …`.
2. Press `a` again on the same echo → pauses (log `toggled echo playback`); `a` again → resumes.
3. Select a different echo, press `a` → switches to that echo's media/line.
4. Press `Tab`.
   - Expect: source-turn media reloads and plays from the turn's first line, looping the turn. Log `ECHOES: re-armed source turn loop […]`.
   - **Verify specifically:** after auditioning an echo from a *different* work via `a`, `Tab` returns audio to the source work (not the echo's). This exercises the source-media reload.
5. Press `Escape` → overlay closes (unchanged).

- [ ] **Step 3: Confirm via log**

Run: `rg 'ECHOES: (playing echo|toggled echo|re-armed)' ~/utono/linux-lit/linux-lit-dev.log | tail -15`
Expected: lines matching the actions taken above.

- [ ] **Step 4: Note the AB-loop-after-reload caveat**

Tell the user: if `Tab`'s turn loop does not engage after an echo from another work was played (i.e. it plays once but doesn't loop), it's the `SetAbLoop`-after-`loadfile` timing flagged in the spec. The fix is to route the loop set through the post-`file-loaded` deferral (mirroring `pending_seek_after_load` in `src/mpv/client.rs`). Only pursue this if the manual test shows the loop failing.

---

## Self-Review

**Spec coverage:**
- `a` plays echo without opening work, pause/resume toggle, source loop preserved → Task 3 (`play_selected_echo`: preserves `a_time`/`b_time`, only sets `loop_active=false`; toggles via `echo_playing_link`). ✓
- New `line_start_time` query → Task 1 (with unit test). ✓
- `echo_playing_link` AppState field → Task 2. ✓
- `Tab` reloads source media + re-arms loop + plays from first line → Task 4 (`play_source_turn` with `LoadFileAndSeek` + `SetAbLoop`). ✓
- `Escape` unchanged + reset `echo_playing_link` → Task 5 Step 3. ✓
- Wiring (`a` arm, `Tab` repointed) → Task 5. ✓
- Footer hint → Task 6. ✓
- Error handling (missing line/media/timestamp → toast + return) → Task 3 (three guarded `.show(...)` paths). ✓
- Open item (SetAbLoop-after-loadfile ordering) → Task 7 Step 4 manual caveat + spec-noted deferral fallback. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; commands have expected output. ✓

**Type consistency:** `line_start_time(&Connection, i64, i64) -> Option<f64>` used identically in Task 1 (test + impl) and Task 3 (caller). `echo_playing_link: Option<i64>` declared (Task 2) and used as `Some(link.link_id)` / `None` (Tasks 3, 4, 5) — `link_id` is `i64` per `StoredEchoLink`. `play_selected_echo(state, tokio_handle)` and `play_source_turn(state)` signatures match their Task-5 call sites. `MediaItem` fields (`media_id`, `path`) match `src/db/models.rs:80`. ✓

**Note:** Verified — both `MediaItem` (`src/db/models.rs:79`) and `StoredEchoLink` (`queries.rs:1051`) already `#[derive(Debug, Clone)]`, so all `.clone()` calls in Task 3/4 compile with no out-of-file changes.
