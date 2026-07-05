use gtk4::prelude::*;

use crate::app::AppState;
use crate::log_fmt;

// ---------------------------------------------------------------------------
// Re-exports — keep `navigation::` paths working for all 11+ external callers
// ---------------------------------------------------------------------------

// viewport.rs
pub use super::viewport::invalidate_page_tops;
pub use super::viewport::is_line_on_screen;
pub(crate) use super::viewport::VisibleRange;

// scroll.rs
pub use super::scroll::resnap_page;
pub use super::scroll::refresh_bottom_clip;
pub use super::scroll::scroll_paragraph_to_top;
pub(crate) use super::scroll::PageTurnLock;
pub(crate) use super::scroll::snap_scroll_to_line;

// highlight.rs — pub items
pub use super::highlight::{
    update_highlight_only, update_highlight_and_ensure_visible,
    update_highlight_and_advance_page, update_highlight_and_show,
    update_highlight_and_center,
};

// ---------------------------------------------------------------------------
// Internal imports from sibling modules
// ---------------------------------------------------------------------------

use super::viewport::{
    last_fully_visible_line, next_page_top, prev_page_top, NextPage,
    back_up_for_speaker_state, page_turn_top_state,
    scene_header_top_state,
    is_dialogue_line,
    next_dialogue_line, prev_dialogue_line, buffer_line_text,
    next_dialogue_from, is_line_fully_visible, lines_per_page,
    clamp_page_top_to_scroll_ceiling, column_split, would_empty_right_column,
};
use super::scroll::{
    set_page, set_page_instant, set_page_instant_offset, scroll_to_cursor, center_cursor,
    scroll_after_jump_forward, scroll_after_jump_backward,
    PageDirection,
};
use super::highlight::{
    update_highlight, auto_show_vocab_popup,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Seconds to seek before a line's start_time when navigating.
/// Provides audio context so playback doesn't start at a hard cut.
pub const SEEK_PREROLL: f64 = 0.2;

/// The seek target for a line whose audio begins at `start`: back up by
/// `SEEK_PREROLL` for context, clamped at 0 so we never seek negative. The
/// single idiom every nav/search/concordance/echo seek repeats.
/// NOTE: distinct from the A-B-loop `CHUNK_PREROLL`/`TURN_PREROLL` computations,
/// which back up by their own constants.
pub fn preroll_seek_time(start: f64) -> f64 {
    (start - SEEK_PREROLL).max(0.0)
}

/// Seconds to highlight a line before playback actually reaches it.
/// Used by the MPV client's time-pos sync.
pub const SYNC_PREROLL: f64 = 0.0;

/// Minimum silent gap (seconds) between a line's end_time and the next
/// line's start_time required to trigger an early jump to the next line
/// during MPV playback sync. Gaps at or below this keep normal timing.
pub const SYNC_GAP_THRESHOLD: f64 = 1.5;

/// Seconds before the next line's start_time to jump the highlight when
/// crossing a gap that exceeds SYNC_GAP_THRESHOLD. Anchored on the next
/// line's start (not the previous line's end) so the lead stays correct
/// even when the previous line's end_time overshoots the actual speech.
pub const SYNC_GAP_PREROLL: f64 = 1.5;

/// Brief window during which playback-sync is suppressed while MPV processes a
/// seek, so the highlight doesn't fight the in-flight seek. Used at every
/// manual-seek site (search, timestamp set, gamepad, echo/concordance jumps).
pub const SYNC_SUPPRESS_SEEK: std::time::Duration = std::time::Duration::from_millis(500);

/// "Suppress sync indefinitely" sentinel (24h) — set when there is no active
/// timestamp to sync against, cleared by the next real sync event.
pub const SYNC_SUPPRESS_INDEFINITE: std::time::Duration = std::time::Duration::from_secs(86400);

// ---------------------------------------------------------------------------
// PageChangeReason
// ---------------------------------------------------------------------------

/// Why the viewport's page changed. Drives which consumers fire inside
/// `after_page_change`. Mirrors the `reason` field on foliate-js's `relocate`
/// CustomEvent (paginator.js:952-969).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageChangeReason {
    /// User pressed page-forward (x, Ctrl+d, Space).
    Forward,
    /// User pressed page-backward (y, Shift+,).
    Backward,
    /// User jumped to a specific line (gg, G, jump-to-bookmark via picker).
    JumpToLine,
    /// User toggled a bookmark and we're refreshing the cursor on it.
    JumpToBookmark,
    /// User jumped to a chapter via [ ] keys.
    Chapter,
    /// User jumped to a scene via 2 / 3 keys (plays).
    Scene,
    /// User jumped to a vocab match.
    Vocab,
    /// User pressed comma/q/j/k for dialogue navigation.
    Dialogue,
    /// Cursor-only movement with no audio seek. Currently unused: j/k now seek
    /// like comma/q (Dialogue). Kept for a future no-seek cursor verb.
    #[allow(dead_code)]
    Cursor,
    /// MPV CursorSync drove the cursor to a new line; do NOT re-seek MPV.
    MpvSync,
    /// Layout refresh after font/size/translation change. Not a navigation.
    Resnap,
    /// Work just loaded; AppState is being initialized. Skip most consumers.
    WorkLoad,
}

impl PageChangeReason {
    /// Whether to call `seek_to_current_line` after the page change. False for
    /// MPV-driven changes (would loop) and pure layout refreshes.
    pub(crate) fn should_seek(self) -> bool {
        !matches!(self, Self::MpvSync | Self::Resnap | Self::WorkLoad | Self::Cursor)
    }

    /// Whether to call `auto_show_vocab_popup` after the page change. False
    /// for system-driven changes that the user didn't request.
    pub(crate) fn should_show_vocab(self) -> bool {
        !matches!(self, Self::MpvSync | Self::Resnap | Self::WorkLoad)
    }
}

/// Single rendezvous called at the tail of every page-mutating function.
/// Mirrors the listener pattern around foliate-js's `relocate` CustomEvent
/// (paginator.js:952-969): one canonical "page changed" signal that all
/// consumers (page label, vocab popup, MPV seek) project from in a
/// deterministic order.
///
/// Each consumer consults the reason flags so the function shape is
/// the same for every caller — the differences are in the reason, not in
/// scattered if/else around the call sites.
pub(crate) fn after_page_change(state: &mut AppState, reason: PageChangeReason) {
    // F4: invalidate cache unconditionally; snap_scroll_to_line repopulates
    // if any scroll happened. For Cursor / Dialogue navigations that don't
    // page-turn, the next is_line_fully_visible call falls back to recompute
    // — slightly slower but always correct.
    state.last_visible_range.set(None);

    // Highlight always repaints — consumer order matters: highlight first so
    // downstream consumers (vocab popup positioning) see the new cursor.
    update_highlight(state);

    if reason.should_seek() {
        seek_to_current_line(state);
    }

    if reason.should_show_vocab() {
        auto_show_vocab_popup(state);
    }
}

// ---------------------------------------------------------------------------
// Cursor verbs
// ---------------------------------------------------------------------------

/// Move cursor by `delta` lines (j/k).
/// Going down: page turn when cursor reaches the last visible line.
/// Going up: smooth scroll to keep cursor visible.
/// Jump to the first line.
pub fn jump_to_start(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let line_count = state.effective_line_count();
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let target = (0..line_count)
        .find(|&i| is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup))
        .unwrap_or(0);

    state.current_line = target;
    state.page_back_stack.clear();
    state.page_back_stack.push((state.page_top_line, state.page_top_offset));
    set_page_instant(state, 0);
    after_page_change(state, PageChangeReason::JumpToLine);
}

/// Jump to the last line.
pub fn jump_to_end(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

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

    // Prose grid: land on the STORED final page (offset-aware — it can start
    // mid-paragraph), and put the cursor on the document's last line. Mirrors
    // the play-table branch above; keeps G on the same grid x/j/sync use so the
    // landing is never off the rendered final page.
    if let Some(table) = crate::input::prose_pages::active_prose_page_table(state) {
        let p = *table.last().expect("validated tables are non-empty");
        state.page_back_stack.clear();
        crate::input::scroll::set_page_instant_offset(state, p.start_line, p.start_off);
        state.current_line = line_count - 1;
        after_page_change(state, PageChangeReason::JumpToLine);
        log_fmt!("PAGES_PROSE: page {}/{} (G) top=({},{})",
            table.len(), table.len(), p.start_line, p.start_off);
        return;
    }

    // Find the last dialogue line in the buffer (skips trailing stage
    // directions, blanks, exit markers). For prose works there typically
    // isn't a difference; for plays this lands on the last spoken line.
    // Scope stage_lookup here so it's dropped before we mutate state below.
    let target = {
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        let mut t = line_count - 1;
        loop {
            if !state.translation_lines.get(t).copied().unwrap_or(false)
                && is_dialogue_line(&state.buffer, t, state.is_prose(), &stage_lookup)
            {
                break;
            }
            if t == 0 {
                break;
            }
            t -= 1;
        }
        t
    };

    // Anchor on the canonical final spread (the page the user reaches by paging
    // forward to the end). When the work's tail is a short section that starts
    // with a section-break marker (e.g. a lone EPILOGUE), `last_page_top` keeps
    // the last FULL two-column spread, whose right column clamps BEFORE that
    // marker — so the absolute last dialogue line (`target`) may sit on the
    // following, intentionally-suppressed spread and NOT be visible here. Land
    // the page first, then place the cursor on the last dialogue line that is
    // actually on this spread (mirrors the forward final-spread guard) so the
    // highlight is never off-page.
    let new_top = last_page_top(state);
    // Clear the back-stack WITHOUT seeding it with the came-from page: a `y` from
    // the final spread must tile back to the page immediately BEFORE `new_top`,
    // not return to wherever the jump originated. With an empty stack,
    // `page_backward` uses `prev_page_top(new_top)`, which now lands on the
    // nearest real boundary below the (forward-pulled) final-spread top.
    state.page_back_stack.clear();
    set_page_instant(state, new_top);

    // Redefine stage_lookup after state mutations so there is no overlapping borrow.
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };

    if state.one_section_per_page() {
        // The last page is the final sonnet; land the cursor on its first verse
        // line (the first dialogue at/after the section heading), matching the
        // gg/x landing for every other sonnet.
        let first = next_dialogue_from(&state.buffer, new_top, line_count, state.is_prose(), &stage_lookup)
            .min(line_count.saturating_sub(1));
        state.current_line = first;
        after_page_change(state, PageChangeReason::JumpToLine);
        return;
    }

    let cs = column_split(state, new_top);
    let on_page = prev_dialogue_line(&state.buffer, &state.translation_lines, cs.page_end + 1, state.is_prose(), &stage_lookup)
        .filter(|&d| d >= new_top && d <= cs.page_end)
        .unwrap_or(target.min(cs.page_end));
    state.current_line = on_page;

    after_page_change(state, PageChangeReason::JumpToLine);
}

/// The page top of the CANONICAL spread that contains `target` — the same spread
/// the reader reaches by paging FORWARD through the work, so `target` sits where
/// natural pagination places it (not force-top-aligned). Walks the forward page
/// chain (`next_page_top`) from the section/chapter header above `target` until
/// the spread whose span `[page_top, page_end]` (or `next_page_top` boundary in
/// single-column) contains `target`. Idempotent: re-running from any earlier
/// boundary yields the same top, so a jump here agrees with what `x`/`y` produce.
///
/// Used by bookmark and structural jumps so they land on the canonical spread
/// rather than a top-aligned page that disagrees with the natural pagination.
pub(crate) fn canonical_page_top_for(state: &AppState, target: usize) -> usize {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return 0;
    }
    if let Some(table) = crate::input::page_table::active_page_table(state) {
        if let Some(i) = crate::input::page_table::page_for_line(&table, target.min(line_count - 1)) {
            return table[i].left_start;
        }
    }
    let target = target.min(line_count - 1);
    // Start from a real page boundary at or before the target: the SECTION/SCENE
    // header above it. The forward walk via next_page_top is idempotent ONLY from
    // a genuine page boundary — so we must NOT start from a per-speaker back-up,
    // which lands on the *speaker* line just above the target when there's no
    // section break immediately above (e.g. a match mid-scene). That speaker line
    // sits mid-page, so the walk's first spread "contains" the target and returns
    // a too-late top (the bug: match line 4043 -> speaker 4042 instead of the
    // real page boundary 4032). scene_header_top backs all the way to the scene's
    // section-break line, a true anchor that the forward chain passes through.
    let mut top = scene_header_top_state(state, target).min(target);
    let two_col = state.column_count() == 2;
    let mut guard = 0;
    loop {
        // Does this spread already contain the target?
        let page_end = if two_col {
            column_split(state, top).page_end
        } else {
            last_fully_visible_line(state, top)
        };
        if target <= page_end {
            return top;
        }
        let next = next_page_top(state, top).new_top;
        if next <= top || next >= line_count {
            // Forward chain exhausted before reaching the target (shouldn't happen
            // for an in-bounds target, but stay safe): keep the last top.
            return top;
        }
        top = next;
        guard += 1;
        if guard > line_count {
            return top;
        }
    }
}

/// Two-column forward-nav guard: when a tentative page top `candidate` falls in
/// the work's FINAL spread region, return the canonical final spread
/// (`last_page_top`) instead so the tail content fills BOTH columns rather than
/// landing as a lonely left column with an empty right.
///
/// Returns `Some(anchor)` when `candidate` should be redirected, `None` when it
/// is fine as-is. Callers in single-column mode should not call this (it assumes
/// two-column geometry); it returns `None` defensively if columns != 2.
///
/// A candidate is "in the final region" when it isn't already the anchor AND
/// either:
///   (a) turning to it would leave the right column EMPTY (a lone EPILOGUE), or
///   (b) its page span overlaps the anchor's tail — its forward boundary reaches
///       at or past where the anchor's page begins, so the two pages cover the
///       same trailing content (the UNDERFILLED-final-spread case where
///       `next_page_top` picked a boundary one short of the canonical one).
///
/// This unifies the three forward-nav redirect sites that previously each
/// re-derived the final-region test with slightly different arithmetic:
/// `page_forward` (x), `scroll_after_jump_forward` (q/j), and
/// `update_highlight_and_advance_page` (playback sync).
pub(crate) fn redirect_to_final_spread(state: &AppState, candidate: usize) -> Option<usize> {
    if state.column_count() != 2 {
        return None;
    }
    let anchor = last_page_top(state);
    if candidate == anchor {
        return None;
    }
    let empty_right = super::viewport::would_empty_right_column(state, candidate);
    // `next_page_top` is the candidate's forward boundary — where its right column
    // ends. If that reaches at/past the anchor's top while the candidate sits
    // before the anchor, the candidate's page and the anchor's page cover the same
    // tail, so the candidate is an underfilled stand-in for the canonical spread.
    let cand_end = super::viewport::column_split(state, candidate).next_page_top;
    let overlaps_anchor = candidate < anchor && cand_end > anchor;
    if empty_right || overlaps_anchor {
        Some(anchor)
    } else {
        None
    }
}

