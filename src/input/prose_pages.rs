//! Pure prose page-table types + invariant suite (GTK-free, unit-testable).
//! A prose page boundary is (buffer_line, row_offset_px); offsets are pixel
//! offsets from the buffer line's top, snapped to visual-row tops by the
//! GTK-bound generator (snapping itself is not re-checkable here).
//! `end` is EXCLUSIVE and must equal the next page's `start` exactly —
//! zero gaps, zero overlaps: the machine-checked no-text-loss guarantee.

use crate::input::page_top::PageTop;

/// Bound on main-loop iterations spun while waiting for a pending re-layout.
/// Generation runs synchronously on the main loop, so an unbounded spin would
/// hang the app if some source stays permanently ready.
const MAX_LAYOUT_SPINS: usize = 256;

/// DIAGNOSTIC: dump every tag applied across a buffer line, plus its text and
/// its measured height, so the SAME line can be compared at generation and at
/// render. `LIT_TRACE_TAGS=<first>:<last>` selects the window; `phase` names
/// the moment ("GEN" / "RENDER"). Tag names are collected per toggle point, so
/// a tag covering only part of the line is still listed, with its span.
///
/// Two back-to-back generation sweeps agree exactly (PAGES_PROSE_SWEEP), and
/// wrap width is identical at both moments, so any height divergence must come
/// from the line's CONTENT or its TAGS. This prints both.
pub fn trace_line_tags(
    buffer: &impl gtk4::prelude::IsA<gtk4::TextBuffer>,
    text_view: &impl gtk4::prelude::IsA<gtk4::TextView>,
    phase: &str,
) {
    use gtk4::prelude::{Cast, TextBufferExt, TextTagExt, TextViewExt, WidgetExt};
    let buffer = buffer.upcast_ref::<gtk4::TextBuffer>();
    let text_view = text_view.upcast_ref::<gtk4::TextView>();
    if !crate::logging::debug_mode() {
        return;
    }
    let Some((a, b)) = std::env::var("LIT_TRACE_TAGS").ok().and_then(|v| {
        let (x, y) = v.split_once(':')?;
        Some((x.parse::<i32>().ok()?, y.parse::<i32>().ok()?))
    }) else {
        return;
    };
    let total = buffer.line_count();
    for line in a..=b.min(total.saturating_sub(1)) {
        let Some(start) = buffer.iter_at_line(line) else {
            continue;
        };
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let text = buffer.text(&start, &end, false);
        let height = text_view.line_yrange(&start).1;
        // Walk the toggle points so partial-line tags are captured with spans.
        let mut spans: Vec<String> = Vec::new();
        let mut it = start;
        loop {
            for tag in it.toggled_tags(true) {
                let name = tag
                    .name()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "<anon>".to_string());
                spans.push(format!("{}@{}", name, it.line_offset()));
            }
            if !it.forward_to_tag_toggle(None::<&gtk4::TextTag>)
                || it.offset() >= end.offset()
            {
                break;
            }
        }
        // Row geometry straight from Pango: the layout the view will actually
        // render. `rows` is the wrapped-row count; `pw` the layout's own pixel
        // width. If `rows` differs between phases at equal wrap width, the
        // FONT/metrics changed, not the geometry.
        let (rows, pw, ph) = {
            let layout = text_view.create_pango_layout(Some(text.as_str()));
            let (w, h) = layout.pixel_size();
            (layout.line_count(), w, h)
        };
        crate::log_fmt!(
            "TAGTRACE[{}]: line={} h={} rows={} pw={} ph={} chars={} \
             above={} below={} inwrap={} font={:?} tags=[{}] text={:?}",
            phase,
            line,
            height,
            rows,
            pw,
            ph,
            text.chars().count(),
            text_view.pixels_above_lines(),
            text_view.pixels_below_lines(),
            text_view.pixels_inside_wrap(),
            text_view
                .pango_context()
                .font_description()
                .map(|d| d.to_str().to_string()),
            spans.join(" "),
            text.as_str()
        );
    }
}

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
    // pv7: prose generation now measures AFTER the buffer-wide `font-size`
    // TextTag is in effect (see `record_prose_pages`). EVERY table stored at
    // pv5 or pv6 was generated from heights for the pre-tag, smaller face —
    // each 4+ row paragraph charged ~29px short — so those tables pin pages
    // that render 44-127px taller than the card. The generation fix alone
    // cannot repair them: the fingerprint is a function of geometry+font, and
    // none of that changed, so a stale bad table stays a "hit" forever. The
    // bump is what forces regeneration. (pv6 is skipped rather than reused:
    // it belongs to the reverted line-box fix e1b17ac0, and its tables carry
    // the same bad heights.)
    // pv8: generation now measures TRUE wrapped heights, forcing GTK layout
    // validation (`validate_all_lines_by_scrolling`) before the sweep instead
    // of trusting `line_yrange` for lines far from the viewport. EVERY table
    // stored at pv7 or earlier was built from a sweep that under-measured
    // never-displayed lines by whole rows, so those tables pin pages that
    // render 13-114px taller than the card. NOTE: the pv7 bump alone did NOT
    // fix this — regeneration re-rolled the same race (the same fingerprint
    // produced 801, 806, and 808 pages across separate runs), which is why
    // the determinism test, not the bump, is what proves this fixed.
    format!("{base}|uh{usable}|cw{cw}|pv8")
}

