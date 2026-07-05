# Prose Visual-Row Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prose pages fill with wrapped visual rows (paragraphs split across
pages), pinned to a validated `prose_pages` table in lit.db, with
phrase-timestamp-accurate sync page turns.

**Architecture:** A page boundary for prose becomes `(buffer_line,
row_offset_px)` snapped to a real visual-row top. A GTK walk generalizes the
existing over-tall step into the universal forward rule; a pure, GTK-free
module holds the page types and a zero-gap invariant suite; storage mirrors
`src/db/play_pages.rs`; sync turns mid-paragraph fire on `phrase_timestamps`
crossing times (char-fraction interpolation fallback).

**Tech Stack:** Rust + GTK4/sourceview5 (linux-lit), rusqlite, SQLite
(lit.db at `~/utono/litdb/data/lit.db`), Python (litdb scripts), hot repo
schema (SQL + Python mirror).

**Spec:** `docs/plans/2026-07-05-prose-visual-row-pagination-design.md`

## Global Constraints

- Build with `cargo build`; NEVER `cargo run` (the user runs the app).
- Prose-only gating: every new path requires `state.is_prose() &&
  state.column_count() == 1`. Two-column plays and `column_split` are
  untouched.
- `page_top_offset` is the sub-line contract: viewport top =
  `line_yrange(page_top_line).y + page_top_offset`. Back-stack entries are
  `(page_top_line, page_top_offset)`.
- Every stored/used offset must be snapped via
  `scroll::snap_value_to_display_row` — never mid-glyph-row — and forward
  steps must strictly advance or fall back.
- Never re-infer structure from buffer text; `lit.db` metadata is
  authoritative.
- `LIT_NO_PAGE_TABLE=1` must disable the prose table gate (same env var as
  plays); `LIT_GEN_PAGE_TABLE=1` forces regeneration.
- lit.db schema changes land in linux-lit's `ensure_schema` first; the hot
  repo mirror is Task 10, last.
- Log tags: prefix all new page-table logging with `PAGES_PROSE:` so it is
  greppable next to the play `PAGES:` lines.
- Commit after each task; end commit messages with the standard co-author
  trailer used in this repo.

---

### Task 1: Pure prose page types + invariant suite

**Files:**
- Create: `src/input/prose_pages.rs`
- Modify: `src/input/mod.rs` (add `pub mod prose_pages;` next to
  `pub mod page_table;`)

**Interfaces:**
- Produces: `ProsePage { start_line: usize, start_off: i32, end_line: usize,
  end_off: i32 }` (end is EXCLUSIVE: page i's `(end_line, end_off)` ==
  page i+1's `(start_line, start_off)`; the last page's end is
  `(line_count-1, heights[line_count-1])`).
- Produces: `validate_prose_pages(pages: &[ProsePage], ctx: &ProseValidateCtx)
  -> Result<(), String>` and
  `prose_page_for_position(pages: &[ProsePage], line: usize, off: i32)
  -> Option<usize>`.
- Consumed by: Tasks 3, 5, 6, 9.

- [ ] **Step 1: Write the module with failing-first tests**

Create `src/input/prose_pages.rs`:

```rust
//! Pure prose page-table types + invariant suite (GTK-free, unit-testable).
//! A prose page boundary is (buffer_line, row_offset_px); offsets are pixel
//! offsets from the buffer line's top, snapped to visual-row tops by the
//! GTK-bound generator (snapping itself is not re-checkable here).
//! `end` is EXCLUSIVE and must equal the next page's `start` exactly —
//! zero gaps, zero overlaps: the machine-checked no-text-loss guarantee.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProsePage {
    pub start_line: usize,
    pub start_off: i32,
    pub end_line: usize,
    pub end_off: i32,
}

pub struct ProseValidateCtx<'a> {
    pub line_count: usize,
    /// Per-buffer-line pixel heights (line_yrange), at generation layout.
    pub heights: &'a [i32],
    /// widget_height - descender_guard - BASE_BOTTOM_MARGIN at that layout.
    pub usable_height: i32,
}

/// Lexicographic order on (line, off).
fn pos_le(al: usize, ao: i32, bl: usize, bo: i32) -> bool {
    (al, ao) <= (bl, bo)
}

/// Pixel height of the half-open interval [start, end) given per-line heights.
fn page_px(p: &ProsePage, heights: &[i32]) -> i64 {
    let mut px: i64 = 0;
    for l in p.start_line..=p.end_line.min(heights.len().saturating_sub(1)) {
        px += heights[l] as i64;
    }
    px - p.start_off as i64 - (heights[p.end_line.min(heights.len() - 1)] - p.end_off) as i64
}

/// Invariant suite (design doc §2). Returns the FIRST violation as
/// "<name>: <details>".
pub fn validate_prose_pages(
    pages: &[ProsePage],
    ctx: &ProseValidateCtx,
) -> Result<(), String> {
    if pages.is_empty() {
        return Err("coverage: no pages".into());
    }
    if ctx.line_count == 0 || ctx.heights.len() < ctx.line_count {
        return Err("sanity: bad ctx".into());
    }
    let first = &pages[0];
    if first.start_line != 0 || first.start_off != 0 {
        return Err(format!(
            "coverage: first page starts at ({}, {}) not (0, 0)",
            first.start_line, first.start_off
        ));
    }
    for (i, p) in pages.iter().enumerate() {
        // sanity: offsets inside their lines, positions ordered.
        if p.start_line >= ctx.line_count || p.end_line >= ctx.line_count {
            return Err(format!("sanity: page {} line out of range", i + 1));
        }
        if p.start_off < 0 || p.start_off >= ctx.heights[p.start_line].max(1) {
            return Err(format!(
                "sanity: page {} start_off {} outside line {} height {}",
                i + 1, p.start_off, p.start_line, ctx.heights[p.start_line]
            ));
        }
        if p.end_off <= 0 && !(p.end_off == 0 && p.end_line > p.start_line) {
            return Err(format!("sanity: page {} end_off {}", i + 1, p.end_off));
        }
        if p.end_off > ctx.heights[p.end_line] {
            return Err(format!(
                "sanity: page {} end_off {} > line {} height {}",
                i + 1, p.end_off, p.end_line, ctx.heights[p.end_line]
            ));
        }
        if !pos_le(p.start_line, p.start_off + 1, p.end_line, p.end_off) {
            return Err(format!("ordering: page {} end not after start", i + 1));
        }
        // adjacency: exclusive end == next start. THE no-text-loss rule.
        if let Some(n) = pages.get(i + 1) {
            let matches_next = (p.end_line == n.start_line && p.end_off == n.start_off)
                // A boundary exactly at a line's full height is the same
                // position as the next line's top (normalized form).
                || (p.end_off == ctx.heights[p.end_line]
                    && n.start_line == p.end_line + 1
                    && n.start_off == 0);
            if !matches_next {
                return Err(format!(
                    "coverage: page {} ends at ({}, {}) but page {} starts at ({}, {})",
                    i + 1, p.end_line, p.end_off, i + 2, n.start_line, n.start_off
                ));
            }
        }
        // fit: the page's pixel height must fit the viewport.
        let px = page_px(p, ctx.heights);
        if px > ctx.usable_height as i64 {
            return Err(format!(
                "fit: page {} spans {}px > usable {}",
                i + 1, px, ctx.usable_height
            ));
        }
        if px <= 0 {
            return Err(format!("fit: page {} spans {}px (empty/negative)", i + 1, px));
        }
    }
    // tail: last page must reach the document's pixel end.
    let last = pages.last().unwrap();
    let last_line = ctx.line_count - 1;
    if !(last.end_line == last_line && last.end_off == ctx.heights[last_line]) {
        return Err(format!(
            "tail: last page ends at ({}, {}) not ({}, {})",
            last.end_line, last.end_off, last_line, ctx.heights[last_line]
        ));
    }
    Ok(())
}

/// Page containing position (line, off). A position exactly at a page's start
/// resolves to THAT page (page tops are canonical — same convention as
/// play `page_for_line`). Adjacency is exact, so there is no overlap case.
pub fn prose_page_for_position(
    pages: &[ProsePage],
    line: usize,
    off: i32,
) -> Option<usize> {
    let idx = pages.partition_point(|p| pos_le(p.start_line, p.start_off, line, off));
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    let p = &pages[i];
    // Inside [start, end)?
    let before_end = (line, off) < (p.end_line, p.end_off)
        || (p.end_off == 0 && line < p.end_line); // normalized-end form
    before_end.then_some(i)
}

/// Page whose interval contains buffer line `line`'s FIRST row (off = 0).
/// The design's "a line maps to the page containing its first row" rule.
pub fn prose_page_for_line(pages: &[ProsePage], line: usize) -> Option<usize> {
    prose_page_for_position(pages, line, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 4 paragraphs, heights 100/250/40/60, usable 120. Paragraph 1 (250px)
    // straddles three boundaries. Pages tile the pixel space exactly.
    fn heights() -> Vec<i32> { vec![100, 250, 40, 60] }

    fn ok_pages() -> Vec<ProsePage> {
        vec![
            ProsePage { start_line: 0, start_off: 0,   end_line: 1, end_off: 20 },
            ProsePage { start_line: 1, start_off: 20,  end_line: 1, end_off: 140 },
            ProsePage { start_line: 1, start_off: 140, end_line: 2, end_off: 10 },
            ProsePage { start_line: 2, start_off: 10,  end_line: 3, end_off: 60 },
        ]
    }

    fn ctx(h: &[i32]) -> ProseValidateCtx<'_> {
        ProseValidateCtx { line_count: h.len(), heights: h, usable_height: 120 }
    }

    #[test]
    fn valid_pages_pass() {
        let h = heights();
        assert_eq!(validate_prose_pages(&ok_pages(), &ctx(&h)), Ok(()));
    }

    #[test]
    fn gap_between_pages_fails_coverage() {
        let h = heights();
        let mut p = ok_pages();
        p[2].start_off = 150; // 10px of paragraph 1's rows on no page
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn overlap_fails_coverage() {
        let h = heights();
        let mut p = ok_pages();
        p[2].start_off = 130; // re-shows 10px already on page 2
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn overfull_page_fails_fit() {
        let h = heights();
        let mut p = ok_pages();
        p[0].end_off = 40; // page 1 = 100 + 40 = 140px > 120
        // keep adjacency so ONLY fit fails
        p[1].start_off = 40;
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("fit"), "got: {err}");
    }

    #[test]
    fn short_tail_fails() {
        let h = heights();
        let p = &ok_pages()[..3];
        let err = validate_prose_pages(p, &ctx(&h)).unwrap_err();
        assert!(err.contains("tail"), "got: {err}");
    }

    #[test]
    fn first_page_must_start_at_origin() {
        let h = heights();
        let mut p = ok_pages();
        p[0].start_off = 5;
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn position_lookup_resolves_pages_and_tops() {
        let p = ok_pages();
        assert_eq!(prose_page_for_position(&p, 0, 0), Some(0));
        assert_eq!(prose_page_for_position(&p, 1, 19), Some(0));
        assert_eq!(prose_page_for_position(&p, 1, 20), Some(1), "page top is canonical");
        assert_eq!(prose_page_for_position(&p, 1, 200), Some(2));
        assert_eq!(prose_page_for_position(&p, 3, 59), Some(3));
        assert_eq!(prose_page_for_position(&p, 3, 60), None, "past document end");
        // line -> page containing its FIRST row
        assert_eq!(prose_page_for_line(&p, 1), Some(0));
        assert_eq!(prose_page_for_line(&p, 2), Some(2));
    }

    #[test]
    fn normalized_full_height_end_matches_next_line_top() {
        // Page ends at exactly line 0's full height; next starts at (1, 0).
        let h = vec![100, 100];
        let p = vec![
            ProsePage { start_line: 0, start_off: 0, end_line: 0, end_off: 100 },
            ProsePage { start_line: 1, start_off: 0, end_line: 1, end_off: 100 },
        ];
        let c = ProseValidateCtx { line_count: 2, heights: &h, usable_height: 120 };
        assert_eq!(validate_prose_pages(&p, &c), Ok(()));
    }
}
```

