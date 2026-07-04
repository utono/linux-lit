# Pinned Play Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Store each Shakespeare play's page spreads in lit.db by citation
(`line_mapping` ids), generated and validated in-app at the live layout, so
two-column navigation becomes table lookups instead of runtime heuristics.

**Architecture:** A new `src/db/play_pages.rs` persists spreads keyed by
`(canonical_abbrev, layout_fingerprint)`. A new `src/input/page_table.rs`
holds the pure invariant suite, the fingerprint, the generator (records the
existing live engine's forward walk once, validates, stores), and the runtime
`PageTable`. `navigation.rs` and `scroll.rs` take the table path when
`active_page_table()` is `Some`; every fallback mode keeps today's live
engine untouched. A read-only `validate-play-pages` skill audits stored
tables via SQL.

**Design doc:** `docs/plans/2026-07-04-pinned-play-pagination-design.md`.
One amendment made here: tables are keyed by `(work_abbrev,
layout_fingerprint)` — not abbrev alone — so a headless test generation
(1280×720) can never clobber the production (1920×1200) rows.

**Tech Stack:** Rust, rusqlite, GTK4/sourceview5, cargo test, nav-fuzz
harness (cage/grim/wtype), sqlite3 CLI (skill).

## Global Constraints

- Never `cargo run` — the user launches the app; agents verify headlessly.
- Kill headless instances ONLY with `pkill -f "cage -- ./target/debug/linux-lit"`.
- Branch `feat/pinned-play-pagination` off `master`; finish by merging back
  `--no-ff`, re-verifying, pushing, deleting the branch.
- The 6 pre-existing dirty files (app/mod.rs's `warm_word_cache` hunk,
  concordance.rs, keymap.rs, keymap_config.rs, journal_keybinds_overlay.rs,
  keybinds_overlay.rs) are the USER's uncommitted work — never commit or
  revert them. When committing app/mod.rs changes, stage hunks selectively
  (`git diff`-filtered patch + `git apply --cached`, as done for the
  clip-boundary work).
- lit.db lives at `~/utono/litdb/data/lit.db`; `open_db()` is read-only,
  `open_db_rw()` writes (`src/db/queries.rs:26,741`).
- Pagination reads authoritative metadata (`section_starts`,
  `is_dialogue_line`) — never re-infer structure from buffer text.
- The shared logs (`linux-lit-dev.log` / `linux-lit-release.log`) may belong
  to the user's live instance — trust screenshots and cargo test output, not
  log tails, whenever a live instance may be running.
- `cargo test --bins` currently has 2 PRE-EXISTING failures unrelated to this
  work (`alt_bracketleft_is_toggle_column_layout`, `test_list_works`) — they
  do not block, but no NEW failures are allowed.

---

### Task 0: Branch

- [ ] **Step 1:**

```bash
cd ~/utono/linux-lit && git checkout -b feat/pinned-play-pagination master
```

---

### Task 1: DB layer — `src/db/play_pages.rs`

**Files:**
- Create: `src/db/play_pages.rs`
- Modify: `src/db/mod.rs` (add `pub mod play_pages;`)
- Test: `#[cfg(test)]` module inside `src/db/play_pages.rs`

**Interfaces:**
- Consumes: `rusqlite::Connection` (caller opens via `queries::open_db()` /
  `open_db_rw()`).
- Produces:
  - `pub struct PageRow { pub page_no: i64, pub left_start_id: i64, pub split_id: Option<i64>, pub end_id: i64 }`
  - `pub struct PagesMeta { pub layout_fingerprint: String, pub db_fingerprint: u64, pub page_count: i64, pub generated_at: String, pub validated: bool }`
  - `pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()>`
  - `pub fn load_pages(conn: &Connection, canonical_abbrev: &str, layout_fingerprint: &str) -> rusqlite::Result<Option<(PagesMeta, Vec<PageRow>)>>`
  - `pub fn store_pages(conn: &mut Connection, canonical_abbrev: &str, meta: &PagesMeta, rows: &[PageRow]) -> rusqlite::Result<()>` (one transaction; replaces any existing rows for that `(abbrev, fingerprint)`)

- [ ] **Step 1: Write the failing tests**

Create `src/db/play_pages.rs` containing ONLY the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    fn sample_meta() -> PagesMeta {
        PagesMeta {
            layout_fingerprint: "v1|abc".into(),
            db_fingerprint: 42,
            page_count: 2,
            generated_at: "2026-07-04T12:00:00Z".into(),
            validated: true,
        }
    }

    fn sample_rows() -> Vec<PageRow> {
        vec![
            PageRow { page_no: 1, left_start_id: 100, split_id: Some(140), end_id: 180 },
            PageRow { page_no: 2, left_start_id: 181, split_id: None, end_id: 200 },
        ]
    }

    #[test]
    fn roundtrips_pages_and_meta() {
        let mut conn = mem();
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()).unwrap();
        let (meta, rows) = load_pages(&conn, "MND", "v1|abc").unwrap().unwrap();
        assert_eq!(meta.db_fingerprint, 42);
        assert_eq!(meta.page_count, 2);
        assert!(meta.validated);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].split_id, Some(140));
        assert_eq!(rows[1].split_id, None);
        assert_eq!(rows[1].end_id, 200);
    }

    #[test]
    fn load_misses_on_wrong_fingerprint_or_abbrev() {
        let mut conn = mem();
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()).unwrap();
        assert!(load_pages(&conn, "MND", "v1|OTHER").unwrap().is_none());
        assert!(load_pages(&conn, "Ham", "v1|abc").unwrap().is_none());
    }

    #[test]
    fn store_replaces_same_key_only() {
        let mut conn = mem();
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()).unwrap();
        // A second layout's table coexists (the headless-vs-production case).
        let mut meta2 = sample_meta();
        meta2.layout_fingerprint = "v1|headless".into();
        store_pages(&mut conn, "MND", &meta2, &sample_rows()[..1]).unwrap();
        // Re-store the first layout with 1 row: replaces its rows, not layout 2's.
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()[..1]).unwrap();
        let (_, rows1) = load_pages(&conn, "MND", "v1|abc").unwrap().unwrap();
        let (_, rows2) = load_pages(&conn, "MND", "v1|headless").unwrap().unwrap();
        assert_eq!(rows1.len(), 1);
        assert_eq!(rows2.len(), 1);
    }

    #[test]
    fn unvalidated_meta_loads_as_none() {
        let mut conn = mem();
        let mut meta = sample_meta();
        meta.validated = false;
        store_pages(&mut conn, "MND", &meta, &sample_rows()).unwrap();
        assert!(load_pages(&conn, "MND", "v1|abc").unwrap().is_none());
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail**

Add `pub mod play_pages;` to `src/db/mod.rs`.
Run: `cargo test --bin linux-lit db::play_pages 2>&1 | tail -5`
Expected: FAIL — `ensure_schema`/`store_pages`/`load_pages` not found.

- [ ] **Step 3: Implement**

Add above the test module in `src/db/play_pages.rs`:

```rust
//! Persisted page spreads for two-column plays, keyed by citation
//! (`line_mapping` ids) and the layout fingerprint they were generated at.
//! See docs/plans/2026-07-04-pinned-play-pagination-design.md.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct PageRow {
    pub page_no: i64,
    pub left_start_id: i64,
    /// First line of the right column; None = empty right column (watermark).
    pub split_id: Option<i64>,
    /// Last line ON the page, inclusive.
    pub end_id: i64,
}

#[derive(Debug, Clone)]
pub struct PagesMeta {
    pub layout_fingerprint: String,
    pub db_fingerprint: u64,
    pub page_count: i64,
    pub generated_at: String,
    pub validated: bool,
}

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS play_pages (
             work_abbrev        TEXT NOT NULL,
             layout_fingerprint TEXT NOT NULL,
             page_no            INTEGER NOT NULL,
             left_start_id      INTEGER NOT NULL,
             split_id           INTEGER,
             end_id             INTEGER NOT NULL,
             PRIMARY KEY (work_abbrev, layout_fingerprint, page_no)
         );
         CREATE TABLE IF NOT EXISTS play_pages_meta (
             work_abbrev        TEXT NOT NULL,
             layout_fingerprint TEXT NOT NULL,
             db_fingerprint     TEXT NOT NULL,
             page_count         INTEGER NOT NULL,
             generated_at       TEXT NOT NULL,
             validated          INTEGER NOT NULL,
             PRIMARY KEY (work_abbrev, layout_fingerprint)
         );",
    )
}

pub fn load_pages(
    conn: &Connection,
    canonical_abbrev: &str,
    layout_fingerprint: &str,
) -> rusqlite::Result<Option<(PagesMeta, Vec<PageRow>)>> {
    let meta: Option<PagesMeta> = conn
        .query_row(
            "SELECT db_fingerprint, page_count, generated_at, validated
             FROM play_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
            params![canonical_abbrev, layout_fingerprint],
            |row| {
                let db_fp: String = row.get(0)?;
                Ok(PagesMeta {
                    layout_fingerprint: layout_fingerprint.to_string(),
                    db_fingerprint: db_fp.parse::<u64>().unwrap_or(0),
                    page_count: row.get(1)?,
                    generated_at: row.get(2)?,
                    validated: row.get::<_, i64>(3)? != 0,
                })
            },
        )
        .optional()?;
    let Some(meta) = meta else { return Ok(None) };
    if !meta.validated {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT page_no, left_start_id, split_id, end_id FROM play_pages
         WHERE work_abbrev = ?1 AND layout_fingerprint = ?2 ORDER BY page_no",
    )?;
    let rows = stmt
        .query_map(params![canonical_abbrev, layout_fingerprint], |row| {
            Ok(PageRow {
                page_no: row.get(0)?,
                left_start_id: row.get(1)?,
                split_id: row.get(2)?,
                end_id: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() as i64 != meta.page_count {
        return Ok(None); // partial write / manual tampering: treat as absent
    }
    Ok(Some((meta, rows)))
}

pub fn store_pages(
    conn: &mut Connection,
    canonical_abbrev: &str,
    meta: &PagesMeta,
    rows: &[PageRow],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM play_pages WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![canonical_abbrev, meta.layout_fingerprint],
    )?;
    tx.execute(
        "DELETE FROM play_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![canonical_abbrev, meta.layout_fingerprint],
    )?;
    for r in rows {
        tx.execute(
            "INSERT INTO play_pages
             (work_abbrev, layout_fingerprint, page_no, left_start_id, split_id, end_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![canonical_abbrev, meta.layout_fingerprint, r.page_no,
                    r.left_start_id, r.split_id, r.end_id],
        )?;
    }
    tx.execute(
        "INSERT INTO play_pages_meta
         (work_abbrev, layout_fingerprint, db_fingerprint, page_count, generated_at, validated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![canonical_abbrev, meta.layout_fingerprint,
                meta.db_fingerprint.to_string(), rows.len() as i64,
                meta.generated_at, meta.validated as i64],
    )?;
    tx.commit()
}
```

(`db_fingerprint` is stored as TEXT because it is a `u64` from
`snapshot::db_fingerprint` and SQLite INTEGER is i64.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin linux-lit db::play_pages 2>&1 | tail -3`
Expected: `test result: ok. 4 passed` (filtered).

- [ ] **Step 5: Commit**

```bash
git add src/db/play_pages.rs src/db/mod.rs
git commit -m "feat(db): play_pages schema + rw layer keyed by (abbrev, layout fingerprint)"
```

---

### Task 2: Pure spreads + invariant suite — `src/input/page_table.rs`

**Files:**
- Create: `src/input/page_table.rs`
- Modify: `src/input/mod.rs` (add `pub mod page_table;`)
- Test: `#[cfg(test)]` module inside `src/input/page_table.rs`

**Interfaces:**
- Produces:
  - `pub struct Spread { pub left_start: usize, pub split: Option<usize>, pub end: usize }` (buffer-line space; `end` inclusive)
  - `pub struct ValidateCtx<'a> { pub line_count: usize, pub is_dialogue: &'a [bool], pub section_starts: Option<&'a [bool]>, pub heights: &'a [i32], pub usable_height: i32 }`
  - `pub fn validate_spreads(spreads: &[Spread], ctx: &ValidateCtx) -> Result<(), String>`
  - `pub fn page_for_line(spreads: &[Spread], line: usize) -> Option<usize>` (binary search over `left_start`, verify containment via `end`)

- [ ] **Step 1: Write the failing tests**

Create `src/input/page_table.rs` with the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // 10 lines, all dialogue, uniform height 10, viewport fits 3+3 per spread.
    fn ctx(heights: &[i32], dlg: &[bool]) -> ValidateCtx<'_> {
        ValidateCtx {
            line_count: heights.len(),
            is_dialogue: dlg,
            section_starts: None,
            heights,
            usable_height: 30,
        }
    }

    fn ok_spreads() -> Vec<Spread> {
        vec![
            Spread { left_start: 0, split: Some(3), end: 5 },
            Spread { left_start: 6, split: Some(9), end: 9 },
        ]
    }

    #[test]
    fn valid_table_passes() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        assert!(validate_spreads(&ok_spreads(), &ctx(&h, &d)).is_ok());
    }

    #[test]
    fn gap_between_pages_fails_coverage() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let s = vec![
            Spread { left_start: 0, split: Some(3), end: 5 },
            Spread { left_start: 7, split: Some(9), end: 9 }, // line 6 dropped
        ];
        let err = validate_spreads(&s, &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn tail_not_reached_fails() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let s = vec![Spread { left_start: 0, split: Some(3), end: 5 }]; // 6..9 missing
        let err = validate_spreads(&s, &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("tail"), "got: {err}");
    }

    #[test]
    fn overfull_column_fails_fit() {
        let mut h = vec![10; 10];
        h[1] = 25; // left col 0..=2 sums to 45 > usable 30
        let d = vec![true; 10];
        let err = validate_spreads(&ok_spreads(), &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("fit"), "got: {err}");
    }

    #[test]
    fn disordered_split_fails_sanity() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let s = vec![
            Spread { left_start: 0, split: Some(7), end: 5 }, // split > end
            Spread { left_start: 6, split: Some(9), end: 9 },
        ];
        let err = validate_spreads(&s, &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("sanity"), "got: {err}");
    }

    #[test]
    fn empty_right_requires_section_start_next() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let mut ss = vec![false; 10];
        let s = vec![
            Spread { left_start: 0, split: None, end: 5 },
            Spread { left_start: 6, split: Some(9), end: 9 },
        ];
        // Without a section start at the next page top: fail.
        let c1 = ValidateCtx { section_starts: Some(&ss), ..ctx(&h, &d) };
        assert!(validate_spreads(&s, &c1).unwrap_err().contains("watermark"));
        // With it: pass.
        ss[6] = true;
        let c2 = ValidateCtx { section_starts: Some(&ss), ..ctx(&h, &d) };
        assert!(validate_spreads(&s, &c2).is_ok());
    }

    #[test]
    fn page_for_line_finds_containing_page() {
        let s = ok_spreads();
        assert_eq!(page_for_line(&s, 0), Some(0));
        assert_eq!(page_for_line(&s, 5), Some(0));
        assert_eq!(page_for_line(&s, 6), Some(1));
        assert_eq!(page_for_line(&s, 9), Some(1));
        assert_eq!(page_for_line(&s, 10), None);
    }
}
```

- [ ] **Step 2: Register module, run tests to verify they fail**

Add `pub mod page_table;` to `src/input/mod.rs`.
Run: `cargo test --bin linux-lit page_table 2>&1 | tail -5`
Expected: FAIL — types not found.

- [ ] **Step 3: Implement**

Add above the tests:

```rust
//! Pure page-table types + the invariant suite shared by the in-app generator
//! and (structurally) the validate-play-pages skill. Everything here is
//! GTK-free so it is unit-testable. Buffer-line space; `end` is inclusive.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub left_start: usize,
    /// First line of the right column; None = empty right (watermark spread).
    pub split: Option<usize>,
    pub end: usize,
}