/// Diagnostic: re-measure every recorded page against the SAME heights vector
/// the validator used, and report any page whose occupied height exceeds the
/// card. Emits `PAGES_PROSE_DRIFT:` lines.
///
/// This exists because a page can pass `validate_prose_pages` at generation
/// time and still overflow at RENDER time (`CLIP_WARN ... prose-1col OVERFLOW`,
/// clip floored to 0, last line poking out unmasked). The two sides compute
/// algebraically identical sums — `page_px` and
/// `scroll::exact_page_content_height` both charge `[start_line ..= end-1]`
/// minus `start_off` when `end_off == 0` — so a disagreement can ONLY mean the
/// per-line HEIGHTS differed between the two moments, never the arithmetic.
/// That makes this the decisive probe: a page reported here as fitting, which
/// the reader later logs as overflowing, proves the generation-time heights
/// were stale (GTK validates TextView line layout lazily and CACHES the coarse
/// single-row estimate it returns for a far-off line, so the pre-validation
/// sweep in `record_prose_pages` can seed wrong values that the per-page walk
/// then reads back). Conversely a page flagged here never should have been
/// stored, and the bug is in the boundary walk.
///
/// Cheap (one pass over the pages, no GTK calls — `heights` is already built)
/// and debug-gated, so it can stay in permanently as a tripwire.
fn log_generation_height_drift(
    pages: &[ProsePage],
    heights: &[i32],
    usable: i32,
    fit_slack: i32,
) {
    if !crate::logging::debug_mode() {
        return;
    }
    let mut over = 0usize;
    let mut worst = (0usize, 0i64);
    for (i, p) in pages.iter().enumerate() {
        // The render side's charge for this page: whole lines
        // `[start_line ..= last_rendered_line]`, less the part of the first
        // line scrolled off the top, less the unrendered tail of a
        // mid-paragraph last line. Mirrors `exact_page_content_height` +
        // `prose_bottom_head_for` (which yields `None` when `end_off == 0`).
        let last = last_rendered_line(p);
        let end = last.min(heights.len().saturating_sub(1));
        if p.start_line > end {
            continue;
        }
        let mut px: i64 = heights[p.start_line..=end].iter().map(|&h| h as i64).sum();
        px -= p.start_off as i64;
        if p.end_off > 0 {
            // Last line straddles: only its first `end_off` px are on-page.
            px -= (heights[end] - p.end_off).max(0) as i64;
        }
        if px > usable as i64 {
            over += 1;
            if px - usable as i64 > worst.1 - usable as i64 || worst.1 == 0 {
                worst = (i + 1, px);
            }
            // Diagnostic: which boundary shape produced the overshoot. The
            // page's own geometry says whether the extra pixels are a
            // mid-paragraph straddle (end_off > 0), a whole-paragraph end
            // (end_off == 0), or a top offset.
            crate::log_fmt!(
                "PAGES_PROSE_DRIFT: over page {} ({},{})..({},{}) px={} over={} \
                 lines={} last_h={}",
                i + 1, p.start_line, p.start_off, p.end_line, p.end_off,
                px, px - usable as i64,
                end.saturating_sub(p.start_line) + 1,
                heights.get(end).copied().unwrap_or(-1)
            );
            // Only the pages past the slack tolerance can actually floor the
            // render clip to 0; log those individually.
            if px > (usable + fit_slack) as i64 {
                crate::log_fmt!(
                    "PAGES_PROSE_DRIFT: page {} ({},{})..({},{}) px={} > usable={} (+slack {}) \
                     — stored anyway; render will log CLIP_WARN",
                    i + 1, p.start_line, p.start_off, p.end_line, p.end_off,
                    px, usable, fit_slack
                );
            }
        }
    }
    crate::log_fmt!(
        "PAGES_PROSE_DRIFT: summary pages={} over_usable={} worst=page {} at {}px usable={} slack={}",
        pages.len(), over, worst.0, worst.1, usable, fit_slack
    );
}

/// Scroll the view's vadjustment through the ENTIRE buffer in viewport-sized
/// steps, pumping the main context at each stop, so GTK actually validates
/// every line's layout before the caller sweeps `line_yrange`.
///
/// `line_yrange` does NOT validate-and-measure synchronously as this file
/// used to assume: GTK gates full layout validation on proximity to the
/// scrolled position, and a line the viewport has never visited returns a
/// PROVISIONAL estimate that GTK then caches as final. Two back-to-back
/// sweeps taken from the same unmoved viewport therefore agree with each
/// other while both are systematically short on far-off lines — which is
/// exactly what let a stored table overflow the card by up to 225px while
/// the convergence guard reported `delta_sum=0` (`ROOT-CAUSE.md`).
///
/// There is no synchronous "validate this range" call reachable from safe
/// Rust: `gtk4`/`sourceview5`'s `TextView`/`View` bind `line_yrange`,
/// `scroll_to_iter`, `scroll_mark_onscreen`, `move_mark_onscreen`,
/// `place_cursor_onscreen`, but not `gtk_text_view_validate_onscreen` /
/// `gtk_text_layout_validate` — checked directly against both crates'
/// generated bindings, neither is exposed. Driving the SAME vadjustment the
/// reader itself scrolls (`snap_scroll_to_line_offset` uses it too) is the
/// least-bad remaining lever.
///
/// Cost: `ceil(buffer_height / viewport_height)` stops, NOT one per page —
/// this function's callers' history includes a doc comment rejecting
/// per-PAGE scrolling as "O(pages) full re-layouts, far too slow on a
/// novel" (≈800 pages on BH). Stepping by a full viewport instead needs
/// roughly one stop per RENDERED page rather than one per stored page (on BH,
/// measured ≈700 stops, ≈700ms) — cheaper than per-page scrolling mainly
/// because it skips the boundary-walk bookkeeping each page-stop would repeat,
/// not because the stop COUNT differs much on a book whose pages are close to
/// one viewport tall each. See the caller's generation-time log
/// (`PAGES_PROSE_VALIDATE`) for the measured wall-clock on the work actually
/// being generated.
fn validate_all_lines_by_scrolling(state: &mut crate::app::AppState, line_count: usize) {
    use gtk4::prelude::{AdjustmentExt, TextBufferExt, TextViewExt};
    let adj = state.scrolled_window.vadjustment();
    let saved_value = adj.value();
    let page_size = adj.page_size();
    if page_size <= 0.0 || line_count == 0 {
        // No allocated viewport yet (e.g. a unit/headless context with a
        // zero-height widget) — nothing to validate by scrolling; the sweep
        // below will read whatever GTK already has, same as before this fix.
        return;
    }
    // The buffer's total pixel extent: the bottom of the LAST line, which is
    // also `adjustment.upper()` once the view has laid out that far — but we
    // cannot assume it has yet (that is the whole problem), so read it off
    // the end iter and step from there rather than trusting `adj.upper()`
    // up front.
    let last_line = line_count - 1;
    let buffer_bottom = state
        .buffer
        .iter_at_line(last_line as i32)
        .map(|it| {
            let (y, h) = state.text_view.line_yrange(&it);
            (y + h) as f64
        })
        .unwrap_or(0.0);
    let started = std::time::Instant::now();
    let mut pos = 0.0f64;
    let mut spins_total = 0usize;
    let mut stops = 0usize;
    loop {
        stops += 1;
        adj.set_value(pos);
        // Pump the main context so GTK actually re-lays-out around the new
        // scroll position before we move on — a bare `set_value` only queues
        // the work.
        let mut spins = 0;
        while glib::MainContext::default().pending() && spins < MAX_LAYOUT_SPINS {
            glib::MainContext::default().iteration(false);
            spins += 1;
        }
        spins_total += spins;
        if pos >= buffer_bottom || pos >= adj.upper() {
            break;
        }
        pos += page_size;
    }
    crate::log_fmt!(
        "PAGES_PROSE_VALIDATE: scrolled through buffer_bottom={:.0} page_size={:.0} \
         stops={} spins={} elapsed_ms={}",
        buffer_bottom, page_size, stops, spins_total, started.elapsed().as_millis()
    );
    // Restore the viewport exactly where it was — this function must be
    // invisible to anything watching scroll position (the caller restores
    // `state.page_top` separately; this restores the underlying GTK
    // adjustment those page-top scrolls drive).
    adj.set_value(saved_value);
    let mut spins = 0;
    while glib::MainContext::default().pending() && spins < MAX_LAYOUT_SPINS {
        glib::MainContext::default().iteration(false);
        spins += 1;
    }
}

