# Per-line verse reader finish — Phase A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the linux-lit reader for the per-line verse data model the litdb reimport produced: each verse `line_mapping` row is one display line with its own timestamp; an empty verse row renders as a stanza gap; per-line cursor seek falls out of a now-1:1 buffer↔work map; and the block-granularity verse-karaoke branch stops being called (its deletion deferred to Phase C).

**Architecture:** The reader already tolerates per-line verse rows without crashing (`split('\n')` on a single-line row yields that one line). Phase A makes three focused changes in three files: (1) retire the now-vestigial verse `\n`-split in `prepare_block_buffer` so intent is explicit; (2) render an empty verse row as gap-only in `apply_block_typography`; (3) neutralize the whole-block verse-karaoke branch in `phrase_highlight.rs` so verse karaoke takes the line-by-line path. `build_line_map_blocks` is unchanged — a test locks the 1:1 per-line mapping that gives per-line seek. Cursor navigation must skip empty verse rows.

**Tech Stack:** Rust, GTK4 (gtk4-rs), cargo test (bin crate), cage/grim/wtype e2e.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-24-per-line-verse-reader-finish-design.md` (Phase A section). It supersedes the Facet-2 portion of `docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md`.
- This is **Phase A only** — per-line verse finish. Phase B (inline italics) and Phase C (verse karaoke rendering + `block_buffer_range` deletion) are separate plans. Do NOT delete `block_buffer_range` here; only stop calling its whole-block-verse branch.
- **Bin crate:** tests run `cargo test --bin linux-lit <name>` (NOT `--lib`; fall back to plain `cargo test <name>` if the target flag errors). Also `cargo build --bin linux-lit`, `cargo clippy --bin linux-lit`.
- Do NOT run the app (`cargo run`) — the user launches it. Headless verification uses the cage/grim harness (CLAUDE.md "Headless Verification").
- `block_type` ∈ `{'prose','verse','heading'}` — unchanged, no 4th value. A stanza break is an EMPTY `verse` row (empty/whitespace `canonical_text`).
- **Empty-line preservation is load-bearing:** an empty verse row must remain one empty buffer line with its own `source_index` entry. Do NOT "optimize away" empty buffer lines — the stanza gap and the 1:1 source-index↔row contract both depend on it.
- Non-LoJ / non-verse rendering must be byte-identical after every task (regression guard).
- Branch per the project convention (worktree off master); commit on the feature branch; merge `--no-ff` from the main checkout when done. Do NOT stash/checkout/restore user files.
- `mk(bt, txt)` test helper already exists in `src/app/text_prep.rs` tests (~line 272) — reuse it there. The `build_line_map_blocks` test lives in `src/text_file_map.rs` and builds full `Line{}` literals (its own module).

## File Structure

- `src/app/text_prep.rs` — `prepare_block_buffer` (retire verse `\n`-split) + tests (Task 1).
- `src/text_file_map.rs` — no production change; add the 1:1 per-line-verse mapping test (Task 2).
- `src/app/formatting.rs` — `apply_block_typography` empty-verse-row = gap-only (Task 3).
- `src/input/phrase_highlight.rs` — neutralize the whole-block verse-karaoke branch (Task 4).
- `src/input/navigation.rs` (+ wherever line-cursor moves live) — cursor skips empty verse rows (Task 5).
- Headless acceptance (Task 6) — no production code.

---

## Task 1: Retire the verse `\n`-split in `prepare_block_buffer`

**Files:**
- Modify: `src/app/text_prep.rs` (`prepare_block_buffer`, ~lines 231–264)
- Test: same file's `#[cfg(test)] mod tests` (reuse `mk`, ~line 272)