/// The page top of the work's CANONICAL FINAL spread — sized so the tail content
/// fills both columns (two-column) rather than landing as a lonely left column
/// with an empty right. Used by `jump_to_end` and by the forward-nav guard that
/// redirects an "empty right column" turn to this anchor.
///
/// Depends ONLY on layout + `line_count` — NOT on any target line or the current
/// scroll position — so it is idempotent: `G` lands on the same spread no matter
/// where it starts, and recomputing from that spread returns it unchanged. (It
/// took a `target` argument historically; that made the walk's start point
/// `target`-relative and broke idempotency — see the two-column branch comment.)
pub(crate) fn last_page_top(state: &AppState) -> usize {
    let line_count = state.effective_line_count();
    let widget_height = state.text_view.height();
    let columns = state.column_count() as i32;
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    if columns == 2 && widget_height > 0 && line_count > 0 {
        // Two-column: land on the CANONICAL last spread — the same page the user
        // reaches by paging forward to the end, so both columns are filled and
        // the work's tail sits in the right column. Walk the forward page chain
        // (next_page_top) from the work's START until the spread whose forward
        // boundary reaches the end of the work; that spread's top is the anchor.
        //
        // Start at line 0 (always a page boundary), NOT a `target`-relative
        // offset. The forward walk's result must depend ONLY on layout +
        // line_count — never on `target` or the current scroll position — or
        // `last_page_top` is not idempotent and `G` disagrees with itself. An
        // earlier `target.saturating_sub(lpp*3)` "safe early start" broke exactly
        // this: `lpp` comes from `lines_per_page` measured at the CURRENT
        // `page_top`, which is content-dependent (a region of tall lines reports
        // a tiny lpp). For H8, jump_to_end's `target=4321` with a degenerate
        // `lpp=6` gave start=4303, which snapped the loop directly ONTO the
        // lone-EPILOGUE spread (4303) — PAST the `top=4271` pull point that
        // corrects it to the full-right-column spread (4282). So G landed on 4303
        // but recomputing from page_top=4303 (or from any earlier start) yielded
        // 4282. Walking from 0 always traverses 4271 and hits the pull. The walk
        // is O(page_count) but last_page_top is not hot.
        let mut top = 0usize;
        // Advance forward, but STOP at the last spread whose right column is
        // non-empty. The very last page of a work with a short trailing section
        // (a lone EPILOGUE) has the tail ALONE in its left column and an empty
        // right column — pressing G must NOT land there (it would move the
        // EPILOGUE out of the right column into a lonely left column). The
        // canonical final spread is the one just before that: the tail fills the
        // RIGHT column, both columns full (this is also the page the saved-
        // position startup restores and the page `x` stops on). Keep the last
        // `top` whose own right column is non-empty.
        let mut last_full = if would_empty_right_column(state, top) { None } else { Some(top) };
        let mut guard = 0;
        loop {
            let next = super::viewport::next_page_top(state, top).new_top;
            let we_next = if next < line_count { would_empty_right_column(state, next) } else { true };
            if next >= line_count || next <= top {
                break; // reached the end of the forward chain
            }
            // Advancing a full page to `next` would land on a page with an empty
            // right column. There are TWO reasons a page empties its right column,
            // and they need opposite handling:
            //
            //  (a) The work's TRUE END — a short trailing section (a lone EPILOGUE)
            //      whose tail sits alone. Here the natural page boundary SKIPS a
            //      better final spread: a top a few lines past `top` whose left
            //      column holds the dialogue tail and whose right column absorbs
            //      the trailing section, reaching the work's end. At 1112px the
            //      walk gives top=4296 (page_end=4336, EPILOGUE cut off) while
            //      top=4303 reaches page_end=4347 (full EPILOGUE in the right
            //      column). We pull forward to that spread and STOP — it is the end.
            //
            //  (b) A MID-WORK SCENE-OPENING boundary — in a play, a scene whose
            //      header + stage direction + entrance fills the spread, pushing
            //      the first spoken line off the page (`next_page_top`'s documented
            //      H8 case). This ALSO empties the right column, but it is NOT the
            //      end: thousands of lines of dialogue remain below. Breaking here
            //      strands G hundreds of pages early (H8 landed ~1701 of ~4324).
            //      We must keep walking PAST this boundary.
            //
            // The discriminator is whether any dialogue remains at/below `next`.
            // If none remains, we are at the tail (case a): pull forward and break.
            // If dialogue remains, this is a mid-work scene break (case b): fall
            // through and advance `top = next` to keep walking. (`next` itself has
            // an empty right column, so it does NOT update `last_full` — the next
            // full-right-column spread the walk reaches will.)
            // A dialogue TAIL (case a with spoken lines, e.g. MND's epilogue) is
            // caught by the chain-end check below even though dialogue remains.
            let dialogue_at_or_below_next =
                (next..line_count).any(|i| is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup));
            // Second true-end signal: the forward chain ENDS at `next` — no page
            // exists past it. A dialogue tail (MND: the remainder of Robin's
            // spoken epilogue, plain 5.1 lines, no trailing section) makes
            // `dialogue_at_or_below_next` true even at the work's real end, which
            // used to misclassify it as case (b) and strand the anchor one spread
            // short (the tail was unreachable by x/G/startup). A mid-work
            // scene-opening boundary (case b, H8) always has further pages, so
            // its chain continues and this stays false. Short-circuit on
            // `we_next` so the extra layout walk only runs at empty-right
            // boundaries, not every page.
            let chain_ends_at_next = we_next && {
                let nn = super::viewport::next_page_top(state, next).new_top;
                nn >= line_count || nn <= next
            };
            if we_next && (!dialogue_at_or_below_next || chain_ends_at_next) {
                // Case (a): the true end. Search [top+1, next) for the SMALLEST top
                // whose spread leaves no dialogue below it and still has a non-empty
                // right column — that is the canonical last spread.
                let mut pulled = None;
                for t in (top + 1)..next.min(line_count) {
                    let tcs = super::viewport::column_split(state, t);
                    let dialogue_below = (tcs.next_page_top..line_count)
                        .any(|i| is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup));
                    if !dialogue_below && !would_empty_right_column(state, t) {
                        pulled = Some(t);
                        break;
                    }
                }
                if let Some(t) = pulled {
                    last_full = Some(t);
                }
                break;
            }
            top = next;
            if !would_empty_right_column(state, top) {
                last_full = Some(top);
            }
            guard += 1;
            if guard > line_count {
                break;
            }
        }
        let chosen = last_full.unwrap_or(top);
        // The work's tail may be too short to fill a full scroll viewport, so the
        // viewport CAN'T scroll down to `chosen` — `set_page_instant` would clamp
        // it to the scroll ceiling, leaving page_top and the rendered top
        // disagreeing (the bug where G's spread looked different from its computed
        // top, and j's highlight landed on an off-screen EPILOGUE line). Return
        // the clamped top so every downstream consumer (cursor placement, the
        // forward-nav anchor, page_backward) agrees with what's actually on
        // screen.
        clamp_page_top_to_scroll_ceiling(state, chosen)
    } else if state.one_section_per_page() && line_count > 0 {
        // One section per page (sonnet_sequence): the last page is the last
        // section — its top is the last section-start boundary at or before the
        // final line. Scanning the viewport-fill backward would pack several
        // trailing sonnets onto one page.
        let mut top = line_count - 1;
        while top > 0 && !state.is_section_start(top) {
            top -= 1;
        }
        top
    } else if widget_height > 0 && line_count > 0 {
        // Single column: accumulate `widget_height` of content backward.
        let capacity = widget_height;
        let mut total: i32 = 0;
        let mut top = line_count - 1;
        loop {
            let Some(iter) = state.buffer.iter_at_line(top as i32) else { break };
            let (_y, h) = state.text_view.line_yrange(&iter);
            if total + h > capacity && top != line_count - 1 {
                top += 1;
                break;
            }
            total += h;
            if top == 0 {
                break;
            }
            top -= 1;
        }
        top
    } else {
        // Layout not ready — fall back to lpp anchor (scaled by column count).
        let lpp = lines_per_page(state) * (columns as usize);
        line_count.saturating_sub(lpp)
    }
}

/// Toggle between one- and two-column e-reader layout (Alt+[). No-op in scroll
/// mode (two columns are e-reader-only). Flips the current work's effective
/// column count and stores it as a per-work override (keyed by `work.abbrev`),
/// persists it, shows/hides the right column, and recomputes the current page
/// so current_line stays visible.
pub fn toggle_column_layout(state: &mut AppState) {
    if !matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader) {
        crate::logging::log("COLUMNS: ignored (not e-reader mode)");
        return;
    }
    let Some(abbrev) = state.current_work.as_ref().map(|w| w.abbrev.clone()) else {
        return;
    };

    // Flip the work's currently-effective count (override or default) and store
    // it as a per-work override.
    let current = state.column_count();
    let new_count: u8 = if current >= 2 { 1 } else { 2 };
    state.config.column_overrides.insert(abbrev, new_count);
    crate::config::save(&state.config);

    let two = new_count == 2;
    state.right_scrolled_overlay.set_visible(two);
    state.column_divider.set_visible(two);
    if !two {
        state.right_bottom_clip.set_height_request(0);
    }

    // Scansion appends a line-type label per verse line in single column but
    // omits it in two columns (it would overflow the per-column width budget).
    // The column count just changed, so rebuild the buffer to add/drop those
    // labels before pages are recomputed below — otherwise the stale labels
    // either overflow (1→2) or vanish (2→1) until the next full reformat.
    // column_overrides was already updated above, so column_count() inside
    // rebuild_buffer_text now reflects new_count.
    if state.scansion.level != crate::scansion::ScanLevel::Off {
        crate::app::rebuild_buffer_text(state);
    }

    // Card width depends on column count (two columns fill more of the window),
    // so resize the card to match the new layout before recomputing pages.
    crate::app::layout::apply_card_sizing(
        &state.content_hbox,
        state.window.width(),
        state.config.column_width,
        new_count,
        state.translations_visible,
    );

    // Page boundaries depend on column_count(); the cached page-tops index is
    // stale after a toggle, so invalidate it before recomputing.
    invalidate_page_tops(state);

    let top = back_up_for_speaker_state(state, state.page_top_line);
    set_page_instant(state, top);
    if !is_line_on_screen(state, state.current_line) {
        let new_top = page_turn_top_state(state, state.current_line);
        set_page_instant(state, new_top);
    }
    after_page_change(state, PageChangeReason::JumpToLine);
    crate::logging::log(&format!("COLUMNS: now {} column(s)", new_count));
}


/// Page forward (Ctrl+d/f). The next page starts at the dialogue line
/// immediately after the last dialogue line visible on the current page,
/// backed up by one if preceded by a speaker name.
/// In two-column mode, if the first dialogue line of the RIGHT column is the
/// opening dialogue of a new scene (the nearest line above it is a scene/act
/// marker, with no earlier dialogue of that scene on the spread), return the
/// right column's start line so `x` can turn the page by moving that scene to
/// the top of the left column. Returns `None` otherwise (normal page turn).
fn scene_snap_top(state: &AppState, line_count: usize) -> Option<usize> {
    use crate::db::line_types;
    if state.column_count() != 2 {
        return None;
    }
    // Anthology works pack excerpts into BOTH columns (column_split advances the
    // split to the next excerpt for the right column). The play "snap the next
    // scene to the left column" turn would re-show the right-column excerpt as
    // the next spread's left column (duplicating it). Fall through to the normal
    // next_page_top turn, which advances a full spread.
    if state.is_anthology() {
        return None;
    }
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let cs = column_split(state, state.page_top_line);
    let split = cs.split;
    let page_end = cs.page_end;
    if split <= state.page_top_line || split >= line_count {
        return None;
    }
    // First dialogue line at/after the right column's start.
    let rc_first_dlg = next_dialogue_from(&state.buffer, split, line_count, state.is_prose(), &stage_lookup);
    if rc_first_dlg >= line_count {
        return None;
    }
    // Never snap a scene to the left column if doing so would empty the right
    // column (the scene is the work's short tail, e.g. a lone EPILOGUE) — the
    // tail should stay in the right column of the current spread.
    if would_empty_right_column(state, split) {
        return None;
    }
    // Walk back from the right column's first dialogue: if we reach a section
    // boundary before any other dialogue line, this is the scene's first line.
    // Authoritative boundary (DB div columns) when loaded; legacy text marker
    // check as the mid-load fallback.
    let has_bitmap = state.section_starts().is_some();
    let is_section = |idx: usize| -> bool {
        if has_bitmap {
            state.is_section_start(idx)
        } else {
            line_types::is_act_scene_marker(buffer_line_text(&state.buffer, idx).trim())
        }
    };
    let mut i = rc_first_dlg;
    while i > split {
        i -= 1;
        if is_section(i) {
            // Only snap when the boundary is genuinely INSIDE this spread's right
            // column ([split, page_end]). If it sits past `page_end` (i.e. the
            // right column shows the OLD scene's trailing exit chrome — blank /
            // `[They exit.]` / blank — and the new scene starts on the NEXT page),
            // the new scene already begins the next spread naturally via
            // `next_page_top`; snapping to `split` would re-show that exit chrome
            // at the top of the next page (the Jn 1596 `x` 3-line overlap of
            // blank/'[They exit.]'/blank). Let the normal path tile to the boundary.
            if i <= page_end {
                return Some(split);
            }
            return None;
        }
        if is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup) {
            return None;
        }
    }
    // No boundary between split and the first dialogue: also check the boundary
    // may sit exactly at `split` (right column opens on the marker line itself).
    if is_section(split) {
        Some(split)
    } else {
        None
    }
}

/// Forward sub-line step for an over-tall prose paragraph at `page_top_line`.
/// Returns `Some(new_offset)` (snapped to a real visual-row top) when the
/// paragraph still has rows below the current fold, or `None` when the paragraph
/// fits the viewport or is exhausted (caller does a normal line turn). GTK-bound;
/// the pure decision is `viewport::overtall_next_offset`.
fn overtall_forward_step(state: &mut AppState) -> Option<i32> {
    let top = state.page_top_line;
    let iter = state.buffer.iter_at_line(top as i32)?;
    let (y, para_h) = state.text_view.line_yrange(&iter);
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return None;
    }
    let descender_guard = crate::input::viewport::descender_guard_px(&state.text_view, top);
    let usable = widget_height - descender_guard - crate::input::scroll::BASE_BOTTOM_MARGIN;
    let cur_off = state.page_top_offset;
    // Pure decision: is there a viewport's worth of rows still below the fold?
    let raw = crate::input::viewport::overtall_next_offset(cur_off, para_h, usable)?;
    // Snap the raw target (y + raw) DOWN to a real visual-row top so the next
    // page never starts mid-glyph-row; convert back to an offset from the line top.
    let snapped_val = crate::input::scroll::snap_value_to_display_row(state, (y + raw) as f64);
    let new_off = (snapped_val - y as f64).round() as i32;
    // The snap must still ADVANCE past the current offset (a degenerate snap that
    // landed back at/below cur_off would stall the chain — fall through to a line
    // turn instead).
    if new_off > cur_off {
        Some(new_off)
    } else {
        None
    }
}

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
    let y0 = top_y + state.page_top_offset;
    // Bounded forward walk from `top` (a validated point close to the current
    // viewport) accumulating REAL per-line heights, rather than reading
    // `line_yrange` on the document's LAST buffer line: for a large document
    // most far-off lines are still un-validated by GTK, so their reported
    // height is a coarse single-row estimate, not the true wrapped height —
    // using that as a document-wide "total" corrupts the fill decision (a
    // severe underfill was observed: pages advancing by only 1-2 lines with
    // most of the viewport left blank). Walking from `top` keeps every
    // `line_yrange` call on lines GTK has just measured for real.
    let mut total = top_y; // pixel top of `top`; walk accumulates to its bottom and beyond
    for i in top..line_count {
        let Some(li) = state.buffer.iter_at_line(i as i32) else { break };
        let (ly, lh) = state.text_view.line_yrange(&li);
        total = ly + lh;
        // Stop once we've walked far enough past the current viewport's
        // bottom fold that we know for certain more content remains below
        // it — no need to walk the whole document to prove that.
        if total - y0 > usable {
            break;
        }
    }
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
    // A residual mid-paragraph offset on a NON-over-tall line is now a
    // legitimate landing: the design mandates full row-fill, so a page may
    // start (and end) partway down any paragraph. The single-column prose
    // bottom-clip path (`update_bottom_clip`, scroll.rs) is offset-aware — it
    // clips per visual row against the live scroll value regardless of
    // `range.count` — and `is_line_start_visible` / `last_fully_visible_line`
    // both account for `page_top_offset`, so no deferral is needed. (The old
    // "Fix 2" deferral arm rounded such a landing FORWARD to the next line's
    // top, which made the real page taller than the `usable` budget while the
    // next boundary was still computed as if the page had shown exactly
    // `usable` pixels — the hidden tail rows were skipped forever. Removed.)
    if (new_top, new_off) <= (top, state.page_top_offset) {
        return None;
    }
    Some((new_top, new_off))
}

pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    if state.page_turn_lock.is_locked() {
        return;
    }

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
        let landing = {
            let stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            let first_dlg = next_dialogue_from(&state.buffer, next.left_start,
                state.effective_line_count(), state.is_prose(), &stage_lookup);
            if cur + 2 == table.len() {
                // Landing ON the final page: last on-page dialogue (matches the
                // redirect_to_final_spread landing rule).
                prev_dialogue_line(&state.buffer, &state.translation_lines,
                        next.end + 1, state.is_prose(), &stage_lookup)
                    .filter(|&d| d >= next.left_start)
                    .unwrap_or(first_dlg.min(next.end))
            } else {
                first_dlg.min(next.end)
            }
        };
        state.page_back_stack.push((state.page_top_line, state.page_top_offset));
        state.current_line = landing;
        set_page(state, next.left_start, PageDirection::Forward);
        after_page_change(state, PageChangeReason::Forward);
        log_fmt!("PAGES: page {}/{} top={}", cur + 2, table.len(), next.left_start);
        return;
    }

    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };

    // Final-spread guard: when the current spread already shows the work's last
    // content in its right column, there is no next page — turning would just
    // empty the right column (a lone EPILOGUE as the left column). Instead move
    // the highlight to the work's last dialogue line and stay put.
    if state.column_count() == 2 {
        let cs = column_split(state, state.page_top_line);
        if cs.next_page_top >= line_count {
            // Cap at the last ACTUALLY-VISIBLE line, not column_split's page_end:
            // when the work's tail is too short to fill the viewport the scroll is
            // clamped, so page_end can name a line that's rendered off-screen
            // (below the clamp). Advancing the cursor there leaves NO visible
            // highlight. `last_fully_visible_line` reflects the clamped viewport.
            let visible_end = super::viewport::last_fully_visible_line(state, state.page_top_line)
                .min(cs.page_end);
            let last_dlg = prev_dialogue_line(&state.buffer, &state.translation_lines, visible_end + 1, state.is_prose(), &stage_lookup)
                .filter(|&d| d >= state.page_top_line)
                .unwrap_or(visible_end);
            if last_dlg > state.current_line {
                log_fmt!("PAGE_FWD: final spread — cursor {}->{} (no turn)",
                         state.current_line, last_dlg);
                state.current_line = last_dlg;
                after_page_change(state, PageChangeReason::Forward);
            } else {
                log_fmt!("PAGE_FWD: final spread, cursor already at last ({})", state.current_line);
            }
            return;
        }
    }

    // Scene-aware turn: if the right column opens a new scene, move that scene
    // to the top of the left column instead of paging by viewport height.
    if let Some(snap_top) = scene_snap_top(state, line_count) {
        // Near the document end the scene start may sit past the scroll ceiling,
        // so set_page would clamp it back below page_top_line — leaving the view
        // stuck on the same spread (page_top never advances, x oscillates).
        // Only take the scene-snap when it yields real forward progress.
        let clamped = clamp_page_top_to_scroll_ceiling(state, snap_top);
        if clamped > state.page_top_line {
            let next_dialogue = next_dialogue_from(&state.buffer, clamped, line_count, state.is_prose(), &stage_lookup);
            log_fmt!("PAGE_FWD: scene-snap page_top={} -> new_top={} next_dialogue={}",
                     state.page_top_line, clamped, next_dialogue);
            state.page_back_stack.push((state.page_top_line, state.page_top_offset));
            state.current_line = next_dialogue.min(line_count.saturating_sub(1));
            set_page(state, clamped, PageDirection::Forward);
            after_page_change(state, PageChangeReason::Forward);
            return;
        }
        log_fmt!("PAGE_FWD: scene-snap to {} skipped (clamps to {} <= page_top {})",
                 snap_top, clamped, state.page_top_line);
        // Fall through: the normal path / jump_to_end handles the final spread.
    }

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

    let NextPage { new_top, next_dialogue } = next_page_top(state, state.page_top_line);
    if next_dialogue >= line_count {
        log_fmt!("PAGE_FWD: at end, returning");
        return; // already at end
    }

    let candidate_top = if new_top <= state.page_top_line {
        next_dialogue
    } else {
        new_top
    };
    // If the turn lands in the work's FINAL spread region, redirect to the
    // canonical final spread so the tail fills both columns (covers the
    // lone-EPILOGUE empty-right case and the underfilled too-early final spread).
    // See `redirect_to_final_spread`; shared with q/j and playback sync.
    let (candidate_top, redirected_to_final) = match redirect_to_final_spread(state, candidate_top) {
        Some(anchor) => {
            log_fmt!("PAGE_FWD: final-region candidate={} -> anchor={}", candidate_top, anchor);
            (anchor, true)
        }
        None => (candidate_top, false),
    };
    let effective_top = clamp_page_top_to_scroll_ceiling(state, candidate_top);
    log_fmt!(
        "PAGE_FWD: page_top={} new_top={} next_dialogue={} candidate_top={} effective_top={} prose={}",
        state.page_top_line, new_top, next_dialogue, candidate_top, effective_top, state.is_prose()
    );
    if effective_top > state.page_top_line {
        // The final-spread redirect can pull the top FORWARD PAST `next_dialogue`
        // (which was computed for the natural, pre-redirect turn), stranding the
        // cursor above the page with no visible highlight. The anchor page is the
        // work's last spread — mirror jump_to_end and land the cursor on its last
        // on-page dialogue line. (Computed before the state mutations below so
        // `stage_lookup`'s borrow of `state` has ended.)
        let landing = if redirected_to_final {
            // Fresh closure (not the fn-scope `stage_lookup`) so its borrow of
            // `state` ends here, before the mutations below — mirrors jump_to_end.
            let anchor_stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            let cs = column_split(state, effective_top);
            prev_dialogue_line(&state.buffer, &state.translation_lines, cs.page_end + 1, state.is_prose(), &anchor_stage_lookup)
                .filter(|&d| d >= effective_top && d <= cs.page_end)
                .unwrap_or(next_dialogue.min(line_count.saturating_sub(1)))
        } else {
            next_dialogue
        };
        state.page_back_stack.push((state.page_top_line, state.page_top_offset));
        state.current_line = landing;
        set_page(state, effective_top, PageDirection::Forward);
        after_page_change(state, PageChangeReason::Forward);
        return;
    }

    log_fmt!("PAGE_FWD: ceiling hit, jumping to end");
    jump_to_end(state);
}

/// Page backward (Shift+,). Pop the previous page_top from the history
/// stack so we return to exactly the same page that page_forward came from.
/// When the history stack is empty (e.g. resumed mid-book, or user has paged
/// back through all history), compute a previous page by stepping one
/// viewport-height of lines up from the current page_top.
pub fn page_backward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    if state.page_turn_lock.is_locked() {
        return;
    }

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

    // Pinned prose table (single column): index arithmetic, but ONLY when the
    // current position sits EXACTLY on the grid. Off-grid (a resumed mid-page
    // scroll, or an over-tall step the table doesn't model) keeps the existing
    // back-stack/live behavior so the popped entry — which knows the precise
    // sub-page scroll — wins. On-grid, the table is authoritative.
    if let Some(table) = crate::input::prose_pages::active_prose_page_table(state) {
        if let Some(cur) = crate::input::prose_pages::prose_page_for_position(
            &table, state.page_top_line, state.page_top_offset)
        {
            let on_grid = table[cur].start_line == state.page_top_line
                && table[cur].start_off == state.page_top_offset;
            if on_grid {
                if cur == 0 {
                    let first = first_dialogue_line(state);
                    if first < state.current_line {
                        state.current_line = first;
                        after_page_change(state, PageChangeReason::Backward);
                    }
                    log_fmt!("PAGES_PROSE: page 1/{} (at start)", table.len());
                    return;
                }
                let prev = table[cur - 1];
                // Drop any back-stack entry that would return us to THIS page or
                // ahead (a stale forward-nav leftover) — the table is the source
                // of truth for the previous page here.
                while let Some(&(t, o)) = state.page_back_stack.last() {
                    if (t, o) >= (prev.start_line, prev.start_off) {
                        state.page_back_stack.pop();
                    } else {
                        break;
                    }
                }
                state.current_line = prev.start_line;
                crate::input::scroll::set_page_instant_offset(state, prev.start_line, prev.start_off);
                after_page_change(state, PageChangeReason::Backward);
                log_fmt!("PAGES_PROSE: page {}/{} top=({},{})",
                         cur, table.len(), prev.start_line, prev.start_off);
                return;
            }
        }
        // Off-grid: fall through to the live back-stack/prev-page path below.
    }

    // First-spread guard: when the current spread already shows the work's first
    // content in its left column (page top at 0), there is no previous page —
    // move the highlight to the work's first dialogue line and stay put. Mirrors
    // the final-spread guard in page_forward.
    if state.page_top_line == 0 {
        let first = first_dialogue_line(state);
        if first < state.current_line {
            log_fmt!("PAGE_BWD: first spread — cursor {}->{} (no turn)", state.current_line, first);
            state.current_line = first;
            after_page_change(state, PageChangeReason::Backward);
        } else {
            log_fmt!("PAGE_BWD: first spread, cursor already at first ({})", state.current_line);
        }
        return;
    }

    let line_count = state.effective_line_count();
    // Pop the most recent back-stack entry that is actually BEHIND us. A stale
    // entry at or ahead of the current page top (left over from forward nav that
    // didn't clear the stack) would make `y` jump FORWARD — never correct.
    // (Do this before defining stage_lookup to avoid overlapping borrows.)
    while let Some(&(top, off)) = state.page_back_stack.last() {
        // A stale entry is one at/ahead of the current position. Compare the line,
        // and for the SAME line compare the offset (a mid-paragraph entry behind
        // the current scroll within the same over-tall paragraph is NOT stale).
        let stale = top > state.page_top_line
            || (top == state.page_top_line && off >= state.page_top_offset);
        if !stale {
            break;
        }
        log_fmt!("PAGE_BWD: dropping stale stack entry ({},{}) (>= page_top {},{})",
                 top, off, state.page_top_line, state.page_top_offset);
        state.page_back_stack.pop();
    }
    // Over-tall restore: if the popped entry is the SAME buffer line we're on (a
    // step back WITHIN an over-tall paragraph), restore the scroll offset directly
    // without a line turn, and return early.
    if let Some(&(prev_top, prev_off)) = state.page_back_stack.last() {
        if prev_top == state.page_top_line && prev_off < state.page_top_offset {
            state.page_back_stack.pop();
            log_fmt!("PAGE_BWD: within-paragraph restore line={} offset {}->{}",
                     prev_top, state.page_top_offset, prev_off);
            set_page_instant_offset(state, prev_top, prev_off);
            // Cursor: keep it on the same paragraph line (over-tall = one line).
            after_page_change(state, PageChangeReason::Backward);
            return;
        }
    }
    let (new_top, next_dialogue) = if let Some((prev_top, _prev_off)) = state.page_back_stack.pop() {
        // Use a local stage_lookup scoped to this block so it's dropped before
        // the mutable use of state below.
        let nd = {
            let stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            next_dialogue_from(&state.buffer, prev_top, line_count, state.is_prose(), &stage_lookup)
        };
        log_fmt!("PAGE_BWD: stack pop new_top={} next_dialogue={} from page_top={}",
                 prev_top, nd, state.page_top_line);
        (prev_top, nd)
    } else {
        // `prev_page_top` returns the closest forward boundary that tiles into the
        // current page (no gap, no overlap). A short distance back is FINE — it
        // means the current top is a speaker line one row past the previous page's
        // end (forward `back_up_for_speaker`), so the previous page legitimately
        // ends one line before it. (An earlier "degenerate guard" stepped back an
        // extra page in that case, which CREATED a multi-line gap — removed.)
        let np = prev_page_top(state, state.page_top_line);
        log_fmt!("PAGE_BWD: prev_page_top new_top={} next_dialogue={} from page_top={}",
                 np.new_top, np.next_dialogue, state.page_top_line);
        (np.new_top, np.next_dialogue)
    };

    // A `y` page turn lands the cursor on the LAST dialogue line of the page it
    // turns to (bottom of the right column in two-column mode), not the first —
    // what a reader expects from paging backward. Compute it from the landed
    // page's geometry; fall back to the page's first dialogue if none is found.
    set_page(state, new_top, PageDirection::Backward);
    // Redefine stage_lookup after all mutable state operations above.
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let cursor = if state.one_section_per_page() {
        // One section per page (sonnet_sequence): paging back lands on the
        // sonnet's FIRST verse line, the same landing as gg/x — not the last.
        next_dialogue_from(&state.buffer, new_top, state.effective_line_count(), state.is_prose(), &stage_lookup)
            .min(state.effective_line_count().saturating_sub(1))
    } else if state.column_count() == 2 {
        let cs = super::viewport::column_split(state, new_top);
        prev_dialogue_line(&state.buffer, &state.translation_lines, cs.page_end + 1, state.is_prose(), &stage_lookup)
            .filter(|&d| d >= new_top && d <= cs.page_end)
            .unwrap_or(next_dialogue)
    } else {
        let last_vis = last_fully_visible_line(state, new_top);
        prev_dialogue_line(&state.buffer, &state.translation_lines, last_vis + 1, state.is_prose(), &stage_lookup)
            .filter(|&d| d >= new_top && d <= last_vis)
            .unwrap_or(next_dialogue)
    };
    state.current_line = cursor;
    log_fmt!("PAGE_BWD: cursor -> last-visible dialogue {} (new_top={})", cursor, new_top);
    after_page_change(state, PageChangeReason::Backward);
}

/// Move cursor to the last fully visible line on the current page (`Q` key).
pub fn cursor_to_page_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let last_vis = last_fully_visible_line(state, state.page_top_line);
    if state.current_line != last_vis {
        state.current_line = last_vis;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Go to previous page and place cursor on its last visible line (shift+comma).
pub fn page_backward_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    if state.page_turn_lock.is_locked() {
        return;
    }
    if state.page_top_line == 0 {
        log_fmt!("NAV_BACK_BOTTOM: at start of work");
        return;
    }

    let line_count = state.effective_line_count();
    let (prev_top, new_top) = if let Some((prev, _prev_off)) = state.page_back_stack.pop() {
        let nd = {
            let stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            next_dialogue_from(&state.buffer, prev, line_count, state.is_prose(), &stage_lookup)
        };
        let top = back_up_for_speaker_state(state, nd);
        log_fmt!("NAV_BACK_BOTTOM: stack pop prev={} new_top={} from page_top={}",
                 prev, top, state.page_top_line);
        (prev, top)
    } else {
        let np = prev_page_top(state, state.page_top_line);
        log_fmt!("NAV_BACK_BOTTOM: prev_page_top new_top={} from page_top={}",
                 np.new_top, state.page_top_line);
        let top = back_up_for_speaker_state(state, np.new_top);
        (np.new_top, top)
    };
    let _ = prev_top;
    set_page(state, new_top, PageDirection::Backward);
    let last_vis = last_fully_visible_line(state, state.page_top_line);
    log_fmt!("NAV_BACK: Shift+comma prev_top={} new_top={} current_line={}", prev_top, new_top, last_vis);
    state.current_line = last_vis;
    state.pending_advance = None;
    state.pending_advance_ignore_bl = None;
    after_page_change(state, PageChangeReason::Backward);
}

