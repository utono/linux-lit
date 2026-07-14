# Chapter Nav From Divisions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Source the reader's chapter nav, gutter sign, and chapter-number logic from `(div1, div2)` divisions instead of the active media's `is_track_mark` flag, so chapters work with no media loaded; and swap the `c` / `Ctrl+c` keys so the structural-chapter toggle gets the easy plain key.

**Architecture:** `Line.is_chapter` keeps its name/type but changes its SOURCE — it now means "this line begins a new `div1` boundary" (chapter in prose, act in plays), computed by a new pure helper `mark_chapter_starts(&mut [Line], is_prose)` called in both load paths. The track-mark column (`line_timestamps.is_track_mark`) stops touching `line.is_chapter`; the track-mark setter (`set_chapter`) and its undo/snapshot path are decoupled to operate on the DB column + their own snapshot state only. The `(`/`&` chapter-jump keys and all other `Line.is_chapter` consumers are unchanged in code and automatically reflect divisions.

**Tech Stack:** Rust, GTK4, rusqlite (SQLite), Pango. Reader source under `~/utono/linux-lit/src`.

## Global Constraints

- **Do not run the app.** Verify with `cargo build`, `cargo test --bins`, `cargo clippy`. The user runs `cargo run` / the e2e harness. (CLAUDE.md)
- **Clippy baseline:** the warning count must not rise above the current baseline of **119** warnings.
- **Authoritative-metadata rule:** chapter/scene boundaries come from `(div1, div2)`, never re-inferred from buffer text (`is_act_scene_marker` / `is_separator`). (CLAUDE.md "Pagination & Scene Boundaries")
- **Keybind changes touch all four surfaces in lockstep:** `keymap_config.rs` (compiled default), `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (stow source; the deployed `~/.config/linux-lit/keymap.json` is a symlink to it, so editing the stow source updates both), the `Ctrl+/` keybinds overlay (`src/ui/keybinds_overlay.rs`), and this plan's relabel steps. The JSON silently overrides compiled defaults — both binding sources MUST agree. (CLAUDE.md)
- **`(` is `parenleft` (the `4` key) and `&` is `ampersand` (the `5` key)** on the RPD layout — not `(`/`)`. (CLAUDE.md "Keyboard Layout")
- **Bump `SNAPSHOT_VERSION`** only if `LineMap`'s serialized shape changes. It does NOT change in this plan (we change how `Line.is_chapter` is sourced at load, not the cached `LineMap` shape), so do not bump it — but verify in Task 7.

---

## File Structure

- `src/db/queries.rs` — `load_work`: replace the `is_track_mark`-based `chapter_map` (lines ~193-201, 223) with a `mark_chapter_starts` call; rename the `Timestamp.is_chapter` field reference. The `is_track_mark` SELECT term (line 146) STAYS (still feeds `Timestamp.is_track_mark`, consumed by the undo/snapshot path). `upsert_chapter` / `restore_timestamp` / `get_timestamp_snapshot` unchanged (already write/read the `is_track_mark` column).
- `src/db/models.rs` — rename `Timestamp.is_chapter` → `Timestamp.is_track_mark` (it holds the track-mark column, not the structural chapter). `Line.is_chapter` keeps its name and doc-comment updated.
- `src/text_file_map.rs` — new pure helper `mark_chapter_starts(&mut [Line], is_prose)` + unit tests, placed beside `build_section_starts` (which already walks `(div1,div2)` boundaries on buffer lines; this helper walks work `Line`s).
- `src/input/actions/pickers.rs` — reload path (lines ~529-550): drop the `chapter_set` from `ts.is_track_mark`; call `mark_chapter_starts` after re-attaching timestamps.
- `src/input/timestamps.rs` — decouple `set_chapter` (track-mark setter) and `undo_timestamp` from `line.is_chapter`: stop reading/writing `line.is_chapter`; track the track-mark state via `TimestampSnapshot.is_track_mark` and the DB only. Rename `TimestampSnapshot.is_chapter` → `is_track_mark`. The sign-column update no longer writes the `is_chapter_line` sign for a track mark.
- `src/input/keymap_config.rs` — swap plain `c` ↔ `Ctrl+c` actions.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — swap the same two binds.
- `src/ui/keybinds_overlay.rs` — relabel plain `c` / `Ctrl+c` cap + `describe()` arms; update `(`/`&` help; via the `update-cairo-keybinds-overlay` skill.
- `~/utono/litdb/.claude/commands/litdb/timestamps-signs.md` — sign-name doc update (litdb side, lockstep) — Task 9.

---

## Task 1: `mark_chapter_starts` pure helper + tests

The testable core. A new pure function over `&mut [Line]`, separate from `build_section_starts` (which is buffer-line / file-line oriented). Lives in `src/text_file_map.rs` beside `build_section_starts`.

**Files:**
- Modify: `src/text_file_map.rs` (add the helper near `build_section_starts`, ~line 678; add a `#[cfg(test)]` module if none covers it)
- Test: same file (`#[cfg(test)] mod mark_chapter_starts_tests`)

**Interfaces:**
- Consumes: `crate::db::models::Line` (has `div1: i64`, `is_chapter: bool`).
- Produces: `pub(crate) fn mark_chapter_starts(lines: &mut [Line], is_prose: bool)` — sets `is_chapter = true` on the first line of each `div1` boundary. Prose: each `div1 > 0` (front matter `div1 = 0` is not a chapter). Non-prose: each change of `div1` from the previous line. `lines` MUST be in canonical `(div1, div2, line_in_div, sub_line)` order. Clears `is_chapter` to false on every other line (idempotent — safe to call on already-flagged lines in the reload path).

- [ ] **Step 1: Write the failing tests**

Add to `src/text_file_map.rs` (use a `Line` builder local to the test that fills only `div1`; other fields get defaults via a helper). Place near the existing tests at the bottom of the file:

```rust
#[cfg(test)]
mod mark_chapter_starts_tests {
    use super::mark_chapter_starts;
    use crate::db::models::Line;

    /// Minimal Line with only div1 set; everything else defaulted.
    fn line(div1: i64) -> Line {
        Line {
            id: 0,
            citation: String::new(),
            text: String::new(),
            normalized: String::new(),
            speaker: None,
            is_dialogue: false,
            timestamp: None,
            div1,
            div2: 0,
            line_in_div: 0,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    fn flags(lines: &[Line]) -> Vec<bool> {
        lines.iter().map(|l| l.is_chapter).collect()
    }

    #[test]
    fn prose_skips_front_matter_marks_each_div1() {
        // div1: 0,0,1,1,2,2 -> chapter at first 1 and first 2, NOT front matter.
        let mut lines = vec![line(0), line(0), line(1), line(1), line(2), line(2)];
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![false, false, true, false, true, false]);
    }

    #[test]
    fn play_marks_each_div1_change_including_first() {
        // div1: 1,1,2,2 -> chapter at first 1 and first 2 (non-prose: any change,
        // and the first mapped line is a change from "no previous").
        let mut lines = vec![line(1), line(1), line(2), line(2)];
        mark_chapter_starts(&mut lines, false);
        assert_eq!(flags(&lines), vec![true, false, true, false]);
    }

    #[test]
    fn prose_first_div1_is_one_marks_first_line() {
        // No front matter: div1 1,1,2 -> first 1 is a chapter.
        let mut lines = vec![line(1), line(1), line(2)];
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![true, false, true]);
    }

    #[test]
    fn empty_input_is_noop() {
        let mut lines: Vec<Line> = vec![];
        mark_chapter_starts(&mut lines, true);
        assert!(lines.is_empty());
    }

    #[test]
    fn single_div1_zero_prose_no_chapter() {
        let mut lines = vec![line(0)];
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![false]);
    }

    #[test]
    fn single_div1_zero_nonprose_is_chapter() {
        // Non-prose treats the first mapped line as a div1 boundary regardless of value.
        let mut lines = vec![line(0)];
        mark_chapter_starts(&mut lines, false);
        assert_eq!(flags(&lines), vec![true]);
    }

    #[test]
    fn clears_stale_flags_idempotent() {
        // Reload path may call on lines with stale true flags; helper must reset.
        let mut lines = vec![line(0), line(1)];
        lines[0].is_chapter = true; // stale
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![false, true]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bins mark_chapter_starts_tests 2>&1 | tail -20`
Expected: FAIL — `cannot find function mark_chapter_starts in this scope` (helper not defined yet).

- [ ] **Step 3: Write the helper**

Add to `src/text_file_map.rs`, immediately above `fn build_section_starts(` (~line 678):

```rust
/// Set `is_chapter = true` on the first line of each div1 boundary, clearing it
/// elsewhere (idempotent — safe to re-call on already-flagged lines).
///
/// Prose: each `div1 > 0` (front matter `div1 == 0` is not a chapter).
/// Non-prose: each change of `div1` from the previous line (the first mapped
/// line always counts, as a change from "no previous").
///
/// `lines` MUST be in canonical (div1, div2, line_in_div, sub_line) order — the
/// same order `load_work` SELECTs them in.
pub(crate) fn mark_chapter_starts(lines: &mut [crate::db::models::Line], is_prose: bool) {
    let mut prev_div1: Option<i64> = None;
    for line in lines.iter_mut() {
        let is_start = if is_prose {
            // A new div1 boundary where div1 > 0; front matter (0) never counts.
            line.div1 > 0 && Some(line.div1) != prev_div1
        } else {
            // Any change of div1 (including the first mapped line).
            Some(line.div1) != prev_div1
        };
        line.is_chapter = is_start;
        prev_div1 = Some(line.div1);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bins mark_chapter_starts_tests 2>&1 | tail -20`
Expected: PASS — all 7 tests `ok`.

- [ ] **Step 5: Commit**

```bash
git add src/text_file_map.rs
git commit -m "feat(chapters): add mark_chapter_starts div1-boundary helper

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 2: Rename `Timestamp.is_chapter` → `Timestamp.is_track_mark`

The struct field holds the `line_timestamps.is_track_mark` column. Rename it so it stops masquerading as the structural chapter. Same for `TimestampSnapshot.is_chapter` (it also holds the track-mark column, read by `get_timestamp_snapshot`).

This is a mechanical rename across the field's read/write sites. After it, the compiler points at every consumer — which is the safety net for Tasks 3-5.

**Files:**
- Modify: `src/db/models.rs:65` (`Timestamp.is_chapter` → `is_track_mark`)
- Modify: `src/input/timestamps.rs:11` (`TimestampSnapshot.is_chapter` → `is_track_mark`)
- Modify: `src/db/queries.rs:161` (the `Timestamp { ... is_chapter: ... }` initializer), `src/db/queries.rs:1441` (`TimestampSnapshot { ... is_chapter: ... }` in `get_timestamp_snapshot`)

**Interfaces:**
- Produces: `Timestamp.is_track_mark: bool` and `TimestampSnapshot.is_track_mark: bool`. Tasks 3-5 reference these new names.

- [ ] **Step 1: Rename the struct fields**

In `src/db/models.rs`, change line 65:

```rust
    pub is_track_mark: bool,
```

In `src/input/timestamps.rs`, change line 11:

```rust
    pub is_track_mark: bool,
```

- [ ] **Step 2: Update the two initializers**

In `src/db/queries.rs` `load_work` (~line 161), inside the `Timestamp { ... }` map closure, rename the field:

```rust
                is_track_mark: row.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
```

In `src/db/queries.rs` `get_timestamp_snapshot` (~line 1441), inside the `TimestampSnapshot { ... }`:

```rust
            is_track_mark: row.get::<_, bool>(3).unwrap_or(false),
```

- [ ] **Step 3: Build to find all remaining references**

Run: `cargo build 2>&1 | rg "is_chapter" | rg "Timestamp|snap\.|ts\.|other\.|\.is_chapter" ; cargo build 2>&1 | tail -30`
Expected: compile errors at the remaining `ts.is_chapter` / `snap.is_chapter` sites that Tasks 3-5 will fix:
- `src/db/queries.rs:197` (`ts.is_chapter` in the chapter_map — Task 4 deletes it)
- `src/input/actions/pickers.rs:542` (`ts.is_chapter` in chapter_set — Task 5 deletes it)
- `src/input/timestamps.rs:308` (`other.is_chapter` — Task 3), `:555`, `:605` (`snap.is_chapter` — Task 3)

These are EXPECTED failures resolved by the next tasks. Do not fix them here beyond confirming the list matches. (If extra unexpected sites appear, note them — they are additional consumers to handle.)

- [ ] **Step 4: Commit (WIP — build is intentionally red)**

> Note: build is red until Task 5. If your workflow forbids red commits, fold Tasks 2-5 into one commit at the end of Task 5 instead. The steps stay separate either way.

```bash
git add src/db/models.rs src/input/timestamps.rs src/db/queries.rs
git commit -m "refactor(ts): rename Timestamp/TimestampSnapshot is_chapter -> is_track_mark (WIP)

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 3: Decouple the track-mark setter + undo from `line.is_chapter`

`set_chapter` (the `c`/track-mark setter) currently uses `line.is_chapter` as a live proxy for the track-mark state: it READS it for a ±10s nearby-rejection (lines 305-320) and WRITES it (line 347). `undo_timestamp` restores it (lines 589, 605, 611). After the repoint, `line.is_chapter` is the STRUCTURAL flag — the setter must not touch it. The track-mark truth is the DB column (`is_track_mark`), surfaced via `Timestamp.is_track_mark` / `TimestampSnapshot.is_track_mark`.

**Files:**
- Modify: `src/input/timestamps.rs` — `set_chapter` (~287-360), `undo_timestamp` (~523-end)

**Interfaces:**
- Consumes: `Timestamp.is_track_mark`, `TimestampSnapshot.is_track_mark` (from Task 2), `upsert_chapter` (returns the new track-mark bool), `restore_timestamp(.., is_track_mark)`.
- Produces: `set_chapter` and `undo_timestamp` no longer read or write `Line.is_chapter`.

- [ ] **Step 1: Fix the nearby-rejection read in `set_chapter`**

In `src/input/timestamps.rs`, the block at ~lines 303-322 reads `line.is_chapter` and `other.is_chapter` to reject a track mark within 10s of an existing track mark. Replace the use of the structural `is_chapter` with the track-mark state from the line's `timestamp` / the loaded `work.timestamps`. The check is "is this line already a track mark, and is there another track-marked line within 10s". Rewrite the block:

```rust
    // If not already a track mark, reject if another track mark sits within ±10s.
    {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        let already = work
            .timestamps
            .iter()
            .any(|t| t.line_id == work.lines[line_idx].id
                && t.media_id == media_id
                && t.is_track_mark);
        if !already {
            let nearby = work.timestamps.iter().any(|t| {
                t.media_id == media_id
                    && t.is_track_mark
                    && t.line_id != work.lines[line_idx].id
                    && (t.start - time_pos).abs() <= 10.0
            });
            if nearby {
                crate::logging::log(&format!(
                    "TS: track mark rejected — another within 10s of {:.2}",
                    time_pos,
                ));
                return false;
            }
        }
    }
```

(Note: `work.timestamps` is the full per-work timestamp vec loaded by `load_work`; it carries `is_track_mark` and `start`. This replaces the old `line.is_chapter` + `other.timestamp` scan.)

- [ ] **Step 2: Stop writing `line.is_chapter` in `set_chapter`; fix the sign update**

At ~line 347 `line.is_chapter = new_val;` — DELETE this line. The structural flag must not be touched by the track-mark setter.

At ~line 349 `let is_ch = state.current_work...lines[line_idx].is_chapter` — replace with the track-mark return value `new_val` (rename the local to `is_tm`):

```rust
    let is_tm = new_val;
    crate::logging::log(&format!("TS: toggle track mark is_track_mark={} start_time={:.2} line={}", is_tm, time_pos, line_idx));

    resync_mpv_timestamps(state);

    // Update sign column for this line. The track-mark setter must NOT touch the
    // structural is_chapter sign (that follows divisions now) — pass None.
    let buffer_line = state.current_line;
    set_sign_columns(state, buffer_line, true, true, None);
    redraw_sign_gutters(state);

    true
}
```

(Passing `None` to `set_sign_columns`'s `is_chapter` param leaves `is_chapter_line` untouched — confirmed by its doc at line 234-236. If a dedicated track-mark sign is wanted, that is Task 9's sign audit, out of this task.)

- [ ] **Step 3: Stop writing `line.is_chapter` in `undo_timestamp`**

In `undo_timestamp` (~lines 555, 586-611), the restore path writes `line.is_chapter`. Since `set_chapter` no longer changes `line.is_chapter`, undo must not either.

- Line ~555: `snap.is_chapter,` → `snap.is_track_mark,` (the `restore_timestamp` arg is the track-mark column — name updated in Task 2).
- Line ~589 `line.is_chapter = false;` — DELETE (the `None` previous branch).
- Line ~605 `line.is_chapter = snap.is_chapter;` — DELETE.
- Line ~611 `let is_ch = line.is_chapter;` — replace the value with the track-mark snapshot state for the sign update; rename to `is_tm`:

```rust
        let is_tm = match &undo.previous {
            Some(snap) => snap.is_track_mark,
            None => false,
        };
        let work_idx = work.lines.iter().position(|l| l.id == undo.line_mapping_id);
        (work_idx, has_ts, is_man, is_tm)
    };
```

Then where the tuple is destructured / used for `set_sign_columns` below the block, pass `None` for the `is_chapter` sign param (same rationale as Step 2 — undo of a track mark must not move the structural sign). Find the `set_sign_columns(...)` call in the tail of `undo_timestamp` and change its last argument to `None`; the `is_tm` value is only logged, not written to the structural sign.

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | tail -30`
Expected: the `timestamps.rs` `is_chapter` errors from Task 2 are gone. Remaining errors only at `queries.rs:197` and `pickers.rs:542` (Tasks 4-5).

- [ ] **Step 5: Commit (WIP — still red until Task 5)**

```bash
git add src/input/timestamps.rs
git commit -m "refactor(ts): decouple track-mark setter+undo from Line.is_chapter

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 4: Repoint `load_work` to div1 boundaries

Replace the `is_track_mark`-based `chapter_map` with `mark_chapter_starts`. Keep the `is_track_mark` SELECT term (still feeds `Timestamp.is_track_mark` for the undo path).

**Files:**
- Modify: `src/db/queries.rs` `load_work` — delete the `chapter_map` block (~193-201); change the per-line `is_chapter` assignment (~223); call `mark_chapter_starts` after assembling `lines`.

**Interfaces:**
- Consumes: `crate::text_file_map::mark_chapter_starts` (Task 1), `is_prose` (already computed at line 106).

- [ ] **Step 1: Delete the `chapter_map` block**

In `src/db/queries.rs`, DELETE lines ~193-201 (the `// 5b. Build chapter lookup ...` block):

```rust
    // 5b. Build chapter lookup from already-loaded timestamps (no extra DB query)
    let mut chapter_map: HashMap<i64, bool> = HashMap::new();
    if let Some(mid) = media_id {
        for ts in &timestamps {
            if ts.media_id == mid && ts.is_track_mark {
                chapter_map.insert(ts.line_id, true);
            }
        }
    }
```

- [ ] **Step 2: Drop the per-line `is_chapter` assignment from the attach map**

In the `// 6. Attach timestamps ...` map (~218-227), REMOVE the `line.is_chapter = chapter_map.contains_key(&line.id);` line so the closure becomes:

```rust
    // 6. Attach timestamps and spoken status to lines
    let mut lines: Vec<Line> = lines
        .into_iter()
        .map(|mut line| {
            line.timestamp = ts_map.get(&line.id).copied();
            line.is_spoken = spoken_map.get(&line.id).copied();
            line
        })
        .collect();

    // 6b. Mark structural chapter starts from div1 boundaries (media-independent).
    crate::text_file_map::mark_chapter_starts(&mut lines, is_prose);
```

(Note `let lines` becomes `let mut lines` so the helper can mutate it.)

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -30`
Expected: only the `pickers.rs:542` error from Task 2 remains. (`queries.rs` is now clean; `chapter_map` and the `ts.is_track_mark` read in 5b are gone — the SELECT term at line 146 stays.)

- [ ] **Step 4: Commit (WIP — red until Task 5)**

```bash
git add src/db/queries.rs
git commit -m "refactor(db): source load_work Line.is_chapter from div1 boundaries

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 5: Repoint the `pickers.rs` reload path to div1 boundaries

The media-switch reload (`confirm_media`) rebuilds per-line timestamps and currently sets `line.is_chapter` from a `chapter_set` of `is_track_mark` timestamps. Switch it to `mark_chapter_starts` so switching media never changes the structural chapter flags.

**Files:**
- Modify: `src/input/actions/pickers.rs` (~529-550)

**Interfaces:**
- Consumes: `crate::text_file_map::mark_chapter_starts`, the work's `work_type` (for `is_prose`).

- [ ] **Step 1: Drop the `chapter_set` and re-mark from div1**

In `src/input/actions/pickers.rs`, the block at ~529-550 currently builds `chapter_set` and sets `line.is_chapter = chapter_set.contains(&line.id)`. Replace it so it only rebuilds `ts_map`, then calls the helper:

```rust
                if let Some(ref mut work) = s.current_work {
                    // Build the timestamp lookup from work.timestamps for the new media.
                    let mut ts_map: std::collections::HashMap<i64, crate::db::models::TimeRange> =
                        std::collections::HashMap::new();
                    for ts in &work.timestamps {
                        if ts.media_id == media_id {
                            ts_map.entry(ts.line_id).or_insert(crate::db::models::TimeRange {
                                start: ts.start,
                                end: ts.end,
                                sentence_start: ts.sentence_start,
                                is_manual: ts.is_manual,
                            });
                        }
                    }
                    for line in &mut work.lines {
                        line.timestamp = ts_map.get(&line.id).copied();
                    }
                    // Structural chapter flags follow div1 boundaries, NOT the new
                    // media's track marks — re-mark so switching media never moves them.
                    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
                    crate::text_file_map::mark_chapter_starts(&mut work.lines, is_prose);
                    // Re-send timestamps to MPV
                    let mut ts_data: Vec<(i64, f64, f64)> = work
```

(The `chapter_set` `HashSet` declaration and its `if ts.is_track_mark { chapter_set.insert(...) }` are removed; the rest of the block — the MPV `SetTimestamps` send — is unchanged.)

- [ ] **Step 2: Build clean**

Run: `cargo build 2>&1 | tail -30`
Expected: SUCCESS (0 errors). The whole repoint now compiles.

- [ ] **Step 3: Run the full bin test suite**

Run: `cargo test --bins 2>&1 | tail -20`
Expected: PASS, including the Task 1 `mark_chapter_starts_tests`.

- [ ] **Step 4: Clippy within baseline**

Run: `cargo clippy 2>&1 | rg "warning:" | wc -l`
Expected: ≤ 119.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/pickers.rs
git commit -m "refactor(pickers): source reload Line.is_chapter from div1 boundaries

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 6: Swap the `c` and `Ctrl+c` keybinds

Plain `c` → `ToggleChapterStart` (structural; the only thing affecting nav/sign). `Ctrl+c` → `SetChapter` (track mark; export-only). Update both binding sources in lockstep.

**Files:**
- Modify: `src/input/keymap_config.rs:236` and `:325`
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (lines for `c` / `Ctrl+c`)

**Interfaces:**
- No code interfaces change; `Action::SetChapter` and `Action::ToggleChapterStart` already exist and dispatch correctly (`keymap.rs:2345`, `chapters.rs`).

- [ ] **Step 1: Swap in the compiled defaults**

In `src/input/keymap_config.rs`, change line 236 (in `bookmark`/chapter group) from `ToggleChapterStart` on `Ctrl+c` to `SetChapter`:

```rust
        (KeyCombo::ctrl("c"), Action::SetChapter),
```

and change line 325 (in `timestamp_bindings()`) from `SetChapter` on plain `c` to `ToggleChapterStart`:

```rust
        (KeyCombo::plain("c"), Action::ToggleChapterStart),
```

- [ ] **Step 2: Swap in the stow keymap.json**

In `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, swap the two entries:

```json
    {"key": "c", "action": "ToggleChapterStart"},
    {"key": "c", "ctrl": true, "action": "SetChapter"},
```

(The deployed `~/.config/linux-lit/keymap.json` is a symlink to this file — no separate edit or re-stow needed. Confirm with `readlink ~/.config/linux-lit/keymap.json`.)

- [ ] **Step 3: Build + verify the binds resolve**

Run: `cargo build 2>&1 | tail -5`
Expected: SUCCESS.

Run: `rg -n 'plain\("c"\)|ctrl\("c"\)' src/input/keymap_config.rs`
Expected: plain `c` → `ToggleChapterStart`, `ctrl("c")` → `SetChapter`.

Run: `rg -n '"key": "c"' ~/.config/linux-lit/keymap.json`
Expected: matches the swapped JSON (read through the symlink).

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap_config.rs
git commit -m "feat(keymap): swap c<->Ctrl+c — plain c toggles structural chapter

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

(The keymap.json lives in `~/tty-dotfiles` — commit it there separately if the user wants the stow change tracked. Note it in the handoff; do not auto-commit another repo without asking.)

---

## Task 7: Verify consumers + snapshot version unchanged

No-code task: confirm the unchanged consumers behave and nothing else needs touching. Each step is a verification, not an edit.

**Files:** none modified (verification only).

- [ ] **Step 1: Confirm `Line.is_chapter` consumers are unchanged and still compile**

Run: `rg -n "\.is_chapter\b" src/input/navigation.rs src/app/scene_synopsis.rs src/gutter.rs src/app/mod.rs src/text_file_map.rs src/input/actions/pickers.rs`
Expected: these read `Line.is_chapter` (nav-jump, scene_synopsis, gutter sign map at `app/mod.rs:3299`, pickers gutter at `pickers.rs:614`) — all unchanged, now reflecting divisions. No `Timestamp`/`ts.`/`snap.` `.is_chapter` left anywhere:

Run: `rg -n "ts\.is_chapter|snap\.is_chapter|other\.is_chapter|Timestamp.*is_chapter|\.is_chapter:" src/`
Expected: NO matches (all renamed to `is_track_mark` or deleted).

- [ ] **Step 2: Confirm `SNAPSHOT_VERSION` does NOT need a bump**

Run: `rg -n "SNAPSHOT_VERSION" src/snapshot.rs`
Read the serialized `LineMap` shape. `mark_chapter_starts` runs at `load_work` time on `Vec<Line>` (not part of the cached `LineMap` buffer mapping), and `is_chapter` is a `Line` field, not a `LineMap` field. Confirm the cached struct shape is unchanged → no bump.

> If (and only if) the snapshot caches `Line.is_chapter` such that a stale cache would show old (media-derived) chapters: bump `SNAPSHOT_VERSION` and note it. Verify by checking whether `snapshot.rs` serializes `Line.is_chapter`. If unsure, the safe move is to bump — a stale chapter map is the documented failure mode (see memory `project_reader_gloss_sd_stale_snapshot`).

- [ ] **Step 3: Full build + bin tests + clippy**

Run: `cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -10 && cargo clippy 2>&1 | rg -c "warning:"`
Expected: build OK, tests PASS, clippy count ≤ 119.

- [ ] **Step 4: Commit (only if Step 2 required a snapshot bump; else skip)**

```bash
git add src/snapshot.rs
git commit -m "chore(snapshot): bump SNAPSHOT_VERSION for div1-sourced is_chapter

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 8: Relabel the `Ctrl+/` keybinds overlay

Update the cap strip + detail `describe()` arms for plain `c`, `Ctrl+c`, and the `(`/`&` help. The overlay is hand-maintained with no compile-time enforcement; use the skill that carries the exhaustive cross-reference.

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (lines ~59, ~526, ~691, and the `(`/`&` rows/describe arms)

- [ ] **Step 1: Invoke the overlay skill**

Use the `update-cairo-keybinds-overlay` skill. Give it this change set:
- **plain `c`**: cap label "set chapter" → "toggle ch start"; its `describe()` long help → "Toggle whether this line begins a structural chapter (a `(div1,div2)` division boundary) — what `(`/`&` jump between and what the gutter chapter sign marks. -> chapters::toggle_chapter_start — src/input/actions/chapters.rs".
- **`Ctrl+c`** (the `("C-c", ...)` modifier entry on the `c` key row): label "toggle ch start" → "set track mark"; its `describe()` arm → "Set an audio track mark on this line at MPV's current position (export metadata for ffmpeg chapter embedding), distinct from the structural chapter that plain c toggles. -> timestamps::set_chapter — src/input/timestamps.rs".
- **`(` (`parenleft`) / `&` (`ampersand`)**: their describe help → "Jump to previous/next chapter — a `div1` boundary (chapter in prose, act in a play), independent of any loaded audio. -> navigation::jump_to_prev_chapter / jump_to_next_chapter — src/input/navigation.rs".

- [ ] **Step 2: Update the cap-strip entry**

In `src/ui/keybinds_overlay.rs` line ~59, change:

```rust
    key("c", "C", "toggle ch start", "C: show chapter", &[("C-c", "set track mark")]),
```

- [ ] **Step 3: Update the `describe()` arms**

In `src/ui/keybinds_overlay.rs` (~520-535), update/add arms. Replace the `"set chapter"` arm and the `"toggle ch start"` arm so the descriptions match the new key assignment (the long-help text above). Add a `"set track mark"` arm. Ensure the short-help map (~691) gets a `"set track mark" => "set audio track mark"` entry and the `"toggle ch start"` short help reads "toggle structural chapter". Confirm `(`/`&` describe arms reflect the div1 wording.

- [ ] **Step 4: Run the skill's three-pass cross-reference**

The skill mandates three passes: (1) no blank detail slot hides a real binding; (2) no label names the wrong action; (3) every label has a `describe()` arm. Run them. Confirm plain `c`, `Ctrl+c`, `(`, `&` each render a correct, non-blank detail row naming the right action + `src/` path.

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: SUCCESS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): relabel c=structural chapter, Ctrl+c=track mark; (/& = div1 jump

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

---

## Task 9: Gutter sign-name audit + litdb sign doc (lockstep)

Audit the gutter sign-type names against post-repoint behavior and rename only the signs whose meaning changed; update the litdb sign doc in lockstep.

**Files:**
- Modify (if any sign changed meaning): `src/input/timestamps.rs` sign columns + `src/gutter.rs`
- Modify: `~/utono/litdb/.claude/commands/litdb/timestamps-signs.md`

- [ ] **Step 1: Enumerate the signs and classify**

Run: `rg -n "lit_signs_chapter|chapter_a|chapter_b|chapter_loop|\"chapter\"|is_chapter_line" src/input/timestamps.rs src/gutter.rs`
For each sign, decide:
- **The chapter sign marking a division-boundary line** (driven by `Line.is_chapter`): KEEP "chapter" — it is now correctly a structural chapter. After Task 3, `set_sign_columns(.., None)` means the track-mark setter no longer writes this sign; the structural sign is written from the div1-sourced `is_chapter_line` map (verify the gutter map at `app/mod.rs:3299` / `pickers.rs:614` still populates `is_chapter_line` from `Line.is_chapter` — it does, unchanged).
- **Any sign that specifically reflected the audio track mark** (A/B-loop status on a track-marked line): rename to a `track_mark`-based name.

- [ ] **Step 2: Rename only the changed signs (if any)**

Apply renames in `src/` for signs whose meaning is now "track mark". If the audit finds none (the only chapter sign is the structural one, and there is no separate audio-track-mark sign), record "no sign rename needed" and skip to Step 3. Do not rename signs whose meaning is unchanged.

- [ ] **Step 3: Update the litdb sign doc**

Edit `~/utono/litdb/.claude/commands/litdb/timestamps-signs.md`: where a sign name changed in Step 2, update it; note that the gutter "chapter" sign now reflects `(div1,div2)` divisions, not the audio track mark. (`media_manager.py`'s stdout label was already renamed to `track_marks` in the litdb rename pass — no action there.)

- [ ] **Step 4: Build + verify**

Run: `cargo build 2>&1 | tail -5 && cargo clippy 2>&1 | rg -c "warning:"`
Expected: SUCCESS, clippy ≤ 119.

- [ ] **Step 5: Commit**

```bash
git add src/input/timestamps.rs src/gutter.rs
git commit -m "refactor(signs): audit gutter chapter sign for div1 source; rename track-mark signs

$(printf '\n')Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_016ocfuc9mU952c1RgVKJLpM"
```

(The litdb doc lives in `~/utono/litdb` — commit it in that repo separately; flag in handoff, don't auto-commit another repo without asking.)

---

## Task 10: Final verification + handoff for live/e2e check

The acceptance criteria here are partly visual ("chapters render with no media; `(`/`&` jump act-to-act"). Per CLAUDE.md, the agent cannot launch cage from the live session — surface what's verified and hand the visual check to the user.

**Files:** none.

- [ ] **Step 1: Full green run**

Run: `cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -8 && cargo clippy 2>&1 | rg -c "warning:"`
Expected: build OK, all bin tests PASS, clippy ≤ 119.

- [ ] **Step 2: Grep audit — no audio flag drives chapters anymore**

Run: `rg -n "is_track_mark" src/`
Expected ONLY: the `load_work` SELECT term (`queries.rs:146`), the `Timestamp` initializer (`queries.rs:161`), `upsert_chapter` / `restore_timestamp` / `get_timestamp_snapshot` (the DB write/read of the column), and the `is_track_mark` struct fields. NO `is_track_mark` feeding `Line.is_chapter` or any nav/sign decision.

Run: `rg -n "chapter_map|chapter_set" src/`
Expected: NO matches (both deleted).

- [ ] **Step 3: Write the integration check as a data-gated bin test (optional but recommended)**

If `~/utono/litdb/data/lit.db` is present, add a `#[cfg_attr(not(feature = "db-tests"), ignore)]` or `#[ignore]` test in `src/db/queries.rs`'s test module (or `tests/`) that loads a divided prose work (e.g. `Cromwell`) WITH NO MEDIA and asserts: count of `Line.is_chapter == true` equals the number of distinct `div1 > 0` values, and the first line of each `div1` is flagged. Load a play and assert chapter starts fall on `div1` (act) boundaries. Gate it so a bare `cargo test --bins` stays green without the DB.

Run: `cargo test --bins -- --ignored chapter_from_div 2>&1 | tail -15` (only if DB present)
Expected: PASS, or skipped if DB absent.

- [ ] **Step 4: Hand off the visual check to the user**

State plainly that runtime/visual verification is blocked from the agent shell (live dwl owns the seat). Ask the user to run, with NO audio loaded:

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

and to eyeball a divided prose work (Cromwell) + a play:

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

then in the reader confirm: gutter chapter signs render at chapter boundaries with no media; `(` / `&` jump chapter-to-chapter (prose) and act-to-act (play); plain `c` toggles a structural chapter (reloads in place); `Ctrl+c` sets an audio track mark only when media is loaded (no effect on nav/sign).

- [ ] **Step 5: Finish the branch (after user confirms the visual check)**

Per CLAUDE.md "Finishing a Branch": merge `--no-ff` to `master`, re-verify, push, delete the feature branch. Do NOT do this until the user confirms the visual check passes. Remember the `~/tty-dotfiles` keymap.json and `~/utono/litdb` doc are separate repos — commit/push them in their own repos.

---

## Self-Review

**1. Spec coverage:**
- "Repoint chapter nav/sign/number to div1 for all work types" → Tasks 1, 4, 5 (helper + both load paths); consumers verified in Task 7.
- "`mark_chapter_starts` pure helper, prose vs non-prose, front-matter rule" → Task 1 (with the exact prose/non-prose cases the spec's Testing section lists).
- "Drop the `is_track_mark`-based chapter_map / chapter_set" → Tasks 4, 5.
- "Dead `Timestamp.is_chapter` field / SELECT term — verify with rg" → **Corrected in plan:** the field is NOT dead (undo/snapshot + setter consume it). Plan keeps the SELECT + field, renames the field to `is_track_mark` (Task 2), and decouples the setter/undo (Task 3). This is a deliberate, documented deviation from the spec's "remove them too" — surfaced because the spec said "unless a remaining consumer is found — verify with rg," and consumers WERE found.
- "Swap `c`/`Ctrl+c`, plain c = structural" → Task 6 (both binding sources).
- "Relabel overlay; `(`/`&` help" → Task 8.
- "Gutter sign-name audit + litdb doc lockstep" → Task 9.
- "Testing: unit, integration data-gated, build/clippy ≤119, headless visual" → Tasks 1, 7, 10.

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N". Task 9's renames are conditional ("if the audit finds any") with an explicit "record no rename needed and skip" branch — that is a real instruction, not a placeholder, because the spec explicitly defers the sign classification to the planner/implementer audit.

**3. Type consistency:** `mark_chapter_starts(lines: &mut [Line], is_prose: bool)` used identically in Tasks 1, 4, 5. `Timestamp.is_track_mark` / `TimestampSnapshot.is_track_mark` renamed in Task 2 and consumed under those names in Tasks 3-5. `Action::ToggleChapterStart` / `Action::SetChapter` are existing variants (unchanged), only re-bound in Task 6.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-26-chapter-nav-from-divisions.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
