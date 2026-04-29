use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;

/// Seconds to seek before a line's start_time when navigating.
/// Provides audio context so playback doesn't start at a hard cut.
pub const SEEK_PREROLL: f64 = 0.2;

/// Seconds to highlight a line before playback actually reaches it.
/// Used by the MPV client's time-pos sync.
pub const SYNC_PREROLL: f64 = 0.0;

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
    // Clear page_history — y after gg should go to the next page forward
    // (via the natural empty-history path), not pop a stale entry.
    state.page_history.clear();
    let top = target.saturating_sub(1);
    set_page_instant(state, top);
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

    // Clear page_history — y after G should go to the PREVIOUS page (via
    // prev_page_top fallback), not back to wherever the user was before G.
    state.page_history.clear();

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

/// Find the next dialogue line at or after `from`.
fn next_dialogue_from(buffer: &sourceview5::Buffer, from: usize, line_count: usize) -> usize {
    use crate::db::line_types;
    for i in from..line_count {
        let text = buffer_line_text(buffer, i);
        if !line_types::is_blank(&text) && line_types::is_dialogue(&text, false) {
            return i;
        }
    }
    from
}

/// Find the last dialogue line in the range [from, from+count).
fn last_dialogue_in_page(buffer: &sourceview5::Buffer, from: usize, count: usize, line_count: usize) -> usize {
    use crate::db::line_types;
    let end = (from + count).min(line_count);
    let mut last = from;
    for i in from..end {
        let text = buffer_line_text(buffer, i);
        if !line_types::is_blank(&text) && line_types::is_dialogue(&text, false) {
            last = i;
        }
    }
    last
}

/// Given a dialogue line that will be the top of a page, back up over any
/// non-dialogue content immediately preceding it: blanks, speakers, stage
/// directions, and act/scene markers. Stops at the previous dialogue line or
/// the start of the buffer. Ensures scene-transition context (exit directions,
/// scene headers, entrance directions, asides) is visible at the top of the
/// new page rather than skipped between pages.
fn back_up_for_speaker(buffer: &sourceview5::Buffer, line: usize) -> usize {
    use crate::db::line_types;
    let mut top = line;
    while top > 0 {
        let prev = buffer_line_text(buffer, top - 1);
        let trimmed = prev.trim();
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

/// Find the last buffer line that fits within the viewport starting from
/// `top`, matching the bottom clip calculation exactly. A line is included
/// only if its full height fits in the remaining usable space (widget height
/// minus descender guard). This ensures page_forward doesn't count clipped
/// lines as "seen". Trailing speaker names and blanks are trimmed so a
/// dangling speaker at the bottom doesn't count as "visible" content.
fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return top;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, top);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    let is_prose = state.is_prose();
    // Trim because this function feeds page-boundary placement decisions
    // (next_page_top): we don't want to put a partial verse stanza or
    // dangling speaker at the BOTTOM of the new page.
    let trimmed = trim_visible_range(range, top, &state.text_view, &state.buffer, is_prose);
    trimmed.last_fit
}

/// Result of stepping forward one page from `top`: the new page-top
/// (after backing up for a speaker) and the next dialogue line that
/// would become `state.current_line`. Both equal `line_count` when there
/// is no further dialogue.
#[derive(Clone, Copy)]
struct NextPage {
    /// Page-top of the page that follows `top` — what `set_page` is called
    /// with. Equals `line_count` if past the last page.
    new_top: usize,
    /// First dialogue line on the next page — what `state.current_line`
    /// should become. Equals `line_count` if past the last page.
    next_dialogue: usize,
}

/// Compute the next-page boundary from `top`, using the same logic as
/// `page_forward`. Returns `{ line_count, line_count }` when there is no
/// further dialogue. Pure — does not mutate `state`.
fn next_page_top(state: &AppState, top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    if line_count == 0 || top >= line_count {
        return NextPage { new_top: line_count, next_dialogue: line_count };
    }
    let last_visible = last_fully_visible_line(state, top);
    let last = last_dialogue_in_page(
        &state.buffer,
        top,
        last_visible.saturating_sub(top) + 1,
        line_count,
    );
    let next_dialogue = next_dialogue_from(&state.buffer, last + 1, line_count);
    if next_dialogue >= line_count {
        return NextPage { new_top: line_count, next_dialogue: line_count };
    }
    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
    NextPage { new_top, next_dialogue }
}

/// Backward mirror of `next_page_top`. Returns the page boundary immediately
/// before `current_top`, with the same `back_up_for_speaker` + `next_dialogue`
/// post-processing the forward path uses.
///
/// Three-tier lookup:
/// 1. F8 `page_tops` cache — `binary_search` for the fast path (O(log n)).
/// 2. Cold-start linear walk from line 0 looking for the page whose
///    `next_page_top` equals `current_top`. O(page_count) one-time cost.
/// 3. Lpp approximation — pathological case where `current_top` is not on any
///    forward-walkable boundary (e.g., user resumed at an arbitrary line via
///    concordance). Preserves the historical fallback rather than refusing to
///    move.
///
/// Mirrors foliate-js's `atStart`/`atEnd` page-index lookups
/// (paginator.js:1050-1054) — exact previous boundary instead of approximation.
fn prev_page_top(state: &AppState, current_top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    if current_top == 0 || line_count == 0 {
        return NextPage { new_top: 0, next_dialogue: 0 };
    }

    // Tier 1: F8 cache fast path.
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            if let Ok(idx) = tops.binary_search(&current_top) {
                if idx > 0 {
                    let prev_top = tops[idx - 1];
                    let next_dialogue = next_dialogue_from(&state.buffer, prev_top, line_count);
                    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
                    return NextPage { new_top, next_dialogue };
                }
            }
        }
    }

    // Tier 2: cold-start linear walk from 0 looking for the page whose
    // next_page_top equals current_top.
    let mut top: usize = 0;
    while top < current_top {
        let next = next_page_top(state, top).new_top;
        if next == current_top {
            let next_dialogue = next_dialogue_from(&state.buffer, top, line_count);
            let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
            return NextPage { new_top, next_dialogue };
        }
        if next <= top {
            break; // safety: no progress
        }
        top = next;
    }

    // Tier 3: lpp approximation — current_top is not on any forward-walkable
    // boundary. Preserve historical behavior rather than refusing to move.
    let lpp = lines_per_page(state).max(1);
    let approx = current_top.saturating_sub(lpp);
    let next_dialogue = next_dialogue_from(&state.buffer, approx, line_count);
    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
    NextPage { new_top, next_dialogue }
}

/// Build the page_tops index by walking next_page_top from line 0 to the end
/// of the work. Result is the same as repeatedly calling next_page_top from
/// line 0; cost is O(line_count) once instead of O(line_count²) on every
/// overlay-label refresh.
fn build_page_tops(state: &AppState) -> Vec<usize> {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Vec::new();
    }
    let mut tops = vec![0usize];
    let mut top: usize = 0;
    while top < line_count {
        let next = next_page_top(state, top).new_top;
        if next <= top || next >= line_count {
            break;
        }
        tops.push(next);
        top = next;
    }
    tops
}

/// Return the 1-indexed viewport page that contains `target_line`. Reads
/// from the page_tops cache; builds it on first need.
///
/// If the text_view hasn't been laid out yet (height ≤ 0), build returns a
/// degenerate `[0]` index that would lock the label to "page 1". We refuse
/// to cache that — return 1 directly and leave the cache empty so the next
/// call after layout completes triggers a real build.
pub fn viewport_page_for_line(state: &AppState, target_line: usize) -> usize {
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            return page_for_line_in_index(tops, target_line);
        }
    }
    // Don't build (and cache) before layout — next_page_top returns garbage
    // when widget_height ≤ 0 and the resulting [0] index would stick.
    if state.text_view.height() <= 0 {
        return 1;
    }
    let tops = build_page_tops(state);
    let page = page_for_line_in_index(&tops, target_line);
    *state.page_tops.borrow_mut() = Some(tops);
    page
}