pub struct ValidateCtx<'a> {
    pub line_count: usize,
    pub is_dialogue: &'a [bool],
    pub section_starts: Option<&'a [bool]>,
    /// Per-buffer-line pixel heights (line_yrange), measured at the layout
    /// the table is generated for.
    pub heights: &'a [i32],
    /// widget_height - descender_guard - BASE_BOTTOM_MARGIN at that layout.
    pub usable_height: i32,
}

/// The invariant suite (design doc §Invariant suite, items 1-4). Returns the
/// FIRST violated invariant as "<name>: <details>".
pub fn validate_spreads(spreads: &[Spread], ctx: &ValidateCtx) -> Result<(), String> {
    if spreads.is_empty() {
        return Err("coverage: no spreads".into());
    }
    // sanity + monotone, contiguous coverage
    let mut expect_start = spreads[0].left_start;
    if expect_start != 0 {
        return Err(format!("coverage: first page starts at {expect_start}, not 0"));
    }
    for (i, s) in spreads.iter().enumerate() {
        if s.left_start != expect_start {
            return Err(format!(
                "coverage: page {} starts at {} but previous page ended at {}",
                i + 1, s.left_start, expect_start.saturating_sub(1)
            ));
        }
        if let Some(sp) = s.split {
            if !(s.left_start <= sp && sp <= s.end + 1) {
                return Err(format!(
                    "sanity: page {} split {} outside [{}, {}]",
                    i + 1, sp, s.left_start, s.end + 1
                ));
            }
        }
        if s.end < s.left_start || s.end >= ctx.line_count {
            return Err(format!(
                "sanity: page {} end {} outside [{}, {})",
                i + 1, s.end, s.left_start, ctx.line_count
            ));
        }
        // watermark: an empty right column is only sanctioned when the NEXT
        // page opens a (div1,div2) section (authoritative bitmap, never text).
        if s.split.is_none() && i + 1 < spreads.len() {
            let next_top = spreads[i + 1].left_start;
            let opens_section = ctx
                .section_starts
                .and_then(|ss| ss.get(next_top).copied())
                .unwrap_or(false);
            if !opens_section {
                return Err(format!(
                    "watermark: page {} has an empty right column but page {} does not open a section",
                    i + 1, i + 2
                ));
            }
        }
        // fit: each column's summed heights must fit usable_height.
        let col_sum = |a: usize, b_incl: usize| -> i32 {
            ctx.heights[a..=b_incl.min(ctx.heights.len() - 1)].iter().sum()
        };
        let (left_end, right_range) = match s.split {
            Some(sp) if sp > s.left_start => (sp - 1, (sp <= s.end).then_some((sp, s.end))),
            Some(sp) => (s.left_start, (sp <= s.end).then_some((sp, s.end))), // empty left
            None => (s.end, None),
        };
        if left_end >= s.left_start && s.split != Some(s.left_start) {
            let sum = col_sum(s.left_start, left_end);
            if sum > ctx.usable_height {
                return Err(format!(
                    "fit: page {} left column {}..={} sums to {} > usable {}",
                    i + 1, s.left_start, left_end, sum, ctx.usable_height
                ));
            }
        }
        if let Some((a, b)) = right_range {
            let sum = col_sum(a, b);
            if sum > ctx.usable_height {
                return Err(format!(
                    "fit: page {} right column {}..={} sums to {} > usable {}",
                    i + 1, a, b, sum, ctx.usable_height
                ));
            }
        }
        expect_start = s.end + 1;
    }
    // tail: every dialogue line at/after the last page's end must be ON a page.
    let last_end = spreads.last().unwrap().end;
    if let Some(missed) = (last_end + 1..ctx.line_count)
        .find(|&i| ctx.is_dialogue.get(i).copied().unwrap_or(false))
    {
        return Err(format!(
            "tail: dialogue line {} lies past the last page (end {})",
            missed, last_end
        ));
    }
    Ok(())
}

