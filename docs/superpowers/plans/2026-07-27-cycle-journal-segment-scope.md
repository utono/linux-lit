# Segment-Scoped Journal Stop for the `\` Cycle — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `\` overlay cycle's journal stop show only Q&A whose citation span covers the cursor's segment, skipping the stop entirely when none does.

**Architecture:** Three changes in `src/input/actions/journal.rs`, one in `src/input/actions/overlay_cycle.rs`. A new pure helper resolves the "lap anchor" line so the probe and the open cannot disagree. `open_journal_scene` gains a scope enum: the `\` cycle passes `SegmentOnly` (tier-1 citation-span lookup only, silent miss); `Ctrl+j` passes `SegmentElseBand` (today's behavior). `journal_has_content_at_cursor` drops its scene-band fallback.

**Tech Stack:** Rust, GTK4, rusqlite. Tests are `cargo test --bins` unit tests on pure helpers, plus a headless cage/grim run for the on-screen criterion.

**Spec:** `docs/superpowers/specs/2026-07-27-cycle-journal-segment-scope-design.md`

## Global Constraints

- Build with `cargo build`. **Never run `cargo run`** — the user launches the app.
- `cargo clippy` must be clean; `cargo test` must stay green.
- The house test pattern in `journal.rs` tests **pure helpers on plain values**, never a constructed `AppState`. Keep new logic in free functions that take plain arguments so they are unit-testable; `AppState` access stays in a thin wrapper.
- Anchor position fields are `Option<(usize, usize, i32)>`; `AppState::current_line` is `usize`.
- No keybind moves in this change, so `keymap.json` and the keycap strip are untouched.
- Commit after each task. Do not merge to master until Task 5's verification passes.

## File Structure

| File | Responsibility in this change |
| --- | --- |
| `src/input/actions/journal.rs` | anchor helper (new), `journal_has_content_at_cursor` (drop fallback), `open_journal_scene` (add scope param), `toggle_overlay` call site |
| `src/input/actions/overlay_cycle.rs` | `Stop::open` passes `SegmentOnly` |
| `src/ui/keybinds_overlay.rs` | stale describe() arm correction (Task 4) |

---

### Task 1: Pure anchor-resolution helper

The probe (`journal.rs:1253`) resolves from the lap anchor; the open (`:1306`) uses raw `current_line`. Extract the rule into one tested function so they cannot drift.

**Files:**
- Modify: `src/input/actions/journal.rs` (add helper near `current_work_abbrev`, line 12-17)
- Test: `src/input/actions/journal.rs` (`mod tests`, starts line 3257)

**Interfaces:**
- Produces: `fn lap_anchor_line(gloss_return: Option<(usize, usize, i32)>, journal_return: Option<(usize, usize, i32)>, current_line: usize) -> usize` — used by Tasks 2 and 3.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/input/actions/journal.rs`:

```rust
    /// The `\` lap anchors on the position the lap STARTED from. Opening the
    /// gloss stop moves the cursor to the end of the glossed passage, so the
    /// journal stop must not probe the live cursor. Regression for the
    /// probe/open mismatch fixed 2026-07-27.
    #[test]
    fn lap_anchor_prefers_gloss_return_pos() {
        assert_eq!(lap_anchor_line(Some((424, 400, 0)), None, 437), 424);
    }

    #[test]
    fn lap_anchor_falls_back_to_journal_return_pos() {
        assert_eq!(lap_anchor_line(None, Some((910, 900, 0)), 979), 910);
    }

    #[test]
    fn lap_anchor_prefers_gloss_over_journal() {
        assert_eq!(lap_anchor_line(Some((424, 400, 0)), Some((910, 900, 0)), 979), 424);
    }

    #[test]
    fn lap_anchor_uses_cursor_when_no_overlay_open() {
        assert_eq!(lap_anchor_line(None, None, 979), 979);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bins lap_anchor 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'lap_anchor_line' in this scope`

- [ ] **Step 3: Write the helper**

Add after `current_work_abbrev` (line 17) in `src/input/actions/journal.rs`:

```rust
/// The line the `\` lap is anchored to.
///
/// An open overlay has already moved the cursor to the end of its own passage,
/// so the live `current_line` is the wrong question to ask once a lap is under
/// way. `gloss_return_pos` / `journal.return_pos` hold the reader position the
/// lap started from; fall back to the cursor when no overlay is open.
///
/// Pure so both the probe and the open resolve identically — they disagreed
/// before 2026-07-27 and opened a different entry than they probed.
fn lap_anchor_line(
    gloss_return: Option<(usize, usize, i32)>,
    journal_return: Option<(usize, usize, i32)>,
    current_line: usize,
) -> usize {
    gloss_return
        .or(journal_return)
        .map(|(line, _, _)| line)
        .unwrap_or(current_line)
}

/// `lap_anchor_line` read off live state.
fn lap_anchor_for(s: &AppState) -> usize {
    lap_anchor_line(s.gloss_return_pos, s.journal.return_pos, s.current_line)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bins lap_anchor 2>&1 | tail -20`
Expected: PASS, 4 tests

- [ ] **Step 5: Build and commit**

```bash
cargo build 2>&1 | tail -5
git add src/input/actions/journal.rs
git commit -m "refactor(journal): extract lap_anchor_line as a tested pure helper"
```

---

### Task 2: The probe drops its scene-band fallback

`journal_has_content_at_cursor` currently returns true when the whole chapter band is non-empty, regardless of the cursor. That is the tier that lets the `\` cycle open a foreign Q&A.

**Files:**
- Modify: `src/input/actions/journal.rs:1240-1281`

**Interfaces:**
- Consumes: `lap_anchor_for` from Task 1.
- Produces: `journal_has_content_at_cursor` — unchanged signature `(&Rc<RefCell<AppState>>) -> bool`, now true only on a citation-span hit.

- [ ] **Step 1: Replace the function body**

In `src/input/actions/journal.rs`, replace the whole of `journal_has_content_at_cursor` (currently lines 1240-1281, ending with the `find_scene_band_pages` expression) with:

```rust
pub(crate) fn journal_has_content_at_cursor(state: &Rc<RefCell<AppState>>) -> bool {
    let s = state.borrow();
    if s.current_work.is_none() {
        return false;
    }
    let abbrev = current_work_abbrev(&s);
    let Ok(conn) = crate::db::queries::open_db() else {
        return false;
    };

    // SPAN-SCOPED ONLY (2026-07-27). The scene-band fallback that used to sit
    // here answered "does this CHAPTER have any Q&A" — a question with no
    // reference to the cursor — so `\` opened whichever entry sorted oldest in
    // the band. The `\` lap shows material about the segment under the cursor,
    // so the only hit that counts is a `scope='passage'` entry whose citation
    // span contains the anchor. `scope='scene'` entries carry no span and are
    // deliberately unreachable by `\`; Ctrl+j and the picker still reach them.
    let anchor = lap_anchor_for(&s);
    s.current_work
        .as_ref()
        .and_then(|w| s.work_line_for_buffer(anchor).and_then(|wi| w.lines.get(wi)))
        .map(|l| (l.div1, l.div2, l.line_in_div))
        .and_then(|(d1, d2, lid)| {
            crate::db::journal::find_journal_page_for_line(&conn, &abbrev, d1, d2, lid).ok()?
        })
        .is_some()
}
```

Also update the doc comment above it (lines 1233-1239) — it currently says "the same two lookups `open_journal_scene` performs" — to:

```rust
/// Whether the journal Q&A stop has anything to show for the cursor: a
/// `scope='passage'` entry whose citation span covers the lap anchor. Performed
/// WITHOUT opening the overlay or touching any state.
///
/// Matches `open_journal_scene(state, JournalOpenScope::SegmentOnly)` exactly —
/// same anchor, same query. The `\` overlay cycle probes with this before
/// tearing down the current overlay; see `gloss::gloss_covers_cursor`, whose
/// span-only shape this mirrors.
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles clean. If `find_scene_band_pages` or `current_scene_divs` is now an unused import in this file, remove the import — do not leave a warning.

- [ ] **Step 3: Verify the existing suite still passes**

