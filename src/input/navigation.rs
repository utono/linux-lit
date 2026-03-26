use gtk4::prelude::*;

use crate::app::AppState;

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
        scroll_to_current_line(state);
    }
}

pub fn jump_to_start(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    state.current_line = 0;
    update_highlight(state);
    scroll_to_current_line(state);
}

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
    scroll_to_current_line(state);
}

pub fn scroll_half_page(state: &mut AppState, direction: i32) {
    move_cursor(state, direction * 15);
}

pub fn scroll_full_page(state: &mut AppState, direction: i32) {
    move_cursor(state, direction * 30);
}

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
            scroll_to_current_line(state);
            return;
        }
    }
}

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
            scroll_to_current_line(state);
            return;
        }
    }
}

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

/// Smoothly scroll the text view to keep the current line at ~40% from top.
fn scroll_to_current_line(state: &AppState) {
    let Some(mut iter) = state.buffer.iter_at_line(state.current_line as i32) else {
        return;
    };

    // Get the y coordinate of the target line in buffer coordinates
    let iter_rect = state.text_view.iter_location(&iter);
    let target_y = iter_rect.y() as f64;

    let adj = state.scrolled_window.vadjustment();
    let page_size = adj.page_size();

    // Target: place the line at 40% from the top of the visible area
    let target_value = (target_y - page_size * 0.4).max(0.0).min(adj.upper() - page_size);
    let start_value = adj.value();
    let distance = target_value - start_value;

    // For small movements (single line), skip animation to feel snappy
    if distance.abs() < page_size * 0.15 {
        // Still ensure the line is visible — use scroll_to_iter as fallback
        state
            .text_view
            .scroll_to_iter(&mut iter, 0.1, false, 0.0, 0.0);
        return;
    }

    // Animate over ~150ms at ~60fps (frames every ~16ms)
    let total_frames = 9;
    let frame_ms = 16;
    let adj_clone = adj.clone();

    for frame in 1..=total_frames {
        let progress = frame as f64 / total_frames as f64;
        // Ease-out cubic: decelerates toward the end
        let eased = 1.0 - (1.0 - progress).powi(3);
        let value = start_value + distance * eased;
        let delay = std::time::Duration::from_millis(frame * frame_ms);
        let adj_for_frame = adj_clone.clone();
        glib::timeout_add_local_once(delay, move || {
            adj_for_frame.set_value(value);
        });
    }
}
