use gtk4::prelude::*;

use crate::app::AppState;

/// Move cursor by `delta` lines. Only triggers a page turn if cursor
/// moves beyond the visible area.
pub fn move_cursor(state: &mut AppState, delta: i32) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let line_count = work.lines.len();
    if line_count == 0 {
        return;
    }

    let new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    if new_line != state.current_line {
        state.current_line = new_line;
        update_highlight(state);
        ensure_cursor_visible(state);
    }
}

/// Jump to the first line.
pub fn jump_to_start(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    state.current_line = 0;
    update_highlight(state);
    scroll_to_line_instant(state, 0);
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
    scroll_to_line_instant(state, state.current_line);
}

/// Page forward — like turning to the next page of an e-reader.
/// Moves viewport by one page with 2 lines of overlap for reading continuity.
/// Cursor moves to the first line of the new page.
pub fn page_forward(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let line_count = work.lines.len();
    if line_count == 0 {
        return;
    }

    let visible_lines = visible_line_count(state);
    let overlap = 2;
    let advance = (visible_lines as i32 - overlap).max(1);

    let new_line = ((state.current_line as i32 + advance) as usize).min(line_count - 1);
    state.current_line = new_line;
    update_highlight(state);
    page_turn_animate(state, 1);
}

/// Page backward — like turning to the previous page.
pub fn page_backward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let visible_lines = visible_line_count(state);
    let overlap = 2;
    let retreat = (visible_lines as i32 - overlap).max(1);

    let new_line = (state.current_line as i32 - retreat).max(0) as usize;
    state.current_line = new_line;
    update_highlight(state);
    page_turn_animate(state, -1);
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
            ensure_cursor_visible(state);
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
            ensure_cursor_visible(state);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
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

/// Ensure the cursor line is visible. If it's already on screen, do nothing.
/// If it's off screen, do a smooth page turn to bring it into view.
fn ensure_cursor_visible(state: &AppState) {
    let Some(iter) = state.buffer.iter_at_line(state.current_line as i32) else {
        return;
    };

    let adj = state.scrolled_window.vadjustment();
    let page_top = adj.value();
    let page_size = adj.page_size();
    let page_bottom = page_top + page_size;

    let iter_rect = state.text_view.iter_location(&iter);
    let line_y = iter_rect.y() as f64;
    let line_h = iter_rect.height() as f64;

    // Comfortable margin: keep cursor at least 1 line height from edges
    let margin = line_h;

    if line_y >= page_top + margin && line_y + line_h <= page_bottom - margin {
        // Already visible with margin — do nothing
        return;
    }

    // Cursor went off screen — do a page turn
    if line_y < page_top + margin {
        // Went above — page turn backward: place cursor near bottom of new page
        let target = (line_y - page_size + line_h + margin).max(0.0);
        page_turn_to_value(state, target);
    } else {
        // Went below — page turn forward: place cursor near top of new page
        page_turn_to_value(state, (line_y - margin).max(0.0));
    }
}

/// Instant scroll — no animation. Used for gg/G jumps.
fn scroll_to_line_instant(state: &AppState, line: usize) {
    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return;
    };

    let iter_rect = state.text_view.iter_location(&iter);
    let target_y = iter_rect.y() as f64;

    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();
    let margin = page_size * 0.1;

    let target_value = (target_y - margin).max(0.0).min(adj.upper() - page_size);
    adj.set_value(target_value);
}

/// Animate a smooth page turn to a target scroll value.
/// Uses ease-in-out for a clean, non-distracting slide.
fn page_turn_to_value(state: &AppState, target_value: f64) {
    let adj = state.scrolled_window.vadjustment();
    let start_value = adj.value();
    let page_size = adj.page_size();
    let clamped_target = target_value.max(0.0).min(adj.upper() - page_size);
    let distance = clamped_target - start_value;

    if distance.abs() < 1.0 {
        return;
    }

    // 200ms animation, ~60fps
    let total_frames: u64 = 12;
    let frame_ms: u64 = 16;

    for frame in 1..=total_frames {
        let progress = frame as f64 / total_frames as f64;
        // Ease-in-out cubic: smooth acceleration and deceleration
        let eased = if progress < 0.5 {
            4.0 * progress * progress * progress
        } else {
            1.0 - (-2.0 * progress + 2.0_f64).powi(3) / 2.0
        };
        let value = start_value + distance * eased;
        let delay = std::time::Duration::from_millis(frame * frame_ms);
        let adj_for_frame = adj.clone();
        glib::timeout_add_local_once(delay, move || {
            adj_for_frame.set_value(value);
        });
    }
}

/// Animate a page turn in the given direction (+1 forward, -1 backward).
/// Places the cursor line near the top of the new page.
fn page_turn_animate(state: &AppState, _direction: i32) {
    let Some(iter) = state.buffer.iter_at_line(state.current_line as i32) else {
        return;
    };

    let iter_rect = state.text_view.iter_location(&iter);
    let line_y = iter_rect.y() as f64;
    let line_h = iter_rect.height() as f64;

    // Place the cursor line near the top with a small margin
    let target_value = (line_y - line_h).max(0.0);
    page_turn_to_value(state, target_value);
}

/// Estimate the number of visible lines on screen.
fn visible_line_count(state: &AppState) -> usize {
    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();

    // Get line height from a sample line
    if let Some(iter) = state.buffer.iter_at_line(0) {
        let rect = state.text_view.iter_location(&iter);
        let line_h = rect.height() as f64;
        if line_h > 0.0 {
            return (page_size / line_h).floor() as usize;
        }
    }

    // Fallback
    20
}