- [ ] **Step 2: Register the module and run the tests**

In `src/input/mod.rs`, add `pub mod prose_pages;` beside `pub mod page_table;`.

Run: `cargo test --bins prose_pages -- --nocapture` (falls back to
`cargo test prose_pages` if the input mods are lib-visible — match how
`page_table::tests` runs in this repo).
Expected: all 8 tests PASS. Fix compile errors only; do not weaken assertions.

- [ ] **Step 3: Commit**

```bash
git add src/input/prose_pages.rs src/input/mod.rs
git commit -m "feat(prose-pages): pure page types + zero-gap invariant suite"
```

---

### Task 2: lit.db storage module `db::prose_pages`

**Files:**
- Create: `src/db/prose_pages.rs` (mirror of `src/db/play_pages.rs`)
- Modify: `src/db/mod.rs` (add `pub mod prose_pages;`)

**Interfaces:**
- Produces: `ProsePageRow { page_no: i64, start_line_id: i64, start_off: i64,
  end_line_id: i64, end_off: i64 }`, `PagesMeta` (re-use shape of
  `play_pages::PagesMeta` — define locally, do not import, so the modules
  stay independent), `ensure_schema(conn)`, `load_pages(conn, abbrev, fp)
  -> rusqlite::Result<Option<(PagesMeta, Vec<ProsePageRow>)>>`,
  `store_pages(conn, abbrev, meta, rows)`.
- Consumed by: Task 5.

- [ ] **Step 1: Write the module + roundtrip tests**

Create `src/db/prose_pages.rs`. Copy the structure of
`src/db/play_pages.rs` verbatim and adapt:

```rust
//! Persisted visual-row prose pages, keyed by citation (`line_mapping` ids)
//! + pixel row offsets, and the layout fingerprint they were generated at.
//! See docs/plans/2026-07-05-prose-visual-row-pagination-design.md.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct ProsePageRow {
    pub page_no: i64,
    pub start_line_id: i64,
    /// Pixel offset from start line's top; a snapped visual-row top.
    pub start_off: i64,
    pub end_line_id: i64,
    /// Exclusive pixel bottom edge within the end line.
    pub end_off: i64,
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
        "CREATE TABLE IF NOT EXISTS prose_pages (
             work_abbrev        TEXT NOT NULL,
             layout_fingerprint TEXT NOT NULL,
             page_no            INTEGER NOT NULL,
             start_line_id      INTEGER NOT NULL,
             start_row_offset   INTEGER NOT NULL,
             end_line_id        INTEGER NOT NULL,
             end_row_offset     INTEGER NOT NULL,
             PRIMARY KEY (work_abbrev, layout_fingerprint, page_no)
         );
         CREATE TABLE IF NOT EXISTS prose_pages_meta (
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
    abbrev: &str,
    layout_fingerprint: &str,
) -> rusqlite::Result<Option<(PagesMeta, Vec<ProsePageRow>)>> {
    let meta: Option<PagesMeta> = conn
        .query_row(
            "SELECT db_fingerprint, page_count, generated_at, validated
             FROM prose_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
            params![abbrev, layout_fingerprint],
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
        "SELECT page_no, start_line_id, start_row_offset, end_line_id, end_row_offset
         FROM prose_pages
         WHERE work_abbrev = ?1 AND layout_fingerprint = ?2 ORDER BY page_no",
    )?;
    let rows = stmt
        .query_map(params![abbrev, layout_fingerprint], |row| {
            Ok(ProsePageRow {
                page_no: row.get(0)?,
                start_line_id: row.get(1)?,
                start_off: row.get(2)?,
                end_line_id: row.get(3)?,
                end_off: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() as i64 != meta.page_count {
        return Ok(None);
    }
    Ok(Some((meta, rows)))
}

pub fn store_pages(
    conn: &mut Connection,
    abbrev: &str,
    meta: &PagesMeta,
    rows: &[ProsePageRow],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM prose_pages WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![abbrev, meta.layout_fingerprint],
    )?;
    tx.execute(
        "DELETE FROM prose_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![abbrev, meta.layout_fingerprint],
    )?;
    for r in rows {
        tx.execute(
            "INSERT INTO prose_pages
             (work_abbrev, layout_fingerprint, page_no,
              start_line_id, start_row_offset, end_line_id, end_row_offset)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![abbrev, meta.layout_fingerprint, r.page_no,
                    r.start_line_id, r.start_off, r.end_line_id, r.end_off],
        )?;
    }
    tx.execute(
        "INSERT INTO prose_pages_meta
         (work_abbrev, layout_fingerprint, db_fingerprint, page_count, generated_at, validated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![abbrev, meta.layout_fingerprint,
                meta.db_fingerprint.to_string(), rows.len() as i64,
                meta.generated_at, meta.validated as i64],
    )?;
    tx.commit()
}

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
            generated_at: "epoch:1751700000".into(),
            validated: true,
        }
    }

    fn sample_rows() -> Vec<ProsePageRow> {
        vec![
            ProsePageRow { page_no: 1, start_line_id: 100, start_off: 0,
                           end_line_id: 101, end_off: 240 },
            ProsePageRow { page_no: 2, start_line_id: 101, start_off: 240,
                           end_line_id: 105, end_off: 60 },
        ]
    }

    #[test]
    fn roundtrips_pages_and_meta() {
        let mut conn = mem();
        store_pages(&mut conn, "BH", &sample_meta(), &sample_rows()).unwrap();
        let (meta, rows) = load_pages(&conn, "BH", "v1|abc").unwrap().unwrap();
        assert_eq!(meta.db_fingerprint, 42);
        assert_eq!(rows, sample_rows());
    }

    #[test]
    fn load_misses_on_wrong_fingerprint_or_abbrev() {
        let mut conn = mem();
        store_pages(&mut conn, "BH", &sample_meta(), &sample_rows()).unwrap();
        assert!(load_pages(&conn, "BH", "v1|OTHER").unwrap().is_none());
        assert!(load_pages(&conn, "DC", "v1|abc").unwrap().is_none());
    }

    #[test]
    fn unvalidated_meta_loads_as_none() {
        let mut conn = mem();
        let mut meta = sample_meta();
        meta.validated = false;
        store_pages(&mut conn, "BH", &meta, &sample_rows()).unwrap();
        assert!(load_pages(&conn, "BH", "v1|abc").unwrap().is_none());
    }

    #[test]
    fn row_count_mismatch_loads_as_none() {
        let mut conn = mem();
        let mut meta = sample_meta();
        meta.page_count = 3; // lies about count
        // store_pages writes rows.len() as page_count, so tamper directly:
        store_pages(&mut conn, "BH", &sample_meta(), &sample_rows()).unwrap();
        conn.execute("UPDATE prose_pages_meta SET page_count = 3", []).unwrap();
        assert!(load_pages(&conn, "BH", "v1|abc").unwrap().is_none());
    }
}
```