/// Drop the page_tops cache. Called when font/size changes invalidate page
/// boundaries (resnap_page) and when a new work loads (display_work).
pub fn invalidate_page_tops(state: &AppState) {
    *state.page_tops.borrow_mut() = None;
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

    // Remember current page so page_backward can return to it exactly
    state.page_history.push(state.page_top_line);

    state.current_line = next_dialogue;
    set_page(state, new_top, PageDirection::Forward);
    after_page_change(state, PageChangeReason::Forward);
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

    // History pop wins — preserves exact undo of any forward page-mutating
    // navigation (page_forward, jump_to_line, chapter, scene, etc.). Only
    // fall through to prev_page_top when history is empty (resume mid-book
    // or paged back through all of history).
    let (new_top, next_dialogue) = if let Some(prev_top) = state.page_history.pop() {
        let line_count = state.effective_line_count();
        let next = next_dialogue_from(&state.buffer, prev_top, line_count);
        let top = back_up_for_speaker(&state.buffer, next);
        log_fmt!("PAGE_BWD: from history prev_top={} next={} new_top={} current_line={}",
                 prev_top, next, top, state.current_line);
        (top, next)
    } else if state.page_top_line == 0 {
        log_fmt!("PAGE_BWD: no history and at start of work");
        return;
    } else {
        let np = prev_page_top(state, state.page_top_line);
        log_fmt!("PAGE_BWD: no history — prev_page_top new_top={} next_dialogue={} from page_top={}",
                 np.new_top, np.next_dialogue, state.page_top_line);
        (np.new_top, np.next_dialogue)
    };

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

/// Go to previous page and place cursor on its last visible line (shift+comma).
pub fn page_backward_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let prev_top = if let Some(t) = state.page_history.pop() {
        t
    } else if state.page_top_line == 0 {
        log_fmt!("NAV_BACK_BOTTOM: at start of work, no history");
        return;
    } else {
        let np = prev_page_top(state, state.page_top_line);
        log_fmt!("NAV_BACK_BOTTOM: no history — prev_page_top new_top={} from page_top={}",
                 np.new_top, state.page_top_line);
            // Discard np.next_dialogue: this function places the cursor on
            // the last visible line, computed below via last_fully_visible_line,
            // not the first dialogue line of the page.
            np.new_top
    };
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

/// Check if a buffer line is blank (empty or whitespace only).
fn is_blank_buffer_line(buffer: &sourceview5::Buffer, line: usize) -> bool {
    let text = buffer_line_text(buffer, line);
    text.trim().is_empty()
}

/// Get the text content of a buffer line.
fn buffer_line_text(buffer: &sourceview5::Buffer, line: usize) -> String {
    let Some(start) = buffer.iter_at_line(line as i32) else {
        return String::new();
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(&start, &end, false).to_string()
}

/// Check if a buffer line is a dialogue line (not blank, speaker, stage direction, or marker).
fn is_dialogue_line(buffer: &sourceview5::Buffer, line: usize) -> bool {
    use crate::db::line_types;
    let text = buffer_line_text(buffer, line);
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !line_types::is_speaker(trimmed)
        && !line_types::is_stage_direction(trimmed)
        && !line_types::is_act_scene_marker(trimmed)
        && !line_types::is_separator(trimmed)
}

/// Find the next dialogue line after `current`. Skips translation lines.
fn next_dialogue_line(
    buffer: &sourceview5::Buffer,
    translation_lines: &[bool],
    current: usize,
    line_count: usize,
) -> Option<usize> {
    let mut i = current + 1;
    while i < line_count {
        if !translation_lines.get(i).copied().unwrap_or(false)
            && is_dialogue_line(buffer, i)
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the previous dialogue line before `current`. Skips translation lines.
fn prev_dialogue_line(
    buffer: &sourceview5::Buffer,
    translation_lines: &[bool],
    current: usize,
) -> Option<usize> {
    if current == 0 {
        return None;
    }
    let mut i = current - 1;
    loop {
        if !translation_lines.get(i).copied().unwrap_or(false)
            && is_dialogue_line(buffer, i)
        {
            return Some(i);
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

/// Find the best page-top for a forward page turn targeting `target_line`.
/// Backs up over any non-dialogue content (blanks, speakers, stage directions,
/// scene markers) immediately preceding the target so transition context is
/// visible at the top of the new page rather than dropped between pages.
fn page_turn_top(buffer: &sourceview5::Buffer, target_line: usize) -> usize {
    back_up_for_speaker(buffer, target_line)
}

/// Find the page-top for a chapter jump: walk backward from `target_line` to
/// the nearest act/scene marker so the viewport starts on the scene title.
/// Falls back to `page_turn_top` if no marker is found within `MAX_LOOKBACK`.
fn chapter_page_top(buffer: &sourceview5::Buffer, target_line: usize) -> usize {
    use crate::db::line_types;
    const MAX_LOOKBACK: usize = 50;
    let lookback_floor = target_line.saturating_sub(MAX_LOOKBACK);
    let mut i = target_line;
    while i > lookback_floor {
        let text = buffer_line_text(buffer, i - 1);
        if line_types::is_act_scene_marker(text.trim()) {
            return i - 1;
        }
        i -= 1;
    }
    page_turn_top(buffer, target_line)
}

/// Previous chapter line (`[` key).
pub fn jump_to_prev_chapter(state: &mut AppState) {
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
                // Cursor sits past the current scene's first dialogue line —
                // first 2-press should rewind to the start of *this* scene.
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
            next_dialogue_line(&state.buffer, &state.translation_lines, m, line_count)
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
            state.page_history.push(state.page_top_line);
            set_page_instant(state, top);
        }
    }
    // Page label uses the target line, not page_top, because page_top may
    // be a blank spacer. Override the label after after_page_change runs.
    after_page_change(state, PageChangeReason::JumpToBookmark);
    if let Some(text) = state.page_label_text_for_buffer(buffer_line) {
        state.page_line_label.set_text(&text);
        state.page_line_label.set_visible(true);
    }
}

// ---------------------------------------------------------------------------
// Page management
// ---------------------------------------------------------------------------

/// Check if a line is on screen at all (no padding requirement).
/// Used by playback sync to avoid premature page turns.
pub fn is_line_on_screen(state: &AppState, line: usize) -> bool {
    is_line_fully_visible(state, line)
}

/// Check whether a buffer line is fully visible in the viewport.
/// Sums line heights from page_top against widget_height minus a descender
/// guard and the text_view bottom margin (which GTK reserves for padding
/// and is not available for text rendering).
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
    // F4: fast path — consult the cache (raw range — every line genuinely
    // rendered on screen, no F9 trim applied because "is line N drawn?" is
    // a different question from "where should the next page boundary land?").
    if let Some(cached) = state.last_visible_range.get() {
        return line <= cached.last_fit && cached.count > 0;
    }
    // Cold-start fallback: recompute the raw range. No trim — see comment
    // above for why visibility-checks use raw, not trimmed.
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return true;
    }
    let descender_guard = descender_guard_px(&state.text_view, state.page_top_line);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;
    let line_count = state.effective_line_count();
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        state.page_top_line,
        line_count,
        usable_height,
    );
    line <= range.last_fit && range.count > 0
}


/// Scroll just enough to keep the current line visible. No page turn.
fn scroll_to_cursor(state: &mut AppState) {
    center_cursor(state);
}

/// Mode-aware scroll after a forward jump (`q` / next paragraph or dialogue).
/// `prev_line` is the cursor position before the jump. In e-reader mode, if the
/// new position triggers a page turn, the previous line becomes the top of the
/// new page (continuity — last line of old page = first line of new page).
fn scroll_after_jump_forward(state: &mut AppState, _prev_line: usize) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                // Remember current page for backward navigation
                state.page_history.push(state.page_top_line);
                // Put the dialogue line at the top, backing up to include speaker name
                let new_top = page_turn_top(&state.buffer, state.current_line);
                log_fmt!("NAV_PAGE_FWD: current={} old_top={} new_top={} history_len={}", state.current_line, state.page_top_line, new_top, state.page_history.len());
                set_page_instant(state, new_top);
            }
        }
    }
}

/// Mode-aware scroll after a backward jump (`,` / prev paragraph or dialogue).
/// In e-reader mode, page-turns when cursor reaches the top line of the page.
fn scroll_after_jump_backward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if state.current_line < state.page_top_line {
                // Pop history first — undoes the matching forward jump that
                // pushed page_top (k after j returns to the same viewport,
                // not a new page that just contains current_line). Fall back
                // to page_turn_top(current_line) when history is empty so a
                // cold backward navigation still lands on a real page boundary
                // instead of the old lpp approximation.
                let new_top = if let Some(prev_top) = state.page_history.pop() {
                    log_fmt!("NAV_PAGE_BACK: from history prev_top={} current={} old_top={} history_len={}",
                             prev_top, state.current_line, state.page_top_line, state.page_history.len());
                    prev_top
                } else {
                    let top = page_turn_top(&state.buffer, state.current_line);
                    log_fmt!("NAV_PAGE_BACK: no history — page_turn_top new_top={} current={} old_top={}",
                             top, state.current_line, state.page_top_line);
                    top
                };
                set_page_instant(state, new_top);
            }
        }
    }
}

/// Scroll the viewport so the current line is vertically centered.
/// Near document edges, clamps so no blank space appears (scrolloff behavior).
fn center_cursor(state: &mut AppState) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    let line_y = scroll_value_for_line(state, state.current_line);
    let offset = adj.page_size() * 0.25;
    let centered = (line_y - offset).max(0.0).min(max_scroll);
    crate::logging::log(&format!(
        "CENTER: line={} line_y={:.0} offset={:.0} centered={:.0} max={:.0} old_val={:.0}",
        state.current_line, line_y, offset, centered, max_scroll, adj.value()
    ));
    adj.set_value(centered);
}

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

