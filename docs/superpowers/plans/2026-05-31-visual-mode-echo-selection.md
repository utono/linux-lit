# Visual-mode Echo Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bind `i` in Visual mode to show cross-work echoes for the selected range (one or more speaker turns), mirroring the Reader-mode `i` echoes overlay.

**Architecture:** Add `show_echoes_for_selection` to `src/input/actions/echoes.rs` as a sibling of `show_echoes_for_cursor_line`. Both build a `(turn_lines, speaker, EchoTurnKey)` then share the existing cache-hit / live-embed-and-persist tail (`persist_and_load`, `render_echoes`, `EchoSession`). The selection's turn lines come from the Visual range instead of `cursor_turn`. Wire it into `handle_visual_key`, threading `tokio_handle` through (which that handler does not currently receive).

**Tech Stack:** Rust, GTK4 (gtk4 / libadwaita), rusqlite (SQLite), Voyage AI embeddings via `crate::voyage::embed_query`, Tokio runtime + `glib::spawn_future_local`.

---

## File Structure

- **Modify** `src/input/actions/echoes.rs`
  - Add pure helper `selection_turn_lines(work, start_wi, end_wi) -> Vec<Line>` (testable, no GTK/DB).
  - Add `selection_key(...) -> EchoTurnKey` builder for an ad-hoc multi-turn selection.
  - Add `pub(crate) fn show_echoes_for_selection(state_rc, tokio_handle)`.
  - Add a `#[cfg(test)]` module testing `selection_turn_lines` and `selection_key`.
- **Modify** `src/input/keymap.rs`
  - Change `handle_visual_key` signature to accept `tokio_handle: &tokio::runtime::Handle`.
  - Update its call site (line ~80) to pass `tokio_handle`.
  - Add an `"i"` arm dispatching `show_echoes_for_selection`.

No new files. No schema changes (existing `echo_turns` / `echo_links` tables and `save_echo_turn` / `insert_echo_links` already support arbitrary `(start_line, end_line)` keys).

---

## Reference facts (verified in source)

- `Line` fields (`src/db/models.rs:19`): `id: i64`, `text: String`, `speaker: Option<String>`, `div1: i64`, `div2: i64`, `line_in_div: i64`.
- `EchoTurnKey` (`src/db/queries.rs:1041`): `{ work_abbrev: String, div1: i64, div2: i64, start_line: i64, end_line: i64, speaker: String, turn_text: String }`. `start_line`/`end_line` are `line_in_div` values (see `show_echoes_for_cursor_line` key build, `echoes.rs:119-127`).
- `AppState::work_line_for_buffer(buffer_line: usize) -> Option<usize>` maps a buffer line to a work-line index (used at `echoes.rs:34` and `visual.rs:529`).
- `SelectionState::range(&self) -> (usize, usize)` returns `(start, end)` buffer lines, start ≤ end (`src/input/visual.rs:21-22`).
- `state.visual_selection: Option<SelectionState>` (`src/app.rs:163`).
- Shared tail helpers in `echoes.rs`: `build_source_header(turn, speaker)`, `persist_and_load(key, candidates) -> (Option<i64>, Vec<StoredEchoLink>)`, `render_echoes(&mut s)`, `first_sentence(passage)`, `EchoSession`.
- `find_similar_passages(conn, &embedding, query_text, exclude_work, top_n, affect_weight)` (`src/db/queries.rs:962`).
- `crate::voyage::embed_query(text).await -> Result<Vec<f32>, VoyageError>` (`src/voyage.rs:28`).
- `handle_action_popup_key` already takes `tokio_handle` and forwards it to `execute_action` (`keymap.rs:919`, `:957`) — the pattern to copy for `handle_visual_key`.

---

## Task 1: Pure selection→turn-lines + key helpers (TDD)

**Files:**
- Modify: `src/input/actions/echoes.rs`
- Test: `src/input/actions/echoes.rs` (inline `#[cfg(test)]` module)

`selection_turn_lines` clips the Visual range to valid work-line indices and returns the cloned `Line`s. `selection_key` builds the `EchoTurnKey` from the first/last line of that slice. These are pure (take `&[Line]`), so they're unit-testable without GTK, DB, or network.

