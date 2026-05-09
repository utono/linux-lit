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
    back_up_for_speaker, page_turn_top, chapter_page_top,
    is_dialogue_line, is_blank_buffer_line,
    next_dialogue_line, prev_dialogue_line, buffer_line_text,
    is_line_fully_visible, lines_per_page,
    clamp_page_top_to_scroll_ceiling,
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
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    let target = if let Some(ref lm) = state.line_map {
        lm.dialogue_buffer_lines.first().copied().unwrap_or(0)
    } else {
        work.lines
            .iter()
            .position(|l| l.is_dialogue)
            .unwrap_or(0)
    };

    state.current_line = target;
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
    state.current_line = target;

    // Compute new_top such that line `new_top`'s y can actually be reached
    // by GTK's vadjustment (no clamping). The constraint:
    //   y(new_top) <= upper - page_size
    //              = (top_margin + content_height + bottom_margin) - page_size
    // Equivalently, the sum of line heights from new_top to line_count-1
    // (i.e., `content_below_new_top`) must be >= page_size - bottom_margin
    // for line `new_top` to be scrollable to the viewport top.
    //
    // Walk backward from line_count-1, accumulating heights, stop as soon
    // as cumulative >= required. The smallest top satisfying this is the
    // anchor. Sidesteps the GTK scroll-clamp bug seen with the simple
    // `line_count - lpp` heuristic.
    let widget_height = state.text_view.height();
    let new_top = if widget_height > 0 && line_count > 0 {
        // For jump_to_end, the last buffer line is the last content. There's
        // no "next page" requiring descender_guard or bottom_margin headroom
        // — the buffer simply ends. So usable_height = full widget_height.
        // Walk backward from line_count - 1, accumulating heights; the
        // smallest top such that total <= widget_height is the new anchor.
        // This ensures (a) every line from new_top down to line_count - 1
        // fits in the viewport and (b) y(new_top) is reachable by
        // vadjustment.set_value (no clamp).
        let usable_height = widget_height;
        let mut total: i32 = 0;
        let mut top = line_count - 1;
        loop {
            let Some(iter) = state.buffer.iter_at_line(top as i32) else { break };
            let (_y, h) = state.text_view.line_yrange(&iter);
            if total + h > usable_height && top != line_count - 1 {
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
        // Layout not ready — fall back to lpp anchor.
        let lpp = lines_per_page(state);
        line_count.saturating_sub(lpp)
    };
    set_page_instant(state, new_top);
    after_page_change(state, PageChangeReason::JumpToLine);
}

/// Page forward (Ctrl+d/f). The next page starts at the dialogue line
/// immediately after the last dialogue line visible on the current page,
/// backed up by one if preceded by a speaker name.
pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

    let NextPage { new_top, next_dialogue } = next_page_top(state, state.page_top_line);
    if next_dialogue >= line_count {
        return; // already at end
    }

    let effective_top = clamp_page_top_to_scroll_ceiling(state, new_top);
    if effective_top > state.page_top_line {
        state.current_line = next_dialogue;
        set_page(state, effective_top, PageDirection::Forward);
        after_page_change(state, PageChangeReason::Forward);
        return;
    }

    // The next page boundary exceeds the scroll ceiling — delegate to
    // jump_to_end which computes the correct last-page anchor with
    // enough room to show all remaining content cleanly.
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

    if state.page_top_line == 0 {
        log_fmt!("PAGE_BWD: at start of work");
        return;
    }
    let np = prev_page_top(state, state.page_top_line);
    let (new_top, next_dialogue) = (np.new_top, np.next_dialogue);
    log_fmt!("PAGE_BWD: prev_page_top new_top={} next_dialogue={} from page_top={}",
             new_top, next_dialogue, state.page_top_line);

    state.current_line = next_dialogue;
    set_page(state, new_top, PageDirection::Backward);
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
    let top = back_up_for_speaker(&state.buffer, state.current_line);
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
    if state.page_top_line == 0 {
        log_fmt!("NAV_BACK_BOTTOM: at start of work");
        return;
    }
    let np = prev_page_top(state, state.page_top_line);
    log_fmt!("NAV_BACK_BOTTOM: prev_page_top new_top={} from page_top={}",
             np.new_top, state.page_top_line);
    let prev_top = np.new_top;
    let new_top = back_up_for_speaker(&state.buffer, prev_top);
    // Set page first so last_fully_visible_line computes against the new page
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
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Paragraph);
    }
}

/// Previous chapter line (`[` key).
pub fn jump_to_prev_chapter(state: &mut AppState) {
    if state.translations_visible {
        crate::app::toggle_translations(state);
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
        let top = chapter_page_top(&state.buffer, line_idx);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, top);
            }
        }
        after_page_change(state, PageChangeReason::Chapter);
    }
}

