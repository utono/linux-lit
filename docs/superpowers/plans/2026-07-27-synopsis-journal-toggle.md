# Synopsis ↔ Scene-Q&A Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `\` in the synopsis overlay opens the newest `scope='scene'` journal entry for the band being displayed; `\` in that entry returns to the synopsis.

**Architecture:** A new DB query returns the band's newest scene entry. The synopsis `\` arm (currently a consumed no-op) hops to it via the existing `land_on_page`, recording the origin band in a new `AppState` field. The journal `\` arm checks that field first and returns to the synopsis when set, otherwise falls through to today's overlay cycle.

**Tech Stack:** Rust, GTK4, rusqlite.

**Spec:** `docs/superpowers/specs/2026-07-27-synopsis-journal-toggle-design.md`

## Global Constraints

- Build with `cargo build`. **Never run `cargo run`** — the user launches the app.
- `cargo clippy` must be clean; `cargo test` must stay green (1206 tests at branch point).
- House test pattern: test PURE HELPERS on plain values. Never construct an `AppState` in a test.
- **Every keybind change updates its legend in the SAME change** — required, not optional.
- The synopsis renders in the **gloss overlay** widget (`show_synopsis`), which is why its keys live in a separate `keymap.rs` arm from the gloss overlay's own.
- No schema change. No `keymap.json` change (`\` is already routed; only handler bodies change).
- Commit after each task.

## File Structure

| File | Responsibility |
| --- | --- |
| `src/db/journal.rs` | `find_newest_scene_page` query |
| `src/app/mod.rs` | `journal_from_synopsis: Option<(i64, i64)>` field |
| `src/input/actions/journal.rs` | hop + return handlers; marker clearing |
| `src/input/keymap.rs` | synopsis `\` arm (~line 3219); journal `\` arm (~line 2391) |
| `src/ui/synopsis_keybinds_overlay.rs` | `\` legend entry |
| `src/ui/journal_keybinds_overlay.rs` | `\` return entry + the `Ctrl+n/p` wording fix |

---

### Task 1: The newest-scene-entry query

**Files:**
- Modify: `src/db/journal.rs` (add beside `find_scene_band_pages`, line 245)

**Interfaces:**
- Produces: `pub fn find_newest_scene_page(conn: &Connection, work_abbrev: &str, div1: i64, div2: i64) -> Result<Option<JournalPage>, rusqlite::Error>`

- [ ] **Step 1: Write the query**

Add after `find_scene_band_pages` in `src/db/journal.rs`:

```rust
/// The band's most recently CREATED scene-scoped entry — the synopsis `\`
/// target. Scene-scoped ONLY: passage entries belong to a span inside the
/// band, and the reader's `\` already reaches those from the passage they
/// cover (see the 2026-07-27 segment-scoping change).
///
/// "Newest" is newest CREATED. `timestamp` is a creation stamp; journal
/// entries have no last-viewed tracking, and this deliberately does not add
/// any. `id DESC` breaks ties for rows written in the same second.
pub fn find_newest_scene_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Option<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND scope = 'scene'
         ORDER BY timestamp DESC, id DESC
         LIMIT 1",
    ))?;
    let mut rows = stmt.query_map(
        rusqlite::params![work_abbrev, div1, div2],
        map_journal_page_row,
    )?;
    rows.next().transpose()
}
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles clean. (Unused until Task 3 — a dead-code warning here is expected and must NOT be silenced with `#[allow]`.)

- [ ] **Step 3: Commit**

```bash
git add src/db/journal.rs
git commit -m "feat(journal): query the band's newest scene-scoped entry"
```

---

### Task 2: The origin marker

**Files:**
- Modify: `src/app/mod.rs` (field declaration near `synopsis_overlay_scene`, ~line 691; initializer in the `AppState` constructor, ~line 2236 area)
- Test: `src/input/actions/journal.rs` (`mod tests`)