/// The page whose [left_start, end] interval contains `line`.
pub fn page_for_line(spreads: &[Spread], line: usize) -> Option<usize> {
    let idx = spreads.partition_point(|s| s.left_start <= line);
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    (line <= spreads[i].end).then_some(i)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin linux-lit page_table 2>&1 | tail -3`
Expected: `test result: ok. 7 passed` (filtered).

- [ ] **Step 5: Commit**

```bash
git add src/input/page_table.rs src/input/mod.rs
git commit -m "feat(pagination): pure Spread type, invariant suite, page_for_line"
```

---

### Task 3: Layout fingerprint

**Files:**
- Modify: `src/input/page_table.rs` (append)
- Test: extend the same `#[cfg(test)]` module

**Interfaces:**
- Produces:
  - `pub fn fingerprint_string(parts: &FingerprintParts) -> String` (pure)
  - `pub struct FingerprintParts { pub font_family: String, pub font_size: u32, pub ascent: i32, pub descent: i32, pub char_width: i32, pub width: i32, pub height: i32, pub line_spacing: u32, pub text_margins: u32, pub columns: u8 }`
  - `pub fn layout_fingerprint(state: &crate::app::AppState) -> String` (GTK wrapper)

- [ ] **Step 1: Write the failing test** (append to the tests module)

```rust
    #[test]
    fn fingerprint_is_stable_and_input_sensitive() {
        let p = FingerprintParts {
            font_family: "Charter".into(), font_size: 17,
            ascent: 16, descent: 5, char_width: 9,
            width: 1920, height: 1200, line_spacing: 6, text_margins: 24,
            columns: 2,
        };
        let a = fingerprint_string(&p);
        assert_eq!(a, fingerprint_string(&p), "must be deterministic");
        assert!(a.starts_with("v1|"), "schema-versioned: {a}");
        let mut q = FingerprintParts { font_size: 18, ..p };
        assert_ne!(a, fingerprint_string(&q));
        q = FingerprintParts { descent: 6, font_size: 17, ..q };
        assert_ne!(a, fingerprint_string(&q));
    }
```

(`FingerprintParts` needs `#[derive(Clone)]` for the struct-update syntax.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin linux-lit fingerprint_is_stable 2>&1 | tail -3`
Expected: FAIL — `FingerprintParts` not found.

- [ ] **Step 3: Implement** (append above the tests)

```rust
/// Everything the page geometry depends on. `ascent`/`descent`/`char_width`
/// come from a Pango metrics probe of the ACTIVE font so a font-stack upgrade
/// that changes metrics at the same nominal size invalidates stored tables.
#[derive(Debug, Clone)]
pub struct FingerprintParts {
    pub font_family: String,
    pub font_size: u32,
    pub ascent: i32,
    pub descent: i32,
    pub char_width: i32,
    pub width: i32,
    pub height: i32,
    pub line_spacing: u32,
    pub text_margins: u32,
    pub columns: u8,
}

/// "v1|" + the parts, pipe-joined. Human-readable on purpose: the
/// validate-play-pages skill prints it verbatim so a stale table is
/// self-explaining (you can see WHICH input moved).
pub fn fingerprint_string(p: &FingerprintParts) -> String {
    format!(
        "v1|{}|{}|{}|{}|{}|{}x{}|{}|{}|{}",
        p.font_family, p.font_size, p.ascent, p.descent, p.char_width,
        p.width, p.height, p.line_spacing, p.text_margins, p.columns
    )
}

/// GTK wrapper: probe the live view. Uses the same font source as
/// `descender_guard_px` (the `font-size` tag, avoiding the CSS-application
/// race) and the toplevel window size (the dwl-tiled size, e.g. 1920x1200).
pub fn layout_fingerprint(state: &crate::app::AppState) -> String {
    use gtk4::prelude::{TextTagExt, TextBufferExt, WidgetExt};
    let ctx = state.text_view.pango_context();
    let font_desc = state
        .text_view
        .buffer()
        .tag_table()
        .lookup("font-size")
        .and_then(|tag| tag.font_desc());
    let metrics = ctx.metrics(font_desc.as_ref(), None);
    let parts = FingerprintParts {
        font_family: state.config.font_family.clone(),
        font_size: state.config.font_size,
        ascent: metrics.ascent() / pango::SCALE,
        descent: metrics.descent() / pango::SCALE,
        char_width: metrics.approximate_char_width() / pango::SCALE,
        width: state.window.width(),
        height: state.window.height(),
        line_spacing: state.config.line_spacing,
        text_margins: state.config.text_margins,
        columns: state.column_count(),
    };
    fingerprint_string(&parts)
}
```

NOTE for the implementer: check the real field names before building —
`state.config.font_family` / `font_size` / `line_spacing` / `text_margins`
exist in `src/config.rs`; `state.column_count()` returns the resolved column
count (u8 or usize — cast to match `FingerprintParts.columns: u8`). If
`font_family` is named differently (e.g. `font`), use the actual name in BOTH
the wrapper and nothing else (the pure fn takes strings).

- [ ] **Step 4: Run tests**

Run: `cargo test --bin linux-lit page_table 2>&1 | tail -3`
Expected: all page_table tests pass (8 now).

- [ ] **Step 5: Commit**

```bash
git add src/input/page_table.rs
git commit -m "feat(pagination): layout fingerprint (pure composition + live probe)"
```

---

### Task 4: Generator — record the live walk, validate, store

**Files:**
- Modify: `src/input/page_table.rs` (append `record_spreads`, `generate_and_store`)
- Modify: `src/app/mod.rs` (AppState field + one hook call; STAGE SELECTIVELY —
  see Global Constraints)
- Test: manual/headless (no pure seam for the walk itself; the invariant suite
  from Task 2 is the gate)

**Interfaces:**
- Consumes: `super::viewport::{next_page_top, column_split, descender_guard_px, is_dialogue_line}`, `crate::input::scroll::BASE_BOTTOM_MARGIN`, `crate::snapshot::db_fingerprint`, `crate::db::play_pages`, Task 2/3 items.
- Produces:
  - `pub fn record_spreads(state: &crate::app::AppState) -> Result<Vec<Spread>, String>`
  - `pub fn generate_and_store(state: &crate::app::AppState)` (all gating inside; safe to call unconditionally from the hook)
  - New AppState field: `pub page_table_gen_attempted: std::cell::Cell<bool>` (reset to `false` in `display_work` when a new work loads)

- [ ] **Step 1: Implement `record_spreads`** (append to page_table.rs)

```rust
/// Walk the LIVE engine's forward chain once, recording every spread. This is
/// the same chain `x` follows (next_page_top/column_split), so the recorded
/// table reproduces exactly what paging forward shows. Includes the
/// determinism check (design invariant 5): re-deriving a page's boundary from
/// its own top must agree with what the chain said.
pub fn record_spreads(state: &crate::app::AppState) -> Result<Vec<Spread>, String> {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Err("no lines".into());
    }
    let mut spreads = Vec::new();
    let mut top = 0usize;
    let mut guard = 0usize;
    loop {
        let cs = crate::input::viewport::column_split(state, top);
        let split = if cs.split <= top || cs.split > cs.page_end {
            None // empty right column (watermark) or empty left handled below
        } else {
            Some(cs.split)
        };
        // Empty LEFT column (first-spread short-opening): cs.split == top.
        let split = if cs.split == top { Some(top) } else { split };
        let end = cs.page_end.min(line_count.saturating_sub(1));
        spreads.push(Spread { left_start: top, split, end });
        let next = crate::input::viewport::next_page_top(state, top).new_top;
        if next >= line_count || next <= top {
            break;
        }
        // Determinism (invariant 5): column_split at `next` must not claim
        // lines this spread already covered.
        if next <= end && crate::input::viewport::column_split(state, next).page_end <= end {
            return Err(format!("determinism: chain from {top} regressed at {next}"));
        }
        top = next;
        guard += 1;
        if guard > line_count {
            return Err("determinism: forward chain did not terminate".into());
        }
    }
    // The final spread must be the same anchor G uses.
    let anchor = crate::input::navigation::last_page_top(state);
    if spreads.last().map(|s| s.left_start) != Some(anchor) {
        // Replace the trailing spread(s) with the canonical final spread so the
        // chain and G agree (the dialogue-tail pull-forward case).
        while spreads.last().map_or(false, |s| s.left_start >= anchor) {
            spreads.pop();
        }
        let cs = crate::input::viewport::column_split(state, anchor);
        let split = (cs.split > anchor && cs.split <= cs.page_end).then_some(cs.split);
        spreads.push(Spread {
            left_start: anchor,
            split,
            end: cs.page_end.min(line_count.saturating_sub(1)),
        });
        // Truncate the previous spread so coverage stays contiguous.
        let n = spreads.len();
        if n >= 2 {
            let prev_end = anchor.saturating_sub(1);
            let prev = &mut spreads[n - 2];
            if prev.end >= anchor {
                prev.end = prev_end;
                if prev.split.map_or(false, |sp| sp > prev_end) {
                    prev.split = None;
                }
            }
        }
    }
    Ok(spreads)
}
```

NOTE: `last_page_top` is `pub(crate)` in `src/input/navigation.rs` — it is
already visible from `input::page_table`. `ColumnSplit`'s fields used here
(`split`, `page_end`) exist; check `next_page_top`'s return struct name
(`NextPage { new_top, .. }`) in `viewport.rs` and match it.

- [ ] **Step 2: Implement `generate_and_store`** (append)

```rust
/// Gate, record, validate, persist. Called from the app's settled-layout hook;
/// every early return logs its reason so the fallback is diagnosable.
pub fn generate_and_store(state: &crate::app::AppState) {
    if std::env::var_os("LIT_NO_PAGE_TABLE").is_some() {
        return;
    }
    let force = std::env::var_os("LIT_GEN_PAGE_TABLE").is_some();
    if state.page_table_gen_attempted.get() {
        return;
    }
    state.page_table_gen_attempted.set(true);
    let Some(work) = state.current_work.as_ref() else { return };
    if state.column_count() != 2 || state.translations_visible {
        crate::logging::log("PAGES: gen skipped (not 2-col reader state)");
        return;
    }
    if state.page_table.borrow().is_some() && !force {
        return; // already loaded from the DB this session
    }
    let fp = layout_fingerprint(state);
    let spreads = match record_spreads(state) {
        Ok(s) => s,
        Err(e) => {
            crate::logging::log(&format!("PAGES: VALIDATE_FAIL {e}"));
            return;
        }
    };
    // Build the validation context from live geometry + authoritative metadata.
    let line_count = state.effective_line_count();
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let is_dialogue: Vec<bool> = (0..line_count)
        .map(|i| crate::input::viewport::is_dialogue_line(
            &state.buffer, i, state.is_prose(), &stage_lookup))
        .collect();
    let heights: Vec<i32> = (0..line_count)
        .map(|i| state.buffer.iter_at_line(i as i32)
            .map(|it| state.text_view.line_yrange(&it).1)
            .unwrap_or(0))
        .collect();
    let widget_height = state.text_view.height();
    let guard = crate::input::viewport::descender_guard_px(&state.text_view, 0);
    let usable = widget_height - guard - crate::input::scroll::BASE_BOTTOM_MARGIN;
    let ss_vec = state.section_starts().map(|s| s.to_vec());
    let ctx = ValidateCtx {
        line_count,
        is_dialogue: &is_dialogue,
        section_starts: ss_vec.as_deref(),
        heights: &heights,
        usable_height: usable,
    };
    if let Err(e) = validate_spreads(&spreads, &ctx) {
        crate::logging::log(&format!("PAGES: VALIDATE_FAIL {e}"));
        return;
    }
    // Map buffer lines -> line_mapping ids. Boundary lines are always work
    // lines (page tops/splits land on real content), so a missing mapping is
    // a hard failure, not something to paper over.
    let id_of = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| work.lines.get(wi))
            .map(|l| l.id)
    };
    let mut rows = Vec::with_capacity(spreads.len());
    for (i, s) in spreads.iter().enumerate() {
        let (Some(ls), Some(end)) = (id_of(s.left_start), id_of(s.end)) else {
            crate::logging::log(&format!(
                "PAGES: VALIDATE_FAIL citation: page {} boundary has no line_mapping id", i + 1));
            return;
        };
        let split_id = match s.split {
            Some(sp) => match id_of(sp) {
                Some(v) => Some(v),
                None => {
                    crate::logging::log(&format!(
                        "PAGES: VALIDATE_FAIL citation: page {} split has no id", i + 1));
                    return;
                }
            },
            None => None,
        };
        rows.push(crate::db::play_pages::PageRow {
            page_no: (i + 1) as i64,
            left_start_id: ls,
            split_id,
            end_id: end,
        });
    }
    let meta = crate::db::play_pages::PagesMeta {
        layout_fingerprint: fp.clone(),
        db_fingerprint: crate::snapshot::db_fingerprint(work),
        page_count: rows.len() as i64,
        generated_at: chrono_free_timestamp(),
        validated: true,
    };
    match crate::db::queries::open_db_rw() {
        Ok(mut conn) => {
            if let Err(e) = crate::db::play_pages::ensure_schema(&conn)
                .and_then(|_| crate::db::play_pages::store_pages(
                    &mut conn, &work.canonical_abbrev, &meta, &rows))
            {
                crate::logging::log(&format!("PAGES: store failed ({e}) — will retry next load"));
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("PAGES: db open failed ({e})"));
            return;
        }
    }
    crate::logging::log(&format!(
        "PAGES: generated {} pages for {} fp={}", rows.len(), work.canonical_abbrev, fp));
    // Make the fresh table active this session.
    *state.page_table.borrow_mut() = Some(std::rc::Rc::new(spreads));
}

