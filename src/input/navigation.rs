use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

// Page overlap: when turning pages, this many lines from the old page
// remain visible on the new page for reading continuity.
const PAGE_OVERLAP: usize = 1;

/// Seconds to seek before a line's start_time when navigating.
/// Provides audio context so playback doesn't start at a hard cut.
pub const SEEK_PREROLL: f64 = 0.2;

/// Seconds to highlight a line before playback actually reaches it.
/// Used by the MPV client's time-pos sync.
pub const SYNC_PREROLL: f64 = 0.3;

/// Move cursor by `delta` lines (j/k).
/// Going down: page turn when cursor reaches the last visible line.
/// Going up: smooth scroll to keep cursor visible.
pub fn move_cursor(state: &mut AppState, delta: i32) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

    let mut new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    // Skip over translation lines
    if state.translations_visible && !state.translation_lines.is_empty() {
        let direction = if delta > 0 { 1i32 } else { -1i32 };
        while new_line < state.translation_lines.len()
            && state.translation_lines[new_line]
        {
            let next = new_line as i32 + direction;
            if next < 0 || next >= line_count as i32 {
                break;
            }
            new_line = next as usize;
        }
    }

    if new_line == state.current_line {
        return;
    }

    state.current_line = new_line;
    update_highlight(state);

    center_cursor(state);

    auto_show_vocab_popup(state);
}

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
    update_highlight(state);
    set_page_instant(state, target);
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
    state.current_line = line_count - 1;
    update_highlight(state);
    let lpp = lines_per_page(state);
    let new_top = line_count.saturating_sub(lpp);
    set_page_instant(state, new_top);
}

/// Page forward (Ctrl+d/f). Advance by one page with overlap.
pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

    let lpp = lines_per_page(state);
    let advance = lpp.saturating_sub(PAGE_OVERLAP).max(1);
    let new_top = (state.page_top_line + advance).min(line_count.saturating_sub(1));

    // Move cursor to center of the new page
    state.current_line = (new_top + lpp / 2).min(line_count - 1);
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top);
}

/// Page backward (Ctrl+u/b). Go back by one page with overlap.
pub fn page_backward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let lpp = lines_per_page(state);
    let retreat = lpp.saturating_sub(PAGE_OVERLAP).max(1);
    let new_top = state.page_top_line.saturating_sub(retreat);

    let line_count = state.effective_line_count();
    // Move cursor to center of the new page
    state.current_line = (new_top + lpp / 2).min(line_count.saturating_sub(1));
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top);
}

/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
/// Otherwise, jump to the previous dialogue line.
/// When a media_id is active, skips lines marked as not spoken in that media.
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    let has_media = state.media_id.is_some();
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if state.current_line == 0 {
            return;
        }

        if let Some(ref lm) = state.line_map {
            // Prose with sentence groups: navigate between sentences
            if !lm.sentence_groups.is_empty() {
                prev_sentence_start(&lm.sentence_groups, state.current_line)
            } else {
                let lines = if has_media {
                    &lm.spoken_dialogue_buffer_lines
                } else {
                    &lm.dialogue_buffer_lines
                };
                lines
                    .iter()
                    .rev()
                    .find(|&&bl| bl < state.current_line)
                    .copied()
            }
        } else {
            let mut found = None;
            for i in (0..state.current_line).rev() {
                if work.lines[i].is_dialogue
                    && (!has_media || work.lines[i].is_spoken != Some(false))
                {
                    found = Some(i);
                    break;
                }
            }
            found
        }
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => scroll_to_cursor(state),
        }
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
}

