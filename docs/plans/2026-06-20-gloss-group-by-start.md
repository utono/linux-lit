# Gloss grouping by start line + reader-gloss-first picker

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make all glosses that share a START citation (regardless of end/span) appear together in the gloss overlay and cycle together via Ctrl+n/p, and default the Ctrl+g picker to the reader-gloss filter so reader-glosses show first.

**Problem:** Glosses on the same first line can be anchored to different-length passages (e.g. 2H6 2.1.1: teacher-generic spans .1–.4, reader-gloss spans .1–.8). The overlay's `gloss_list` is built by `find_all_glosses(start, end, ...)` which matches start AND end exactly, so the two never co-list and Ctrl+n/p can't reach the other. The picker also opens on the teacher-generic filter.

**Architecture:** Add a start-only query `find_glosses_by_start`; use it in the overlay-open and gloss_list-refresh paths so the cycling list is keyed by start citation. Leave exact-match `find_all_glosses` for save-time re-fetch (identity of the just-saved passage). Change the picker default filter to `ReaderGloss`.

**Tech Stack:** Rust, rusqlite/SQLite (`lit.db`), GTK4.

---

## Background facts (verified)
- `find_all_glosses` (`src/db/queries.rs:1537`) matches `start_citation = ?2 AND end_citation = ?3` exactly.
- `find_glossed_passages` (picker discovery) returns DISTINCT `p.id` rows — so different-span passages on the same start line are SEPARATE picker rows; the picker filter (one type at a time) currently hides the reader row.
- Ctrl+n/p → `navigate_gloss(±1)` walks `gloss_list` (glosses of current passage). Alt+n/p → `navigate_gloss_passage` (between passages).
- `gloss_list` is a `Vec<SavedGloss>`; each `SavedGloss` carries its own `gloss_text` (with its own `<verse>` lines) and `passage_id`. So glosses of different spans can safely co-list — the overlay re-renders each gloss's own stored text on cycle.
- `find_all_glosses` is called at 13 sites (visual.rs ×6, gloss.rs ×4, keymap.rs ×1, synopsis.rs ×1, pickers indirectly). The overlay-open + cycle-refresh sites need start-grouping; the save-re-fetch sites must stay exact.

---

### Task 1: Add `find_glosses_by_start` query

**Files:**
- Modify: `src/db/queries.rs` (add fn after `find_all_glosses`, ~line 1575)
- Test: `src/db/queries.rs` (existing `#[cfg(test)]` mod if present, else add one)

- [ ] **Step 1: Add the function**

After `find_all_glosses` closes, add a near-identical fn that drops the
`end_citation` predicate and orders reader-gloss first, then by timestamp:

```rust
/// Like `find_all_glosses` but matches on START citation only (any end/span),
/// so glosses anchored to different-length passages that share a first line
/// co-list and cycle together. Reader-gloss rows sort first, then by recency.
pub fn find_glosses_by_start(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    gloss_types: &[&str],
) -> Result<Vec<SavedGloss>, rusqlite::Error> {
    if gloss_types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..gloss_types.len())
        .map(|i| format!("?{}", i + 3))
        .collect();
    let sql = format!(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id, g.gloss_type \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND g.gloss_type IN ({}) \
         ORDER BY (g.gloss_type = 'reader-gloss') DESC, g.timestamp DESC",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(work_abbrev.to_string()));
    params.push(Box::new(start_citation.to_string()));
    for gt in gloss_types {
        params.push(Box::new(gt.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(SavedGloss {
            gloss_id: row.get(0)?,
            gloss_text: row.get(1)?,
            timestamp: row.get(2)?,
            passage_id: row.get(3)?,
            gloss_type: row.get(4)?,
        })
    })?;
    rows.collect()
}
```

NOTE: verify the `SavedGloss` field order/names against the struct
(`src/db/queries.rs`, `gloss_id, passage_id, gloss_text, timestamp, gloss_type`)
and match `find_all_glosses`'s exact row-mapping order (it selects
`g.id, g.gloss_text, g.timestamp, p.id, g.gloss_type`). Mirror it precisely.

- [ ] **Step 2: Build**

Run: `cargo build` — clean (the fn is unused until Task 2; expect a temporary
dead-code warning, resolved next task).