**Interfaces:**
- Produces: `AppState.journal_from_synopsis: Option<(i64, i64)>`, and `fn take_synopsis_origin(marker: &mut Option<(i64, i64)>) -> Option<(i64, i64)>` in `journal.rs`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/input/actions/journal.rs`:

```rust
    /// The synopsis→journal hop records its origin band; the return hop TAKES
    /// it, so a later journal session opened any other way cannot inherit a
    /// stale marker and hijack `\`.
    #[test]
    fn synopsis_origin_is_taken_not_copied() {
        let mut marker = Some((10, 0));
        assert_eq!(take_synopsis_origin(&mut marker), Some((10, 0)));
        assert_eq!(marker, None, "the marker must be consumed");
        assert_eq!(take_synopsis_origin(&mut marker), None);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --bins synopsis_origin 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'take_synopsis_origin' in this scope`

- [ ] **Step 3: Add the field**

In `src/app/mod.rs`, beside `synopsis_overlay_scene`:

```rust
    /// The `(div1, div2)` band a synopsis→journal `\` hop came FROM, or None
    /// when the journal was opened any other way. Set on the hop, TAKEN on the
    /// return hop, and cleared wherever a journal session ends — a stale
    /// marker would make a later unrelated journal `\` jump to a synopsis
    /// instead of advancing the overlay cycle.
    pub journal_from_synopsis: Option<(i64, i64)>,
```

Add `journal_from_synopsis: None,` to the `AppState` constructor alongside the other `None` initializers.

- [ ] **Step 4: Add the helper**

In `src/input/actions/journal.rs`, near the other small helpers at the top:

```rust
/// Take the synopsis-origin band, leaving None. Pure so the take-not-copy
/// contract is testable without an AppState.
fn take_synopsis_origin(marker: &mut Option<(i64, i64)>) -> Option<(i64, i64)> {
    marker.take()
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --bins synopsis_origin 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 6: Build and commit**

```bash
cargo build 2>&1 | tail -5
git add src/app/mod.rs src/input/actions/journal.rs
git commit -m "feat(journal): add the synopsis-origin marker"
```

---

### Task 3: The hop and the return

**Files:**
- Modify: `src/input/actions/journal.rs` (two new pub(crate) fns; marker clearing in `close_overlay`, line 1427)
- Modify: `src/input/actions/overlay_cycle.rs` (marker clearing in `close_current`)
- Modify: `src/input/keymap.rs` (synopsis `\` arm ~3219; journal `\` arm ~2391)

**Interfaces:**
- Consumes: `find_newest_scene_page` (Task 1), `journal_from_synopsis` + `take_synopsis_origin` (Task 2).
- Produces: `pub(crate) fn open_scene_qa_from_synopsis(state: &Rc<RefCell<AppState>>) -> bool` and `pub(crate) fn return_to_synopsis(state: &Rc<RefCell<AppState>>) -> bool`.

- [ ] **Step 1: Write the hop**

Add to `src/input/actions/journal.rs`:

```rust
/// Synopsis `\`: open the band's newest scene-scoped Q&A. Returns whether an
/// entry was opened; false means the band has none and the caller keeps the
/// synopsis open (this function emits the miss toast).
pub(crate) fn open_scene_qa_from_synopsis(state: &Rc<RefCell<AppState>>) -> bool {
    let (abbrev, div1, div2, unit) = {
        let s = state.borrow();
        if s.current_work.is_none() {
            return false;
        }
        let (d1, d2) = s.synopsis_overlay_scene;
        // "chapter" for prose, "scene" for plays — match the surface's wording.
        let unit = if crate::app::scene_synopsis::is_chapter_work(&s) {
            "chapter"
        } else {
            "scene"
        };
        (current_work_abbrev(&s), d1, d2, unit)
    };

    let page = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::journal::find_newest_scene_page(&conn, &abbrev, div1, div2).ok()
        })
        .flatten();

    let Some(page) = page else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("No journal entry for this {}", unit),
            3,
        );
        return false;
    };

    let mut s = state.borrow_mut();
    // Close the synopsis (it renders in the gloss overlay widget) before the
    // journal takes over.
    s.gloss_overlay.hide();
    // Same prior-session cleanup every journal open path performs, so a stale
    // filter/search never leaks into this session.
    s.journal.filter = None;
    s.journal_overlay.clear_search_tags();
    s.journal.search = None;
    s.journal.last_pattern = None;
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    s.journal_from_synopsis = Some((div1, div2));
    s.input_mode = InputMode::JournalOverlay;
    let id = page.id;
    land_on_page(&mut s, JournalBand::Scene(div1, div2), id);
    s.journal.entry_page_id = s.journal.pages.get(s.journal.page_index).map(|p| p.id);
    true
}