/// Next dialogue line (`q` key).
/// Jump to next dialogue line. Page turn when target is not fully visible.
/// When a media_id is active, skips lines marked as not spoken in that media.
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let has_media = state.media_id.is_some();
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        if let Some(ref lm) = state.line_map {
            // Prose with sentence groups: navigate between sentences
            if !lm.sentence_groups.is_empty() {
                next_sentence_start(&lm.sentence_groups, state.current_line)
            } else {
                let lines = if has_media {
                    &lm.spoken_dialogue_buffer_lines
                } else {
                    &lm.dialogue_buffer_lines
                };
                lines.iter().find(|&&bl| bl > state.current_line).copied()
            }
        } else {
            let line_count = work.lines.len();
            let mut found = None;
            for i in (state.current_line + 1)..line_count {
                if work.lines[i].is_dialogue
                    && (!has_media || work.lines[i].is_spoken != Some(false))
                {
                    found = Some(i);
                    break;
                }
            }
            found
        }
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                if needs_page_turn_down(state, line_idx) {
                    set_page(state, line_idx);
                }
            }
        }
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
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
        if let Some(ref lm) = state.line_map {
            let mut found = None;
            for bl in (0..state.current_line).rev() {
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
            for i in (0..state.current_line).rev() {
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
        update_highlight(state);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => scroll_to_cursor(state),
        }
        seek_to_current_line(state);
    }
}

/// Next chapter line (`{` key).
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
        update_highlight(state);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                if needs_page_turn_down(state, line_idx) {
                    set_page(state, line_idx);
                }
            }
        }
        seek_to_current_line(state);
    }
}

/// Restore cursor position after loading a work (used on startup with MRU).
pub fn restore_cursor(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => {
            let adj = state.scrolled_window.vadjustment();
            let max_scroll = adj.upper() - adj.page_size();
            let line_y = scroll_value_for_line(state, state.current_line);
            let offset = adj.page_size() * 0.35;
            let centered = (line_y - offset).max(0.0).min(max_scroll.max(0.0));
            adj.set_value(centered);
        }
        crate::config::NavigationMode::EReader => {
            let new_top = state.current_line.saturating_sub(PAGE_OVERLAP);
            set_page_instant(state, new_top);
        }
    }
}

// ---------------------------------------------------------------------------
// Page management
// ---------------------------------------------------------------------------

/// Ensure cursor is on the current page. If not, page turn to show it.
/// For backward movement, cursor appears near page bottom.
/// For forward movement, cursor appears near page top.
fn ensure_cursor_on_page(state: &mut AppState) {
    let page_top = state.page_top_line;
    let lpp = lines_per_page(state);
    let page_bottom = page_top + lpp;

    crate::logging::log(&format!(
        "ENSURE: cursor={} page=[{}..{}) lpp={}",
        state.current_line, page_top, page_bottom, lpp
    ));

    let page_last = page_top + lpp.saturating_sub(1);

    if state.current_line >= page_top && state.current_line < page_last {
        return; // within page and not at the last line
    }

    if state.current_line < page_top {
        // Went above — new page with cursor near bottom
        let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
        set_page(state, new_top);
    } else {
        // At or past last line — new page with this line at top
        set_page(state, state.current_line);
    }
}

/// Check if a line is fully visible within the viewport.
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return false;
    };
    let visible = state.text_view.visible_rect();
    let loc = state.text_view.iter_location(&iter);
    loc.y() >= visible.y() && loc.y() + loc.height() <= visible.y() + visible.height()
}

/// Check if a line is the last visible line and has no visible blank space
/// after it (i.e., the next line is not visible or doesn't exist).
/// Used to trigger page turns when the bottom of the viewport is full.
fn needs_page_turn_down(state: &AppState, line: usize) -> bool {
    if !is_line_fully_visible(state, line) {
        return true;
    }
    // Check if the next line's top is visible — if not, we're at the edge
    let line_count = state.effective_line_count();
    if line + 1 >= line_count {
        return false; // at end of document
    }
    !is_line_fully_visible(state, line + 1)
}

