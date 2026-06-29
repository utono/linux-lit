# Fold-Aware Stage-Direction Mapping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Map a folded multi-line stage direction to its DB stage row(s) so it carries a citation and is colored / addressable like any other line.

**Architecture:** One real change in `build_line_map_mode`'s `WholeLine` stage branch: when no single DB row matches the folded buffer line, concatenate consecutive `sub_line > 0` DB rows (space-joined, as `clean_file_lines` joins) and match that run; map the folded line to the first row, mark all consumed rows' reverse lookup, advance the cursor. Plus a `SNAPSHOT_VERSION` bump so stale snapshots rebuild, and two comment corrections.

**Tech Stack:** Rust, SQLite (rusqlite, only in the gated real-data test). Pure-logic tests via `cargo test --bins`.

## Global Constraints

- Do NOT run the app (`cargo run`); only `cargo build` / `cargo test`. The user runs the app and does the visual confirm. (CLAUDE.md)
- `cargo test --bins` must stay green; clippy warning count must not increase (baseline 118).
- The fix is in `MatchMode::WholeLine` ONLY (plays/verse). Do NOT touch `ParagraphAccumulate` (prose/BCP), which has no `sub_line>0` stage path.
- `clean_file_lines` joins continuation lines with a SINGLE space (`joined.push(' ')`); the DB-row concatenation MUST use the same single-space join so the comparison is exact (verified byte-identical for 2H6 1.4.43).
- The existing single-row exact match (`work_lines[wi].text.trim() == want`) stays as the FIRST attempt; multi-row accumulation is the fallback only when it fails.
- On a failed multi-row match, leave `buffer_to_work[buf_idx] = None` and `db_cursor` UNCHANGED (identical to today — no new mis-mapping).
- `WINDOW` (= 50, text_file_map.rs:141) bounds the scan; the accumulation must stay within it.
- `build_line_map(file_lines, work_lines, is_prose)` is the `WholeLine` entry; `Line` test fixtures use the `make_line_div(id, text, normalized, is_dialogue, div1, div2)` helper (which sets `sub_line: 0`) — SD fixtures need `sub_line > 0`, so build those `Line`s with an explicit helper (Task 1 Step 1).
- Commit messages end with the Co-Authored-By / Claude-Session trailer per CLAUDE.md.

---

### Task 1: Fold-aware multi-row stage match + unit tests

**Files:**
- Modify: `src/text_file_map.rs` — the `WholeLine` stage branch (`:294-308`), the comment (`:289-293`).
- Test: `src/text_file_map.rs` `#[cfg(test)] mod` (the module holding `make_line_div`, ~`:1139`).

**Interfaces:**
- Consumes: `work_lines: &[Line]` (with `.sub_line: i64`, `.text: String`), `WINDOW`, `db_cursor`.
- Produces: no signature change; folded multi-row SD buffer lines now map.

- [ ] **Step 1: Write the failing unit tests.** Add to the test module in `src/text_file_map.rs` (after `make_line_div`):

