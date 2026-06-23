use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::AnimationExt;

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

    // Remember the pattern as MRU so n/N can reactivate search after Escape.
    state.last_search_query = Some(query.to_string());

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
            collect_line(line_text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    } else {
        // Original: search work.lines
        for (line_idx, line) in work.lines.iter().enumerate() {
            collect_line(&line.text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    }

    state.search_matches = new_matches;

    apply_highlights(&state);

    let total = state.search_matches.len();
    if total > 0 {
        // Direction set by which key opened the bar: `/` forward (first match at
        // or after the cursor), `?` backward (last match at or before it). Land
        // on the match's CANONICAL spread (same as n/N), not top-aligned.
        let cur = state.current_line;
        let idx = if state.search_backward {
            state
                .search_matches
                .iter()
                .rposition(|m| m.line_index <= cur)
                .unwrap_or(total - 1)
        } else {
            state
                .search_matches
                .iter()
                .position(|m| m.line_index >= cur)
                .unwrap_or(0)
        };
        land_on_match_idx(&mut state, idx);
        // Submit behaves like n/N: always seek MPV to the found line's start
        // time; keep playing if it was already playing, stay paused otherwise.
        // (Previously this force-paused WITHOUT seeking, so a later resume
        // played from a stale position, not the match.)
        seek_and_resume(&mut state);
    } else {
        state.search_bar.update_counter(0, 0);
    }
}

/// Toggle playback. If resuming, seek to current line's start_time first.
/// Clears sync suppression so cursor tracking resumes.
pub fn toggle_playback(state: &mut AppState) {
    // Hide translations when starting playback
    crate::app::translations::hide_translations_for_navigation(state);
    // Suppress cursor fade on the first sync-driven highlight after unpause
    if let Some(prev) = state.cursor_fade_anim.take() {
        prev.skip();
    }
    state.prev_highlight_line.set(None);
    if let Some(ref work) = state.current_work {
        if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
            if let Some(ts) = &work.lines[work_idx].timestamp {
                let seek_time = (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0);
                let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
                // Suppress sync briefly so the preroll seek doesn't pull
                // the cursor back to the previous line
                state.suppress_sync_until =
                    Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
            } else {
                // Current line has no timestamp — clear any indefinite suppression
                // (e.g. from navigate-while-paused) so sync resumes on playback.
                state.suppress_sync_until = None;
            }
        }
    }
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
}

/// Jump to next match. Does NOT wrap: at the last match, show the right edge
/// toast and stay put.
pub fn next_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    if state.search_match_idx + 1 >= total {
        let q = state.last_search_query.clone().unwrap_or_default();
        edge_toast(state, Side::Right, &q);
        return;
    }
    goto_match_idx(state, state.search_match_idx + 1);
}

/// Jump to previous match. Does NOT wrap: at the first match, show the left
/// edge toast and stay put.
pub fn prev_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    if state.search_match_idx == 0 {
        let q = state.last_search_query.clone().unwrap_or_default();
        edge_toast(state, Side::Left, &q);
        return;
    }
    goto_match_idx(state, state.search_match_idx - 1);
}