/// Scroll just enough to keep the current line visible. No page turn.
fn scroll_to_cursor(state: &mut AppState) {
    if let Some(iter) = state.buffer.iter_at_line(state.current_line as i32) {
        let visible = state.text_view.visible_rect();
        let loc = state.text_view.iter_location(&iter);

        if loc.y() < visible.y() {
            // Cursor above viewport — scroll up
            let target = scroll_value_for_line(state, state.current_line);
            state.scrolled_window.vadjustment().set_value(target);
            state.page_top_line = state.current_line;
        } else if loc.y() + loc.height() > visible.y() + visible.height() {
            // Cursor below viewport — scroll so cursor is at bottom
            let adj = state.scrolled_window.vadjustment();
            let target = (loc.y() + loc.height()) as f64 - adj.page_size();
            adj.set_value(target.max(0.0));
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
    let offset = adj.page_size() * 0.35;
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
fn seek_to_current_line(state: &mut AppState) {
    let has_timestamp = state
        .current_work
        .as_ref()
        .and_then(|work| {
            state
                .work_line_for_buffer(state.current_line)
                .and_then(|wi| work.lines[wi].timestamp.as_ref())
        })
        .is_some();

    if has_timestamp {
        // Brief suppression while MPV processes the seek
        state.suppress_sync_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
        if let Some(ref work) = state.current_work {
            if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
                if let Some(ts) = &work.lines[work_idx].timestamp {
                    let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
                    let _ = state
                        .cmd_tx
                        .try_send(crate::mpv::MpvCommand::Seek(seek_time));
                }
            }
        }
    } else {
        // No timestamp — suppress indefinitely so cursor stays put
        state.suppress_sync_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(86400));
    }
}

/// Set the page top line and scroll to it with crossfade animation.
fn set_page(state: &mut AppState, new_top: usize) {
    state.page_top_line = new_top;
    let target = scroll_value_for_line(state, new_top);
    crate::logging::log(&format!(
        "PAGE_TURN: top_line={} cursor={} target={:.0}",
        new_top, state.current_line, target
    ));
    crossfade_to(state, target);
}

/// Set the page top line and scroll instantly (no animation). For gg/G/restore.
fn set_page_instant(state: &mut AppState, new_top: usize) {
    state.page_top_line = new_top;
    let target = scroll_value_for_line(state, new_top);
    state.scrolled_window.vadjustment().set_value(target);
}

/// Scroll the viewport by a fixed step without moving the cursor or seeking audio.
/// `delta` is +1 for down, -1 for up.  Scrolls by approximately one wrapped line height.
pub fn scroll_viewport(state: &mut AppState, delta: i32) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    // Use the height of the current line as the scroll step
    let step = state.buffer.iter_at_line(state.current_line as i32)
        .map(|iter| {
            let rect = state.text_view.iter_location(&iter);
            rect.height() as f64
        })
        .unwrap_or(30.0)
        .max(20.0);
    let new_val = (adj.value() + step * delta as f64).max(0.0).min(max_scroll);
    adj.set_value(new_val);
}

/// Get the vadjustment value that places the given line at the top of the viewport.
/// Uses the previous line's bottom to avoid its tail peeking above the target line.
fn scroll_value_for_line(state: &AppState, line: usize) -> f64 {
    let adj = state.scrolled_window.vadjustment();
    let max = adj.upper() - adj.page_size();

    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return 0.0;
    };
    let rect = state.text_view.iter_location(&iter);
    (rect.y() as f64).max(0.0).min(max)
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

/// Scroll to make the current line visible without re-applying highlight.
/// Used for deferred prose sentence scrolling.
pub fn ensure_visible_no_highlight(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, state.current_line) {
                set_page(state, state.current_line);
            } else {
                ensure_cursor_on_page(state);
            }
        }
    }
}

pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, state.current_line) {
                set_page(state, state.current_line);
            } else {
                ensure_cursor_on_page(state);
            }
        }
    }
    auto_show_vocab_popup(state);
}

/// Update highlight and center the current line on screen.
pub fn update_highlight_and_center(state: &mut AppState) {
    update_highlight(state);
    let lpp = lines_per_page(state);
    let new_top = state.current_line.saturating_sub(lpp / 2);
    set_page_instant(state, new_top);
    auto_show_vocab_popup(state);
}