/// Next chapter line.
pub fn jump_to_next_chapter(state: &mut AppState) {
    if state.translations_visible {
        crate::app::toggle_translations(state);
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
        let top = chapter_page_top(&state.buffer, line_idx);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, top);
            }
        }
        after_page_change(state, PageChangeReason::Chapter);
    }
}

/// Previous scene marker (used for plays on the `2` key).
///
/// Walks backward from `current_line` looking for an act/scene marker, then
/// places the cursor on the first dialogue line of that scene. The viewport
/// top is pinned to the scene marker via `chapter_page_top` so the scene
/// header stays visible above the cursorline.
pub fn jump_to_prev_scene(state: &mut AppState) {
    use crate::db::line_types;
    let line_count = state.effective_line_count();
    let (marker, cursor) = {
        if state.current_line == 0 {
            return;
        }
        let is_marker_at = |bl: usize| -> bool {
            let text = buffer_line_text(&state.buffer, bl);
            line_types::is_act_scene_marker(text.trim())
        };
        let current_scene_start = (0..=state.current_line).rev().find(|&bl| is_marker_at(bl));
        let marker = match current_scene_start {
            Some(start) if start < state.current_line => {
                let cur_first = next_dialogue_line(
                    &state.buffer,
                    &state.translation_lines,
                    start,
                    line_count,
                );
                if cur_first.map(|d| d < state.current_line).unwrap_or(false) {
                    Some(start)
                } else {
                    (0..start).rev().find(|&bl| is_marker_at(bl))
                }
            }
            Some(start) => (0..start).rev().find(|&bl| is_marker_at(bl)),
            None => (0..state.current_line).rev().find(|&bl| is_marker_at(bl)),
        };
        let cursor = marker.and_then(|m| {
            let cap = ((m + 1)..line_count).find(|&bl| is_marker_at(bl))
                .unwrap_or(line_count);
            next_dialogue_line(&state.buffer, &state.translation_lines, m, cap)
                .or(Some(m))
        });
        (marker, cursor)
    };

    if let (Some(marker_idx), Some(cursor_idx)) = (marker, cursor) {
        state.current_line = cursor_idx;
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, marker_idx);
            }
        }
        after_page_change(state, PageChangeReason::Scene);
    }
}

/// Next scene marker (used for plays on the `3` key).
pub fn jump_to_next_scene(state: &mut AppState) {
    use crate::db::line_types;
    let line_count = state.effective_line_count();
    let (marker, cursor) = {
        let mut marker = None;
        for bl in (state.current_line + 1)..line_count {
            let text = buffer_line_text(&state.buffer, bl);
            if line_types::is_act_scene_marker(text.trim()) {
                marker = Some(bl);
                break;
            }
        }
        let cursor = marker.and_then(|m| {
            next_dialogue_line(&state.buffer, &state.translation_lines, m, line_count)
                .or(Some(m))
        });
        (marker, cursor)
    };

    if let (Some(marker_idx), Some(cursor_idx)) = (marker, cursor) {
        state.current_line = cursor_idx;
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, marker_idx);
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

/// Jump to the next bookmarked line (wraps around).
pub fn next_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || !is_bm.iter().any(|&b| b) {
        return;
    }
    let line_count = is_bm.len();
    for offset in 1..=line_count {
        let idx = (state.current_line + offset) % line_count;
        if is_bm[idx] {
            drop(is_bm);
            jump_to_line(state, idx);
            return;
        }
    }
}

/// Jump to the previous bookmarked line (wraps around).
pub fn prev_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || !is_bm.iter().any(|&b| b) {
        return;
    }
    let line_count = is_bm.len();
    for offset in 1..=line_count {
        let idx = (state.current_line + line_count - offset) % line_count;
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
    state.current_line = buffer_line;
    let top = page_turn_top(&state.buffer, buffer_line);
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
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => scroll_to_cursor(state),
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

    fn is_dialogue_line(text: &str) -> bool {
        !line_types::is_blank(text) && line_types::is_dialogue(text, false)
    }

    /// Collect all dialogue line indices in the file.
    fn dialogue_indices(lines: &[String]) -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, text)| is_dialogue_line(text))
            .map(|(i, _)| i)
            .collect()
    }

    /// Simulate next_dialogue_from on plain strings.
    fn next_dialogue(lines: &[String], from: usize) -> Option<usize> {
        for i in from..lines.len() {
            if is_dialogue_line(&lines[i]) {
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
            if is_dialogue_line(&lines[i]) {
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
                is_dialogue_line(&lines[h]),
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
