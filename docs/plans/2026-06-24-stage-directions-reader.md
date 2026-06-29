# Stage Directions in the Reader — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the linux-lit reader display, navigate, and gloss-select the stage-direction rows litdb added to `lit.db` (the `sub_line` model), driven by DB metadata rather than text inference, and merge the parked rendering branch while dropping its now-unnecessary `inject_stage_directions` workaround.

**Architecture:** `Line` gains a `sub_line` field; queries order by it; `build_line_map` matches stage `.txt` lines to DB stage rows by raw text (the linchpin that gives stage buffer lines a `buffer_to_work` entry); nav classifiers read stage/dialogue-ness from the mapped `Line` with regex as a no-mapping fallback; the rendering branch is already present on this branch, so its workaround is deleted; the snapshot version is bumped.

**Tech Stack:** Rust, GTK4 (gtk4-rs / sourceview5), SQLite (rusqlite), bincode snapshots.

## Branch

Work continues on the **current branch `feat/gloss-overlay-stage-directions`**, which already carries the `<stage>` parse/render commits (`6371f2b`, `9bb0f34`, `95d4a56`, `8f43af2`, `30a0355`) this work builds on. This is a deliberate stack (the new work depends on those commits and then deletes the injection from §5). Do NOT branch off master — that would lose the rendering commits.

## Global Constraints

- Do NOT run the app (`cargo run`) or launch a compositor; the user runs visual checks. Agent verifies with `cargo build`, `cargo test --bins`, `cargo clippy`. (linux-lit CLAUDE.md.)
- Authoritative metadata: read a per-line fact from `lit.db` via `LineMap`/`Line`; never re-infer it by classifying buffer text where a DB mapping exists. Regex classifiers remain ONLY as the no-mapping fallback. (CLAUDE.md "Pagination & Scene Boundaries"; memory `feedback_authoritative_metadata_not_text_inference`.)
- Stage directions are display-only: a `sub_line > 0` row is never a cursor stop for dialogue nav/sync and never spoken by TTS.
- `normalize()` in `text_file_map.rs` is perf-critical (the hot path in `build_line_map`); do NOT change it. Stage matching is added alongside it.
- Verified DB facts to rely on: `line_mapping.sub_line` exists; spoken rows `sub_line=0` keep their scholarly `line_in_div`; stage rows `sub_line=1..N` share the host line's `line_in_div`, have `[bracketed]` `canonical_text`, `speaker=NULL`; a `.txt` stage line is byte-identical to its DB stage row (1:1, incl. multi-line). `2H6`/`2H6-Amb` are byte-identical (3537 rows each).
- Commit trailer on every commit:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
  ```

---

## File map

- `src/db/models.rs` — `Line` gains `pub sub_line: i64` (Task 1).
- `src/db/queries.rs` — `load_work` reads `sub_line`, forces `is_dialogue=false` for stage rows, `ORDER BY … , sub_line`; 3 other queries get `, sub_line` (Tasks 1, 2).
- `src/db/concordance.rs` — `ORDER BY … , sub_line` (Task 2).
- `src/text_file_map.rs` — stage-line raw-text matching in `build_line_map_mode`; test-helper `Line` constructors updated (Tasks 1, 3).
- `src/snapshot.rs`, `src/input/actions/echoes.rs`, `src/ui/translation_overlay.rs` — test/builder `Line` constructors updated for the new field (Task 1).
- `src/input/viewport.rs` — `StageLookup` + DB-first classifier primitives, threaded through `is_dialogue_line`/dialogue helpers and the pagination `is_stage`/`is_dialogue` closures (Tasks 4a–4c).
- `src/app/mod.rs`, `src/input/navigation.rs`, `src/input/scroll.rs` — wire the real `sub_line` lookup at state-bearing nav call sites (Task 4d).
- `src/ui/gloss_overlay.rs` — delete `inject_stage_directions`, its tests, and the call site (Task 5).
- `src/snapshot.rs` — `SNAPSHOT_VERSION` 8 → 9 (Task 6).
- `src/app/mod.rs`, `src/text_file_map.rs` — refresh stale `-Amb` comments + parity regression test (Task 7).

---

### Task 1: `Line.sub_line` field + load it

**Files:**
- Modify: `src/db/models.rs:25-41` (struct), `src/db/queries.rs:108-146` (`load_work` line query + row map)
- Modify (constructors): `src/snapshot.rs:459`, `src/input/actions/echoes.rs:1650`, `src/ui/translation_overlay.rs:401`, `src/text_file_map.rs:1084` and `:1720` (test helpers)
- Test: `src/db/queries.rs` (a `#[cfg(test)]` assertion is impractical without a DB; the field/behavior is covered by Task 3's build_line_map test + the live DB). This task's gate is a clean compile + the explicit `is_dialogue` rule.

**Interfaces:**
- Produces: `Line { …, pub sub_line: i64 }`. For a `line_mapping` row, `sub_line` = the column value; `is_dialogue` is forced `false` when `sub_line > 0`.

- [ ] **Step 1: Add the field to the struct**

In `src/db/models.rs`, add to `Line` (after `line_in_div`, before `is_chapter`):