/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let target = {
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        prev_dialogue_line(&state.buffer, &state.translation_lines, state.current_line, state.is_prose(), &stage_lookup)
    };
    if let Some(target) = target {
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
        scroll_after_jump_backward(state);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Next dialogue line (`q` key).
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }
    let target = {
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        next_dialogue_line(&state.buffer, &state.translation_lines, state.current_line, line_count, state.is_prose(), &stage_lookup)
    };
    if let Some(target) = target {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Move cursor to previous dialogue line and seek media to it (`k` key).
pub fn cursor_prev_line(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let target = if state.is_prose() {
        // Prose works (.txt or DB) have no play-style dialogue structure and
        // few/no DB-mapped buffer lines, so dialogue-stepping skips headings
        // ("CHAPTER I") and behaves erratically. Step one plain buffer line.
        Some(state.current_line - 1)
    } else {
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        prev_dialogue_line(&state.buffer, &state.translation_lines, state.current_line, state.is_prose(), &stage_lookup)
    };
    let Some(target) = target else {
        return;
    };
    state.current_line = target;
    state.pending_advance = None;
    state.pending_advance_ignore_bl = None;
    state.prev_highlight_line.set(None);
    // Translation view: highlight follows the cursor, scroll only to keep it
    // within a vim-style scrolloff margin (not a page turn). Seek MPV to the new
    // line's start time so audio follows the cursor here too.
    if state.translations_visible {
        update_highlight_only(state);
        super::scroll::scroll_cursor_into_view_scrolloff(state);
        seek_to_current_line(state);
        return;
    }
    scroll_after_jump_backward(state);
    after_page_change(state, PageChangeReason::Dialogue);
}

/// Move cursor to next dialogue line and seek media to it (`j` key).
pub fn cursor_next_dialogue(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }
    let target = if state.is_prose() {
        // Prose: step one plain buffer line (see cursor_prev_line). Lands on
        // every line incl. headings; plays keep dialogue-skipping below.
        (state.current_line + 1 < line_count).then(|| state.current_line + 1)
    } else {
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        next_dialogue_line(&state.buffer, &state.translation_lines, state.current_line, line_count, state.is_prose(), &stage_lookup)
    };
    if let Some(target) = target {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        // Translation view: highlight follows the cursor, scroll only to keep
        // it within a vim-style scrolloff margin (not a page turn). Seek MPV to
        // the new line's start time so audio follows the cursor here too.
        if state.translations_visible {
            update_highlight_only(state);
            super::scroll::scroll_cursor_into_view_scrolloff(state);
            seek_to_current_line(state);
            return;
        }
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Buffer-line index of the first DIALOGUE line of each act (`div1` run) in a
/// play, in document order. An "act boundary" is the first mapped buffer line
/// whose work-line carries a new `div1`; from there we advance with
/// `next_dialogue_line` so the returned line is the act's first *spoken* line —
/// never the entrance stage direction or speaker name that opens the act. This
/// reads the authoritative `(div1)` metadata on `Work.lines` directly (the same
/// source the dropped `is_chapter` flag was derived from), so it needs no
/// chapter marking. Returns empty when there's no current work / no line_map.
fn act_dialogue_lines(state: &AppState) -> Vec<usize> {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let line_count = state.effective_line_count();
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| work.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let mut out = Vec::new();
    let mut prev_div1: Option<i64> = None;
    for bl in 0..line_count {
        let Some(wi) = state.work_line_for_buffer(bl) else { continue };
        let Some(line) = work.lines.get(wi) else { continue };
        if Some(line.div1) == prev_div1 {
            continue;
        }
        prev_div1 = Some(line.div1);
        // First dialogue AT OR AFTER this act boundary (skips the entrance stage
        // direction / speaker chrome). `next_dialogue_from` is inclusive of `bl`,
        // so an act that opens directly on a dialogue line is handled too.
        let d = next_dialogue_from(&state.buffer, bl, line_count, state.is_prose(), &stage_lookup);
        if d < line_count {
            // Dedup: consecutive acts resolve to the same first dialogue only if a
            // div1 has no dialogue of its own; keep entries strictly increasing.
            if out.last() != Some(&d) {
                out.push(d);
            }
        }
    }
    out
}

/// Previous chapter line (`[` key).
pub fn jump_to_prev_chapter(state: &mut AppState) {
    crate::app::translations::hide_translations_for_navigation(state);
    // Plays jump between ACTS, landing on each act's first dialogue line
    // (never a stage direction). Prose uses the is_chapter-based path below.
    if state.current_work.as_ref().map(|w| w.work_type == "play").unwrap_or(false) {
        jump_to_prev_act(state);
        return;
    }
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if state.current_line == 0 {
            return;
        }
        // Find the current chapter's start (the nearest chapter line at or
        // before current_line). If we're past it, jump there. If we're already
        // on it, jump to the previous chapter's start.
        let is_chapter_at = |bl: usize| -> bool {
            if let Some(ref lm) = state.line_map {
                lm.buffer_to_work
                    .get(bl)
                    .and_then(|o| o.as_ref())
                    .map(|wi| work.lines[*wi].is_chapter)
                    .unwrap_or(false)
            } else {
                work.lines.get(bl).map(|l| l.is_chapter).unwrap_or(false)
            }
        };
        let current_chapter_start = (0..=state.current_line).rev().find(|&bl| is_chapter_at(bl));
        match current_chapter_start {
            Some(start) if start < state.current_line => Some(start),
            Some(start) => (0..start).rev().find(|&bl| is_chapter_at(bl)),
            None => (0..state.current_line).rev().find(|&bl| is_chapter_at(bl)),
        }
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        state.page_back_stack.clear();
        state.page_back_stack.push((state.page_top_line, state.page_top_offset));
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                if is_line_fully_visible(state, line_idx) {
                    update_highlight_only(state);
                } else {
                    // Canonical spread containing the chapter line — same page the
                    // reader reaches paging through, so the header sits where
                    // pagination places it (consistent with bookmark jumps).
                    let top = canonical_page_top_for(state, line_idx);
                    set_page_instant(state, top);
                    // See jump_to_next_chapter: a chapter/act header that fills a
                    // degenerate spread leaves the cursor off-page; advance until
                    // it's visible.
                    ensure_cursor_visible_ereader(state, line_idx);
                }
            }
        }
        after_page_change(state, PageChangeReason::Chapter);
    }
}

/// Next chapter line.
pub fn jump_to_next_chapter(state: &mut AppState) {
    crate::app::translations::hide_translations_for_navigation(state);
    // Plays jump between ACTS (see jump_to_prev_chapter).
    if state.current_work.as_ref().map(|w| w.work_type == "play").unwrap_or(false) {
        jump_to_next_act(state);
        return;
    }
    let line_count = state.effective_line_count();
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if let Some(ref lm) = state.line_map {
            let mut found = None;
            for bl in (state.current_line + 1)..lm.buffer_to_work.len() {
                if let Some(Some(wi)) = lm.buffer_to_work.get(bl) {
                    if work.lines[*wi].is_chapter {
                        found = Some(bl);
                        break;
                    }
                }
            }
            found
        } else {
            let mut found = None;
            for i in (state.current_line + 1)..line_count {
                if work.lines[i].is_chapter {
                    found = Some(i);
                    break;
                }
            }
            found
        }
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        state.page_back_stack.clear();
        state.page_back_stack.push((state.page_top_line, state.page_top_offset));
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                if is_line_fully_visible(state, line_idx) {
                    update_highlight_only(state);
                } else {
                    // Canonical spread containing the chapter line — same page the
                    // reader reaches paging through (consistent with bookmark jumps).
                    let top = canonical_page_top_for(state, line_idx);
                    set_page_instant(state, top);
                    // canonical_page_top_for backs up to the chapter/act header so
                    // it shows above the cursor — but when that header fills a
                    // degenerate spread (Err/Tro: an ACT/scene header whose page is
                    // a lone 1-line spread at a short viewport), the cursor
                    // (line_idx, the chapter's first dialogue) lands OFF-PAGE below
                    // it. Advance until the cursor is actually visible — same guard
                    // as the scene jumps.
                    ensure_cursor_visible_ereader(state, line_idx);
                }
            }
        }
        after_page_change(state, PageChangeReason::Chapter);
    }
}

/// Land the cursor on `line_idx` for an act/chapter jump: same page/scroll
/// handling as the chapter jump (canonical spread + degenerate-spread guard),
/// reported as a `Chapter` page change. Shared by the play act-jumps.
fn land_on_chapter_target(state: &mut AppState, line_idx: usize) {
    state.current_line = line_idx;
    state.page_back_stack.clear();
    state.page_back_stack.push((state.page_top_line, state.page_top_offset));
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if is_line_fully_visible(state, line_idx) {
                update_highlight_only(state);
            } else {
                let top = canonical_page_top_for(state, line_idx);
                set_page_instant(state, top);
                ensure_cursor_visible_ereader(state, line_idx);
            }
        }
    }
    after_page_change(state, PageChangeReason::Chapter);
}

/// Plays only: jump to the first dialogue line of the previous act (`(` key).
/// Acts come from authoritative `(div1)` metadata; the cursor lands on the
/// act's first spoken line, never an entrance stage direction.
pub fn jump_to_prev_act(state: &mut AppState) {
    let acts = act_dialogue_lines(state);
    // The previous act is the last act-dialogue line strictly before the cursor.
    let target = acts.iter().rev().find(|&&d| d < state.current_line).copied();
    if let Some(line_idx) = target {
        land_on_chapter_target(state, line_idx);
    }
}

/// Plays only: jump to the first dialogue line of the next act (`&` key).
pub fn jump_to_next_act(state: &mut AppState) {
    let acts = act_dialogue_lines(state);
    let target = acts.iter().find(|&&d| d > state.current_line).copied();
    if let Some(line_idx) = target {
        land_on_chapter_target(state, line_idx);
    }
}

/// Previous scene marker (used for plays on the `2` key).
///
/// Walks backward from `current_line` looking for an act/scene marker, then
/// places the cursor on the first dialogue line of that scene. The viewport
/// top is pinned to the scene marker so the scene header stays visible above
/// the cursorline.
pub fn jump_to_prev_scene(state: &mut AppState) {
    use crate::db::line_types;
    let line_count = state.effective_line_count();
    let (marker, cursor) = {
        if state.current_line == 0 {
            return;
        }
        // Authoritative `section_starts` bitmap, not `is_act_scene_marker` text
        // (which false-positives on dialogue like 'act of hares…' — see
        // jump_to_next_scene). Text fallback only mid-load (no bitmap).
        let has_bitmap = state.section_starts().is_some();
        let is_marker_at = |bl: usize| -> bool {
            if has_bitmap {
                state.is_section_start(bl)
            } else {
                line_types::is_act_scene_marker(buffer_line_text(&state.buffer, bl).trim())
            }
        };
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        // Walk markers backward from just above the cursor and pick the FIRST
        // one whose opening dialogue is strictly before `current_line` — that's
        // the previous scene. This naturally skips an `Act N` header that sits
        // directly above its first `Scene 1` (no dialogue between them, so its
        // first dialogue equals the scene's and is NOT before the cursor), and
        // it never resolves to the current scene's own start (whose first
        // dialogue is at/after the cursor when we're sitting on it).
        let mut marker = None;
        let mut cursor = None;
        let mut bl = state.current_line;
        while bl > 0 {
            bl -= 1;
            if !is_marker_at(bl) {
                continue;
            }
            let first = next_dialogue_line(
                &state.buffer,
                &state.translation_lines,
                bl,
                line_count,
                state.is_prose(),
                &stage_lookup,
            );
            if let Some(d) = first {
                if d < state.current_line {
                    marker = Some(bl);
                    cursor = Some(d);
                    break;
                }
            }
        }
        (marker, cursor)
    };

    if let (Some(marker_idx), Some(cursor_idx)) = (marker, cursor) {
        let first_dialogue = first_dialogue_line(state);
        if cursor_idx < first_dialogue {
            return;
        }
        state.current_line = cursor_idx;
        // See jump_to_next_scene: clear but don't push, so `y` pages back one
        // viewport into skipped content rather than teleporting to the origin.
        state.page_back_stack.clear();
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                let top = header_page_top(state, marker_idx);
                // In two-column mode, if the new scene would render in the RIGHT
                // column (its header isn't this page's left-column top), re-page
                // so the scene begins the LEFT column — even if the cursor line
                // is already on screen. Otherwise keep the cheap highlight-only
                // path when the cursor is fully visible.
                if scene_starts_in_right_column(state, top) {
                    set_page_instant(state, top);
                } else if is_line_fully_visible(state, cursor_idx) {
                    update_highlight_only(state);
                } else {
                    set_page_instant(state, top);
                }
                ensure_cursor_visible_ereader(state, cursor_idx);
            }
        }
        after_page_change(state, PageChangeReason::Scene);
    }
}

/// After a two-column scene jump anchors the page on the scene's marker, make
/// sure the scene's first dialogue (`cursor_idx`) is actually visible. Normally
/// the marker page shows it, but when a scene's header + entrance fill the whole
/// spread and the first spoken line is pushed off (H8 1.4 at a short viewport:
/// `Scene 4` / `=====` / `[Trumpets…]` / `WOLSEY` fill the page, dialogue at 1706
/// doesn't fit), the cursor lands OFF-PAGE below a dialogue-less spread. Advance
/// the page top one spread at a time (via the now-always-progressing
/// `next_page_top`) until the cursor is visible. Bounded; only engages for the
/// degenerate dialogue-less-marker-spread case — normal scenes never loop.
pub(crate) fn ensure_cursor_visible_ereader(state: &mut AppState, cursor_idx: usize) {
    let line_count = state.effective_line_count();
    let mut guard = 0;
    while !is_line_fully_visible(state, cursor_idx) {
        let next = column_split(state, state.page_top_line).next_page_top;
        if next <= state.page_top_line || next >= line_count {
            break;
        }
        set_page_instant(state, next);
        guard += 1;
        if guard > 8 {
            break;
        }
    }
}

/// In two-column mode, return true when the scene whose page-top is `scene_top`
/// would currently render in the RIGHT column — i.e. `scene_top` is not already
/// the left-column top of the current page. Single-column always returns false
/// (no left/right distinction). Used to force a re-pagination so a scene jumped
/// to by `2`/`3` starts the left column instead of the right.
fn scene_starts_in_right_column(state: &AppState, scene_top: usize) -> bool {
    if state.column_count() != 2 {
        return false;
    }
    if scene_top == state.page_top_line {
        return false;
    }
    let cs = column_split(state, state.page_top_line);
    // The scene's header sits in the right column of the current page.
    scene_top >= cs.split && scene_top <= cs.page_end
}

/// Given a scene/act marker line, return the line that should sit at the page
/// top. When the marker is a `Scene` header that opens an act (an `Act N`
/// header sits just above it, separated only by `=` rules and blanks), back up
/// to the `Act N` line so the whole act/scene header block shows at the top.
fn header_page_top(state: &AppState, marker: usize) -> usize {
    use crate::db::line_types;
    let is_act = |bl: usize| {
        let text = buffer_line_text(&state.buffer, bl);
        text.trim().to_uppercase().starts_with("ACT ")
    };
    // The marker itself is the Act line — nothing to back up to.
    if marker == 0 || is_act(marker) {
        return marker;
    }
    // Walk upward across separators/blanks. If we reach an Act line, use it;
    // any real content (dialogue, speaker, stage direction) stops the search.
    let mut i = marker;
    while i > 0 {
        i -= 1;
        if is_act(i) {
            return i;
        }
        let text = buffer_line_text(&state.buffer, i);
        if line_types::is_separator(text.trim()) || text.trim().is_empty() {
            continue;
        }
        break;
    }
    marker
}