- [ ] **Step 1: Write the failing test**

Add at the bottom of `src/input/actions/echoes.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    fn line(id: i64, speaker: Option<&str>, div1: i64, div2: i64, line_in_div: i64, text: &str) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: String::new(),
            speaker: speaker.map(|s| s.to_string()),
            is_dialogue: true,
            timestamp: None,
            div1,
            div2,
            line_in_div,
            is_chapter: false,
            is_spoken: None,
        }
    }

    fn sample_work_lines() -> Vec<Line> {
        vec![
            line(10, Some("HAMLET"), 1, 2, 1, "To be, or not to be"),
            line(11, Some("HAMLET"), 1, 2, 2, "that is the question"),
            line(12, Some("OPHELIA"), 1, 2, 3, "Good my lord"),
            line(13, Some("OPHELIA"), 1, 2, 4, "How does your honour"),
            line(14, Some("HAMLET"), 1, 2, 5, "I humbly thank you"),
        ]
    }

    #[test]
    fn selection_turn_lines_clips_and_collects_range() {
        let work = sample_work_lines();
        // Select work-index 1..=3 (the second Hamlet line through second Ophelia line).
        let got = selection_turn_lines(&work, 1, 3);
        let ids: Vec<i64> = got.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![11, 12, 13]);
    }

    #[test]
    fn selection_turn_lines_clamps_end_past_bounds() {
        let work = sample_work_lines();
        let got = selection_turn_lines(&work, 3, 999);
        let ids: Vec<i64> = got.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![13, 14]);
    }

    #[test]
    fn selection_turn_lines_empty_when_start_past_end_of_work() {
        let work = sample_work_lines();
        assert!(selection_turn_lines(&work, 99, 100).is_empty());
    }

    #[test]
    fn selection_key_uses_first_and_last_line_div_and_line_in_div() {
        let work = sample_work_lines();
        let turn = selection_turn_lines(&work, 1, 3);
        let key = selection_key("HAM", &turn);
        assert_eq!(key.work_abbrev, "HAM");
        assert_eq!(key.div1, 1);
        assert_eq!(key.div2, 2);
        assert_eq!(key.start_line, 2); // line_in_div of id=11
        assert_eq!(key.end_line, 4); // line_in_div of id=13
        // Multi-speaker selection: speaker label is the first line's speaker.
        assert_eq!(key.speaker, "HAMLET");
        assert_eq!(key.turn_text, "that is the question Good my lord How does your honour");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib selection_turn_lines 2>&1 | tail -20`
Expected: FAIL — compile error `cannot find function selection_turn_lines` / `selection_key`.

- [ ] **Step 3: Write minimal implementation**

Add these two functions to `src/input/actions/echoes.rs` (place them just above `pub(crate) fn show_echoes_for_cursor_line`, after `cursor_turn`):

```rust
/// Clip a Visual selection's work-line index range to valid bounds and return
/// the cloned lines. `start_wi`/`end_wi` are work-line indices (start <= end).
fn selection_turn_lines(work_lines: &[Line], start_wi: usize, end_wi: usize) -> Vec<Line> {
    if start_wi >= work_lines.len() {
        return Vec::new();
    }
    let end = end_wi.min(work_lines.len().saturating_sub(1));
    work_lines[start_wi..=end].to_vec()
}

/// Build an `EchoTurnKey` for an ad-hoc (possibly multi-turn, possibly
/// multi-speaker) selection. The speaker label is the first selected line's
/// speaker, falling back to "?" when absent. `turn_text` joins the selected
/// line texts with spaces, matching the cursor-turn key format.
fn selection_key(work_abbrev: &str, turn: &[Line]) -> crate::db::queries::EchoTurnKey {
    let first = turn.first().expect("selection_key called with empty turn");
    let last = turn.last().unwrap();
    let speaker = first.speaker.clone().unwrap_or_else(|| "?".to_string());
    let turn_text = turn.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
    crate::db::queries::EchoTurnKey {
        work_abbrev: work_abbrev.to_string(),
        div1: first.div1,
        div2: first.div2,
        start_line: first.line_in_div,
        end_line: last.line_in_div,
        speaker,
        turn_text,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib 'tests::selection' 2>&1 | tail -20`