/// If vocab auto-popup is enabled, show/update the popup for the current line.
fn auto_show_vocab_popup(state: &mut AppState) {
    if !state.vocab_popup_auto {
        return;
    }
    // If popup is already visible, refresh it for the new line
    if state.vocab_popup.is_visible() {
        crate::app::refresh_vocab_popup(state);
    } else {
        crate::app::open_vocab_popup(state);
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

/// Find the start of the previous sentence group relative to `current_line`.
fn prev_sentence_start(groups: &[crate::text_file_map::SentenceGroup], current_line: usize) -> Option<usize> {
    for g in groups.iter().rev() {
        if g.line_range.start < current_line {
            return Some(g.line_range.start);
        }
    }
    None
}

/// Find the start of the next sentence group relative to `current_line`.
fn next_sentence_start(groups: &[crate::text_file_map::SentenceGroup], current_line: usize) -> Option<usize> {
    for g in groups.iter() {
        if g.line_range.start > current_line {
            return Some(g.line_range.start);
        }
    }
    None
}

/// Dim all lines except the current one. The current line keeps full foreground.
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let (buf_start, buf_end) = buffer.bounds();

    // Apply dim to entire buffer
    buffer.apply_tag(tag, &buf_start, &buf_end);

    // Remove dim from current sentence group (with character-level precision on boundary lines)
    let sentence_group = state.line_map.as_ref().and_then(|lm| {
        crate::text_file_map::sentence_group_for(&lm.sentence_groups, state.current_line)
    });
    if let Some(group) = sentence_group {
        let first_line = group.line_range.start;
        let last_line = group.line_range.end.saturating_sub(1);
        for line_idx in group.line_range.clone() {
            if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }

                let undim_start = if line_idx == first_line && group.start_col > 0 {
                    // First line: undim from start_col to end of line
                    let mut iter = line_start;
                    iter.set_line_offset(group.start_col as i32);
                    iter
                } else {
                    line_start
                };

                let undim_end = if line_idx == last_line {
                    if let Some(end_col) = group.end_col {
                        // Last line: undim from start of line to end_col
                        let mut iter = line_start;
                        iter.set_line_offset(end_col as i32);
                        iter
                    } else {
                        line_end
                    }
                } else {
                    line_end
                };

                buffer.remove_tag(tag, &undim_start, &undim_end);
            }
        }
    } else if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.remove_tag(tag, &line_start, &line_end);
    }

    // When a chunk is active, undim all lines within the chunk range
    if state.ab_repeat.chunk_index.is_some() {
        if let (Some(a), Some(b)) = (state.ab_repeat.a_line, state.ab_repeat.b_line) {
            for line_idx in a..=b {
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

    // When visual selection is active, undim and highlight selected lines
    crate::input::visual::apply_selection_highlight(state);
}

// ---------------------------------------------------------------------------
// Crossfade animation
// ---------------------------------------------------------------------------

/// Page turn: snap scroll position instantly.
fn crossfade_to(state: &AppState, target_value: f64) {
    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();
    let clamped = target_value.max(0.0).min(adj.upper() - page_size);
    adj.set_value(clamped);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count how many buffer lines fit in the viewport starting from `page_top_line`.
/// Uses next-line y positions to measure actual occupied height including all spacing.
fn lines_per_page(state: &AppState) -> usize {
    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();
    let line_count = state.effective_line_count();
    let start = state.page_top_line;

    if line_count == 0 || start >= line_count {
        return 15; // fallback
    }

    let Some(start_iter) = state.buffer.iter_at_line(start as i32) else {
        return 15;
    };
    let start_y = state.text_view.iter_location(&start_iter).y() as f64;
    let limit_y = start_y + page_size;

    let mut count = 0;
    for i in start..line_count {
        // Determine the bottom of line i by looking at where line i+1 starts,
        // or using iter_location height for the last line in the buffer.
        let line_bottom = if i + 1 < line_count {
            if let Some(next_iter) = state.buffer.iter_at_line((i + 1) as i32) {
                state.text_view.iter_location(&next_iter).y() as f64
            } else {
                break;
            }
        } else {
            let Some(iter) = state.buffer.iter_at_line(i as i32) else {
                break;
            };
            let rect = state.text_view.iter_location(&iter);
            rect.y() as f64 + rect.height() as f64
        };

        if line_bottom > limit_y {
            break;
        }
        count += 1;
    }

    count.max(1)
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
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, target_line) {
                set_page(state, target_line);
            }
        }
    }
    seek_to_current_line(state);
}