/// US-Central-ish ISO timestamp without adding a chrono dependency: SQLite's
/// own clock via a throwaway query would need a connection; use std time UTC.
fn chrono_free_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{now}")
}
```

NOTE: `state.page_table` is added in Task 5 Step 1 — implement Tasks 4 and 5's
state fields together if compiling between them matters; the plan orders the
field additions first in Task 5, so an alternative is to implement Task 5
Step 1 (fields only) before this step. Either order is fine as long as the
commit at the end of THIS task compiles. If the repo already has `chrono` in
Cargo.toml, use `chrono::Local::now().to_rfc3339()` instead of the epoch
format (check `rg chrono Cargo.toml`).

- [ ] **Step 3: Add the AppState fields + hook** (`src/app/mod.rs`)

Next to `prev_highlight_line` in `pub struct AppState` add:

```rust
    /// Pinned play page table (buffer-line space), loaded from lit.db when the
    /// layout fingerprint matches, or generated+stored after first settled
    /// layout. None = live engine. See input::page_table.
    pub page_table: std::cell::RefCell<Option<std::rc::Rc<Vec<crate::input::page_table::Spread>>>>,
    /// One generation attempt per work load (reset in display_work).
    pub page_table_gen_attempted: std::cell::Cell<bool>,
```

Initializer additions:

```rust
        page_table: std::cell::RefCell::new(None),
        page_table_gen_attempted: std::cell::Cell::new(false),
