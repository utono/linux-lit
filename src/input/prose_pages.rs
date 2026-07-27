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
    /// widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN at that layout.
    pub usable_height: i32,
    /// Ink-free overshoot the fit invariant tolerates (see `prose_fit_slack`).
    /// A boundary advanced past a row whose INK fits the budget may land up
    /// to one inter-paragraph gap past `usable_height` in BOX space — that
    /// excess is trailing/leading spacing only, never glyph ink.
    pub fit_slack: i32,
    /// Buffer lines that START a chapter (`Line.is_chapter`), ascending. Each
    /// must begin a page — a chapter heading never renders mid-page, like a
    /// printed book. Empty for a work with no chapter data, which disables the
    /// invariant. See the `chapter` check in `validate_prose_pages`.
    pub chapter_starts: &'a [usize],
}

/// Maximum ink-free pixel gap between one display row's bottom and the next
/// row's top: the paragraph spacing (`pixels_below_lines` + `pixels_above_lines`)
/// plus intra-paragraph wrap spacing, plus rounding. Bounds both the
/// `next_row_top_if_row_fits` boundary correction in `prose_next_boundary`
/// and the validator's fit tolerance, so the two stay provably consistent.
pub fn prose_fit_slack(view: &sourceview5::View) -> i32 {
    use gtk4::prelude::TextViewExt;
    view.pixels_above_lines() + view.pixels_below_lines() + view.pixels_inside_wrap() + 2
}

/// Lexicographic order on (line, off).
fn pos_le(al: usize, ao: i32, bl: usize, bo: i32) -> bool {
    (al, ao) <= (bl, bo)
}