/// Clear dim tags from the old visible range before a page turn.
/// Called before page_top_line is updated.
fn clear_old_page_dim(state: &AppState) {
    if !state.dim_enabled {
        return;
    }
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let lpp = lines_per_page(state);
    let margin = 5;
    let old_start = state.page_top_line.saturating_sub(margin);
    let old_end = (state.page_top_line + lpp + margin)
        .min(state.effective_line_count());
    let start_iter = buffer.iter_at_line(old_start as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let end_iter = buffer.iter_at_line(old_end as i32)
        .unwrap_or_else(|| buffer.end_iter());
    buffer.remove_tag(tag, &start_iter, &end_iter);
}

/// Direction of a page turn, used by the Slide transition.
#[derive(Clone, Copy)]
#[derive(Debug)]
enum PageDirection {
    Forward,
    Backward,
}

/// Re-entrancy lock for animated page turns.
///
/// `set_page` calls `try_acquire` before mutating `page_top_line` or starting
/// an animation. The animation's `connect_done` callback calls `release`. While
/// locked, secondary turn requests (typically from MPV `CursorSync` arriving
/// mid-animation) are dropped so they don't compose with the in-flight turn.
///
/// `set_page_instant` does NOT consult the lock — it has no animation, so the
/// re-entrancy window doesn't exist for that path.
///
/// Mirrors foliate-js's `Paginator.#locked` (paginator.js:1060-1071).
///
/// Uses `Cell<bool>` rather than `bool` so a `&PageTurnLock` borrow can mutate
/// it from a `connect_done` closure without a `&mut AppState`.
pub(crate) struct PageTurnLock {
    locked: std::cell::Cell<bool>,
}

impl PageTurnLock {
    pub(crate) fn new() -> Self {
        Self { locked: std::cell::Cell::new(false) }
    }

    /// Attempt to take the lock. Returns true if acquired, false if already held.
    pub(crate) fn try_acquire(&self) -> bool {
        if self.locked.get() {
            false
        } else {
            self.locked.set(true);
            true
        }
    }

    /// Release the lock. Idempotent — releasing when unlocked is a no-op.
    pub(crate) fn release(&self) {
        self.locked.set(false);
    }

    /// Peek without mutating.
    pub(crate) fn is_locked(&self) -> bool {
        self.locked.get()
    }
}

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

    /// Whether to refresh the page label. True for everything except WorkLoad
    /// (display_work handles label setup itself).
    pub(crate) fn should_update_label(self) -> bool {
        !matches!(self, Self::WorkLoad)
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

    if reason.should_update_label() {
        if let Some(text) = state.page_label_text_for_buffer(state.page_top_line) {
            state.page_line_label.set_text(&text);
            state.page_line_label.set_visible(true);
        }
    }

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

/// Result of a single height-summing walk over the buffer starting at a page
/// top: which line was the last to fully fit, the total pixel height consumed
/// by lines [page_top, last_fit], and the count of lines included.
///
/// Mirrors what foliate-js `getVisibleRange` returns (paginator.js:94-151) —
/// a single source of truth for "what's on screen right now" that all four
/// previous callers (`last_fully_visible_line`, `is_line_fully_visible`,
/// `update_bottom_clip`, `lines_per_page`) project from.
///
/// Convention: when the buffer is empty or no line fits, `count == 0` and
/// `total_height == 0`. `last_fit` is then equal to `page_top` but should be
/// treated as meaningless by callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisibleRange {
    pub(crate) last_fit: usize,
    pub(crate) total_height: i32,
    pub(crate) count: usize,
}

/// Walk the buffer from `page_top`, summing line heights against the viewport's
/// `usable_height` (caller computes that from `widget_height - descender_guard
/// - bottom_margin`). Returns the largest range that fully fits.
///
/// Caller-specific short-circuits (`loading_work`, `widget_height <= 0`, empty
/// buffer) stay in the callers — they're not part of this kernel.
///
/// Pure: given a sorted vec of viewport-page top line indices, return the
/// 1-indexed page that contains `target_line`. Empty index returns 1.
/// `target_line` past the last page-top returns `tops.len()`.
///
/// Mirrors what foliate-js paginator.js does in O(log n) via index lookup
/// (`atStart`/`atEnd` use page indices, paginator.js:1050-1054), replacing
/// linux-lit's previous O(n²) replay-from-line-0 walk.
pub(crate) fn page_for_line_in_index(tops: &[usize], target_line: usize) -> usize {
    if tops.is_empty() {
        return 1;
    }
    // partition_point returns the index of the first element > target_line.
    // Because tops[0] is always 0 (the first page), partition_point >= 1 for
    // any target_line >= 0 — that's exactly the page number we want.
    tops.partition_point(|&t| t <= target_line).max(1)
}

/// Mirrors `getVisibleRange` in foliate-js paginator.js (lines 94-151) in
/// purpose: one canonical visibility computation. Future foliate reads of that
/// function map directly to this one.
pub(crate) fn visible_range(
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    page_top: usize,
    line_count: usize,
    usable_height: i32,
) -> VisibleRange {
    let mut total_height: i32 = 0;
    let mut count: usize = 0;
    let mut last_fit: usize = page_top;
    for i in page_top..line_count {
        let Some(iter) = buffer.iter_at_line(i as i32) else { break };
        let (_y, h) = text_view.line_yrange(&iter);
        if total_height + h > usable_height {
            break;
        }
        total_height += h;
        last_fit = i;
        count += 1;
    }
    VisibleRange { last_fit, total_height, count }
}

/// Trim trailing dangling-context lines (speakers, blanks, stage directions)
/// from a `VisibleRange` so the page never ends on a line that exists only
/// to introduce or annotate the dialogue that should follow it. Pure variant
/// separated for unit testability — the GTK-bound `trim_trailing_speakers`
/// wraps this with line_types and line_yrange calls.
///
/// Stops at `page_top` so the trim never deletes the page top itself.
pub(crate) fn trim_trailing_speakers_pure<F, H>(
    mut range: VisibleRange,
    page_top: usize,
    is_dangling_context: F,
    line_height: H,
) -> VisibleRange
where
    F: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
{
    while range.last_fit > page_top && is_dangling_context(range.last_fit) {
        range.total_height -= line_height(range.last_fit);
        range.last_fit -= 1;
        range.count = range.count.saturating_sub(1);
    }
    range
}

/// GTK-bound wrapper for `trim_trailing_speakers_pure`. Reads line text via
/// `buffer` and classifies via `crate::db::line_types`; reads heights via
/// `text_view.line_yrange`. Stage directions are treated as dangling-context
/// just like speakers — neither should be the last line of a page (they
/// preface the dialogue that follows, not summarize what came before).
pub(crate) fn trim_trailing_speakers(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> VisibleRange {
    use crate::db::line_types;
    let is_dangling_context = |i: usize| -> bool {
        let text = {
            let Some(start) = buffer.iter_at_line(i as i32) else { return false };
            let mut end = start;
            if !end.ends_line() { end.forward_to_line_end(); }
            buffer.text(&start, &end, false).to_string()
        };
        line_types::is_speaker(&text)
            || line_types::is_blank(&text)
            || line_types::is_stage_direction(&text)
    };
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_trailing_speakers_pure(range, page_top, is_dangling_context, line_height)
}

/// Pure: given closures that classify line kinds, find the start of the
/// "block" containing `last_fit`. A block is a multi-line stage-direction
/// run (any work) or a multi-line verse stanza (non-prose works only).
///
/// Returns `last_fit` unchanged when `last_fit` is not in a recognized block,
/// or when the block is just one line (no backup needed).
///
/// Stops at `page_top` so the trim never deletes the page top itself.
///
/// Mirrors foliate-js's per-element visibility rule (paginator.js:104-106) —
/// block atomicity instead of per-line visibility.
pub(crate) fn block_start_for_line_pure<B, S, D, L>(
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
) -> usize
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
{
    // Stage takes precedence: a stage-direction line inside a non-prose
    // work is a stage block, never a verse line.
    if is_stage(last_fit) {
        let mut start = last_fit;
        while start > page_top && is_stage(start - 1) {
            start -= 1;
        }
        // Single-line "block" — not multi-line, no backup needed.
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    // Verse stanza: only in non-prose works, only on dialogue lines.
    if !is_prose && is_dialogue(last_fit) {
        let mut start = last_fit;
        while start > page_top {
            let prev = start - 1;
            if is_blank(prev) || is_speaker(prev) || is_stage(prev) {
                break;
            }
            start -= 1;
        }
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    last_fit
}

/// Pure: trim a `VisibleRange` so the last fitting line isn't mid-block.
/// Backs `last_fit` up to the line before block-start when the block fully
/// fits; returns the range unchanged when the block doesn't fit at all
/// (`block_start <= page_top` — overflow fallback policy from F9 spec).
///
/// `line_height` closure provides per-line heights for `total_height` accounting.
pub(crate) fn trim_block_atoms_pure<B, S, D, L, H>(
    range: VisibleRange,
    page_top: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
    line_height: &H,
) -> VisibleRange
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
{
    if range.count == 0 || range.last_fit == page_top {
        return range;
    }
    let block_start = block_start_for_line_pure(
        page_top, range.last_fit, is_prose,
        is_blank, is_speaker, is_stage, is_dialogue,
    );
    if block_start == range.last_fit {
        return range; // not in a block
    }
    if block_start <= page_top {
        return range; // overflow: block extends to (or past) page_top
    }
    // Only trim if the block actually continues past last_fit (i.e., we're
    // splitting it mid-block). If the next line is OUTSIDE the same block
    // (a blank, speaker, stage transition, or end of buffer), the block
    // ends at last_fit and there's nothing to atomicize — leave as-is.
    let continues = if is_stage(range.last_fit) {
        // Stage block: continues iff next line is also stage.
        is_stage(range.last_fit + 1)
    } else if !is_prose && is_dialogue(range.last_fit) {
        // Verse stanza: continues iff next line is dialogue (not blank/
        // speaker/stage which would close the stanza).
        let next = range.last_fit + 1;
        !is_blank(next) && !is_speaker(next) && !is_stage(next) && is_dialogue(next)
    } else {
        false
    };
    if !continues {
        return range; // block ends here; nothing to atomicize
    }
    // Drop lines [block_start, range.last_fit] from the range.
    let mut new_total_height = range.total_height;
    for i in block_start..=range.last_fit {
        new_total_height -= line_height(i);
    }
    let new_last_fit = block_start - 1;
    let new_count = new_last_fit - page_top + 1;
    // Overflow guard: if trimming would leave half or fewer lines visible,
    // the block is too tall to fit on one page (e.g., a verse stanza longer
    // than the viewport sitting just below a speaker). Keep the per-line
    // split — better to show a useful page that splits mid-block than a
    // useless 1-line page that just shows the speaker. This is the F9
    // best-effort policy from the spec ("atomic if it fits, split if not").
    if new_count * 2 <= range.count {
        return range;
    }
    VisibleRange {
        last_fit: new_last_fit,
        total_height: new_total_height,
        count: new_count,
    }
}

/// GTK-bound wrapper for `block_start_for_line_pure`. Reads line text via
/// `buffer` and classifies via `crate::db::line_types`.
///
/// Most callers want trim_visible_range instead.
#[allow(dead_code)]
pub(crate) fn block_start_for_line(
    buffer: &sourceview5::Buffer,
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
) -> usize {
    use crate::db::line_types;
    let line_text = |i: usize| -> String {
        let Some(start) = buffer.iter_at_line(i as i32) else { return String::new() };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        buffer.text(&start, &end, false).to_string()
    };
    let is_blank = |i: usize| line_types::is_blank(&line_text(i));
    let is_speaker = |i: usize| line_types::is_speaker(&line_text(i));
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
    block_start_for_line_pure(page_top, last_fit, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue)
}

/// GTK-bound wrapper for `trim_block_atoms_pure`. Reads line text and heights
/// from `text_view`/`buffer`. `is_prose` is the work-type flag (true for novel/
/// essay/etc., false for plays and poetry).
///
/// Most callers want trim_visible_range instead.
pub(crate) fn trim_block_atoms(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
) -> VisibleRange {
    use crate::db::line_types;
    let line_text = |i: usize| -> String {
        let Some(start) = buffer.iter_at_line(i as i32) else { return String::new() };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        buffer.text(&start, &end, false).to_string()
    };
    let is_blank = |i: usize| line_types::is_blank(&line_text(i));
    let is_speaker = |i: usize| line_types::is_speaker(&line_text(i));
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_block_atoms_pure(range, page_top, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height)
}

/// Scan (page_top, last_fit] for a line that starts a new chapter or scene
/// (detected from buffer text via `is_act_scene_marker` or `is_separator`).
/// If found, clamp last_fit to the line before it so the new section starts
/// on the next page. Uses text-based detection rather than DB flags so it
/// works uniformly for prose, verse, and plays.
fn clamp_at_section_break(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> VisibleRange {
    use crate::db::line_types;
    if range.count <= 1 {
        return range;
    }
    // Scan for the first section break strictly after page_top.
    let mut break_line = None;
    for i in (page_top + 1)..=range.last_fit {
        let text = buffer_line_text(buffer, i);
        let trimmed = text.trim();
        if line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed) {
            break_line = Some(i);
            break;
        }
    }
    let break_line = match break_line {
        Some(b) => b,
        None => return range,
    };
    let clamped_last = break_line.saturating_sub(1);
    if clamped_last < page_top {
        return range;
    }
    // Recompute total_height by walking line heights up to clamped_last.
    let mut total = 0i32;
    for i in page_top..=clamped_last {
        if let Some(iter) = buffer.iter_at_line(i as i32) {
            let (_y, h) = text_view.line_yrange(&iter);
            total += h;
        }
    }
    VisibleRange {
        last_fit: clamped_last,
        total_height: total,
        count: clamped_last - page_top + 1,
    }
}

/// Canonical composition: apply section-break clamping, then
/// `trim_trailing_speakers`, then `trim_block_atoms`,
/// then `trim_trailing_speakers` again on the result. All callers that compute a
/// visible range for "what's on this page" should go through this wrapper so all
/// trims fire in the right order.
///
/// Order matters:
/// 0. Section-break clamp first — ensures a new chapter/scene starts at the top
///    of the next page rather than appearing partway down the current page.
/// 1. Speaker trim first removes a dangling speaker at the bottom.
/// 2. Block trim sees the new `last_fit` and decides whether THAT line is
///    mid-block; if so, backs up to the line before block-start.
/// 3. Speaker trim AGAIN — block trim usually leaves `last_fit` on a speaker
///    (the line that introduced the now-removed block), which is itself
///    dangling-context. Without this second pass, is_line_fully_visible would
///    treat a fully-visible dialogue line as off-page just because the trim
///    chain reported `last_fit` on the speaker above it.
pub(crate) fn trim_visible_range(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
) -> VisibleRange {
    // Pre-pass: clamp at the first section break (act/scene marker or
    // separator) strictly inside the range so chapters/scenes always start
    // at the top of the next page.
    let r = clamp_at_section_break(range, page_top, text_view, buffer);
    let r = trim_trailing_speakers(r, page_top, text_view, buffer);
    let r = trim_block_atoms(r, page_top, text_view, buffer, is_prose);
    trim_trailing_speakers(r, page_top, text_view, buffer)
}

/// Capture the entire card (spacers + text) as a static Picture overlay.
/// Uses WidgetPaintable → Snapshot → RenderNode → Texture to freeze the frame.
/// Returns the Picture (already added to page_turn_overlay) or None if capture fails.
fn capture_page_snapshot(state: &AppState) -> Option<gtk4::Picture> {
    let widget = &state.card_vbox;
    let w = widget.width();
    let h = widget.height();
    if w <= 0 || h <= 0 {
        return None;
    }

    // Try the texture approach first (frozen snapshot, immune to opacity changes)
    let paintable = gtk4::WidgetPaintable::new(Some(widget));
    let snapshot = gtk4::Snapshot::new();
    use gtk4::prelude::PaintableExt;
    paintable.snapshot(&snapshot, w as f64, h as f64);

    if let Some(node) = snapshot.to_node() {
        if let Some(native) = widget.native() {
            if let Some(renderer) = native.renderer() {
                let viewport = gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
                let texture = renderer.render_texture(&node, Some(&viewport));
                let pic = gtk4::Picture::for_paintable(&texture);
                pic.set_content_fit(gtk4::ContentFit::Fill);
                pic.set_size_request(w, h);
                state.page_turn_overlay.add_overlay(&pic);
                return Some(pic);
            }
        }
    }

    // Fallback: create a WidgetPaintable, then immediately disconnect it
    // from the widget so it becomes a frozen snapshot of the current frame.
    // Texture path failed (e.g. stale GPU state after suspend/resume).
    let wp = gtk4::WidgetPaintable::new(Some(widget));
    wp.set_widget(gtk4::Widget::NONE);
    let pic = gtk4::Picture::for_paintable(&wp);
    pic.set_content_fit(gtk4::ContentFit::Fill);
    pic.set_size_request(w, h);
    state.page_turn_overlay.add_overlay(&pic);
    Some(pic)
}

/// Set the page top line with an animated transition based on config.transition_style.
fn set_page(state: &mut AppState, new_top: usize, direction: PageDirection) {
    // During work loading, GTK layout is invalid — skip page turns entirely.
    if state.loading_work.get() {
        log_fmt!("PAGE_TURN: SKIPPED (loading_work=true) new_top={}", new_top);
        return;
    }
    // F1: drop racing turns so MPV CursorSync arriving mid-animation can't
    // compose with a key-driven turn. set_page_instant does not go through
    // here — it has no animation window.
    if !state.page_turn_lock.try_acquire() {
        log_fmt!(
            "PAGE_TURN: SKIPPED (locked=true) new_top={} old_top={} requested_dir={:?}",
            new_top, state.page_top_line, direction
        );
        return;
    }
    log_fmt!(
        "PAGE_TURN: new_top={} old_top={} current_line={} transition={:?}",
        new_top, state.page_top_line, state.current_line, state.config.transition_style
    );

    match state.config.transition_style {
        crate::config::TransitionStyle::Instant => {
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.page_turn_lock.release();
        }
        crate::config::TransitionStyle::Crossfade => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                state.page_turn_lock.release();
                return;
            };

            // Cancel any in-flight page animation
            if let Some(prev) = state.page_turn_anim.take() {
                prev.skip();
            }

            // Hide the card while GTK scrolls to the new position.
            // The snapshot is a sibling overlay in page_turn_overlay (which wraps
            // card_vbox), so hiding card_vbox doesn't hide the snapshot.
            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);

            // Gentle crossfade: value goes 1.0 → 0.0 over 800ms.
            // New content fades in fast (quadratic), snapshot fades out slow
            // (inverse quadratic). Keeps combined brightness near 100%.
            let overlay = state.page_turn_overlay.clone();
            let card = state.card_vbox.clone();
            let snap = snapshot_pic.clone();
            let target = adw::CallbackAnimationTarget::new(move |value| {
                let t = 1.0 - value as f64; // progress 0→1
                // Smoothstep S-curve for gentle timing
                let s = t * t * (3.0 - 2.0 * t);
                // Use sine/cosine on the smoothstepped value to preserve
                // total brightness: sin²+cos² = 1, no dark dip at midpoint.
                card.set_opacity((s * std::f64::consts::FRAC_PI_2).sin());
                snap.set_opacity((s * std::f64::consts::FRAC_PI_2).cos());
            });
            let anim = adw::TimedAnimation::new(
                &snapshot_pic,
                1.0,  // from
                0.0,  // to
                700,  // duration ms
                target,
            );
            anim.set_easing(adw::Easing::Linear);

            let snap_cleanup = snapshot_pic.clone();
            let lock = std::rc::Rc::clone(&state.page_turn_lock);
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                lock.release();
            });

            anim.play();
            state.page_turn_anim = Some(anim);
        }
        crate::config::TransitionStyle::Slide => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                state.page_turn_lock.release();
                return;
            };

            // Cancel any in-flight page animation
            if let Some(prev) = state.page_turn_anim.take() {
                prev.skip();
            }

            let width = state.card_vbox.width() as f64;

            // Hide card while GTK scrolls to the new position
            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.card_vbox.set_margin_start(0);
            state.card_vbox.set_margin_end(0);

            // Animate snapshot sliding out: 0.0 → 1.0 progress, 250ms, ease-out-cubic.
            // Reveals card on the first frame after scroll settles.
            let overlay = state.page_turn_overlay.clone();
            let card = state.card_vbox.clone();
            let snap = snapshot_pic.clone();
            let is_forward = matches!(direction, PageDirection::Forward);
            let revealed = std::cell::Cell::new(false);
            let target = adw::CallbackAnimationTarget::new(move |progress| {
                if !revealed.get() {
                    card.set_opacity(1.0);
                    revealed.set(true);
                }
                let offset = (width * progress) as i32;
                if is_forward {
                    snap.set_margin_start(0);
                    snap.set_margin_end(offset);
                    card.set_margin_start((width as i32) - offset);
                    card.set_margin_end(0);
                } else {
                    snap.set_margin_start(offset);
                    snap.set_margin_end(0);
                    card.set_margin_start(0);
                    card.set_margin_end((width as i32) - offset);
                }
            });
            let anim = adw::TimedAnimation::new(
                &snapshot_pic,
                0.0,  // from
                1.0,  // to
                250,  // duration ms
                target,
            );
            anim.set_easing(adw::Easing::EaseOutCubic);

            let snap_cleanup = snapshot_pic.clone();
            let card_cleanup = state.card_vbox.clone();
            let lock = std::rc::Rc::clone(&state.page_turn_lock);
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                card_cleanup.set_margin_start(0);
                card_cleanup.set_margin_end(0);
                lock.release();
            });

            anim.play();
            state.page_turn_anim = Some(anim);
        }
    }
}