```rust
    pub line_in_div: i64,
    /// Sub-line within a spoken line: 0 = the spoken line itself; 1..N = stage
    /// directions sharing that line's `line_in_div` (document order). Stage rows
    /// have `speaker=None` and `is_dialogue=false`.
    pub sub_line: i64,
    /// Whether this line is a chapter marker.
    pub is_chapter: bool,
```

- [ ] **Step 2: Build to find every construction site**

Run: `cargo build 2>&1 | rg "missing field|src/.*\.rs:"`
Expected: errors at each `Line { … }` literal lacking `sub_line` — `queries.rs:123`, `snapshot.rs:459`, `echoes.rs:1650`, `translation_overlay.rs:401`, `text_file_map.rs:1084`, `text_file_map.rs:1720`.

- [ ] **Step 3: Load `sub_line` in `load_work` and force stage rows non-dialogue**

In `src/db/queries.rs`, the line query (≈108) selects columns then maps rows (≈123). Add `sub_line` to the SELECT and read it; force `is_dialogue=false` when `sub_line > 0`. Replace the query + the relevant part of the row closure:

```rust
    let mut line_stmt = conn.prepare(
        "SELECT id, canonical_text, normalized_text, speaker, div1, div2, line_in_div, sub_line \
         FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div, sub_line",
    )?;
```

and in the row closure (after reading `line_in_div`):

```rust
            let line_in_div: i64 = row.get(6)?;
            let sub_line: i64 = row.get(7)?;
            let citation = crate::db::models::citation(abbrev, div1, div2, line_in_div);
            Ok(Line {
                id: row.get(0)?,
                citation,
                // A stage direction (sub_line > 0) is never spoken dialogue.
                is_dialogue: sub_line == 0 && line_types::is_dialogue(&text, is_prose),
                text,
                normalized,
                speaker,
                timestamp: None,
                div1,
                div2,
                line_in_div,
                sub_line,
```

(Keep the remaining fields `is_chapter`, `is_spoken`, etc. as they are — just insert the `sub_line` line in the literal.)

- [ ] **Step 4: Add `sub_line` to the other 5 constructors**

Each is a `Line { … }` literal. Add `sub_line: 0,` (these are spoken/test lines) right after the `line_in_div` field:

- `src/snapshot.rs:459` (test fixture): add `sub_line: 0,` after `line_in_div: 1,`.
- `src/input/actions/echoes.rs:1650` (`fn line(...)` test helper): add `sub_line: 0,` after `line_in_div,`.
- `src/ui/translation_overlay.rs:401` (`fn mk(...)` test helper): add `sub_line: 0,` after its `line_in_div`.
- `src/text_file_map.rs:1084` (`make_line` test helper): add `sub_line: 0,` after its `line_in_div`.
- `src/text_file_map.rs:1720` (`make_acc_line` test helper): add `sub_line: 0,` after its `line_in_div`.

- [ ] **Step 5: Build clean**

Run: `cargo build`
Expected: PASS, no `missing field` errors. Warnings unchanged from baseline aside from a possible `field sub_line is never read` until Task 4 reads it — acceptable (clears in Task 4).

- [ ] **Step 6: Commit**

```bash
git add src/db/models.rs src/db/queries.rs src/snapshot.rs src/input/actions/echoes.rs src/ui/translation_overlay.rs src/text_file_map.rs
git commit -m "$(cat <<'EOF'
feat(reader): add Line.sub_line, load it, force stage rows non-dialogue

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 2: ORDER BY sweep — append `, sub_line`

**Files:**
- Modify: `src/db/queries.rs:573`, `:1292`, `:2220`
- Modify: `src/db/concordance.rs:35`

(`queries.rs:112` was already updated in Task 1's `load_work` query.)

**Interfaces:**
- Produces: every `line_mapping`-ordering query returns rows in `(div1,div2,line_in_div,sub_line)` order.

- [ ] **Step 1: Append `, sub_line` at each site**

- `src/db/queries.rs:573` — `ORDER BY div1, div2, line_in_div` → `ORDER BY div1, div2, line_in_div, sub_line`.
- `src/db/queries.rs:1292` — `ORDER BY lm.div1, lm.div2, lm.line_in_div` → `… lm.line_in_div, lm.sub_line`.
- `src/db/queries.rs:2220` — `ORDER BY work_abbrev, div1, div2, line_in_div` → `… line_in_div, sub_line`.
- `src/db/concordance.rs:35` — `ORDER BY lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div` → append `, lm.sub_line`.

- [ ] **Step 2: Confirm no site missed**

Run: `rg 'ORDER BY[^;]*line_in_div' src/`
Expected: every match now ends with `sub_line` (the four above + the `load_work` query from Task 1). Any line-ordering query NOT ending in `sub_line` must be evaluated; chunk/journal queries ordering by `a_line`/div only are out of scope and need no change.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS (SQL string changes only).

- [ ] **Step 4: Commit**

```bash
git add src/db/queries.rs src/db/concordance.rs
git commit -m "$(cat <<'EOF'
feat(reader): order line_mapping queries by sub_line

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 3: `build_line_map` matches stage lines (the linchpin)