fn first_dialogue_line(state: &AppState) -> usize {
    if let Some(ref lm) = state.line_map {
        lm.dialogue_buffer_lines.first().copied().unwrap_or(0)
    } else {
        state.current_work.as_ref()
            .and_then(|w| w.lines.iter().position(|l| l.is_dialogue))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Speaker-turn navigation (J / K)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Next,
    Prev,
}

/// Pure scan over a line range `[0, len)`: from line `from`, return the first
/// dialogue line of the next / previous speaker turn — the next run of
/// consecutive lines whose speaker differs from `from`'s speaker. Returns the
/// FIRST dialogue line of that run in both directions. `None` when there is no
/// such turn ahead / behind (range boundary, or no speakers at all).
///
/// `speaker_at(i)` returns the authoritative `line_mapping.speaker` for line `i`
/// (None for unmapped chrome lines); `is_dialogue_at(i)` is the dialogue test.
/// Both are accessors so the scan reads lazily and allocates nothing — matching
/// the in-place scan pattern used by `next_dialogue_line` / `prev_dialogue_line`.
/// Operates only on per-line speaker metadata, never on buffer text (CLAUDE.md
/// → authoritative-boundary principle).
pub(crate) fn speaker_turn_target(
    len: usize,
    from: usize,
    dir: Direction,
    speaker_at: impl Fn(usize) -> Option<String>,
    is_dialogue_at: impl Fn(usize) -> bool,
) -> Option<usize> {
    let cur = speaker_at(from);
    match dir {
        Direction::Next => {
            ((from + 1)..len).find(|&i| is_dialogue_at(i) && speaker_at(i) != cur)
        }
        Direction::Prev => {
            let mut i = from;
            let prev_block_speaker;
            loop {
                if i == 0 {
                    return None;
                }
                i -= 1;
                if is_dialogue_at(i) && speaker_at(i) != cur {
                    prev_block_speaker = speaker_at(i);
                    break;
                }
            }
            let mut first = i;
            while i > 0 {
                i -= 1;
                if is_dialogue_at(i) {
                    if speaker_at(i) == prev_block_speaker {
                        first = i;
                    } else {
                        break;
                    }
                }
            }
            Some(first)
        }
    }
}

/// Pure scan: the FIRST dialogue line of the speaker block that contains `from`
/// — walking backward over the run of consecutive same-speaker dialogue lines.
/// Returns `None` when `from` is not itself a dialogue line with a speaker
/// (a chrome / stage-direction line has no block to anchor to). Used by `K` /
/// Shift+, to land on the top of the current speech before stepping to the
/// previous speaker. Reads only per-line speaker metadata (CLAUDE.md →
/// authoritative-boundary principle).
pub(crate) fn current_block_first_line(
    from: usize,
    speaker_at: impl Fn(usize) -> Option<String>,
    is_dialogue_at: impl Fn(usize) -> bool,
) -> Option<usize> {
    if !is_dialogue_at(from) {
        return None;
    }
    let cur = speaker_at(from);
    if cur.is_none() {
        return None;
    }
    let mut first = from;
    let mut i = from;
    while i > 0 {
        i -= 1;
        if is_dialogue_at(i) {
            if speaker_at(i) == cur {
                first = i;
            } else {
                break;
            }
        }
    }
    Some(first)
}

/// Pure scan: the next/prev non-blank buffer line from `from` in `dir`, or None
/// at the buffer edge. The prose analog of a speaker turn — prose has no speaker
/// metadata, so `q`/`,` step paragraph-to-paragraph instead. For
/// paragraph-granularity prose (e.g. BH) every buffer line is a paragraph, so
/// this is just the adjacent line; blank-separated prose skips the blanks.
pub(crate) fn adjacent_paragraph(
    line_count: usize,
    from: usize,
    dir: Direction,
    is_blank_at: impl Fn(usize) -> bool,
) -> Option<usize> {
    match dir {
        Direction::Next => ((from + 1)..line_count).find(|&i| !is_blank_at(i)),
        Direction::Prev => (0..from).rev().find(|&i| !is_blank_at(i)),
    }
}

/// Jump to the first dialogue line of the NEXT speaker turn (`J`). Seeks audio.
/// On prose (no speaker metadata) jumps to the NEXT paragraph instead.
pub fn jump_to_next_speaker(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if state.is_prose() {
        let target = adjacent_paragraph(line_count, state.current_line, Direction::Next, |i| {
            crate::db::line_types::is_blank(buffer_line_text(&state.buffer, i).trim())
        });
        if let Some(target) = target {
            let prev_line = state.current_line;
            state.current_line = target;
            state.pending_advance = None;
            state.pending_advance_ignore_bl = None;
            log_fmt!("PARAGRAPH_NEXT: {} -> {}", prev_line, target);
            scroll_after_jump_forward(state, prev_line);
            after_page_change(state, PageChangeReason::Dialogue);
        }
        return;
    }
    let target = {
        let work = state.current_work.as_ref().unwrap();
        speaker_turn_target(
            line_count,
            state.current_line,
            Direction::Next,
            |i| state.work_line_for_buffer(i).and_then(|wi| work.lines.get(wi)).and_then(|l| l.speaker.clone()),
            |i| is_dialogue_line(&state.buffer, i, state.is_prose(), &|bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            }),
        )
    };
    if let Some(target) = target {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        log_fmt!("SPEAKER_NEXT: {} -> {}", prev_line, target);
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Block-aware previous-speaker jump (`K` / Shift+,). Two-stage: if the cursor
/// is mid-block (not on the first dialogue line of the current speaker's run),
/// land on that first line first. Only when already at the block top does it
/// step to the first line of the PREVIOUS speaker turn. Seeks audio.
pub fn jump_to_prev_speaker(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if state.is_prose() {
        let target = adjacent_paragraph(line_count, state.current_line, Direction::Prev, |i| {
            crate::db::line_types::is_blank(buffer_line_text(&state.buffer, i).trim())
        });
        if let Some(target) = target {
            let prev_line = state.current_line;
            state.current_line = target;
            state.pending_advance = None;
            state.pending_advance_ignore_bl = None;
            state.prev_highlight_line.set(None);
            log_fmt!("PARAGRAPH_PREV: {} -> {}", prev_line, target);
            scroll_after_jump_backward(state);
            after_page_change(state, PageChangeReason::Dialogue);
        }
        return;
    }
    let speaker_at = |i: usize| {
        let work = state.current_work.as_ref().unwrap();
        state.work_line_for_buffer(i).and_then(|wi| work.lines.get(wi)).and_then(|l| l.speaker.clone())
    };
    // Stage 1: if mid-block, snap to the top of the current speech instead of
    // leaving it. `current_block_first_line` returns None for a non-dialogue
    // cursor, so chrome lines fall straight through to the prev-speaker scan.
    let block_top = current_block_first_line(
        state.current_line,
        speaker_at,
        |i| is_dialogue_line(&state.buffer, i, state.is_prose(), &|bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        }),
    );
    if let Some(top) = block_top {
        if top != state.current_line {
            log_fmt!("SPEAKER_PREV (block top): {} -> {}", state.current_line, top);
            state.current_line = top;
            state.pending_advance = None;
            state.pending_advance_ignore_bl = None;
            state.prev_highlight_line.set(None);
            scroll_after_jump_backward(state);
            after_page_change(state, PageChangeReason::Dialogue);
            return;
        }
    }
    // Stage 2: already at the block top (or on a chrome line) — step to the
    // previous speaker turn.
    let target = speaker_turn_target(
        line_count,
        state.current_line,
        Direction::Prev,
        speaker_at,
        |i| is_dialogue_line(&state.buffer, i, state.is_prose(), &|bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        }),
    );
    if let Some(target) = target {
        log_fmt!("SPEAKER_PREV: {} -> {}", state.current_line, target);
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
        scroll_after_jump_backward(state);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Next scene marker (used for plays on the `3` key).
pub fn jump_to_next_scene(state: &mut AppState) {
    use crate::db::line_types;
    let line_count = state.effective_line_count();
    // Find the next scene boundary from the AUTHORITATIVE `section_starts` bitmap
    // (DB `(div1,div2)`), NOT `is_act_scene_marker` buffer-text classification.
    // The text heuristic matches any line starting with "ACT "/"SCENE "/… — which
    // false-positives on ordinary dialogue (Tro 2438 'act of hares, are they not
    // monsters?' was misread as an ACT marker), so `3` jumped into the middle of a
    // speech, landing off the pagination chain and breaking `y` tiling. The bitmap
    // marks exactly the real boundaries. Fall back to the text scan only mid-load
    // (no bitmap yet). See CLAUDE.md "authoritative-boundary principle".
    let has_bitmap = state.section_starts().is_some();
    let (marker, cursor) = {
        let mut marker = None;
        for bl in (state.current_line + 1)..line_count {
            let is_boundary = if has_bitmap {
                state.is_section_start(bl)
            } else {
                line_types::is_act_scene_marker(buffer_line_text(&state.buffer, bl).trim())
            };
            if is_boundary {
                marker = Some(bl);
                break;
            }
        }
        let stage_lookup = |bi: usize| -> Option<i64> {
            state.work_line_for_buffer(bi)
                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                .map(|l| l.sub_line)
        };
        let cursor = marker.and_then(|m| {
            next_dialogue_line(&state.buffer, &state.translation_lines, m, line_count, state.is_prose(), &stage_lookup)
        });
        (marker, cursor)
    };

    if let (Some(marker_idx), Some(cursor_idx)) = (marker, cursor) {
        state.current_line = cursor_idx;
        // Clear the back-stack but do NOT push the jump origin: a scene jump can
        // skip many pages (e.g. mid-Scene 3 -> EPILOGUE), and `y` should page
        // back one viewport into the skipped content, not teleport back to where
        // `3` was pressed. An empty stack makes page_backward use prev_page_top.
        state.page_back_stack.clear();
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                // In two-column mode, if the new scene would render in the RIGHT
                // column, re-page so it begins the LEFT column even when the
                // cursor line is already on screen (see jump_to_prev_scene).
                if scene_starts_in_right_column(state, marker_idx) {
                    set_page_instant(state, marker_idx);
                } else if is_line_fully_visible(state, cursor_idx) {
                    update_highlight_only(state);
                } else {
                    set_page_instant(state, marker_idx);
                }
                ensure_cursor_visible_ereader(state, cursor_idx);
            }
        }
        after_page_change(state, PageChangeReason::Scene);
    }
}

/// Jump to the next structural section: scene marker for plays, chapter
/// for prose. Encapsulates the work_type routing so the dispatch table
/// stays clean.
pub fn jump_to_next_section(state: &mut AppState) {
    let is_play = state.current_work.as_ref()
        .map(|w| w.work_type == "play")
        .unwrap_or(false);
    if is_play {
        jump_to_next_scene(state);
    } else {
        jump_to_next_chapter(state);
    }
    // Mirror `;` (ShowCurrentChapter): surface the new act/scene (or chapter)
    // as the same transient toast after landing.
    show_current_chapter(state);
}

/// Jump to the previous structural section: scene marker for plays,
/// chapter for prose.
pub fn jump_to_prev_section(state: &mut AppState) {
    let is_play = state.current_work.as_ref()
        .map(|w| w.work_type == "play")
        .unwrap_or(false);
    if is_play {
        jump_to_prev_scene(state);
    } else {
        jump_to_prev_chapter(state);
    }
    // Mirror `;` (ShowCurrentChapter): surface the new act/scene (or chapter)
    // as the same transient toast after landing.
    show_current_chapter(state);
}

/// Show the act/scene (plays) or chapter (prose) containing the current line as
/// a transient toast.
pub fn show_current_chapter(state: &mut AppState) {
    let abbrev = match &state.current_work {
        Some(w) => w.abbrev.clone(),
        None => {
            log_fmt!("SHOW_CHAPTER (;): no current work — nothing to show");
            return;
        }
    };
    log_fmt!("SHOW_CHAPTER (;): abbrev={} current_line={} is_prose={}",
        abbrev, state.current_line, state.is_prose());

    // Plays/verse (anything not prose) show the authoritative act/scene label
    // derived from the line's (div1, div2) — never inferred from buffer text,
    // and never the "Chapter N of M" form even when the work happens to carry
    // chapter_breaks. Prose-vs-verse is decided by work_type, not by whether
    // chapter markers exist. See CLAUDE.md → authoritative-boundary principle.
    if !state.is_prose() {
        let (div1, div2) = crate::app::scene_synopsis::current_scene_divs(state);
        let label = crate::app::scene_synopsis::scene_label_for(state, div1, div2);
        let text = format!("{} — {}", abbrev, label);
        show_chapter_toast(state, &text);
        return;
    }

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    let chapter_lines: Vec<usize> = if let Some(ref lm) = state.line_map {
        lm.chapter_breaks.clone()
    } else {
        work.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_chapter)
            .map(|(i, _)| i)
            .collect()
    };

    // Prose work with no chapter markers: fall back to the scene label so the
    // toast still shows something meaningful rather than nothing.
    if chapter_lines.is_empty() {
        let (div1, div2) = crate::app::scene_synopsis::current_scene_divs(state);
        let label = crate::app::scene_synopsis::scene_label_for(state, div1, div2);
        let text = format!("{} — {}", abbrev, label);
        show_chapter_toast(state, &text);
        return;
    }

    let work_line_text = |bl: usize| -> &str {
        if let Some(ref lm) = state.line_map {
            lm.buffer_to_work.get(bl)
                .and_then(|o| o.as_ref())
                .map(|wi| work.lines[*wi].text.as_str())
                .unwrap_or("")
        } else {
            work.lines.get(bl).map(|l| l.text.as_str()).unwrap_or("")
        }
    };

    let current_bl = state.current_line;
    let has_implicit_first = chapter_lines[0] > 0;
    let total = chapter_lines.len() + if has_implicit_first { 1 } else { 0 };

    let (chapter_num, title) = match chapter_lines.iter().rposition(|&bl| bl <= current_bl) {
        Some(idx) => {
            let num = idx + 1 + if has_implicit_first { 1 } else { 0 };
            (num, work_line_text(chapter_lines[idx]).trim().to_string())
        }
        None => {
            // Before the first marker — this is the implicit Chapter 1.
            // Scan backward from the cursor for a line containing
            // "chapter" or "part " to find its heading; fall back to work title.
            let title = (0..=current_bl)
                .rev()
                .map(|bl| work_line_text(bl).trim())
                .find(|t| {
                    let lower = t.to_lowercase();
                    lower.contains("chapter") || lower.contains("part ")
                })
                .unwrap_or("")
                .to_string();
            let title = if title.is_empty() {
                work.title.clone()
            } else {
                title
            };
            (1, title)
        }
    };

    let text = format!("{} — Chapter {} of {} — {}", abbrev, chapter_num, total, title);
    show_chapter_toast(state, &text);
}

/// Show `text` in the chapter toast for 3 seconds.
///
/// Uses a generation counter (`chapter_toast_gen`) so rapid `;` presses don't
/// cut a later toast short: each call bumps the generation and the scheduled
/// hide-timeout captures it; when the timeout fires it only hides the toast if
/// its generation is still current. A stale timer (one whose toast has since
/// been superseded) is a no-op, so the latest toast always gets its full 3s.
pub(crate) fn show_chapter_toast(state: &AppState, text: &str) {
    let gen = state.chapter_toast_gen.get().wrapping_add(1);
    state.chapter_toast_gen.set(gen);
    log_fmt!("CHAPTER_TOAST: show gen={} text={:?}", gen, text);

    state.chapter_toast.set_text(text);
    state.chapter_toast.set_visible(true);

    let toast = state.chapter_toast.clone();
    let gen_cell = state.chapter_toast_gen.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        if gen_cell.get() != gen {
            log_fmt!("CHAPTER_TOAST: hide gen={} superseded (current={}), keeping visible",
                gen, gen_cell.get());
            return;
        }
        log_fmt!("CHAPTER_TOAST: hide gen={}", gen);
        toast.set_visible(false);
    });
}

/// Jump to the next bookmarked line (wraps around).
/// Jump to the next bookmarked line after the cursor. Does NOT wrap: if there is
/// no bookmark past the current line, the cursor stays put.
pub fn next_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() {
        return;
    }
    let line_count = is_bm.len();
    for idx in (state.current_line + 1)..line_count {
        if is_bm[idx] {
            drop(is_bm);
            jump_to_line(state, idx);
            return;
        }
    }
}

/// Jump to the previous bookmarked line before the cursor. Does NOT wrap: if
/// there is no bookmark before the current line, the cursor stays put.
pub fn prev_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || state.current_line == 0 {
        return;
    }
    let line_count = is_bm.len();
    let start = state.current_line.min(line_count);
    for idx in (0..start).rev() {
        if is_bm[idx] {
            drop(is_bm);
            jump_to_line(state, idx);
            return;
        }
    }
}

/// Resolve a `line_mapping.id` to its buffer line, honoring the optional
/// `line_map` (text-file works split/merge lines, so buffer and work indices
/// diverge; without a map they are 1:1).
pub(crate) fn buffer_line_for_line_id(s: &AppState, lm_id: i64) -> Option<usize> {
    if let Some(ref lm) = s.line_map {
        s.current_work.as_ref().and_then(|w| {
            let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
            Some(lm.work_to_buffer[work_idx])
        })
    } else {
        s.current_work.as_ref().and_then(|w| {
            w.lines.iter().position(|l| l.id == lm_id)
        })
    }
}

/// The `line_mapping.id` of the current cursor line, honoring the optional
/// `line_map` (the inverse of `buffer_line_for_line_id`).
pub(crate) fn current_line_id(s: &AppState) -> Option<i64> {
    s.current_work.as_ref().and_then(|w| {
        let work_idx = if let Some(ref lm) = s.line_map {
            lm.buffer_to_work.get(s.current_line).copied().flatten()
        } else {
            Some(s.current_line)
        };
        work_idx.and_then(|wi| w.lines.get(wi).map(|l| l.id))
    })
}

/// Jump to a specific buffer line (used by bookmark jump-to-recent).
pub fn jump_to_line(state: &mut AppState, buffer_line: usize) {
    let line_count = state.effective_line_count();
    if buffer_line >= line_count {
        return;
    }
    // If the target is already fully visible on the current spread/page, just
    // move the highlight — no scroll, no page turn. Re-paging an on-screen line
    // shifts the viewport for no reason (a bookmark on the current page would
    // otherwise jolt the spread). Mirrors scroll_after_jump_{forward,backward}.
    if super::viewport::is_line_fully_visible(state, buffer_line) {
        state.current_line = buffer_line;
        after_page_change(state, PageChangeReason::JumpToBookmark);
        return;
    }

    state.current_line = buffer_line;
    state.page_back_stack.clear();
    state.page_back_stack.push((state.page_top_line, state.page_top_offset));
    // Land on the CANONICAL spread for this line — the same page paging through
    // the work shows — so the bookmark sits where natural pagination places it,
    // not force-top-aligned (which page_turn_top_state would do).
    let top = canonical_page_top_for(state, buffer_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            set_page_instant(state, top);
        }
    }
    after_page_change(state, PageChangeReason::JumpToBookmark);
}

// ---------------------------------------------------------------------------
// Seek
// ---------------------------------------------------------------------------