- [ ] **Step 2: Register + test**

Add `pub mod prose_pages;` to `src/db/mod.rs`.
Run: `cargo test db::prose_pages -- --nocapture`
Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/db/prose_pages.rs src/db/mod.rs
git commit -m "feat(prose-pages): lit.db prose_pages/prose_pages_meta storage"
```

---

### Task 3: Live engine — universal visual-row forward boundary

Replaces the over-tall special case with the general fill rule in the
`x`/PageForward path. (Cursor-follow and sync route in Tasks 4/6/9.)

**Files:**
- Modify: `src/input/navigation.rs` (replace `overtall_forward_step` usage in
  `page_forward`, currently `src/input/navigation.rs:886-896`; keep the
  function for `page_backward`'s restore path)
- Modify: `src/input/viewport.rs` (pure end-of-document decision)

**Interfaces:**
- Consumes: `scroll::snap_value_to_display_row(state, f64) -> f64`,
  `state.text_view.line_at_y(i32) -> (TextIter, i32)`,
  `text_view.line_yrange(&iter) -> (i32, i32)`,
  `scroll::set_page_instant_offset(state, top, off)`,
  `viewport::descender_guard_px`, `scroll::BASE_BOTTOM_MARGIN`.
- Produces: `prose_next_boundary(state: &mut AppState) -> Option<(usize, i32)>`
  in `navigation.rs` — the next page's `(top, offset)` strictly after the
  current viewport, or `None` at document end. Task 5's generator and Task 4's
  cursor-follow call this exact function.

- [ ] **Step 1: Pure decision + unit test in viewport.rs**

Add to `src/input/viewport.rs` (near `overtall_next_offset`):

```rust
/// Pure fill decision for prose visual-row paging. Given the viewport's
/// absolute top pixel `y0`, the document's total pixel height `total`, and
/// the `usable` viewport height: the RAW next boundary pixel, or None when
/// the current page already shows the document tail.
pub(crate) fn prose_raw_next_boundary(y0: i32, total: i32, usable: i32) -> Option<i32> {
    let usable = usable.max(1);
    if total - y0 > usable {
        Some(y0 + usable)
    } else {
        None
    }
}
```

Add to `viewport.rs`'s `#[cfg(test)] mod tests`:

```rust
#[test]
fn prose_raw_next_boundary_fills_then_stops() {
    // 1000px doc, 300px viewport: boundaries at 300, 600, 900, then None
    // (at y0=900 only 100px remain — the last page).
    assert_eq!(prose_raw_next_boundary(0, 1000, 300), Some(300));
    assert_eq!(prose_raw_next_boundary(600, 1000, 300), Some(900));
    assert_eq!(prose_raw_next_boundary(900, 1000, 300), None);
    assert_eq!(prose_raw_next_boundary(700, 1000, 300), None);
}
```

Run: `cargo test prose_raw_next_boundary` — Expected: PASS.

- [ ] **Step 2: GTK walk in navigation.rs**

Add below `overtall_forward_step` (`src/input/navigation.rs:756`):

```rust
/// Universal prose forward boundary: the next page's (top_line, offset),
/// snapped to a real visual-row top, strictly after the current viewport.
/// Generalizes `overtall_forward_step` from "within one over-tall paragraph"
/// to "anywhere in the document" — pages fill with visual rows and split
/// paragraphs at the boundary. `None` = current page shows the document tail.
pub(crate) fn prose_next_boundary(state: &mut AppState) -> Option<(usize, i32)> {
    use gtk4::prelude::{TextBufferExt, TextViewExt, WidgetExt};
    let top = state.page_top_line;
    let iter = state.buffer.iter_at_line(top as i32)?;
    let (top_y, _h) = state.text_view.line_yrange(&iter);
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return None;
    }
    let guard = crate::input::viewport::descender_guard_px(&state.text_view, top);
    let usable = widget_height - guard - crate::input::scroll::BASE_BOTTOM_MARGIN;
    let line_count = state.effective_line_count();
    let last_iter = state.buffer.iter_at_line((line_count.saturating_sub(1)) as i32)?;
    let (ly, lh) = state.text_view.line_yrange(&last_iter);
    let total = ly + lh;
    let y0 = top_y + state.page_top_offset;
    let raw = crate::input::viewport::prose_raw_next_boundary(y0, total, usable)?;
    // Snap DOWN to a real visual-row top; never start a page mid-glyph-row.
    let snapped = crate::input::scroll::snap_value_to_display_row(state, raw as f64);
    if snapped <= y0 as f64 {
        return None; // degenerate snap: fall back to a whole-line turn
    }
    // Locate the buffer line containing the snapped pixel.
    let (bline_iter, _) = state.text_view.line_at_y(snapped as i32);
    let bline = bline_iter.line().max(0) as usize;
    let biter = state.buffer.iter_at_line(bline as i32)?;
    let (by, bh) = state.text_view.line_yrange(&biter);
    let mut new_top = bline;
    let mut new_off = (snapped - by as f64).round() as i32;
    // Normalize: a boundary at (or past) a line's full height is the next
    // line's top; a boundary inside a BLANK line starts at the next line.
    if new_off >= bh && bline + 1 < line_count {
        new_top = bline + 1;
        new_off = 0;
    } else if new_off > 0
        && crate::input::viewport::buffer_line_text(&state.buffer, bline)
            .trim()
            .is_empty()
        && bline + 1 < line_count
    {
        new_top = bline + 1;
        new_off = 0;
    }
    if (new_top, new_off) <= (top, state.page_top_offset) {
        return None;
    }
    Some((new_top, new_off))
}
```

- [ ] **Step 3: Route `page_forward`'s prose branch through it**