```rust
/// A stage-direction work line (sub_line > 0). normalize() of stage text is
/// empty, so pass "" as normalized (matches how the spoken-line matcher skips it).
fn make_stage_line(id: i64, text: &str, div1: i64, div2: i64, line_in_div: i64, sub_line: i64) -> Line {
    Line {
        id,
        citation: String::new(),
        text: text.to_string(),
        normalized: String::new(),
        speaker: None,
        is_dialogue: false,
        timestamp: None,
        div1,
        div2,
        line_in_div,
        sub_line,
        is_chapter: false,
        is_spoken: None,
    }
}

#[test]
fn folded_multiline_stage_direction_maps_to_its_rows() {
    // Buffer: dialogue, then a FOLDED SD (clean_file_lines joined two source
    // lines with a space), then more dialogue.
    let file_lines: Vec<String> = vec![
        "Lay hands upon these traitors and their trash.".into(),
        "[The Guard arrest Margery Jourdain and her accomplices and seize their papers.]".into(),
        "Beldam, I think we watched you at an".into(),
    ];
    // DB: the dialogue rows + the SD split across TWO sub_line>0 rows.
    let work_lines = vec![
        make_line_div(1, "Lay hands upon these traitors and their trash.",
            "lay hands upon these traitors and their trash", true, 1, 4),
        make_stage_line(2, "[The Guard arrest Margery Jourdain and her", 1, 4, 43, 1),
        make_stage_line(3, "accomplices and seize their papers.]", 1, 4, 43, 2),
        make_line_div(4, "Beldam, I think we watched you at an",
            "beldam i think we watched you at an", true, 1, 4),
    ];
    let lm = build_line_map(&file_lines, &work_lines, false);
    // Folded buffer line 1 -> first SD row (work idx 1).
    assert_eq!(lm.buffer_to_work[1], Some(1), "folded SD must map to its first DB row");
    // BOTH SD rows' reverse lookup point at the folded buffer line 1.
    assert_eq!(lm.work_to_buffer[1], 1);
    assert_eq!(lm.work_to_buffer[2], 1);
    // Surrounding dialogue unaffected.
    assert_eq!(lm.buffer_to_work[0], Some(0));
    assert_eq!(lm.buffer_to_work[2], Some(3));
}

#[test]
fn single_line_stage_direction_still_maps_1to1() {
    let file_lines: Vec<String> = vec!["[To Jourdain.]".into()];
    let work_lines = vec![make_stage_line(1, "[To Jourdain.]", 1, 4, 43, 3)];
    let lm = build_line_map(&file_lines, &work_lines, false);
    assert_eq!(lm.buffer_to_work[0], Some(0));
    assert_eq!(lm.work_to_buffer[0], 0);
}

#[test]
fn unmatched_folded_stage_direction_stays_none() {
    // A folded SD whose join matches no DB run leaves buffer_to_work None.
    let file_lines: Vec<String> = vec!["[Nobody arrests anyone at all here.]".into()];
    let work_lines = vec![
        make_stage_line(1, "[The Guard arrest Margery Jourdain and her", 1, 4, 43, 1),
        make_stage_line(2, "accomplices and seize their papers.]", 1, 4, 43, 2),
    ];
    let lm = build_line_map(&file_lines, &work_lines, false);
    assert_eq!(lm.buffer_to_work[0], None);
}
```

- [ ] **Step 2: Run tests to verify they fail.**

Run: `cargo test --bins folded_multiline_stage_direction_maps_to_its_rows`
Expected: FAIL — `assert_eq!(lm.buffer_to_work[1], Some(1))` fails (folded line is currently `None`).

- [ ] **Step 3: Implement the multi-row fallback.** In `src/text_file_map.rs`, replace the `WholeLine` stage branch (`:294-308`) with:

```rust
                if line_types::is_stage_direction(file_lines[buf_idx].trim()) {
                    let want = file_lines[buf_idx].trim();
                    let window_end = (db_cursor + WINDOW).min(n_work);

                    // Fast path: a single DB stage row equals the buffer line
                    // (single-line SDs and unfolded directions).
                    let mut matched_single = false;
                    for wi in db_cursor..window_end {
                        if work_lines[wi].sub_line > 0
                            && work_lines[wi].text.trim() == want
                        {
                            buffer_to_work[buf_idx] = Some(wi);
                            work_to_buffer[wi] = buf_idx;
                            db_cursor = wi + 1;
                            matched += 1;
                            matched_single = true;
                            break;
                        }
                    }
                    if matched_single {
                        continue;
                    }

                    // Fallback: `clean_file_lines` folds a multi-line stage
                    // direction into ONE buffer line (space-joined). It then
                    // matches NO single DB row, but DOES match a run of
                    // consecutive `sub_line > 0` rows joined the same way. Find
                    // that run, map the folded line to its FIRST row, and point
                    // every consumed row's reverse lookup at the folded line.
                    for start in db_cursor..window_end {
                        if work_lines[start].sub_line == 0 {
                            continue; // runs begin on a stage row
                        }
                        let mut joined = String::new();
                        let mut end = start;
                        while end < window_end && work_lines[end].sub_line > 0 {
                            if !joined.is_empty() {
                                joined.push(' ');
                            }
                            joined.push_str(work_lines[end].text.trim());
                            if joined.len() > want.len() {
                                break; // overshot — this run can't equal `want`
                            }
                            if joined == want {
                                for wi in start..=end {
                                    work_to_buffer[wi] = buf_idx;
                                }
                                buffer_to_work[buf_idx] = Some(start);
                                db_cursor = end + 1;
                                matched += 1;
                                break;
                            }
                            end += 1;
                        }
                        if buffer_to_work[buf_idx].is_some() {
                            break;
                        }
                    }
                    continue;
                }
```