```

Hook: find the resize-tick branch that logs `"RESIZE_TICK: deferred layout
refresh"` (search that string in `src/app/mod.rs`). At its end — after the
refresh work — add:

```rust
            // Pinned play pagination: once layout is settled, generate+store
            // the page table if this work/layout doesn't have one (no-op when
            // one was already loaded from lit.db, or on any fallback mode).
            {
                let st = state_rc.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                    let s = st.borrow();
                    crate::input::page_table::generate_and_store(&s);
                });
            }
```

(Adapt the Rc/RefCell variable name to what that closure actually holds —
the surrounding code in the resize tick already clones an
`Rc<RefCell<AppState>>`; reuse its pattern. The 400ms delay keeps generation
off the reveal path.)

In `display_work` (where per-work state is reset — near where
`state.translations_visible = false;` is set), add:

```rust
    state.page_table_gen_attempted.set(false);
    *state.page_table.borrow_mut() = None;
```

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | tail -1`
Expected: Finished, no new errors. (Task 5 Step 1's fields are required —
if you deferred them, add them now.)

- [ ] **Step 5: Commit (selective staging for app/mod.rs)**

```bash
cd ~/utono/linux-lit
git diff src/app/mod.rs | python3 -c "
import sys
lines = sys.stdin.read().split('\n')
header = lines[:4]
hunks, cur = [], None
for l in lines[4:]:
    if l.startswith('@@'):
        if cur: hunks.append(cur)
        cur = [l]
    elif cur is not None:
        cur.append(l)
if cur: hunks.append(cur)
mine = [h for h in hunks if 'page_table' in '\n'.join(h)]
print('\n'.join(header + [l for h in mine for l in h]))
" > /tmp/pages-appmod.patch
git apply --cached /tmp/pages-appmod.patch
git add src/input/page_table.rs
git commit -m "feat(pagination): in-app page-table generator with invariant gate"
```

---

### Task 5: Runtime consumption — load, gate, navigate, clip

**Files:**
- Modify: `src/input/page_table.rs` (append `load_for_work`, `active_page_table`)
- Modify: `src/app/mod.rs` (call `load_for_work` in `display_work` after the
  line map is ready; selective staging again)
- Modify: `src/input/navigation.rs` (`page_forward`, `page_backward`,
  `jump_to_end`, `canonical_page_top_for`)
- Modify: `src/input/scroll.rs` (`snap_scroll_to_line_offset`: table spread
  replaces `column_split` for exact ends)

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces:
  - `pub fn load_for_work(state: &crate::app::AppState)` — reads lit.db, checks
    both fingerprints, resolves ids→buffer lines, sets `state.page_table`.
  - `pub fn active_page_table(state: &crate::app::AppState) -> Option<std::rc::Rc<Vec<Spread>>>`
    — the ONE gate every consumer calls: `None` when table absent, or
    `LIT_NO_PAGE_TABLE` set, or `translations_visible`, or scroll mode, or
    `column_count() != 2`.
  - `pub fn spread_for_top(spreads: &[Spread], top: usize) -> Option<&Spread>`
    (exact `left_start` match).

- [ ] **Step 1: Implement load + gate** (append to page_table.rs)

```rust
/// Load a stored table for the current work if BOTH fingerprints match.
/// Resolves line_mapping ids to buffer lines via the id->buffer map built
/// from the live line map; any unresolvable id drops the whole table (stale
/// after re-import — db_fingerprint should have caught it, belt+braces).
pub fn load_for_work(state: &crate::app::AppState) {
    *state.page_table.borrow_mut() = None;
    if std::env::var_os("LIT_NO_PAGE_TABLE").is_some() {
        return;
    }
    let Some(work) = state.current_work.as_ref() else { return };
    if state.column_count() != 2 {
        return;
    }
    let fp = layout_fingerprint(state);
    let Ok(conn) = crate::db::queries::open_db() else { return };
    // Schema may not exist yet on a fresh lit.db; open_db is read-only, so
    // just probe and bail quietly.
    let loaded = match crate::db::play_pages::load_pages(&conn, &work.canonical_abbrev, &fp) {
        Ok(v) => v,
        Err(_) => None, // missing tables etc.
    };
    let Some((meta, rows)) = loaded else {
        crate::logging::log(&format!("PAGES: no table for {} fp={}", work.canonical_abbrev, fp));
        return;
    };
    if meta.db_fingerprint != crate::snapshot::db_fingerprint(work) {
        crate::logging::log("PAGES: fallback (db_fingerprint stale — re-import?)");
        return;
    }
    // id -> buffer line, built once.
    let line_count = state.effective_line_count();
    let mut id_to_buf = std::collections::HashMap::new();
    for bi in 0..line_count {
        if let Some(wi) = state.work_line_for_buffer(bi) {
            if let Some(l) = work.lines.get(wi) {
                id_to_buf.entry(l.id).or_insert(bi);
            }
        }
    }
    let mut spreads = Vec::with_capacity(rows.len());
    for r in &rows {
        let (Some(&ls), Some(&end)) = (id_to_buf.get(&r.left_start_id), id_to_buf.get(&r.end_id)) else {
            crate::logging::log("PAGES: fallback (row id not in buffer)");
            return;
        };
        let split = match r.split_id {
            Some(id) => match id_to_buf.get(&id) {
                Some(&b) => Some(b),
                None => {
                    crate::logging::log("PAGES: fallback (split id not in buffer)");
                    return;
                }
            },
            None => None,
        };
        spreads.push(Spread { left_start: ls, split, end });
    }
    crate::logging::log(&format!(
        "PAGES: table hit ({} pages) for {}", spreads.len(), work.canonical_abbrev));
    *state.page_table.borrow_mut() = Some(std::rc::Rc::new(spreads));
}