In `page_forward` (`src/input/navigation.rs`), replace the over-tall branch
(the `if state.column_count() == 1 { if let Some(off) = overtall_forward_step
(state) { ... } }` block at lines 886-896) with:

```rust
    // Prose visual-row fill (single column): the next page starts at the
    // snapped row boundary one viewport below the current one — paragraphs
    // split across pages, no underfill, no skipped tails. Subsumes the old
    // over-tall-paragraph special case. Non-prose single-column works keep
    // the whole-line path below.
    if state.column_count() == 1 && state.is_prose() {
        if let Some((nt, no)) = prose_next_boundary(state) {
            state.page_back_stack.push((state.page_top_line, state.page_top_offset));
            log_fmt!("PAGE_FWD: prose row-fill ({},{}) -> ({},{})",
                     state.page_top_line, state.page_top_offset, nt, no);
            let stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            // Cursor: the first content line whose text is on the new page.
            let landing = if no > 0 {
                nt // straddling paragraph is the current content
            } else {
                next_dialogue_from(&state.buffer, nt, state.effective_line_count(),
                                   state.is_prose(), &stage_lookup)
            };
            state.current_line = landing.min(state.effective_line_count().saturating_sub(1));
            crate::input::scroll::set_page_instant_offset(state, nt, no);
            after_page_change(state, PageChangeReason::Forward);
            return;
        }
        // No next boundary: we are on the final page. Move the cursor to the
        // last visible content line (mirror of the 2-col final-spread guard).
        let visible_end = super::viewport::last_fully_visible_line(state, state.page_top_line);
        if visible_end > state.current_line {
            state.current_line = visible_end;
            after_page_change(state, PageChangeReason::Forward);
        }
        log_fmt!("PAGE_FWD: prose final page (top={} off={})",
                 state.page_top_line, state.page_top_offset);
        return;
    }
    // Over-tall NON-prose single-column paragraph (BCP etc.): keep the old
    // within-paragraph step so those works do not regress.
    if state.column_count() == 1 {
        if let Some(off) = overtall_forward_step(state) {
            state.page_back_stack.push((state.page_top_line, state.page_top_offset));
            log_fmt!("PAGE_FWD: over-tall within-paragraph line={} offset {}->{}",
                     state.page_top_line, state.page_top_offset, off);
            let top = state.page_top_line;
            crate::input::scroll::set_page_instant_offset(state, top, off);
            after_page_change(state, PageChangeReason::Forward);
            return;
        }
    }
```

`page_backward` needs no change in this task: its back-stack entries are
`(top, offset)` pairs and `set_page_instant_offset` restore already exists
(`src/input/navigation.rs:1035-1048`).

- [ ] **Step 4: Build + unit suite**

Run: `cargo build && cargo test --bins`
Expected: clean build, all existing tests still PASS (especially
`viewport::tests` and `page_table::tests`).

- [ ] **Step 5: Headless spot-check (BH pages 54-55, the screenshot bug)**

```bash
cd ~/utono/linux-lit
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_DEV=1 LIT_HEADLESS_TEST=1 \
  LIT_LOG_PATH=/tmp/prose-fill.log LINUX_LIT_WORK=BH \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 4
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 2
# x repeatedly through the over-tall paragraph region; screenshot each page
for i in 1 2 3 4 5 6; do wtype "x"; sleep 1; grim /tmp/prose-$i.png; done
pkill -f "cage -- ./target/debug/linux-lit"
rg "PAGE_FWD: prose row-fill" /tmp/prose-fill.log | head
```

Read each PNG. Expected: consecutive pages tile — the last visual row of
page N is immediately above page N+1's first row; the Sladdery tail
("—in which Mr. Sladdery … entire truth.") appears; no page has a blank
bottom deeper than one row + the reserved margin. If `grim` yields a ~2-byte
PNG, sleep and retry (surface not mapped yet).

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs src/input/viewport.rs
git commit -m "feat(prose): visual-row page fill for x — paragraphs split at snapped row boundaries"
```

---

### Task 4: Cursor-follow (`j`/`q`) turns use the same boundary rule

This is the screenshot bug's exact path (`NAV_PAGE_FWD`, cursor step past an
over-tall paragraph).

**Files:**
- Modify: `src/input/scroll.rs` — `scroll_after_jump_forward`
  (`src/input/scroll.rs:1107-1183`)

**Interfaces:**
- Consumes: `navigation::prose_next_boundary` (Task 3),
  `viewport::is_line_fully_visible`.
- Produces: prose cursor-follow turns that advance one row-fill boundary at a
  time until the cursor's paragraph START row is on-page.

- [ ] **Step 1: Add the prose arm**

In `scroll_after_jump_forward`, immediately after the
`if super::viewport::is_line_fully_visible(...) { return; }` early-out
(line 1114-1116), insert:

```rust
            // Prose visual-row fill: advance one boundary at a time (the same
            // rule as x — Task 3's prose_next_boundary) until the cursor
            // line's FIRST row is on the page. This is what fixes the
            // j-past-an-over-tall-paragraph tail skip: the intermediate
            // sub-line pages are stepped through, never jumped over.
            if state.is_prose() && state.column_count() == 1 {
                let target = state.current_line;
                let mut guard = 0usize;
                let from = (state.page_top_line, state.page_top_offset);
                while !super::viewport::is_line_fully_visible(state, target) {
                    let Some((nt, no)) = super::navigation::prose_next_boundary(state)
                    else { break };
                    // Advance the live page state directly; one back entry
                    // for the whole jump (matches the single-entry rule below).
                    state.page_top_line = nt;
                    state.page_top_offset = no;
                    guard += 1;
                    if guard > state.effective_line_count().max(64) {
                        break; // safety: never loop forever
                    }
                }
                if (state.page_top_line, state.page_top_offset) != from {
                    let (nt, no) = (state.page_top_line, state.page_top_offset);
                    // Restore pre-jump state for the back stack, then land.
                    state.page_top_line = from.0;
                    state.page_top_offset = from.1;
                    state.page_back_stack.clear();
                    state.page_back_stack.push(from);
                    log_fmt!("NAV_PAGE_FWD: prose row-fill current={} ({},{})->({},{})",
                             state.current_line, from.0, from.1, nt, no);
                    set_page_instant_offset(state, nt, no);
                }
                return;
            }