Then update the comment block above it (`:289-293`) to:

```rust
                // Stage directions normalize to empty (brackets stripped), so the
                // spoken-line matcher below skips them. Match a stage buffer line
                // to its DB stage row(s) (sub_line > 0) by RAW trimmed text. A
                // single-line SD matches one row 1:1. A multi-line SD that
                // `clean_file_lines` FOLDED into one space-joined buffer line
                // matches a RUN of consecutive sub_line>0 rows joined the same way
                // — see the fallback below (the old "byte-identical 1:1" assumption
                // breaks for folded directions now that lit.db has sub_line rows).
```

- [ ] **Step 4: Run the new tests to verify they pass.**

Run: `cargo test --bins folded_multiline_stage_direction_maps_to_its_rows single_line_stage_direction_still_maps_1to1 unmatched_folded_stage_direction_stays_none`
Expected: all PASS.

- [ ] **Step 5: Run the full pure suite (no regression in the binding layer).**

Run: `cargo test --bins`
Expected: 445+3 = all pass (the 3 new tests added; the existing `build_line_map` / stage-direction tests still green).

- [ ] **Step 6: Clippy parity.**

Run: `cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: `generated 118 warnings`.

- [ ] **Step 7: Commit.**

```bash
git add src/text_file_map.rs
git commit -m "fix(linemap): map folded multi-line stage directions to their DB rows

clean_file_lines folds a multi-line SD into one space-joined buffer line; the
stage matcher's raw-text 1:1 match then failed (the SD is now 2+ sub_line>0 DB
rows), leaving it UNMAPPED so reader-gloss/u/./bookmarks skipped it. Add a
fallback that joins consecutive sub_line>0 rows the same way and maps the folded
line to the first row, pointing every consumed row's reverse lookup at it.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: Real-data regression test (through the app's prep path)

**Files:**
- Test: `src/text_file_map.rs` `#[cfg(test)] mod` — a lit.db-gated test that reproduces the bug through `clean_file_lines` + `build_line_map` (NOT raw `.txt`).

**Interfaces:**
- Consumes: `crate::db::queries::{open_db, load_work}`, `crate::app::text_prep` clean path, `build_line_map`.

**Note:** The earlier misleading repro ran on the raw `.txt` (unfolded) and passed. This test MUST run through the same `clean_file_lines` fold the app uses, or it won't reproduce the bug. Confirm the clean function is reachable from the test; if `clean_file_lines` is private (`fn`, not `pub`), use the public `crate::app::text_prep::prepare_text_only(&work)` which calls it and returns `cleaned_lines`.

- [ ] **Step 1: Check how to reach the cleaned lines.** Read `src/app/text_prep.rs`: `clean_file_lines` is private; `prepare_text_only(work: &Work) -> Option<PreparedTextOnly>` is public and its result has `cleaned_lines: Vec<String>`. Use that.

- [ ] **Step 2: Write the gated regression test.** Add to the test module:

```rust
/// Regression for the folded-SD coloring bug: build the map through the SAME
/// clean_file_lines fold the app uses (NOT raw .txt — that was the misleading
/// repro), and assert the folded `[The Guard arrest...]` SD in 2H6-Amb 1.4 maps
/// to (1,4,43) instead of staying UNMAPPED. Skipped when lit.db is unavailable.
#[test]
fn h6_amb_folded_guard_sd_maps_through_clean_path() {
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => { eprintln!("skip: no lit.db"); return; }
    };
    let work = match crate::db::queries::load_work(&conn, "2H6-Amb") {
        Ok(w) => w,
        Err(_) => { eprintln!("skip: 2H6-Amb not loaded"); return; }
    };
    let prepared = match crate::app::text_prep::prepare_text_only(&work) {
        Some(p) => p,
        None => { eprintln!("skip: no text_file"); return; }
    };
    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
    let lm = build_line_map(&prepared.cleaned_lines, &work.lines, is_prose);

    // The folded SD renders as one cleaned buffer line containing "Guard arrest".
    let sd_buf = prepared.cleaned_lines.iter()
        .position(|l| l.contains("Guard arrest"));
    let sd_buf = match sd_buf {
        Some(b) => b,
        None => { eprintln!("skip: SD not in cleaned text"); return; }
    };
    let wi = lm.buffer_to_work[sd_buf];
    assert!(wi.is_some(),
        "folded SD buffer line {sd_buf} must map (was the bug: UNMAPPED -> uncolored)");
    let l = &work.lines[wi.unwrap()];
    assert_eq!((l.div1, l.div2, l.line_in_div), (1, 4, 43),
        "folded SD must map to citation 1.4.43");
}
```