/// Entry point for n / N pressed in reader mode (concordance already handled by
/// the caller). `pressed_next` is true for `n`, false for `N`. If matches are
/// live, step within them; otherwise reactivate the MRU search. In backward
/// (`?`) search mode the keys are REVERSED so `n` repeats in the search
/// direction (earlier) and `N` reverses it (later) — vim semantics.
pub fn reactivate_and_step(state_rc: &Rc<RefCell<AppState>>, pressed_next: bool) {
    // n repeats in the search direction: forward search -> n=next; backward (?)
    // search -> n=prev. XOR the pressed key with the search direction.
    let forward = pressed_next != state_rc.borrow().search_backward;

    // Live matches: just step (handles its own end-of-list edge toasts).
    if !state_rc.borrow().search_matches.is_empty() {
        let mut state = state_rc.borrow_mut();
        if forward {
            next_match(&mut state);
        } else {
            prev_match(&mut state);
        }
        return;
    }

    // No live matches — try to reactivate from MRU.
    let mru = state_rc.borrow().last_search_query.clone();
    let mru = match mru {
        Some(p) if !p.is_empty() => p,
        _ => return, // nothing to reactivate
    };

    // Pre-fill the entry so execute_search's query() reads the MRU pattern,
    // then collect matches + apply full highlights WITHOUT execute_search's
    // auto-navigate (we seed the index from the cursor ourselves below).
    state_rc.borrow().search_bar.set_text(&mru);
    collect_matches(state_rc);

    let mut state = state_rc.borrow_mut();
    let total = state.search_matches.len();
    if total == 0 {
        // Pattern has no matches in this work; empty counter is the feedback.
        state.search_bar.update_counter(0, 0);
        return;
    }

    let cur = state.current_line;
    if forward {
        match state
            .search_matches
            .iter()
            .position(|m| m.line_index >= cur)
        {
            Some(idx) => goto_match_idx(&mut state, idx),
            None => {
                let q = state.last_search_query.clone().unwrap_or_default();
                edge_toast(&state, Side::Right, &q);
            }
        }
    } else {
        match state
            .search_matches
            .iter()
            .rposition(|m| m.line_index <= cur)
        {
            Some(idx) => goto_match_idx(&mut state, idx),
            None => {
                let q = state.last_search_query.clone().unwrap_or_default();
                edge_toast(&state, Side::Left, &q);
            }
        }
    }
}

/// Always seek to the current line's start time (with the usual preroll). Keep
/// playing if playback was already playing (ResumeAndSeek); if it was paused,
/// seek the audio position but stay paused (Seek) — never begin playback.
fn seek_and_resume(state: &mut AppState) {
    let start = match state.current_work.as_ref().and_then(|work| {
        state
            .work_line_for_buffer(state.current_line)
            .and_then(|idx| work.lines[idx].timestamp.as_ref().map(|ts| ts.start))
    }) {
        Some(start) => start,
        None => return,
    };
    let seek_time = (start - crate::input::navigation::SEEK_PREROLL).max(0.0);

    let cmd = if state.mpv_playing {
        crate::mpv::MpvCommand::ResumeAndSeek(seek_time)
    } else {
        crate::mpv::MpvCommand::Seek(seek_time)
    };
    let _ = state.cmd_tx.try_send(cmd);

    // Suppress cursor-sync briefly so the seek isn't immediately yanked back to
    // the previous line by the next TimePos event (the second-seek + re-pause
    // seen in the log). Also drop any in-flight fade and the prev-highlight
    // bookkeeping so the highlight settles on the match line. Mirrors
    // toggle_playback's post-seek handling.
    if let Some(prev) = state.cursor_fade_anim.take() {
        prev.skip();
    }
    state.prev_highlight_line.set(None);
    state.suppress_sync_until =
        Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
}

/// Clear all search state: highlights and matches.
pub fn clear_search(state: &mut AppState) {
    clear_highlights(state);
    state.search_matches.clear();
    state.search_match_idx = 0;
}

// --- internal helpers ---

fn push_page_back_dedup(state: &mut AppState) {
    let top = state.page_top_line;
    if state.page_back_stack.last() != Some(&top) {
        state.page_back_stack.push(top);
    }
}