```

Note: `is_line_fully_visible` must be true when the line's first row is on
the current page even if its tail hangs past the fold. Verify its prose
behavior: run `rg -n "fn is_line_fully_visible" src/input/viewport.rs` and
read the function. If it requires the WHOLE line visible (it compares the
line's full yrange against the viewport), add and use this variant in
`viewport.rs` instead of changing the original:

```rust
/// True when `line`'s FIRST visual row is inside the current viewport.
/// Prose row-fill visibility: a straddling paragraph "is on" the page where
/// it starts (and on later pages via page_top_line == line with offset > 0).
pub(crate) fn is_line_start_visible(state: &crate::app::AppState, line: usize) -> bool {
    use gtk4::prelude::{TextBufferExt, TextViewExt, WidgetExt};
    if line == state.page_top_line {
        return state.page_top_offset == 0;
    }
    let Some(iter) = state.buffer.iter_at_line(line as i32) else { return false };
    let (y, _h) = state.text_view.line_yrange(&iter);
    let Some(titer) = state.buffer.iter_at_line(state.page_top_line as i32) else { return false };
    let (ty, _th) = state.text_view.line_yrange(&titer);
    let y0 = ty + state.page_top_offset;
    let guard = descender_guard_px(&state.text_view, state.page_top_line);
    let usable = state.text_view.height() - guard - crate::input::scroll::BASE_BOTTOM_MARGIN;
    y >= y0 && y < y0 + usable
}
```

and use `is_line_start_visible` for both the early-out and the loop condition
in the prose arm (leave the play paths on `is_line_fully_visible`).

- [ ] **Step 2: Build + reproduce the original screenshots' sequence headlessly**

Run: `cargo build`

```bash
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_DEV=1 LIT_HEADLESS_TEST=1 \
  LIT_LOG_PATH=/tmp/prose-j.log LINUX_LIT_WORK=BH \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 4
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 2
for i in 1 2 3 4 5 6 7 8; do wtype "j"; sleep 1; done
grim /tmp/prose-j-final.png
pkill -f "cage -- ./target/debug/linux-lit"
rg "NAV_PAGE_FWD" /tmp/prose-j.log
```

Expected: `NAV_PAGE_FWD: prose row-fill` lines with intermediate offsets; the
screenshot shows no skipped text between consecutive j-turned pages (verify by
reading the PNG and the preceding page's capture — repeat with `grim` per
press if needed).

- [ ] **Step 3: Commit**

```bash
git add src/input/scroll.rs src/input/viewport.rs
git commit -m "fix(prose): j/cursor-follow page turns step row-fill boundaries — no more tail skip"
```

---

### Task 5: Generation, persistence, load, and gate for the prose table

**Files:**
- Modify: `src/input/prose_pages.rs` (add GTK-bound generation/load/gate —
  mirrors `src/input/page_table.rs:323-586`)
- Modify: `src/app/mod.rs` (three new AppState fields)
- Modify: the play-table hook call sites (find with
  `rg -n "page_table::(load_for_work|generate_and_store|revalidate_on_resize)" src/`)

**Interfaces:**
- Consumes: Task 1 types + validator, Task 2 storage, Task 3
  `prose_next_boundary`, `page_table::layout_fingerprint` (reuse — it already
  encodes `columns`), `snapshot::db_fingerprint`.
- Produces: `active_prose_page_table(state) -> Option<Rc<Vec<ProsePage>>>`,
  `prose_table_boundary_for_line(state, line) -> Option<(usize, i32)>`,
  `prose_table_page_end(state) -> Option<(usize, i32)>` (current page's
  exclusive end), `generate_and_store_prose(state)`, `load_for_prose_work
  (state)`, `revalidate_prose_on_resize(state)`. Tasks 6 and 9 consume these.

- [ ] **Step 1: AppState fields**

In `src/app/mod.rs`, next to the existing `page_table` fields (search
`rg -n "page_table" src/app/mod.rs`), add and initialize (same pattern):

```rust
    /// Pinned prose page table (visual-row pages) for the current work, when
    /// one was loaded/generated for the CURRENT layout fingerprint.
    pub prose_page_table: std::cell::RefCell<Option<std::rc::Rc<Vec<crate::input::prose_pages::ProsePage>>>>,
    pub prose_page_table_fp: std::cell::RefCell<String>,
    pub prose_page_table_gen_attempted: std::cell::Cell<bool>,
```

Initialize as `RefCell::new(None)`, `RefCell::new(String::new())`,
`Cell::new(false)` at the same construction site as the play fields, and
reset `prose_page_table_gen_attempted` wherever the play
`page_table_gen_attempted` is reset on work switch (find with
`rg -n "page_table_gen_attempted" src/`).

- [ ] **Step 2: Generation + load + gate in `src/input/prose_pages.rs`**

Append (GTK-bound section; the pure section from Task 1 stays on top):

```rust
/// Walk the LIVE engine's forward chain from (0,0), recording every page.
/// Boundaries come from `navigation::prose_next_boundary` — the same
/// function x/j use — so the stored grid IS the live grid.
pub fn record_prose_pages(
    state: &mut crate::app::AppState,
) -> Result<Vec<ProsePage>, String> {
    use gtk4::prelude::{TextBufferExt, TextViewExt};
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Err("no lines".into());
    }
    // Drive the walk through the real page state, then restore it.
    let saved = (state.page_top_line, state.page_top_offset);
    state.page_top_line = 0;
    state.page_top_offset = 0;
    let mut pages: Vec<ProsePage> = Vec::new();
    let mut guard = 0usize;
    loop {
        let start = (state.page_top_line, state.page_top_offset);
        match crate::input::navigation::prose_next_boundary(state) {
            Some((nl, no)) => {
                pages.push(ProsePage {
                    start_line: start.0, start_off: start.1,
                    end_line: nl, end_off: no,
                });
                state.page_top_line = nl;
                state.page_top_offset = no;
            }
            None => {
                // Final page: ends at the document's pixel end.
                let last = line_count - 1;
                let h = state.buffer.iter_at_line(last as i32)
                    .map(|it| state.text_view.line_yrange(&it).1)
                    .unwrap_or(0);
                pages.push(ProsePage {
                    start_line: start.0, start_off: start.1,
                    end_line: last, end_off: h,
                });
                break;
            }
        }
        guard += 1;
        if guard > line_count.max(64) * 4 {
            state.page_top_line = saved.0;
            state.page_top_offset = saved.1;
            return Err("determinism: forward chain did not terminate".into());
        }
    }
    state.page_top_line = saved.0;
    state.page_top_offset = saved.1;
    Ok(pages)
}
```

Then `generate_and_store_prose`, `load_for_prose_work`,
`revalidate_prose_on_resize`, `active_prose_page_table` — copy
`page_table.rs:323-586` and adapt mechanically. The deltas (everything else
is identical):

- Gate on `state.column_count() == 1 && state.is_prose()` instead of `== 2`.
- Env vars unchanged (`LIT_NO_PAGE_TABLE`, `LIT_GEN_PAGE_TABLE`).
- After `record_prose_pages`, build heights exactly as
  `page_table.rs:360-364` does and validate with
  `validate_prose_pages(&pages, &ProseValidateCtx { line_count, heights:
  &heights, usable_height: usable })`; on `Err(e)` log
  `PAGES_PROSE: VALIDATE_FAIL {e}` and return.
- Citation mapping: BOUNDARY LINES ONLY. `id_of(bi)` as in
  `page_table.rs:396-400`. A page's `start_line`/`end_line` with no
  `line_mapping` id is a hard `PAGES_PROSE: VALIDATE_FAIL citation` (Task 3's
  boundary normalization already skips blank lines, and both BH boundary
  lines in practice are real paragraphs; do NOT snap — snapping would break
  exact adjacency).
- Store via `crate::db::prose_pages::{ensure_schema, store_pages}` with
  `ProsePageRow { page_no: (i+1) as i64, start_line_id, start_off:
  p.start_off as i64, end_line_id, end_off: p.end_off as i64 }`.
- Loader: resolve ids via the same `id_to_buf` map
  (`page_table.rs:486-494`); any unresolvable id → log
  `PAGES_PROSE: fallback (row id not in buffer)` and return. No unsnap logic
  (no chrome snapping was done). Rebuild `ProsePage` with
  `start_off: r.start_off as i32` etc.
- `active_prose_page_table(state)`: `None` when `LIT_NO_PAGE_TABLE` set,
  `state.translations_visible`, `state.column_count() != 1`,
  `!state.is_prose()`, or navigation mode is not EReader (same match as
  `page_table.rs:575-586`).

Add the two consumer helpers:

```rust
/// The stored page boundary whose interval contains `line`'s FIRST row —
/// where a cursor-follow / sync landing for that line should put the page.
pub fn prose_table_boundary_for_line(
    state: &crate::app::AppState,
    line: usize,
) -> Option<(usize, i32)> {
    let table = active_prose_page_table(state)?;
    let i = prose_page_for_line(&table, line)?;
    Some((table[i].start_line, table[i].start_off))
}