/// Pixel height of the half-open interval [start, end) given per-line heights.
fn page_px(p: &ProsePage, heights: &[i32]) -> i64 {
    let end = p.end_line.min(heights.len().saturating_sub(1));
    let px: i64 = heights[p.start_line..=end].iter().map(|&h| h as i64).sum();
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
        // fit: the page's pixel height must fit the viewport. `fit_slack`
        // tolerates a boundary advanced past a fully-fitting row into the
        // following inter-paragraph gap (box-space overshoot with no ink).
        let px = page_px(p, ctx.heights);
        if px > (ctx.usable_height + ctx.fit_slack) as i64 {
            return Err(format!(
                "fit: page {} spans {}px > usable {} (+slack {})",
                i + 1, px, ctx.usable_height, ctx.fit_slack
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
    // chapter: every chapter-start line must BEGIN a page, so a heading never
    // renders mid-page (the printed-book rule `prose_next_boundary` enforces
    // via its chapter clamp). This is the only content-aware invariant here;
    // the rest are pure geometry. Front matter before chapter 1 is not a
    // chapter start, so a work with no chapters passes vacuously.
    for &cs in ctx.chapter_starts {
        if cs == 0 {
            continue; // line 0 is page 1's start by the `first` invariant
        }
        let starts_a_page = pages
            .iter()
            .any(|p| p.start_line == cs && p.start_off == 0);
        if !starts_a_page {
            return Err(format!(
                "chapter: chapter-start line {cs} does not begin a page \
                 (heading would render mid-page)"
            ));
        }
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

// ---------------------------------------------------------------------------
// GTK-bound generation / persistence / load / gate (mirrors page_table.rs
// 323-586 for the single-column prose case). All new logging uses the
// `PAGES_PROSE:` prefix.
// ---------------------------------------------------------------------------

/// Prose-specific layout fingerprint. Extends the play `layout_fingerprint`
/// with the LIVE usable height (`text_view.height() - descender_guard -
/// SINGLE_COLUMN_BOTTOM_MARGIN`). That derived value can settle ±1px across runs at the
/// SAME window size — the play fingerprint (window geometry + font metrics)
/// would not notice, but a 1px shift in `usable` moves a prose visual-row
/// boundary. Encoding it keeps a stored prose grid from being loaded against a
/// live layout that would compute a different boundary. NOTE: this does NOT
/// change `page_table::layout_fingerprint` — the play tables' stored
/// fingerprints must stay valid.
pub fn prose_layout_fingerprint(state: &crate::app::AppState) -> String {
    use gtk4::prelude::WidgetExt;
    let base = crate::input::page_table::layout_fingerprint(state);
    let widget_height = state.text_view.height();
    let guard = crate::input::viewport::descender_guard_px(&state.text_view, 0);
    let usable = widget_height - guard - crate::input::scroll::SINGLE_COLUMN_BOTTOM_MARGIN;
    // `pv3`: prose-boundary normalization version. Bumped when the meaning of a
    // stored boundary changes so every previously-stored prose table misses and
    // regenerates lazily. pv2 = leading-gap page ends normalized to (L, 0) (a
    // paragraph with zero visible rows on a page is never that page's end_line
    // with a positive offset). pv3 = chapter-at-top: the page breaks before any
    // chapter heading, so a heading always opens a page (prose_next_boundary's
    // chapter_clamp). Does NOT touch the play `layout_fingerprint`.
    // `cw`: the effective (prose-widened, font-adaptive) card width. The base
    // fingerprint's window size proxied card width only while column_width was
    // a constant; the 78-char prose measure makes card width vary with font
    // metrics AND the measure rule, so key it explicitly — a measure-rule
    // change must miss stored tables and regenerate, never serve stale
    // boundaries.
    let cw = crate::app::layout::effective_column_width(state);
    // pv4: row-fit boundary correction (a row whose ink fits the budget stays
    // on its page even when the raw boundary lands in the following gap) —
    // pv3 tables have one-row-short pages at such boundaries.
    // pv5: single-column fill reserve reduced to SINGLE_COLUMN_BOTTOM_MARGIN
    // (was BASE_BOTTOM_MARGIN — every pv4 page is ~one line short). The `uh`
    // component already shifts with the reserve; the bump makes the miss
    // explicit and independent of geometry coincidences.
    format!("{base}|uh{usable}|cw{cw}|pv5")
}

/// Walk the LIVE engine's forward chain from (0,0), recording every page.
/// Boundaries come from `navigation::prose_next_boundary` — the same function
/// x/j use — so the stored grid IS the live grid.
///
/// **Pre-validation is load-bearing.** `prose_next_boundary` accumulates REAL
/// per-line heights via `line_yrange`, but GTK4 validates a TextView's line
/// layout lazily around the currently-scrolled viewport. A walk that only
/// mutated `page_top_line` (no scroll) left every line past the initial
/// validation frontier reporting a coarse single-row height ESTIMATE — so the
/// bounded walk under-accumulated, `total - y0` never exceeded `usable`,
/// `prose_next_boundary` returned `None` mid-document, and the "final page"
/// spanned the whole un-walked remainder (observed: a 601941px page ~1/4 into
/// BH). We fix that by forcing GTK to validate EVERY line's true height up
/// front (a single `line_yrange` sweep — `line_yrange` validates the line
/// synchronously and GTK caches the result), so the subsequent walk reads real
/// heights the whole way WITHOUT scrolling per page (which was O(pages) full
/// re-layouts and far too slow on a novel). The walk then just mutates the page
/// state and restores it; it is synchronous with no GTK main-loop iteration
/// between steps, so no idle/render callback observes the intermediate state.
pub fn record_prose_pages(
    state: &mut crate::app::AppState,
) -> Result<Vec<ProsePage>, String> {
    use gtk4::prelude::{TextBufferExt, TextViewExt};
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Err("no lines".into());
    }
    // Force GTK to validate every line's true wrapped height once, so the
    // per-page boundary walk below never reads a lazy single-row estimate for a
    // far-off line (see the doc comment). The results are cached by GTK.
    for i in 0..line_count {
        if let Some(it) = state.buffer.iter_at_line(i as i32) {
            let _ = state.text_view.line_yrange(&it);
        }
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

/// Gate, record, validate, persist. Called from the app's settled-layout hook;
/// every early return logs its reason so the fallback is diagnosable. Takes
/// `&mut AppState` because `record_prose_pages` drives `page_top_line`.
pub fn generate_and_store_prose(state: &mut crate::app::AppState) {
    if std::env::var_os("LIT_NO_PAGE_TABLE").is_some() {
        return;
    }
    let force = std::env::var_os("LIT_GEN_PAGE_TABLE").is_some();
    if state.prose_page_table_gen_attempted.get() {
        return;
    }
    if state.current_work.is_none() {
        return;
    }
    // Reader-state gate BEFORE latching `gen_attempted`. A transient non-ready
    // tick (e.g. the layout momentarily 2-col during startup, or translations
    // briefly visible) must NOT burn the one-shot attempt for the session — the
    // set below only fires once the state gate has passed, so a later settled
    // tick can still generate. (This is the fix for the ordering bug where one
    // early tick permanently disabled prose-table generation.)
    if state.column_count() != 1 || !state.is_prose() || state.translations_visible {
        crate::logging::log("PAGES_PROSE: gen skipped (not 1-col prose reader state)");
        return;
    }
    state.prose_page_table_gen_attempted.set(true);
    if state.prose_page_table.borrow().is_some() && !force {
        return; // already loaded from the DB this session
    }
    use gtk4::prelude::{TextBufferExt, TextViewExt, WidgetExt};
    let fp = prose_layout_fingerprint(state);
    let pages = match record_prose_pages(state) {
        Ok(p) => p,
        Err(e) => {
            crate::logging::log(&format!("PAGES_PROSE: VALIDATE_FAIL {e}"));
            return;
        }
    };
    // Build the validation context from live geometry (heights measured AFTER
    // the full forward walk, so every line has been really measured by GTK).
    let line_count = state.effective_line_count();
    let heights: Vec<i32> = (0..line_count)
        .map(|i| state.buffer.iter_at_line(i as i32)
            .map(|it| state.text_view.line_yrange(&it).1)
            .unwrap_or(0))
        .collect();
    let widget_height = state.text_view.height();
    let guard = crate::input::viewport::descender_guard_px(&state.text_view, 0);
    let usable = widget_height - guard - crate::input::scroll::SINGLE_COLUMN_BOTTOM_MARGIN;
    // Chapter-start buffer lines, from the line map when present (it maps
    // buffer -> work rows) else straight off the work lines. Same resolution
    // `prose_next_boundary`'s `is_chapter_at` closure uses, so the validator
    // and the generator agree on what a chapter start is.
    let chapter_starts: Vec<usize> = match state.current_work.as_ref() {
        Some(work) => (0..line_count)
            .filter(|&bl| {
                if let Some(ref lm) = state.line_map {
                    lm.buffer_to_work
                        .get(bl)
                        .and_then(|o| o.as_ref())
                        .and_then(|wi| work.lines.get(*wi))
                        .map(|l| l.is_chapter)
                        .unwrap_or(false)
                } else {
                    work.lines.get(bl).map(|l| l.is_chapter).unwrap_or(false)
                }
            })
            .collect(),
        None => Vec::new(),
    };
    let ctx = ProseValidateCtx {
        line_count,
        heights: &heights,
        usable_height: usable,
        fit_slack: prose_fit_slack(&state.text_view),
        chapter_starts: &chapter_starts,
    };
    if let Err(e) = validate_prose_pages(&pages, &ctx) {
        crate::logging::log(&format!("PAGES_PROSE: VALIDATE_FAIL {e}"));
        return;
    }
    // Citation mapping: BOUNDARY LINES ONLY (start_line / end_line of each
    // page). A boundary line with no line_mapping id is a hard failure — do NOT
    // snap (snapping would break the exact page-to-page adjacency the no-text-
    // loss guarantee depends on).
    let work = state.current_work.as_ref().unwrap();
    let id_of = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| work.lines.get(wi))
            .map(|l| l.id)
    };
    let mut rows = Vec::with_capacity(pages.len());
    for (i, p) in pages.iter().enumerate() {
        let (Some(start_line_id), Some(end_line_id)) = (id_of(p.start_line), id_of(p.end_line)) else {
            crate::logging::log(&format!(
                "PAGES_PROSE: VALIDATE_FAIL citation: page {} boundary has no line_mapping id", i + 1));
            return;
        };
        rows.push(crate::db::prose_pages::ProsePageRow {
            page_no: (i + 1) as i64,
            start_line_id,
            start_off: p.start_off as i64,
            end_line_id,
            end_off: p.end_off as i64,
        });
    }
    let meta = crate::db::prose_pages::PagesMeta {
        layout_fingerprint: fp.clone(),
        db_fingerprint: crate::snapshot::db_fingerprint(work),
        page_count: rows.len() as i64,
        generated_at: epoch_timestamp(),
        validated: true,
    };
    match crate::db::queries::open_db_rw() {
        Ok(mut conn) => {
            if let Err(e) = crate::db::prose_pages::ensure_schema(&conn)
                .and_then(|_| crate::db::prose_pages::store_pages(
                    &mut conn, &work.abbrev, &meta, &rows))
            {
                crate::logging::log(&format!("PAGES_PROSE: store failed ({e}) — will retry next load"));
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("PAGES_PROSE: db open failed ({e})"));
            return;
        }
    }
    crate::logging::log(&format!(
        "PAGES_PROSE: generated {} pages for {} fp={}", rows.len(), work.abbrev, fp));
    // Make the fresh table active this session.
    *state.prose_page_table.borrow_mut() = Some(std::rc::Rc::new(pages));
    *state.prose_page_table_fp.borrow_mut() = fp;
}

/// Load a stored prose table for the current work if BOTH fingerprints match.
/// Resolves line_mapping ids to buffer lines via the id->buffer map; any
/// unresolvable id drops the whole table.
pub fn load_for_prose_work(state: &crate::app::AppState) {
    *state.prose_page_table.borrow_mut() = None;
    state.prose_page_table_fp.borrow_mut().clear();
    if std::env::var_os("LIT_NO_PAGE_TABLE").is_some() {
        return;
    }
    let Some(work) = state.current_work.as_ref() else { return };
    if state.column_count() != 1 || !state.is_prose() || state.translations_visible {
        return;
    }
    let fp = prose_layout_fingerprint(state);
    let Ok(conn) = crate::db::queries::open_db() else { return };
    // missing tables etc. fall back to None.
    let loaded = crate::db::prose_pages::load_pages(&conn, &work.abbrev, &fp).unwrap_or_default();
    let Some((meta, rows)) = loaded else {
        crate::logging::log(&format!("PAGES_PROSE: no table for {} fp={}", work.abbrev, fp));
        return;
    };
    if meta.db_fingerprint != crate::snapshot::db_fingerprint(work) {
        crate::logging::log("PAGES_PROSE: fallback (db_fingerprint stale — re-import?)");
        return;
    }
    // id -> buffer line, built once. No unsnap logic: no chrome snapping was
    // done on the storage side (boundary ids are exact real paragraphs).
    let line_count = state.effective_line_count();
    let mut id_to_buf = std::collections::HashMap::new();
    for bi in 0..line_count {
        if let Some(wi) = state.work_line_for_buffer(bi) {
            if let Some(l) = work.lines.get(wi) {
                id_to_buf.entry(l.id).or_insert(bi);
            }
        }
    }
    let mut pages = Vec::with_capacity(rows.len());
    for r in &rows {
        let (Some(&start_line), Some(&end_line)) =
            (id_to_buf.get(&r.start_line_id), id_to_buf.get(&r.end_line_id)) else {
            crate::logging::log("PAGES_PROSE: fallback (row id not in buffer)");
            return;
        };
        pages.push(ProsePage {
            start_line,
            start_off: r.start_off as i32,
            end_line,
            end_off: r.end_off as i32,
        });
    }
    crate::logging::log(&format!(
        "PAGES_PROSE: table hit ({} pages) for {}", pages.len(), work.abbrev));
    *state.prose_page_table.borrow_mut() = Some(std::rc::Rc::new(pages));
    *state.prose_page_table_fp.borrow_mut() = fp;
}

/// Drop a loaded/generated prose table whose fingerprint no longer matches the
/// CURRENT layout, then try `load_for_prose_work` once. Mirror of
/// `page_table::revalidate_on_resize`: this function itself never
/// (re)generates — it only drops and retries a LOAD.
///
/// Its CALLER (the resize tick in `app/mod.rs`) does regenerate as of
/// 2026-07-27, by clearing `prose_page_table_gen_attempted` when this drops a
/// table that no stored fingerprint replaces. Before that, a resize left the
/// reader on the live engine for the rest of the session — the generation
/// latch resets only on work load — which silently lost pinned pagination and
/// the chapter-at-top rule baked into the stored grid.
pub fn revalidate_prose_on_resize(state: &crate::app::AppState) {
    if state.prose_page_table.borrow().is_none() {
        return;
    }
    let current_fp = prose_layout_fingerprint(state);
    if current_fp == *state.prose_page_table_fp.borrow() {
        return; // still valid at this geometry
    }
    crate::logging::log("PAGES_PROSE: dropped table (layout changed)");
    *state.prose_page_table.borrow_mut() = None;
    state.prose_page_table_fp.borrow_mut().clear();
    load_for_prose_work(state);
}

/// The single consumption gate for the prose table. `None` when disabled, when
/// translations are visible, when not single-column prose, or when navigation
/// mode is not EReader.
pub fn active_prose_page_table(
    state: &crate::app::AppState,
) -> Option<std::rc::Rc<Vec<ProsePage>>> {
    if std::env::var_os("LIT_NO_PAGE_TABLE").is_some() {
        return None;
    }
    if state.translations_visible
        || state.column_count() != 1
        || !state.is_prose()
        || !matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader)
    {
        return None;
    }
    state.prose_page_table.borrow().clone()
}

/// The stored page boundary whose interval contains `line`'s FIRST row — where
/// a cursor-follow / sync landing for that line should put the page.
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

/// Stored `end_off` of the page whose top is exactly `(top_line, top_off)`.
/// Companion to `prose_table_last_line_for_top`: that gives the last rendered
/// LINE, this gives how much of that line is actually on the page. The clip
/// needs both or it charges a straddling last line its full height and the page
/// reads as overfull. `None` = no table, or not a canonical page top.
pub fn prose_table_end_off_for_top(
    state: &crate::app::AppState,
    top_line: usize,
    top_off: i32,
) -> Option<i32> {
    let table = active_prose_page_table(state)?;
    let i = prose_page_for_position(&table, top_line, top_off)?;
    let p = &table[i];
    if p.start_line != top_line || p.start_off != top_off {
        return None; // not a canonical page top — live path
    }
    Some(p.end_off)
}

/// Last buffer line RENDERED on the stored page whose top is
/// `(top_line, top_off)` — the prose analogue of `page_table::table_end_for_top`
/// (the "read the table, never re-walk live" lesson). The page interval is
/// `[start, end)` with `end` EXCLUSIVE: when `end_off > 0`, `end_line` itself
/// has ink on the page (its first `end_off` px), so it is the last rendered
/// line; when `end_off == 0` (normalized full-height end), `end_line` starts the
/// NEXT page, so the last rendered line is `end_line - 1`. `None` = no prose
/// table active, or `(top_line, top_off)` is not a canonical stored page top:
/// callers fall back to the live `visible_range` walk.
pub fn prose_table_last_line_for_top(
    state: &crate::app::AppState,
    top_line: usize,
    top_off: i32,
) -> Option<usize> {
    let table = active_prose_page_table(state)?;
    let i = prose_page_for_position(&table, top_line, top_off)?;
    let p = &table[i];
    if p.start_line != top_line || p.start_off != top_off {
        return None; // not a canonical page top — live path
    }
    Some(last_rendered_line(p))
}

/// Inclusive last RENDERED buffer line of `p`. Pure half of
/// `prose_table_last_line_for_top`, split out so the exclusive/inclusive
/// conversion the bottom clip depends on is unit-testable without GTK.
///
/// `end` is EXCLUSIVE: `end_off > 0` means `end_line` has ink on this page (its
/// first `end_off` px), so it IS the last rendered line; `end_off == 0` is the
/// normalized full-height form where `end_line` starts the NEXT page.
pub(crate) fn last_rendered_line(p: &ProsePage) -> usize {
    if p.end_off > 0 {
        p.end_line
    } else {
        p.end_line.saturating_sub(1)
    }
}

/// True when buffer line `line` is RENDERED on stored page `p` — i.e. it lies
/// in `[start_line, last_rendered_line(p)]`.
///
/// The table, not the viewport geometry, is the authority for "is it on this
/// page". On a page whose stored span overflows the viewport (CLIP_WARN #12)
/// the renderer still clips at the STORED end, so a line can be painted on
/// screen while a geometric fit-walk calls it invisible. Navigation checks that
/// re-walk the live geometry then disagree with what the user can see. Same
/// lesson as `page_table::table_end_for_top`: read the table, never re-walk.
pub(crate) fn line_on_stored_page(p: &ProsePage, line: usize) -> bool {
    line >= p.start_line && line <= last_rendered_line(p)
}

/// True when `line` is rendered on the CURRENT prose page (matched by
/// `(page_top_line, page_top_offset)`). `None` = no prose table active or the
/// current top is off-grid: the caller falls back to the live geometry walk.
pub fn prose_table_line_on_current_page(
    state: &crate::app::AppState,
    line: usize,
) -> Option<bool> {
    let table = active_prose_page_table(state)?;
    let i = prose_page_for_position(&table, state.page_top_line, state.page_top_offset)?;
    let p = &table[i];
    if p.start_line != state.page_top_line || p.start_off != state.page_top_offset {
        return None; // not a canonical page top — live path
    }
    Some(line_on_stored_page(p, line))
}

/// Last buffer line shown WHOLE on the stored page whose top is
/// `(top_line, top_off)` — for a clip check ("does the bottom line fit?"). A
/// prose row-fill page can END mid-line (`end_off < end_line`'s full height):
/// that final line is only PARTIALLY on the page (its overflow is hidden by the
/// bottom clip and belongs to the next page), so it must NOT be required to fit
/// whole. The last WHOLE line is then `end_line - 1`; only when the page ends at
/// a line's full height is `end_line` itself fully shown. `None` = no prose
/// table, or not a canonical page top (live path).
pub fn prose_table_last_whole_line_for_top(
    state: &crate::app::AppState,
    top_line: usize,
    top_off: i32,
) -> Option<usize> {
    use gtk4::prelude::{TextBufferExt, TextViewExt};
    let table = active_prose_page_table(state)?;
    let i = prose_page_for_position(&table, top_line, top_off)?;
    let p = &table[i];
    if p.start_line != top_line || p.start_off != top_off {
        return None;
    }
    // end is EXCLUSIVE. If it lands at end_line's FULL height (or the normalized
    // end_off == 0 form), end_line is shown whole -> it is the last whole line.
    // Otherwise end_line is only partially shown (its overflow hidden by the
    // bottom clip, belonging to the next page), so the last WHOLE line is the
    // one above it.
    if p.end_off == 0 {
        // Ends at the previous line's full height. The last whole line is
        // end_line-1 IF it is at/after the page's first whole line; otherwise
        // (a single partial line) there is none.
        return whole_line_or_none(p.end_line.wrapping_sub(1), p.start_line, p.start_off);
    }
    let end_line_height = state.buffer.iter_at_line(p.end_line as i32)
        .map(|it| state.text_view.line_yrange(&it).1)
        .unwrap_or(0);
    if p.end_off >= end_line_height {
        // Ends at end_line's full height -> end_line is shown whole.
        whole_line_or_none(p.end_line, p.start_line, p.start_off)
    } else {
        // end_line is partial -> the last whole line is the one above it.
        whole_line_or_none(p.end_line.wrapping_sub(1), p.start_line, p.start_off)
    }
}

/// `Some(candidate)` if `candidate` is a genuinely whole line on a page whose
/// first line is `start_line` (partial when `start_off > 0`): the first whole
/// line is `start_line` when `start_off == 0`, else `start_line + 1`. Returns
/// `None` when no whole line exists (e.g. a single over-tall paragraph page
/// showing only a middle slice — nothing on it may be required to fit whole).
fn whole_line_or_none(candidate: usize, start_line: usize, start_off: i32) -> Option<usize> {
    let first_whole = if start_off > 0 { start_line + 1 } else { start_line };
    (candidate != usize::MAX && candidate >= first_whole).then_some(candidate)
}

/// Re-anchor an off-grid (page_top_line, page_top_offset) onto the active prose
/// grid (mirror of `page_table::resnap_to_table`). No-op when no prose table is
/// active, the current top is already a canonical page start, or the cursor's
/// line is not covered by the table.
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

/// ISO-ish timestamp without adding a chrono dependency: seconds since epoch,
/// prefixed so it's self-explaining in a DB browse. (Mirror of the play
/// table's helper.)
fn epoch_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{now}")
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
        ProseValidateCtx { line_count: h.len(), heights: h, usable_height: 120, fit_slack: 0, chapter_starts: &[] }
    }

    #[test]
    fn valid_pages_pass() {
        let h = heights();
        assert_eq!(validate_prose_pages(&ok_pages(), &ctx(&h)), Ok(()));
    }

    /// The bottom clip turns this into its EXCLUSIVE `exact_end` with `+ 1`,
    /// so an off-by-one here paints one line too many — which is exactly the
    /// "chapter heading rendered below the end of the previous chapter" bug
    /// (2026-07-27). Values taken from the real BH-Barrett v5 table: page 82
    /// is (686,0)..(697,0), so the last rendered line is 696 and the clip's
    /// exclusive end is 697 — the "CHAPTER VIII" line, which must NOT paint.
    #[test]
    fn last_rendered_line_respects_exclusive_end() {
        // end_off == 0: end_line starts the NEXT page.
        let p = ProsePage { start_line: 686, start_off: 0, end_line: 697, end_off: 0 };
        assert_eq!(last_rendered_line(&p), 696);
        assert_eq!(last_rendered_line(&p) + 1, 697, "exclusive exact_end");
        // end_off > 0: end_line has ink on THIS page, so it is the last line.
        let q = ProsePage { start_line: 10, start_off: 0, end_line: 14, end_off: 40 };
        assert_eq!(last_rendered_line(&q), 14);
        // Degenerate page at the document start must not underflow.
        let z = ProsePage { start_line: 0, start_off: 0, end_line: 0, end_off: 0 };
        assert_eq!(last_rendered_line(&z), 0);
    }

    /// A dialogue/segment jump (`q` `,` `;` `'` `j` `k`) must count a target as
    /// "on this page" when the STORED page renders it — even when the page
    /// overflows the viewport and the target's rows fall past where rows stop
    /// fitting. Geometry and the table disagree on an overflowing page; the
    /// renderer clips at the stored end (`d2696b1f`), so navigation must read
    /// the same authority or the cursor is trapped.
    ///
    /// Real BH-Barrett case (2026-07-27): page top 931, stored end 936,
    /// `total=1175 > widget_h=1098` (CLIP_WARN #12). Pressing `q` at line 934
    /// computed target 935 — inside the stored page — but the geometric check
    /// said "not visible", so `keep_jump_if_on_page` reverted 935 -> 934 and
    /// `q` did nothing, forever.
    #[test]
    fn line_within_stored_page_is_on_page_even_when_page_overflows() {
        // (931,0)..(936,80): end_off > 0, so 936 is the last RENDERED line.
        let p = ProsePage { start_line: 931, start_off: 0, end_line: 936, end_off: 80 };
        assert_eq!(last_rendered_line(&p), 936);

        // The trapped target: strictly inside the stored span.
        assert!(
            line_on_stored_page(&p, 935),
            "935 is rendered on the stored page 931..=936"
        );
        // The page top and the last rendered line are both on-page.
        assert!(line_on_stored_page(&p, 931), "page top is on-page");
        assert!(line_on_stored_page(&p, 936), "last rendered line is on-page");
        // Neighbours outside the stored span are NOT — a jump there needs a turn,
        // which the no-page-turn rule must still refuse.
        assert!(!line_on_stored_page(&p, 930), "line above the page top");
        assert!(!line_on_stored_page(&p, 937), "line past the stored end");
    }

    /// The exclusive-end form must not claim a line the page does not render.
    /// `end_off == 0` means `end_line` starts the NEXT page.
    #[test]
    fn line_on_stored_page_respects_exclusive_end() {
        let p = ProsePage { start_line: 686, start_off: 0, end_line: 697, end_off: 0 };
        assert!(line_on_stored_page(&p, 696), "last rendered line");
        assert!(
            !line_on_stored_page(&p, 697),
            "697 starts the next page (the CHAPTER VIII line)"
        );
    }

    /// A chapter heading must never render mid-page (the printed-book rule).
    /// `ok_pages()` has page 2 starting at line 1 offset 140 — so line 1 is
    /// NOT a page start, and declaring it a chapter start must fail.
    #[test]
    fn chapter_start_not_at_a_page_top_fails() {
        let h = heights();
        let c = ProseValidateCtx { chapter_starts: &[1], ..ctx(&h) };
        let err = validate_prose_pages(&ok_pages(), &c).unwrap_err();
        assert!(err.starts_with("chapter:"), "got: {err}");
    }

    /// A line that DOES begin a page is a legitimate chapter start. Uses its
    /// own uniform-height fixture so the page spans are trivially inside the
    /// fit budget and ONLY the chapter invariant is under test.
    #[test]
    fn chapter_start_at_a_page_top_passes() {
        // Four 50px lines, one line per page: every line is a page top.
        let h = vec![50, 50, 50, 50];
        let pages = vec![
            ProsePage { start_line: 0, start_off: 0, end_line: 1, end_off: 0 },
            ProsePage { start_line: 1, start_off: 0, end_line: 2, end_off: 0 },
            ProsePage { start_line: 2, start_off: 0, end_line: 3, end_off: 50 },
        ];
        let c = ProseValidateCtx {
            line_count: 4,
            heights: &h,
            usable_height: 120,
            fit_slack: 0,
            chapter_starts: &[1, 2],
        };
        assert_eq!(validate_prose_pages(&pages, &c), Ok(()));
    }

    /// Line 0 is page 1's start by construction, so a chapter there is fine —
    /// and a work with no chapter data disables the invariant entirely.
    #[test]
    fn chapter_at_line_zero_and_no_chapters_both_pass() {
        let h = heights();
        let at_zero = ProseValidateCtx { chapter_starts: &[0], ..ctx(&h) };
        assert_eq!(validate_prose_pages(&ok_pages(), &at_zero), Ok(()));
        let none = ProseValidateCtx { chapter_starts: &[], ..ctx(&h) };
        assert_eq!(validate_prose_pages(&ok_pages(), &none), Ok(()));
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
    fn leading_gap_normalized_end_puts_line_on_next_page() {
        // Part B invariant: a page ending exactly at (L, 0) does NOT contain L —
        // line L belongs to the NEXT page. This is the normalized form a
        // leading-gap boundary collapses to (prose_next_boundary Part A): the
        // paragraph shows zero rows on this page, so its first row (L, 0) is the
        // next page's top. Mirrors the offset-aware on-page rule the sync
        // advance path uses: L is on the page iff (L,0) < (end_line, end_off).
        let h = vec![100, 100, 100];
        let p = vec![
            // page 0 ends at (1, 0): line 1 has ZERO rows here (leading gap).
            ProsePage { start_line: 0, start_off: 0, end_line: 1, end_off: 0 },
            // page 1 starts at line 1's top and carries it whole into line 2.
            ProsePage { start_line: 1, start_off: 0, end_line: 2, end_off: 100 },
        ];
        let c = ProseValidateCtx { line_count: 3, heights: &h, usable_height: 220, fit_slack: 0, chapter_starts: &[] };
        assert_eq!(validate_prose_pages(&p, &c), Ok(()));
        // Line 1's FIRST row lands on page 1, never page 0.
        assert_eq!(prose_page_for_line(&p, 1), Some(1),
            "a page ending at (1,0) does not contain line 1");
        // The exclusive-end position (1, 0) resolves to the page it OPENS (page 1),
        // not the page it closes (page 0) — page tops are canonical.
        assert_eq!(prose_page_for_position(&p, 1, 0), Some(1));
        // And a true straddle (end_off > 0) DOES keep the line on-page.
        assert_eq!(prose_page_for_position(&p, 2, 50), Some(1),
            "line 2 with 50px on page 1 is on-page (true straddle)");
    }

    #[test]
    fn normalized_full_height_end_matches_next_line_top() {
        // Page ends at exactly line 0's full height; next starts at (1, 0).
        let h = vec![100, 100];
        let p = vec![
            ProsePage { start_line: 0, start_off: 0, end_line: 0, end_off: 100 },
            ProsePage { start_line: 1, start_off: 0, end_line: 1, end_off: 100 },
        ];
        let c = ProseValidateCtx { line_count: 2, heights: &h, usable_height: 120, fit_slack: 0, chapter_starts: &[] };
        assert_eq!(validate_prose_pages(&p, &c), Ok(()));
    }
}