/// Collect matches for the current search_bar query into state.search_matches
/// and apply the dim "all matches" highlight. Does NOT navigate, set the
/// current-match highlight, or touch MPV. Used by reactivate_and_step.
fn collect_matches(state_rc: &Rc<RefCell<AppState>>) {
    let mut state = state_rc.borrow_mut();
    let query = state.search_bar.query();

    clear_highlights(&state);
    state.search_matches.clear();
    state.search_match_idx = 0;

    if query.is_empty() {
        state.search_bar.update_counter(0, 0);
        return;
    }
    state.last_search_query = Some(query.to_string());

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let case_sensitive = query.chars().any(|c| c.is_uppercase());
    let mut new_matches: Vec<SearchMatch> = Vec::new();

    if state.line_map.is_some() {
        let text = state
            .buffer
            .text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        for (line_idx, line_text) in text.as_str().lines().enumerate() {
            collect_line(line_text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    } else {
        for (line_idx, line) in work.lines.iter().enumerate() {
            collect_line(&line.text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    }

    state.search_matches = new_matches;
    apply_highlights(&state);
    state
        .search_bar
        .update_counter(0, state.search_matches.len());
}

/// Push every occurrence of `query` in `line_text` onto `out`, smart-cased.
fn collect_line(
    line_text: &str,
    query: &str,
    case_sensitive: bool,
    line_idx: usize,
    out: &mut Vec<SearchMatch>,
) {
    if case_sensitive {
        let mut search_start = 0;
        while let Some(pos) = line_text[search_start..].find(query) {
            let byte_start = search_start + pos;
            let byte_end = byte_start + query.len();
            out.push(SearchMatch { line_index: line_idx, byte_start, byte_end });
            search_start = byte_end;
        }
    } else {
        let text_lower = line_text.to_lowercase();
        let query_lower = query.to_lowercase();
        let mut search_start = 0;
        while let Some(pos) = text_lower[search_start..].find(&query_lower) {
            let byte_start = search_start + pos;
            let byte_end = byte_start + query_lower.len();
            out.push(SearchMatch { line_index: line_idx, byte_start, byte_end });
            search_start = byte_end;
        }
    }
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// Truncate an over-long pattern for display so the centered toast text stays
/// on one line within the card (keeps the first ~24 chars + an ellipsis).
fn display_pattern(query: &str) -> String {
    const MAX: usize = 24;
    if query.chars().count() <= MAX {
        query.to_string()
    } else {
        let head: String = query.chars().take(MAX).collect();
        format!("{}\u{2026}", head)
    }
}

/// Show the centered search-edge toast for 3s. `Side::Left` reports no earlier
/// occurrence, `Side::Right` no later occurrence. Reuses chapter_toast's
/// centered-bottom placement so it stays fully visible below the card.
fn edge_toast(state: &AppState, side: Side, query: &str) {
    let p = display_pattern(query);
    let text = match side {
        Side::Left => format!("No earlier occurrence of \u{201c}{}\u{201d}", p),
        Side::Right => format!("No later occurrence of \u{201c}{}\u{201d}", p),
    };
    crate::ui::toast::show_transient(&state.search_toast, &text, 3);
}

/// Select match `new_idx`, highlight it, and land on its CANONICAL spread —
/// the same page paging through the work shows — even when the match is already
/// visible on the current page. MPV playback sync drifts `page_top`, so the
/// "current spread" is often a non-canonical view of the same line; landing on
/// the canonical top is what the reader expects. Does NOT touch MPV; callers
/// decide whether to seek/resume (n/N) or pause (live search).
fn land_on_match_idx(state: &mut AppState, new_idx: usize) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    remove_current_highlight(state);
    state.search_match_idx = new_idx.min(total - 1);
    let line = state.search_matches[state.search_match_idx].line_index;
    state.current_line = line;
    apply_current_highlight(state);
    state
        .search_bar
        .update_counter(state.search_match_idx, total);
    push_page_back_dedup(state);

    // Land on the CANONICAL spread for the match. Pagination is driven by
    // column_count(), NOT navigation_mode: a two-column work paginates into
    // e-reader spreads even when navigation_mode == Scroll (page_forward uses
    // next_page_top regardless of mode), so the canonical landing must too.
    // Only a true single-column SCROLL view free-scrolls the cursor to center.
    let paginated = state.column_count() == 2
        || matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader);
    if paginated {
        let top = crate::input::navigation::canonical_page_top_for(state, line);
        crate::input::scroll::set_page_instant(state, top);
        // canonical_page_top_for backs up to the section/chapter header, so on a
        // degenerate lone-line header spread (Err/Tro at a short viewport) the
        // matched line can land off-page below it. Advance until the cursor is
        // actually visible — same guard the chapter jump uses.
        crate::input::navigation::ensure_cursor_visible_ereader(state, line);
    } else {
        crate::input::scroll::center_cursor(state);
    }
}

/// Navigate n/N: land on the match's canonical spread, then seek + resume MPV.
fn goto_match_idx(state: &mut AppState, new_idx: usize) {
    land_on_match_idx(state, new_idx);
    seek_and_resume(state);
}

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