/// Exclusive end of the CURRENT page (matched by (page_top_line,
/// page_top_offset)). None = current position is off-grid or no table.
pub fn prose_table_page_end(state: &crate::app::AppState) -> Option<(usize, i32)> {
    let table = active_prose_page_table(state)?;
    let i = prose_page_for_position(&table, state.page_top_line, state.page_top_offset)?;
    let p = &table[i];
    (p.start_line == state.page_top_line && p.start_off == state.page_top_offset)
        .then_some((p.end_line, p.end_off))
}
```

- [ ] **Step 3: Hook the lifecycle call sites**

Run: `rg -n "page_table::(load_for_work|generate_and_store|revalidate_on_resize)|resnap_to_table" src/`

At EVERY site found, add the prose twin immediately after the play call:

```rust
crate::input::prose_pages::load_for_prose_work(&s);      // after load_for_work
crate::input::prose_pages::generate_and_store_prose(&mut s); // after generate_and_store
crate::input::prose_pages::revalidate_prose_on_resize(&s);   // after revalidate_on_resize
```

(Adjust `&s`/`&mut s`/`state` to the local binding at each site. The play
gates make the two calls mutually exclusive per work — a 2-col play no-ops
the prose fns and vice versa. NOTE: `generate_and_store_prose` and
`record_prose_pages` need `&mut AppState` because the walk drives
`page_top_line` — if a call site only has `&AppState`, wrap the fields it
mutates in the same interior-mutability pattern the site already uses for
`page_table` (`RefCell`), or restructure `record_prose_pages` to take the
raw parts (`text_view`, `buffer`, initial usable) instead of `&mut state`;
prefer the raw-parts refactor if any site blocks on borrowing.)

- [ ] **Step 4: Table consumption in `page_forward` / `page_backward`**

In `page_forward`, immediately BEFORE the live prose branch added in Task 3,
insert:

```rust
    // Pinned prose table: pure index arithmetic (mirrors the play table arm).
    if let Some(table) = crate::input::prose_pages::active_prose_page_table(state) {
        if let Some(cur) = crate::input::prose_pages::prose_page_for_position(
            &table, state.page_top_line, state.page_top_offset)
        {
            if cur + 1 >= table.len() {
                let visible_end = super::viewport::last_fully_visible_line(state, state.page_top_line);
                if visible_end > state.current_line {
                    state.current_line = visible_end;
                    after_page_change(state, PageChangeReason::Forward);
                }
                log_fmt!("PAGES_PROSE: page {}/{} (at end)", cur + 1, table.len());
                return;
            }
            let next = table[cur + 1];
            state.page_back_stack.push((state.page_top_line, state.page_top_offset));
            state.current_line = next.start_line;
            crate::input::scroll::set_page_instant_offset(state, next.start_line, next.start_off);
            after_page_change(state, PageChangeReason::Forward);
            log_fmt!("PAGES_PROSE: page {}/{} top=({},{})",
                     cur + 2, table.len(), next.start_line, next.start_off);
            return;
        }
        // Off-grid (resume from an old session): fall through to live fill;
        // the next turn lands back on the grid via Task 6's resnap.
    }
```

In `page_backward`, before its live fallback (after the back-stack pop logic
— read the function first; the stack path must keep priority), add the
mirror: `prose_page_for_position` on the current `(top, offset)`; if `cur ==
0` move cursor to first content line; else `set_page_instant_offset` to
`table[cur - 1]`'s start and set `state.current_line = table[cur - 1]
.start_line`, logging `PAGES_PROSE: page {cur}/{len} top=(..)`.

- [ ] **Step 5: Build, test, generate for BH headlessly**

```bash
cargo build && cargo test --bins
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_DEV=1 LIT_HEADLESS_TEST=1 \
  LIT_GEN_PAGE_TABLE=1 LIT_LOG_PATH=/tmp/prose-gen.log LINUX_LIT_WORK=BH \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 6
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 6   # settled-layout hook fires generation
pkill -f "cage -- ./target/debug/linux-lit"
rg "PAGES_PROSE" /tmp/prose-gen.log
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT COUNT(*) FROM prose_pages WHERE work_abbrev='BH';
   SELECT * FROM prose_pages_meta WHERE work_abbrev='BH';"
```

Expected: `PAGES_PROSE: generated N pages for BH fp=v1|…` in the log;
`prose_pages` rows present; meta `validated=1`. A `VALIDATE_FAIL` here is a
real engine bug — STOP and debug it (the invariants are the acceptance
criteria), do not weaken the validator.

- [ ] **Step 6: Commit**

```bash
git add src/input/prose_pages.rs src/app/mod.rs src/input/navigation.rs \
        $(git diff --name-only | rg 'app|scroll' || true)
git commit -m "feat(prose-pages): generate, validate, persist + consume pinned prose page table"
```

---

### Task 6: Route highlight/visibility consumers through the prose grid

**Files:**
- Modify: `src/input/highlight.rs` (the two `table_top_for` sites at lines
  39 and 75)
- Modify: `src/input/scroll.rs` (`scroll_after_jump_forward`'s table lookup,
  line 1120-1123)
- Modify: `src/input/prose_pages.rs` (resnap helper)

**Interfaces:**
- Consumes: Task 5 `prose_table_boundary_for_line`, `active_prose_page_table`.
- Produces: sync/nav landings on the stored grid; `resnap_prose_to_table
  (state)` (mirror of `page_table::resnap_to_table`,
  `src/input/page_table.rs:615-629`).

- [ ] **Step 1: Teach the three landing sites the prose grid**

At each site, the current pattern is
`crate::input::page_table::table_top_for(state, line)` feeding a whole-line
`set_page*`. Add the prose branch FIRST (it returns an offset):

```rust
    if let Some((pt, po)) =
        crate::input::prose_pages::prose_table_boundary_for_line(state, state.current_line)
    {
        if (pt, po) != (state.page_top_line, state.page_top_offset) {
            crate::input::scroll::set_page_instant_offset(state, pt, po);
        }
    } else if let Some(t) = crate::input::page_table::table_top_for(state, state.current_line) {
        // existing play-table branch, unchanged
        ...
    } else {
        // existing live fallback, unchanged
        ...
    }
```

For `highlight.rs`, read lines 20-100 first (`Read` the range) and preserve
each site's surrounding conditions exactly — only the top source changes.
In `scroll_after_jump_forward` the prose arm from Task 4 runs BEFORE the
table lookup; move the Task 4 arm's body to: try
`prose_table_boundary_for_line` first, and only fall back to the
`prose_next_boundary` walk when it returns `None` (no table).

- [ ] **Step 2: Resnap after grid swap**

Append to `src/input/prose_pages.rs`:

```rust
/// Re-anchor an off-grid (page_top_line, page_top_offset) onto the active
/// prose grid (mirror of page_table::resnap_to_table).
pub fn resnap_prose_to_table(state: &mut crate::app::AppState) {
    let Some(table) = active_prose_page_table(state) else { return };
    if prose_page_for_position(&table, state.page_top_line, state.page_top_offset)
        .map(|i| (table[i].start_line, table[i].start_off)
             == (state.page_top_line, state.page_top_offset))
        .unwrap_or(false)
    {
        return; // already on the grid
    }
    let Some(i) = prose_page_for_line(&table, state.current_line) else { return };
    let (t, o) = (table[i].start_line, table[i].start_off);
    crate::logging::log(&format!(
        "PAGES_PROSE: resnap off-grid ({},{}) -> ({},{}) (cursor {})",
        state.page_top_line, state.page_top_offset, t, o, state.current_line
    ));
    crate::input::scroll::set_page_instant_offset(state, t, o);
}
```

Call it wherever `page_table::resnap_to_table` is called
(`rg -n "resnap_to_table" src/` — add the prose twin after each).

- [ ] **Step 3: Build + full e2e sweep**

```bash
cargo build && cargo test --bins
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work BH --secs 120
```

Expected: clipping invariant PASS; fuzz FAIL count 0 for BH. Open every PNG
in `target/ui/` and report contents inline per the UI review protocol. If
the fuzz's prose assertions predate offsets (they compare whole-line tops),
apply the play-table lesson: make the assertion read the prose table
boundaries in table mode (`nav_test.rs` — same pattern as the
`s.is_section_start` fix), and note it in the commit.

- [ ] **Step 4: Commit**

```bash
git add -A src/
git commit -m "feat(prose-pages): sync/nav landings + resnap on the pinned prose grid"
```

---

### Task 7: No-text-loss e2e assertion

**Files:**
- Create: `tests/prose_row_fill.rs`
- Test: itself (uses `tests/harness/mod.rs`)

**Interfaces:**
- Consumes: harness helpers in `tests/harness/mod.rs` (cage launch,
  screenshot, `wtype` key injection, `TEST_VIEWPORT_RECT` region) — read that
  file first and reuse its existing launch/screenshot functions verbatim.
- Produces: `#[ignore]`d test `prose_pages_tile_without_gaps`.

- [ ] **Step 1: Write the test**

The assertion that catches this bug class end-to-end: page N's bottom row
and page N+1's top row are ADJACENT — implemented via the log, not pixels.
Under `LIT_HEADLESS_TEST` the app already logs page state; drive `x` 12
times from `gg` in BH, parse `PAGES_PROSE: page K/N top=(line,off)` lines
from the isolated `LIT_LOG_PATH` log, load the stored table from lit.db
(`rusqlite` dev-dependency is already available to tests via the crate), and
assert:

