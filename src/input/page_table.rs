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
    /// widget_height - descender_guard - TWO_COLUMN_BOTTOM_MARGIN at that layout.
    pub usable_height: i32,
}

/// The invariant suite (design doc §Invariant suite, items 1-4). Returns the
/// FIRST violated invariant as "<name>: <details>".
pub fn validate_spreads(spreads: &[Spread], ctx: &ValidateCtx) -> Result<(), String> {
    if spreads.is_empty() {
        return Err("coverage: no spreads".into());
    }
    // sanity + monotone coverage. The live engine legitimately TRIMS
    // non-dialogue tails (dangling speaker names, blank lines, stage
    // directions) off a page's end, so `column_split(top).page_end` can be
    // several lines SHORT of where the next page actually starts. Consecutive
    // recorded spreads can therefore have a gap — as long as every line in
    // that gap is non-dialogue, nothing was lost off the page and this is not
    // a coverage failure. A dialogue line stranded in a gap IS still a
    // failure: that's real content the table would never surface. Pages must
    // stay strictly ordered (`s.left_start > prev.end`); the first page's gap
    // is checked against line 0 the same way.
    //
    // EXCEPTION: the final consecutive pair. The last page is the canonical
    // `G`/final-`x` anchor (`last_page_top`), forward-pulled to fill both
    // columns, and is deliberately allowed to OVERLAP the chain's last
    // natural page (see `record_spreads`) — the documented, benign `y`-from-
    // the-end seam. For that pair only, require `final.left_start >
    // prev.left_start` (still strictly progressing, no duplicate/backward
    // page) and `final.end >= prev.end` (the anchor doesn't regress content
    // coverage); skip the dialogue-gap scan for that pair since an overlap by
    // construction leaves no gap. Interior pairs keep the strict rule above.
    let mut expect_start = 0usize; // one-past the previous page's end (or 0 initially)
    let last_idx = spreads.len() - 1;
    for (i, s) in spreads.iter().enumerate() {
        let is_final_pair = i == last_idx && i > 0;
        if is_final_pair {
            let prev = &spreads[i - 1];
            if !(s.left_start > prev.left_start) {
                return Err(format!(
                    "coverage: final page {} starts at {} but must be strictly after page {}'s start {}",
                    i + 1, s.left_start, i, prev.left_start
                ));
            }
            if !(s.end >= prev.end) {
                return Err(format!(
                    "coverage: final page {} ends at {} which regresses page {}'s end {}",
                    i + 1, s.end, i, prev.end
                ));
            }
        } else {
            if s.left_start < expect_start {
                return Err(format!(
                    "coverage: page {} starts at {} but previous page ended at {}",
                    i + 1, s.left_start, expect_start.saturating_sub(1)
                ));
            }
            if let Some(bad) = (expect_start..s.left_start)
                .find(|&j| ctx.is_dialogue.get(j).copied().unwrap_or(false))
            {
                return Err(format!(
                    "coverage: dialogue line {} falls between page {} and page {}",
                    bad, i, i + 1
                ));
            }
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
                // Diagnostic detail: where the pages sit and any section-start
                // lines near the gap, so a failure names the divergence.
                let win_lo = s.end.saturating_sub(2);
                let win_hi = (next_top + 6).min(ctx.line_count);
                let nearby: Vec<usize> = ctx
                    .section_starts
                    .map(|ss| {
                        (win_lo..win_hi).filter(|&j| ss.get(j).copied().unwrap_or(false)).collect()
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "watermark: page {} has an empty right column but page {} does not open a section \
                     (top={} end={} next_top={} section_starts_nearby={:?})",
                    i + 1, i + 2, s.left_start, s.end, next_top, nearby
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
///
/// Overlap is possible only between the last two pages (the canonical final
/// anchor spread is allowed to overlap its predecessor — see
/// `record_spreads`/`validate_spreads`'s final-pair exception). When `line`
/// falls in that overlap, prefer the EARLIER page: that's the natural
/// reading order (the reader reaches `line` on the earlier page first when
/// paging forward), and matches the live engine's own behavior of not
/// jumping ahead to the anchor until the forward-nav guard actually fires.
///
/// EXCEPTION: `line` that is itself a page's own `left_start` (a page TOP —
/// e.g. `state.page_top_line`, used by nav to ask "which page am I currently
/// on") always resolves to THAT page, never to an earlier page that merely
/// happens to overlap it. Without this, `page_backward`/`page_forward`
/// looking up their own current page top would be redirected to the earlier
/// overlapping page and mis-navigate by an extra page at the overlap.
pub fn page_for_line(spreads: &[Spread], line: usize) -> Option<usize> {
    let idx = spreads.partition_point(|s| s.left_start <= line);
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    if !(line <= spreads[i].end) {
        return None;
    }
    if spreads[i].left_start == line {
        return Some(i);
    }
    if i > 0 && line <= spreads[i - 1].end {
        return Some(i - 1);
    }
    Some(i)
}

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
    /// The running-head strip height (`TOP_SPACER_HEIGHT`) reserved above the
    /// first text line. It shrinks the columns' usable height, so it changes
    /// how many rows fit per page — a stored table baked at a different spacer
    /// height must be invalidated and regenerated. Included so the app
    /// self-heals when the strip height changes (no manual LIT_GEN_PAGE_TABLE).
    pub top_spacer_height: i32,
    /// The TEXT VIEW's height — the value the fit check actually validates
    /// against (`usable = view_height - guard - TWO_COLUMN_BOTTOM_MARGIN`).
    ///
    /// Added 2026-07-27. Every other size here is an INPUT to the layout;
    /// `width`/`height` above are the WINDOW's. The view's height is DERIVED
    /// from the window minus surrounding chrome, and it can differ between
    /// generation and load even when the window size is identical. Without it
    /// in the fingerprint, a table generated at a taller view kept matching and
    /// rendered a column TALLER than the viewport — `paged_bottom_clip` returns
    /// 0 on overflow (it cannot size a negative clip box), so the last row
    /// showed clipped with nothing masking it. Observed on Ant-Arkangel page 89:
    /// a 1102px left column in a 1071px usable height, cutting "A simple
    /// countryman that brought her figs." See
    /// docs/troubleshooting/clip-prevention.md checklist #12.
    pub view_height: i32,
}

/// "v1|" + the parts, pipe-joined. Human-readable on purpose: the
/// validate-play-pages skill prints it verbatim so a stale table is
/// self-explaining (you can see WHICH input moved).
pub fn fingerprint_string(p: &FingerprintParts) -> String {
    format!(
        "v5|{}|{}|{}|{}|{}|{}x{}|{}|{}|{}|{}|{}",
        p.font_family, p.font_size, p.ascent, p.descent, p.char_width,
        p.width, p.height, p.line_spacing, p.text_margins, p.columns,
        p.top_spacer_height, p.view_height
    )
}

/// GTK wrapper: probe the live view. Uses the same font source as
/// `descender_guard_px` (the `font-size` tag, avoiding the CSS-application
/// race) and the toplevel window size (the dwl-tiled size, e.g. 1920x1200).
pub fn layout_fingerprint(state: &crate::app::AppState) -> String {
    use gtk4::prelude::{TextTagExt, TextBufferExt, TextViewExt, WidgetExt};
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
        top_spacer_height: crate::app::TOP_SPACER_HEIGHT,
        // The height the fit check validates against — see the field's doc.
        view_height: state.text_view.height(),
    };
    fingerprint_string(&parts)
}

/// Walk the LIVE engine's forward chain once, recording every spread. The
/// chain advances on `column_split(top).next_page_top` — the documented
/// source of truth for spread boundaries (see
/// docs/troubleshooting/page-turning-mechanics.md, "column_split is the
/// source of truth") — so an empty-right (watermark) spread's successor
/// starts EXACTLY at the `(div1,div2)` section marker `column_split` found,
/// never at a dialogue-driven boundary that can skip past it. Includes a
/// non-termination guard (design invariant 5): the chain must strictly
/// advance and terminate within `line_count` steps.
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
        let next = cs.next_page_top;
        if next >= line_count || next <= top {
            break;
        }
        top = next;
        guard += 1;
        if guard > line_count {
            return Err("determinism: forward chain did not terminate".into());
        }
    }
    // The FINAL page must BE the same canonical anchor spread `G`/final-`x`
    // lands on in the live engine (`last_page_top`, navigation.rs) — not the
    // chain's own natural short tail page. This is deliberate live-engine UX
    // (docs/troubleshooting/page-turning-mechanics.md, "final spread is
    // special"): the anchor is forward-pulled so the tail content fills both
    // columns, and it is ALLOWED to overlap the chain's last natural page —
    // paging backward (`y`) from the anchor legitimately does not tile with
    // it ("a small benign seam — the fuzz exempts it"). Model that seam here
    // rather than truncating a survivor to force contiguity: drop whole
    // trailing spreads whose `left_start >= anchor` (never mutate a
    // surviving spread's `end`/`split`), then push the anchor spread itself.
    let anchor = crate::input::navigation::last_page_top(state);
    if spreads.last().map(|s| s.left_start) != Some(anchor) {
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
    }
    Ok(spreads)
}

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
    let Some(work) = state.current_work.as_ref() else { return };
    // Reader-state gate BEFORE latching `gen_attempted` (parity with the prose
    // path). A transient non-ready tick (layout momentarily 1-col, or
    // translations briefly visible) must NOT burn the one-shot attempt — the set
    // below only fires once the 2-col state gate has passed.
    if state.column_count() != 2 || state.translations_visible {
        crate::logging::log("PAGES: gen skipped (not 2-col reader state)");
        return;
    }
    state.page_table_gen_attempted.set(true);
    if state.page_table.borrow().is_some() && !force {
        return; // already loaded from the DB this session
    }
    use gtk4::prelude::{TextBufferExt, TextViewExt, WidgetExt};
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
    let usable = widget_height - guard - crate::input::scroll::TWO_COLUMN_BOTTOM_MARGIN;
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
    // Map buffer lines -> line_mapping ids. Boundary lines are usually work
    // lines (page tops/splits/ends land on real content), but a buffer line
    // can legitimately have no `line_mapping` row: a title-page preamble
    // (author/title lines before the first mapped line, e.g. MND's buffer
    // lines 0..6) or a blank/formatting-only line elsewhere in the buffer
    // (`mapped_buffer_lines` < total buffer lines is the normal case). The id
    // is purely a storage key (round-tripped back to A buffer line within the
    // same page by `load_for_work`), so snapping an unmapped boundary to the
    // nearest mapped line WITHIN this page is equivalent for pagination
    // purposes. `left_start` snaps forward (its page begins there); `end`
    // snaps backward (its page runs through there). Only a page with NO
    // mapped line at all in its range is a hard failure. A `split` with no
    // mapped line anywhere in its own range `[sp, end]` means the right
    // column holds nothing but unmapped filler (e.g. a lone blank line) —
    // there is no real content to navigate to there, so it is folded into
    // `None` (an empty right column) rather than failing generation.
    let id_of = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| work.lines.get(wi))
            .map(|l| l.id)
    };
    let id_of_fwd = |from: usize, to_incl: usize| -> Option<i64> {
        (from..=to_incl).find_map(id_of)
    };
    let id_of_back = |from_incl: usize, down_to: usize| -> Option<i64> {
        (down_to..=from_incl).rev().find_map(id_of)
    };
    let mut spreads = spreads;
    let mut rows = Vec::with_capacity(spreads.len());
    for (i, s) in spreads.iter_mut().enumerate() {
        let (Some(ls), Some(end)) = (id_of_fwd(s.left_start, s.end), id_of_back(s.end, s.left_start)) else {
            crate::logging::log(&format!(
                "PAGES: VALIDATE_FAIL citation: page {} boundary has no line_mapping id", i + 1));
            return;
        };
        let split_id = s.split.and_then(|sp| id_of_fwd(sp, s.end));
        if split_id.is_none() {
            s.split = None; // keep the in-memory spread consistent with the stored row
        }
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
        generated_at: epoch_timestamp(),
        validated: true,
    };
    match crate::db::queries::open_db_rw() {
        Ok(mut conn) => {
            if let Err(e) = crate::db::play_pages::ensure_schema(&conn)
                .and_then(|_| crate::db::play_pages::store_pages(
                    &mut conn, &work.abbrev, &meta, &rows))
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
        "PAGES: generated {} pages for {} fp={}", rows.len(), work.abbrev, fp));
    // Make the fresh table active this session.
    *state.page_table.borrow_mut() = Some(std::rc::Rc::new(spreads));
    *state.page_table_fp.borrow_mut() = fp;
}

