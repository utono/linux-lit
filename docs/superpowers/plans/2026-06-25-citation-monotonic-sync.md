# Citation-Monotonic Sync + Citation-Keyed Gloss/Resume Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make playback sync, glossed-passage seek/jump, and resume position robust by moving them off fragile signals (raw timestamp, buffer text, raw buffer index) onto the authoritative per-line citation / `line_mapping_id`.

**Architecture:** Five independent code changes plus one DB migration. Part A constrains `find_line_for_time` to pick the timestamp candidate nearest the cursor in work-index (citation) order. Part B nulls bad 2H6 timestamps in lit.db. Parts D/E resolve gloss source lines by citation now that `-Amb` editions render canonical, parity-numbered text. Part F persists resume position as `line_mapping_id`. Part G swaps a translations text-join for a citation join.

**Tech Stack:** Rust, GTK4, SQLite (rusqlite), Tokio, MPV IPC. Pure-logic tests via `cargo test --bins`.

## Global Constraints

- Do NOT run the app (`cargo run`); only `cargo build` / `cargo test`. The user runs the app. (CLAUDE.md)
- `cargo test --bins` must stay green; UI/e2e tests are `#[ignore]`d and user-run.
- `-Amb`/`-BBC`/`-DC` editions are line-parity with their base work in `line_mapping` (same `text_file`, identical `(div1,div2,line_in_div)`). Verified 2026-06-25. The tests `text_file_map.rs::base_and_amb_line_mapping_are_parity` and `h6_amb_glossed_lines_match_by_citation` guard this — do NOT remove them.
- `MpvCommand::SetTimestamps` carries `timestamps: Vec<(i64 line_id, f64 start, f64 end)>` (sorted by `start`) and `line_id_to_index: HashMap<i64, usize>` (line_id → work index). Selection logic must use these only.
- Config uses `#[serde(default)]` — new fields are backward-compatible; never remove `work_positions` (legacy fallback).
- lit.db path: `~/utono/litdb/data/lit.db`. DB changes go in the `~/utono/litdb` repo, not linux-lit.
- Commit messages end with the Co-Authored-By / Claude-Session trailer per CLAUDE.md.

---

### Task 1: Citation-monotonic line selection in `find_line_for_time` (Part A)

**Files:**
- Modify: `src/mpv/client.rs` — `find_line_for_time` (currently `:291-328`), its call site (`:55`), `run`'s local state (`:17-22`).
- Test: `src/mpv/client.rs` `#[cfg(test)] mod tests` (existing, `:330+`).

**Interfaces:**
- Consumes: `timestamps: &[(i64,f64,f64)]` (sorted by `.1`), `line_id_to_index: &HashMap<i64,usize>`.
- Produces: `find_line_for_time(time_pos, timestamps, line_id_to_index, last_synced_work_idx: Option<usize>) -> Option<usize>` — returns a work index, choosing among duplicate-timestamp candidates the one nearest `last_synced_work_idx` (ties → forward/larger index). Caller updates `last_synced_work_idx` to the returned value.

- [ ] **Step 1: Write the failing test** — append to `mod tests` in `src/mpv/client.rs`:

```rust
#[test]
fn picks_nearest_candidate_on_duplicate_timestamp() {
    // Two work lines share start=2484: the spirit's line (work idx 37) and the
    // re-read (work idx 71). Cursor is near the first → must stay on 37.
    let timestamps = vec![(/*id*/100, 2484.0, 2485.0), (/*id*/200, 2484.0, 2485.0)];
    let mut map = std::collections::HashMap::new();
    map.insert(100, 37usize);
    map.insert(200, 71usize);
    // Effective time lands in the shared bracket.
    let got = find_line_for_time(2484.5, &timestamps, &map, Some(36));
    assert_eq!(got, Some(37));
}

#[test]
fn backward_seek_picks_near_earlier_candidate() {
    // Cursor far ahead (71); audio seeks back into the 2484 bracket → choose 37.
    let timestamps = vec![(100, 2484.0, 2485.0), (200, 2484.0, 2485.0)];
    let mut map = std::collections::HashMap::new();
    map.insert(100, 37usize);
    map.insert(200, 71usize);
    let got = find_line_for_time(2484.5, &timestamps, &map, Some(71));
    assert_eq!(got, Some(71)); // 71 is its own nearest; the near earlier 37 loses only if 71 is closer — here cursor==71 so 71 wins
}
```