/// Seek MPV to the current line's start time (with preroll).
/// Called on every cursor movement so audio follows the reader.
/// When the target line has a timestamp, suppresses CursorSync briefly while MPV
/// processes the seek. When it has no timestamp, suppresses indefinitely so the
/// cursor stays where the user put it.
pub fn seek_to_current_line(state: &mut AppState) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    let work_idx = match state.work_line_for_buffer(state.current_line) {
        Some(wi) => wi,
        None => return,
    };

    if let Some(ts) = &work.lines[work_idx].timestamp {
        // Exact timestamp — brief suppression while MPV processes the seek.
        // Don't shorten an existing longer suppression (e.g. from display_work).
        let new_until = std::time::Instant::now() + SYNC_SUPPRESS_SEEK;
        if state.suppress_sync_until.map_or(true, |existing| new_until > existing) {
            state.suppress_sync_until = Some(new_until);
        }
        let seek_time = preroll_seek_time(ts.start);
        log_fmt!("SEEK: line={} work_idx={} start={:.2} seek={:.2} suppress=500ms", state.current_line, work_idx, ts.start, seek_time);
        let _ = state
            .cmd_tx
            .try_send(crate::mpv::MpvCommand::Seek(seek_time));
    } else {
        // No timestamp — suppress indefinitely so cursor stays put
        log_fmt!("SEEK: line={} work_idx={} NO_TIMESTAMP suppress=86400s", state.current_line, work_idx);
        state.suppress_sync_until =
            Some(std::time::Instant::now() + SYNC_SUPPRESS_INDEFINITE);
    }
}

// ---------------------------------------------------------------------------
// Vocab jump
// ---------------------------------------------------------------------------

/// Jump to the next vocab word occurrence after current position.
pub fn jump_to_next_vocab(state: &mut AppState) {
    if state.vocab_matches.is_empty() {
        return;
    }

    let next_idx = match state.vocab_match_idx {
        Some(idx) => {
            if idx + 1 < state.vocab_matches.len() {
                idx + 1
            } else {
                0
            }
        }
        None => {
            state.vocab_matches
                .iter()
                .position(|m| m.line_index > state.current_line)
                .unwrap_or(0)
        }
    };

    state.vocab_match_idx = Some(next_idx);
    let target_line = state.vocab_matches[next_idx].line_index;
    state.current_line = target_line;
    state.page_back_stack.clear();
    state.page_back_stack.push((state.page_top_line, state.page_top_offset));
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            // Land on the CANONICAL spread for this line — the same page paging
            // through the work shows — not force-top-aligned. Mirrors bookmark
            // jump_to_line and search n/N; previously this top-aligned target_line.
            if !is_line_fully_visible(state, target_line) {
                set_page_instant(state, canonical_page_top_for(state, target_line));
            }
        }
    }
    after_page_change(state, PageChangeReason::Vocab);
}