/// Whether two per-line height vectors disagree beyond `tolerance_px` on any
/// line. Used to refuse storing a table built on heights that do not match an
/// independent measurement — a stronger check than the old two-sweep
/// comparison, which read the same cache twice and so always agreed.
pub(crate) fn heights_disagree(a: &[i32], b: &[i32], tolerance_px: i32) -> bool {
    a.len() != b.len()
        || a.iter().zip(b).any(|(x, y)| (x - y).abs() > tolerance_px)
}

/// Walk the LIVE engine's forward chain from (0,0), recording every page.
/// Boundaries come from `navigation::prose_next_boundary` — the same function
/// x/j use — so the stored grid IS the live grid.
///
/// **Pre-validation is load-bearing.** `prose_next_boundary` accumulates REAL
/// per-line heights via `line_yrange`. `line_yrange` does NOT validate a far-
/// off line synchronously — GTK gates full layout validation on proximity to
/// the viewport, and an unvisited line returns a provisional estimate GTK
/// then caches as final (see `validate_all_lines_by_scrolling`'s doc comment,
/// and `ROOT-CAUSE.md`, for the measured evidence). A walk that only mutated
/// `page_top_line` (no scroll) left every line past the initial validation
/// frontier reporting that coarse estimate — so the bounded walk under-
/// accumulated, `total - y0` never exceeded `usable`, `prose_next_boundary`
/// returned `None` mid-document, and the "final page" spanned the whole
/// un-walked remainder (observed: a 601941px page ~1/4 into BH). We fix that
/// by scrolling the viewport through the WHOLE buffer up front
/// (`validate_all_lines_by_scrolling`) so every line gets a real layout pass,
/// then sweeping `line_yrange` once to read the now-genuine heights, so the
/// subsequent walk reads real heights the whole way WITHOUT scrolling per
/// PAGE (which was O(pages) full re-layouts and far too slow on a novel). The
/// walk then just mutates the page state and restores it; it is synchronous
/// with no GTK main-loop iteration between steps, so no idle/render callback
/// observes the intermediate state.
pub fn record_prose_pages(
    state: &mut crate::app::AppState,
) -> Result<Vec<ProsePage>, String> {
    use gtk4::prelude::{TextBufferExt, TextViewExt};
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Err("no lines".into());
    }
    // FONT MUST BE IN EFFECT BEFORE THE SWEEP (2026-08-05).
    //
    // The body font is a buffer-wide `font-size` TextTag applied by
    // `reapply_font`, NOT the view's CSS font. Applying that tag invalidates
    // every line's layout, but GTK re-measures lazily: a `line_yrange` sweep
    // run before the view has processed the invalidation returns heights for
    // the PREVIOUS (smaller) face. Every 4+ row paragraph then comes back ~29px
    // short, the boundary walk over-fills each page, and the stored table
    // renders 44-127px taller than the card — `paged_bottom_clip` floors to 0
    // and the last line poked out unmasked (clip-prevention.md #12, prose).
    //
    // This is NOT the lazy-validation-frontier problem the doc comment above
    // describes (that one is about far-off lines never validated at all, and is
    // still handled by the sweep itself). Two back-to-back sweeps cannot detect
    // it — both read the same post-invalidation cache and agree exactly, which
    // is why `changed_between_sweeps=0` looked like proof and was not.
    //
    // Pumping the main loop lets GTK apply the pending re-layout, so the sweep
    // below measures the face the view will actually render.
    {
        use gtk4::prelude::WidgetExt;
        state.text_view.queue_resize();
        let mut spins = 0;
        while glib::MainContext::default().pending() && spins < MAX_LAYOUT_SPINS {
            glib::MainContext::default().iteration(false);
            spins += 1;
        }
    }
    // FORCE FULL VALIDATION BEFORE THE SWEEP (2026-08-05, Option B).
    //
    // `line_yrange` looked like a synchronous validate-and-measure call (the
    // doc comment below used to say so), but it is not: GTK gates full layout
    // validation on proximity to the viewport, and a line far from the
    // scrolled position returns a PROVISIONAL single-row-ish estimate that GTK
    // then caches as if it were final. A buffer-wide `line_yrange` sweep taken
    // from wherever the viewport happens to sit at generation time therefore
    // under-measures every line it has never displayed — by whole wrapped
    // rows. Two back-to-back sweeps agree with each other (both read the same
    // stale cache) while disagreeing with the real render by up to 225px
    // across a dozen lines (ROOT-CAUSE.md); the same fingerprint produced
    // 801/806/808 pages on three runs because generation depended on how much
    // of the buffer GTK happened to have validated.
    //
    // Task 1/1b measured whether an independent Pango layout could replace
    // this sweep (Option A) and found it disagrees with `line_yrange` even on
    // KNOWN-DISPLAYED lines, under-counting wrapped rows by a margin that
    // scales with row count, for a reason that resisted two separate
    // corrective hypotheses (dialogue-indent per-line margins; line-spacing
    // applied via tags) — both tested and refuted. So this is Option B: make
    // the premise TRUE instead. Scroll the vadjustment through the whole
    // buffer in viewport-sized steps, pumping the main context at each stop so
    // GTK actually validates every line's layout, before taking the sweep.
    // There is no synchronous "validate this range now" call reachable from
    // safe Rust — `gtk4`/`sourceview5` bind `line_yrange`, `scroll_to_iter`,
    // `scroll_mark_onscreen`, `move_mark_onscreen`, `place_cursor_onscreen`,
    // but not `gtk_text_view_validate_onscreen` / `gtk_text_layout_validate`
    // (checked directly against both crates' generated bindings; neither
    // exists) — so stepping the viewport is the least-bad lever available.
    //
    // This IS a full-buffer re-layout cost, and it is real: measured ≈700
    // stops / ≈700ms on BH (~7,300 lines), a ≈1.6x increase in total
    // generation wall-clock (task-2b-report.md has the before/after). It is
    // cheaper than the PER-PAGE scrolling this function's own doc comment
    // once rejected mainly because each stop here is a bare scroll+pump —
    // not because the stop COUNT is dramatically smaller on a book whose
    // pages are close to one viewport tall each. Generation is cached in
    // lit.db per (work, layout fingerprint) and only re-runs on a font/
    // geometry/pv-version change, not on every load, which is what makes a
    // one-time ~1.6x hit acceptable here; see the caller's generation-time
    // log (`PAGES_PROSE_VALIDATE`, `record_prose_pages_ms`) for the measured
    // cost on the work actually being generated.
    validate_all_lines_by_scrolling(state, line_count);
    // Force GTK to validate every line's true wrapped height once, so the
    // per-page boundary walk below never reads a lazy single-row estimate for a
    // far-off line (see the doc comment). The results are cached by GTK.
    let sweep1: Vec<i32> = (0..line_count)
        .map(|i| {
            state
                .buffer
                .iter_at_line(i as i32)
                .map(|it| state.text_view.line_yrange(&it).1)
                .unwrap_or(0)
        })
        .collect();
    // DIAGNOSTIC (LIT_TRACE_PANGO=1): compare EVERY line's `line_yrange` height
    // against an independent Pango layout at the view's real wrap width. Two
    // back-to-back `line_yrange` sweeps agreeing proves only that they read the
    // same cache — not that the cache is right. Pango is the ground truth the
    // view itself renders from, so a disagreement here is a line whose stored
    // height is wrong at generation, which is exactly what lets a page be
    // charged too little and overflow at render.
    if crate::logging::debug_mode() && std::env::var_os("LIT_TRACE_PANGO").is_some() {
        use gtk4::prelude::{TextTagExt, WidgetExt};
        let tv = &state.text_view;
        let wrap_w = tv.width() - tv.left_margin() - tv.right_margin();
        let above = tv.pixels_above_lines();
        let below = tv.pixels_below_lines();
        let mut disagree = 0usize;
        let mut delta_sum = 0i64;
        let mut worst: (usize, i32, i32) = (0, 0, 0);
        let mut examples: Vec<String> = Vec::new();
        // Per-line (pango_h, yrange_h) pairs, kept so the DISPLAYED/OFFSCREEN
        // split below can classify by position without re-measuring anything.
        let mut pairs: Vec<(i32, i32)> = Vec::with_capacity(line_count);
        // TASK 1b (2026-08-05-prose-page-height-truth): the view-level wrap_w
        // above is BLIND to per-line TextTag margins (dialogue-indent,
        // block-blockquote-indent, verse-indent-*), which is a candidate
        // explanation for Task 1's whole-row misses on multi-row lines. This
        // second, CORRECTED pass resolves the effective per-line left/right
        // margin the same way GTK does: walk every tag applied at the line's
        // start iter, and for each of left-margin/right-margin independently
        // take the value from the highest-PRIORITY tag that has it explicitly
        // set (`is_left_margin_set`/`is_right_margin_set`) — GTK resolves
        // competing tag properties by priority, and tag-table insertion order
        // is the default priority (later-added wins), so reading `.priority()`
        // directly (rather than assuming insertion order) is exact regardless
        // of any future re-ordering.
        let mut corrected_disagree = 0usize;
        let mut corrected_delta_sum = 0i64;
        let mut corrected_examples: Vec<String> = Vec::new();
        // Per-line (corrected_pango_h, yrange_h, corrected_wrap_w) kept for the
        // displayed/offscreen split below.
        let mut corrected_pairs: Vec<(i32, i32, i32)> = Vec::with_capacity(line_count);
        for i in 0..line_count {
            let Some(start) = state.buffer.iter_at_line(i as i32) else {
                pairs.push((0, 0));
                corrected_pairs.push((0, 0, wrap_w));
                continue;
            };
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            let text = state.buffer.text(&start, &end, false);
            let layout = tv.create_pango_layout(Some(text.as_str()));
            layout.set_width(wrap_w * pango::SCALE);
            layout.set_wrap(pango::WrapMode::WordChar);
            // The body font comes from the buffer-wide `font-size` TextTag, NOT
            // the view's context (which still reports the CSS default). Without
            // this the probe measures the wrong face and every line disagrees.
            layout.set_font_description(Some(&pango::FontDescription::from_string(
                &crate::ui::font_string(
                    state.config.font_family.as_str(),
                    state.config.font_size as i32,
                ),
            )));
            // The view charges each line box as ink + the paragraph spacing.
            let pango_h = layout.pixel_size().1 + above + below;
            let yr = sweep1[i];
            pairs.push((pango_h, yr));
            if pango_h != yr {
                disagree += 1;
                delta_sum += (pango_h - yr) as i64;
                if (pango_h - yr).abs() > (worst.1 - worst.2).abs() {
                    worst = (i, pango_h, yr);
                }
                if examples.len() < 8 {
                    examples.push(format!("L{i}:yr={yr}/pango={pango_h}"));
                }
            }

            // Resolve the effective per-line left/right margin from applied
            // tags, highest priority wins per property (independently).
            let mut best_left: Option<(i32, i32)> = None; // (priority, value)
            let mut best_right: Option<(i32, i32)> = None;
            for tag in start.tags() {
                let prio = tag.priority();
                if tag.is_left_margin_set() {
                    let v = tag.left_margin();
                    if best_left.is_none_or(|(p, _)| prio > p) {
                        best_left = Some((prio, v));
                    }
                }
                if tag.is_right_margin_set() {
                    let v = tag.right_margin();
                    if best_right.is_none_or(|(p, _)| prio > p) {
                        best_right = Some((prio, v));
                    }
                }
            }
            // Tag left/right margins are PARAGRAPH-absolute (they replace the
            // view's own margin, they do not add to it) — same semantics
            // `set_left_margin`/`set_right_margin` document for the view
            // itself. Fall back to the view-level margin when no tag sets it.
            let eff_left = best_left.map(|(_, v)| v).unwrap_or_else(|| tv.left_margin());
            let eff_right = best_right.map(|(_, v)| v).unwrap_or_else(|| tv.right_margin());
            let corrected_wrap_w = (tv.width() - eff_left - eff_right).max(0);

            let clayout = tv.create_pango_layout(Some(text.as_str()));
            clayout.set_width(corrected_wrap_w * pango::SCALE);
            clayout.set_wrap(pango::WrapMode::WordChar);
            clayout.set_font_description(Some(&pango::FontDescription::from_string(
                &crate::ui::font_string(
                    state.config.font_family.as_str(),
                    state.config.font_size as i32,
                ),
            )));
            let corrected_pango_h = clayout.pixel_size().1 + above + below;
            corrected_pairs.push((corrected_pango_h, yr, corrected_wrap_w));
            if corrected_pango_h != yr {
                corrected_disagree += 1;
                corrected_delta_sum += (corrected_pango_h - yr) as i64;
                if corrected_examples.len() < 8 {
                    corrected_examples.push(format!(
                        "L{i}:yr={yr}/cpango={corrected_pango_h}/cw={corrected_wrap_w}"
                    ));
                }
            }
        }
        crate::log_fmt!(
            "PAGES_PROSE_PANGO: lines={} disagree={} delta_sum={} \
             worst=line {} pango={} yrange={} wrap_w={} above={} below={} ex=[{}]",
            line_count, disagree, delta_sum,
            worst.0, worst.1, worst.2, wrap_w, above, below,
            examples.join(" ")
        );
        crate::log_fmt!(
            "PAGES_PROSE_PANGO_CORRECTED: lines={} disagree={} delta_sum={} \
             base_wrap_w={} ex=[{}]",
            line_count, corrected_disagree, corrected_delta_sum, wrap_w,
            corrected_examples.join(" ")
        );
        // DIAGNOSTIC (LIT_TRACE_PANGO=1), split by display state (Task 1,
        // 2026-08-05-prose-page-height-truth plan).
        //
        // "Displayed" = lines GTK has actually laid out and painted on screen
        // before this generation ran, as opposed to lines `line_yrange` has
        // only ever estimated. Generation fires once per settled layout/resize
        // with the viewport anchored at `state.page_top.line()` for the whole
        // sweep (confirmed in TRACE-FINDINGS.md: page_top stayed constant
        // through generation) — so the only lines with a real on-screen paint
        // behind them are the single page's worth starting at page_top. We
        // derive that count the same way the rest of this file computes a
        // page's usable height (widget_height - descender_guard -
        // SINGLE_COLUMN_BOTTOM_MARGIN), then walk forward from page_top
        // summing `sweep1` (the SAME heights generation used — not a fresh
        // GTK call, which would just re-ask the lazily-validated cache and
        // beg the question of what "displayed" means) until the budget is
        // exhausted. That walk can itself be one row optimistic about the
        // very last line if that line's own height was under-measured, so we
        // exclude a one-line margin band on each side of the displayed
        // window's boundary from BOTH populations — a line that was only
        // partially scrolled through, or whose displayed-ness hinges on the
        // number we're trying to validate, is excluded rather than forced
        // into a bucket.
        let page_top = state.page_top.line().min(line_count.saturating_sub(1));
        let descender_guard =
            crate::input::viewport::descender_guard_px(&state.text_view, page_top);
        let usable_height =
            tv.height() - descender_guard - crate::input::scroll::SINGLE_COLUMN_BOTTOM_MARGIN;
        let mut displayed_end = page_top; // exclusive upper bound of the displayed window
        if usable_height > 0 {
            let mut total = 0i32;
            for (i, &(_pango_h, yr)) in pairs.iter().enumerate().skip(page_top) {
                if total + yr > usable_height {
                    break;
                }
                total += yr;
                displayed_end = i + 1;
            }
        }
        const MARGIN: usize = 1;
        let displayed_hi = displayed_end.saturating_sub(MARGIN);
        let offscreen_lo = displayed_end + MARGIN;

        let mut displayed_lines = 0usize;
        let mut displayed_disagree = 0usize;
        let mut displayed_delta_sum = 0i64;
        let mut displayed_ex: Vec<String> = Vec::new();
        let mut offscreen_lines = 0usize;
        let mut offscreen_disagree = 0usize;
        let mut offscreen_delta_sum = 0i64;
        let mut offscreen_ex: Vec<String> = Vec::new();
        for (i, &(pango_h, yr)) in pairs.iter().enumerate() {
            if i >= page_top && i < displayed_hi {
                displayed_lines += 1;
                if pango_h != yr {
                    displayed_disagree += 1;
                    displayed_delta_sum += (pango_h - yr) as i64;
                    if displayed_ex.len() < 8 {
                        displayed_ex.push(format!("L{i}:yr={yr}/pango={pango_h}"));
                    }
                }
            } else if i >= offscreen_lo {
                offscreen_lines += 1;
                if pango_h != yr {
                    offscreen_disagree += 1;
                    offscreen_delta_sum += (pango_h - yr) as i64;
                    if offscreen_ex.len() < 8 {
                        offscreen_ex.push(format!("L{i}:yr={yr}/pango={pango_h}"));
                    }
                }
            }
            // else: inside the excluded margin band around the displayed
            // window's boundary, or before page_top (never reached this run
            // — page_top is generation's starting scroll position) — skip.
        }
        crate::log_fmt!(
            "PAGES_PROSE_PANGO_SPLIT: page_top={} displayed_end={} usable_height={} \
             displayed_lines={} displayed_disagree={} displayed_delta_sum={} \
             offscreen_lines={} offscreen_disagree={} offscreen_delta_sum={} \
             displayed_ex=[{}] offscreen_ex=[{}]",
            page_top, displayed_end, usable_height,
            displayed_lines, displayed_disagree, displayed_delta_sum,
            offscreen_lines, offscreen_disagree, offscreen_delta_sum,
            displayed_ex.join(" "), offscreen_ex.join(" ")
        );

        // TASK 1b: same displayed/offscreen split, but against the
        // per-line-margin-CORRECTED Pango measurement instead of the raw
        // view-width one. Reuses the identical `page_top`/`displayed_hi`/
        // `offscreen_lo` window computed above (from the SAME sweep1 heights),
        // so the two splits are directly comparable line-for-line.
        let mut c_displayed_lines = 0usize;
        let mut c_displayed_disagree = 0usize;
        let mut c_displayed_delta_sum = 0i64;
        let mut c_displayed_ex: Vec<String> = Vec::new();
        let mut c_offscreen_lines = 0usize;
        let mut c_offscreen_disagree = 0usize;
        let mut c_offscreen_delta_sum = 0i64;
        for (i, &(cpango_h, yr, cw)) in corrected_pairs.iter().enumerate() {
            if i >= page_top && i < displayed_hi {
                c_displayed_lines += 1;
                if cpango_h != yr {
                    c_displayed_disagree += 1;
                    c_displayed_delta_sum += (cpango_h - yr) as i64;
                    if c_displayed_ex.len() < 8 {
                        c_displayed_ex.push(format!(
                            "L{i}:yr={yr}/cpango={cpango_h}/cw={cw}"
                        ));
                    }
                }
            } else if i >= offscreen_lo {
                c_offscreen_lines += 1;
                if cpango_h != yr {
                    c_offscreen_disagree += 1;
                    c_offscreen_delta_sum += (cpango_h - yr) as i64;
                }
            }
        }
        crate::log_fmt!(
            "PAGES_PROSE_PANGO_CORRECTED_SPLIT: page_top={} displayed_end={} \
             c_displayed_lines={} c_displayed_disagree={} c_displayed_delta_sum={} \
             c_offscreen_lines={} c_offscreen_disagree={} c_offscreen_delta_sum={} \
             c_displayed_ex=[{}]",
            page_top, displayed_end,
            c_displayed_lines, c_displayed_disagree, c_displayed_delta_sum,
            c_offscreen_lines, c_offscreen_disagree, c_offscreen_delta_sum,
            c_displayed_ex.join(" ")
        );
    }
    // CONVERGENCE GUARD (always on, not just debug builds).
    //
    // Re-run `validate_all_lines_by_scrolling` a SECOND time — an
    // independently-driven full layout pass, not a second read of the same
    // cache — and sweep again. Compare `sweep1` against `sweep2` with
    // `heights_disagree`.
    //
    // This is a real independent check, unlike the guard this replaces: the
    // old version swept `line_yrange` TWICE with NO validation between the
    // two reads, so both hit the identical cache and it reported
    // `changed_between_sweeps=0` / `delta_sum=0` on the very run whose real
    // render disagreed with the stored table by up to 225px across a dozen
    // lines (ROOT-CAUSE.md) — self-referential, and unable to see the bug it
    // existed to catch.
    //
    // A tempting cheaper alternative — sweep once BEFORE validation, once
    // after, and compare those — was tried FIRST and rejected: measured live
    // (BH-Barrett ch37), it disagreed on 1283/7300 lines by construction,
    // because the pre-validation sweep is exactly the lazy provisional
    // estimate validation exists to correct. That comparison cannot
    // distinguish "validation worked" from "validation is still broken" —
    // both look like disagreement — so it is not a guard, it is a guaranteed
    // false failure on every cold generation. Two INDEPENDENTLY-VALIDATED
    // sweeps do not have that problem: if the scroll-and-settle pass is
    // doing its job, driving it twice from scratch must land on the same
    // heights both times, and a difference means validation itself did not
    // converge (e.g. spun out on `MAX_LAYOUT_SPINS` before settling).
    //
    // Why not compare against Pango instead (the other independent
    // measurement already in this file)? Task 1/1b measured it directly:
    // Pango disagreed with `line_yrange` on 13/13 KNOWN-DISPLAYED lines
    // across 3 chapters, under-counting wrapped rows by a margin that SCALES
    // with row count (-1 row at 10 rows, -4 rows at 28), for a reason that
    // resisted two separate corrective hypotheses (dialogue-indent per-line
    // margins; line-spacing applied via tags — both tested and refuted, see
    // the progress ledger). A guard built on that comparison would fire on
    // every multi-row paragraph in a HEALTHY table — it would cry wolf
    // constantly and get disabled, which is worse than no guard at all.
    //
    // Cost: one more `validate_all_lines_by_scrolling` pass, ≈ the same
    // ~700-900ms as the first (see `PAGES_PROSE_VALIDATE` in the log, emitted
    // twice per generation now). Generation is cached per (work,
    // fingerprint) and only re-runs on a font/geometry/pv-version change, so
    // paying for a genuinely independent check once per cache-miss is
    // worthwhile: a table that fails this guard would otherwise be pinned in
    // lit.db and served on every load until the next regeneration.
    validate_all_lines_by_scrolling(state, line_count);
    let sweep2: Vec<i32> = (0..line_count)
        .map(|i| {
            state
                .buffer
                .iter_at_line(i as i32)
                .map(|it| state.text_view.line_yrange(&it).1)
                .unwrap_or(0)
        })
        .collect();
    {
        let tolerance_px = 0;
        let disagree = heights_disagree(&sweep1, &sweep2, tolerance_px);
        let mut changed = 0usize;
        let mut first_change = None;
        let mut worst_change: Option<(usize, i32, i32)> = None;
        let mut worst_abs = 0i64;
        let mut delta_sum = 0i64;
        for i in 0..line_count {
            let before = sweep1[i];
            let after = sweep2[i];
            if before != after {
                changed += 1;
                let delta = (after - before) as i64;
                delta_sum += delta;
                if first_change.is_none() {
                    first_change = Some((i, before, after));
                }
                if delta.abs() > worst_abs {
                    worst_abs = delta.abs();
                    worst_change = Some((i, before, after));
                }
            }
        }
        use gtk4::prelude::WidgetExt;
        crate::log_fmt!(
            "PAGES_PROSE_SWEEP: lines={} changed_between_sweeps={} delta_sum={} first={:?} \
             tv_width={} left_margin={} right_margin={} wrap_w={}",
            line_count, changed, delta_sum, first_change,
            state.text_view.width(),
            state.text_view.left_margin(),
            state.text_view.right_margin(),
            state.text_view.width() - state.text_view.left_margin() - state.text_view.right_margin()
        );
        // Dump the generation-time heights for one window, to diff against
        // RENDER_HEIGHTS for the same lines. `LIT_TRACE_HEIGHTS=<first>:<last>`.
        if let Some((a, b)) = std::env::var("LIT_TRACE_HEIGHTS").ok().and_then(|v| {
            let (x, y) = v.split_once(':')?;
            Some((x.parse::<usize>().ok()?, y.parse::<usize>().ok()?))
        }) {
            let b = b.min(line_count.saturating_sub(1));
            if a <= b {
                crate::log_fmt!(
                    "GEN_HEIGHTS: [{}..={}] = {:?}",
                    a, b, &sweep1[a..=b]
                );
            }
        }
        // Refuse rather than store a table built on heights that do not
        // reproduce across two independently-driven validation passes. The
        // caller logs VALIDATE_FAIL and falls back to the live engine, which
        // measures at render time and cannot disagree with itself.
        if disagree {
            let (worst_i, worst_before, worst_after) =
                worst_change.unwrap_or((0, 0, 0));
            crate::log_fmt!(
                "VALIDATE_FAIL: {changed}/{line_count} line heights disagree between \
                 two independently-validated sweeps beyond {tolerance_px}px \
                 (delta_sum={delta_sum}px, worst=line {worst_i} {worst_before}px -> \
                 {worst_after}px, first={first_change:?})"
            );
            return Err(format!(
                "layout not settled: {changed}/{line_count} line heights changed \
                 between two independently-validated sweeps (delta_sum={delta_sum}px, \
                 worst=line {worst_i} {worst_before}px -> {worst_after}px). \
                 Generating now would pin an over-packed table; deferring to the \
                 live engine."
            ));
        }
    }
    // Drive the walk through the real page state, then restore it.
    let saved = state.page_top;
    state.page_top = PageTop::at_line_start(0);
    let mut pages: Vec<ProsePage> = Vec::new();
    let mut guard = 0usize;
    loop {
        let start = (state.page_top.line(), state.page_top.offset());
        match crate::input::navigation::prose_next_boundary(state) {
            Some((nl, no)) => {
                pages.push(ProsePage {
                    start_line: start.0, start_off: start.1,
                    end_line: nl, end_off: no,
                });
                state.page_top = PageTop::new(nl, no);
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
            state.page_top = saved;
            return Err("determinism: forward chain did not terminate".into());
        }
    }
    state.page_top = saved;
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
    let fp = prose_layout_fingerprint(state);

    // Tell the user why the window is about to stop responding.
    //
    // `record_prose_pages` walks and measures EVERY line, and on a long work
    // that blocks the main loop for a long time — LoJ (21,520 rows) measured
    // 36.7s. The window cannot repaint and keys are declined (see
    // keymap::handle_key's re-entrancy guard) for the whole of it, which is
    // indistinguishable from a hang; it was reported as "linux-lit is frozen"
    // on 2026-08-08.
    //
    // The toast must be PAINTED before we block, so pump the loop until it is
    // on screen. `record_prose_pages` pumps too, but only after it has already
    // begun the work this message is meant to announce.
    let toast_gen = crate::input::navigation::show_chapter_toast_hold(
        state,
        "Paginating\u{2026}",
    );
    {
        let mut spins = 0;
        while glib::MainContext::default().pending() && spins < MAX_LAYOUT_SPINS {
            glib::MainContext::default().iteration(false);
            spins += 1;
        }
    }
    // The body below has several early returns (validate/citation/store
    // failures). Wrapping it keeps the release on ONE path, so a future edit
    // that adds another `return` cannot strand a held toast on screen.
    generate_and_store_prose_inner(state, fp);
    crate::input::navigation::release_chapter_toast_hold(state, toast_gen);
}

