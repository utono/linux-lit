use gtk4::prelude::*;

use crate::app::AppState;

// Page overlap: when turning pages, this many lines from the old page
// remain visible on the new page for reading continuity.
const PAGE_OVERLAP: usize = 1;

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

    // Check if cursor is still on the current page
    let page_top = state.page_top_line;
    let page_bottom = page_top + lines_per_page(state);

    if new_line < page_top {
        // Went above current page — page turn backward.
        // New page has cursor at the bottom.
        let lpp = lines_per_page(state);
        let new_top = new_line.saturating_sub(lpp.saturating_sub(1));
        set_page(state, new_top);
    } else if new_line >= page_bottom {
        // Went below current page — page turn forward.
        // New page has cursor at the top, with overlap from old page.
        let new_top = new_line.saturating_sub(PAGE_OVERLAP);
        set_page(state, new_top);
    }
    // Otherwise cursor is within the current page — no scroll needed.
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
    set_page(state, new_top);
}

/// Jump to the previous dialogue line (`,` key).
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    if state.current_line == 0 {
        return;
    }

    for i in (0..state.current_line).rev() {
        if work.lines[i].is_dialogue {
            state.current_line = i;
            update_highlight(state);
            ensure_cursor_on_page(state);
            return;
        }
    }
}

/// Jump to the next dialogue line (`q` key).
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let line_count = work.lines.len();
    for i in (state.current_line + 1)..line_count {
        if work.lines[i].is_dialogue {
            state.current_line = i;
            update_highlight(state);
            ensure_cursor_on_page(state);
            return;
        }
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

    if state.current_line >= page_top && state.current_line < page_bottom {
        // Already on page — nothing to do
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

/// Estimate lines per page from viewport height and average line height.
fn lines_per_page(state: &AppState) -> usize {
    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();

    // Sample a few lines to get average height
    let mut total_h = 0.0;
    let mut count = 0;
    let start = state.page_top_line;
    for i in start..(start + 5).min(state.current_work.as_ref().map_or(0, |w| w.lines.len())) {
        if let Some(iter) = state.buffer.iter_at_line(i as i32) {
            let rect = state.text_view.iter_location(&iter);
            if rect.height() > 0 {
                total_h += rect.height() as f64;
                count += 1;
            }
        }
    }

    if count > 0 && total_h > 0.0 {
        let avg_h = total_h / count as f64;
        (page_size / avg_h).floor() as usize
    } else {
        15 // fallback
    }
}