Expected: PASS — 4 tests pass. (A `dead_code` warning on `selection_key`/`selection_turn_lines` is expected until Task 2 uses them; that's fine.)

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Add pure selection->turn helpers for visual-mode echoes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `show_echoes_for_selection` entry point

**Files:**
- Modify: `src/input/actions/echoes.rs`

Mirror `show_echoes_for_cursor_line` (`echoes.rs:93-270`) but source the turn from the Visual selection. The cache-hit and live-embed-and-persist branches are copied with the only differences being: turn comes from `selection_turn_lines`, key from `selection_key`, the enriched query uses the selection (no addressee inference), and Visual mode is exited before showing the overlay. This is not factored into a shared helper because the two functions differ in their setup; copying the proven tail keeps each readable in isolation.

- [ ] **Step 1: Add the function**

Add to `src/input/actions/echoes.rs`, immediately after `show_echoes_for_cursor_line`'s closing (after `echoes.rs:271`, the `});` and trailing `}` of that fn):

```rust
/// Visual-mode `i`: show echoes for the selected range (one or more speaker
/// turns). Mirrors `show_echoes_for_cursor_line` but builds its turn from the
/// Visual selection. Exits Visual mode, then lands in the EchoesOverlay state.
pub(crate) fn show_echoes_for_selection(
    state_rc: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (turn, speaker, source_work) = {
        let s = state_rc.borrow();
        let (start, end) = match &s.visual_selection {
            Some(sel) => sel.range(),
            None => {
                crate::logging::log("ECHOES: no visual selection");
                return;
            }
        };
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        // Map buffer-line range to work-line indices.
        let start_wi = match s.work_line_for_buffer(start) {
            Some(i) => i,
            None => {
                crate::logging::log("ECHOES: selection start has no work line");
                return;
            }
        };
        let end_wi = s.work_line_for_buffer(end).unwrap_or(start_wi);
        let (lo, hi) = (start_wi.min(end_wi), start_wi.max(end_wi));
        let turn = selection_turn_lines(&work.lines, lo, hi);
        if turn.is_empty() {
            crate::logging::log("ECHOES: empty selection turn");
            return;
        }
        let speaker = turn.first().and_then(|l| l.speaker.clone()).unwrap_or_else(|| "?".to_string());
        (turn, speaker, work.abbrev.clone())
    };

    // Leave Visual mode; the overlay will own the input mode below.
    crate::input::visual::exit_visual_mode(&mut state_rc.borrow_mut());

    let turn_text = turn.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
    let key = selection_key(&source_work, &turn);
    let origin_line_id = turn.first().map(|l| l.id).unwrap_or(0);

    // Cache hit: load stored links and render immediately, no API call.
    let cached = crate::db::queries::open_db().ok().and_then(|conn| {
        let turn_id = crate::db::queries::find_echo_turn(&conn, &key).ok().flatten()?;
        let links = crate::db::queries::load_echo_links(&conn, turn_id).ok()?;
        if links.is_empty() { None } else { Some((turn_id, links)) }
    });

    if let Some((turn_id, links)) = cached {
        let titles = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
            .unwrap_or_default();
        let source_doc = build_source_header(&turn, &speaker);
        let mut s = state_rc.borrow_mut();
        s.echo_overlay_source = source_doc.clone();
        s.echo_overlay_links = links.clone();
        s.echo_overlay_index = 0;
        s.echo_overlay_titles = titles.clone();
        s.echo_overlay_turn_id = Some(turn_id);
        s.echo_overlay_turn_key = Some(key.clone());
        s.echo_session = Some(EchoSession {
            turn_key: key,
            turn_id: Some(turn_id),
            links,
            selected: 0,
            titles,
            source_doc,
            origin_work: source_work.clone(),
            origin_line_id,
        });
        s.input_mode = crate::app::InputMode::EchoesOverlay;
        render_echoes(&mut s);
        crate::logging::log("ECHOES: showing cached echoes (selection)");
        return;
    }

    // Live embed. For a multi-turn selection there is no single addressee, so
    // the query is just "{speaker}: {text}".
    let query = format!("{}: {}", speaker, turn_text);
    let key_for_async = key.clone();

    let affect_weight;
    {
        let mut s = state_rc.borrow_mut();
        affect_weight = s.config.echo_affect_weight;
        s.echo_overlay_turn_key = Some(key);
        s.gloss_overlay.show_loading_message("Searching for echoes...");
        s.input_mode = crate::app::InputMode::EchoesOverlay;
    }

    let query_text = turn_text.clone();
    let state_for_result = Rc::clone(state_rc);
    let echo_handle = tokio_handle.clone();

    glib::spawn_future_local(async move {
        let embed_result = echo_handle
            .spawn(async move { crate::voyage::embed_query(&query).await })
            .await;

        let raw = match embed_result {
            Ok(Ok(embedding)) => crate::db::queries::open_db()
                .ok()
                .and_then(|conn| {
                    crate::db::queries::find_similar_passages(
                        &conn, &embedding, &query_text, &source_work, 60, affect_weight,
                    )
                    .ok()
                })
                .unwrap_or_default(),
            Ok(Err(e)) => {
                crate::logging::log(&format!("ECHOES: embed error: {}", e));
                Vec::new()
            }
            Err(e) => {
                crate::logging::log(&format!("ECHOES: embed join error: {}", e));
                Vec::new()
            }
        };

        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();
        for cand in raw {
            let dedup_key = first_sentence(&cand.passage_text).to_lowercase();
            if dedup_key.is_empty() || !seen.insert(dedup_key) {
                continue;
            }
            candidates.push(cand);
            if candidates.len() >= 15 {
                break;
            }
        }

        if candidates.is_empty() {
            let s = state_for_result.borrow();
            s.gloss_overlay.show("No echoes found for this selection.", "");
            crate::logging::log("ECHOES: no candidates (selection)");
            return;
        }

        let titles = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
            .unwrap_or_default();

        candidates.sort_by(|a, b| {
            let ta = titles.get(&a.work_abbrev).map(|s| s.as_str()).unwrap_or(a.work_abbrev.as_str());
            let tb = titles.get(&b.work_abbrev).map(|s| s.as_str()).unwrap_or(b.work_abbrev.as_str());
            ta.cmp(tb)
                .then(a.div1.cmp(&b.div1))
                .then(a.div2.cmp(&b.div2))
        });

        let (turn_id, links) = persist_and_load(&key_for_async, &candidates);

        let mut s = state_for_result.borrow_mut();
        let source_doc = build_source_header(&turn, &speaker);
        s.echo_overlay_links = links.clone();
        s.echo_overlay_index = 0;
        s.echo_overlay_titles = titles.clone();
        s.echo_overlay_source = source_doc.clone();
        s.echo_overlay_turn_id = turn_id;
        s.echo_session = Some(EchoSession {
            turn_key: key_for_async.clone(),
            turn_id,
            links,
            selected: 0,
            titles,
            source_doc,
            origin_work: source_work.clone(),
            origin_line_id,
        });
        render_echoes(&mut s);
        crate::logging::log("ECHOES: searched and cached echoes (selection)");
    });
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean. `dead_code` warnings for `show_echoes_for_selection` are expected until Task 3 wires it. Note: `exit_visual_mode` is `pub fn` in `src/input/visual.rs` (used at `visual.rs:555`); confirm the `crate::input::visual::exit_visual_mode` path resolves — if the build reports it private, change its `fn` to `pub fn`.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Add show_echoes_for_selection for visual-mode echoes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Wire `i` into Visual mode (thread tokio_handle)

**Files:**
- Modify: `src/input/keymap.rs`

`handle_visual_key` does not currently receive `tokio_handle`. Add it to the signature, pass it from the call site in `handle_key` (which has it), and add the `"i"` arm.

- [ ] **Step 1: Update the call site**

In `src/input/keymap.rs`, the mode-dispatch match in `handle_key` (~line 80) reads:

```rust
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name),
```

Change it to:

```rust
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name, tokio_handle),
```

- [ ] **Step 2: Update the function signature**

In `src/input/keymap.rs`, `handle_visual_key` (~line 963) currently reads:

```rust
fn handle_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
```

Change it to:

```rust
fn handle_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
```

- [ ] **Step 3: Add the `"i"` arm**

In `handle_visual_key`'s `match key_name { ... }`, add this arm immediately before the final `_ => { ... true }` catch-all (the one at `keymap.rs:998`):

```rust
        "i" => {
            crate::input::actions::echoes::show_echoes_for_selection(state, tokio_handle);
            true
        }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean, no `dead_code` warning for `show_echoes_for_selection` anymore.