/// The single consumption gate. Every navigation/render consumer goes through
/// this; adding a fallback mode means adding ONE condition here.
pub fn active_page_table(state: &crate::app::AppState) -> Option<std::rc::Rc<Vec<Spread>>> {
    if std::env::var_os("LIT_NO_PAGE_TABLE").is_some() {
        return None;
    }
    if state.translations_visible
        || state.column_count() != 2
        || !matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader)
    {
        return None;
    }
    state.page_table.borrow().clone()
}

/// The spread whose top is exactly `top` (page tops are canonical).
pub fn spread_for_top(spreads: &[Spread], top: usize) -> Option<&Spread> {
    spreads.iter().find(|s| s.left_start == top)
}
```

- [ ] **Step 2: Call `load_for_work` in `display_work`**

In `src/app/mod.rs`, immediately after the point where the line map is ready
and per-work state was reset in Task 4 Step 3 (the same block), replace the
plain `*state.page_table.borrow_mut() = None;` with:

```rust
    state.page_table_gen_attempted.set(false);
    crate::input::page_table::load_for_work(state);
```

(`load_for_work` starts by clearing the cell, so the None-reset is subsumed.
It must run AFTER `current_work`, the buffer, and the line map are set —
place it at the END of the layout-independent part of `display_work`; the
fingerprint uses window size, which is valid by then. If `display_work` runs
before first layout (width 0), the fingerprint won't match and the table
loads on the resize-tick generation hook instead — `generate_and_store`
already sets `state.page_table` on success, and an existing DB row will be
found by the NEXT load. To make the common path deterministic, ALSO call
`crate::input::page_table::load_for_work(&s)` at the top of the Task 4 hook
closure, before `generate_and_store(&s)`.)

- [ ] **Step 3: Navigation — table branches** (`src/input/navigation.rs`)

At the TOP of `page_forward` (after the `page_turn_lock` check), add:

```rust
    // Pinned page table: navigation is index arithmetic; none of the
    // heuristics below run. See input::page_table / the design doc.
    if let Some(table) = crate::input::page_table::active_page_table(state) {
        let cur = crate::input::page_table::page_for_line(&table, state.page_top_line)
            .unwrap_or(0);
        if cur + 1 >= table.len() {
            // Final page: move the highlight to the last on-page dialogue line.
            let s = table[cur];
            let stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            let last_dlg = prev_dialogue_line(&state.buffer, &state.translation_lines,
                    s.end + 1, state.is_prose(), &stage_lookup)
                .filter(|&d| d >= s.left_start)
                .unwrap_or(s.end);
            if last_dlg > state.current_line {
                state.current_line = last_dlg;
                after_page_change(state, PageChangeReason::Forward);
            }
            log_fmt!("PAGES: page {}/{} (at end)", cur + 1, table.len());
            return;
        }
        let next = table[cur + 1];
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        let first_dlg = next_dialogue_from(&state.buffer, next.left_start,
            state.effective_line_count(), state.is_prose(), &stage_lookup);
        state.page_back_stack.push((state.page_top_line, state.page_top_offset));
        state.current_line = if cur + 2 == table.len() {
            // Landing ON the final page: last on-page dialogue (matches the
            // redirect_to_final_spread landing rule).
            prev_dialogue_line(&state.buffer, &state.translation_lines,
                    next.end + 1, state.is_prose(), &stage_lookup)
                .filter(|&d| d >= next.left_start)
                .unwrap_or(first_dlg.min(next.end))
        } else {
            first_dlg.min(next.end)
        };
        set_page(state, next.left_start, PageDirection::Forward);
        after_page_change(state, PageChangeReason::Forward);
        log_fmt!("PAGES: page {}/{} top={}", cur + 2, table.len(), next.left_start);
        return;
    }
```

At the TOP of `page_backward` (after its lock check, BEFORE the first-spread
guard), add:

```rust
    if let Some(table) = crate::input::page_table::active_page_table(state) {
        let cur = crate::input::page_table::page_for_line(&table, state.page_top_line)
            .unwrap_or(0);
        if cur == 0 {
            let first = first_dialogue_line(state);
            if first < state.current_line {
                state.current_line = first;
                after_page_change(state, PageChangeReason::Backward);
            }
            log_fmt!("PAGES: page 1/{} (at start)", table.len());
            return;
        }
        let prev = table[cur - 1];
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        // Landing rule mirrors the live engine: last visible dialogue on the
        // previous page.
        state.current_line = prev_dialogue_line(&state.buffer, &state.translation_lines,
                prev.end + 1, state.is_prose(), &stage_lookup)
            .filter(|&d| d >= prev.left_start)
            .unwrap_or(prev.left_start);
        set_page(state, prev.left_start, PageDirection::Backward);
        after_page_change(state, PageChangeReason::Backward);
        log_fmt!("PAGES: page {}/{} top={}", cur, table.len(), prev.left_start);
        return;
    }
```

In `jump_to_end`, after the `line_count == 0` return, add:

```rust
    if let Some(table) = crate::input::page_table::active_page_table(state) {
        let s = *table.last().expect("validated tables are non-empty");
        state.page_back_stack.clear();
        set_page_instant(state, s.left_start);
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        state.current_line = prev_dialogue_line(&state.buffer, &state.translation_lines,
                s.end + 1, state.is_prose(), &stage_lookup)
            .filter(|&d| d >= s.left_start)
            .unwrap_or(s.end);
        after_page_change(state, PageChangeReason::JumpToLine);
        log_fmt!("PAGES: page {}/{} (G)", table.len(), table.len());
        return;
    }
```

In `canonical_page_top_for`, after the `line_count == 0` return, add:

```rust
    if let Some(table) = crate::input::page_table::active_page_table(state) {
        if let Some(i) = crate::input::page_table::page_for_line(&table, target.min(line_count - 1)) {
            return table[i].left_start;
        }
    }
```

- [ ] **Step 4: Render/clip wiring** (`src/input/scroll.rs`)

In `snap_scroll_to_line_offset`, the two-column block computes
`let cs = ... column_split(state, effective_top)`. Replace that computation
with a table-aware version:

```rust
    let two_col = state.column_count() == 2;
    let table = crate::input::page_table::active_page_table(state);
    let table_spread = table.as_ref().and_then(|t|
        crate::input::page_table::spread_for_top(t, effective_top).copied());
    let cs = if two_col {
        if let Some(s) = table_spread {
            // Synthesize the ColumnSplit the downstream code expects from the
            // stored spread — column_split() is NOT consulted in table mode.
            Some(super::viewport::ColumnSplit {
                split: s.split.unwrap_or(s.end + 1),
                page_end: s.end,
                next_page_top: s.end + 1,
            })
        } else {
            Some(super::viewport::column_split(state, effective_top))
        }
    } else {
        None
    };