/// Re-scroll to the current page_top and recalculate the bottom clip.
/// Called after font/size changes that invalidate line heights.
pub fn resnap_page(state: &mut AppState) {
    snap_scroll_to_line(state, state.page_top_line);
}

/// Re-run `update_bottom_clip` against the current viewport state without
/// touching the scroll position. Use after font / line-height changes that
/// don't shift `page_top_line` — e.g. translation toggle, where the caller
/// has already restored a custom scroll value and a full `resnap_page`
/// would clobber it.
pub fn refresh_bottom_clip(state: &AppState) {
    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        state.page_top_line,
        state.effective_line_count(),
    );
}

/// Fire `update_bottom_clip` twice: once via `idle_add` (covers the common
/// case where layout is already settled), once via `timeout(100ms)` (covers
/// post-font-change where GTK's layout pass for new metrics hasn't completed
/// yet — `line_yrange` returns pre-layout heights on the first pass and the
/// clip ends up sized for the OLD font's line heights). The second fire reads
/// post-layout metrics and corrects.
fn schedule_bottom_clip_update(
    text_view: sourceview5::View,
    bottom_clip: gtk4::Box,
    scrolled_window: gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
) {
    let tv1 = text_view.clone();
    let bc1 = bottom_clip.clone();
    let sw1 = scrolled_window.clone();
    glib::idle_add_local_once(move || {
        update_bottom_clip(&tv1, &bc1, &sw1, page_top, line_count);
    });
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, page_top, line_count);
    });
}

/// Set the page top line and scroll instantly (no animation). For gg/G/restore.
fn set_page_instant(state: &mut AppState, new_top: usize) {
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    snap_scroll_to_line(state, new_top);
}

/// Scroll so `line` is at the top of the viewport, then size the bottom clip
/// overlay to hide any partially-visible line at the bottom of the page.
pub(crate) fn snap_scroll_to_line(state: &mut AppState, line: usize) {
    // Position the target line at the very top of the viewport using
    // vadjustment for pixel-perfect positioning (scroll_to_iter is imprecise).
    // GTK's set_value clamps to [0, upper - page_size], so when the requested
    // y exceeds the document's max scroll (e.g., jump_to_end on the last
    // page), the actual scroll position is less than y. Detect that case
    // explicitly: if clamped, the caller's logical page_top is too far —
    // there's no recovery here in snap_scroll_to_line, but the bottom_clip
    // overlay's height-based clamp at least keeps the bottom-edge tidy.
    if let Some(iter) = state.buffer.iter_at_line(line as i32) {
        let (y, _h) = state.text_view.line_yrange(&iter);
        let adj = state.scrolled_window.vadjustment();
        let max_value = (adj.upper() - adj.page_size()).max(0.0);
        let target = (y as f64).min(max_value);
        adj.set_value(target);
    }

    // F4: populate the cache synchronously so MPV sync handlers reading
    // is_line_fully_visible right after this call see the new range, not
    // stale state. The idle-scheduled update_bottom_clip below ALSO writes
    // the cache as a backstop for layout-not-yet-flushed cases.
    let widget_height = state.text_view.height();
    if widget_height > 0 {
        let descender_guard = descender_guard_px(&state.text_view, line);
        let bottom_margin = state.text_view.bottom_margin();
        let usable_height = widget_height - descender_guard - bottom_margin;
        let line_count = state.effective_line_count();
        // Cache the RAW range — every line genuinely rendered on screen.
        // Consumers split into two camps: is_line_fully_visible needs raw
        // ("is line N drawn?"); last_fully_visible_line needs trimmed
        // ("where should the next page start?") and applies trim itself.
        let range = visible_range(&state.text_view, &state.buffer, line, line_count, usable_height);
        state.last_visible_range.set(Some(range));
    } else {
        state.last_visible_range.set(None);
    }

    // Update page line indicator (citation for plays, line_mapping.id otherwise)
    if let Some(text) = state.page_label_text_for_buffer(line) {
        state.page_line_label.set_text(&text);
        state.page_line_label.set_visible(true);
    }

    // Schedule the clip height update for the next frame, after GTK has
    // completed the scroll and updated line layout positions. Fires twice
    // (idle + 100ms timeout) — the timeout backstop catches post-font-change
    // cases where the first idle pass sees pre-layout line heights.
    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        line,
        state.effective_line_count(),
    );
}