/// Journal `\` when the session was entered from a synopsis: close the journal
/// and reopen that synopsis. Returns false when there is no origin marker, so
/// the caller falls through to the overlay cycle.
pub(crate) fn return_to_synopsis(state: &Rc<RefCell<AppState>>) -> bool {
    let origin = {
        let mut s = state.borrow_mut();
        take_synopsis_origin(&mut s.journal_from_synopsis)
    };
    let Some((div1, div2)) = origin else {
        return false;
    };
    {
        let mut s = state.borrow_mut();
        s.journal_overlay.clear_rewrite_diff();
        s.rewrite_browse = None;
        s.tts.stop();
        s.journal_overlay.hide();
        s.journal.entry_page_id.take();
        let pos = s.journal.return_pos.take();
        crate::app::return_to_reader_mode(&mut s);
        crate::app::restore_saved_position_resnap(&mut s, pos);
        // Reopen on the band we came from, not wherever the cursor now sits.
        s.synopsis_overlay_scene = (div1, div2);
    }
    crate::app::scene_synopsis::show_synopsis_overlay(state);
    true
}
```

- [ ] **Step 2: Clear the marker wherever a journal session ends**

In `src/input/actions/journal.rs`, inside `close_overlay` (line 1427), beside the existing `rewrite_browse = None`:

```rust
    // A journal session that ends any other way must not leave an origin
    // marker for the NEXT journal open to inherit — `\` there would jump to a
    // synopsis instead of advancing the overlay cycle.
    state.borrow_mut().journal_from_synopsis = None;
```

In `src/input/actions/overlay_cycle.rs`, inside `close_current`'s `Stop::Journal` arm, beside `s.journal.entry_page_id.take()`:

```rust
            s.journal_from_synopsis = None;
```

- [ ] **Step 3: Wire the synopsis `\` arm**

In `src/input/keymap.rs`, replace the consumed no-op (~line 3219, the arm whose comment reads "(lap is gloss → journal → reader). Consumed no-op."):

```rust
        // `\`: toggle to this band's newest scene-scoped Q&A (spec
        // 2026-07-27-synopsis-journal-toggle). NOT the reader's segment-scoped
        // lap — a band surface gets a band-scoped toggle. A miss toasts and
        // leaves the synopsis open.
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::journal::open_scene_qa_from_synopsis(state);
            true
        }
```

- [ ] **Step 4: Wire the journal `\` arm**

In `src/input/keymap.rs` (~line 2391), replace the body:

```rust
        // `\`: return to the synopsis when this session was entered from one;
        // otherwise advance the segment-overlay cycle as before.
        "backslash" if !is_ctrl && !is_alt => {
            if !crate::input::actions::journal::return_to_synopsis(state) {
                crate::input::actions::overlay_cycle::cycle_from_journal(state);
            }
            true
        }
```

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | rg 'test result' | tail -1 && cargo clippy 2>&1 | rg -c '^error'`
Expected: build clean, tests PASS, clippy 0 errors

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/journal.rs src/input/actions/overlay_cycle.rs src/input/keymap.rs
git commit -m "feat(synopsis): toggle to the band's newest scene Q&A on backslash"
```

---

### Task 4: Legends

Both surfaces gain a `\` entry, and one existing line is corrected.

**Files:**
- Modify: `src/ui/synopsis_keybinds_overlay.rs` (`GROUPS`, line 22)
- Modify: `src/ui/journal_keybinds_overlay.rs` (`GROUPS`, line 27)

- [ ] **Step 1: Add the synopsis legend entry**

In `src/ui/synopsis_keybinds_overlay.rs`, add to the "Navigation" group:

```rust
        ("\\", "newest scene Q&A for this chapter/scene (\\ returns)"),
```

- [ ] **Step 2: Add the journal legend entry and fix the stale line**

In `src/ui/journal_keybinds_overlay.rs`, in the "Navigation" group, replace:

```rust
        ("Ctrl+n / Ctrl+p", "nav_page: next / prev Q&A in band"),
