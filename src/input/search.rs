use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{AppState, SearchMatch};

/// Run search against loaded work, update highlights and counter.
/// Called on every keystroke in the search entry.
pub fn execute_search(state_rc: &Rc<RefCell<AppState>>) {
    // try_borrow_mut: show() calls set_text("") which fires connect_changed
    // while state is already borrowed from the "slash" key handler.
    let Ok(mut state) = state_rc.try_borrow_mut() else {
        return;
    };
    let query = state.search_bar.query();

    clear_highlights(&state);
    state.search_matches.clear();
    state.search_match_idx = 0;

    if query.is_empty() {
        state.search_bar.update_counter(0, 0);
        return;
    }

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Smart-case: if query has uppercase, match case-sensitively;
    // otherwise match case-insensitively.
    // Always search in line.text to keep byte offsets consistent with the buffer.
    let case_sensitive = query.chars().any(|c| c.is_uppercase());

    // Collect into a local vec to avoid simultaneous immutable+mutable borrow of state.
    let mut new_matches: Vec<SearchMatch> = Vec::new();

    for (line_idx, line) in work.lines.iter().enumerate() {
        if case_sensitive {
            let mut search_start = 0;
            while let Some(pos) = line.text[search_start..].find(&*query) {
                let byte_start = search_start + pos;
                let byte_end = byte_start + query.len();
                new_matches.push(SearchMatch {
                    line_index: line_idx,
                    byte_start,
                    byte_end,
                });
                search_start = byte_end;
            }
        } else {
            // Case-insensitive: lowercase both sides, but track byte positions in original text
            let text_lower = line.text.to_lowercase();
            let query_lower = query.to_lowercase();
            let mut search_start = 0;
            while let Some(pos) = text_lower[search_start..].find(&*query_lower) {
                let byte_start = search_start + pos;
                let byte_end = byte_start + query_lower.len();
                new_matches.push(SearchMatch {
                    line_index: line_idx,
                    byte_start,
                    byte_end,
                });
                search_start = byte_end;
            }
        }
    }

    state.search_matches = new_matches;

    apply_highlights(&state);

    let total = state.search_matches.len();
    if total > 0 {
        // Jump to first match at or after current_line
        let idx = state
            .search_matches
            .iter()
            .position(|m| m.line_index >= state.current_line)
            .unwrap_or(0);
        state.search_match_idx = idx;
        apply_current_highlight(&state);
        state.search_bar.update_counter(idx, total);
    } else {
        state.search_bar.update_counter(0, 0);
    }
}

/// Jump to next match, wrapping around.
pub fn next_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    remove_current_highlight(state);
    state.search_match_idx = (state.search_match_idx + 1) % total;
    let m = &state.search_matches[state.search_match_idx];
    state.current_line = m.line_index;
    apply_current_highlight(state);
    state.search_bar.update_counter(state.search_match_idx, total);
    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Jump to previous match, wrapping around.
pub fn prev_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    remove_current_highlight(state);
    state.search_match_idx = (state.search_match_idx + total - 1) % total;
    let m = &state.search_matches[state.search_match_idx];
    state.current_line = m.line_index;
    apply_current_highlight(state);
    state.search_bar.update_counter(state.search_match_idx, total);
    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Clear all search state: highlights, matches, active flag.
pub fn clear_search(state: &mut AppState) {
    clear_highlights(state);
    state.search_matches.clear();
    state.search_match_idx = 0;
    state.search_active = false;
}

// --- internal helpers ---

fn clear_highlights(state: &AppState) {
    let (start, end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.search_tag, &start, &end);
    state
        .buffer
        .remove_tag(&state.search_current_tag, &start, &end);
}

fn apply_highlights(state: &AppState) {
    for m in &state.search_matches {
        let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
            continue;
        };
        let mut start = line_start;
        start.set_line_offset(0);
        // Convert byte offset to char offset for GTK TextIter
        let line_text = &state.current_work.as_ref().unwrap().lines[m.line_index].text;
        let char_start = line_text[..m.byte_start].chars().count() as i32;
        let char_end = line_text[..m.byte_end].chars().count() as i32;
        let mut match_start = line_start;
        match_start.forward_chars(char_start);
        let mut match_end = line_start;
        match_end.forward_chars(char_end);
        state
            .buffer
            .apply_tag(&state.search_tag, &match_start, &match_end);
    }
}

fn apply_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() {
        return;
    }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
        return;
    };
    let line_text = &state.current_work.as_ref().unwrap().lines[m.line_index].text;
    let char_start = line_text[..m.byte_start].chars().count() as i32;
    let char_end = line_text[..m.byte_end].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state
        .buffer
        .apply_tag(&state.search_current_tag, &match_start, &match_end);
}

fn remove_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() {
        return;
    }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
        return;
    };
    let line_text = &state.current_work.as_ref().unwrap().lines[m.line_index].text;
    let char_start = line_text[..m.byte_start].chars().count() as i32;
    let char_end = line_text[..m.byte_end].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state
        .buffer
        .remove_tag(&state.search_current_tag, &match_start, &match_end);
}