/// Pixel descent of the active font, queried from Pango. Mirrors foliate-js's
/// approach (paginator.js:83-91) of measuring the engine rather than estimating
/// from line height — fixes mixed-font-size pages where the bottom line uses a
/// different font than the page top (translations smaller, chapter titles
/// larger).
///
/// `_page_top` is unused but kept in the signature so the four callers (which
/// pass it from their local context) don't need to change.
///
/// Reads the active font from the `font-size` TextTag (which `reapply_font`
/// updates synchronously) rather than the widget's Pango context (which only
/// reflects font changes after GTK applies the next CSS pass). This avoids a
/// one-frame race where descender stayed clipped briefly after font cycling.
///
/// Returns the descent in pixels, with a small safety floor (4 px) and ceiling
/// (24 px) to prevent absurd values from a missing/broken font from corrupting
/// the visible-range calculation.
fn descender_guard_px(text_view: &sourceview5::View, _page_top: usize) -> i32 {
    use gtk4::prelude::{TextTagExt, TextBufferExt, WidgetExt};
    let ctx = text_view.pango_context();

    // Prefer the explicit font from the `font-size` tag we set in reapply_font.
    // This avoids the GTK CSS-application race where pango_context().metrics()
    // returns the OLD font's descent for one frame after a font change.
    let font_desc = text_view
        .buffer()
        .tag_table()
        .lookup("font-size")
        .and_then(|tag| tag.font_desc());

    let metrics = ctx.metrics(font_desc.as_ref(), None);
    let descent_px = metrics.descent() / pango::SCALE;
    descent_px.clamp(4, 24)
}

/// Set the bottom clip to hide everything below the last fully-visible line.
/// Uses buffer line_yrange (absolute coords) to sum heights from page_top,
/// avoiding buffer_to_window_coords which may be stale after scroll_to_iter.
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
) {
    let widget_height = text_view.height();
    if widget_height <= 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    let buf = text_view.buffer();
    let buf_sv: sourceview5::Buffer = match buf.downcast::<sourceview5::Buffer>() {
        Ok(b) => b,
        Err(_) => {
            bottom_clip.set_height_request(0);
            return;
        }
    };

    // When page_top would render the last buffer line within widget_height,
    // skip the descender_guard reservation — there's no next page below,
    // so no risk of half-clipping a "partial line" descender. Just fit
    // content into widget_height and let bottom_clip cover any whitespace.
    let bottom_margin = text_view.bottom_margin();
    let descender_guard = descender_guard_px(text_view, page_top);
    let usable_height = {
        // Probe: does fitting from page_top with widget_height usable
        // include line_count - 1? If yes, this is the last page, drop
        // the descender_guard (and bottom_margin) reservation.
        let probe = visible_range(text_view, &buf_sv, page_top, line_count, widget_height);
        if probe.last_fit + 1 >= line_count {
            widget_height
        } else {
            widget_height - descender_guard - bottom_margin
        }
    };

    let range = visible_range(text_view, &buf_sv, page_top, line_count, usable_height);

    if range.count == 0 || range.total_height == 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    // Section-break clamp + trailing-speaker trim. The section-break clamp
    // ensures the bottom clip hides chapter/scene headings that would
    // otherwise appear at the bottom of the page — they belong at the top
    // of the next page.
    let trimmed = clamp_at_section_break(range, page_top, text_view, &buf_sv);
    let trimmed = trim_trailing_speakers(trimmed, page_top, text_view, &buf_sv);

    let clip = (widget_height - trimmed.total_height).max(0);
    let scroll_val = scrolled_window.vadjustment().value();
    let expected_y = if let Some(iter) = buf_sv.iter_at_line(page_top as i32) {
        let (y, _h) = text_view.line_yrange(&iter);
        y as f64
    } else {
        -1.0
    };
    let scroll_offset = scroll_val - expected_y;
    // Skip the redundant set_height_request when the clip value would be
    // unchanged. Calling set_height_request even with the same value forces
    // GTK to revalidate layout, which can re-shape Pango glyphs by 1-2px
    // (subpixel positioning). Visible as a "shift then snap back" 100ms
    // after the work first appears (the post-snap timeout backstop).
    let cur = bottom_clip.height_request();
    if cur == clip {
        crate::logging::log(&format!(
            "BOTTOM_CLIP: widget_h={} total_h={} clip={} page_top={} scroll_val={:.1} expected_y={:.1} offset={:.1} (unchanged, skipped)",
            widget_height, trimmed.total_height, clip, page_top, scroll_val, expected_y, scroll_offset
        ));
        return;
    }
    crate::logging::log(&format!(
        "BOTTOM_CLIP: widget_h={} total_h={} clip={} page_top={} scroll_val={:.1} expected_y={:.1} offset={:.1}",
        widget_height, trimmed.total_height, clip, page_top, scroll_val, expected_y, scroll_offset
    ));
    bottom_clip.set_height_request(clip);
}

/// Scroll the viewport by a fixed step without moving the cursor or seeking audio.
/// `delta` is +1 for down, -1 for up.  Scrolls by ~3 line heights per step,
/// similar to browser-style scrolling.
pub fn scroll_viewport(state: &mut AppState, delta: i32) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    // Scroll by 3 line heights per keypress
    let line_height = state.buffer.iter_at_line(state.current_line as i32)
        .map(|iter| {
            let rect = state.text_view.iter_location(&iter);
            rect.height() as f64
        })
        .unwrap_or(30.0)
        .max(20.0);
    let step = line_height * 3.0;
    let new_val = (adj.value() + step * delta as f64).max(0.0).min(max_scroll);
    adj.set_value(new_val);
}

/// Get the vadjustment value that places the given line at the top of the viewport.
/// Uses `line_yrange` which includes `pixels_above_lines` in the y coordinate,
/// so the line's full visual extent (including top spacing) is visible.
fn scroll_value_for_line(state: &AppState, line: usize) -> f64 {
    let adj = state.scrolled_window.vadjustment();
    let max = adj.upper() - adj.page_size();

    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return 0.0;
    };
    let (y, _h) = state.text_view.line_yrange(&iter);
    (y as f64).max(0.0).min(max)
}

// ---------------------------------------------------------------------------
// Highlight
// ---------------------------------------------------------------------------

/// Update highlight and ensure cursor is visible on the current page.
/// Update highlight only (no scrolling). Used for prose sentence mode where
/// scrolling is deferred until the next sentence is about to start.
pub fn update_highlight_only(state: &mut AppState) {
    update_highlight(state);
    auto_show_vocab_popup(state);
}

/// Scroll so that a paragraph's first line lands at the cursor position
/// (~35% down the viewport). Used when playback crosses a paragraph boundary.
pub fn scroll_paragraph_to_top(state: &mut AppState, para_start: usize) {
    // F1: if a page-turn animation is in flight, drop this sync request.
    // The next CursorSync after release will pick up the new state.
    // Mirrors foliate Paginator.goTo's #locked early-return (paginator.js:1023).
    if state.page_turn_lock.is_locked() {
        crate::logging::log(&format!(
            "PARA_SCROLL: SKIP (page_turn_locked) para_start={} page_top={}",
            para_start, state.page_top_line
        ));
        return;
    }
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => {
            let adj = state.scrolled_window.vadjustment();
            let max_scroll = adj.upper() - adj.page_size();
            if max_scroll <= 0.0 {
                return;
            }
            let line_y = scroll_value_for_line(state, para_start);
            let offset = adj.page_size() * 0.25;
            let val = (line_y - offset).max(0.0).min(max_scroll);
            adj.set_value(val);
        }
        crate::config::NavigationMode::EReader => {
            // Only page-turn if the paragraph start is off-screen
            if !is_line_on_screen(state, para_start) {
                crate::logging::log(&format!(
                    "PARA_SCROLL: para_start={} page_top={}",
                    para_start, state.page_top_line
                ));
                let dir = if para_start >= state.page_top_line { PageDirection::Forward } else { PageDirection::Backward };
                set_page(state, para_start, dir);
            }
        }
    }
}


pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                let new_top = if state.current_line >= state.page_top_line {
                    page_turn_top(&state.buffer, state.current_line)
                } else {
                    state.current_line
                };
                log_fmt!(
                    "SYNC_PAGE: ensure_visible triggered, current_line={} page_top={} new_top={}",
                    state.current_line, state.page_top_line, new_top
                );
                let dir = if state.current_line >= state.page_top_line { PageDirection::Forward } else { PageDirection::Backward };
                set_page(state, new_top, dir);
            }
        }
    }
    auto_show_vocab_popup(state);
}

/// Like update_highlight_and_ensure_visible, but turns the page when the
/// highlight advances past the last visible line. The new page starts with
/// the new line at the top — no overlap with the previous page's content.
/// Used by playback sync.
pub fn update_highlight_and_advance_page(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            let last_vis = last_fully_visible_line(state, state.page_top_line);
            log_fmt!(
                "SYNC_ADVANCE: current={} last_vis={} page_top={}",
                state.current_line, last_vis, state.page_top_line
            );
            if state.current_line > last_vis {
                let new_top = page_turn_top(&state.buffer, state.current_line);
                state.page_history.push(state.page_top_line);
                set_page(state, new_top, PageDirection::Forward);
            }
        }
    }
    auto_show_vocab_popup(state);
}

/// Apply highlight tags, snap scroll to page_top_line, apply bottom clip,
/// then show the scrolled window. Called at the end of display_work_at
/// after the buffer and cursor position are fully set up.
pub fn update_highlight_and_show(state: &mut AppState) {
    if state.page_top_line == 0 && state.current_line > 0 {
        state.page_top_line = state.current_line;
    }
    update_highlight(state);

    // Page label is intentionally NOT set here. text_view.height() is still
    // 0 (scrolled_window is hidden), so viewport_page_for_line would build
    // a degenerate page_tops index and return page 1, causing a visible
    // "# - 1" flash. The resize tick refreshes the label once layout
    // settles (see app.rs deferred-layout-refresh branch).

    let scroll_to = state.page_top_line;
    let line_count = state.effective_line_count();

    // Snap scroll position synchronously. line_yrange may return 0 if GTK
    // hasn't validated the layout yet, so we defer to an idle callback.
    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let loading_flag = state.loading_work.clone();
    let refresh_flag = state.needs_layout_refresh.clone();
    let buffer = state.buffer.clone();
    let scrolled_window = state.scrolled_window.clone();

    glib::idle_add_local_once(move || {
        if let Some(iter) = buffer.iter_at_line(scroll_to as i32) {
            let (y, _h) = text_view.line_yrange(&iter);
            let adj = scrolled_window.vadjustment();
            let max_scroll = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value((y as f64).max(0.0).min(max_scroll));
        }
        // Show the scrolled window so GTK can complete the layout pass.
        scrolled_window.set_visible(true);
        // Defer clip calculation to the next idle tick so line_yrange
        // returns accurate heights after the widget is visible and laid out.
        glib::idle_add_local_once(move || {
            update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, scroll_to, line_count);
            loading_flag.set(false);
            // Signal the resize tick to refresh layout once line metrics
            // are valid (may take one or more frames after the scrolled
            // window becomes visible and GTK reflows the text).
            refresh_flag.set(true);
        });
    });
}