**Files:**
- Modify: `src/text_file_map.rs` — `build_line_map_mode`, `MatchMode::WholeLine` arm (the `for buf_idx` loop, ≈287-359)
- Test: `src/text_file_map.rs` (existing `#[cfg(test)] mod tests`, with `make_line`/`make_acc_line` helpers ≈1077-1090)

**Interfaces:**
- Consumes: `Line.sub_line` (Task 1), `crate::db::line_types::is_stage_direction`.
- Produces: a stage `.txt` line gets a `buffer_to_work` / `work_to_buffer` entry pointing at its matching DB stage row.

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `src/text_file_map.rs`. The `make_line(id, text, normalized, is_dialogue)` helper builds a `Line` (it sets `sub_line: 0` after Task 1); build the stage rows by constructing `Line` directly with `sub_line > 0`, or extend a small local helper. Use this self-contained test:

```rust
#[test]
fn stage_lines_map_to_their_db_rows() {
    // .txt has a spoken line, a multi-line stage direction, another stage line,
    // then a spoken line — mirroring 2H6 1.4.43.
    let file_lines: Vec<String> = vec![
        "Lay hands upon these traitors and their trash.".into(),
        "[The Guard arrest Margery Jourdain and her".into(),
        "accomplices and seize their papers.]".into(),
        "[To Jourdain.]".into(),
        "Beldam, I think we watched you at an".into(),
    ];
    // DB rows in (line_in_div, sub_line) order. sub_line>0 are stage rows.
    let mk = |id: i64, text: &str, sub: i64, dialogue: bool| crate::db::models::Line {
        id, citation: String::new(), text: text.into(),
        normalized: super::normalize(text), speaker: None,
        is_dialogue: dialogue, timestamp: None, div1: 1, div2: 4,
        line_in_div: if id < 4 { 43 } else { 44 }, sub_line: sub,
        is_chapter: false, is_spoken: None,
    };
    let work_lines = vec![
        mk(1, "Lay hands upon these traitors and their trash.", 0, true),
        mk(2, "[The Guard arrest Margery Jourdain and her", 1, false),
        mk(3, "accomplices and seize their papers.]", 2, false),
        mk(3, "[To Jourdain.]", 3, false), // id reused only for line_in_div branch; fine for test
        mk(4, "Beldam, I think we watched you at an", 0, true),
    ];
    let lm = super::build_line_map(&file_lines, &work_lines, false);
    // Every buffer line, including the three stage lines, maps to a work row.
    assert_eq!(lm.buffer_to_work[0], Some(0), "spoken line maps");
    assert_eq!(lm.buffer_to_work[1], Some(1), "stage line 1 (multi-line open) maps");
    assert_eq!(lm.buffer_to_work[2], Some(2), "stage line 2 (multi-line close) maps");
    assert_eq!(lm.buffer_to_work[3], Some(3), "[To Jourdain.] maps");
    assert_eq!(lm.buffer_to_work[4], Some(4), "next spoken line maps");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins stage_lines_map_to_their_db_rows`
Expected: FAIL — the stage-line assertions are `None` (normalize() strips brackets → empty → matcher skips them).

- [ ] **Step 3: Add stage matching at the top of the `WholeLine` per-buffer loop**

In `build_line_map_mode`, the `MatchMode::WholeLine` arm has `for buf_idx in 0..n_buf {` then `let nf = &norm_file[buf_idx]; if nf.is_empty() { continue; }`. Insert a stage-line branch BEFORE the `if nf.is_empty()` check so a bracket-stripped-to-empty stage line is handled instead of skipped:

```rust
            for buf_idx in 0..n_buf {
                // Stage directions normalize to empty (brackets stripped), so the
                // spoken-line matcher below skips them. Match a stage .txt line to
                // the next DB stage row (sub_line > 0) by RAW trimmed text — the
                // litdb parser derived stage text from this same folger-cleaned
                // .txt, so it is byte-identical 1:1 (incl. multi-line stage dirs).
                if line_types::is_stage_direction(file_lines[buf_idx].trim()) {
                    let want = file_lines[buf_idx].trim();
                    let window_end = (db_cursor + WINDOW).min(n_work);
                    for wi in db_cursor..window_end {
                        if work_lines[wi].sub_line > 0
                            && work_lines[wi].text.trim() == want
                        {
                            buffer_to_work[buf_idx] = Some(wi);
                            work_to_buffer[wi] = buf_idx;
                            db_cursor = wi + 1;
                            matched += 1;
                            break;
                        }
                    }
                    continue;
                }

                let nf = &norm_file[buf_idx];
                if nf.is_empty() {
                    continue;
                }
```

