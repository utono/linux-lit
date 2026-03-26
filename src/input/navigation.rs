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

/// Move cursor by `delta` lines (j/k). Cursor moves within the current page.
/// When cursor goes past a page boundary, a page turn happens.
pub fn move_cursor(state: &mut AppState, delta: i32) {
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
    if line_count == 0 {
        return;
    }

    let new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    if new_line == state.current_line {
        return;
    }

    state.current_line = new_line;
    update_highlight(state);
    seek_to_current_line(state);

    // Check if cursor is still on the current page
    let page_top = state.page_top_line;
    let page_bottom = page_top + lines_per_page(state);

    if new_line < page_top {
        let lpp = lines_per_page(state);
        let new_top = new_line.saturating_sub(lpp.saturating_sub(1));
        set_page(state, new_top);
    } else if new_line >= page_bottom {
        let new_top = new_line.saturating_sub(PAGE_OVERLAP);
        set_page(state, new_top);
    } else if !is_line_fully_visible(state, new_line) {
        // Highlight padding may push line below viewport — page turn to show it
        let new_top = new_line.saturating_sub(PAGE_OVERLAP);
        set_page(state, new_top);
    }
}

/// Jump to the first line.
pub fn jump_to_start(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    state.current_line = 0;
    update_highlight(state);
    set_page_instant(state, 0);
}

/// Jump to the last line.
pub fn jump_to_end(state: &mut AppState) {
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
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
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
    if line_count == 0 {
        return;
    }

    let lpp = lines_per_page(state);
    let advance = lpp.saturating_sub(PAGE_OVERLAP).max(1);
    let new_top = (state.page_top_line + advance).min(line_count.saturating_sub(1));

    // Move cursor to first line of new page (after overlap)
    state.current_line = (new_top + PAGE_OVERLAP).min(line_count - 1);
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

    // Move cursor to last line of new page (before overlap)
    let new_bottom = new_top + lpp.saturating_sub(1);
    let line_count = state
        .current_work
        .as_ref()
        .map_or(0, |w| w.lines.len());
    state.current_line = new_bottom.min(line_count.saturating_sub(1));
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top);
}

/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
/// Otherwise, jump to the previous dialogue line.
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if state.current_line == 0 {
            return;
        }

        // At top of page — page backward, keep cursor
        if state.current_line == state.page_top_line {
            page_backward(state);
            return;
        }

        let mut found = None;
        for i in (0..state.current_line).rev() {
            if work.lines[i].is_dialogue {
                found = Some(i);
                break;
            }
        }
        found
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        ensure_cursor_on_page(state);
        seek_to_current_line(state);
    }
}

/// Next dialogue line (`q` key).
/// If cursor is at the last visible line of the page, just page forward (don't move cursor).
/// Otherwise, jump to the next dialogue line.
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let line_count = work.lines.len();

        // At bottom of page — page forward, keep cursor
        let lpp = lines_per_page(state);
        let page_last = (state.page_top_line + lpp)
            .saturating_sub(1)
            .min(line_count.saturating_sub(1));
        if state.current_line >= page_last {
            page_forward(state);
            return;
        }

        let mut found = None;
        for i in (state.current_line + 1)..line_count {
            if work.lines[i].is_dialogue {
                found = Some(i);
                break;
            }
        }
        found
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        ensure_cursor_on_page(state);
        seek_to_current_line(state);
    }
}

/// Restore cursor position after loading a work (used on startup with MRU).
pub fn restore_cursor(state: &mut AppState) {
    update_highlight(state);
    // Place cursor's page so cursor is near the top
    let new_top = state.current_line.saturating_sub(PAGE_OVERLAP);
    set_page_instant(state, new_top);
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

    if state.current_line >= page_top && state.current_line < page_bottom {
        return;
    }

    if state.current_line < page_top {
        // Went above — new page with cursor near bottom
        let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
        set_page(state, new_top);
    } else {
        // Went below — new page with cursor near top (with overlap)
        let new_top = state.current_line.saturating_sub(PAGE_OVERLAP);
        set_page(state, new_top);
    }
}

/// Seek MPV to the current line's start time (with preroll).
/// Called on every cursor movement so audio follows the reader.
fn seek_to_current_line(state: &AppState) {
    if let Some(ref work) = state.current_work {
        if let Some(ts) = &work.lines[state.current_line].timestamp {
            let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
            let _ = state
                .cmd_tx
                .try_send(crate::mpv::MpvCommand::Seek(seek_time));
        }
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

/// Get the vadjustment value that places the given line at the top of the viewport.
fn scroll_value_for_line(state: &AppState, line: usize) -> f64 {
    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return 0.0;
    };
    let rect = state.text_view.iter_location(&iter);
    let adj = state.scrolled_window.vadjustment();
    let max = adj.upper() - adj.page_size();
    (rect.y() as f64).max(0.0).min(max)
}

// ---------------------------------------------------------------------------
// Highlight
// ---------------------------------------------------------------------------

/// Update highlight and ensure cursor is visible on the current page.
pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    ensure_cursor_on_page(state);
}

/// Remove highlight from old line, apply to new current line.
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.highlight_tag;
    let (start, end) = buffer.bounds();
    buffer.remove_tag(tag, &start, &end);

    if let Some(iter) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = iter;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.apply_tag(tag, &iter, &line_end);
        crate::logging::log(&format!(
            "HIGHLIGHT: line={} iter_offset={} end_offset={} tag_applied=true",
            state.current_line,
            iter.offset(),
            line_end.offset()
        ));
    } else {
        crate::logging::log(&format!(
            "HIGHLIGHT: line={} iter_at_line=NONE tag_applied=false",
            state.current_line
        ));
    }
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

/// Check if a line fits entirely within the viewport by looking at where
/// the next line starts. This accounts for word-wrap height and all padding.
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    let adj = state.scrolled_window.vadjustment();
    let viewport_bottom = adj.value() + adj.page_size();

    let line_count = state.current_work.as_ref().map_or(0, |w| w.lines.len());
    if line + 1 < line_count {
        // Use the top of the next line as the bottom boundary of this line
        let Some(next_iter) = state.buffer.iter_at_line((line + 1) as i32) else {
            return true;
        };
        let next_y = state.text_view.iter_location(&next_iter).y() as f64;
        next_y <= viewport_bottom
    } else {
        // Last line — use its own rect bottom
        let Some(iter) = state.buffer.iter_at_line(line as i32) else {
            return true;
        };
        let rect = state.text_view.iter_location(&iter);
        let line_bottom = rect.y() as f64 + rect.height() as f64;
        line_bottom <= viewport_bottom
    }
}

/// Count how many buffer lines fit in the viewport starting from `page_top_line`.
/// Uses next-line y positions to measure actual occupied height including all spacing.
fn lines_per_page(state: &AppState) -> usize {
    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();
    let line_count = state.current_work.as_ref().map_or(0, |w| w.lines.len());
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