```

NOTE: check `ColumnSplit`'s real field set in `viewport.rs` before writing
this — if it has more fields than `{split, page_end, next_page_top}`,
populate them from the spread equivalently or add a small
`ColumnSplit::from_spread(s: &Spread)` constructor in viewport.rs. The
empty-right representation must match what `update_next_scene_watermark`
expects (`split` beyond `page_end` = empty right; verify against its
`page_end < split` test). Everything downstream (left `exact_end =
cs.split`, right `exact_end = cs.page_end + 1`, the watermark, the clip
scheduling with `cursor_line`) is unchanged — the table only supplies the
boundary values.

- [ ] **Step 5: Build + full unit suite**

Run: `cargo build 2>&1 | tail -1 && cargo test --bins 2>&1 | tail -3`
Expected: build Finished; the SAME 2 pre-existing failures only.

- [ ] **Step 6: Headless smoke of the table path**

```bash
cd ~/utono/linux-lit
command rm -rf /tmp/xdg-pages; mkdir -p /tmp/xdg-pages && chmod 700 /tmp/xdg-pages
XDG_RUNTIME_DIR=/tmp/xdg-pages LIT_HEADLESS_TEST=1 LIT_GEN_PAGE_TABLE=1 \
  LIT_START_WORK=MND LIT_START_POS=3450 GSK_RENDERER=cairo \
  ./scripts/e2e-env.sh cage -- ./target/debug/linux-lit >/dev/null 2>/tmp/cage-pages.log &
sleep 8
export WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/tmp/xdg-pages
# Pin the headless output to PRODUCTION geometry (cage's wlroots headless
# default is 1280x720; cage supports wlr-output-management — verified
# 2026-07-04). This makes the generated table's fingerprint match (or nearly
# match) the user's real layout, so this test exercises the production path.
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 5
grim /tmp/pages-1.png
wtype "y"; sleep 2; wtype "x"; sleep 2; grim /tmp/pages-2.png
pkill -f "cage -- ./target/debug/linux-lit" || true
python3 -c "
from PIL import Image; import numpy as np
a=np.array(Image.open('/tmp/pages-1.png')); b=np.array(Image.open('/tmp/pages-2.png'))
print('roundtrip identical' if (a==b).all() else 'ROUNDTRIP DIFFERS')"
```

Expected: `roundtrip identical` — and READ both PNGs: the final spread ends
at "And Robin shall restore amends." with the cursor highlighted on it and no
clipping. Do NOT trust the shared release log if the user's app is running;
if no live instance exists, also confirm `rg "PAGES: generated|PAGES: table
hit|PAGES: page" linux-lit-release.log` shows generation on first run.
Second run of the same command must show `PAGES: table hit` (loaded, not
regenerated). NOTE: this writes a `1280x720`-fingerprint table into the real
lit.db — harmless (distinct key) but clean it after verification:

```bash
sqlite3 ~/utono/litdb/data/lit.db "DELETE FROM play_pages WHERE layout_fingerprint LIKE '%|1280x720|%'; DELETE FROM play_pages_meta WHERE layout_fingerprint LIKE '%|1280x720|%';"
```

- [ ] **Step 7: Commit (selective staging for app/mod.rs)**

```bash
cd ~/utono/linux-lit
git diff src/app/mod.rs | python3 -c "
import sys
lines = sys.stdin.read().split('\n')
header = lines[:4]
hunks, cur = [], None
for l in lines[4:]:
    if l.startswith('@@'):
        if cur: hunks.append(cur)
        cur = [l]
    elif cur is not None:
        cur.append(l)
if cur: hunks.append(cur)
mine = [h for h in hunks if 'page_table' in '\n'.join(h)]
print('\n'.join(header + [l for h in mine for l in h]))
" > /tmp/pages-appmod2.patch
git apply --cached /tmp/pages-appmod2.patch
git add src/input/page_table.rs src/input/navigation.rs src/input/scroll.rs
git commit -m "feat(pagination): table-driven navigation + render wiring with live-engine fallback"
```

---

### Task 6: Fuzz assertion + regression runs

**Files:**
- Modify: `src/input/nav_test.rs` (add the PAGES monotonicity assertion)
- Test: nav-fuzz runs (logs at `/tmp/fuzz-nav.log`)

**Interfaces:**
- Consumes: the `PAGES: page N/M` log lines from Task 5.

- [ ] **Step 1: Add the assertion**

In `src/input/nav_test.rs`, find where PageForward/PageBackward outcomes are
checked (search `landing off-page`). Add a tracked `last_page_no:
Option<(usize, usize)>` alongside the existing per-step state; after each
PageForward/PageBackward action in table mode, read the current page via
`crate::input::page_table::active_page_table` + `page_for_line(...,
state.page_top_line)` and assert it moved by exactly ±1 (or pinned at the
first/last page). Report failures in the existing FAIL format with category
`Pages: non-monotone (was N now M)`. Skip the check entirely when
`active_page_table` is `None` (live-engine runs must not change behavior).

- [ ] **Step 2: Fuzz — table path ON**

```bash
cd ~/utono/linux-lit
LIT_GEN_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
LIT_GEN_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Ham
```

Expected: 0 failures each. (Check whether `run-fuzz.sh` passes env through;
if it scrubs the environment, add `LIT_GEN_PAGE_TABLE` to the vars it
forwards — read the script first.)

OPTIONAL but recommended while in `run-fuzz.sh`: add a
`wlr-randr --output HEADLESS-1 --custom-mode 1920x1200` step after its cage
launch (plus a settle sleep) so fuzz runs exercise PRODUCTION geometry — the
generated test table then shares the production fingerprint shape, and the
same run doubles as a check of the user's real table dimensions. Clean up
generated rows afterward as in Step 4.

- [ ] **Step 3: Fuzz — fallback path (must stay identical to today)**

```bash
LIT_NO_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
```

Expected: 0 failures.

- [ ] **Step 4: Clean the test tables** (same sqlite3 DELETE as Task 5 Step 6)

- [ ] **Step 5: Commit**

```bash
git add src/input/nav_test.rs
git commit -m "test(nav-fuzz): assert page-number monotonicity in table mode"
```

---

### Task 7: validate-play-pages skill

**Files:**
- Create: `.claude/skills/validate-play-pages/SKILL.md`
- Create: `.claude/skills/validate-play-pages/validate-play-pages.sh` (chmod +x)