- [ ] **Step 3: Run it.**

Run: `cargo test --bins h6_amb_folded_guard_sd_maps_through_clean_path -- --nocapture`
Expected: PASS (or "skip: ..." if lit.db is absent on the runner — acceptable, it's gated). If it FAILS with the SD `None`, Task 1's fix is incomplete — return to Task 1.

- [ ] **Step 4: Commit.**

```bash
git add src/text_file_map.rs
git commit -m "test(linemap): real-data regression for folded 2H6-Amb SD mapping

Builds the map through clean_file_lines (the app's fold path, not raw .txt) and
asserts the folded [The Guard arrest...] SD maps to 1.4.43. lit.db-gated.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Bump SNAPSHOT_VERSION + fix the clean_file_lines comment

**Files:**
- Modify: `src/snapshot.rs:39` (version) + the comment block above it.
- Modify: `src/app/text_prep.rs:83` (the stale fold comment).

**Interfaces:** none (constant + comments).

- [ ] **Step 1: Bump the version.** In `src/snapshot.rs`, change `pub const SNAPSHOT_VERSION: u32 = 9;` to `= 10;` and prepend a comment line above the existing version comment:

```rust
// Bumped to 10: build_line_map now maps a FOLDED multi-line stage direction to
// its sub_line>0 DB rows (previously UNMAPPED). A snapshot built before this fix
// cached the SD as None; its db_fingerprint and .txt mtime are unchanged, so only
// a version bump invalidates it. Serialized shape is unchanged (same Vec types).
```

- [ ] **Step 2: Correct the clean_file_lines comment.** In `src/app/text_prep.rs`, replace the stale claim (currently around `:83`, "Stage directions normalize to empty in the line map, so folding them doesn't disturb work-line mapping."):

```rust
        // Multi-line stage direction: the Folger source hard-wraps a single
        // bracketed direction across several lines (opens with `[`, no closing
        // `]`). Fold those source lines into one buffer line so GTK soft-wraps
        // the direction naturally instead of preserving the mid-sentence breaks.
        // The folded line no longer matches a single DB stage row 1:1, so
        // build_line_map's stage matcher re-joins consecutive sub_line>0 rows to
        // map it (see src/text_file_map.rs); keep the single-space join here in
        // sync with that matcher's join.
```

- [ ] **Step 3: Build + full suite.**

Run: `cargo build && cargo test --bins`
Expected: clean build, all pass.

- [ ] **Step 4: Commit.**

```bash
git add src/snapshot.rs src/app/text_prep.rs
git commit -m "fix(snapshot): bump SNAPSHOT_VERSION to 10 for fold-aware SD mapping

Stale snapshots cached the folded SD as unmapped and won't auto-invalidate
(db_fingerprint + .txt mtime unchanged); the bump rebuilds them once. Also
correct the now-false clean_file_lines fold comment.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 4: Gate + user visual confirm

**Files:** none (verification).

- [ ] **Step 1: Full pure suite + clippy.**

Run: `cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: all pass; `generated 118 warnings`.

- [ ] **Step 2: Hand the user the visual confirm.** Report: no manual cache clear is needed (the SNAPSHOT_VERSION bump rebuilds on next open). Ask the user to:
  1. `cargo run`, open **2H6-Amb**, go to Act 1 Scene 4, to the spread with `[The Guard arrest Margery Jourdain... ]`.
  2. Confirm that SD is now rose-tinted like the surrounding glossed lines (the bug was: it stayed blue).
  3. Addressability per the policy (Task 5): with the cursor on that SD, confirm `.` (set chapter) and bookmark act on it (it's mapped now), and that `u` (set start time) NO-OPs with a toast (the SD is not spoken). On a SPOKEN SD (a line marked `is_spoken=1`), `u` should succeed.

---

### Task 5: Gate `u` (set start time) to spoken stage directions

**Files:**
- Modify: `src/input/timestamps.rs` — `set_start_time` (early guard, after the `work_line_for_buffer` resolution ~:98).
- Modify: `src/input/navigation.rs` — make `show_chapter_toast` callable from timestamps (visibility), OR add a small toast call.
- Test: `src/input/timestamps.rs` — pure gate-decision helper test.

**Interfaces:**
- Consumes: `work.lines[line_idx]` with `.sub_line: i64` and `.is_spoken: Option<bool>`.
- Produces: `set_start_time` returns `false` (no write) when the cursor line is a stage direction (`sub_line > 0`) that is NOT spoken (`is_spoken != Some(true)`), surfacing a toast. Dialogue lines and spoken SDs are unaffected.

**Policy (decided with the user mid-flight):** `u` (audio start time) is meaningful only on a line that is actually spoken in the media. Stage directions are spoken only in specific works (data-driven, NOT a hardcoded `abbrev == "H8"` — the SDs currently marked `is_spoken=1` are in 1H4-Amb/2H6-Amb, so the work-hardcode would be wrong). `.` (chapter) and bookmark are position references, NOT audio — they stay allowed on any mapped line (do NOT gate them). `set_end_time` is part of the same audio-timestamp family — apply the SAME gate to it for consistency (an end time on an unspoken SD is equally meaningless).

- [ ] **Step 1: Write the failing pure test.** Extract the gate decision into a pure predicate and test it. Add to `src/input/timestamps.rs`:

```rust
/// `u`/end-time are audio timestamps — meaningful only on a SPOKEN line. A stage
/// direction (`sub_line > 0`) that is not marked spoken (`is_spoken != Some(true)`)
/// must be rejected; dialogue lines (`sub_line == 0`) and spoken SDs pass.
fn timestamp_allowed(sub_line: i64, is_spoken: Option<bool>) -> bool {
    sub_line == 0 || is_spoken == Some(true)
}

#[cfg(test)]
mod timestamp_gate_tests {
    use super::timestamp_allowed;
    #[test]
    fn dialogue_line_allowed() {
        assert!(timestamp_allowed(0, None));
        assert!(timestamp_allowed(0, Some(false)));
    }
    #[test]
    fn unspoken_stage_direction_rejected() {
        assert!(!timestamp_allowed(1, None));
        assert!(!timestamp_allowed(2, Some(false)));
    }
    #[test]
    fn spoken_stage_direction_allowed() {
        assert!(timestamp_allowed(1, Some(true)));
    }
}
```

- [ ] **Step 2: Run to verify it fails.**

Run: `cargo test --bins timestamp_gate_tests`
Expected: FAIL — `timestamp_allowed` not defined (compile error).

- [ ] **Step 3: Implement the gate in `set_start_time` and `set_end_time`.** After the `line_idx` is resolved via `work_line_for_buffer` (in `set_start_time` ~:98, and the matching point in `set_end_time`), before any DB write, add:

```rust
    {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        let l = &work.lines[line_idx];
        if !timestamp_allowed(l.sub_line, l.is_spoken) {
            crate::logging::log(&format!(
                "TS: refused start/end time on unspoken stage direction (line {}, sub_line {})",
                line_idx, l.sub_line
            ));
            crate::input::navigation::show_chapter_toast(
                state, "Not a spoken line — no timestamp set",
            );
            return false;
        }
    }
```

(Place the same guard in `set_end_time` after its `line_idx` resolution.) The `timestamp_allowed` helper is defined once (Step 1) and used by both.

- [ ] **Step 4: Make `show_chapter_toast` callable.** In `src/input/navigation.rs`, change `fn show_chapter_toast` to `pub(crate) fn show_chapter_toast`. (It's the existing transient-toast helper; reuse it rather than adding a parallel toast.)

- [ ] **Step 5: Run tests + build.**

Run: `cargo test --bins timestamp_gate_tests && cargo build`
Expected: the 3 gate tests PASS; build clean.

- [ ] **Step 6: Full suite + clippy.**

Run: `cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: all pass; `generated 118 warnings`.

- [ ] **Step 7: Commit.**

```bash
git add src/input/timestamps.rs src/input/navigation.rs
git commit -m "feat(timestamps): gate u/end-time to spoken lines only

Now that stage directions map (and are cursor-addressable), u (set start time)
and set_end_time must NOT set a meaningless audio timestamp on an UNSPOKEN stage
direction. Gate on is_spoken: a dialogue line (sub_line==0) or a spoken SD
(is_spoken==Some(true)) passes; an unspoken SD no-ops with a toast. Data-driven
(is_spoken), not a hardcoded work. Chapter/bookmark stay allowed on any line.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```