- [ ] **Step 5: Run clippy and the test suite**

Run: `cargo clippy 2>&1 | tail -20 && cargo test --lib 2>&1 | tail -20`
Expected: clippy clean (no new warnings in `echoes.rs` / `keymap.rs`); all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Bind i in visual mode to show_echoes_for_selection

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Manual verification (user runs the app)

The echo path is GTK + Voyage + DB coupled and cannot be exercised in `cargo test`. The author must NOT run `cargo run` (per project CLAUDE.md — only the user runs the app). Hand these steps to the user.

- [ ] **Step 1: Build for the user**

Run: `cargo build 2>&1 | tail -5`
Expected: clean build.

- [ ] **Step 2: User reproduction script**

Ask the user to:
1. Launch with `cargo run`, open a Shakespeare work.
2. Press `V` to enter Visual mode, `j` to extend over a single speaker turn, press `i`.
   - Expect: Visual mode exits, echoes overlay opens (same as Reader-mode `i` on that turn).
3. Press `V`, extend `j` across 3+ turns / multiple speakers, press `i`.
   - Expect: "Searching for echoes..." then the echoes overlay with cross-work matches.
4. Jump into an echo's work, then press `alt+i`.
   - Expect: returns to the originating work and reopens the overlay (sticky session works for the synthesized selection turn).

