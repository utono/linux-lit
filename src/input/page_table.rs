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
    };
    fingerprint_string(&parts)
}

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
        generated_at: epoch_timestamp(),
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
}
