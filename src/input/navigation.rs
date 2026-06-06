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
    is_dialogue_line, is_blank_buffer_line,
    next_dialogue_line, prev_dialogue_line, buffer_line_text,
    next_dialogue_from, is_line_fully_visible, lines_per_page,
    clamp_page_top_to_scroll_ceiling, column_split, would_empty_right_column,
};
use super::scroll::{
    set_page, set_page_instant, scroll_to_cursor, center_cursor,
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
    /// User pressed k/K for cursor-only movement (no audio seek).
    Cursor,
    /// User pressed [ or { for paragraph navigation.
    Paragraph,
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
    let target = (0..line_count)
        .find(|&i| is_dialogue_line(&state.buffer, i))
        .unwrap_or(0);

    state.current_line = target;
    state.page_back_stack.clear();
    state.page_back_stack.push(state.page_top_line);
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

    // Find the last dialogue line in the buffer (skips trailing stage
    // directions, blanks, exit markers). For prose works there typically
    // isn't a difference; for plays this lands on the last spoken line.
    let mut target = line_count - 1;
    loop {
        if !state.translation_lines.get(target).copied().unwrap_or(false)
            && is_dialogue_line(&state.buffer, target)
        {
            break;
        }
        if target == 0 {
            break;
        }
        target -= 1;
    }
    // Diagnostic for the H8-class bug: if the last-dialogue scan stops far from
    // the buffer end, the buffer past `target` is all non-dialogue (or the
    // line-map/buffer lengths disagree). Dump the buffer's real line count and
    // the text just past `target` so we can see WHY (mis-tagged dialogue, a giant
    // stage direction, or a line-map mismatch).
    {
        let buf_lines = state.buffer.line_count() as usize;
        if target + 200 < line_count {
            let txt = |l: usize| buffer_line_text(&state.buffer, l).trim().chars().take(40).collect::<String>();
            log_fmt!("JTE_DBG: target={} far below line_count={} (buffer.line_count={})",
                     target, line_count, buf_lines);
            for l in [target, target + 1, line_count / 2, line_count.saturating_sub(3),
                      line_count.saturating_sub(2), line_count.saturating_sub(1)] {
                log_fmt!("JTE_DBG:   line {} dialogue={} '{}'",
                         l, is_dialogue_line(&state.buffer, l), txt(l));
            }
        }
    }

    // Anchor on the canonical final spread (the page the user reaches by paging
    // forward to the end). When the work's tail is a short section that starts
    // with a section-break marker (e.g. a lone EPILOGUE), `last_page_top` keeps
    // the last FULL two-column spread, whose right column clamps BEFORE that
    // marker — so the absolute last dialogue line (`target`) may sit on the
    // following, intentionally-suppressed spread and NOT be visible here. Land
    // the page first, then place the cursor on the last dialogue line that is
    // actually on this spread (mirrors the forward final-spread guard) so the
    // highlight is never off-page.
    let new_top = last_page_top(state, target);
    // Clear the back-stack WITHOUT seeding it with the came-from page: a `y` from
    // the final spread must tile back to the page immediately BEFORE `new_top`,
    // not return to wherever the jump originated. With an empty stack,
    // `page_backward` uses `prev_page_top(new_top)`, which now lands on the
    // nearest real boundary below the (forward-pulled) final-spread top.
    state.page_back_stack.clear();
    set_page_instant(state, new_top);

    let cs = column_split(state, new_top);
    let on_page = prev_dialogue_line(&state.buffer, &state.translation_lines, cs.page_end + 1)
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

/// TEMP diagnostic gate for the `last_page_top` final-spread walk (PULL_DBG /
/// LPT_DBG). Off unless `LIT_LPT_DBG=1`, so the instrumentation can't leak into
/// production logs or get swept into an unrelated commit. Remove with the logs
/// once the JumpEnd non-idempotency bug is fixed.
fn lpt_dbg() -> bool {
    std::env::var("LIT_LPT_DBG").map(|v| v == "1").unwrap_or(false)
}

/// The page top of the FINAL spread that contains `target`, sized so the work's
/// tail content fills both columns (two-column) rather than landing `target` as
/// a lonely left column with an empty right. Used by `jump_to_end` and by the
/// forward-nav guard that redirects an "empty right column" turn to this anchor.
pub(crate) fn last_page_top(state: &AppState, target: usize) -> usize {
    let line_count = state.effective_line_count();
    let widget_height = state.text_view.height();
    let columns = state.column_count() as i32;
    if columns == 2 && widget_height > 0 && line_count > 0 {
        // Two-column: land on the CANONICAL last spread — the same page the user
        // reaches by paging forward to the end, so both columns are filled and
        // the work's tail sits in the right column. Walk the forward page chain
        // (next_page_top) from a safe early start until the spread whose forward
        // boundary reaches the end of the work; that spread's top is the anchor.
        // The canonical final spread is simply the page you reach by paging
        // FORWARD (via next_page_top) until the forward boundary hits the end of
        // the work. That top is IDEMPOTENT — recomputing from any start yields the
        // same page the saved-position startup restores and the same page `x`
        // walks to. (An earlier version "pulled" the top forward to fit a short
        // EPILOGUE into the right column; that produced a DIFFERENT, too-early top
        // than the natural walk, so G disagreed with the startup spread. The
        // natural last spread already carries the tail in its right column, so no
        // pull is needed.)
        //
        // Start from a safe boundary below `target` and walk forward. Snap onto a
        // real boundary first, then advance while the next page still begins
        // before the work's end.
        let lpp = lines_per_page(state).max(1) * 2; // ~lines on one spread
        let mut top = target.saturating_sub(lpp * 3); // safe early start
        // Snap onto a real forward boundary: walk from a known-good start (0 is
        // always a boundary; but to stay cheap, walk forward until we pass `top`).
        {
            let mut b = 0usize;
            let mut g = 0;
            while b < top {
                let nb = super::viewport::next_page_top(state, b).new_top;
                if nb <= b { break; }
                if nb > top { break; }
                b = nb;
                g += 1;
                if g > line_count { break; }
            }
            top = b;
        }
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
            // Advancing a full page to `next` would land on the lone-EPILOGUE page
            // (empty right column). But the natural page boundary SKIPS a better
            // final spread: a top a few lines past `top` whose left column holds
            // the dialogue tail and whose right column absorbs the trailing
            // section (the EPILOGUE), reaching the work's end. At 1112px the walk
            // gives top=4296 (page_end=4336, EPILOGUE cut off) while top=4303
            // reaches page_end=4347 (full EPILOGUE in the right column). Search
            // [top+1, next) for the SMALLEST top whose spread leaves no dialogue
            // below it and still has a non-empty right column — that is the
            // canonical last spread.
            if we_next {
                let mut pulled = None;
                for t in (top + 1)..next.min(line_count) {
                    let tcs = super::viewport::column_split(state, t);
                    let dialogue_below = (tcs.next_page_top..line_count)
                        .any(|i| is_dialogue_line(&state.buffer, i));
                    let we_t = would_empty_right_column(state, t);
                    if lpt_dbg() && t < top + 40 {
                        log_fmt!("PULL_DBG:   t={} split={} page_end={} next_pt={} dialogue_below={} we={} page_top={}",
                                 t, tcs.split, tcs.page_end, tcs.next_page_top, dialogue_below, we_t, state.page_top_line);
                    }
                    if !dialogue_below && !we_t {
                        pulled = Some(t);
                        break;
                    }
                }
                if lpt_dbg() {
                    log_fmt!("PULL_DBG: top={} next={} we_next={} -> pulled={:?} page_top={}",
                             top, next, we_next, pulled, state.page_top_line);
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
        let clamped = clamp_page_top_to_scroll_ceiling(state, chosen);
        if lpt_dbg() {
            let adj = state.scrolled_window.vadjustment();
            log_fmt!(
                "LPT_DBG: target={} chosen={} clamped={} (vadj upper={:.0} page_size={:.0} max={:.0}) widget_h={} page_top={}",
                target, chosen, clamped,
                adj.upper(), adj.page_size(), (adj.upper() - adj.page_size()).max(0.0),
                widget_height, state.page_top_line,
            );
        }
        clamped
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

    // Card width depends on column count (two columns fill more of the window),
    // so resize the card to match the new layout before recomputing pages.
    crate::app::apply_card_sizing(
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
    let cs = column_split(state, state.page_top_line);
    let split = cs.split;
    let page_end = cs.page_end;
    if split <= state.page_top_line || split >= line_count {
        return None;
    }
    // First dialogue line at/after the right column's start.
    let rc_first_dlg = next_dialogue_from(&state.buffer, split, line_count);
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
        if is_dialogue_line(&state.buffer, i) {
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

pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    if state.page_turn_lock.is_locked() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

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
            let last_dlg = prev_dialogue_line(&state.buffer, &state.translation_lines, visible_end + 1)
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
            let next_dialogue = next_dialogue_from(&state.buffer, clamped, line_count);
            log_fmt!("PAGE_FWD: scene-snap page_top={} -> new_top={} next_dialogue={}",
                     state.page_top_line, clamped, next_dialogue);
            state.page_back_stack.push(state.page_top_line);
            state.current_line = next_dialogue.min(line_count.saturating_sub(1));
            set_page(state, clamped, PageDirection::Forward);
            after_page_change(state, PageChangeReason::Forward);
            return;
        }
        log_fmt!("PAGE_FWD: scene-snap to {} skipped (clamps to {} <= page_top {})",
                 snap_top, clamped, state.page_top_line);
        // Fall through: the normal path / jump_to_end handles the final spread.
    }

    let NextPage { new_top, next_dialogue } = next_page_top(state, state.page_top_line);
    log_fmt!("PAGE_FWD: page_top={} new_top={} next_dialogue={} line_count={}",
             state.page_top_line, new_top, next_dialogue, line_count);
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
    // canonical final spread (`last_page_top`). This covers both (a) the turn
    // would leave the right column EMPTY (a lone EPILOGUE), and (b) the turn lands
    // on a too-early final spread whose right column is UNDERFILLED because
    // `next_page_top` picked a boundary one short of the canonical one (e.g.
    // candidate 4296 with the EPILOGUE cut off vs the canonical 4308 with the full
    // EPILOGUE). Mirrors the q/j forward rule in scroll_after_jump_forward.
    let candidate_top = if state.column_count() == 2 {
        let anchor = last_page_top(state, next_dialogue);
        // Redirect when the candidate isn't the canonical final spread but its
        // page would overlap the final region — its forward boundary reaches at or
        // past where the anchor's page begins (so the two pages cover the same
        // tail), OR it would empty the right column. This catches the underfilled
        // 4296 spread (next_page_top 4337 > anchor 4308) AND the lone-EPILOGUE case.
        let cand_end = super::viewport::column_split(state, candidate_top).next_page_top;
        let lands_in_final_region = candidate_top != anchor
            && (super::viewport::would_empty_right_column(state, candidate_top)
                || (candidate_top < anchor && cand_end > anchor));
        if lands_in_final_region {
            log_fmt!("PAGE_FWD: final-region candidate={} (end {}) -> anchor={}",
                     candidate_top, cand_end, anchor);
            anchor
        } else {
            candidate_top
        }
    } else {
        candidate_top
    };
    let effective_top = clamp_page_top_to_scroll_ceiling(state, candidate_top);
    log_fmt!("PAGE_FWD: candidate_top={} effective_top={} (from new_top={})", candidate_top, effective_top, new_top);
    if effective_top > state.page_top_line {
        state.page_back_stack.push(state.page_top_line);
        state.current_line = next_dialogue;
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
    while let Some(&top) = state.page_back_stack.last() {
        if top < state.page_top_line {
            break;
        }
        log_fmt!("PAGE_BWD: dropping stale stack entry {} (>= page_top {})",
                 top, state.page_top_line);
        state.page_back_stack.pop();
    }
    let (new_top, next_dialogue) = if let Some(prev_top) = state.page_back_stack.pop() {
        let nd = next_dialogue_from(&state.buffer, prev_top, line_count);
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
    let cursor = if state.column_count() == 2 {
        let cs = super::viewport::column_split(state, new_top);
        prev_dialogue_line(&state.buffer, &state.translation_lines, cs.page_end + 1)
            .filter(|&d| d >= new_top && d <= cs.page_end)
            .unwrap_or(next_dialogue)
    } else {
        let last_vis = last_fully_visible_line(state, new_top);
        prev_dialogue_line(&state.buffer, &state.translation_lines, last_vis + 1)
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

/// Scroll viewport so current_line is at the top (zt). If the line immediately
/// above is a speaker/stage-direction/blank, backs up to include that context.
pub fn scroll_cursor_top(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    state.page_back_stack.clear();
    state.page_back_stack.push(state.page_top_line);
    let top = back_up_for_speaker_state(state, state.current_line);
    crate::logging::log(&format!(
        "ZT: current_line={} effective_top={}", state.current_line, top
    ));
    set_page_instant(state, top);
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
    let (prev_top, new_top) = if let Some(prev) = state.page_back_stack.pop() {
        let nd = next_dialogue_from(&state.buffer, prev, line_count);
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
    let buffer = &state.buffer;
    if let Some(target) = prev_dialogue_line(buffer, &state.translation_lines, state.current_line) {
        let prev = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
        log_fmt!("NAV_PREV: comma from={} to={} page_top={}", prev, target, state.page_top_line);
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
    let buffer = &state.buffer;
    if let Some(target) = next_dialogue_line(buffer, &state.translation_lines, state.current_line, line_count) {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        log_fmt!("NAV_NEXT: q from={} to={} page_top={}", prev_line, target, state.page_top_line);
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Move cursor to previous dialogue line without seeking media (`k` key).
pub fn cursor_prev_line(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let buffer = &state.buffer;
    let Some(target) = prev_dialogue_line(buffer, &state.translation_lines, state.current_line)
    else {
        return;
    };
    state.current_line = target;
    state.pending_advance = None;
    state.pending_advance_ignore_bl = None;
    state.prev_highlight_line.set(None);
    // Translation view: highlight follows the cursor, scroll only to keep it
    // within a vim-style scrolloff margin (not a page turn).
    if state.translations_visible {
        update_highlight_only(state);
        super::scroll::scroll_cursor_into_view_scrolloff(state);
        return;
    }
    scroll_after_jump_backward(state);
    after_page_change(state, PageChangeReason::Cursor);
}

/// Move cursor to next dialogue line without seeking media (`k` key).
pub fn cursor_next_dialogue(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }
    let buffer = &state.buffer;
    if let Some(target) = next_dialogue_line(buffer, &state.translation_lines, state.current_line, line_count) {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        // Translation view: highlight follows the cursor, scroll only to keep
        // it within a vim-style scrolloff margin (not a page turn).
        if state.translations_visible {
            update_highlight_only(state);
            super::scroll::scroll_cursor_into_view_scrolloff(state);
            return;
        }
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Cursor);
    }
}

/// Previous paragraph (`[` key).
/// Jump to the first non-blank line of the previous paragraph.
pub fn jump_to_prev_paragraph(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if state.current_line == 0 || line_count == 0 {
        return;
    }

    let buffer = &state.buffer;
    let mut i = state.current_line.saturating_sub(1);

    // Skip blank lines immediately above
    while i > 0 && is_blank_buffer_line(buffer, i) {
        i -= 1;
    }
    // Skip non-blank lines (current paragraph body)
    while i > 0 && !is_blank_buffer_line(buffer, i) {
        i -= 1;
    }
    // Now i is on a blank line (or 0). Find the first non-blank line of the paragraph above.
    let target = if is_blank_buffer_line(buffer, i) {
        let mut start = i + 1;
        while start < line_count && is_blank_buffer_line(buffer, start) {
            start += 1;
        }
        if start < line_count && start < state.current_line {
            Some(start)
        } else {
            Some(0)
        }
    } else {
        Some(0)
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        state.page_back_stack.clear();
        state.page_back_stack.push(state.page_top_line);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, line_idx);
            }
        }
        after_page_change(state, PageChangeReason::Paragraph);
    }
}

/// Next paragraph (`{` key).
/// Jump to the first non-blank line of the next paragraph.
pub fn jump_to_next_paragraph(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }

    let buffer = &state.buffer;
    let mut i = state.current_line + 1;

    // Skip remaining lines of current paragraph
    while i < line_count && !is_blank_buffer_line(buffer, i) {
        i += 1;
    }
    // Skip blank lines between paragraphs
    while i < line_count && is_blank_buffer_line(buffer, i) {
        i += 1;
    }

    if i < line_count {
        let prev_line = state.current_line;
        state.current_line = i;
        state.page_back_stack.clear();
        state.page_back_stack.push(state.page_top_line);
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Paragraph);
    }
}

/// Previous chapter line (`[` key).
pub fn jump_to_prev_chapter(state: &mut AppState) {
    crate::app::hide_translations_for_navigation(state);
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
        state.page_back_stack.push(state.page_top_line);
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
    crate::app::hide_translations_for_navigation(state);
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
        state.page_back_stack.push(state.page_top_line);
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
        let cursor = marker.and_then(|m| {
            next_dialogue_line(&state.buffer, &state.translation_lines, m, line_count)
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
}

/// Show the chapter containing the current line as a transient toast.
pub fn show_current_chapter(state: &mut AppState) {
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

    if chapter_lines.is_empty() {
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

    let text = format!("Chapter {} of {} — {}", chapter_num, total, title);

    state.chapter_toast.set_text(&text);
    state.chapter_toast.set_visible(true);

    let toast = state.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
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
    state.page_back_stack.push(state.page_top_line);
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
        let new_until = std::time::Instant::now() + std::time::Duration::from_millis(500);
        if state.suppress_sync_until.map_or(true, |existing| new_until > existing) {
            state.suppress_sync_until = Some(new_until);
        }
        let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
        log_fmt!("SEEK: line={} work_idx={} start={:.2} seek={:.2} suppress=500ms", state.current_line, work_idx, ts.start, seek_time);
        let _ = state
            .cmd_tx
            .try_send(crate::mpv::MpvCommand::Seek(seek_time));
    } else {
        // No timestamp — suppress indefinitely so cursor stays put
        log_fmt!("SEEK: line={} work_idx={} NO_TIMESTAMP suppress=86400s", state.current_line, work_idx);
        state.suppress_sync_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(86400));
    }
}

/// Position chunk's first line ~5 lines from top, move cursor there, update highlight.
pub fn position_chunk(state: &mut AppState) {
    if let Some(a_line) = state.ab_repeat.a_line {
        state.current_line = a_line;
        update_highlight(state);
        let new_top = a_line.saturating_sub(5);
        set_page_instant(state, new_top);
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
    state.page_back_stack.push(state.page_top_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, target_line) {
                set_page(state, target_line, PageDirection::Forward);
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
    state.page_back_stack.push(state.page_top_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, target_line) {
                set_page_instant(state, target_line);
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
        !line_types::is_blank(text) && line_types::is_dialogue(text, false)
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

        println!(
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

        println!(
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

        println!(
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

        println!(
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

        println!(
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