(The rest of the loop body is unchanged. `db_cursor`, `WINDOW`, `buffer_to_work`, `work_to_buffer`, `matched`, `work_lines`, `file_lines` are all already in scope in this arm.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins stage_lines_map_to_their_db_rows`
Expected: PASS.

- [ ] **Step 5: Run the full pure suite (spoken matching unregressed)**

Run: `cargo test --bins`
Expected: PASS — all existing `text_file_map` tests still green (spoken-line matching path untouched).

- [ ] **Step 6: Commit**

```bash
git add src/text_file_map.rs
git commit -m "$(cat <<'EOF'
feat(reader): map stage .txt lines to their DB sub_line rows in build_line_map

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4 overview: DB-driven stage/dialogue classification

DB-driven nav is entangled: `is_dialogue_line` has ~20 call sites across 5 files,
and the `is_stage` closures live in GTK wrappers (`block_start_for_line`,
`trim_block_atoms`) that take `buffer`, NOT `AppState`. To convert cleanly
without a giant signature churn, we introduce ONE shared lookup type — a
`StageLookup` closure `Fn(usize) -> Option<i64>` returning a buffer line's mapped
`sub_line` (or `None` when unmapped) — and thread it as a single extra parameter.
A `no_stage_lookup()` constant provides the always-`None` form for pure/test
callers (preserving the regex fallback). Task 4 is split into 4a–4d.

**Shadowing caution:** `navigation.rs:1914` and the `nav_test.rs` mod define
their OWN local `is_dialogue_line(&[String], usize)` for pure tests. Do NOT touch
those — they are unrelated to the GTK `viewport::is_dialogue_line`.

---

### Task 4a: `StageLookup` type + DB-first classifier primitives

**Files:**
- Modify: `src/input/viewport.rs` (add helpers near the other `is_*` functions, ≈408)
- Test: `src/input/viewport.rs` (`#[cfg(test)] mod`)

**Interfaces:**
- Consumes: `Line.sub_line` (Task 1).
- Produces:
  - `pub(crate) type StageLookup<'a> = &'a dyn Fn(usize) -> Option<i64>;` — maps a buffer line index to its mapped `sub_line`, or `None` if unmapped.
  - `pub(crate) fn no_stage_lookup() -> StageLookup<'static>` — the always-`None` lookup (regex-only fallback).
  - `pub(crate) fn is_stage_db_first(line_index, text, lookup) -> bool`
  - `pub(crate) fn is_dialogue_db_first(line_index, text, is_prose, lookup) -> bool`

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `src/input/viewport.rs`:

```rust
#[test]
fn db_first_classifiers_prefer_db_then_regex() {
    let stage = |_: usize| Some(2i64);   // mapped stage row
    let spoken = |_: usize| Some(0i64);  // mapped spoken row
    let unmapped = |_: usize| None;

    // is_stage: mapped sub_line>0 => stage regardless of text.
    assert!(super::is_stage_db_first(0, "anything", &stage));
    assert!(!super::is_stage_db_first(0, "[looks bracketed]", &spoken));
    // unmapped => regex fallback.
    assert!(super::is_stage_db_first(0, "[To Jourdain.]", &unmapped));
    assert!(!super::is_stage_db_first(0, "Lay hands.", &unmapped));

    // is_dialogue: mapped stage row => NOT dialogue; mapped spoken => regex on text.
    assert!(!super::is_dialogue_db_first(0, "[To Jourdain.]", false, &stage));
    assert!(super::is_dialogue_db_first(0, "Lay hands.", false, &spoken));
    // unmapped => regex fallback.
    assert!(super::is_dialogue_db_first(0, "Lay hands.", false, &unmapped));
    assert!(!super::is_dialogue_db_first(0, "[To Jourdain.]", false, &unmapped));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins db_first_classifiers_prefer_db_then_regex`
Expected: FAIL — the `is_*_db_first` helpers are not defined.

- [ ] **Step 3: Implement the type + helpers**

Add to `src/input/viewport.rs` (near the other `is_*` helpers; `line_types` is already imported in this module):

```rust
/// Maps a buffer line index to its mapped DB `sub_line` (0 = spoken, >0 = stage
/// direction), or `None` when the buffer line has no mapped work line.
pub(crate) type StageLookup<'a> = &'a dyn Fn(usize) -> Option<i64>;

/// The always-`None` lookup: forces pure regex classification. Used by callers
/// with no `AppState`/`LineMap` in scope (tests, mid-load, no-coverage works).
pub(crate) fn no_stage_lookup() -> StageLookup<'static> {
    &|_| None
}

/// Stage-direction check: prefer the mapped DB row (`sub_line > 0`), else regex.
pub(crate) fn is_stage_db_first(line_index: usize, text: &str, lookup: StageLookup) -> bool {
    match lookup(line_index) {
        Some(sub_line) => sub_line > 0,
        None => line_types::is_stage_direction(text),
    }
}

/// Dialogue check: a mapped stage row (`sub_line > 0`) is never dialogue; a
/// mapped spoken row falls through to the text heuristic (it still distinguishes
/// speaker/separator/blank); an unmapped line uses the text heuristic entirely.
pub(crate) fn is_dialogue_db_first(
    line_index: usize,
    text: &str,
    is_prose: bool,
    lookup: StageLookup,
) -> bool {
    if let Some(sub_line) = lookup(line_index) {
        if sub_line > 0 {
            return false; // stage direction: never dialogue
        }
    }
    line_types::is_dialogue(text, is_prose)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins db_first_classifiers_prefer_db_then_regex`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/viewport.rs
git commit -m "$(cat <<'EOF'
feat(reader): StageLookup + DB-first stage/dialogue classifier primitives

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4b: thread `StageLookup` into `is_dialogue_line` + its helpers

**Files:**
- Modify: `src/input/viewport.rs` — `is_dialogue_line` (≈664), `next_dialogue_line` (≈680), `prev_dialogue_line` (≈699), `next_dialogue_from` (≈723), `last_dialogue_in_page` (≈730ish), and their internal call sites (≈689, 710, 725, 737, 1283)

**Interfaces:**
- Consumes: `StageLookup`, `is_dialogue_db_first` (Task 4a).
- Produces: `is_dialogue_line(buffer, line, lookup: StageLookup)` and the four `*_dialogue_*` helpers each take a trailing `lookup: StageLookup` and forward it.

- [ ] **Step 1: Change `is_dialogue_line` to take a lookup and use `is_dialogue_db_first`**

`is_dialogue_line` (≈664) currently: `pub(crate) fn is_dialogue_line(buffer: &sourceview5::Buffer, line: usize) -> bool { … is_dialogue(trimmed, …) && !is_speaker && !is_stage … }`. Add a trailing `lookup: StageLookup` parameter and route the dialogue decision through `is_dialogue_db_first`, keeping the existing speaker/separator exclusions for the regex path:

```rust
pub(crate) fn is_dialogue_line(buffer: &sourceview5::Buffer, line: usize, lookup: StageLookup) -> bool {
    let text = /* existing: read the line's text */;
    let trimmed = text.trim();
    // DB-first: a mapped stage row is never dialogue; otherwise fall through to
    // the existing text heuristic (which also excludes speaker/separator).
    if let Some(sub_line) = lookup(line) {
        if sub_line > 0 { return false; }
    }
    // (existing body, unchanged:)
    !line_types::is_blank(trimmed)
        && !line_types::is_speaker(trimmed)
        && !line_types::is_stage_direction(trimmed)
        /* …whatever the current body is… */
}
```

(Preserve the exact current body; only add the `lookup` param and the early stage return. Read ≈664-672 first and keep its logic verbatim aside from the inserted check.)

- [ ] **Step 2: Add the trailing `lookup` param to the four helpers and forward it**

`next_dialogue_line`, `prev_dialogue_line`, `next_dialogue_from`, `last_dialogue_in_page` each call `is_dialogue_line(buffer, i)` internally (≈689, 710, 725, 737). Add `lookup: StageLookup` as the last parameter of each, and pass it through: `is_dialogue_line(buffer, i, lookup)`. Also the internal `is_dialogue_line(&state.buffer, l)` at ≈1283 — that function HAS `state`, so build a real lookup there (see Task 4d's lookup-builder) rather than `no_stage_lookup()`.

- [ ] **Step 3: Fix all callers to compile (temporary `no_stage_lookup()`)**

Adding the param breaks ~20 call sites. To keep the build green at THIS task, pass `no_stage_lookup()` at every external caller for now (Task 4d replaces the ones that have `state` with a real lookup). Do NOT touch the shadowing local `is_dialogue_line(&[String], usize)` in `navigation.rs:1914` / `nav_test.rs` — those are different functions.

Run: `rg -n "viewport::is_dialogue_line\(|is_dialogue_line\(&s(tate)?\.buffer" src/`
For each GTK-`viewport` call, append `, crate::input::viewport::no_stage_lookup()`.

- [ ] **Step 4: Build + test**

Run: `cargo build && cargo test --bins`
Expected: PASS (behavior unchanged — every caller passes `no_stage_lookup()`, so classification is still regex-only at this checkpoint).

- [ ] **Step 5: Commit**

```bash
git add src/input/viewport.rs src/app/mod.rs src/input/navigation.rs src/input/scroll.rs src/input/nav_test.rs
git commit -m "$(cat <<'EOF'
refactor(reader): thread StageLookup through is_dialogue_line + helpers

No behavior change yet: all callers pass no_stage_lookup() (regex-only).
Task 4d wires the real lookup at state-bearing call sites.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4c: thread `StageLookup` into the pagination `is_stage` closures

**Files:**
- Modify: `src/input/viewport.rs` — `block_start_for_line` (≈396) and `trim_block_atoms` (≈423) signatures + their `is_stage`/`is_dialogue` closures (≈411-412, ≈439-440); the call site `trim_block_atoms(...)` at ≈592 and any `block_start_for_line(...)` caller

**Interfaces:**
- Consumes: `StageLookup`, `is_stage_db_first`, `is_dialogue_db_first` (Task 4a).
- Produces: `block_start_for_line` and `trim_block_atoms` each take a trailing `lookup: StageLookup` and build their `is_stage`/`is_dialogue` closures through the DB-first helpers.

- [ ] **Step 1: Add `lookup: StageLookup` to both wrapper signatures**

`block_start_for_line(buffer, page_top, last_fit, is_prose)` → add `, lookup: StageLookup`.
`trim_block_atoms(range, page_top, text_view, buffer, is_prose)` → add `, lookup: StageLookup`.

- [ ] **Step 2: Build the `is_stage`/`is_dialogue` closures through the helpers**

In each function, replace:

```rust
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
```

with:

```rust
    let is_stage = |i: usize| is_stage_db_first(i, &line_text(i), lookup);
    let is_dialogue = |i: usize| is_dialogue_db_first(i, &line_text(i), is_prose, lookup);
```

(Leave `is_blank`, `is_speaker`, `is_stanza_number` as regex — out of scope per the spec.)

- [ ] **Step 3: Fix the callers**

The `trim_block_atoms(r, page_top, text_view, buffer, is_prose)` call at ≈592 is inside a function — determine whether that function has `state`/`AppState`. If yes, build the real lookup (Task 4d's builder) and pass it; if not, thread `lookup` down from ITS signature (add the param) up to the nearest state-bearing caller. Same for any `block_start_for_line` caller. Where the chain reaches a pure/test context, pass `no_stage_lookup()`.

Run: `rg -n "block_start_for_line\(|trim_block_atoms\(" src/ | rg -v "_pure|_text|fn |trim_block_atoms_"`
Thread `lookup` through each, terminating at `no_stage_lookup()` in pure contexts or the real lookup where `state` exists.

- [ ] **Step 4: Build + test**

Run: `cargo build && cargo test --bins`
Expected: PASS. Existing `trim_block_atoms_*` / `block_start_for_line` tests still pass — update those test call sites to pass `no_stage_lookup()` (they are pure, no DB).

- [ ] **Step 5: Commit**

```bash
git add src/input/viewport.rs src/app/mod.rs src/input/navigation.rs src/input/scroll.rs
git commit -m "$(cat <<'EOF'
refactor(reader): thread StageLookup into pagination is_stage/is_dialogue closures

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4d: wire the real lookup at state-bearing call sites

**Files:**
- Modify: `src/app/mod.rs`, `src/input/navigation.rs`, `src/input/scroll.rs` — replace `no_stage_lookup()` with a real `sub_line` lookup wherever `state: &AppState` is in scope

**Interfaces:**
- Consumes: `AppState::work_line_for_buffer` (`app/mod.rs:573`), `current_work.lines`, `Line.sub_line`.
- Produces: a closure `|bi: usize| state.work_line_for_buffer(bi).and_then(|wi| state.current_work.as_ref()?.lines.get(wi)).map(|l| l.sub_line)` passed as the `StageLookup` at every call site that has `state`.

- [ ] **Step 1: Define the lookup-builder pattern**

At each state-bearing call site, the real lookup is:

```rust
    let stage_lookup = |bi: usize| {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
```

then pass `&stage_lookup` where the API wants `StageLookup`. (Borrow-checker note: `stage_lookup` borrows `state` immutably; ensure the call doesn't also need `&mut state` simultaneously. If a site holds `&mut state`, compute the needed dialogue/stage indices before the mutable borrow, or reborrow `state` immutably for the closure's lifetime.)

- [ ] **Step 2: Replace `no_stage_lookup()` with the real lookup at state-bearing sites**

Go through the `no_stage_lookup()` placeholders added in 4b/4c. For each whose enclosing function has `state: &AppState` (or `s: &AppState`):
- `src/app/mod.rs:2948`, `:2956` (saved-cursor dialogue snap)
- `src/input/navigation.rs:185, 211, 435, 444, 641, 1388, 1421, 1442`
- `src/input/scroll.rs:394`
- `src/input/viewport.rs:1283`

build a `stage_lookup` (Step 1) and pass `&stage_lookup`. Leave `no_stage_lookup()` only where there is genuinely no `state` (pure helpers / tests).

- [ ] **Step 3: Build + test + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS. The `field sub_line is never read` warning from Task 1 is now cleared (read here and in Task 3). No new clippy warnings. If clippy flags the `type StageLookup` or the `&dyn Fn` boxing, adjust per its suggestion (e.g. `impl Fn` generics on the helpers instead of a `dyn` alias) — keep the same call shape.

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs src/input/navigation.rs src/input/scroll.rs src/input/viewport.rs
git commit -m "$(cat <<'EOF'
feat(reader): wire real sub_line lookup into DB-driven nav classification

Stage rows are now classified non-dialogue from the mapped Line at every
state-bearing nav/pagination site; regex remains the no-mapping fallback.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 5: Delete `inject_stage_directions` (drop the workaround)

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — remove the helper (≈1805), its tests mod (`stage_inject_tests` ≈2190), and the call site in `show_gloss_with_color` (≈605-610)

**Interfaces:**
- Consumes: nothing new — real stage `Line`s now flow through the gloss selection (Tasks 1, 3), so `build_source_header` emits `<stage>` directly.
- Produces: `show_gloss_with_color` renders the passed `gloss` text unchanged (no injection).

- [ ] **Step 1: Remove the call site**

In `src/ui/gloss_overlay.rs` `show_gloss_with_color` (≈605), delete the two injection lines so the function uses its `gloss` parameter directly:

```rust
    pub fn show_gloss_with_color(&self, original: &str, gloss: &str, card_width: i32, card_height: i32, root_color: Option<&str>, source_line_numbers: &[(String, i64)]) {
        // No synopsis label bolding in gloss view.
        self.synopsis_label_ranges.borrow_mut().clear();
```

(Delete the `// Splice in any stage directions…` comment, the `let gloss_injected = …` line, and the `let gloss = gloss_injected.as_str();` line. `original` is now unused by the body — if the compiler warns, rename the parameter back to `_original`.)

- [ ] **Step 2: Remove the helper function**

Delete the entire `fn inject_stage_directions(gloss_text: &str, source_text: &str) -> String { … }` (≈1805 to its closing brace).

- [ ] **Step 3: Remove its tests**

Delete the entire `#[cfg(test)] mod stage_inject_tests { … }` block (≈2190 to its closing brace), including the tests `injects_stage_between_verses`, `no_stage_in_source_is_identity`, and `injects_trailing_stage_after_last_verse`.

- [ ] **Step 4: Build + test + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS — no references to `inject_stage_directions` remain. Confirm with:

Run: `rg -n "inject_stage_directions" src/`
Expected: no output.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
refactor(gloss): drop inject_stage_directions; real DB stage rows render directly

The result/loading cards now receive real stage Lines via the buffer-sourced
selection (Line.sub_line + build_line_map stage matching), so build_source_header
emits <stage> directly. The injection workaround is no longer needed.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 6: Bump `SNAPSHOT_VERSION`

**Files:**
- Modify: `src/snapshot.rs:35`

**Interfaces:**
- Produces: `SNAPSHOT_VERSION = 9`, forcing every work's cached snapshot to regenerate.

- [ ] **Step 1: Bump the constant + document why**

In `src/snapshot.rs`, change `pub const SNAPSHOT_VERSION: u32 = 8;` to `9`, and add a one-line comment above it in the existing version-history comment block:

```rust
// v9: lit.db gained line_mapping.sub_line stage-direction rows; LineMap now
// references stage lines (build_line_map maps them), so the serialized shape and
// buffer_to_work indices changed. Bump forces every work's snapshot to rebuild.
pub const SNAPSHOT_VERSION: u32 = 9;
```

- [ ] **Step 2: Build + test**

Run: `cargo build && cargo test --bins`
Expected: PASS. The existing snapshot test that builds a snapshot at `SNAPSHOT_VERSION` and one at `SNAPSHOT_VERSION + 1` (≈425) still passes (it is relative to the constant).

- [ ] **Step 3: Commit**

```bash
git add src/snapshot.rs
git commit -m "$(cat <<'EOF'
chore(snapshot): bump SNAPSHOT_VERSION to 9 for sub_line stage rows

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 7: Refresh `-Amb` gloss-matching comments + parity test

**Files:**
- Modify: `src/app/mod.rs:~3585` (comment), `src/text_file_map.rs:~1018` (test comment)
- Test: `src/text_file_map.rs` (a base/`-Amb` parity assertion; gated on DB availability like the existing `-Amb` regression test)

**Interfaces:**
- No behavior change. Text-matching for glossed source lines is retained; only the rationale comments are corrected and a parity regression is added.

- [ ] **Step 1: Correct the comment in `apply_reader_gloss_highlighting`**

In `src/app/mod.rs` (≈3585), the comment says `-Amb` editions "renumber lines (it inserts stage directions as numbered rows), so the tuple does not align". That is no longer true (base and `-Amb` are byte-identical; base now also has stage rows). Replace the comment with the current reality, keeping the text-match code:

```rust
    // Match glossed source lines by TEXT (not citation tuple). Base and the
    // production editions (-Amb/-BBC/-DC) are now byte-identical in line_mapping
    // (same div/line/sub_line/text — litdb folger-stage-directions), so a tuple
    // match would also work; text-matching is retained as the edition-robust,
    // harmless choice (it never mismatches identical text). Same approach as
    // jump_to_gloss_source_start.
```

- [ ] **Step 2: Correct the test comment in `text_file_map.rs`**

In the `-Amb` regression test (≈1018), update the doc comment that explains the `-Amb` renumbering rationale to note editions are now byte-identical and the text-match is retained as edition-robust (keep the test itself).

- [ ] **Step 3: Add a base/`-Amb` parity regression test**

Add to the `text_file_map.rs` tests mod (or `queries.rs` if it has DB-gated tests — follow the existing `-Amb`-regression gating pattern, skipping when `lit.db` or the `-Amb` rows are unavailable):

```rust
#[test]
fn base_and_amb_line_mapping_are_parity() {
    // litdb folger-stage-directions made production editions byte-identical to
    // base. Guard against a future divergence: a known passage must resolve to
    // the same (div1,div2,line_in_div,sub_line,text) on 2H6 and 2H6-Amb.
    let conn = match crate::db::queries::open_db() { Ok(c) => c, Err(_) => return };
    let q = "SELECT div1,div2,line_in_div,sub_line,canonical_text FROM line_mapping \
             WHERE work_abbrev=?1 ORDER BY div1,div2,line_in_div,sub_line";
    let rows = |abbrev: &str| -> Vec<(i64,i64,i64,i64,String)> {
        let mut st = conn.prepare(q).unwrap();
        st.query_map([abbrev], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
            .unwrap().filter_map(Result::ok).collect()
    };
    let base = rows("2H6");
    let amb = rows("2H6-Amb");
    if base.is_empty() || amb.is_empty() { return; } // rows unavailable: skip
    assert_eq!(base, amb, "2H6 and 2H6-Amb line_mapping must be byte-identical");
}
```

(Use the project's actual `open_db()` path; if a different DB-test helper exists, mirror it.)

- [ ] **Step 4: Build + test**

Run: `cargo build && cargo test --bins`
Expected: PASS (the parity test passes against the live DB, or skips if rows unavailable).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs src/text_file_map.rs
git commit -m "$(cat <<'EOF'
docs(gloss): refresh -Amb matching rationale; add base/-Amb parity test

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 8: Full verification + user visual gate

**Files:** none (verification only).

- [ ] **Step 1: Full build + pure suite + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: all PASS, no new clippy warnings, `SNAPSHOT_VERSION == 9`, no `inject_stage_directions` references.

- [ ] **Step 2: Ask the user to run the headless / visual checks**

Per the Global Constraints the agent must not launch the app. Ask the user to verify on stage-bearing works (`2H6`, `2H6-Amb`, `Ham`):
1. Stage directions render interleaved + italic in the reading card AND in the gloss overlay (both the `Glossing…` loading card and the result card), sourced from real rows (no injection).
2. `,` `q` `y` `x` `g` `GG` `j` `k` skip stage lines and land on dialogue.
3. Glosses still highlight the correct passage lines on base and `-Amb`.

Provide the commands:

```bash
cd ~/utono/linux-lit && cargo build
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work 2H6
```

and the manual single-work launch from CLAUDE.md Headless Verification for eyeballing the gloss spread on `2H6` 1.4.43–50.

- [ ] **Step 3: Finish the branch**

After the user confirms, follow the repo "Finishing a Branch" convention: merge `feat/gloss-overlay-stage-directions` `--no-ff` to master, re-verify build/tests on the merged result, push, delete the branch.

---

## Self-Review

**Spec coverage:**
- §1 `Line.sub_line` + `is_dialogue=false` for stage → Task 1. ✓
- §2 ORDER BY sweep (5 sites) → Task 1 (`load_work`) + Task 2 (other 4). ✓
- §3 `build_line_map` stage matching (linchpin, raw-text 1:1, multi-line) → Task 3. ✓
- §4 DB-driven nav classification with regex fallback (stage-vs-dialogue only) → Tasks 4a (primitives), 4b (is_dialogue_line threading), 4c (pagination closures), 4d (wire real lookup). ✓
- §5 merge rendering branch + drop `inject_stage_directions` → branch already carries the rendering commits (see Branch section); Task 5 deletes the workaround. ✓
- §6 SNAPSHOT_VERSION 8→9 → Task 6. ✓
- §7 refresh `-Amb` comments + parity test, keep text-match → Task 7. ✓
- §8 verification (unit + headless/visual) → Tasks 3,4,7 unit + Task 8 gate. ✓
- Out-of-scope items (speaker/separator regex, tuple-switch, litdb, delete-coloring bug) — none implemented. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases". Each code step shows complete code. Task 1 Step 4 enumerates all 5 extra constructor sites by file:line. Task 4 Step 5/6 say "confirm the binding name `state`/adapt" — this is a real instruction (the closures live in several functions), not a placeholder; the grep commands make it concrete.

**Type consistency:** `Line.sub_line: i64` defined Task 1, read Tasks 3/4/7. Task 4's shared mechanism is consistent across 4a–4d: `type StageLookup<'a> = &'a dyn Fn(usize) -> Option<i64>`, `no_stage_lookup() -> StageLookup<'static>`, `is_stage_db_first(usize, &str, StageLookup) -> bool`, `is_dialogue_db_first(usize, &str, bool, StageLookup) -> bool` (4a); `is_dialogue_line` and the four `*_dialogue_*` helpers + `block_start_for_line`/`trim_block_atoms` each gain a trailing `lookup: StageLookup` (4b/4c); the real `|bi| state.work_line_for_buffer(bi)…map(|l| l.sub_line)` lookup is passed as `&stage_lookup` (4d). The real `is_dialogue_line` body (viewport.rs:664-677) uses `buffer_line_text` + the speaker/stage/separator exclusion chain — Task 4b preserves it verbatim and only inserts the early stage return. `build_line_map`/`build_line_map_mode` and `work_line_for_buffer` used with their real signatures (verified). `SNAPSHOT_VERSION` matches the verified `= 8` starting value.

**Note on §4 scope:** Tasks 4a–4d convert only the stage/dialogue distinction to DB-driven (per spec); `is_blank`/`is_speaker`/`is_stanza_number`/separator/act-scene classifiers stay regex. The shadowing local `is_dialogue_line(&[String], usize)` in navigation.rs/nav_test.rs test mods is explicitly left untouched.

**Note on Task 4 risk:** 4b/4c add a parameter to widely-called functions; the "pass `no_stage_lookup()` everywhere first, then swap in real lookups (4d)" sequence keeps every intermediate commit green and behavior-preserving, isolating the behavioral change to 4d. Each of 4a–4d is independently testable/reviewable.
