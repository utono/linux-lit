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

fn scroll_to_current_line(state: &AppState) {
    if let Some(mut iter) = state.buffer.iter_at_line(state.current_line as i32) {
        state
            .text_view
            .scroll_to_iter(&mut iter, 0.0, true, 0.0, 0.4);
    }
}