Note: the second test asserts that when the cursor is exactly on a candidate, that candidate is kept (no spurious move). Adjust the expected value if you intend backward seeks to prefer the lower index — but "nearest to last_synced" with cursor==71 yields 71.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins -p linux-lit picks_nearest_candidate_on_duplicate_timestamp`
Expected: FAIL — `find_line_for_time` takes 3 args, not 4 (compile error).

- [ ] **Step 3: Add `last_synced_work_idx` param + nearest-candidate selection.**

Replace the body of `find_line_for_time` (`:291-328`) so that after computing `active` (keep the existing `partition_point` + gap-aware promotion exactly as-is), it builds the candidate set of work indices sharing the chosen entry's `start_time` and picks the nearest:

```rust
fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)],
    line_id_to_index: &HashMap<i64, usize>,
    last_synced_work_idx: Option<usize>,
) -> Option<usize> {
    use crate::input::navigation::{SYNC_GAP_PREROLL, SYNC_GAP_THRESHOLD, SYNC_PREROLL};

    let effective_time = time_pos + SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }

    // (unchanged) gap-aware early jump
    let mut active = idx - 1;
    if let Some(&(_, b_start, _)) = timestamps.get(idx) {
        let (_, a_start, a_end) = timestamps[idx - 1];
        let trigger = b_start - SYNC_GAP_PREROLL;
        let qualifies = if a_end > a_start {
            b_start - a_end > SYNC_GAP_THRESHOLD
        } else {
            true
        };
        if qualifies && time_pos >= trigger {
            active = idx;
        }
    }

    // Candidate set: all timestamp entries sharing `active`'s start_time resolve
    // to distinct work indices (a re-spoken line carries the first occurrence's
    // timestamp). Pick the one nearest the cursor in citation/work-index order,
    // breaking ties toward the forward (larger) index so normal progress always
    // advances. Without a cursor anchor (first sync after load/seek), fall back
    // to the single `active` candidate (legacy behavior).
    let active_start = timestamps[active].1;
    let candidates: Vec<usize> = timestamps
        .iter()
        .filter(|ts| (ts.1 - active_start).abs() < f64::EPSILON)
        .filter_map(|ts| line_id_to_index.get(&ts.0).copied())
        .collect();

    match (last_synced_work_idx, candidates.as_slice()) {
        (_, []) => line_id_to_index.get(&timestamps[active].0).copied(),
        (None, _) => line_id_to_index.get(&timestamps[active].0).copied(),
        (Some(anchor), cands) => cands
            .iter()
            .copied()
            .min_by(|&a, &b| {
                let da = (a as isize - anchor as isize).unsigned_abs();
                let db = (b as isize - anchor as isize).unsigned_abs();
                // nearest wins; tie -> larger index (forward)
                da.cmp(&db).then(b.cmp(&a))
            }),
    }
}
```

- [ ] **Step 4: Thread `last_synced_work_idx` through `run`.** In `run` (`:13-78`), add a local `let mut last_synced_work_idx: Option<usize> = None;` near the other locals (`:17-22`). At the call site (`:55-57`), pass it and update it:

```rust
if let Some(idx) = find_line_for_time(pos, &timestamps, &line_id_to_index, last_synced_work_idx) {
    last_synced_work_idx = Some(idx);
    let _ = evt_tx.send(MpvEvent::CursorSync(idx)).await;
}
```

Reset `last_synced_work_idx = None;` when `timestamps` is replaced, so a work
switch doesn't anchor on the old work's index. `handle_command` owns the
`timestamps` mutation but not this local, so reset it in `run` in BOTH arms that
forward a command to `handle_command` (the `tokio::select!` arm at `:65-67` and
the `else`-branch arm at `:70-72`). In each, before the `handle_command(...)`
call, add:

```rust
if matches!(cmd, MpvCommand::SetTimestamps { .. }) {
    last_synced_work_idx = None;
}
```

Bind the received command to `cmd` first if it isn't already (the select arm
already has `Some(cmd) = cmd_rx.recv()`; the else-branch has `Some(cmd) =>`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins`
Expected: PASS, including the existing `test_find_line_for_time*` (now called with `None` as the 4th arg — update those existing test call sites to pass `None`).