- [ ] **Step 3: Confirm via log**

Run: `rg 'KEY: name=i|ECHOES:.*selection|ACTION' ~/utono/linux-lit/linux-lit-dev.log | tail -20`
Expected: after a Visual-mode `i`, a `KEY: name=i` line is now followed by an `ECHOES: ... (selection)` line (previously the `i` was silently consumed with no follow-up).

---

## Self-Review

**Spec coverage:**
- Visual `i` binding → Task 3. ✓
- Selection → turn mapping (reuse `work_line_for_buffer` pattern) → Task 2 setup + Task 1 helper. ✓
- 1 turn / 2-turn → cache path; >2 turns / cache miss → live embed → Task 2 (cache-hit branch + live-embed branch; both routed by the same `find_echo_turn` lookup, exactly as the spec's "try cached, else live-embed"). ✓
- Synthesize/persist a turn (user decision) → Task 2 reuses `persist_and_load` (`save_echo_turn` + `insert_echo_links`). ✓
- Echoes overlay end-state + `alt+i` return → Task 2 sets `echo_session` / `echo_overlay_turn_id`; verified in Task 4 step 2.4. ✓
- Exit Visual mode first → Task 2 calls `exit_visual_mode`. ✓
- Reader `i` unchanged → no task touches `show_echoes_for_cursor_line`. ✓
- Open item (speaker label for multi-speaker selections) → resolved: first line's speaker, fallback "?" (Task 1 `selection_key`, asserted in test).

**Placeholder scan:** No TBD/TODO; every code step shows full code; commands have expected output. ✓

**Type consistency:** `selection_turn_lines(&[Line], usize, usize) -> Vec<Line>` and `selection_key(&str, &[Line]) -> EchoTurnKey` are used identically in Task 1 (tests) and Task 2 (caller). `show_echoes_for_selection(state_rc, tokio_handle)` signature matches the Task 3 call site. `EchoTurnKey` field names match `src/db/queries.rs:1041`. ✓

**Note on the spec's "try precomputed match first, fall back to live embed":** the cache lookup is keyed on the exact `(work, div1, div2, start_line, end_line)` of the *selection*, not on a snap-to-nearest precomputed turn. A 1-turn selection whose bounds match a previously-echoed turn hits the cache; otherwise it live-embeds and persists. This matches `show_echoes_for_cursor_line`'s own behavior and the user's "synthesize/persist a turn" decision. Snapping a selection to a *different* precomputed turn's bounds is intentionally not done — it would echo text the user didn't select.