/// Update highlight and center the current line on screen.
pub fn update_highlight_and_center(state: &mut AppState) {
    update_highlight(state);
    let lpp = lines_per_page(state);
    let new_top = state.current_line.saturating_sub(lpp / 2);
    set_page_instant(state, new_top);
    auto_show_vocab_popup(state);
}

/// If vocab auto-popup is enabled, show/update the popup when the paragraph changes.
fn auto_show_vocab_popup(state: &mut AppState) {
    if !state.vocab_popup_auto {
        return;
    }
    // Refresh whenever the current line changes; the refresh function
    // decides whether to show (line has vocab words) or hide (line has none).
    if state.vocab_popup_line != Some(state.current_line) {
        if state.vocab_popup.is_visible() {
            crate::app::refresh_vocab_popup(state);
        } else {
            crate::app::open_vocab_popup(state);
        }
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

/// Update visual state for the current line. Only applies dim/cursor tags
/// to the visible range (page_top_line +/- margin) for performance.
/// When dim is off, fades out the old cursor highlight smoothly.
fn update_highlight(state: &mut AppState) {
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let cl_tag = &state.cursor_line_tag;
    let fade_tag = &state.cursor_fade_tag;

    // Compute visible range with margin for scroll overshoot
    let lpp = lines_per_page(state);
    let margin = 5;
    let vis_start = state.page_top_line.saturating_sub(margin);
    let vis_end = (state.page_top_line + lpp + margin)
        .min(state.effective_line_count());

    // Get iters for visible range
    let vis_start_iter = buffer.iter_at_line(vis_start as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let vis_end_iter = buffer.iter_at_line(vis_end as i32)
        .unwrap_or_else(|| buffer.end_iter());

    // Clear cursor line tag from full buffer (lightweight — only one line has it)
    let (buf_start, buf_end) = buffer.bounds();
    buffer.remove_tag(cl_tag, &buf_start, &buf_end);

    if !state.dim_enabled {
        // Remove dimming in visible range
        buffer.remove_tag(tag, &vis_start_iter, &vis_end_iter);

        // Apply fade-out to the old cursor line (if it changed)
        if state.config.show_cursor_line {
        if let Some(old_line) = state.prev_highlight_line.get() {
            if old_line != state.current_line {
                // Cancel any in-flight cursor fade
                if let Some(prev) = state.cursor_fade_anim.take() {
                    prev.skip();
                }

                // Remove any existing fade, then apply to old line
                buffer.remove_tag(fade_tag, &buf_start, &buf_end);
                if let Some(old_start) = buffer.iter_at_line(old_line as i32) {
                    let mut old_end = old_start;
                    if !old_end.ends_line() {
                        old_end.forward_to_line_end();
                    }
                    buffer.apply_tag(fade_tag, &old_start, &old_end);
                }

                // Animate fade-out: alpha from 1.0 → 0.0, 150ms, ease-out-quad
                let fade_tag_clone = fade_tag.clone();
                let buf_clone = buffer.clone();
                let (rc_r, rc_g, rc_b) = crate::theme::root_color_rgb(&state.theme.root_color);
                let fade_alpha_max = if state.theme.is_light { 0.13_f32 } else { 0.15_f32 };
                let target = adw::CallbackAnimationTarget::new(move |value| {
                    let alpha = value as f32 * fade_alpha_max;
                    use gtk4::prelude::TextTagExt;
                    fade_tag_clone.set_paragraph_background_rgba(Some(
                        &gtk4::gdk::RGBA::new(rc_r, rc_g, rc_b, alpha),
                    ));
                    if value <= 0.0 {
                        let (s, e) = buf_clone.bounds();
                        buf_clone.remove_tag(&fade_tag_clone, &s, &e);
                    }
                });
                // Need a widget to attach the animation to — use text_view
                let anim = adw::TimedAnimation::new(
                    &state.text_view,
                    1.0,  // from
                    0.0,  // to
                    700,  // duration ms
                    target,
                );
                anim.set_easing(adw::Easing::EaseOutQuad);
                anim.play();
                state.cursor_fade_anim = Some(anim);
            }
        }
        } // show_cursor_line

        // Apply cursor line background to new line (if enabled)
        if state.config.show_cursor_line {
            if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                buffer.apply_tag(cl_tag, &line_start, &line_end);
            }
        }
        // When visual selection is active, apply highlight even when dim is off
        crate::input::visual::clear_selection_highlight(state);
        crate::input::visual::apply_selection_highlight(state);
        state.prev_highlight_line.set(Some(state.current_line));
        return;
    }

    // Dim visible range
    buffer.apply_tag(tag, &vis_start_iter, &vis_end_iter);

    // Undim the current line
    if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.remove_tag(tag, &line_start, &line_end);
    }

    // When a chunk is active, undim lines within the chunk range
    // (only the portion that overlaps with visible range)
    if state.ab_repeat.chunk_index.is_some() {
        if let (Some(a), Some(b)) = (state.ab_repeat.a_line, state.ab_repeat.b_line) {
            let chunk_start = a.max(vis_start);
            let chunk_end = b.min(vis_end.saturating_sub(1));
            for line_idx in chunk_start..=chunk_end {
                if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
                    let mut line_end = line_start;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    buffer.remove_tag(tag, &line_start, &line_end);
                }
            }
        }
    }

    // When visual selection is active, clear stale highlight then re-apply
    crate::input::visual::clear_selection_highlight(state);
    crate::input::visual::apply_selection_highlight(state);
    state.prev_highlight_line.set(Some(state.current_line));
}


// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count how many buffer lines are fully visible starting from `page_top_line`.
/// Returns a calibrated estimate (35) during work load when GTK layout is
/// invalid, and a small fallback (15) for empty or past-end buffers.
fn lines_per_page(state: &AppState) -> usize {
    if state.loading_work.get() {
        return 35;
    }

    let line_count = state.effective_line_count();
    let start = state.page_top_line;

    if line_count == 0 || start >= line_count {
        return 15;
    }

    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return 15;
    }

    let descender_guard = descender_guard_px(&state.text_view, start);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        start,
        line_count,
        usable_height,
    );
    range.count.max(1)
}

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

// --- Cross-work concordance navigation ---

/// Jump to the current concordance occurrence.
/// Loads the work if different from current, positions cursor on the line.
pub fn concordance_jump_to_current(
    state: &Rc<RefCell<AppState>>,
    _handle: &tokio::runtime::Handle,
) {
    let (target_abbrev, target_line_id) = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        let hit = match conc.current_hit() {
            Some(h) => h,
            None => return,
        };
        (hit.work_abbrev.clone(), hit.line_mapping_id)
    };

    let current_abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());

    crate::logging::log(&format!(
        "CONC_JUMP: target_abbrev='{}' target_line_id={} current_abbrev={:?}",
        target_abbrev, target_line_id, current_abbrev
    ));

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        // Cross-work jump: spawn a new instance of linux-lit with the target work/line.
        // This avoids GTK lazy layout issues when loading large buffers in-process.
        crate::logging::log(&format!(
            "CONC_JUMP: spawning new instance for '{}' line_id={}", target_abbrev, target_line_id
        ));

        // Pause current MPV
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
        }

        let exe = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("target/debug/linux-lit"));
        let _ = std::process::Command::new(exe)
            .env("LINUX_LIT_WORK", &target_abbrev)
            .env("LINUX_LIT_LINE_ID", target_line_id.to_string())
            .spawn();
    } else {
        // Same work, just move cursor
        crate::logging::log("CONC_JUMP: same work, positioning cursor");
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
        crate::logging::log(&format!(
            "CONC_JUMP: positioned current_line={} page_top={}",
            s.current_line, s.page_top_line
        ));
        drop(s);
    }
}

/// Resolve the buffer index and sentence-start work index for a given line_mapping_id.
fn concordance_resolve_indices(state: &AppState, line_mapping_id: i64) -> Option<(usize, usize)> {
    let work = state.current_work.as_ref()?;
    let work_idx = work.lines.iter().position(|l| l.id == line_mapping_id)?;

    let buf_idx = if let Some(ref lm) = state.line_map {
        lm.work_to_buffer[work_idx]
    } else {
        work_idx
    };

    let seek_work_idx = if let Some(ref lm) = state.line_map {
        if let Some(sg) = crate::text_file_map::sentence_group_for(&lm.sentence_groups, buf_idx) {
            lm.buffer_to_work.get(sg.line_range.start)
                .copied()
                .flatten()
                .unwrap_or(work_idx)
        } else {
            find_sentence_start_by_timestamp(work, work_idx)
        }
    } else {
        find_sentence_start_by_timestamp(work, work_idx)
    };

    Some((buf_idx, seek_work_idx))
}

/// Seek MPV to the sentence start for a concordance hit.
fn concordance_seek(state: &mut AppState, seek_work_idx: usize) {
    if let Some(work) = &state.current_work {
        if let Some(ts) = work.lines.get(seek_work_idx).and_then(|l| l.timestamp.as_ref()) {
            state.suppress_sync_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
            let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
            let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
        }
    }
}

/// Position cursor on the line with the given line_mapping_id (same-work case).
/// The buffer layout is valid, so we scroll immediately.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    let (buf_idx, seek_work_idx) = match concordance_resolve_indices(state, line_mapping_id) {
        Some(v) => v,
        None => {
            crate::logging::log(&format!(
                "CONC_POS: resolve failed for line_mapping_id={}", line_mapping_id
            ));
            return;
        }
    };
    crate::logging::log(&format!(
        "CONC_POS: same-work buf_idx={} seek_work_idx={} line_mapping_id={}",
        buf_idx, seek_work_idx, line_mapping_id
    ));
    state.current_line = buf_idx;
    update_highlight(state);
    center_cursor(state);
    concordance_seek(state, seek_work_idx);
}