```rust
//! Prose row-fill no-text-loss invariant: consecutive pages tile exactly.
mod harness;

#[test]
#[ignore]
fn prose_pages_tile_without_gaps() {
    // 1. Launch BH via the harness (LIT_HEADLESS_TEST, isolated log).
    // 2. Send: "gg" then "x" x12 (harness key helper), 800ms apart.
    // 3. Read the log; collect PAGES_PROSE "top=(l,o)" tuples in order.
    // 4. Open lit.db read-only; load prose_pages rows for BH at the meta's
    //    fingerprint; map page_no -> (start,end).
    // 5. For each consecutive visited pair (a, b): the row whose start == a
    //    must have end == b (exclusive-end tiling — zero gap, zero overlap).
    // 6. Assert at least 10 turns actually happened (guards silent no-ops).
}
```

Write the real code following `tests/smoke.rs`'s structure (read it first;
reuse its launch + key + log-path plumbing — the five numbered steps above
each become 3-10 lines using those helpers).

- [ ] **Step 2: Run it**

```bash
./scripts/e2e-env.sh cargo test --test prose_row_fill -- --ignored --nocapture
```

Expected: PASS with >= 10 tiled transitions printed.

- [ ] **Step 3: Commit**

```bash
git add tests/prose_row_fill.rs
git commit -m "test(prose): e2e no-text-loss tiling invariant for prose row-fill pages"
```

---

### Task 8: litdb — populate phrase_timestamps for BH (both editions)

**Files (all in `~/utono/litdb`):**
- Run: `scripts/build_phrase_timestamps.py` (exists; fix bit-rot only if the
  dry-run fails)

- [ ] **Step 1: Dry-run both editions**

```bash
cd ~/utono/litdb
python scripts/build_phrase_timestamps.py BH 244 \
  ~/Music/dickens-charles/whisperx-cache/BleakHouse_ep6.whisperX-transcript-medium.en.json \
  --dry-run
python scripts/build_phrase_timestamps.py BH 243 \
  ~/Music/dickens-charles/whisperx-cache/BleakHouseTheAudibleDickensCollection_ep6.whisperX-transcript-medium.en.json \
  --dry-run
```

(Media 244 = `BleakHouse_ep6.m4b`, 243 = the Audible edition — pairing
confirmed from `media_files.path`.) Expected: `Built N phrases` with sample
rows whose `"snippet"` text matches sane `[start_char:end_char]` slices.
The Audible file has a "This is Audible." preamble — the aligner must skip
it; if samples look shifted, debug the alignment before writing (the script
is untested at scale; treat failures as script bugs, fix, re-dry-run).

- [ ] **Step 2: Write + verify**

Re-run both WITHOUT `--dry-run`. Expected: `Inserted N phrase_timestamps
rows` + `Verify: N phrases, T0s - T1s` per edition, where T1 is near each
audiobook's duration. Then spot-check the screenshot paragraph:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT pt.start_time, pt.start_char, pt.end_char,
       substr(lm.canonical_text, pt.start_char + 1, pt.end_char - pt.start_char)
FROM phrase_timestamps pt JOIN line_mapping lm ON lm.id = pt.line_mapping_id
WHERE lm.work_abbrev = 'BH' AND lm.canonical_text LIKE 'Has Mr. Tulkinghorn any idea%'
ORDER BY pt.media_id, pt.start_char LIMIT 40;"
```

Expected: monotone `start_time`s walking through the Lady Dedlock paragraph,
snippets matching the text ("Sladdery", "turn them round my finger" appear
late in the char range).

- [ ] **Step 3: Wire the step into the litdb production workflow**

Find where production imports are orchestrated:

```bash
rg -n "line_timestamps|populate_wordstream|whisperx" \
  ~/utono/litdb/scripts/orchestrate_production.py \
  ~/utono/litdb/docs/wizard-gutenberg-workflow.md | head -20
```

Add a `build_phrase_timestamps.py` invocation as the step AFTER line
timestamps are populated, in whichever of the two artifacts drives new
imports (the orchestrator script if it runs the timestamp steps itself; the
wizard workflow doc's step list otherwise — match how the
`populate_wordstream_timestamps` step is described there). The step is
per-media-file, takes the same WhisperX JSON the line-timestamp step used,
and is skipped when no WhisperX cache exists.

- [ ] **Step 4: Commit (litdb repo)**

```bash
cd ~/utono/litdb && git add -A scripts/ docs/ && \
  git commit -m "feat(phrases): run build_phrase_timestamps in the production workflow (+ BH backfill fixes)"
```

(Adjust the message to what actually changed; lit.db data itself is not
committed.)

---

### Task 9: Sync — phrase-timestamp page crossing

**Files:**
- Modify: `src/db/queries.rs` (crossing-time query)
- Modify: `src/app/mod.rs` (pending-cross field)
- Modify: `src/main.rs` (`CursorSync` arm ~line 141-334 sets the pending
  cross; `TimePos` arm at `src/main.rs:377` fires it)
- Modify: `src/input/navigation.rs` (pure interpolation fn + tests)

**Interfaces:**
- Consumes: Task 5 `active_prose_page_table`, `prose_page_for_line`;
  `state.media_id: Option<i64>` (`src/app/mod.rs:309`); work line fields
  `l.id`, `l.timestamp: Option<TimeRange>`.
- Produces: `queries::phrase_crossing_time(conn, line_mapping_id, media_id,
  char_off) -> Option<f64>`; `AppState.pending_prose_cross:
  Option<(f64, usize)>` (fire time, target page index);
  `navigation::interpolate_cross_time(start, end, char_off, char_len) -> f64`.

- [ ] **Step 1: Pure interpolation fallback + test**

In `src/input/navigation.rs` near `preroll_seek_time`
(`src/input/navigation.rs:64`):

```rust
/// Char-fraction interpolation of a page-break crossing time inside one
/// line's audio window — the fallback when no phrase_timestamps exist for
/// the playing media file.
pub fn interpolate_cross_time(start: f64, end: f64, char_off: usize, char_len: usize) -> f64 {
    if char_len == 0 || end <= start {
        return start;
    }
    start + (end - start) * (char_off.min(char_len) as f64 / char_len as f64)
}
```

Test (same file's test mod):

```rust
#[test]
fn interpolate_cross_time_is_proportional_and_clamped() {
    assert_eq!(interpolate_cross_time(10.0, 20.0, 50, 100), 15.0);
    assert_eq!(interpolate_cross_time(10.0, 20.0, 0, 100), 10.0);
    assert_eq!(interpolate_cross_time(10.0, 20.0, 200, 100), 20.0);
    assert_eq!(interpolate_cross_time(10.0, 20.0, 50, 0), 10.0);   // degenerate
    assert_eq!(interpolate_cross_time(10.0, 10.0, 50, 100), 10.0); // degenerate
}
```

Run: `cargo test interpolate_cross_time` — Expected: PASS.

- [ ] **Step 2: Crossing-time query**

In `src/db/queries.rs` (follow the file's existing `pub fn ... (conn:
&Connection, ...)` style):

```rust
/// Narration time at which the audio reaches char offset `char_off` within
/// the line: the first phrase whose char range extends past that offset.
/// None = no phrase rows for this (line, media) — caller interpolates.
pub fn phrase_crossing_time(
    conn: &rusqlite::Connection,
    line_mapping_id: i64,
    media_id: i64,
    char_off: usize,
) -> Option<f64> {
    conn.query_row(
        "SELECT start_time FROM phrase_timestamps
         WHERE line_mapping_id = ?1 AND media_id = ?2 AND end_char > ?3
         ORDER BY start_char LIMIT 1",
        rusqlite::params![line_mapping_id, media_id, char_off as i64],
        |row| row.get(0),
    )
    .ok()
}
```

- [ ] **Step 3: Schedule the cross in the CursorSync arm**

`src/app/mod.rs`: add `pub pending_prose_cross: Option<(f64, usize)>,`
(init `None`; clear it wherever `pending_advance` is cleared on work switch
— `rg -n "pending_advance" src/app/mod.rs src/main.rs`).

In `src/main.rs`'s CursorSync arm, after
`update_highlight_and_advance_page` + `after_page_change` (lines 317-323),
insert:

```rust
                            // Prose straddling paragraph: if the spoken
                            // paragraph continues onto the next stored page,
                            // schedule a TimePos-driven turn at the moment the
                            // narration crosses the page boundary.
                            s.pending_prose_cross = None;
                            if s.is_prose() {
                                if let Some(table) =
                                    crate::input::prose_pages::active_prose_page_table(&s)
                                {
                                    if let Some(pi) = crate::input::prose_pages::prose_page_for_position(
                                        &table, s.page_top_line, s.page_top_offset)
                                    {
                                        let p = table[pi];
                                        // Straddles: this page ENDS inside the cursor's line.
                                        if pi + 1 < table.len() && p.end_line == buffer_line
                                            && crate::input::prose_pages::prose_page_for_line(
                                                &table, buffer_line) != Some(pi + 1)
                                        {
                                            if let Some(t) = prose_cross_time(&s, buffer_line, p.end_off) {
                                                let fire = t - s.config.sync_preroll_secs();
                                                crate::logging::log(&format!(
                                                    "SYNC_PROSE_CROSS: scheduled t={:.2} page {}->{}",
                                                    fire, pi + 1, pi + 2));
                                                s.pending_prose_cross = Some((fire, pi + 1));
                                            }
                                        }
                                    }
                                }
                            }