/// Jump to the previous vocab word occurrence before current position.
pub fn jump_to_prev_vocab(state: &mut AppState) {
    if state.vocab_matches.is_empty() {
        return;
    }

    let prev_idx = match state.vocab_match_idx {
        Some(idx) => {
            if idx > 0 {
                idx - 1
            } else {
                state.vocab_matches.len() - 1
            }
        }
        None => {
            state.vocab_matches
                .iter()
                .rposition(|m| m.line_index < state.current_line)
                .unwrap_or(state.vocab_matches.len() - 1)
        }
    };

    state.vocab_match_idx = Some(prev_idx);
    let target_line = state.vocab_matches[prev_idx].line_index;
    state.current_line = target_line;
    state.page_back_stack.clear();
    state.page_back_stack.push((state.page_top_line, state.page_top_offset));
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            // Land on the CANONICAL spread for this line (see jump_to_next_vocab).
            if !is_line_fully_visible(state, target_line) {
                set_page_instant(state, canonical_page_top_for(state, target_line));
            }
        }
    }
    after_page_change(state, PageChangeReason::Vocab);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod page_turn_tests {
    use crate::db::line_types;

    /// Load the cleaned Troilus text, stripping ## prefixes like the app does.
    fn load_troilus_lines() -> Vec<String> {
        let path = std::path::Path::new(
            "/home/mlj/utono/literature/shakespeare-william/folger-cleaned/troilus-and-cressida.txt",
        );
        if !path.exists() {
            panic!("Troilus cleaned file not found at {:?}", path);
        }
        let contents = std::fs::read_to_string(path).expect("Failed to read Troilus file");
        contents
            .lines()
            .map(|line| {
                if let Some(stripped) = line.strip_prefix("## ") {
                    stripped.to_string()
                } else {
                    line.to_string()
                }
            })
            .collect()
    }

    /// Check if a line index is inside a multi-line `[...]` stage direction block.
    fn is_in_stage_block(lines: &[String], idx: usize) -> bool {
        let text = lines[idx].trim();
        if line_types::is_stage_direction(text) {
            return true;
        }
        let start = idx.saturating_sub(20);
        for i in (start..idx).rev() {
            let prev = lines[i].trim();
            if prev.ends_with(']') {
                return false;
            }
            if prev.starts_with('[') && !prev.ends_with(']') {
                return true;
            }
        }
        false
    }

    fn is_dialogue_line(lines: &[String], idx: usize) -> bool {
        let text = &lines[idx];
        let next = lines.get(idx + 1).map(String::as_str).unwrap_or("");
        !line_types::is_blank(text)
            && line_types::is_dialogue(text, false)
            && !line_types::is_title_above_separator(text, next)
            && !is_in_stage_block(lines, idx)
    }

    /// Collect all dialogue line indices in the file.
    fn dialogue_indices(lines: &[String]) -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(i, _)| is_dialogue_line(lines, *i))
            .map(|(i, _)| i)
            .collect()
    }

    /// Simulate next_dialogue_from on plain strings.
    fn next_dialogue(lines: &[String], from: usize) -> Option<usize> {
        for i in from..lines.len() {
            if is_dialogue_line(lines, i) {
                return Some(i);
            }
        }
        None
    }

    /// Simulate last_dialogue_in_page on plain strings.
    fn last_dialogue_in_range(lines: &[String], from: usize, count: usize) -> usize {
        let end = (from + count).min(lines.len());
        let mut last = from;
        for i in from..end {
            if is_dialogue_line(lines, i) {
                last = i;
            }
        }
        last
    }

    /// Simulate back_up_for_speaker on plain strings.
    fn back_up_for_speaker(lines: &[String], line: usize) -> usize {
        let mut top = line;
        while top > 0 {
            let trimmed = lines[top - 1].trim();
            if trimmed.is_empty()
                || line_types::is_speaker(trimmed)
                || line_types::is_stage_direction(trimmed)
                || line_types::is_act_scene_marker(trimmed)
                || line_types::is_separator(trimmed)
                || is_in_stage_block(lines, top - 1)
            {
                top -= 1;
            } else {
                break;
            }
        }
        top
    }

    #[test]
    fn test_back_up_for_speaker_includes_stage_direction() {
        // Comedy of Errors IV.ii — stage direction `[Enter Dromio…]` between
        // Adriana's curse and Dromio's first dialogue should be included on
        // the new page, not skipped.
        let lines: Vec<String> = vec![
            "curse.",
            "",
            "[Enter Dromio of Syracuse with the key.]",
            "",
            "DROMIO OF SYRACUSE",
            "Here, go—the desk, the purse! Sweet, now make",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        // Target: the dialogue line "Here, go—the desk…" at index 5
        let new_top = back_up_for_speaker(&lines, 5);
        assert_eq!(
            new_top, 1,
            "new page top should land on the blank line above the stage direction (got line {} = '{}')",
            new_top, lines[new_top]
        );
    }

    #[test]
    fn test_back_up_for_speaker_no_stage_direction() {
        // Plain speaker without preceding stage direction — behavior unchanged.
        let lines: Vec<String> = vec![
            "Previous dialogue line.",
            "",
            "ADRIANA",
            "Ah, but I think him better than I say,",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let new_top = back_up_for_speaker(&lines, 3);
        assert_eq!(new_top, 1, "should back up to blank above speaker");
    }

    /// Test page-forward through entire Troilus: every page turn must advance
    /// to the next dialogue line with no gaps or repeats.
    #[test]
    fn test_page_forward_no_gaps_or_repeats() {
        let lines = load_troilus_lines();
        let all_dialogue = dialogue_indices(&lines);
        assert!(
            all_dialogue.len() > 100,
            "Expected 100+ dialogue lines, got {}",
            all_dialogue.len()
        );

        let page_size = 30; // approximate lines per page
        let line_count = lines.len();

        // Track which dialogue lines we've highlighted, in order
        let mut highlighted: Vec<usize> = Vec::new();

        // Start: first dialogue line
        let first = all_dialogue[0];
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut current_line = first;
        highlighted.push(current_line);

        // Page forward until end
        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 500 {
                panic!("Page forward seems stuck after {} iterations", iterations);
            }

            // Simulate last_fully_visible_line: page_top + page_size
            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            let last = last_dialogue_in_range(&lines, page_top, last_visible - page_top + 1);
            let next = match next_dialogue(&lines, last + 1) {
                Some(n) => n,
                None => break, // reached end
            };
            if next >= line_count {
                break;
            }

            let new_top = back_up_for_speaker(&lines, next);
            page_top = new_top;
            current_line = next;
            highlighted.push(current_line);
        }

        // Verify: highlighted lines should be strictly increasing
        for i in 1..highlighted.len() {
            assert!(
                highlighted[i] > highlighted[i - 1],
                "Page forward: highlighted line {} (line {}) is not after {} (line {}), page {}",
                highlighted[i],
                lines[highlighted[i]].chars().take(50).collect::<String>(),
                highlighted[i - 1],
                lines[highlighted[i - 1]].chars().take(50).collect::<String>(),
                i
            );
        }

        // Verify: every highlighted line is a dialogue line
        for &h in &highlighted {
            assert!(
                is_dialogue_line(&lines, h),
                "Highlighted line {} is not dialogue: '{}'",
                h,
                &lines[h]
            );
        }

        // Verify: no dialogue lines were skipped between consecutive highlights
        // (i.e., every dialogue line between two highlighted lines was on the same page)
        for i in 1..highlighted.len() {
            let prev = highlighted[i - 1];
            let curr = highlighted[i];
            // Find all dialogue lines between prev and curr
            let between: Vec<usize> = all_dialogue
                .iter()
                .filter(|&&d| d > prev && d < curr)
                .copied()
                .collect();
            // These should all be on the same page as prev (between page_top and last_visible)
            // We can't verify exact page boundaries without GTK, but we can verify
            // the gap isn't larger than page_size (which would mean we skipped a whole page)
            if !between.is_empty() {
                let gap = curr - prev;
                assert!(
                    gap <= page_size + 10, // allow some slack for speaker/blank lines
                    "Gap too large between highlights: line {} to {} (gap={}, {} dialogue lines between). Page {}",
                    prev, curr, gap, between.len(), i
                );
            }
        }

        eprintln!(
            "Page forward test passed: {} pages, {} to {} ({} dialogue lines total)",
            highlighted.len(),
            highlighted[0],
            highlighted.last().unwrap(),
            all_dialogue.len()
        );
    }

    // --- Prose tests ---

    /// Helper: find next non-blank line at or after `from` (prose mode).
    fn next_nonblank(lines: &[String], from: usize) -> Option<usize> {
        for i in from..lines.len() {
            if !line_types::is_blank(&lines[i]) {
                return Some(i);
            }
        }
        None
    }

    /// Load a prose file's lines.
    fn load_prose_lines(path: &str) -> Vec<String> {
        let p = std::path::Path::new(path);
        if !p.exists() {
            panic!("Prose file not found at {}", path);
        }
        std::fs::read_to_string(p)
            .expect("read")
            .lines()
            .map(String::from)
            .collect()
    }

    /// Test page-forward through Bleak House: every page turn advances
    /// to the next non-blank line with no repeats.
    #[test]
    fn test_page_forward_prose_bleak_house() {
        let path = "/home/mlj/utono/literature/dickens-charles/bleak-house-prepared.txt";
        if !std::path::Path::new(path).exists() {
            eprintln!("SKIP: Bleak House not found");
            return;
        }
        let lines = load_prose_lines(path);
        let page_size = 30;
        let line_count = lines.len();

        let first = next_nonblank(&lines, 0).expect("no non-blank lines");
        let mut page_top = first;
        let mut current_line = first;
        let mut highlighted: Vec<usize> = vec![current_line];

        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 5000 { break; }

            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            // Last non-blank in page range
            let mut last = page_top;
            for i in page_top..=last_visible {
                if i < line_count && !line_types::is_blank(&lines[i]) {
                    last = i;
                }
            }
            let next = match next_nonblank(&lines, last + 1) {
                Some(n) => n,
                None => break,
            };

            page_top = next;
            current_line = next;
            highlighted.push(current_line);
        }

        for i in 1..highlighted.len() {
            assert!(
                highlighted[i] > highlighted[i - 1],
                "Prose forward: line {} not after {} at page {}",
                highlighted[i], highlighted[i - 1], i
            );
        }

        for &h in &highlighted {
            assert!(
                !line_types::is_blank(&lines[h]),
                "Highlighted line {} is blank", h
            );
        }

        eprintln!(
            "Bleak House forward: {} pages, line {} to {} ({} total lines)",
            highlighted.len(), highlighted[0], highlighted.last().unwrap(), line_count
        );
    }

    /// Test page-backward through Bleak House via history stack:
    /// forward all the way recording history, then backward pops history.
    /// Round-trip must be exact.
    #[test]
    fn test_page_backward_prose_bleak_house() {
        let path = "/home/mlj/utono/literature/dickens-charles/bleak-house-prepared.txt";
        if !std::path::Path::new(path).exists() {
            eprintln!("SKIP: Bleak House not found");
            return;
        }
        let lines = load_prose_lines(path);
        let page_size = 30;
        let line_count = lines.len();

        let first = next_nonblank(&lines, 0).expect("no non-blank lines");
        let mut page_top = first;
        let mut history: Vec<usize> = Vec::new();
        let mut forward_tops: Vec<usize> = vec![page_top];

        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 5000 { break; }
            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            let mut last = page_top;
            for i in page_top..=last_visible {
                if i < line_count && !line_types::is_blank(&lines[i]) {
                    last = i;
                }
            }
            let next = match next_nonblank(&lines, last + 1) {
                Some(n) => n,
                None => break,
            };
            history.push(page_top);
            page_top = next;
            forward_tops.push(page_top);
        }

        // Backward via history
        let mut backward_tops: Vec<usize> = vec![page_top];
        while let Some(prev_top) = history.pop() {
            page_top = prev_top;
            backward_tops.push(page_top);
        }

        // Verify exact round-trip
        assert_eq!(
            backward_tops.len(), forward_tops.len(),
            "Forward {} pages but backward {} pages",
            forward_tops.len(), backward_tops.len()
        );
        for i in 0..forward_tops.len() {
            assert_eq!(
                forward_tops[i],
                backward_tops[backward_tops.len() - 1 - i],
                "Round-trip mismatch at page {}",
                i
            );
        }

        eprintln!(
            "Bleak House backward (history): {} pages, {} down to {}",
            backward_tops.len(), backward_tops[0], backward_tops.last().unwrap()
        );
    }

    /// Load the cleaned Comedy of Errors text, stripping ## prefixes like the app does.
    /// This text contains the IV.ii Dromio entrance stage direction that exposed
    /// the original gap bug.
    fn load_comedy_of_errors_lines() -> Vec<String> {
        let path = std::path::Path::new(
            "/home/mlj/utono/literature/shakespeare-william/folger-cleaned/the-comedy-of-errors.txt",
        );
        if !path.exists() {
            panic!("Comedy of Errors cleaned file not found at {:?}", path);
        }
        let contents = std::fs::read_to_string(path).expect("Failed to read Errors file");
        contents
            .lines()
            .map(|line| {
                if let Some(stripped) = line.strip_prefix("## ") {
                    stripped.to_string()
                } else {
                    line.to_string()
                }
            })
            .collect()
    }

    /// A line is "viewable" if it should appear on screen — i.e. anything except
    /// a blank separator line. Stage directions, scene markers, speakers and
    /// dialogue are all viewable content.
    fn is_viewable_line(text: &str) -> bool {
        !line_types::is_blank(text)
    }

    /// Simulate `page_turn_top` on plain strings — same semantics as
    /// `back_up_for_speaker`: walk back over any non-dialogue content.
    fn page_turn_top_sim(lines: &[String], target_line: usize) -> usize {
        back_up_for_speaker(lines, target_line)
    }

    /// Walk a forward-only sequence of viewports, return the union of visited
    /// `[page_top, last_visible]` ranges. `step` produces the next page_top
    /// from the current one, returning None when there are no more pages.
    fn collect_visited_ranges<F>(
        line_count: usize,
        page_size: usize,
        first_top: usize,
        mut step: F,
    ) -> Vec<(usize, usize)>
    where
        F: FnMut(usize) -> Option<usize>,
    {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut top = first_top;
        let mut iterations = 0;
        loop {
            let last = (top + page_size).min(line_count.saturating_sub(1));
            ranges.push((top, last));
            iterations += 1;
            if iterations > 1000 {
                panic!("Forward traversal stuck after {} iterations", iterations);
            }
            match step(top) {
                Some(next) if next > top => top = next,
                _ => break,
            }
        }
        ranges
    }

    /// Coverage test: forward-walk Comedy of Errors with `x` (page_forward).
    /// Every non-blank line — including stage directions, speakers, scene
    /// markers — must appear in at least one visited viewport.
    #[test]
    fn test_x_page_forward_covers_every_line_errors() {
        let lines = load_comedy_of_errors_lines();
        let line_count = lines.len();
        let page_size = 30;

        // Start: top of file, like the app does on initial display.
        let first_top = 0;

        // step: emulate page_forward — find last dialogue in current page,
        // then next dialogue after it, then back up for speaker.
        let lines_ref = &lines;
        let ranges = collect_visited_ranges(line_count, page_size, first_top, |top| {
            let last_visible = (top + page_size).min(line_count.saturating_sub(1));
            let last = last_dialogue_in_range(lines_ref, top, last_visible - top + 1);
            let next = next_dialogue(lines_ref, last + 1)?;
            Some(back_up_for_speaker(lines_ref, next))
        });

        let mut uncovered: Vec<usize> = Vec::new();
        for (i, text) in lines.iter().enumerate() {
            if !is_viewable_line(text) {
                continue;
            }
            let covered = ranges.iter().any(|&(a, b)| i >= a && i <= b);
            if !covered {
                uncovered.push(i);
            }
        }
        if !uncovered.is_empty() {
            let preview: Vec<String> = uncovered
                .iter()
                .take(8)
                .map(|&i| format!("  line {}: '{}'", i, lines[i]))
                .collect();
            panic!(
                "x/page_forward left {} viewable lines uncovered (showing first {}):\n{}",
                uncovered.len(),
                preview.len(),
                preview.join("\n"),
            );
        }
    }

    /// Coverage test: forward-walk Comedy of Errors using `j`/`q`
    /// (cursor_next_dialogue → scroll_after_jump_forward → page_turn_top).
    /// Every non-blank line must appear in at least one visited viewport.
    #[test]
    fn test_j_cursor_next_dialogue_covers_every_line_errors() {
        let lines = load_comedy_of_errors_lines();
        let line_count = lines.len();
        let page_size = 30;

        // Start: top of file like the app does on initial display.
        let first_top = 0;
        // First dialogue line is the initial cursor.
        let first_dialogue = dialogue_indices(&lines)[0];

        // step: emulate j — find next dialogue line. If it falls outside the
        // current viewport (top..top+page_size), do a page turn using
        // page_turn_top. Otherwise the cursor moved within the page (no new
        // viewport range to record), so skip to the next dialogue.
        let lines_ref = &lines;
        let mut current_dialogue = first_dialogue;
        let ranges = collect_visited_ranges(line_count, page_size, first_top, |top| {
            // advance cursor through dialogue lines until one leaves the viewport
            let last_visible = (top + page_size).min(line_count.saturating_sub(1));
            let mut cursor = current_dialogue;
            loop {
                let next = next_dialogue(lines_ref, cursor + 1)?;
                if next > last_visible {
                    // page turn
                    current_dialogue = next;
                    return Some(page_turn_top_sim(lines_ref, next));
                }
                cursor = next;
            }
        });

        let mut uncovered: Vec<usize> = Vec::new();
        for (i, text) in lines.iter().enumerate() {
            if !is_viewable_line(text) {
                continue;
            }
            let covered = ranges.iter().any(|&(a, b)| i >= a && i <= b);
            if !covered {
                uncovered.push(i);
            }
        }
        if !uncovered.is_empty() {
            let preview: Vec<String> = uncovered
                .iter()
                .take(8)
                .map(|&i| format!("  line {}: '{}'", i, lines[i]))
                .collect();
            panic!(
                "j/cursor_next_dialogue left {} viewable lines uncovered (showing first {}):\n{}",
                uncovered.len(),
                preview.len(),
                preview.join("\n"),
            );
        }
    }

    // --- All-Shakespeare page-forward tests ---

    const SHAKESPEARE_DIR: &str =
        "/home/mlj/utono/literature/shakespeare-william/folger-cleaned";

    /// Plays only — skip poetry/sonnets which have no scene structure.
    fn shakespeare_play_files() -> Vec<std::path::PathBuf> {
        let dir = std::path::Path::new(SHAKESPEARE_DIR);
        if !dir.exists() {
            return Vec::new();
        }
        let skip = [
            "shakespeares-sonnets.txt",
            "venus-and-adonis.txt",
            "lucrece.txt",
            "the-phoenix-and-turtle.txt",
        ];
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().map(|e| e == "txt").unwrap_or(false)
                    && !skip.iter().any(|s| p.file_name().unwrap() == std::ffi::OsStr::new(s))
            })
            .collect();
        files.sort();
        files
    }

    fn load_play_lines(path: &std::path::Path) -> Vec<String> {
        let contents = std::fs::read_to_string(path).expect("read");
        let mut result: Vec<String> = Vec::new();
        let file_lines: Vec<&str> = contents.lines().collect();
        for (i, line) in file_lines.iter().enumerate() {
            if line_types::is_blank(line) {
                let next_non_blank = file_lines[i + 1..]
                    .iter()
                    .find(|l| !line_types::is_blank(l));
                if let Some(next) = next_non_blank {
                    if line_types::is_speaker(next) {
                        continue;
                    }
                }
            }
            if let Some(stripped) = line.strip_prefix("## ") {
                result.push(stripped.to_string());
            } else {
                result.push(line.to_string());
            }
        }
        result
    }

    /// Test page-forward through ALL Shakespeare plays: every page turn must
    /// advance (no stuck states where new_top <= old_top).
    #[test]
    fn test_page_forward_all_shakespeare_no_stuck() {
        let files = shakespeare_play_files();
        if files.is_empty() {
            eprintln!("SKIP: no Shakespeare files found");
            return;
        }

        let page_size = 30;
        let mut total_plays = 0;
        let mut total_pages = 0;

        for path in &files {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let lines = load_play_lines(path);
            let line_count = lines.len();
            if line_count == 0 {
                continue;
            }

            let first = match next_dialogue(&lines, 0) {
                Some(d) => d,
                None => continue,
            };
            let mut page_top = back_up_for_speaker(&lines, first);
            let mut pages = 1usize;
            let mut iterations = 0;

            loop {
                iterations += 1;
                if iterations > 2000 {
                    panic!("{}: page forward stuck after {} iterations at page_top={}",
                           name, iterations, page_top);
                }

                let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
                let last = last_dialogue_in_range(&lines, page_top, last_visible - page_top + 1);
                let next = match next_dialogue(&lines, last + 1) {
                    Some(n) => n,
                    None => break,
                };
                let new_top = back_up_for_speaker(&lines, next);

                if new_top <= page_top {
                    // Fallback: skip to next_dialogue directly (mirrors page_forward logic)
                    if next > page_top {
                        page_top = next;
                    } else {
                        panic!("{}: page forward stuck at page_top={} new_top={} next={}",
                               name, page_top, new_top, next);
                    }
                } else {
                    page_top = new_top;
                }
                pages += 1;
            }

            total_plays += 1;
            total_pages += pages;
        }

        eprintln!(
            "Page forward test passed: {} Shakespeare plays, {} total pages, no stuck states",
            total_plays, total_pages
        );
    }

    /// Test page forward+backward round-trip through ALL Shakespeare plays:
    /// forward all the way recording tops, backward via history, verify exact
    /// round-trip.
    #[test]
    fn test_page_forward_backward_roundtrip_all_shakespeare() {
        let files = shakespeare_play_files();
        if files.is_empty() {
            eprintln!("SKIP: no Shakespeare files found");
            return;
        }

        let page_size = 30;

        for path in &files {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let lines = load_play_lines(path);
            let line_count = lines.len();
            if line_count == 0 {
                continue;
            }

            let first = match next_dialogue(&lines, 0) {
                Some(d) => d,
                None => continue,
            };
            let mut page_top = back_up_for_speaker(&lines, first);
            let mut forward_tops: Vec<usize> = vec![page_top];

            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 2000 { break; }

                let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
                let last = last_dialogue_in_range(&lines, page_top, last_visible - page_top + 1);
                let next = match next_dialogue(&lines, last + 1) {
                    Some(n) => n,
                    None => break,
                };
                let new_top = back_up_for_speaker(&lines, next);
                if new_top <= page_top {
                    if next > page_top {
                        page_top = next;
                    } else {
                        break;
                    }
                } else {
                    page_top = new_top;
                }
                forward_tops.push(page_top);
            }

            // Verify forward tops are strictly increasing
            for i in 1..forward_tops.len() {
                assert!(
                    forward_tops[i] > forward_tops[i - 1],
                    "{}: forward page top {} not after {} at page {}",
                    name, forward_tops[i], forward_tops[i - 1], i
                );
            }

            // Backward via history stack
            let mut backward_tops: Vec<usize> = vec![*forward_tops.last().unwrap()];
            for top in forward_tops.iter().rev().skip(1) {
                backward_tops.push(*top);
            }

            assert_eq!(
                forward_tops.len(), backward_tops.len(),
                "{}: forward {} pages but backward {} pages",
                name, forward_tops.len(), backward_tops.len()
            );

            for i in 0..forward_tops.len() {
                assert_eq!(
                    forward_tops[i],
                    backward_tops[backward_tops.len() - 1 - i],
                    "{}: round-trip mismatch at page {}",
                    name, i
                );
            }
        }
    }

    /// Test scene synopsis identification: for each scene boundary in all
    /// Shakespeare plays, verify the first dialogue line after the scene marker
    /// maps to the correct (div1, div2) via the work's line_mapping.
    #[test]
    fn test_scene_synopsis_identification_all_shakespeare() {
        let db_path = "/home/mlj/utono/litdb/data/lit.db";
        if !std::path::Path::new(db_path).exists() {
            eprintln!("SKIP: lit.db not found");
            return;
        }
        let conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).expect("open db");

        let files = shakespeare_play_files();
        if files.is_empty() {
            eprintln!("SKIP: no Shakespeare files found");
            return;
        }

        let mut total_scenes = 0;
        let mut verified = 0;

        for path in &files {
            let _name = path.file_stem().unwrap().to_str().unwrap();
            let lines = load_play_lines(path);

            // Find the work abbreviation from DB by text_file path
            let path_str = path.to_str().unwrap();
            let abbrev: Option<String> = conn.query_row(
                "SELECT abbrev FROM works WHERE text_file = ?1",
                [path_str],
                |row| row.get(0),
            ).ok();
            let abbrev = match abbrev {
                Some(a) => a,
                None => continue,
            };

            // Load synopses for this work
            let mut stmt = conn.prepare(
                "SELECT div1, div2 FROM scene_synopses WHERE work_abbrev = ?1"
            ).unwrap();
            let synopsis_keys: Vec<(i64, i64)> = stmt.query_map([&abbrev], |row| {
                Ok((row.get(0)?, row.get(1)?))
            }).unwrap().filter_map(|r| r.ok()).collect();
            if synopsis_keys.is_empty() {
                continue;
            }

            // Load line data from DB to get div1/div2 per work line
            let mut line_stmt = conn.prepare(
                "SELECT div1, div2, canonical_text FROM line_mapping \
                 WHERE work_abbrev = ?1 ORDER BY id"
            ).unwrap();
            let work_lines: Vec<(i64, i64, String)> = line_stmt.query_map([&abbrev], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            }).unwrap().filter_map(|r| r.ok()).collect();

            // Find scene markers in the cleaned text
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if !line_types::is_act_scene_marker(trimmed) {
                    continue;
                }
                total_scenes += 1;

                // Find first dialogue line after this marker
                let first_dialogue = match next_dialogue(&lines, i) {
                    Some(d) => d,
                    None => continue,
                };

                // The first dialogue text should match a work line, giving us its div1/div2
                let dialogue_text = lines[first_dialogue].trim();
                if let Some((div1, div2, _)) = work_lines.iter().find(|(_, _, text)| {
                    text.trim() == dialogue_text
                }) {
                    // Verify this scene has a synopsis
                    if synopsis_keys.contains(&(*div1, *div2)) {
                        verified += 1;
                    }
                }
            }
        }

        eprintln!(
            "Synopsis identification: {} total scene markers, {} verified with synopsis match",
            total_scenes, verified
        );
        assert!(verified > 0, "Expected at least some synopsis matches");
    }

    // --- Section-break clamping simulation ---

    /// Simulate clamp_at_section_break on plain strings (no pixel heights).
    /// Matches the logic in viewport.rs: skip the opening header block, then
    /// clamp at the first marker/separator found within the visible range.
    fn clamp_at_section_break(
        lines: &[String], page_top: usize, last_fit: usize,
    ) -> usize {
        let mut scan_start = page_top + 1;
        while scan_start <= last_fit {
            let trimmed = lines[scan_start].trim();
            if line_types::is_act_scene_marker(trimmed)
                || line_types::is_separator(trimmed)
                || trimmed.is_empty()
                || line_types::is_stage_direction(trimmed)
            {
                scan_start += 1;
            } else {
                break;
            }
        }
        for i in scan_start..=last_fit {
            let trimmed = lines[i].trim();
            if line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed) {
                let clamped = i.saturating_sub(1);
                if clamped >= page_top {
                    return clamped;
                }
            }
        }
        last_fit
    }

    /// Simulate next_page_top with section-break clamping.
    fn next_page_top_clamped(
        lines: &[String], page_top: usize, page_size: usize,
    ) -> Option<(usize, usize)> {
        let line_count = lines.len();
        let raw_last = (page_top + page_size).min(line_count.saturating_sub(1));
        let last_visible = clamp_at_section_break(lines, page_top, raw_last);
        let last = last_dialogue_in_range(lines, page_top, last_visible - page_top + 1);
        let next = next_dialogue(lines, last + 1)?;
        if next >= line_count { return None; }
        let new_top = back_up_for_speaker(lines, next);
        if new_top <= page_top {
            if next > page_top { Some((next, next)) } else { None }
        } else {
            Some((new_top, next))
        }
    }

    /// Find all scene marker indices in a play.
    fn scene_marker_indices(lines: &[String]) -> Vec<usize> {
        lines.iter().enumerate()
            .filter(|(_, l)| line_types::is_act_scene_marker(l.trim()))
            .map(|(i, _)| i)
            .collect()
    }

    // --- Tests for section-break clamping (x never shows scene break mid-page) ---

    /// x (page_forward) with clamping: no page should contain a scene marker
    /// in its interior (after the opening header block). Verified across all
    /// Shakespeare plays.
    #[test]
    fn test_x_page_forward_no_mid_page_scene_breaks_all_shakespeare() {
        let files = shakespeare_play_files();
        if files.is_empty() {
            eprintln!("SKIP: no Shakespeare files found");
            return;
        }
        let page_size = 30;
        for path in &files {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let lines = load_play_lines(path);
            let line_count = lines.len();
            if line_count == 0 { continue; }
            let first = match next_dialogue(&lines, 0) {
                Some(d) => d,
                None => continue,
            };
            let mut page_top = back_up_for_speaker(&lines, first);
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 2000 { break; }
                let raw_last = (page_top + page_size).min(line_count.saturating_sub(1));
                let last_visible = clamp_at_section_break(&lines, page_top, raw_last);
                // Check: no scene marker in the interior of this page
                // (skip the opening header block, same as clamp_at_section_break does)
                let mut scan = page_top + 1;
                while scan <= last_visible {
                    let t = lines[scan].trim();
                    if line_types::is_act_scene_marker(t)
                        || line_types::is_separator(t)
                        || t.is_empty()
                        || line_types::is_stage_direction(t)
                    {
                        scan += 1;
                    } else {
                        break;
                    }
                }
                for i in scan..=last_visible {
                    let t = lines[i].trim();
                    assert!(
                        !line_types::is_act_scene_marker(t) && !line_types::is_separator(t),
                        "{}: scene break at line {} ('{}') is mid-page (page_top={} last_visible={})",
                        name, i, t, page_top, last_visible
                    );
                }
                match next_page_top_clamped(&lines, page_top, page_size) {
                    Some((new_top, _)) => page_top = new_top,
                    None => break,
                }
            }
        }
    }

    // --- Tests for y after structural jumps (push-before-clear) ---

    /// After a scene jump (`3`), the back-stack is CLEARED (no origin push):
    /// `y` should page back one viewport into the skipped content via
    /// prev_page_top, NOT teleport to the page where `3` was pressed. A scene
    /// jump can skip many pages (mid-scene -> EPILOGUE), so origin-return would
    /// hide everything between. Verify the stack is empty after a scene jump.
    #[test]
    fn test_y_after_scene_jump_pages_back_all_shakespeare() {
        let files = shakespeare_play_files();
        if files.is_empty() {
            eprintln!("SKIP: no Shakespeare files found");
            return;
        }
        let page_size = 30;
        for path in &files {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let lines = load_play_lines(path);
            let line_count = lines.len();
            if line_count == 0 { continue; }
            let markers = scene_marker_indices(&lines);
            if markers.len() < 3 { continue; }
            let first = match next_dialogue(&lines, 0) {
                Some(d) => d,
                None => continue,
            };
            let mut page_top = back_up_for_speaker(&lines, first);
            let mut stack: Vec<usize> = Vec::new();
            for _ in 0..5 {
                match next_page_top_clamped(&lines, page_top, page_size) {
                    Some((new_top, _)) => {
                        stack.push(page_top);
                        page_top = new_top;
                    }
                    None => break,
                }
            }
            if stack.is_empty() { continue; }
            // Simulate scene jump (3): clear stack, DO NOT push (new behavior).
            stack.clear();
            // y on an empty stack falls through to prev_page_top (one viewport
            // back) — model that here as "stack is empty so no teleport".
            assert!(
                stack.is_empty(),
                "{}: scene jump must leave an empty back-stack so y pages back",
                name
            );
        }
    }

    /// y after chapter jump ([/{) returns to the jump origin.
    #[test]
    fn test_y_after_chapter_jump_returns_to_origin() {
        let lines = load_troilus_lines();
        let page_size = 30;
        // Find chapter-like markers (act/scene markers serve as chapter boundaries)
        let markers = scene_marker_indices(&lines);
        assert!(markers.len() >= 3, "Expected at least 3 scene markers");
        // Page forward to build up a position
        let first = next_dialogue(&lines, 0).unwrap();
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut stack: Vec<usize> = Vec::new();
        for _ in 0..8 {
            match next_page_top_clamped(&lines, page_top, page_size) {
                Some((new_top, _)) => {
                    stack.push(page_top);
                    page_top = new_top;
                }
                None => break,
            }
        }
        let origin = page_top;
        // Jump forward to next chapter/scene marker
        let current_line = next_dialogue(&lines, page_top).unwrap();
        let target = *markers.iter().find(|&&m| m > current_line).unwrap();
        // Simulate [/{ jump: clear + push
        stack.clear();
        stack.push(origin);
        let _ = target;
        // y returns to origin
        assert_eq!(stack.pop().unwrap(), origin);
    }

    /// x x x 3 y returns to the page before the scene jump.
    /// x x x 3 y y falls through (empty stack).
    #[test]
    fn test_x_x_x_scene_jump_y_y_sequence() {
        let lines = load_troilus_lines();
        let page_size = 30;
        let markers = scene_marker_indices(&lines);
        assert!(markers.len() >= 3);
        let first = next_dialogue(&lines, 0).unwrap();
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut stack: Vec<usize> = Vec::new();
        let mut tops: Vec<usize> = vec![page_top];
        // x x x
        for _ in 0..3 {
            match next_page_top_clamped(&lines, page_top, page_size) {
                Some((new_top, _)) => {
                    stack.push(page_top);
                    page_top = new_top;
                    tops.push(page_top);
                }
                None => break,
            }
        }
        let before_jump = page_top;
        // 3 (scene jump)
        let current = next_dialogue(&lines, page_top).unwrap();
        let target = *markers.iter().find(|&&m| m > current).unwrap();
        stack.clear();
        stack.push(before_jump);
        let _ = target;
        // y — returns to before_jump
        let popped = stack.pop().unwrap();
        assert_eq!(popped, before_jump, "first y should return to page before scene jump");
        let _ = popped;
        // y again — stack is empty, would fall through to prev_page_top
        assert!(stack.is_empty(), "second y should have empty stack");
    }

    /// 3 3 y returns to the page before the SECOND scene jump (not the first).
    #[test]
    fn test_chained_scene_jumps_only_last_origin_survives() {
        let lines = load_troilus_lines();
        let page_size = 30;
        let markers = scene_marker_indices(&lines);
        assert!(markers.len() >= 5);
        let first = next_dialogue(&lines, 0).unwrap();
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut stack: Vec<usize> = Vec::new();
        // Page forward a bit
        for _ in 0..3 {
            match next_page_top_clamped(&lines, page_top, page_size) {
                Some((new_top, _)) => {
                    stack.push(page_top);
                    page_top = new_top;
                }
                None => break,
            }
        }
        // First scene jump (3)
        let current = next_dialogue(&lines, page_top).unwrap();
        let target1 = *markers.iter().find(|&&m| m > current).unwrap();
        stack.clear();
        stack.push(page_top);
        page_top = target1;
        let after_first_jump = page_top;
        // Second scene jump (3)
        let current2 = next_dialogue(&lines, page_top).unwrap();
        let target2 = *markers.iter().find(|&&m| m > current2).unwrap();
        stack.clear();
        stack.push(after_first_jump);
        let _ = target2;
        // y — returns to after_first_jump (not the original position)
        let popped = stack.pop().unwrap();
        assert_eq!(
            popped, after_first_jump,
            "y after 3 3 should return to page between the two jumps"
        );
    }

    /// x/y round-trip with section-break clamping across all Shakespeare plays.
    /// Forward tops must be strictly increasing; backward via stack must
    /// round-trip exactly.
    #[test]
    fn test_x_y_roundtrip_with_clamping_all_shakespeare() {
        let files = shakespeare_play_files();
        if files.is_empty() {
            eprintln!("SKIP: no Shakespeare files found");
            return;
        }
        let page_size = 30;
        for path in &files {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let lines = load_play_lines(path);
            let line_count = lines.len();
            if line_count == 0 { continue; }
            let first = match next_dialogue(&lines, 0) {
                Some(d) => d,
                None => continue,
            };
            let mut page_top = back_up_for_speaker(&lines, first);
            let mut forward_tops: Vec<usize> = vec![page_top];
            let mut stack: Vec<usize> = Vec::new();
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 2000 { break; }
                match next_page_top_clamped(&lines, page_top, page_size) {
                    Some((new_top, _)) => {
                        stack.push(page_top);
                        page_top = new_top;
                        forward_tops.push(page_top);
                    }
                    None => break,
                }
            }
            // Verify forward tops strictly increasing
            for i in 1..forward_tops.len() {
                assert!(
                    forward_tops[i] > forward_tops[i - 1],
                    "{}: forward top {} not after {} at page {}",
                    name, forward_tops[i], forward_tops[i - 1], i
                );
            }
            // Backward via stack
            let mut backward_tops: Vec<usize> = vec![page_top];
            while let Some(prev) = stack.pop() {
                page_top = prev;
                backward_tops.push(page_top);
            }
            assert_eq!(
                forward_tops.len(), backward_tops.len(),
                "{}: forward {} pages but backward {} pages",
                name, forward_tops.len(), backward_tops.len()
            );
            for i in 0..forward_tops.len() {
                assert_eq!(
                    forward_tops[i],
                    backward_tops[backward_tops.len() - 1 - i],
                    "{}: round-trip mismatch at page {}", name, i
                );
            }
        }
    }
}