```

with:

```rust
        ("Ctrl+n / Ctrl+p", "nav_page: next / prev Q&A in the WORK (all scopes)"),
```

(The old wording said "in band"; `nav_page` walks the whole work via
`find_all_pages_ordered` — no band filter, no scope filter.)

Then add to the same group:

```rust
        ("\\", "back to the synopsis (only when entered from one)"),
```

- [ ] **Step 3: Verify no other legend claims the synopsis `\` is a no-op**

Run: `rg -nF 'no-op' src/ui/*_keybinds_overlay.rs src/input/keymap.rs | rg -i 'synops|backslash'`
Expected: no stale "consumed no-op" note for the synopsis `\` survives. Fix any that does.

- [ ] **Step 4: Build and commit**

```bash
cargo build 2>&1 | tail -5
git add src/ui/synopsis_keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs
git commit -m "docs(legends): synopsis/journal backslash entries; fix Ctrl+n/p wording"
```

---

### Task 5: On-screen verification

**Files:** none modified.

BH-Barrett ch. 10 is the test band — three scene entries, so "newest" is a real choice, and the newest by timestamp is id 52 (`How does the author deploy the motif…`, cited `BH.10.0.947`).

- [ ] **Step 1: Confirm the expected target before driving**

```bash
sqlite3 /home/mlj/utono/litdb/data/lit.db \
  "SELECT id, substr(question,1,50) FROM journal_entries
   WHERE work_abbrev='BH' AND div1=10 AND div2=0 AND scope='scene'
   ORDER BY timestamp DESC, id DESC LIMIT 1;"
```

Whatever this prints is what `\` must open. Re-run it rather than trusting the id above — the DB is live.

- [ ] **Step 2: Drive synopsis → journal → synopsis**

The synopsis opens with `h` from the reader. Run through the env wrapper:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh \
  --label synopsis-toggle --no-clip --settle 1200 \
  --start-work BH-Barrett --start-pos 944 \
  --setup "h" --step "backslash" --step "backslash"
```

Captures: `_0` synopsis, `_1` journal entry, `_2` synopsis again.

Confirm the binds first — `rg -n 'ToggleSynopsis|"h"' src/input/keymap_config.rs` — key names drift, and front matter has no synopsis.

- [ ] **Step 3: Open all three PNGs and report what you see**

Per the UI review protocol, quote the on-screen text. `_1` must show the entry Step 1 named — not an older one from the same band, and not a passage entry. `_2` must be the ch. 10 synopsis again.

- [ ] **Step 4: Verify the miss path**

Land on a band with no scene entry (BH ch. 9 has none — `SELECT count(*) … div1=9` returns 0), open the synopsis, press `\`. Expect the toast "No journal entry for this chapter" and the synopsis still open. Confirm in the log:

```bash
rg -n 'CHAPTER_TOAST' "$(command ls -t /tmp/headless-test.log)"
```

- [ ] **Step 5: Verify the marker does not leak**

Open the journal via `Ctrl+j` (NOT from a synopsis) and press `\`. It must advance the overlay cycle — never jump to a synopsis. A jump here means a stale `journal_from_synopsis` survived a close.

- [ ] **Step 6: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Never a bare `pkill -f target/debug/linux-lit` — that kills the user's live instance.

- [ ] **Step 7: Hand off for real-renderer confirmation**

Cage is software rendering. Give the user the exact steps: open a chapter synopsis on BH ch. 10, press `\`, confirm the newest Q&A opens; press `\` again, confirm the synopsis returns; then Ctrl+j elsewhere and confirm `\` still cycles.

---

## Notes for the merge

- This change met the spec threshold (new bind behavior across two surfaces), so `superpowers:requesting-code-review` runs before merge unless review is explicitly waived. Build, clippy, tests, and Task 5 are correctness and run either way.
- Merge back to master locally with `--no-ff`, re-verify, push, delete the branch.
- **Queued next, already user-approved** (see `.superpowers/sdd/progress.md`): scope-filtered `Ctrl+Shift+n/p` cycling, with the revision family (`browse_step`/`browse_restore`) moving to the `w` cap. Its own spec→plan cycle, after this merges.