- [ ] **Step 6: Commit**

```bash
git add src/mpv/client.rs
git commit -m "fix(sync): pick timestamp candidate nearest cursor in citation order

find_line_for_time now disambiguates duplicate-timestamp lines (re-spoken
passages) by choosing the candidate nearest last_synced_work_idx, ties toward
forward. Fixes premature jump to a re-read line sharing the first occurrence's
timestamp (2H6 1.4 prophecy).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: Retire the `ABERRANT dist>50` guard (Part A cont.)

**Files:**
- Modify: `src/main.rs` `:189-208` (the `ABERRANT` / `dist > 50` block in the `CursorSync` arm).

**Interfaces:**
- Consumes: the now-monotonic `CursorSync(line_idx)` from Task 1.
- Produces: no signature change; removes the magic-number early-`continue`, keeps a wide sanity clamp.

- [ ] **Step 1: Replace the `dist > 50` clamp with a whole-work sanity clamp.** In `src/main.rs`, change the block at `:194-208` so it only rejects a move larger than the whole work (defensive), and keeps the "no work mapping" skip:

```rust
let cur_wi = s.work_line_for_buffer(s.current_line);
if let Some(cwi) = cur_wi {
    // Task 1 makes CursorSync monotonic in citation order, so the old
    // dist>50 magic guard is no longer load-bearing. Keep only a defensive
    // clamp against a single event jumping more than the whole work (corrupt
    // index), which should never happen.
    let work_len = s.current_work.as_ref().map(|w| w.lines.len()).unwrap_or(usize::MAX);
    let dist = (line_idx as isize - cwi as isize).unsigned_abs();
    if dist >= work_len {
        crate::logging::log(&format!(
            "CURSOR_SYNC: INSANE line_idx={} cur_work={} dist={} work_len={} — skipped",
            line_idx, cwi, dist, work_len,
        ));
        continue;
    }
} else {
    crate::logging::log(&format!(
        "CURSOR_SYNC: SKIP no work mapping for current_line={}", s.current_line
    ));
    continue;
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build`
Expected: builds clean (no behavior test here — this is a guard relaxation verified visually in Task 9).

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(sync): replace ABERRANT dist>50 guard with whole-work sanity clamp

The citation-monotonic selection in find_line_for_time supersedes the magic
distance guard; keep only a defensive clamp against a corrupt index.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Null the bad 2H6-Amb re-read timestamps (Part B — lit.db)

**Files:**
- DB: `~/utono/litdb/data/lit.db` (the live DB; changes committed in the `~/utono/litdb` repo per its conventions).

**Interfaces:** none (data only). Affected `line_mapping_id`s: 1175128, 1175130, 1175131, 1175132, 1175133, 1175134.

- [ ] **Step 1: Verify the rows are the re-read prophecy.**

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT lm.id, lm.line_in_div, substr(lm.canonical_text,1,40), lt.start_time
FROM line_mapping lm JOIN line_timestamps lt ON lt.line_mapping_id=lm.id
WHERE lm.id IN (1175128,1175130,1175131,1175132,1175133,1175134)
ORDER BY lm.line_in_div, lt.media_id;"
```
Expected: line_in_div 67,69,70,71,72,73 with start_times in 2467–2493 (the impossible early values).

- [ ] **Step 2: Delete the bad timestamp rows.**

```bash
sqlite3 ~/utono/litdb/data/lit.db "
DELETE FROM line_timestamps
WHERE line_mapping_id IN (1175128,1175130,1175131,1175132,1175133,1175134);"
```

- [ ] **Step 3: Verify they are gone.**

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT COUNT(*) FROM line_timestamps
WHERE line_mapping_id IN (1175128,1175130,1175131,1175132,1175133,1175134);"
```
Expected: `0`.

- [ ] **Step 4: Record the change in litdb's repo** per its conventions (a migration note / SQL file under `~/utono/litdb`), then commit there. Do NOT commit the binary DB into linux-lit.

---

### Task 4: Gloss audio seek by citation (Part D)

**Files:**
- Modify: `src/input/actions/gloss.rs` — `source_block_seek_time` (`:1753-1772`) and `first_source_start_time` (`:2144-2159`).
- Test: `src/input/actions/gloss.rs` existing `#[cfg(test)]` tests for `first_source_start_time` (`:2206+`).

**Interfaces:**
- Consumes: `gloss.start_citation` (a `String` like `"2H6.1.4.71"`), parseable by `crate::app::parse_citation` → `Option<(i64,i64,i64)>`; `work.lines[i]` with `.div1/.div2/.line_in_div: i64` and `.timestamp: Option<Timestamp{start,end}>`.
- Produces: `source_block_seek_time` resolves the source line by citation, reading its `timestamp.start` directly.

- [ ] **Step 1: Write the failing test** — add to gloss.rs tests:

```rust
#[test]
fn seek_resolves_by_citation_not_first_text_match() {
    // verse "Let him shun castles" appears twice; citation points at the 2nd.
    // helper signature defined in Step 3.
    let lines = vec![
        (1i64,4i64,37i64, Some(2484.0)), // first occurrence
        (1,4,71, Some(2620.0)),          // re-read (citation target)
    ];
    let got = start_time_for_citation((1,4,71), &lines);
    assert_eq!(got, Some(2620.0));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins seek_resolves_by_citation_not_first_text_match`
Expected: FAIL — `start_time_for_citation` undefined.

- [ ] **Step 3: Add a pure citation→start-time helper and use it in `source_block_seek_time`.** Add:

```rust
/// Start time of the work line whose citation == `cit`. Pure + testable.
fn start_time_for_citation(
    cit: (i64, i64, i64),
    lines: &[(i64, i64, i64, Option<f64>)], // (div1, div2, line_in_div, start)
) -> Option<f64> {
    lines.iter()
        .find(|(d1, d2, l, _)| (*d1, *d2, *l) == cit)
        .and_then(|(_, _, _, start)| *start)
}
```

Rewrite `source_block_seek_time` (`:1753`) to resolve by the gloss's `start_citation` first, falling back to the existing text scan only when the citation can't be parsed/found:

```rust
fn source_block_seek_time(s: &AppState, index: i32) -> Option<f64> {
    let gloss = s.gloss_list.get(s.gloss_index)?;
    let blocks = crate::ui::gloss_block::gloss_blocks(&gloss.gloss_text);
    let block = blocks.iter().find(|b| b.kind == BlockKind::Source && b.index == index)?;
    let work = s.current_work.as_ref()?;

    // Citation-first (authoritative; -Amb editions are parity-numbered now).
    let start = crate::app::parse_citation(&gloss.start_citation)
        .and_then(|cit| {
            let lines: Vec<(i64, i64, i64, Option<f64>)> = work.lines.iter()
                .map(|l| (l.div1, l.div2, l.line_in_div, l.timestamp.map(|t| t.start)))
                .collect();
            start_time_for_citation(cit, &lines)
        })
        // Fallback: citationless/.txt-only works — match the verse text.
        .or_else(|| {
            let work_pairs: Vec<(String, Option<f64>)> = work.lines.iter()
                .map(|l| (l.text.clone(), l.timestamp.map(|t| t.start)))
                .collect();
            first_source_start_time(&block.display, &work_pairs)
        })?;
    Some(crate::input::navigation::preroll_seek_time(start))
}
```

Keep `first_source_start_time` as the fallback (and its existing tests). Update its doc comment to say it is the citationless fallback, not the primary path.

- [ ] **Step 4: Run tests**

Run: `cargo test --bins`
Expected: PASS (new test + existing `first_source_start_time` tests).

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "fix(gloss): seek glossed-passage audio by citation, not first text match

-Amb editions now render canonical parity-numbered text, so resolve the source
line by start_citation; text match retained only as the citationless fallback.
Fixes wrong-occurrence seek for re-read/refrain lines.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 5: Jump-to-gloss-source by citation first (Part E)

**Files:**
- Modify: `src/input/actions/gloss.rs` `jump_to_gloss_source_start` (`:30-82`), specifically the `by_text` / `start_idx` selection (`:43-58`) and the comment (`:36-42`).

**Interfaces:**
- Consumes: `target: Option<(i64,i64,i64)>` (already a param), `source_text: &str`, `work.lines`.
- Produces: same return (`bool`); destination now citation-first.

- [ ] **Step 1: Invert the lookup to citation-first.** Replace `:43-58`:

```rust
    // -Amb editions now render the canonical parity-numbered .txt (verified
    // 2026-06-25; base and -Amb share text_file and (div1,div2,line_in_div)).
    // Resolve by the citation tuple first — it is unique, so a repeated source
    // line can't land on the wrong occurrence. Text match is the citationless
    // (.txt-only) fallback.
    let by_citation = target.and_then(|t| {
        work.lines.iter().position(|l| (l.div1, l.div2, l.line_in_div) == t)
    });
    let first_src = source_text.lines().next().map(str::trim).unwrap_or("");
    let start_idx = match by_citation.or_else(|| {
        if first_src.is_empty() {
            None
        } else {
            work.lines.iter().position(|l| l.text.trim() == first_src)
        }
    }) {
        Some(i) => i,
        None => return false,
    };
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: clean (no pure test — cursor-landing is integration-level; covered visually in Task 9 and by the existing parity test guaranteeing citation resolves on `-Amb`).

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "fix(gloss): jump to gloss source by citation first, text as fallback

-Amb parity numbering makes the citation tuple authoritative; resolves the
duplicate-first-source-line wrong-occurrence landing.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 6: Persist resume position as `line_mapping_id` (Part F)

**Files:**
- Modify: `src/config.rs` (`:61` add `work_position_ids`).
- Modify: `src/app/mod.rs` — `save_position` (`:3470-3481`), outgoing-work save (`:2407-2409`), resume application (insert after the clamp at `:2929-2931`).

**Interfaces:**
- Consumes: `Config.work_positions: HashMap<String,usize>` (legacy), `state.buffer_to_work`/`work_line_for_buffer`, `work.lines[wi].id`.
- Produces: `Config.work_position_ids: HashMap<String,i64>`; resume prefers id → buffer remap, legacy index as fallback.

- [ ] **Step 1: Add the id map to config.** In `src/config.rs` after `:61`:

```rust
    /// Resume position keyed by line_mapping_id (citation-stable across
    /// re-imports). Preferred over `work_positions` (legacy raw buffer index).
    #[serde(default)]
    pub work_position_ids: HashMap<String, i64>,
```

Add `work_position_ids: HashMap::new(),` to the `Default`/constructor near `:210`.

- [ ] **Step 2: Write the id at both save sites.** In `save_position` (`:3470`), after computing `abbrev`, resolve and store the id:

```rust
pub fn save_position(state: &mut AppState) {
    if let Some(work) = &state.current_work {
        let abbrev = work.abbrev.clone();
        let cc = state.column_count();
        // Resolve the cursor's line_mapping_id (citation-stable).
        let id = state.work_line_for_buffer(state.current_line)
            .and_then(|wi| work.lines.get(wi))
            .map(|l| l.id);
        state.config.last_work = Some(abbrev.clone());
        state.config.work_positions.insert(abbrev.clone(), state.current_line); // legacy fallback
        if let Some(id) = id {
            state.config.work_position_ids.insert(abbrev, id);
        }
        state.config.last_column_count = Some(cc);
        crate::config::save(&state.config);
    }
}
```

At the outgoing-work save (`:2407-2409`), mirror it:

```rust
    if let Some(ref old_work) = state.current_work {
        state.config.work_positions.insert(old_work.abbrev.clone(), state.current_line);
        if let Some(id) = state.work_line_for_buffer(state.current_line)
            .and_then(|wi| old_work.lines.get(wi)).map(|l| l.id)
        {
            state.config.work_position_ids.insert(old_work.abbrev.clone(), id);
        }
    }
```

- [ ] **Step 3: Remap id → buffer on resume, before the snap/page_top block.** In `display_work_at_with_prepared`, immediately after the clamp (`:2929-2931`) and before the `target_line_id.is_none()` blocks (`:2937`), insert:

```rust
    // Part F: when resuming (no explicit concordance target), prefer the
    // citation-stable line_mapping_id over the legacy raw buffer index, so a
    // lit.db re-import / repagination doesn't land on the wrong speech.
    if target_line_id.is_none() && std::env::var("LIT_START_POS").is_err() {
        if let Some(work) = &state.current_work {
            if let Some(&saved_id) = state.config.work_position_ids.get(&work.abbrev) {
                if let Some(work_idx) = work.lines.iter().position(|l| l.id == saved_id) {
                    let buf_idx = if let Some(ref lm) = state.line_map {
                        let bi = *lm.work_to_buffer.get(work_idx).unwrap_or(&state.current_line);
                        if lm.buffer_to_work.get(bi) == Some(&Some(work_idx)) { bi } else { state.current_line }
                    } else {
                        work_idx
                    };
                    state.current_line = buf_idx.min(state.effective_line_count().saturating_sub(1));
                }
            }
        }
    }
```

(The existing snap-to-dialogue at `:2960` and page_top derivation at `:2993` then operate on the corrected line. `LIT_START_POS` and legacy `work_positions` remain the fallbacks via the unchanged `saved_line` at `:2430`.)

- [ ] **Step 4: Build + run pure suite**

Run: `cargo build && cargo test --bins`
Expected: clean build, suite green.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/app/mod.rs
git commit -m "fix(resume): key saved position on line_mapping_id, not raw buffer line

Adds config.work_position_ids; save resolves the cursor's id, resume remaps it
through work_to_buffer (same path as concordance jumps). Legacy work_positions
index kept as fallback. Survives lit.db re-import / repagination.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 7: Translations `-Amb`→base join by citation (Part G)

**Files:**
- Modify: `src/db/queries.rs` `:373-384` (the `-Amb` translations fallback join).

**Interfaces:**
- Consumes: `line_mapping` parity guarantee (Global Constraints).
- Produces: same function output; join keyed on `line_in_div` instead of `normalized_text`.

- [ ] **Step 1: Swap the text join for a citation join.** In the fallback query (`:374-384`), replace `AND b.normalized_text = a.normalized_text` with `AND b.line_in_div = a.line_in_div`:

```rust
                "SELECT a.id, MIN(lt.translation) \
                 FROM line_mapping a \
                 JOIN line_mapping b \
                   ON b.work_abbrev = ?2 \
                  AND b.div1 = a.div1 \
                  AND b.div2 = a.div2 \
                  AND b.line_in_div = a.line_in_div \
                 JOIN line_translations lt ON lt.line_mapping_id = b.id \
                 WHERE a.work_abbrev = ?1 \
                 GROUP BY a.id",
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: clean. (Behavioral parity is guaranteed by the line-parity constraint; no new pure test — exercised by translations display, user-verifiable.)

- [ ] **Step 3: Commit**

```bash
git add src/db/queries.rs
git commit -m "refactor(db): join -Amb translations to base by citation, not text

Base and -Amb are line-parity now; join on line_in_div instead of
normalized_text. Removes a dead aberrant-numbering text-match workaround.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 8: Full pure-suite + clippy gate

**Files:** none (verification task).

- [ ] **Step 1: Run the full pure suite.**

Run: `cargo test --bins`
Expected: all green.

- [ ] **Step 2: Clippy.**

Run: `cargo clippy --bins`
Expected: no new warnings introduced by these changes.

- [ ] **Step 3: If anything fails, fix inline and re-run.** No commit if nothing changed.

---

### Task 9: User-run visual verification (handoff)

**Files:** none. Per CLAUDE.md the agent does NOT run the app — produce the exact commands and expected results for the user.

- [ ] **Step 1: Give the user the sync reproduction.**

> Open `2H6-Amb`, go to Act 1 Scene 4 (the conjuring scene). Start playback (Tab) from before the Spirit's prophecy (~2427 s, "By the eternal God…"). Watch the highlight cross the Spirit's prophecy (lines 34–39). **Expected:** the cursor advances one line at a time and does **not** jump forward to the re-read (`Tell me what fate awaits…` / `Let him shun castles;`).

- [ ] **Step 2: Give the user the gloss-seek check.**

> On a work with a gloss whose source line is re-read/repeats, press `a`/space on the glossed passage. **Expected:** audio seeks to the passage's own occurrence (its citation), not an earlier duplicate.

- [ ] **Step 3: Give the user the resume check.**

> Note your place in a work, quit, relaunch. **Expected:** resumes on the same speech. (Stronger: after a lit.db re-import that shifts lines, resume still lands on the same citation, not a shifted buffer line.)

- [ ] **Step 4: If the e2e harness is available, offer the nav-fuzz command** (does not cover sync, but guards against pagination regressions from the resume change):

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work 2H6-Amb
```