/// Body of `generate_and_store_prose`, split out so the caller owns the
/// "Paginating…" toast's lifetime. Returns early on any failure; the caller
/// releases the toast regardless.
fn generate_and_store_prose_inner(state: &mut crate::app::AppState, fp: String) {
    use gtk4::prelude::{TextBufferExt, TextViewExt, WidgetExt};
    let gen_started = std::time::Instant::now();
    let pages = match record_prose_pages(state) {
        Ok(p) => p,
        Err(e) => {
            crate::logging::log(&format!(
                "PAGES_PROSE: VALIDATE_FAIL {e} (record_prose_pages took {}ms)",
                gen_started.elapsed().as_millis()
            ));
            return;
        }
    };
    let record_elapsed_ms = gen_started.elapsed().as_millis();
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
    // DIAGNOSTIC: the SAME window as GEN_HEIGHTS but measured AFTER the boundary
    // walk — this is the vector the validator actually uses. If it differs from
    // GEN_HEIGHTS the heights moved DURING the walk, not via lazy validation.
    if crate::logging::debug_mode() {
        if let Some((a, b)) = std::env::var("LIT_TRACE_HEIGHTS").ok().and_then(|v| {
            let (x, y) = v.split_once(':')?;
            Some((x.parse::<usize>().ok()?, y.parse::<usize>().ok()?))
        }) {
            let b = b.min(line_count.saturating_sub(1));
            if a <= b {
                crate::log_fmt!("POSTWALK_HEIGHTS: [{}..={}] = {:?}", a, b, &heights[a..=b]);
            }
        }
        trace_line_tags(&state.buffer, &state.text_view, "GEN");
    }
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
    log_generation_height_drift(&pages, &heights, usable, ctx.fit_slack);
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
        "PAGES_PROSE: generated {} pages for {} fp={} record_prose_pages_ms={} total_ms={}",
        rows.len(), work.abbrev, fp, record_elapsed_ms, gen_started.elapsed().as_millis()));
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
    let i = prose_page_for_position(&table, state.page_top.line(), state.page_top.offset())?;
    let p = &table[i];
    (p.start_line == state.page_top.line() && p.start_off == state.page_top.offset())
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
    let i = prose_page_for_position(&table, state.page_top.line(), state.page_top.offset())?;
    let p = &table[i];
    if p.start_line != state.page_top.line() || p.start_off != state.page_top.offset() {
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
    if prose_page_for_position(&table, state.page_top.line(), state.page_top.offset())
        .map(|i| (table[i].start_line, table[i].start_off)
            == (state.page_top.line(), state.page_top.offset()))
        .unwrap_or(false)
    {
        return; // already on the grid
    }
    let Some(i) = prose_page_for_line(&table, state.current_line) else { return };
    let (t, o) = (table[i].start_line, table[i].start_off);
    crate::logging::log(&format!(
        "PAGES_PROSE: resnap off-grid ({},{}) -> ({},{}) (cursor {})",
        state.page_top.line(), state.page_top.offset(), t, o, state.current_line
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

    /// The canonical landing for a line is the STORED PAGE containing it —
    /// never the line itself, and never a geometric guess like `line - 1`.
    ///
    /// Real BH-Barrett case (2026-07-27), reproduced twice with identical
    /// numbers. Cursor line 47 lives on the page that starts at `(42, 603)`.
    /// Two separate sites computed a landing geometrically instead of reading
    /// this table and both produced an off-grid top:
    ///
    ///   - `display_work` recomputed `page_top = current_line - 1` = 46,
    ///     so startup opened off-grid and `resnap_prose_to_table` had to
    ///     correct it (once 23.6s later — the reader showed the WRONG page
    ///     until the late paint landed).
    ///   - `jump_to_line` (journal picker -> Escape source-jump) routed
    ///     through the prose-table-unaware `canonical_page_top_for` and landed
    ///     on 47, the cursor line, dropping the reader out of table mode
    ///     (`BOTTOM_CLIP_ROWFILL page_top=47` instead of
    ///     `BOTTOM_CLIP_EXACT page_top=42 top_off=603`).
    ///
    /// Both now read this rule via `canonical_page_top_offset_for`. The
    /// `start_off` half is the part a bare-`usize` helper structurally cannot
    /// carry: returning 42 alone still mis-frames the page by 603px.
    #[test]
    fn canonical_landing_is_the_stored_page_not_the_line() {
        // Page 8 of the real table: (42,603) covers the paragraph that runs
        // through line 47; the next page opens at (48, 0).
        let pages = vec![
            ProsePage { start_line: 36, start_off: 0,   end_line: 42, end_off: 603 },
            ProsePage { start_line: 42, start_off: 603, end_line: 48, end_off: 0 },
            ProsePage { start_line: 48, start_off: 0,   end_line: 54, end_off: 120 },
        ];

        // The cursor line resolves to the page CONTAINING it...
        let i = prose_page_for_line(&pages, 47).expect("line 47 is covered");
        assert_eq!(
            (pages[i].start_line, pages[i].start_off),
            (42, 603),
            "line 47 must land on the stored page (42,603)"
        );

        // ...not on the line itself (the jump_to_line bug), and not on the
        // `current_line - 1` guess (the display_work bug).
        assert_ne!(pages[i].start_line, 47, "must not land on the cursor line");
        assert_ne!(pages[i].start_line, 46, "must not land on current_line - 1");

        // The off-grid guesses both resolve INTO this same page, which is why
        // resnap could paper over them — and why the fix belongs at the source.
        assert_eq!(prose_page_for_position(&pages, 46, 0), Some(1));
        assert_eq!(prose_page_for_position(&pages, 47, 0), Some(1));

        // The stored top is already canonical: a landing there is a no-op, so
        // the corrected path must not trigger a resnap at all.
        assert_eq!(prose_page_for_position(&pages, 42, 603), Some(1));
    }

    /// A JUMP landing (concordance, echo, vocab, cross-work) must resolve to
    /// the stored page, not to a centring guess. `update_highlight_and_center`
    /// used `current_line - lines_per_page/2` and
    /// `display_work_at_with_prepared` used the target line itself as the page
    /// top; both ignore the grid and both land off it (2026-07-27 audit).
    ///
    /// This pins the RULE the fixed landings share: whatever the guess would
    /// have been, the answer is the page whose interval contains the target's
    /// first row — and it carries a row offset that a bare line cannot express.
    #[test]
    fn jump_landing_prefers_stored_page_over_centering_guess() {
        // Same real BH-Barrett shape as the test above: the middle page is
        // offset-started, which is what the guesses cannot reproduce.
        let pages = vec![
            ProsePage { start_line: 36, start_off: 0,   end_line: 42, end_off: 603 },
            ProsePage { start_line: 42, start_off: 603, end_line: 48, end_off: 0 },
            ProsePage { start_line: 48, start_off: 0,   end_line: 54, end_off: 120 },
        ];
        let target = 47;

        let i = prose_page_for_line(&pages, target).expect("target is covered");
        let (top, off) = (pages[i].start_line, pages[i].start_off);
        assert_eq!((top, off), (42, 603));

        // The centring guess (`update_highlight_and_center`): with a ~12-line
        // page it lands 41 — inside the right page, but NOT at its boundary,
        // so the reader renders a window the pagination never chose.
        let lpp = 12;
        let centering_guess = target.saturating_sub(lpp / 2);
        assert_ne!(centering_guess, top, "centring must not be the landing");
        // It is not even on the same page as the cursor here (41 < 42), which
        // is how a centred jump could show the PREVIOUS page's tail.
        assert_eq!(prose_page_for_position(&pages, centering_guess, 0), Some(0));

        // The cross-work guess (`display_work_at_with_prepared`): the target
        // line as its own page top. Lands mid-page, off-grid.
        assert_ne!(target, top, "the target line is not a page boundary");

        // Only the stored pair is canonical. The offset is load-bearing:
        // dropping it leaves the page 603px short of where it belongs.
        assert_eq!(prose_page_for_position(&pages, top, off), Some(i));
        assert_ne!(off, 0, "this page's offset is what a bare usize loses");
    }
}

#[cfg(test)]
mod height_agreement_tests {
    use super::heights_disagree;

    #[test]
    fn identical_vectors_agree() {
        assert!(!heights_disagree(&[40, 68, 96], &[40, 68, 96], 2));
    }

    #[test]
    fn a_whole_row_of_drift_is_a_disagreement() {
        // +28 is one wrapped row — the exact signature of the lazy-validation
        // bug this guard exists to catch.
        assert!(heights_disagree(&[40, 68, 96], &[40, 96, 96], 2));
    }

    #[test]
    fn sub_tolerance_jitter_is_accepted() {
        assert!(!heights_disagree(&[40, 68], &[41, 69], 2));
    }

    #[test]
    fn a_length_mismatch_is_a_disagreement() {
        assert!(heights_disagree(&[40, 68], &[40], 2));
    }
}
