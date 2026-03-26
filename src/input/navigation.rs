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

    let new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    if new_line == state.current_line {
        return;
    }

    state.current_line = new_line;
    update_highlight(state);

    if delta > 0 && needs_page_turn_down(state, new_line) {
        // Going down and line is at bottom edge — page turn with this line at top
        set_page(state, new_line);
    } else if delta < 0 {
        scroll_to_cursor(state);
    }

    seek_to_current_line(state);
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
    let line_count = state.effective_line_count();
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

        if let Some(ref lm) = state.line_map {
            lm.dialogue_buffer_lines
                .iter()
                .rev()
                .find(|&&bl| bl < state.current_line)
                .copied()
        } else {
            let mut found = None;
            for i in (0..state.current_line).rev() {
                if work.lines[i].is_dialogue {
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
        scroll_to_cursor(state);
        seek_to_current_line(state);
    }
}

/// Next dialogue line (`q` key).
/// Jump to next dialogue line. Page turn when target is not fully visible.
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        if let Some(ref lm) = state.line_map {
            lm.dialogue_buffer_lines
                .iter()
                .find(|&&bl| bl > state.current_line)
                .copied()
        } else {
            let line_count = work.lines.len();
            let mut found = None;
            for i in (state.current_line + 1)..line_count {
                if work.lines[i].is_dialogue {
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
        if needs_page_turn_down(state, line_idx) {
            set_page(state, line_idx);
        }
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

/// Seek MPV to the current line's start time (with preroll).
/// Called on every cursor movement so audio follows the reader.
fn seek_to_current_line(state: &AppState) {
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
pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    if needs_page_turn_down(state, state.current_line) {
        set_page(state, state.current_line);
    } else {
        ensure_cursor_on_page(state);
    }
}

/// Update highlight and center the current line on screen.
pub fn update_highlight_and_center(state: &mut AppState) {
    update_highlight(state);
    let lpp = lines_per_page(state);
    let new_top = state.current_line.saturating_sub(lpp / 2);
    set_page_instant(state, new_top);
}

/// Dim all lines except the current one. The current line keeps full foreground.
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let (buf_start, buf_end) = buffer.bounds();

    // Apply dim to entire buffer
    buffer.apply_tag(tag, &buf_start, &buf_end);

    // Remove dim from current line to restore full brightness
    if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.remove_tag(tag, &line_start, &line_end);
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
