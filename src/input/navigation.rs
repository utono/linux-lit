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
/// Restore cursor position after loading a work (used on startup with MRU).
/// Updates highlight and scrolls to the saved line.
pub fn restore_cursor(state: &mut AppState) {
    update_highlight(state);
    scroll_to_line_instant(state, state.current_line);
}

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
///
/// Two behaviors based on distance:
/// - **Small move** (cursor just barely off-screen): nudge scroll to reveal it.
///   This handles j/k single-line movement — feels like natural reading.
/// - **Large move** (cursor far off-screen): crossfade page turn.
///   This handles dialogue jumps, Ctrl+d/u — feels like turning a page.
fn ensure_cursor_visible(state: &AppState) {
    let Some(iter) = state.buffer.iter_at_line(state.current_line as i32) else {
        return;
    };

    let adj = state.scrolled_window.vadjustment();
    let page_top = adj.value();
    let page_size = adj.page_size();
    let page_bottom = page_top + page_size;

    // Get the full height of the cursor's paragraph (may span multiple visual lines).
    let iter_rect = state.text_view.iter_location(&iter);
    let line_y = iter_rect.y() as f64;
    let full_line_h =
        if let Some(next_iter) = state.buffer.iter_at_line(state.current_line as i32 + 1) {
            let next_rect = state.text_view.iter_location(&next_iter);
            (next_rect.y() as f64 - line_y).max(iter_rect.height() as f64)
        } else {
            iter_rect.height() as f64
        };

    // Already visible — do nothing (page stays still)
    if line_y >= page_top && line_y + full_line_h <= page_bottom {
        return;
    }

    // Cursor went off screen — always do a page turn (e-reader style).
    // No nudging — the page is fixed until a turn happens.
    if line_y < page_top {
        // Went above: position so old page_top appears at bottom of new page.
        // This gives reading continuity — you can see where you were.
        // new_page_top = old_page_top - page_size
        // But ensure the cursor is also visible (it must be above old_page_top).
        let anchored = (page_top - page_size).max(0.0);
        // If cursor would be above the anchored view, adjust to show cursor
        let target = if line_y < anchored {
            (line_y - full_line_h).max(0.0)
        } else {
            anchored
        };
        page_turn_to_value(state, target);
    } else {
        // Went below: position so old page_bottom appears at top of new page.
        let anchored = page_bottom;
        // If cursor would be below the anchored view, adjust to show cursor
        let target = if line_y + full_line_h > anchored + page_size {
            (line_y - full_line_h * 2.0).max(0.0)
        } else {
            anchored
        };
        page_turn_to_value(state, target);
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

/// Animate a page turn as a crossfade: fade out, snap scroll, fade in.
/// Feels like turning a page on a Kindle — quiet and non-distracting.
fn page_turn_to_value(state: &AppState, target_value: f64) {
    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();
    let clamped_target = target_value.max(0.0).min(adj.upper() - page_size);

    if (clamped_target - adj.value()).abs() < 1.0 {
        return;
    }

    let widget = state.scrolled_window.clone();
    let adj_clone = adj.clone();

    // Phase 1: Fade out (0 → 80ms)
    let fade_out_frames: u64 = 5;
    let frame_ms: u64 = 16;

    for frame in 1..=fade_out_frames {
        let progress = frame as f64 / fade_out_frames as f64;
        // Ease-out: fast start, slow end
        let opacity = 1.0 - progress * progress;
        let delay = std::time::Duration::from_millis(frame * frame_ms);
        let w = widget.clone();
        glib::timeout_add_local_once(delay, move || {
            w.set_opacity(opacity);
        });
    }

    // Phase 2: Snap scroll position at the midpoint (80ms)
    let snap_delay = std::time::Duration::from_millis((fade_out_frames + 1) * frame_ms);
    let w = widget.clone();
    glib::timeout_add_local_once(snap_delay, move || {
        adj_clone.set_value(clamped_target);
        w.set_opacity(0.0);
    });

    // Phase 3: Fade in (96ms → 176ms)
    let fade_in_frames: u64 = 5;
    let fade_in_start = fade_out_frames + 2;

    for frame in 1..=fade_in_frames {
        let progress = frame as f64 / fade_in_frames as f64;
        // Ease-in: slow start, fast end
        let opacity = progress * progress;
        let delay = std::time::Duration::from_millis((fade_in_start + frame) * frame_ms);
        let w = widget.clone();
        glib::timeout_add_local_once(delay, move || {
            w.set_opacity(opacity);
        });
    }

    // Ensure opacity is fully restored
    let final_delay = std::time::Duration::from_millis((fade_in_start + fade_in_frames + 1) * frame_ms);
    let w = widget.clone();
    glib::timeout_add_local_once(final_delay, move || {
        w.set_opacity(1.0);
    });
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