/// Jump to a specific vocab match index. Used by concordance picker.
pub fn jump_to_vocab_at(state: &mut AppState, match_idx: usize) {
    if match_idx >= state.vocab_matches.len() {
        return;
    }
    state.vocab_match_idx = Some(match_idx);
    let target_line = state.vocab_matches[match_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, target_line) {
                set_page(state, target_line);
            }
        }
    }
    seek_to_current_line(state);
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
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => scroll_to_cursor(state),
    }
    seek_to_current_line(state);
}

// --- Cross-work concordance navigation ---

/// Jump to the current concordance occurrence.
/// Loads the work if different from current, positions cursor on the line.
pub fn concordance_jump_to_current(
    state: &Rc<RefCell<AppState>>,
    handle: &tokio::runtime::Handle,
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

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        // Need to load a different work
        let state_clone = Rc::clone(state);
        let handle_clone = handle.clone();
        let abbrev = target_abbrev.clone();

        // Check if preloaded work matches
        let preloaded = {
            let mut s = state_clone.borrow_mut();
            if let Some(conc) = &mut s.concordance_state {
                if conc
                    .preloaded_work
                    .as_ref()
                    .map(|p| p.work_abbrev == abbrev)
                    .unwrap_or(false)
                {
                    conc.preloaded_work.take()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(preloaded) = preloaded {
            // Use preloaded work
            let mut s = state_clone.borrow_mut();
            crate::app::display_work(&mut s, preloaded.work);
            concordance_position_cursor(&mut s, target_line_id);
            concordance_update_bar(&s);
            drop(s);
            concordance_preload_next(&state_clone, &handle_clone);
        } else {
            // Async load via spawn_blocking
            glib::spawn_future_local(async move {
                let work = handle_clone
                    .spawn_blocking(move || {
                        let conn =
                            crate::db::queries::open_db().expect("Failed to open lit.db");
                        crate::db::queries::load_work(&conn, &abbrev).ok()
                    })
                    .await
                    .unwrap_or(None);
                if let Some(work) = work {
                    let mut s = state_clone.borrow_mut();
                    crate::app::display_work(&mut s, work);
                    concordance_position_cursor(&mut s, target_line_id);
                    concordance_update_bar(&s);
                    drop(s);
                    concordance_preload_next(&state_clone, &handle_clone);
                }
            });
        }
    } else {
        // Same work, just move cursor
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
        drop(s);
        concordance_preload_next(state, handle);
    }
}

/// Position cursor on the line with the given line_mapping_id.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    if let Some(work) = &state.current_work {
        if let Some(idx) = work.lines.iter().position(|l| l.id == line_mapping_id) {
            state.current_line = idx;
            update_highlight(state);
            center_cursor(state);
            seek_to_current_line(state);
        }
    }
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
fn concordance_preload_next(
    state: &Rc<RefCell<AppState>>,
    handle: &tokio::runtime::Handle,
) {
    let next_abbrev = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        // Preload in forward direction
        match conc.next_work_abbrev(1) {
            Some(a) if Some(a) != s.current_work.as_ref().map(|w| w.abbrev.as_str()) => {
                a.to_string()
            }
            _ => return,
        }
    };

    let state_clone = Rc::clone(state);
    let handle_clone = handle.clone();
    let abbrev = next_abbrev;
    glib::spawn_future_local(async move {
        let work = handle_clone
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                crate::db::queries::load_work(&conn, &abbrev).ok()
            })
            .await
            .unwrap_or(None);
        if let Some(work) = work {
            let mut s = state_clone.borrow_mut();
            if let Some(conc) = &mut s.concordance_state {
                conc.preloaded_work = Some(crate::concordance::PreloadedWork {
                    work_abbrev: work.abbrev.clone(),
                    work,
                });
            }
        }
    });
}