Run: `cargo test --bins 2>&1 | tail -5`
Expected: PASS (no existing test covers this function; this confirms no collateral break)

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "fix(journal): probe only citation-span hits, not the whole scene band"
```

---

### Task 3: `open_journal_scene` takes a scope

The band path must survive for `Ctrl+j`; only the `\` cycle loses it.

**Files:**
- Modify: `src/input/actions/journal.rs:1289-1385` (function), `:1222` (Ctrl+j call site)
- Modify: `src/input/actions/overlay_cycle.rs:82`
- Test: `src/input/actions/journal.rs` (`mod tests`)

**Interfaces:**
- Consumes: `lap_anchor_for` from Task 1.
- Produces: `pub(crate) enum JournalOpenScope { SegmentOnly, SegmentElseBand }` and `pub(crate) fn open_journal_scene(state: &Rc<RefCell<AppState>>, scope: JournalOpenScope) -> bool`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/input/actions/journal.rs`:

```rust
    /// The `\` cycle must never fall through to the chapter band; Ctrl+j must
    /// keep doing so. Guards the scope enum against being collapsed back into
    /// a bool or silently defaulted.
    #[test]
    fn only_segment_else_band_reaches_the_scene_band() {
        assert!(!JournalOpenScope::SegmentOnly.allows_band_fallback());
        assert!(JournalOpenScope::SegmentElseBand.allows_band_fallback());
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --bins only_segment_else_band 2>&1 | tail -20`
Expected: FAIL — `cannot find type 'JournalOpenScope' in this scope`

- [ ] **Step 3: Add the enum**

Add above `open_journal_scene` in `src/input/actions/journal.rs`:

```rust
/// How far `open_journal_scene` may widen its search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum JournalOpenScope {
    /// The `\` overlay cycle: a `scope='passage'` entry covering the lap
    /// anchor, or nothing. A miss returns false silently — no toast, no state
    /// change — so `overlay_cycle::advance` can skip the stop.
    SegmentOnly,
    /// Ctrl+j: the segment entry if there is one, else the whole scene band.
    SegmentElseBand,
}

impl JournalOpenScope {
    /// Whether a segment miss may fall through to the chapter band.
    fn allows_band_fallback(self) -> bool {
        matches!(self, JournalOpenScope::SegmentElseBand)
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --bins only_segment_else_band 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Thread the scope through `open_journal_scene`**

Change the signature (line 1289) to:

```rust
pub(crate) fn open_journal_scene(
    state: &Rc<RefCell<AppState>>,
    scope: JournalOpenScope,
) -> bool {
```

In the `cursor_hit` block (currently lines 1297-1315), change the line resolution from the live cursor to the anchor. Bind the anchor once, before the chain, rather than inside the closure. Replace:

```rust
        let abbrev = current_work_abbrev(&s);
        s.current_work
            .as_ref()
            .and_then(|w| {
                s.work_line_for_buffer(s.current_line)
                    .and_then(|wi| w.lines.get(wi))
            })
```

with:

```rust
        let abbrev = current_work_abbrev(&s);
        // The lap anchor, not the live cursor — arriving here via `\` from the
        // gloss stop leaves the cursor at the END of the glossed passage, so
        // probing `current_line` asks about a different line than
        // `journal_has_content_at_cursor` just approved.
        let anchor = lap_anchor_for(&s);
        s.current_work
            .as_ref()
            .and_then(|w| {
                s.work_line_for_buffer(anchor)
                    .and_then(|wi| w.lines.get(wi))
            })
```

(`s` here is a shared `Ref<AppState>`, so calling `lap_anchor_for(&s)` alongside the other reads is borrow-safe.)

Then, immediately after the `if let Some((pd1, pd2, entry_id)) = cursor_hit { … return true; }` block (which ends at line 1330) and BEFORE the `let (d1, d2, scene_empty) = {` binding at line 1332, insert the early return:

```rust
    // The `\` cycle stops here: no passage Q&A covers the segment, so the stop
    // has nothing to show. Return silently — `overlay_cycle::advance` skips to
    // the next stop and owns the all-empty toast. Emitting the band path's
    // "No journal entry for this segment" toast here would fire on every lap
    // through a segment that simply has no Q&A of its own.
    if !scope.allows_band_fallback() {
        return false;
    }
```

- [ ] **Step 6: Update both call sites**

In `src/input/actions/journal.rs:1222` (`toggle_overlay`, the Ctrl+j open half):

```rust
    open_journal_scene(state, JournalOpenScope::SegmentElseBand);
```

In `src/input/actions/overlay_cycle.rs:82` (inside `Stop::open`):

```rust
            Stop::Journal => {
                crate::input::actions::journal::open_journal_scene(
                    state,
                    crate::input::actions::journal::JournalOpenScope::SegmentOnly,
                );
            }
```

- [ ] **Step 7: Build and test**

Run: `cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -5 && cargo clippy 2>&1 | tail -5`
Expected: build clean, tests PASS, clippy clean

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/journal.rs src/input/actions/overlay_cycle.rs
git commit -m "fix(cycle): scope the journal stop to the cursor's segment"
```

---

### Task 4: Documentation — module doc, legend, ledger

The `overlay_cycle.rs` module doc explains the lap's rules and is the first thing a future session reads; the describe() arm is already stale from yesterday's syntax-stop change.

**Files:**
- Modify: `src/input/actions/overlay_cycle.rs:1-31` (module doc)
- Modify: `src/ui/keybinds_overlay.rs:319-320` (stale describe() arm)

- [ ] **Step 1: Add the scoping rule to the module doc**

Append to the module doc block in `src/input/actions/overlay_cycle.rs`, after the "EMPTY STOPS ARE SKIPPED" paragraph:

```rust
//! EVERY STOP IS SEGMENT-SCOPED (2026-07-27). A stop has content only when
//! something covers the CURSOR'S SEGMENT. The journal stop used to fall back
//! to the whole scene band when no passage Q&A covered the cursor, so `\` on
//! BH-Barrett ch. 10 opened the chapter's oldest Q&A — about a different
//! passage than the one on screen. `open_journal_scene` now takes
//! `JournalOpenScope::SegmentOnly` here; Ctrl+j keeps the band fallback.
//! Consequence: `scope='scene'` journal entries carry no citation span and are
//! unreachable by `\` — reach them with Ctrl+j or the picker.
```

- [ ] **Step 2: Correct the stale describe() arm**

In `src/ui/keybinds_overlay.rs`, replace lines 319-320:

```rust
        "cycle overlays" => "Action::CycleSegmentOverlays (gloss → journal Q&A \
→ syntax → wraps; each stop scoped to the cursor's segment, empty stops \
skipped; Esc exits) — src/input/actions/overlay_cycle.rs",
```

(The old text said "→ back to reader, no wrap", which the 2026-07-26 syntax-stop change already invalidated.)

- [ ] **Step 3: Verify the two overlay legends need no change**

Run: `rg -nF 'cycle overlays' src/ui/*_keybinds_overlay.rs`
Expected: two hits, `gloss_keybinds_overlay.rs:21` and `journal_keybinds_overlay.rs:23`, both already reading "(skips empty; Esc exits)" — accurate under the new rule, which only widens what counts as empty. **Make no edit to these two.**

- [ ] **Step 4: Build and commit**

```bash
cargo build 2>&1 | tail -5
git add src/input/actions/overlay_cycle.rs src/ui/keybinds_overlay.rs
git commit -m "docs(cycle): record segment scoping; fix stale describe() arm"
```

---

### Task 5: On-screen verification

Per the house rule, a green build is not "done" for a visible change, and the check must exercise the surface the user actually touches — reader mode on the reported work.

**Files:** none modified (verification only)

- [ ] **Step 1: Build, then drive the cycle with the headless harness**

`run-headless-test.sh` owns the cage lifecycle, mints an isolated `XDG_RUNTIME_DIR`, and captures a `_0` baseline plus one PNG per `--step`. Use it rather than a hand-rolled cage launch — a bare `cage` reusing `/run/user/1000` makes `grim` screenshot the user's live desktop instead of the test.

```bash
cd ~/utono/linux-lit && cargo build
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh \
  --label cycle-journal --no-clip --settle 900 \
  --step "backslash" --step "backslash"
```

`--no-clip` because the captures are overlays, not the main reading pane. Output lands in `target/ui/cycle-journal_0.png` (reader baseline), `_1.png` (first `\`), `_2.png` (second `\`).

- [ ] **Step 2: Confirm the run landed on a segment with a gloss**

The harness starts at the dev config's `last_work`, which is not guaranteed to be the reported paragraph. Check the fresh log (find it by **mtime**, not name — an ad-hoc run can take a `-{n}` slot) for the `GLOSS-PAGES:` line proving the first `\` opened a gloss:

```bash
rg -l 'GLOSS-PAGES' *.log | head; rg -n 'CURSOR_LINE|GLOSS-PAGES|JOURNAL-PAGINATE|JOURNAL-TIMING' "$(ls -t *.log | head -1)" | tail -20
```

If `_1.png` is not a gloss overlay, land deliberately first with `scripts/land-on.sh WORK div1.div2` — note it takes **`div1.div2` only** (e.g. `BH-Barrett 10.0`), not a full line citation, and takes **no overlay argument** when reader mode is wanted:

```bash
./scripts/e2e-env.sh ./scripts/land-on.sh BH-Barrett 10.0
```

- [ ] **Step 3: Assert the band query did NOT run on the second `\`**

This is the machine-checkable half of the criterion. The scene-band fallback is what logged `JOURNAL-PAGINATE: … heights=[…12 entries…]` and `band_query=3ms` in the bug report. Under `SegmentOnly`, a segment with no passage Q&A must produce **no** `JOURNAL-PAGINATE` line at all for that keypress:

```bash
rg -n 'JOURNAL-PAGINATE|JOURNAL-TIMING' "$(ls -t *.log | head -1)"
```

Expected: no journal pagination line following the second `\`, unless a passage Q&A genuinely covers the cursor's segment — in which case exactly one appears and its citation must match the on-screen passage.

- [ ] **Step 4: Open all three PNGs and report what you see**

Per the UI review protocol, open every capture and quote the on-screen text. A passing exit code is not enough.

Expected: `_1.png` shows the gloss for the segment under the cursor. `_2.png` must **not** show a Q&A whose quoted passage differs from the one under the cursor — the reported failure was a Q&A quoting "Here, beneath the painted ceiling…" while the cursor sat in "It is quite dark now…". Given ch. 10 has Q&A but apparently none covering that segment, the correct result is the **syntax stop, a wrap back to the gloss stop, or the "Nothing else to cycle to for this passage" toast** — any of the three is a pass. A foreign chapter Q&A is the failure.

- [ ] **Step 5: Confirm the `Ctrl+j` path is intact**

The band fallback must survive for the deliberate-browsing bind. Drive it in a second harness run from the same start position:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh \
  --label cycle-journal-ctrlj --no-clip --settle 900 --step "+ctrl:j"
```

Expected: the journal overlay opens on the chapter band, footer reading `Q&A 1 of 3`, and the log shows the `JOURNAL-PAGINATE` line that Step 3 required to be absent. A "No journal entry for this segment" toast here means Task 3's `SegmentOnly` leaked into the Ctrl+j call site.

- [ ] **Step 6: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Never a bare `pkill -f target/debug/linux-lit` — that kills the user's live instance.

- [ ] **Step 7: Hand off for real-renderer confirmation**

Cage is software rendering and can disagree with the real GL renderer. Report the headless result and give the user the exact reproduction: land on BH-Barrett ch. 10 in the "It is quite dark now…" paragraph, press `\` twice, confirm no foreign Q&A appears, then press `Ctrl+j` and confirm the chapter band still opens.

---

## Notes for the merge

- Branch per the house rule; merge back to master locally with `--no-ff`, re-verify the build, push, delete the branch.
- **The user's running instance predates this build** — the fix is not live until they relaunch.
- This change met the spec threshold (it alters a mode's scoping rule across two surfaces), so `superpowers:requesting-code-review` runs before merge unless review gates are explicitly waived. Build, clippy, tests, and Task 5's on-screen check are correctness, not review, and run either way.