- [ ] **Step 3: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(gloss): find_glosses_by_start (start-only match, reader-first)"
```

---

### Task 2: Use start-grouping where the overlay gloss_list is built

**Files:**
- Modify: `src/input/visual.rs` — the THREE overlay-open/refresh sites for the
  action paths (cached-open at ~425, post-save refetch at ~505 for reader;
  the teacher path ~560/640 and inner ~700/899). FOCUS: every site that
  populates `s.gloss_list` for DISPLAY should group by start. The post-save
  `save_gloss` re-fetch may stay exact OR switch to start — see decision below.
- Modify: `src/input/actions/gloss.rs:113` (navigate_gloss_passage open) and
  `:736`, `:843` (add/edit refresh) and `:1981`.
- Modify: `src/input/keymap.rs:404` (picker-confirm open path).

**Decision (apply consistently):** Every place that builds the `gloss_list`
shown in the overlay switches from
`find_all_glosses(conn, work, start, end, TYPES)` to
`find_glosses_by_start(conn, work, start, TYPES)` where `TYPES` is the
three-type array. This makes the displayed/cycled list start-keyed everywhere.
(Leave the citations used for the cache-hash and `save_gloss` arguments
unchanged — only the LIST FETCH changes.)

- [ ] **Step 1: Replace each display-list fetch**

For each call that assigns into `gloss_list` (or `all`/`all_glosses` that then
becomes `gloss_list`), change:
```rust
crate::db::queries::find_all_glosses(
    &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
    &[...types...],
)
```
to:
```rust
crate::db::queries::find_glosses_by_start(
    &conn, &ctx.work_abbrev, &ctx.start_citation,
    &[...types...],
)
```
Apply at: `visual.rs:425, 505, 560, 640, 700, 899`; `gloss.rs:113, 736, 843, 1981`;
`keymap.rs:404`. For `keymap.rs:404` the vars are `passage.work_abbrev` /
`passage.start_citation` (drop `passage.end_citation`).

DO NOT change `synopsis.rs:331` — that is the synopsis-batch passage iteration,
not the reader gloss overlay; leave it exact-match.

For each site, also set the initial selected index from the new list. The
existing code does `s.gloss_index = 0` after fetch — keep that; since the query
now sorts reader-gloss first, index 0 is the reader-gloss when present (satisfies
"reader-gloss first" in the overlay too).

- [ ] **Step 2: Build + verify the cycle list**

Run: `cargo build` — clean, no dead-code warning for `find_glosses_by_start`
(now used). Verify `find_all_glosses` is still used by `synopsis.rs:331` and any
save-identity path so it is NOT dead.

Run: `cargo test --bins 2>&1 | rg 'test result'` — PASS.

- [ ] **Step 3: Commit**

```bash
git add src/input/visual.rs src/input/actions/gloss.rs src/input/keymap.rs
git commit -m "feat(gloss): overlay gloss_list groups by start line (cycles all spans)"
```

---

### Task 3: Default the Ctrl+g picker to the reader-gloss filter

**Files:**
- Modify: `src/input/actions/pickers.rs` — `GlossPickerFilter` default + the two
  `GlossPickerFilter::default()` uses in `open_gloss_picker`.

The picker opens on `GlossPickerFilter::default()` which is `TeacherGeneric`.
Change the default to `ReaderGloss` so reader-glosses show first; Ctrl+t still
cycles to the other two.

- [ ] **Step 1: Move `#[default]` to ReaderGloss**

In the `GlossPickerFilter` enum, move the `#[default]` attribute from
`TeacherGeneric` to `ReaderGloss`:
```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum GlossPickerFilter {
    TeacherGeneric,
    InnerMonologue,
    #[default]
    ReaderGloss,
}
```
Leave `next()` and `gloss_type()` unchanged — the cycle order
(teacher→monologue→reader→teacher) still works; only the STARTING state changes
to ReaderGloss. (After one Ctrl+t from reader it goes to teacher, etc.)

- [ ] **Step 2: Build**

Run: `cargo build` — clean. `open_gloss_picker` already uses
`GlossPickerFilter::default()`, so it now opens filtered to reader-gloss and the
placeholder reads "Filter reader-gloss glosses... (Ctrl+t toggle)".

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/pickers.rs
git commit -m "feat(gloss): Ctrl+g picker defaults to reader-gloss filter"
```

---

### Task 4: Build/test/clippy pass

- [ ] **Step 1:** `cargo test --bins 2>&1 | rg 'test result'` → PASS.
- [ ] **Step 2:** `cargo clippy --bins 2>&1 | rg -i 'queries.rs|visual.rs|pickers.rs|gloss.rs|keymap.rs' | rg -i 'warning|error'` → no new warnings.
- [ ] **Step 3:** commit any clippy fixes (skip if none).

---

### Task 5: User-run runtime verification

Per project rule, the agent does NOT run the app. Ask the user to `cargo run`
and verify on the 2H6 2.1.1 "Believe me, lords" passage:
1. Open the gloss overlay there — confirm BOTH the reader-gloss (21749) and the
   teacher-gloss (21746) are present and Ctrl+n/p cycles between them, with the
   reader-gloss shown first.
2. Open Ctrl+g — confirm it opens on the **reader-gloss** filter (placeholder
   "Filter reader-gloss glosses…") and the "Believe me, lords" reader row is
   listed; Ctrl+t cycles to teacher-generic (showing 21746's row) and monologue.
3. Confirm no regression on a single-gloss passage (cycle wraps at 1).

---

## Self-Review Notes
- Start-only fetch fixes overlay co-listing + Ctrl+n/p for ALL same-start
  glosses regardless of span (Q1 = "group by start line").
- `ORDER BY (gloss_type='reader-gloss') DESC, timestamp DESC` puts reader first
  in the overlay list; `#[default] ReaderGloss` puts reader first in the picker
  (Q2 = "picker default = reader-gloss").
- `find_all_glosses` retained for `synopsis.rs:331` (and any save-identity use),
  so it is not dead.
- No prompt/DB changes; pure query + UI-state change.