```

Add the helper in `src/main.rs` (or `navigation.rs` if main.rs has no free
fns — match repo style):

```rust
/// Time the narration reaches the current page's bottom boundary inside
/// buffer line `bl` (whose boundary pixel offset is `end_off`).
/// phrase_timestamps when available, char-fraction interpolation otherwise.
fn prose_cross_time(s: &app::AppState, bl: usize, end_off: i32) -> Option<f64> {
    use gtk4::prelude::{TextBufferExt, TextViewExt};
    let wi = s.work_line_for_buffer(bl)?;
    let work = s.current_work.as_ref()?;
    let line = work.lines.get(wi)?;
    let ts = line.timestamp.as_ref()?;
    // Boundary pixel -> char offset within the buffer line.
    let iter = s.buffer.iter_at_line(bl as i32)?;
    let (y, _h) = s.text_view.line_yrange(&iter);
    let biter = s.text_view.iter_at_location(1, y + end_off)?;
    let char_off = biter.line_offset().max(0) as usize;
    let media = s.media_id?;
    if let Ok(conn) = crate::db::queries::open_db() {
        if let Some(t) = crate::db::queries::phrase_crossing_time(&conn, line.id, media, char_off) {
            return Some(t);
        }
    }
    // Fallback: interpolate across the line's audio window.
    let char_len = s.buffer.iter_at_line(bl as i32)
        .map(|it| {
            let mut e = it;
            e.forward_to_line_end();
            e.line_offset().max(0) as usize
        })
        .unwrap_or(0);
    Some(crate::input::navigation::interpolate_cross_time(ts.start, ts.end, char_off, char_len))
}
```

Check the exact sync-preroll config accessor first
(`rg -n "preroll" src/config.rs`) and use the real name in place of
`sync_preroll_secs()`. Check `TimeRange` field names in `src/db/models.rs`
(`start`/`end` assumed — verify).

- [ ] **Step 4: Fire it in the TimePos arm**

In `src/main.rs`'s `MpvEvent::TimePos(pos)` arm (line 377), add at the top
of the arm (after the state borrow):

```rust
                        if let Some((fire_at, page_idx)) = s.pending_prose_cross {
                            if pos >= fire_at && s.mpv_playing {
                                s.pending_prose_cross = None;
                                if let Some(table) =
                                    crate::input::prose_pages::active_prose_page_table(&s)
                                {
                                    if let Some(p) = table.get(page_idx).copied() {
                                        crate::logging::log_always(&format!(
                                            "SYNC_PROSE_CROSS: fired pos={:.2} -> page {} top=({},{})",
                                            pos, page_idx + 1, p.start_line, p.start_off));
                                        crate::input::scroll::set_page_instant_offset(
                                            &mut s, p.start_line, p.start_off);
                                        crate::input::navigation::after_page_change(
                                            &mut s,
                                            crate::input::navigation::PageChangeReason::MpvSync,
                                        );
                                    }
                                }
                            }
                        }
```

Cursor stays on the straddling paragraph (`current_line` unchanged) — the
highlight's visible portion follows the page.

- [ ] **Step 5: Build + automated sync verification**

```bash
cargo build && cargo test --bins
```

Then run the playback-sync harness (real mpv) against BH:
invoke the `test-playback-sync` skill per its SKILL.md, targeting BH, and
verify the log shows `SYNC_PROSE_CROSS: scheduled` followed by
`SYNC_PROSE_CROSS: fired` with the page advancing mid-paragraph, and no
premature `update_highlight_and_advance_page` turn for straddling
paragraphs. Expected: no sync stall, no double turn.

- [ ] **Step 6: Commit**

```bash
git add src/db/queries.rs src/app/mod.rs src/main.rs src/input/navigation.rs
git commit -m "feat(sync): phrase-timestamp page crossing for straddling prose paragraphs"
```

---

### Task 10: hot repo schema mirror

**Files (all in `~/utono/hot`):**
- Modify: `schema/schema.sql` (after the `page_spread_meta` block, line ~226)
- Modify: `wizards/hotdb/schema.py` (mirror — read how `page_spread` was
  added in commit `25ed69d` first: `git -C ~/utono/hot show 25ed69d` and
  follow that commit's full pattern, including its tests)

- [ ] **Step 1: Add the tables to `schema/schema.sql`**

hot keys by citation TEXT + canonical BASE abbrev (its convention, unlike
lit.db's per-edition ids — see the `page_spread` header comment):

```sql
CREATE TABLE prose_page (
    work                TEXT NOT NULL REFERENCES work(abbrev),  -- canonical BASE abbrev
    layout_fingerprint  TEXT NOT NULL,
    page_no             INTEGER NOT NULL,     -- 1-based, contiguous
    start_citation      TEXT NOT NULL,        -- paragraph the page opens in
    start_row_offset    INTEGER NOT NULL,     -- px from paragraph top (row-snapped)
    end_citation        TEXT NOT NULL,        -- paragraph the page ends in
    end_row_offset      INTEGER NOT NULL,     -- exclusive px bottom edge
    PRIMARY KEY (work, layout_fingerprint, page_no)
);

CREATE TABLE prose_page_meta (
    work                TEXT NOT NULL REFERENCES work(abbrev),
    layout_fingerprint  TEXT NOT NULL,
    content_fingerprint TEXT NOT NULL,
    page_count          INTEGER NOT NULL,
    generated_at        TEXT NOT NULL,
    validated           INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (work, layout_fingerprint)
);
```

- [ ] **Step 2: Mirror in `wizards/hotdb/schema.py` + tests, following `25ed69d`**

Replicate exactly what that commit did for `page_spread` (schema constant /
migration entry / test), renamed for `prose_page`/`prose_page_meta`. Run
hot's test suite the way that commit's message or `~/utono/hot/CLAUDE.md`
prescribes (read it first — hot has its own conventions).

- [ ] **Step 3: Commit (hot repo)**

```bash
cd ~/utono/hot && git add schema/schema.sql wizards/hotdb/schema.py wizards/tests/
git commit -m "feat(schema): prose_page/prose_page_meta — pinned prose visual-row pagination"
```

---

## Verification (whole feature)

- [ ] `cargo build && cargo test && cargo clippy` clean in linux-lit.
- [ ] `./scripts/e2e-env.sh cargo test -- --ignored --nocapture` (smoke +
  line_clipping + prose_row_fill) all PASS; review every PNG in `target/ui/`
  inline.
- [ ] Nav-fuzz: `./scripts/e2e-env.sh
  .claude/skills/test-headless-navigation/run-fuzz.sh --start-work BH` — 0
  FAILs.
- [ ] Hand the user the manual eyeball check (real GL renderer):
  `crll`, open BH at the Chapter 2 Lady Dedlock paragraph, press `j` through
  it — the Sladdery tail must appear, pages must fill to the bottom row.
- [ ] `LIT_NO_PAGE_TABLE=1` run still paginates correctly (live engine
  fallback).