**Interfaces:**
- Consumes: `Line.block_type`, `Line.text`; `is_verse_line`; `leading_space_tier` (existing, ~line 219).
- Produces: `prepare_block_buffer(&[Line]) -> BlockBuffer { buf_lines, source_index, indent_tiers }` — for a verse row, ONE buffer line (leading spaces stripped, tier from that row's own leading spaces); an EMPTY verse row → one empty buffer line, tier 0. Same `source_index`/`indent_tiers` invariants (non-decreasing, every work-line index emitted exactly once).

- [ ] **Step 1: Write the failing test**

Add to `src/app/text_prep.rs` tests (reuse the existing `mk`):

```rust
#[test]
fn prepare_block_buffer_empty_verse_row_is_one_blank_line_tier0() {
    let work = vec![
        mk("verse", "Stanza one line A,"),
        mk("verse", ""),                 // stanza gap
        mk("verse", "Stanza two line A,"),
    ];
    let b = prepare_block_buffer(&work);
    assert_eq!(b.buf_lines, vec!["Stanza one line A,", "", "Stanza two line A,"]);
    assert_eq!(b.source_index, vec![0, 1, 2]);
    assert_eq!(b.indent_tiers, vec![0, 0, 0]);
}

#[test]
fn prepare_block_buffer_per_line_verse_no_embedded_newline() {
    // per-line model: each verse row is already one line; indent tier from its
    // own leading spaces. (Was: one row "a\n  b" -> 2 lines; now two rows.)
    let work = vec![
        mk("verse", "a"),
        mk("verse", "  b"),   // 2 leading spaces -> tier 1
        mk("verse", "    c"), // 4 -> tier 2
    ];
    let b = prepare_block_buffer(&work);
    assert_eq!(b.buf_lines, vec!["a", "b", "c"]);
    assert_eq!(b.source_index, vec![0, 1, 2]);
    assert_eq!(b.indent_tiers, vec![0, 1, 2]);
}
```

- [ ] **Step 2: Run tests to verify status**

Run: `cargo test --bin linux-lit prepare_block_buffer_empty_verse_row prepare_block_buffer_per_line_verse`
Expected: the NEW tests PASS already IF the split loop happens to produce the same result (it does for these inputs — `"".split('\n')==[""]`, `"a".split('\n')==["a"]`). That is fine — these tests LOCK the behavior we are about to make explicit. If any fails, the current split does something unexpected; investigate before Step 3. Also note: an EXISTING test uses `mk("verse", "l1\n  l2\n    l3")` (embedded `\n`, ~line 285) — that is the OLD block-granularity shape. After Step 3 it must be UPDATED to the per-line shape (see Step 3).

- [ ] **Step 3: Retire the split loop + update the stale block-shape test**

In `prepare_block_buffer` (~line 236), replace the verse branch:

```rust
        if crate::db::line_types::is_verse_line(&l.block_type) {
            for vline in l.text.split('\n') {
                let (tier, n) = leading_space_tier(vline);
                buf_lines.push(vline[n..].to_string());
                source_index.push(wi);
                indent_tiers.push(tier);
            }
        } else {
```

with the per-line form (each verse row is one display line):

```rust
        if crate::db::line_types::is_verse_line(&l.block_type) {
            // Per-line verse model: one row = one display line. Strip this row's
            // own leading spaces (the indent tier); an empty verse row stays one
            // empty buffer line, tier 0 (stanza gap — load-bearing, keep it).
            let (tier, n) = leading_space_tier(&l.text);
            buf_lines.push(l.text[n..].to_string());
            source_index.push(wi);
            indent_tiers.push(tier);
        } else {
```

Then UPDATE the existing block-shape tests that encode embedded `\n`. Find every `mk("verse", "…\n…")` in this file's tests (Step-2 grep: `mk("verse", "l1\n  l2\n    l3")` ~line 285, and `mk("verse", "a\n  b\n    c\nd")` ~line 301) and split each into per-line `mk("verse", …)` rows, updating that test's expected `buf_lines`/`source_index`/`indent_tiers` to the per-line result (each verse line its own row → its own source index). Keep every assertion meaningful; do not delete the tests.

- [ ] **Step 4: Run tests**

Run: `cargo test --bin linux-lit --  prepare_block_buffer` (all `prepare_block_buffer*` tests)
Expected: PASS (new + updated). Then `cargo test --bin linux-lit` — full suite green.

- [ ] **Step 5: Commit**

```bash
git add src/app/text_prep.rs
git commit -m "feat(reader): per-line verse in prepare_block_buffer (retire the \n-split)"
```

---

## Task 2: Lock the 1:1 per-line-verse mapping (per-line seek)

**Files:**
- Modify: `src/text_file_map.rs` — NO production change; add one test.

**Interfaces:**
- Consumes: `build_line_map_blocks(file_lines: &[String], source_index: &[usize], work_lines: &[Line]) -> LineMap` (unchanged, ~line 243).
- Produces: proof that per-line verse rows map 1:1 buffer↔work, so a seek to verse line N resolves to work row N (its own timestamp), not the stanza's first row.

- [ ] **Step 1: Write the test**

Add to `src/text_file_map.rs` tests (full `Line{}` literals — this module has no `mk`):

```rust
#[test]
fn build_line_map_blocks_per_line_verse_maps_each_line_to_own_row() {
    let work: Vec<Line> = ["l1","l2","l3","l4","l5"].iter().map(|t| Line {
        id: 0, citation: String::new(), text: (*t).into(), normalized: String::new(),
        speaker: None, is_dialogue: false, timestamp: None, div1: 1, div2: 0,
        line_in_div: 1, sub_line: 0, is_chapter: false, is_spoken: None,
        block_type: "verse".into(),
    }).collect();
    let file_lines: Vec<String> = work.iter().map(|l| l.text.clone()).collect();
    let source_index: Vec<usize> = (0..work.len()).collect();
    let map = build_line_map_blocks(&file_lines, &source_index, &work);
    assert_eq!(map.buffer_to_work, vec![Some(0),Some(1),Some(2),Some(3),Some(4)]);
    assert_eq!(map.work_to_buffer, vec![0,1,2,3,4]);
}
```

- [ ] **Step 2: Run it**

Run: `cargo test --bin linux-lit build_line_map_blocks_per_line_verse`
Expected: PASS immediately (no production change — `build_line_map_blocks` maps by `source_index` structure, and a strictly-increasing `source_index` gives a 1:1 map). If it FAILS, the mapping is not 1:1 for per-line rows — STOP and report; the per-line-seek premise is wrong and the spec needs revisiting.

- [ ] **Step 3: Commit**

```bash
git add src/text_file_map.rs
git commit -m "test(reader): lock 1:1 per-line-verse buffer↔work map (per-line seek)"
```

---

## Task 3: Empty verse row = stanza gap (gap tag only)

**Files:**
- Modify: `src/app/formatting.rs` (`apply_block_typography` verse branch, ~lines 708–715)

**Interfaces:**
- Consumes: `Line.text`, `is_verse_line`, `state.block_indent_tiers`, the `verse-indent-{tier}` / `verse-stanza-gap` tags (created in `ensure_block_typography_tags`, ~line 652).
- Produces: an empty verse row gets ONLY `verse-stanza-gap` (blank vertical space) — no `verse-indent-*` tag; a non-empty verse row is unchanged.

- [ ] **Step 1: Apply the change**

In `apply_block_typography` (~line 708), replace the verse branch:

```rust
        if crate::db::line_types::is_verse_line(bt) {
            let tier = state.block_indent_tiers.get(bl).copied().unwrap_or(0);
            state
                .buffer
                .apply_tag_by_name(&format!("verse-indent-{tier}"), &start, &end);
            if prev_src != Some(wi) {
                state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
            }
        } else if crate::db::line_types::is_heading_line(bt) {
```

with:

```rust
        if crate::db::line_types::is_verse_line(bt) {
            // An empty verse row is a stanza-gap separator: gap tag only, no
            // indent tag, no cursor/karaoke target.
            if line.text.trim().is_empty() {
                state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
            } else {
                let tier = state.block_indent_tiers.get(bl).copied().unwrap_or(0);
                state
                    .buffer
                    .apply_tag_by_name(&format!("verse-indent-{tier}"), &start, &end);
                if prev_src != Some(wi) {
                    state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
                }
            }
        } else if crate::db::line_types::is_heading_line(bt) {
```

(`line` is the current `work.lines[wi]` in scope in this loop — confirm the binding name by reading the loop head; the grep showed `bt` derived from it, so `line.text` is available. If the binding is named differently, use that name.)

- [ ] **Step 2: Build + regression**

Run: `cargo build --bin linux-lit && cargo test --bin linux-lit` — compiles, suite green (no unit test here; this is a GTK-tag change whose acceptance is the Task 6 headless proof, since vol1 has zero empty verse rows and this is visual). `cargo clippy --bin linux-lit` — no new warnings.

- [ ] **Step 3: Commit**

```bash
git add src/app/formatting.rs
git commit -m "feat(reader): empty verse row renders as stanza gap (gap tag only)"
```

---

## Task 4: Neutralize the whole-block verse-karaoke branch

**Files:**
- Modify: `src/input/phrase_highlight.rs` (the `is_verse_active == Some(true)` branch, ~lines 464–483)

**Interfaces:**
- Consumes: `s.line_map`, `is_verse_line`, `block_buffer_range` (NOT deleted — Phase C owns deletion).
- Produces: verse karaoke takes the normal line-by-line path (same as prose), so no code asserts block granularity for verse at runtime. Prose/non-LoJ karaoke byte-identical.

- [ ] **Step 1: Read the current branch**

Read `src/input/phrase_highlight.rs` around lines 460–490 — the block:

```rust
        let is_verse_active = s.line_map.as_ref().and_then(|lm| {
            lm.buffer_to_work.get(bl).copied().flatten()
                .and_then(|wi| s.current_work.as_ref()?.lines.get(wi))
                .map(|l| crate::db::line_types::is_verse_line(&l.block_type))
        });
        if is_verse_active == Some(true) {
            let (bs, be) = block_buffer_range(&s.line_map.as_ref().unwrap().buffer_to_work, bl);
            let tag = s.phrase_tag.clone();
            let (buf_start, buf_end) = s.buffer.bounds();
            s.buffer.remove_tag(&tag, &buf_start, &buf_end);
            for line in bs..be {
                let line_text = buffer_line_text(s, line);
                apply_char_range_tag(s, &tag, line, 0, line_text.chars().count());
            }
            /* … likely a return / early-continue here … */
        }
        /* … normal per-line tint path below … */
```

- [ ] **Step 2: Remove the whole-block branch so verse falls through to per-line**

Delete the `if is_verse_active == Some(true) { … }` block entirely (including the `is_verse_active` computation if it is used nowhere else — confirm with a grep for `is_verse_active`), so verse and prose both take the normal single-line tint path that follows. Do NOT delete `block_buffer_range` itself (Phase C) — only its call here. Confirm nothing else references the removed block's local `bs`/`be`.

If removing the block changes control flow (e.g. the block ended in `return`/`continue` that the fall-through path also needs), preserve the surrounding path's correctness — read the whole function to place the deletion so the normal path runs for verse exactly as for prose.

- [ ] **Step 3: Update/retire the `block_buffer_range` call-through tests only if they assert the whole-block tint**

`block_buffer_range`'s own unit tests (the `(1,5)`/`(0,1)`/OOB cases) stay — the helper is untouched (Phase C deletes it). Only remove/adjust a test that specifically asserted the whole-block VERSE TINT behavior via this call site, if one exists. Grep the test module; if none targets the call site, no test change.

- [ ] **Step 4: Build + regression (the key guard)**

Run: `cargo build --bin linux-lit && cargo test --bin linux-lit && cargo clippy --bin linux-lit`
Expected: green, no new warnings. The guard: prose/non-LoJ karaoke is byte-identical (the removed branch only ran for `is_verse_active == Some(true)`), and `block_buffer_range` still compiles (now only its own tests reference it — a dead-code warning on it is EXPECTED and acceptable until Phase C; if clippy denies it, add `#[allow(dead_code)]` with a `// Phase C removes this` note, do NOT delete it).

- [ ] **Step 5: Commit**

```bash
git add src/input/phrase_highlight.rs
git commit -m "feat(reader): verse karaoke takes line-by-line path (stop calling whole-block branch)"
```

---

## Task 5: Cursor skips empty verse rows (prose step branch)

**Scope pinned during planning (confirm on read):** LoJ is a prose work; its cursor
step is `cursor_next_dialogue` / `cursor_prev_line` in `src/input/navigation.rs`,
whose `state.is_prose()` branch steps **exactly one plain buffer line** (`current_line
+ 1` / `- 1`) and does NOT skip blanks (`navigation.rs:1631`, "step one plain buffer
line … lands on every line"). Prose paragraphs in LoJ are one row each with NO blank
rows between them, so the empty verse row is the FIRST blank buffer line prose
navigation encounters — it would become a cursor stop. This task makes the prose step
branch skip it. The play (`else`) branch already skips blanks via
`next_dialogue_line`/`prev_dialogue_line` — leave it untouched.

**Files:**
- Modify: `src/input/navigation.rs` — the `is_prose()` step arms of `cursor_next_dialogue`
  (~line 1631) and `cursor_prev_line` (its sibling; grep `fn cursor_prev_line`).

**Interfaces:**
- Consumes: `state.work_line_for_buffer(bl)` → `Line.block_type`/`Line.text`.
- Produces: a prose-step target that would land on an empty verse row advances to the
  nearest non-empty buffer line in the travel direction (clamped to bounds). An empty
  verse row is a visual separator, never a stop.

- [ ] **Step 1: Confirm the two step sites + add the pure helpers**

Read `cursor_next_dialogue` (~1621) and `cursor_prev_line` (grep it). Confirm both have
an `if state.is_prose() { … current_line ± 1 … }` arm. Add pure helpers (near the other
free fns in `navigation.rs`):

```rust
/// True when buffer line `bl` maps to a verse row whose text is empty/whitespace
/// (a stanza-gap separator — never a cursor stop).
fn is_empty_verse_line(state: &AppState, bl: usize) -> bool {
    state.work_line_for_buffer(bl)
        .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
        .is_some_and(|l| crate::db::line_types::is_verse_line(&l.block_type)
            && l.text.trim().is_empty())
}

/// Advance `bl` past consecutive empty verse rows in direction `dir` (+1 down,
/// −1 up), clamped to `[0, line_count)`. Returns the first non-gap line, or the
/// original `bl` if none exists in that direction.
fn skip_empty_verse(state: &AppState, mut bl: usize, dir: isize, line_count: usize) -> usize {
    while is_empty_verse_line(state, bl) {
        let next = bl as isize + dir;
        if next < 0 || next as usize >= line_count { break; }
        bl = next as usize;
    }
    bl
}
```

- [ ] **Step 2: Write the failing test**

Unit-test the pure skip with a synthetic fixture. This module builds full `Line{}`
literals and an `AppState` may be heavy — if constructing a real `AppState` is
impractical here, split the buffer-agnostic core out (a fn taking
`buffer_to_work`-style inputs) and test THAT, mirroring how `phrase_highlight.rs`
unit-tests `block_buffer_range` on a plain `&[Option<usize>]`. Assert: down from a
line above the gap lands past the gap; up lands before it; a run of ≥2 empty rows is
fully skipped; no empty row → identity.

```rust
#[test]
fn skip_empty_verse_advances_past_the_gap() {
    // lines: 0 verse "a", 1 verse "" (gap), 2 verse "b"
    // from 0 stepping DOWN to 1 -> skip to 2; from 2 stepping UP to 1 -> skip to 0.
    // (build the minimal fixture this module's tests use.)
}
```

- [ ] **Step 3: Run fail → implement → pass**

Run: `cargo test --bin linux-lit skip_empty_verse` → FAIL → implement → PASS. Then wire
into BOTH prose step arms: after computing the `current_line ± 1` target, pass it through
`skip_empty_verse(state, target, +1 or −1, line_count)` so the landed line is never an
empty verse row. Do NOT touch the play (`else`) branch.

- [ ] **Step 4: Build + regression**

Run: `cargo build --bin linux-lit && cargo test --bin linux-lit`
Expected: green. Guard: works with NO empty verse rows (all non-LoJ, and LoJ vol1) —
`is_empty_verse_line` is always false, so `skip_empty_verse` is identity → navigation
byte-identical. The play branch is untouched.

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat(reader): prose cursor step skips empty verse rows (stanza gaps aren't stops)"
```

---

## Task 6: Headless on-screen acceptance (non-optional gate)

**Files:** none — cage/grim harness against live LoJ (per-line data, all 6 vols in lit.db).

**Interfaces:** the visible-surface gate. A green build is NOT acceptance for these UI changes.

- [ ] **Step 1: Build**

Run: `cd <worktree> && cargo build` — clean.

- [ ] **Step 2: Land on a real LoJ verse passage**

Launch headless per CLAUDE.md ("Headless Verification": `LIT_NO_MPV=1 GSK_RENDERER=cairo LIT_DEV=1`, cage via the harness `run_in_background`, fresh `XDG_RUNTIME_DIR` — prefer `scripts/land-on.sh LoJ …`; resize `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`; ~3s to map; re-send the first post-resize chord). Land on the duck-epitaph and the Virgil eclogue pages (ch.1).

- [ ] **Step 3: Verify — open every PNG, pixel-measure, report inline**

- **Per-line verse:** each verse line on its OWN line at its correct indent tier (pixel-measure a flush vs an indented line — the tier gap must be real, not antialiasing).
- **Per-line seek:** seek/nav to a specific verse line (not the first of its stanza) and confirm the cursor lands on THAT line, and — if audio is available — that its own timestamp is used. (If no phrase/audio, at least confirm the cursor lands on the right line.)
- **Stanza gap:** since **vol1 has ZERO empty verse rows**, exercise this with a later volume that has one, OR a synthetic fixture. Confirm the empty row renders as blank vertical space, the cursor SKIPS it (Task 5), and — **A4 MUST-VERIFY** — the gap is a SINGLE vertical gap, not a DOUBLE gap where an empty row is immediately followed by a stanza-first line (both would carry `verse-stanza-gap`). Pixel-measure the gap height vs a normal stanza gap. If it double-gaps, that is a Task-3 fix (suppress the stanza-first line's gap tag when the previous row was an empty verse row) — apply it and re-verify.
- **No clipping** (clip-prevention ledger; `LIT_DEBUG_CLIP_COLOR='#ff0000'` if logs disagree with pixels).

- [ ] **Step 4: Regression on screen**

Screenshot a NON-LoJ prose work (BH or PP) — IDENTICAL to pre-change. Confirm prose karaoke path is unaffected (Task 4 only removed the verse branch).

- [ ] **Step 5: Hand the user the real-GL command**

Cage is software rendering; give the exact command to eyeball the verse pages on the real GL renderer and report what was observed headless.

- [ ] **Step 6: Cleanup**

`pkill -f "cage -- ./target/debug/linux-lit"` (EXACTLY this — never a bare `pkill -f target/debug/linux-lit`).

---

## Post-implementation

- Finish per the project convention: merge `--no-ff` to master from the MAIN checkout, re-verify build+tests on master, push, remove the worktree, delete the branch.
- **Retire the carry-forward:** the litdb plan's "per-line seek deferred upstream" note is DONE (Task 2's 1:1 map delivers per-line seek).
- **Follow-ups (out of scope, own plans):** Phase B (inline `_italics_`); Phase C (verse karaoke line-by-line + DELETE `block_buffer_range`, needs the litdb `phrase_timestamps` backfill for LoJ media 233); vols 2–6 stay data-only until vol1 is proven on screen here.

## Self-Review

- **Spec coverage:** A2 retire `\n`-split → Task 1; A3 1:1 map / per-line seek → Task 2; A4 empty-row stanza gap + MUST-VERIFY double-gap → Task 3 + Task 6 Step 3; A5 neutralize whole-block karaoke (defer deletion) → Task 4; cursor skips gap → Task 5; headless proof + regression → Task 6. All covered.
- **Placeholders:** Task 5's exact function name is grep-gated (Step 1 confirms before editing) — a verification gate, not a TODO. Task 3's `line` binding name is confirm-before-use.
- **Type consistency:** `BlockBuffer{buf_lines, source_index, indent_tiers}`, `build_line_map_blocks(file_lines, source_index, work_lines) -> LineMap{buffer_to_work, work_to_buffer}`, `is_verse_line`/`is_heading_line`, `block_buffer_range`, `apply_char_range_tag`, `buffer_line_text` — all read from current master during planning.