**Interfaces:**
- Consumes: lit.db `play_pages` / `play_pages_meta` / `line_mapping`.
- Produces: read-only PASS/STALE/FAIL report per work.

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Read-only audit of lit.db play_pages tables (structural invariants only —
# fit/determinism need live geometry and are enforced at generation time).
set -euo pipefail
DB="$HOME/utono/litdb/data/lit.db"
ABBR="${1:---all}"

works() {
  if [[ "$ABBR" == "--all" ]]; then
    sqlite3 "$DB" "SELECT DISTINCT work_abbrev FROM play_pages_meta ORDER BY 1;"
  else
    echo "$ABBR"
  fi
}

fail=0
for w in $(works); do
  for fp in $(sqlite3 "$DB" "SELECT layout_fingerprint FROM play_pages_meta WHERE work_abbrev='$w';"); do
    meta=$(sqlite3 -separator ' | ' "$DB" \
      "SELECT page_count, generated_at, validated FROM play_pages_meta
       WHERE work_abbrev='$w' AND layout_fingerprint='$fp';")
    rowcount=$(sqlite3 "$DB" \
      "SELECT count(*) FROM play_pages WHERE work_abbrev='$w' AND layout_fingerprint='$fp';")
    # sanity: split/end ordering per row, join sanity against line_mapping
    bad_rows=$(sqlite3 "$DB" "
      SELECT count(*) FROM play_pages p
      LEFT JOIN line_mapping ls ON ls.id = p.left_start_id
      LEFT JOIN line_mapping le ON le.id = p.end_id
      WHERE p.work_abbrev='$w' AND p.layout_fingerprint='$fp'
        AND (ls.id IS NULL OR le.id IS NULL
             OR ls.work_abbrev NOT IN (SELECT work_abbrev FROM line_mapping WHERE id=p.left_start_id)
             OR p.end_id < p.left_start_id);")
    # coverage: page N+1's left_start_id must be > page N's end_id (ids are
    # document-ordered within a work), with no page_no gaps
    holes=$(sqlite3 "$DB" "
      WITH o AS (SELECT page_no, left_start_id, end_id FROM play_pages
                 WHERE work_abbrev='$w' AND layout_fingerprint='$fp' ORDER BY page_no)
      SELECT count(*) FROM o a JOIN o b ON b.page_no = a.page_no + 1
      WHERE b.left_start_id <= a.end_id;")
    gaps=$(sqlite3 "$DB" "
      SELECT count(*) FROM (
        SELECT page_no - ROW_NUMBER() OVER (ORDER BY page_no) AS d
        FROM play_pages WHERE work_abbrev='$w' AND layout_fingerprint='$fp'
      ) GROUP BY d HAVING count(*) >= 0 LIMIT 100;" | wc -l)
    status=PASS
    [[ "$bad_rows" != "0" || "$holes" != "0" || "$gaps" -gt 1 ]] && { status=FAIL; fail=1; }
    [[ "$rowcount" != "$(echo "$meta" | cut -d'|' -f1 | tr -d ' ')" ]] && { status=FAIL; fail=1; }
    echo "$w [$status] fp=$fp rows=$rowcount meta=($meta) bad_rows=$bad_rows overlaps=$holes"
  done
done
echo
echo "NOTE: db_fingerprint staleness and fit/determinism are enforced by the"
echo "app at load/generation time (a stale table logs 'PAGES: fallback' and is"
echo "regenerated on next load). To force regeneration:"
echo "  sqlite3 \"$DB\" \"DELETE FROM play_pages WHERE work_abbrev='<ABBR>'; DELETE FROM play_pages_meta WHERE work_abbrev='<ABBR>';\""
exit $fail
```

- [ ] **Step 2: Write SKILL.md**

```markdown
---
name: validate-play-pages
description: Use when auditing lit.db play_pages tables after a litdb re-import, font or layout change, or suspected play pagination drift — checks structural invariants (coverage, ordering, row/meta consistency) read-only and reports PASS/STALE/FAIL per work
argument-hint: <ABBR> | --all
---

Run the backing script (read-only; never writes lit.db):

    .claude/skills/validate-play-pages/validate-play-pages.sh --all
    .claude/skills/validate-play-pages/validate-play-pages.sh MND

Interpretation:
- **PASS** — rows are structurally sound for that (work, layout fingerprint).
- **FAIL** — overlapping/missing/malformed rows: delete that work's rows (the
  script prints the command) and let the app regenerate on next load.
- Staleness vs. the current text (db_fingerprint) and the geometric fit/
  determinism invariants are enforced by the app itself at load/generation —
  a stale table logs `PAGES: fallback (...)` in the app log and is replaced
  on the next load of that play at the pinned layout.

The fingerprint string is human-readable
(`v1|family|size|ascent|descent|charw|WxH|spacing|margins|cols`), so when a
table is unexpectedly missing you can see which layout input moved.
```

- [ ] **Step 3: Test the script against the tables generated in Task 6**
  (before the cleanup DELETE, or regenerate one headlessly):

Run: `.claude/skills/validate-play-pages/validate-play-pages.sh --all`
Expected: one PASS line per (work, fingerprint), exit 0. Corrupt one row
(`sqlite3 ... "UPDATE play_pages SET end_id = left_start_id - 1 WHERE page_no=2 AND work_abbrev='MND'"`),
re-run, expect FAIL + exit 1; restore by deleting the work's rows.

- [ ] **Step 4: Commit**

```bash
chmod +x .claude/skills/validate-play-pages/validate-play-pages.sh
git add .claude/skills/validate-play-pages
git commit -m "feat(skills): validate-play-pages — read-only structural audit of play_pages"
```

---

### Task 8: Docs

**Files:**
- Modify: `docs/troubleshooting/page-turning-mechanics.md` (new top-level
  section after the architecture overview)
- Modify: `CLAUDE.md` (three sentences under "Pagination & Scene Boundaries")

- [ ] **Step 1: page-turning-mechanics.md** — add:

```markdown
## Pinned page tables (plays)

Two-column plays at the pinned layout (the user's Charter/1920x1200 reading
setup) do NOT run the forward-walk heuristics at all: pages come from lit.db
`play_pages` (keyed by `line_mapping` ids + a layout fingerprint), generated
once in-app by recording the live engine's walk and gating it behind the
invariant suite in `src/input/page_table.rs` (coverage, tail, fit,
watermark-sanity, determinism). `x`/`y`/`G`/`gg`/lookups are index arithmetic
(`PAGES:` log lines). EVERYTHING in this document still applies to the
fallback modes — fingerprint mismatch (font/resolution change, re-import),
interlinear translations, scroll mode, 1-col — which use the live engine
unchanged, and to the generator itself (the table is only as good as the
walk it records; a walk bug becomes a VALIDATE_FAIL or a bad stored table).
Audit stored tables with the validate-play-pages skill.
```

- [ ] **Step 2: CLAUDE.md** — append to the "Pagination & Scene Boundaries"
  section:

```markdown
**Pinned play pagination:** two-column plays at the pinned layout read their
spreads from lit.db `play_pages` (generated in-app, invariant-gated — see
`src/input/page_table.rs` and `docs/plans/2026-07-04-pinned-play-pagination-design.md`).
`PAGES: table hit/fallback/generated` log lines say which engine is active.
Test flags: `LIT_NO_PAGE_TABLE=1` forces the live engine;
`LIT_GEN_PAGE_TABLE=1` forces generation at the current (e.g. headless)
geometry. Audit with the `validate-play-pages` skill.
```

- [ ] **Step 3: Commit**

```bash
git add docs/troubleshooting/page-turning-mechanics.md CLAUDE.md
git commit -m "docs: pinned play page tables (generation, fallbacks, audit skill)"
```

---

### Task 9: Finish the branch

- [ ] **Step 1: Full verification on the branch**

```bash
cargo build 2>&1 | tail -1
cargo test --bins 2>&1 | tail -3          # only the 2 pre-existing failures
./scripts/e2e-env.sh cargo test --test line_clipping --test overlay_clipping -- --ignored --nocapture 2>&1 | rg "test result"
```

- [ ] **Step 2: Merge per house convention**

```bash
git checkout master
git merge --no-ff feat/pinned-play-pagination
cargo build 2>&1 | tail -1 && cargo test --bins 2>&1 | tail -3
git push origin master
git branch -d feat/pinned-play-pagination
```

- [ ] **Step 3: Hand the user the on-display generation step**

The production table generates on the USER's machine the first time each play
loads at their real layout. Tell the user: restart `crll`, open a play, wait
~1s after the page renders (the 400ms settled-layout hook), then check
`rg "PAGES:" linux-lit-dev.log` for `generated N pages`. Subsequent launches
log `table hit`. Then run:

```bash
.claude/skills/validate-play-pages/validate-play-pages.sh --all
```

---

## Self-Review

**1. Spec coverage:** schema+citation keys (T1), invariants 1-4 (T2),
fingerprint (T3), generation+determinism+lazy hook (T4), runtime
consumption/gating/nav/clip + `PAGES:` logs (T5), fuzz monotonicity + both
test flags (T6), skill (T7), docs (T8), merge convention (T9). Design's
"regeneration offered" = lazy regeneration on next load after the skill's
DELETE (documented in T7/T8). ✓

**2. Placeholder scan:** all steps carry code/commands; the three
check-the-real-name NOTEs (config field names, `ColumnSplit` fields,
`run-fuzz.sh` env forwarding) are verification instructions against named
files, not TBDs. ✓

**3. Type consistency:** `Spread{left_start, split: Option<usize>, end}` used
identically in T2/T4/T5; `PageRow`/`PagesMeta` identical in T1/T4/T5;
`page_for_line`/`active_page_table`/`spread_for_top` signatures match across
T5/T6. `db_fingerprint: u64` stored as TEXT (noted in T1). ✓
