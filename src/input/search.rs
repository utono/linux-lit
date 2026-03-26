use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{AppState, SearchMatch};

/// Run search against loaded work, update highlights and counter.
/// Called on every keystroke in the search entry.
pub fn execute_search(state_rc: &Rc<RefCell<AppState>>) {
    let mut state = state_rc.borrow_mut();
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

    if state.line_map.is_some() {
        // Text file mode: search the buffer text directly
        let text = state.buffer.text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        for (line_idx, line_text) in text.as_str().lines().enumerate() {
            if case_sensitive {
                let mut search_start = 0;
                while let Some(pos) = line_text[search_start..].find(&*query) {
                    let byte_start = search_start + pos;
                    let byte_end = byte_start + query.len();
                    new_matches.push(SearchMatch { line_index: line_idx, byte_start, byte_end });
                    search_start = byte_end;
                }
            } else {
                let text_lower = line_text.to_lowercase();
                let query_lower = query.to_lowercase();
                let mut search_start = 0;
                while let Some(pos) = text_lower[search_start..].find(&*query_lower) {
                    let byte_start = search_start + pos;
                    let byte_end = byte_start + query_lower.len();
                    new_matches.push(SearchMatch { line_index: line_idx, byte_start, byte_end });
                    search_start = byte_end;
                }
            }
        }
    } else {
        // Original: search work.lines
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
        let m = &state.search_matches[idx];
        state.current_line = m.line_index;
        apply_current_highlight(&state);
        state.search_bar.update_counter(idx, total);
        crate::input::navigation::update_highlight_and_center(&mut state);
        // Pause playback when search finds results
        let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
    } else {
        state.search_bar.update_counter(0, 0);
    }
}

/// Toggle playback. If resuming, seek to current line's start_time first.
pub fn toggle_playback(state: &AppState) {
    if let Some(ref work) = state.current_work {
        if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
            if let Some(ts) = &work.lines[work_idx].timestamp {
                let seek_time = (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0);
                // Seek first, then toggle — if paused, this means seek+resume;
                // if playing, the seek is harmless and toggle pauses.
                let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
            }
        }
    }
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
}

/// Jump to next match, wrapping around. Starts playback at match line.
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
    crate::input::navigation::update_highlight_and_center(state);
    seek_and_resume(state);
}

/// Jump to previous match, wrapping around. Starts playback at match line.
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
    crate::input::navigation::update_highlight_and_center(state);
    seek_and_resume(state);
}

/// Seek to current line's start_time and resume playback.
fn seek_and_resume(state: &AppState) {
    if let Some(ref work) = state.current_work {
        if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
            if let Some(ts) = &work.lines[work_idx].timestamp {
                let seek_time = (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0);
                let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(seek_time));
            }
        }
    }
}

/// Clear all search state: highlights and matches.
pub fn clear_search(state: &mut AppState) {
    clear_highlights(state);
    state.search_matches.clear();
    state.search_match_idx = 0;
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
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let line_text = state.buffer.text(&line_start, &line_end, false);
        let char_start = line_text[..m.byte_start.min(line_text.len())].chars().count() as i32;
        let char_end = line_text[..m.byte_end.min(line_text.len())].chars().count() as i32;
        let mut match_start = line_start;
        match_start.forward_chars(char_start);
        let mut match_end = line_start;
        match_end.forward_chars(char_end);
        state.buffer.apply_tag(&state.search_tag, &match_start, &match_end);
    }
}

fn apply_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() { return; }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else { return; };
    let mut line_end = line_start;
    if !line_end.ends_line() { line_end.forward_to_line_end(); }
    let line_text = state.buffer.text(&line_start, &line_end, false);
    let char_start = line_text[..m.byte_start.min(line_text.len())].chars().count() as i32;
    let char_end = line_text[..m.byte_end.min(line_text.len())].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state.buffer.apply_tag(&state.search_current_tag, &match_start, &match_end);
}

fn remove_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() { return; }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else { return; };
    let mut line_end = line_start;
    if !line_end.ends_line() { line_end.forward_to_line_end(); }
    let line_text = state.buffer.text(&line_start, &line_end, false);
    let char_start = line_text[..m.byte_start.min(line_text.len())].chars().count() as i32;
    let char_end = line_text[..m.byte_end.min(line_text.len())].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state.buffer.remove_tag(&state.search_current_tag, &match_start, &match_end);
}