/// Load a stored table for the current work if BOTH fingerprints match.
/// Resolves line_mapping ids to buffer lines via the id->buffer map built
/// from the live line map; any unresolvable id drops the whole table (stale
/// after re-import — db_fingerprint should have caught it, belt+braces).
pub fn load_for_work(state: &crate::app::AppState) {
    *state.page_table.borrow_mut() = None;
    state.page_table_fp.borrow_mut().clear();
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
    let loaded = match crate::db::play_pages::load_pages(&conn, &work.abbrev, &fp) {
        Ok(v) => v,
        Err(_) => None, // missing tables etc.
    };
    let Some((meta, rows)) = loaded else {
        crate::logging::log(&format!("PAGES: no table for {} fp={}", work.abbrev, fp));
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
    // Reverse the storage-side forward snap on page/column TOPS. Act/scene
    // headings, separator rules, and blanks have no line_mapping rows, so a
    // top that fell on that chrome was stored as the first MAPPED line at or
    // after it — rendering the page from the entrance stage direction and
    // hiding the "ACT 1 / Scene 1" heading the generator's (validated) walk
    // actually placed at the top. Walk back over contiguous unmapped lines to
    // the start of the chrome block, then skip any leading blanks (those are
    // the previous page's trimmed tail, not this page's heading). A mid-page
    // top whose predecessor is mapped walks zero steps; a blank-only gap
    // round-trips to the stored line — the unsnap is a no-op except where
    // heading chrome was lost.
    let unsnap_top = |mut bl: usize| -> usize {
        let stored = bl;
        while bl > 0 && state.work_line_for_buffer(bl - 1).is_none() {
            bl -= 1;
        }
        while bl < stored
            && state.work_line_for_buffer(bl).is_none()
            && crate::input::viewport::buffer_line_text(&state.buffer, bl).trim().is_empty()
        {
            bl += 1;
        }
        bl
    };
    for r in &rows {
        let (Some(&ls), Some(&end)) = (id_to_buf.get(&r.left_start_id), id_to_buf.get(&r.end_id)) else {
            crate::logging::log("PAGES: fallback (row id not in buffer)");
            return;
        };
        let split = match r.split_id {
            Some(id) => match id_to_buf.get(&id) {
                Some(&b) => Some(unsnap_top(b)),
                None => {
                    crate::logging::log("PAGES: fallback (split id not in buffer)");
                    return;
                }
            },
            None => None,
        };
        spreads.push(Spread { left_start: unsnap_top(ls), split, end });
    }
    crate::logging::log(&format!(
        "PAGES: table hit ({} pages) for {}", spreads.len(), work.abbrev));
    *state.page_table.borrow_mut() = Some(std::rc::Rc::new(spreads));
    *state.page_table_fp.borrow_mut() = fp;
}

/// Drop a loaded/generated table whose fingerprint no longer matches the
/// CURRENT layout — the fix for plain-resize staleness: `load_for_work` only
/// runs at work-load / the settled-layout hook, so a routine window resize
/// (dwl retiling) otherwise leaves the old `Rc<Vec<Spread>>` active with
/// boundaries computed for the old geometry. Called from the resize tick's
/// settled-size branch (see `app::mod`), NOT from every tick — this must run
/// at most once per settled resize.
///
/// If a table is active and its fingerprint has gone stale, clear it (and its
/// recorded fingerprint) and try `load_for_work` once so a stored table
/// matching the NEW fingerprint (if one exists in lit.db for this work/layout)
/// takes over immediately. If none exists, `load_for_work` leaves `page_table`
/// `None` and the live engine's fallback handles rendering/navigation — this
/// function deliberately does NOT call `generate_and_store`, so a page table
/// is never (re)generated as a side effect of a resize; regeneration stays
/// lazy (the existing settled-layout hook covers that separately).
pub fn revalidate_on_resize(state: &crate::app::AppState) {
    if state.page_table.borrow().is_none() {
        return;
    }
    let current_fp = layout_fingerprint(state);
    if current_fp == *state.page_table_fp.borrow() {
        return; // still valid at this geometry
    }
    crate::logging::log("PAGES: dropped table (layout changed)");
    *state.page_table.borrow_mut() = None;
    state.page_table_fp.borrow_mut().clear();
    load_for_work(state);
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

/// The stored page top whose spread contains `line`, when the table drives
/// navigation. Used by the playback-sync page turns so an auto turn lands on
/// the SAME page grid as `x`/`y` — in forward playback the spoken line then
/// sits as the first dialogue line of the new page instead of being
/// force-top-aligned by the live `page_turn_top_state`. `None` (no table
/// active, or the line falls in a trimmed non-dialogue gap) = fall back to
/// the live computation.
pub fn table_top_for(state: &crate::app::AppState, line: usize) -> Option<usize> {
    let table = active_page_table(state)?;
    let i = page_for_line(&table, line)?;
    Some(table[i].left_start)
}

/// Re-anchor an off-grid `page_top_line` onto the ACTIVE table's grid after a
/// table load/regeneration swapped the grid out from under the current page.
/// The startup resume snap runs against whatever grid is active at snap time
/// (often a stored table for a DIFFERENT fingerprint, dropped moments later at
/// settled geometry) — without this, the session reads from an off-grid top
/// until the first sync page turn re-anchors it, which the user experiences as
/// the highlight teleporting mid-page while the page barely shifts. No-op when
/// no table is active, the top is already canonical, or the cursor's line is
/// not covered by the table.
pub fn resnap_to_table(state: &mut crate::app::AppState) {
    let Some(table) = active_page_table(state) else { return };
    if spread_for_top(&table, state.page_top_line).is_some() {
        return; // already on the active grid
    }
    let Some(i) = page_for_line(&table, state.current_line) else { return };
    let t = table[i].left_start;
    if t != state.page_top_line {
        crate::logging::log(&format!(
            "PAGES: resnap off-grid page_top {} -> {} (cursor {})",
            state.page_top_line, t, state.current_line
        ));
        crate::input::scroll::set_page_instant(state, t);
    }
}

/// End (last rendered buffer line) of the stored spread whose top is `top`,
/// when the table drives rendering. The render path (scroll.rs) synthesizes
/// its ColumnSplit from this same spread — column_split() is NOT consulted in
/// table mode — so visibility/boundary checks must read the same source. The
/// live column_split can disagree with the stored spread at a matching layout
/// fingerprint (the fingerprint covers font metrics and window geometry, not
/// engine behavior), and a check against the live engine then never matches
/// what's actually on screen: playback sync and j-navigation walk the cursor
/// past the rendered page end without turning. `None` = no table active or
/// `top` is not a canonical stored page top; fall back to the live engine.
pub fn table_end_for_top(state: &crate::app::AppState, top: usize) -> Option<usize> {
    let table = active_page_table(state)?;
    spread_for_top(&table, top).map(|s| s.end)
}

/// ISO-ish timestamp without adding a chrono dependency (not in Cargo.toml):
/// seconds since epoch, prefixed so it's self-explaining in a DB browse.
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

    // 10 lines, all dialogue, uniform height 10, viewport fits 3+3 per spread.
    fn ctx<'a>(heights: &'a [i32], dlg: &'a [bool]) -> ValidateCtx<'a> {
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

    /// Regression (2026-07-27): a top that is NOT a stored `left_start` must
    /// still resolve to the spread CONTAINING it, so table mode never pairs a
    /// table-chosen top with a live-engine end.
    ///
    /// `spread_for_top` is an exact-top match by design (page tops are
    /// canonical). The render path used to fall straight through to the live
    /// `column_split` when it returned None — which is how a startup snap onto
    /// a non-canonical top produced a left column WIDER than either engine
    /// would choose alone (1102px into a 1098px viewport on Ant-Arkangel,
    /// clipping its last line). `page_for_line` is the containment fallback.
    #[test]
    fn a_top_inside_a_spread_resolves_to_that_spread() {
        let s = ok_spreads();
        // Exact tops still match exactly.
        assert_eq!(spread_for_top(&s, 0).map(|x| x.end), Some(5));
        assert_eq!(spread_for_top(&s, 6).map(|x| x.end), Some(9));
        // A top INSIDE the first spread has no exact match…
        assert!(spread_for_top(&s, 3).is_none());
        // …but is contained by it, which is what the render path now uses.
        let contained = page_for_line(&s, 3).and_then(|i| s.get(i)).copied();
        assert_eq!(contained.map(|x| (x.left_start, x.end)), Some((0, 5)));
    }

    #[test]
    fn valid_table_passes() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        assert!(validate_spreads(&ok_spreads(), &ctx(&h, &d)).is_ok());
    }

    #[test]
    fn gap_between_pages_fails_coverage() {
        // Three pages so the gap (between page 1 and page 2) is an INTERIOR
        // pair, not the final pair — the final-pair overlap exemption must
        // not mask a dialogue line stranded in an interior gap.
        let h = vec![10; 15];
        let d = vec![true; 15]; // line 6 (in the gap) is dialogue -> still a failure
        let s = vec![
            Spread { left_start: 0, split: Some(3), end: 5 },
            Spread { left_start: 7, split: Some(9), end: 9 }, // line 6 dropped
            Spread { left_start: 10, split: Some(13), end: 14 },
        ];
        let c = ValidateCtx {
            line_count: h.len(),
            is_dialogue: &d,
            section_starts: None,
            heights: &h,
            usable_height: 30,
        };
        let err = validate_spreads(&s, &c).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn non_dialogue_gap_between_pages_passes() {
        // A trimmed non-dialogue tail (dangling speaker name / stage
        // direction / blank line) between pages is legitimate: the live
        // engine's column_split(top).page_end can land short of where the
        // next page actually starts.
        let h = vec![10; 10];
        let mut d = vec![true; 10];
        d[6] = false; // the gap line is non-dialogue
        let s = vec![
            Spread { left_start: 0, split: Some(3), end: 5 },
            Spread { left_start: 7, split: Some(9), end: 9 }, // line 6 trimmed
        ];
        assert!(validate_spreads(&s, &ctx(&h, &d)).is_ok());
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
            Spread { left_start: 0, split: None, end: 2 },
            Spread { left_start: 3, split: Some(5), end: 6 },
            Spread { left_start: 7, split: Some(9), end: 9 },
        ];
        // Without a section start at the next page top: fail.
        let c1 = ValidateCtx {
            line_count: h.len(),
            is_dialogue: &d,
            section_starts: Some(&ss),
            heights: &h,
            usable_height: 30,
        };
        assert!(validate_spreads(&s, &c1).unwrap_err().contains("watermark"));
        // With it: pass.
        ss[3] = true;
        let c2 = ValidateCtx {
            line_count: h.len(),
            is_dialogue: &d,
            section_starts: Some(&ss),
            heights: &h,
            usable_height: 30,
        };
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

    #[test]
    fn final_page_may_overlap_predecessor() {
        let h = vec![10; 10];
        let mut d = vec![true; 10];
        d[9] = false; // trailing non-dialogue line so `end: 8` still satisfies the tail rule
        // Page 0: left col 0..=1 (2 lines, sums to 20), right col 2..=4 (3
        // lines, sums to 30) — both fit usable_height 30. Last page's
        // left_start (3) is inside page 0's range (0..=4) — the
        // canonical-anchor overlap. Must still pass: strictly progressing
        // start (3 > 0), and end (8) >= prev.end (4).
        let s = vec![
            Spread { left_start: 0, split: Some(2), end: 4 },
            Spread { left_start: 3, split: Some(6), end: 8 },
        ];
        assert!(validate_spreads(&s, &ctx(&h, &d)).is_ok(), "final-pair overlap must be exempt");

        // The SAME overlap shape one pair earlier (interior, not final) must
        // still fail — the exception is final-pair only.
        let s_interior = vec![
            Spread { left_start: 0, split: Some(2), end: 4 },
            Spread { left_start: 3, split: Some(4), end: 4 }, // overlaps page 0, NOT the last page
            Spread { left_start: 5, split: Some(8), end: 9 },
        ];
        let c = ValidateCtx {
            line_count: h.len(),
            is_dialogue: &d,
            section_starts: None,
            heights: &h,
            usable_height: 30,
        };
        let err = validate_spreads(&s_interior, &c).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn overlap_lines_resolve_to_earlier_page() {
        // Final page (index 1) overlaps the predecessor (index 0): page 0
        // spans 0..=7, page 1 starts at 6 (inside page 0's range) and spans
        // 6..=9. Lines strictly inside the overlap (7) that are NOT
        // themselves a page's own top resolve to the EARLIER page (natural
        // reading order). Line 6 IS page 1's own left_start (its identity as
        // a page top — what `page_top_line` holds after landing on it), so
        // it must resolve to page 1 itself, not be redirected backward — this
        // is what `page_backward`/`page_forward` rely on to find "which page
        // am I on" without mis-navigating by an extra page at the seam.
        let s = vec![
            Spread { left_start: 0, split: Some(3), end: 7 },
            Spread { left_start: 6, split: Some(9), end: 9 },
        ];
        assert_eq!(page_for_line(&s, 5), Some(0), "before overlap: page 0");
        assert_eq!(page_for_line(&s, 6), Some(1), "page 1's own top resolves to page 1");
        assert_eq!(page_for_line(&s, 7), Some(0), "in overlap, not a page top: prefer earlier page");
        assert_eq!(page_for_line(&s, 8), Some(1), "past overlap: page 1");
        assert_eq!(page_for_line(&s, 9), Some(1));
    }

    #[test]
    fn fingerprint_is_stable_and_input_sensitive() {
        let p = FingerprintParts {
            font_family: "Charter".into(), font_size: 17,
            ascent: 16, descent: 5, char_width: 9,
            width: 1920, height: 1200, line_spacing: 6, text_margins: 24,
            columns: 2, top_spacer_height: 64, view_height: 1098,
        };
        let a = fingerprint_string(&p);
        assert_eq!(a, fingerprint_string(&p), "must be deterministic");
        assert!(a.starts_with("v5|"), "schema-versioned: {a}");
        let mut q = FingerprintParts { font_size: 18, ..p.clone() };
        assert_ne!(a, fingerprint_string(&q));
        q = FingerprintParts { descent: 6, font_size: 17, ..q };
        assert_ne!(a, fingerprint_string(&q));
        // top_spacer_height is fingerprinted: changing it must invalidate.
        let taller = FingerprintParts { top_spacer_height: 80, ..p.clone() };
        assert_ne!(a, fingerprint_string(&taller), "spacer height must affect fp");
    }

    /// Regression (2026-07-27): the view height is what the fit check validates
    /// against, so a table baked at a DIFFERENT view height must not keep
    /// matching. It previously did — the fingerprint carried only the WINDOW
    /// size — and Ant-Arkangel page 89 rendered a 1102px column into a 1071px
    /// usable height, clipping its last line with no clip box to mask it
    /// (`paged_bottom_clip` returns 0 on overflow).
    #[test]
    fn view_height_change_invalidates_the_fingerprint() {
        let p = FingerprintParts {
            font_family: "Charter".into(), font_size: 16,
            ascent: 20, descent: 5, char_width: 9,
            width: 1920, height: 1200, line_spacing: 6, text_margins: 40,
            columns: 2, top_spacer_height: 74, view_height: 1098,
        };
        // Same WINDOW size, different TEXT VIEW height — the exact shape that
        // slipped through before.
        let shorter = FingerprintParts { view_height: 1071, ..p.clone() };
        assert_ne!(
            fingerprint_string(&p),
            fingerprint_string(&shorter),
            "a different view height must invalidate the stored table"
        );
    }
}