#[cfg(test)]
mod after_page_change_tests {
    use super::PageChangeReason;

    #[test]
    fn reason_drives_seek_for_user_navigation() {
        assert!(PageChangeReason::Forward.should_seek());
        assert!(PageChangeReason::Backward.should_seek());
        assert!(PageChangeReason::JumpToLine.should_seek());
        assert!(PageChangeReason::JumpToBookmark.should_seek());
        assert!(PageChangeReason::Chapter.should_seek());
        assert!(PageChangeReason::Scene.should_seek());
    }

    #[test]
    fn reason_skips_seek_for_system_driven_changes() {
        assert!(!PageChangeReason::MpvSync.should_seek(),
            "MPV-driven page change must not re-seek MPV");
        assert!(!PageChangeReason::Resnap.should_seek(),
            "resnap is a layout refresh, not a navigation");
        assert!(!PageChangeReason::WorkLoad.should_seek(),
            "work load drives its own seek separately");
    }

    #[test]
    fn reason_drives_vocab_popup_for_user_navigation() {
        assert!(PageChangeReason::Forward.should_show_vocab());
        assert!(PageChangeReason::JumpToBookmark.should_show_vocab());
    }

    #[test]
    fn reason_skips_vocab_for_system_changes() {
        assert!(!PageChangeReason::MpvSync.should_show_vocab());
        assert!(!PageChangeReason::Resnap.should_show_vocab());
        assert!(!PageChangeReason::WorkLoad.should_show_vocab());
    }

    #[test]
    fn reason_skips_seek_for_cursor_only_navigation() {
        assert!(!PageChangeReason::Cursor.should_seek(),
            "cursor-only navigation must not drag audio");
        assert!(PageChangeReason::Cursor.should_show_vocab(),
            "cursor navigation still shows vocab");
    }
}

#[cfg(test)]
mod speaker_turn_tests {
    use super::{adjacent_paragraph, current_block_first_line, speaker_turn_target, Direction};

    /// Build a fixture from compact tuples: (speaker, is_dialogue).
    /// `""` speaker means None (unmapped/chrome line).
    fn fixture(rows: &[(&str, bool)]) -> Vec<(Option<String>, bool)> {
        rows.iter()
            .map(|(sp, dlg)| {
                (if sp.is_empty() { None } else { Some(sp.to_string()) }, *dlg)
            })
            .collect()
    }

    /// Run speaker_turn_target against a fixture.
    fn target(rows: &[(Option<String>, bool)], from: usize, dir: Direction) -> Option<usize> {
        speaker_turn_target(
            rows.len(),
            from,
            dir,
            |i| rows[i].0.clone(),
            |i| rows[i].1,
        )
    }

    // Sequence with wrapped continuation lines, a stage direction, and a
    // re-appearing speaker:  A A [stage] B B A C C
    fn sample() -> Vec<(Option<String>, bool)> {
        fixture(&[
            ("A", true),   // 0
            ("A", true),   // 1  (wrapped continuation of A)
            ("", false),   // 2  [stage direction] — unmapped
            ("B", true),   // 3
            ("B", true),   // 4  (wrapped continuation of B)
            ("A", true),   // 5  (A speaks again)
            ("C", true),   // 6
            ("C", true),   // 7  (wrapped continuation of C)
        ])
    }

    #[test]
    fn next_lands_on_first_line_of_next_turn() {
        let v = sample();
        assert_eq!(target(&v, 0, Direction::Next), Some(3));
        assert_eq!(target(&v, 4, Direction::Next), Some(5));
        assert_eq!(target(&v, 5, Direction::Next), Some(6));
    }

    #[test]
    fn next_returns_none_at_last_turn() {
        let v = sample();
        assert_eq!(target(&v, 6, Direction::Next), None);
        assert_eq!(target(&v, 7, Direction::Next), None);
    }

    #[test]
    fn prev_lands_on_first_line_of_previous_turn() {
        let v = sample();
        assert_eq!(target(&v, 7, Direction::Prev), Some(5));
        assert_eq!(target(&v, 5, Direction::Prev), Some(3));
        assert_eq!(target(&v, 4, Direction::Prev), Some(0));
        assert_eq!(target(&v, 3, Direction::Prev), Some(0));
    }

    #[test]
    fn prev_returns_none_at_first_turn() {
        let v = sample();
        assert_eq!(target(&v, 0, Direction::Prev), None);
        assert_eq!(target(&v, 1, Direction::Prev), None);
    }

    #[test]
    fn none_speaker_origin_treats_any_some_as_different() {
        let v = fixture(&[("", false), ("", true), ("A", true), ("A", true), ("B", true)]);
        assert_eq!(target(&v, 0, Direction::Next), Some(2));
    }

    #[test]
    fn prose_with_no_speakers_is_noop_both_directions() {
        let v = fixture(&[("", true), ("", true), ("", true)]);
        assert_eq!(target(&v, 1, Direction::Next), None);
        assert_eq!(target(&v, 1, Direction::Prev), None);
    }

    #[test]
    fn adjacent_paragraph_steps_and_skips_blanks() {
        // false = non-blank paragraph, true = blank separator.
        // lines: 0=para 1=blank 2=para 3=para 4=blank 5=para
        let blank = [false, true, false, false, true, false];
        let is_blank = |i: usize| blank[i];
        // Next from a paragraph skips the following blank.
        assert_eq!(adjacent_paragraph(6, 0, Direction::Next, is_blank), Some(2));
        // Next from an adjacent-paragraph pair is just the next line.
        assert_eq!(adjacent_paragraph(6, 2, Direction::Next, is_blank), Some(3));
        // Next skips the trailing blank to the last paragraph.
        assert_eq!(adjacent_paragraph(6, 3, Direction::Next, is_blank), Some(5));
        // Prev skips a preceding blank.
        assert_eq!(adjacent_paragraph(6, 5, Direction::Prev, is_blank), Some(3));
        // Edges: no paragraph after the last / before the first.
        assert_eq!(adjacent_paragraph(6, 5, Direction::Next, is_blank), None);
        assert_eq!(adjacent_paragraph(6, 0, Direction::Prev, is_blank), None);
    }

    #[test]
    fn non_dialogue_lines_are_never_a_target() {
        let v = fixture(&[("A", true), ("", false), ("B", true)]);
        assert_eq!(target(&v, 0, Direction::Next), Some(2));
        assert_eq!(target(&v, 2, Direction::Prev), Some(0));
    }

    /// Run current_block_first_line against a fixture.
    fn block_first(rows: &[(Option<String>, bool)], from: usize) -> Option<usize> {
        current_block_first_line(from, |i| rows[i].0.clone(), |i| rows[i].1)
    }

    #[test]
    fn block_first_returns_top_of_current_speech() {
        // A A [stage] B B A C C  (same fixture as `sample`).
        let v = sample();
        // Mid-block B -> top of the B run (3).
        assert_eq!(block_first(&v, 4), Some(3));
        // Mid-block C -> top of the C run (6).
        assert_eq!(block_first(&v, 7), Some(6));
        // Already at a block top returns itself (caller then falls through).
        assert_eq!(block_first(&v, 0), Some(0));
        assert_eq!(block_first(&v, 3), Some(3));
        assert_eq!(block_first(&v, 5), Some(5));
    }

    #[test]
    fn block_first_crosses_embedded_stage_direction() {
        // One speaker's turn split by a standalone stage direction (None):
        //   B  H H [stage] H H  C
        // From the last H (5), the block top is the FIRST H of the turn (1),
        // crossing the [stage] at 3 — a stage direction inside a speech is not
        // a turn boundary; only the speaker label (B->H at 0->1) is.
        let v = fixture(&[
            ("B", true),   // 0  previous speaker
            ("H", true),   // 1  <- true turn start
            ("H", true),   // 2
            ("", false),   // 3  [stage direction] — standalone, None
            ("H", true),   // 4  H resumes (after the stage direction)
            ("H", true),   // 5  cursor here
            ("C", true),   // 6  next speaker
        ]);
        assert_eq!(block_first(&v, 5), Some(1));
        assert_eq!(block_first(&v, 4), Some(1));
        // The line right after the stage direction is NOT the block top.
        assert_ne!(block_first(&v, 5), Some(4));
    }

    #[test]
    fn block_first_stops_at_unmapped_chrome_line() {
        // A wrapped continuation walking back must not cross the [stage] gap
        // into the previous speaker. From line 1 (second A) -> 0, not past it.
        let v = sample();
        assert_eq!(block_first(&v, 1), Some(0));
    }

    #[test]
    fn block_first_none_for_non_dialogue_cursor() {
        // Cursor on a chrome/stage line has no block to anchor to -> None,
        // so jump_to_prev_speaker falls straight through to the prev-speaker scan.
        let v = sample();
        assert_eq!(block_first(&v, 2), None);
        // Also None when the line is dialogue=true but has no speaker.
        let p = fixture(&[("", true), ("", true)]);
        assert_eq!(block_first(&p, 1), None);
    }
}