/// Find the first work-line index sharing the same sentence_start_time as `work_idx`.
/// Falls back to `work_idx` itself if no sentence time data is available.
fn find_sentence_start_by_timestamp(work: &crate::db::models::Work, work_idx: usize) -> usize {
    let target_ss = work.lines[work_idx]
        .timestamp
        .as_ref()
        .and_then(|t| t.sentence_start);
    let target_ss = match target_ss {
        Some(ss) => ss,
        None => return work_idx,
    };
    // Scan backwards to find the first line with the same sentence_start_time
    for i in (0..work_idx).rev() {
        let ss = work.lines[i]
            .timestamp
            .as_ref()
            .and_then(|t| t.sentence_start);
        match ss {
            Some(s) if (s - target_ss).abs() < 0.001 => continue,
            _ => return i + 1,
        }
    }
    0
}

/// Update the concordance status bar from current state.
fn concordance_update_bar(state: &AppState) {
    if let Some(conc) = &state.concordance_state {
        state
            .concordance_bar
            .update(&conc.status_label(), &conc.status_work());
    }
}

/// Kick off background preload of the next work in the concordance direction.
/// Cycle through words on the current line, copying each to the system clipboard.
/// Each press advances to the next word; wraps after the last word.
/// Briefly bolds the word in the buffer for 2 seconds.
pub fn word_cycle_copy(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    // Reset index if we moved to a different line
    let idx = if state.word_cycle_line == Some(state.current_line) {
        state.word_cycle_index % words.len()
    } else {
        0
    };

    let (ref word, char_start, char_end) = words[idx];

    // Copy to clipboard via wl-copy
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(word.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COPY: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!("WORD_COPY: copied '{}' (word {}/{})", word, idx + 1, words.len());

    // Update cycle state
    state.word_cycle_line = Some(state.current_line);
    state.word_cycle_index = idx + 1;

    // Clear multi-word collect state (w is single-word mode)
    state.word_collect_words.clear();
    state.word_collect_ranges.clear();

    // Remove any previous underline tag, then apply to the current word
    apply_word_underline(state, &[(char_start, char_end)]);
}

/// Collect words on the current line, accumulating across presses.
/// Each W press advances to the next word, appends it to the collection,
/// and copies all collected words (space-separated) to the clipboard.
/// Underlines all collected words. Resets on line change.
pub fn word_collect_copy(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    // Reset if we moved to a different line
    let idx = if state.word_cycle_line == Some(state.current_line) {
        state.word_cycle_index % words.len()
    } else {
        state.word_collect_words.clear();
        state.word_collect_ranges.clear();
        0
    };

    let (ref word, char_start, char_end) = words[idx];

    // Append to collection
    state.word_collect_words.push(word.clone());
    state.word_collect_ranges.push((char_start, char_end));

    // Copy all collected words to clipboard
    let phrase = state.word_collect_words.join(" ");
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(phrase.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COLLECT: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!("WORD_COLLECT: copied '{}' ({} words)", phrase, state.word_collect_words.len());

    // Update cycle state
    state.word_cycle_line = Some(state.current_line);
    state.word_cycle_index = idx + 1;

    // Underline all collected words
    let ranges: Vec<(usize, usize)> = state.word_collect_ranges.clone();
    apply_word_underline(state, &ranges);
}

/// Extract words from the current buffer line with their char offsets.
fn extract_buffer_line_words(state: &AppState) -> Vec<(String, usize, usize)> {
    let buf_line_start = state.buffer.iter_at_line(state.current_line as i32).unwrap();
    let buf_line_end = {
        let mut it = buf_line_start;
        if !it.ends_line() { it.forward_to_line_end(); }
        it
    };
    let buf_line_text = state.buffer.text(&buf_line_start, &buf_line_end, false).to_string();

    let mut words: Vec<(String, usize, usize)> = Vec::new();
    for token in buf_line_text.split_whitespace() {
        let token_byte_start = token.as_ptr() as usize - buf_line_text.as_ptr() as usize;
        let token_char_start = buf_line_text[..token_byte_start].chars().count();
        let stripped = token.trim_matches(|c: char| !c.is_alphanumeric());
        if stripped.is_empty() {
            continue;
        }
        let strip_byte_offset = stripped.as_ptr() as usize - token.as_ptr() as usize;
        let char_start = token_char_start + token[..strip_byte_offset].chars().count();
        let char_end = char_start + stripped.chars().count();
        words.push((stripped.to_string(), char_start, char_end));
    }
    words
}

/// Apply the underline tag to the given char ranges on the current line,
/// removing any previous underline first. Auto-removes after 2 seconds.
fn apply_word_underline(state: &mut AppState, ranges: &[(usize, usize)]) {
    let buf = &state.buffer;
    let tag = &state.word_bold_tag;
    let (buf_start, buf_end) = (buf.start_iter(), buf.end_iter());
    buf.remove_tag(tag, &buf_start, &buf_end);

    let line_start = buf.iter_at_line(state.current_line as i32).unwrap();
    for &(char_start, char_end) in ranges {
        let mut word_start = line_start;
        word_start.forward_chars(char_start as i32);
        let mut word_end = word_start;
        word_end.forward_chars((char_end - char_start) as i32);
        buf.apply_tag(tag, &word_start, &word_end);
    }

    // Auto-remove underline after 2 seconds
    let gen = state.word_bold_gen.get() + 1;
    state.word_bold_gen.set(gen);
    let gen_rc = state.word_bold_gen.clone();
    let buf_clone = buf.clone();
    let tag_clone = tag.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        if gen_rc.get() == gen {
            let (s, e) = (buf_clone.start_iter(), buf_clone.end_iter());
            buf_clone.remove_tag(&tag_clone, &s, &e);
        }
    });
}

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

    /// Test page-backward uses history stack: forward all the way, then backward
    /// all the way using the recorded history. Each backward step must return to
    /// the exact previous page_top, and the round-trip must reach the start.
    #[test]
    fn test_page_backward_via_history() {
        let lines = load_troilus_lines();
        let all_dialogue = dialogue_indices(&lines);

        let page_size = 30;
        let line_count = lines.len();

        // Page forward to the end, recording history (simulating page_history)
        let first = all_dialogue[0];
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut history: Vec<usize> = Vec::new();
        let mut forward_tops: Vec<usize> = vec![page_top];

        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 500 { break; }
            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            let last = last_dialogue_in_range(&lines, page_top, last_visible - page_top + 1);
            let next = match next_dialogue(&lines, last + 1) {
                Some(n) => n,
                None => break,
            };
            if next >= line_count { break; }
            // Push current page_top before advancing (like the real code)
            history.push(page_top);
            let new_top = back_up_for_speaker(&lines, next);
            page_top = new_top;
            forward_tops.push(page_top);
        }

        // Now page backward using history (like the real code)
        let mut backward_tops: Vec<usize> = vec![page_top];
        while let Some(prev_top) = history.pop() {
            page_top = prev_top;
            backward_tops.push(page_top);
        }

        // Verify: backward tops are strictly decreasing
        for i in 1..backward_tops.len() {
            assert!(
                backward_tops[i] < backward_tops[i - 1],
                "History backward: top {} is not before {} at step {}",
                backward_tops[i], backward_tops[i - 1], i
            );
        }

        // Verify: backward reaches the first page
        assert_eq!(
            *backward_tops.last().unwrap(), forward_tops[0],
            "Backward didn't reach the first page"
        );

        // Verify: backward is exact reverse of forward
        assert_eq!(
            backward_tops.len(), forward_tops.len(),
            "Forward {} pages but backward {} pages",
            forward_tops.len(), backward_tops.len()
        );
        for i in 0..forward_tops.len() {
            assert_eq!(
                forward_tops[i],
                backward_tops[backward_tops.len() - 1 - i],
                "Round-trip mismatch at page {}: forward={} backward={}",
                i, forward_tops[i], backward_tops[backward_tops.len() - 1 - i]
            );
        }

        println!(
            "Page backward (history) test passed: {} pages, {} down to {}",
            backward_tops.len(),
            backward_tops[0],
            backward_tops.last().unwrap(),
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
mod page_turn_lock_tests {
    use super::PageTurnLock;

    #[test]
    fn try_acquire_succeeds_when_unlocked() {
        let lock = PageTurnLock::new();
        assert!(lock.try_acquire(), "first acquire should succeed");
        assert!(lock.is_locked(), "lock should be held after acquire");
    }

    #[test]
    fn try_acquire_fails_when_locked() {
        let lock = PageTurnLock::new();
        assert!(lock.try_acquire());
        assert!(!lock.try_acquire(), "second acquire should fail");
        assert!(lock.is_locked(), "lock should still be held after rejected acquire");
    }

    #[test]
    fn release_clears_the_lock() {
        let lock = PageTurnLock::new();
        lock.try_acquire();
        lock.release();
        assert!(!lock.is_locked(), "release should clear the lock");
        assert!(lock.try_acquire(), "acquire should succeed after release");
    }

    #[test]
    fn release_when_unlocked_is_a_noop() {
        let lock = PageTurnLock::new();
        lock.release();
        lock.release();
        assert!(!lock.is_locked());
        assert!(lock.try_acquire());
    }

    #[test]
    fn double_release_does_not_re_lock() {
        let lock = PageTurnLock::new();
        lock.try_acquire();
        lock.release();
        lock.release();
        assert!(!lock.is_locked());
    }
}

#[cfg(test)]
mod visible_range_helpers_tests {
    use super::{VisibleRange, trim_trailing_speakers_pure};

    fn line_classifier(speakers_or_blanks: &[usize]) -> impl Fn(usize) -> bool + '_ {
        move |i| speakers_or_blanks.contains(&i)
    }

    fn line_height(_i: usize) -> i32 {
        20
    }

    #[test]
    fn trim_with_no_trailing_speaker_is_identity() {
        let range = VisibleRange { last_fit: 5, total_height: 100, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.total_height, 100);
        assert_eq!(trimmed.count, 6);
    }

    #[test]
    fn trim_drops_one_trailing_speaker() {
        let range = VisibleRange { last_fit: 5, total_height: 100, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 4);
        assert_eq!(trimmed.total_height, 80);
        assert_eq!(trimmed.count, 5);
    }

    #[test]
    fn trim_drops_speaker_with_preceding_blanks() {
        // Lines 3 (blank), 4 (blank), 5 (speaker) — all trim.
        let range = VisibleRange { last_fit: 5, total_height: 120, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[3, 4, 5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 2);
        assert_eq!(trimmed.total_height, 60);
        assert_eq!(trimmed.count, 3);
    }

    #[test]
    fn trim_stops_at_dialogue_line() {
        // Line 5 is speaker, line 4 is dialogue (not in classifier), line 3 is blank.
        // Trim removes line 5 only — line 4 is dialogue, blocks further trim.
        let range = VisibleRange { last_fit: 5, total_height: 120, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[3, 5]), // 4 is dialogue
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 4);
        assert_eq!(trimmed.total_height, 100);
        assert_eq!(trimmed.count, 5);
    }

    #[test]
    fn trim_does_not_cross_page_top() {
        // Every line is a speaker, but page_top is 3 — trim must not delete the
        // page top itself (would leave an empty page).
        let range = VisibleRange { last_fit: 5, total_height: 60, count: 3 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            3,
            &line_classifier(&[3, 4, 5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 3, "must leave page_top in place");
        assert!(trimmed.total_height > 0);
        assert!(trimmed.count >= 1);
    }

    #[test]
    fn trim_with_empty_range_is_noop() {
        // last_fit == page_top, count == 1 — nothing to trim.
        let range = VisibleRange { last_fit: 0, total_height: 20, count: 1 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[0]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 0);
        assert_eq!(trimmed.total_height, 20);
        assert_eq!(trimmed.count, 1);
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
    fn reason_always_updates_label_except_workload() {
        assert!(PageChangeReason::Forward.should_update_label());
        assert!(PageChangeReason::MpvSync.should_update_label());
        assert!(PageChangeReason::Resnap.should_update_label());
        assert!(!PageChangeReason::WorkLoad.should_update_label());
    }

    #[test]
    fn reason_skips_seek_for_cursor_only_navigation() {
        assert!(!PageChangeReason::Cursor.should_seek(),
            "cursor-only navigation must not drag audio");
        assert!(PageChangeReason::Cursor.should_show_vocab(),
            "cursor navigation still shows vocab");
        assert!(PageChangeReason::Cursor.should_update_label());
    }
}

#[cfg(test)]
mod page_tops_tests {
    use super::page_for_line_in_index;

    #[test]
    fn page_for_line_returns_1_for_empty_index() {
        let tops: Vec<usize> = vec![];
        assert_eq!(page_for_line_in_index(&tops, 0), 1);
        assert_eq!(page_for_line_in_index(&tops, 100), 1);
    }

    #[test]
    fn page_for_line_returns_1_for_first_page() {
        let tops = vec![0, 35, 70, 105]; // page 1 starts at 0, page 2 at 35, etc.
        assert_eq!(page_for_line_in_index(&tops, 0), 1);
        assert_eq!(page_for_line_in_index(&tops, 10), 1);
        assert_eq!(page_for_line_in_index(&tops, 34), 1);
    }

    #[test]
    fn page_for_line_returns_2_for_second_page() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(page_for_line_in_index(&tops, 35), 2);
        assert_eq!(page_for_line_in_index(&tops, 50), 2);
        assert_eq!(page_for_line_in_index(&tops, 69), 2);
    }

    #[test]
    fn page_for_line_handles_target_past_end() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(page_for_line_in_index(&tops, 200), 4);
    }

    #[test]
    fn page_for_line_exact_top_match() {
        let tops = vec![0, 35, 70];
        // line 35 is the START of page 2 — partition_point gives index 2,
        // page = 2.
        assert_eq!(page_for_line_in_index(&tops, 35), 2);
    }
}

#[cfg(test)]
mod prev_page_top_tests {
    // Pure tests against the page_tops index lookup. The cold-walk and
    // lpp-fallback paths require GTK and are exercised in manual verification.
    use super::page_for_line_in_index;

    /// Pure helper mirroring prev_page_top's binary_search fast path,
    /// extracted for unit testing without a full AppState.
    fn prev_top_via_index(tops: &[usize], current_top: usize) -> Option<usize> {
        if current_top == 0 {
            return Some(0);
        }
        match tops.binary_search(&current_top) {
            Ok(idx) if idx > 0 => Some(tops[idx - 1]),
            _ => None,
        }
    }

    #[test]
    fn prev_top_returns_zero_for_current_zero() {
        let tops = vec![0, 35, 70];
        assert_eq!(prev_top_via_index(&tops, 0), Some(0));
    }

    #[test]
    fn prev_top_finds_previous_boundary() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(prev_top_via_index(&tops, 70), Some(35));
        assert_eq!(prev_top_via_index(&tops, 35), Some(0));
        assert_eq!(prev_top_via_index(&tops, 105), Some(70));
    }

    #[test]
    fn prev_top_returns_none_for_off_boundary_target() {
        let tops = vec![0, 35, 70];
        // 40 is not a page top; binary_search returns Err — caller falls
        // back to cold-walk or lpp.
        assert_eq!(prev_top_via_index(&tops, 40), None);
    }

    #[test]
    fn page_for_line_after_prev_top_is_consistent() {
        // Sanity: if prev_page_top returns prev_top, page_for_line_in_index
        // for any line in [prev_top, current_top) should return the page
        // index of prev_top. This is the bidirectional symmetry property.
        let tops = vec![0, 35, 70, 105];
        let prev = prev_top_via_index(&tops, 70).unwrap();
        assert_eq!(prev, 35);
        // Line 50 is on the page that starts at 35 — page 2.
        assert_eq!(page_for_line_in_index(&tops, 50), 2);
    }
}

#[cfg(test)]
mod block_atom_tests {
    use super::{VisibleRange, block_start_for_line_pure, trim_block_atoms_pure};

    /// Build classifiers for a synthetic line array.
    /// `kinds` maps line index → 'b' (blank), 's' (speaker), 'd' (stage dir),
    /// 'l' (dialogue line).
    fn classifiers(kinds: &[char]) -> (
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
    ) {
        let is_blank = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'b');
        let is_speaker = move |i: usize| kinds.get(i).map_or(false, |c| *c == 's');
        let is_stage = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'd');
        let is_dialogue = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'l');
        (is_blank, is_speaker, is_stage, is_dialogue)
    }

    #[test]
    fn block_start_in_3line_stage_direction_returns_first_dir_line() {
        // Lines: 0=speaker, 1=dir, 2=dir, 3=dir
        let kinds = ['s', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-block), page_top=0, is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "should back up to first stage-direction line");
    }

    #[test]
    fn block_start_at_block_start_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=dir
        let kinds = ['s', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (at block start), page_top=0
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "no backup when last_fit is already block start");
    }

    #[test]
    fn block_start_in_verse_stanza_non_prose_returns_stanza_start() {
        // Lines: 0=speaker, 1=l, 2=l, 3=l, 4=blank
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-stanza), is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "verse stanza in non-prose work backs up to stanza start");
    }

    #[test]
    fn block_start_in_verse_stanza_prose_returns_unchanged() {
        // Same lines, but is_prose=true — rule does not apply.
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, true, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 3, "verse stanza rule skipped for prose works");
    }

    #[test]
    fn block_start_single_line_stage_direction_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=l
        let kinds = ['s', 'd', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (single dir line)
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "single-line stage direction is not multi-line — no backup");
    }

    #[test]
    fn block_start_stops_at_speaker() {
        // Lines: 0=blank, 1=speaker, 2=l, 3=l (stanza bounded above by speaker)
        let kinds = ['b', 's', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3, page_top=0
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 2, "stanza backup stops at speaker line (returns first dialogue line)");
    }

    #[test]
    fn block_start_stops_at_blank() {
        // Lines: 0=l, 1=blank, 2=l, 3=l (stanza bounded above by blank)
        let kinds = ['l', 'b', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 2, "stanza backup stops at blank line (returns first dialogue line after blank)");
    }

    #[test]
    fn block_start_in_verse_stanza_bounded_above_by_stage_direction() {
        // Lines: 0=stage, 1=l, 2=l (stanza after a stage direction)
        let kinds = ['d', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 2, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "stanza backup stops at stage direction (returns first dialogue line after stage)");
    }

    #[test]
    fn trim_block_atoms_block_fully_fits_reduces_count() {
        // Block at lines 3-5; visible range stops at 4 (mid-block — line 5
        // would render off-screen). Trim backs up to before block start.
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        // Lines 0=speaker, 1=l, 2=l, 3=d, 4=d, 5=d (stage block extends past last_fit)
        let kinds = ['s', 'l', 'l', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        // Block starts at 3 and continues past last_fit (line 5 is also stage);
        // new last_fit = 2; new count = 3 (lines 0,1,2).
        assert_eq!(trimmed.last_fit, 2);
        assert_eq!(trimmed.count, 3);
        assert_eq!(trimmed.total_height, 60); // dropped lines 3 and 4 at 20px each
    }

    #[test]
    fn trim_block_atoms_block_ends_at_last_fit_unchanged() {
        // Block at lines 3-4 ends exactly at last_fit (line 5 is dialogue,
        // outside the stage block). No mid-block split — leave range as-is.
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        let kinds = ['s', 'l', 'l', 'd', 'd', 'l']; // line 5 is dialogue, ends the stage block
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 4, "block ends at last_fit; no trim");
        assert_eq!(trimmed.count, 5);
        assert_eq!(trimmed.total_height, 100);
    }

    #[test]
    fn trim_block_atoms_verse_stanza_ends_at_last_fit_unchanged() {
        // 2-line verse stanza (lines 3-4) ends at last_fit; line 5 is the
        // next speaker, closing the stanza. No trim — stanza fully fits.
        // This is the bug case from manual verification: pressing j with
        // cursor on the previous speaker's last dialogue should not
        // trigger a page-turn that removes a fully-visible 2-line stanza.
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        let kinds = ['s', 'l', 'l', 'l', 'l', 's']; // last_fit=4 dialogue, line 5 speaker closes stanza
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 4, "stanza ends at last_fit; no trim");
        assert_eq!(trimmed.count, 5);
        assert_eq!(trimmed.total_height, 100);
    }

    #[test]
    fn trim_block_atoms_block_doesnt_fit_returns_unchanged() {
        // Whole page IS the block — block_start (== 0) <= page_top (== 0).
        let range = VisibleRange { last_fit: 3, total_height: 80, count: 4 };
        let kinds = ['d', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 3);
        assert_eq!(trimmed.count, 4);
        assert_eq!(trimmed.total_height, 80);
    }

    #[test]
    fn trim_block_atoms_block_too_tall_keeps_per_line_split() {
        // Page-top is a speaker; verse stanza extends from line 1 down past
        // the visible area. block_start = 1 (just below page_top), block_end
        // = 9 (last_fit). Trimming would leave only line 0 (the speaker)
        // visible — useless. Overflow guard fires (new_count=1, range.count=10,
        // 1 * 2 <= 10 → keep per-line split).
        let range = VisibleRange { last_fit: 9, total_height: 200, count: 10 };
        let kinds = ['s', 'l', 'l', 'l', 'l', 'l', 'l', 'l', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 9, "block too tall: keep original last_fit");
        assert_eq!(trimmed.count, 10);
        assert_eq!(trimmed.total_height, 200);
    }

    #[test]
    fn trim_block_atoms_empty_range_unchanged() {
        let range = VisibleRange { last_fit: 0, total_height: 0, count: 0 };
        let kinds: [char; 0] = [];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.count, 0);
    }

    #[test]
    fn trim_block_atoms_at_page_top_unchanged() {
        // last_fit equals page_top — trim is no-op (already minimal).
        let range = VisibleRange { last_fit: 5, total_height: 20, count: 1 };
        let kinds = ['l', 'l', 'l', 'l', 'l', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 5, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.count, 1);
    }
}
