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
    execute_search_with_query(&mut state, &query);
}

/// Core of `execute_search`, callable with a direct `&mut AppState`. The nav
/// fuzz drives the REAL `/` search path through this (collect → highlight →
/// land on the canonical spread → seek) instead of simulating the jump; the
/// entry-driven path above just reads the search bar's text and delegates.
pub(crate) fn execute_search_with_query(state: &mut AppState, query: &str) {
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

    // Query is a regex, smart-cased; invalid patterns degrade to literal.
    // Always search in line.text to keep byte offsets consistent with the buffer.
    let re = build_matcher(query);

    // Collect into a local vec to avoid simultaneous immutable+mutable borrow of state.
    let mut new_matches: Vec<SearchMatch> = Vec::new();

    if state.line_map.is_some() {
        // Text file mode: search the buffer text directly
        let text = state.buffer.text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        for (line_idx, line_text) in text.as_str().lines().enumerate() {
            collect_line(line_text, &re, line_idx, &mut new_matches);
        }
    } else {
        // Original: search work.lines
        for (line_idx, line) in work.lines.iter().enumerate() {
            collect_line(&line.text, &re, line_idx, &mut new_matches);
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
        land_on_match_idx(state, idx);
        // Submit behaves like n/N: always seek MPV to the found line's start
        // time; keep playing if it was already playing, stay paused otherwise.
        // (Previously this force-paused WITHOUT seeking, so a later resume
        // played from a stale position, not the match.)
        seek_and_resume(state);
    } else {
        state.search_bar.update_counter(0, 0);
        crate::logging::log(&format!("SEARCH: no matches for '{query}'"));
    }
}

/// Centered toast for a SUBMITTED pattern with no matches anywhere in the
/// work (the Return-with-[0/0] case; `edge_toast` covers the directional
/// "no earlier/later occurrence" variants).
pub(crate) fn no_match_toast(state: &AppState) {
    let q = state.search_bar.query();
    let text = format!("No match for \u{201c}{}\u{201d}", display_pattern(&q));
    crate::ui::toast::show_transient(&state.search_toast, &text, 3);
}

/// Toggle playback, seeking to the current line's start_time first when
/// resuming. Clears sync suppression so cursor tracking resumes. For a pure
/// pause/resume with no seek, send `MpvCommand::TogglePause` directly
/// (the `TogglePause` action).
pub fn toggle_playback_from_timestamp(state: &mut AppState) {
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
                let seek_time = crate::input::navigation::preroll_seek_time(ts.start);
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
    let seek_time = crate::input::navigation::preroll_seek_time(start);

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
    // toggle_playback_from_timestamp's post-seek handling.
    if let Some(prev) = state.cursor_fade_anim.take() {
        prev.skip();
    }
    state.prev_highlight_line.set(None);
    state.suppress_sync_until =
        Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
    // Karaoke: keep the narration tint present through the search jump — move
    // it to the phrase that will play at the match line's start and hold it
    // through the suppression window (mirrors seek_to_current_line's nav-cue
    // paint). Without this, `/`-Return and n/N landings left the tint behind
    // on the pre-search line (or cleared) until live TimePos ticks repainted
    // it, so the karaoke marking vanished mid-search.
    if crate::input::phrase_highlight::paint_pending_phrase(state, start) {
        state.phrase_paint_hold = state.suppress_sync_until;
    }
}

/// Clear all search state: highlights and matches.
pub fn clear_search(state: &mut AppState) {
    clear_highlights(state);
    state.search_matches.clear();
    state.search_match_idx = 0;
}

// --- internal helpers ---

fn push_page_back_dedup(state: &mut AppState) {
    let entry = (state.page_top_line, state.page_top_offset);
    if state.page_back_stack.last() != Some(&entry) {
        state.page_back_stack.push(entry);
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
    let re = build_matcher(&query);
    let mut new_matches: Vec<SearchMatch> = Vec::new();

    if state.line_map.is_some() {
        let text = state
            .buffer
            .text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        for (line_idx, line_text) in text.as_str().lines().enumerate() {
            collect_line(line_text, &re, line_idx, &mut new_matches);
        }
    } else {
        for (line_idx, line) in work.lines.iter().enumerate() {
            collect_line(&line.text, &re, line_idx, &mut new_matches);
        }
    }

    state.search_matches = new_matches;
    apply_highlights(&state);
    state
        .search_bar
        .update_counter(0, state.search_matches.len());
}

/// Smart-case probe: true if the query contains an uppercase letter that is
/// not part of a `\X` escape (so `\W`, `\S` etc. stay case-insensitive).
fn has_unescaped_uppercase(query: &str) -> bool {
    let mut escaped = false;
    for c in query.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c.is_uppercase() {
            return true;
        }
    }
    false
}

/// Compile the search query as a regex, smart-cased. An invalid pattern
/// (half-typed `jack(`, unclosed `cade[`) silently falls back to an escaped
/// literal with identical substring semantics — incremental search must
/// never error mid-keystroke.
pub(crate) fn build_matcher(query: &str) -> regex::Regex {
    let insensitive = !has_unescaped_uppercase(query);
    regex::RegexBuilder::new(query)
        .case_insensitive(insensitive)
        .build()
        .unwrap_or_else(|_| {
            regex::RegexBuilder::new(&regex::escape(query))
                .case_insensitive(insensitive)
                .build()
                .expect("escaped literal always compiles")
        })
}

/// Push every non-empty regex match of `re` in `line_text` onto `out`.
/// Byte offsets index the original line text (they drive buffer highlights).
/// Zero-width matches (e.g. `a*` mid-typing) are skipped.
fn collect_line(
    line_text: &str,
    re: &regex::Regex,
    line_idx: usize,
    out: &mut Vec<SearchMatch>,
) {
    for m in re.find_iter(line_text) {
        if m.start() == m.end() {
            continue;
        }
        out.push(SearchMatch { line_index: line_idx, byte_start: m.start(), byte_end: m.end() });
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
    // This landing bypasses after_page_change's centralized clear (it calls
    // set_page_instant directly below, not a PageChangeReason path), so clear
    // a stale scheduled prose cross here or a later forward seek can fire it
    // and teleport the page back to the old target.
    state.pending_prose_cross = None;
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
        snap_match_to_prose_grid(state);
    } else {
        crate::input::scroll::center_cursor(state);
    }
    // Route the landing through update_highlight like EVERY other cursor-
    // moving nav path. Without it, a next-match landing on the SAME page
    // (scroll churn nets to zero: set_page_instant forward, prose-grid snap
    // straight back) left the freshly applied current-match tag un-rendered —
    // in the buffer at the right range (has_tag readback true, priority above
    // the pale tag) but still painting the pale all-matches background until
    // some later tag churn re-laid-out the line (a j/k press revealed it;
    // queue_draw alone did NOT — the cached display line, not the paint, was
    // stale). update_highlight's visible-range tag pass forces that relayout,
    // and also keeps the clip-boundary rescheduling invariants that all other
    // cursor moves maintain. Repro: TrollopeBBC `/felix` Return then `n`
    // (2026-07-10).
    crate::input::highlight::update_highlight(state);
}

/// Prose-table mode: re-anchor a search landing onto the stored prose grid.
/// `canonical_page_top_for` returns a bare LINE, but prose page tops are
/// `(line, row-offset px)` — the landing therefore sat off-grid (e.g. (904,0)
/// vs the grid's (904,34)) and the NEXT `x` advanced only the few px to the
/// next grid boundary instead of a full page (TrollopeBBC, 2026-07-10).
/// Anchor to the page containing the MATCH'S OWN wrapped row, not the line's
/// first row: on a paragraph split across pages the matched word can sit on a
/// later row belonging to the next page, and the first-row rule would leave
/// the word hidden under the bottom clip. No-op for plays (their canonical
/// landing already reads the play table) and for live-engine prose.
fn snap_match_to_prose_grid(state: &mut AppState) {
    use gtk4::prelude::{TextBufferExt, TextViewExt};
    let Some(table) = crate::input::prose_pages::active_prose_page_table(state) else {
        return;
    };
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    let m = &state.search_matches[state.search_match_idx.min(total - 1)];
    let (line, byte_start) = (m.line_index, m.byte_start);
    // Pixel offset of the match's display row within its line. 0 on any
    // failure — degrades to the line's first row, the pre-existing landing.
    let off = (|| {
        let line_start = state.buffer.iter_at_line(line as i32)?;
        let m_iter = state
            .buffer
            .iter_at_line_index(line as i32, byte_start as i32)?;
        let line_top = state.text_view.line_yrange(&line_start).0;
        Some(state.text_view.iter_location(&m_iter).y() - line_top)
    })()
    .unwrap_or(0)
    .max(0);
    if let Some(i) = crate::input::prose_pages::prose_page_for_position(&table, line, off) {
        let (t, o) = (table[i].start_line, table[i].start_off);
        if (t, o) != (state.page_top_line, state.page_top_offset) {
            crate::logging::log(&format!(
                "SEARCH: prose-grid landing ({},{}) -> ({},{}) match=({},{})",
                state.page_top_line, state.page_top_offset, t, o, line, off
            ));
            crate::input::scroll::set_page_instant_offset(state, t, o);
        }
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
    crate::log_fmt!(
        "SEARCH_HL: current idx={} line={} chars={}..{}",
        state.search_match_idx, m.line_index, char_start, char_end
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_case_lowercase_query_is_insensitive() {
        assert!(!has_unescaped_uppercase("jack cade"));
        let re = build_matcher("jack cade");
        assert!(re.is_match("Jack Cade"));
        assert!(re.is_match("JACK CADE"));
    }

    #[test]
    fn smart_case_uppercase_query_is_sensitive() {
        assert!(has_unescaped_uppercase("Jack"));
        let re = build_matcher("Jack");
        assert!(re.is_match("Jack Cade"));
        assert!(!re.is_match("jack cade"));
    }

    #[test]
    fn escaped_uppercase_does_not_trigger_case_sensitivity() {
        // \W is a regex class, not an uppercase literal
        assert!(!has_unescaped_uppercase(r"jack\Wcade"));
        let re = build_matcher(r"jack\Wcade");
        assert!(re.is_match("Jack Cade"));
    }

    #[test]
    fn inline_flag_overrides_smart_case() {
        // uppercase in query, but (?i) forces insensitivity
        let re = build_matcher("(?i)Jack");
        assert!(re.is_match("jack cade"));
    }

    #[test]
    fn regex_alternation_matches() {
        let re = build_matcher("jack|john");
        assert!(re.is_match("John Holland"));
        assert!(re.is_match("Jack Cade"));
        assert!(!re.is_match("Dick the butcher"));
    }

    #[test]
    fn invalid_pattern_falls_back_to_literal() {
        // Half-typed regex: unclosed group
        let re = build_matcher("jack(");
        assert!(re.is_match("this jack( literal"));
        assert!(!re.is_match("jack"));
        // Unclosed class falls back too
        let re = build_matcher("cade[");
        assert!(re.is_match("cade[ bracket"));
    }

    fn collect(line: &str, query: &str) -> Vec<(usize, usize)> {
        let re = build_matcher(query);
        let mut out: Vec<crate::app::SearchMatch> = Vec::new();
        collect_line(line, &re, 0, &mut out);
        out.iter().map(|m| (m.byte_start, m.byte_end)).collect()
    }

    #[test]
    fn collect_line_finds_all_occurrences() {
        assert_eq!(collect("cade and Cade and CADE", "cade"), vec![(0, 4), (9, 13), (18, 22)]);
    }

    #[test]
    fn collect_line_offsets_index_original_text_non_ascii() {
        // 'İ' (2 bytes) lowercases to "i̇" (3 bytes): the old lowercase-then-find
        // path shifted offsets on lines like this. Offsets must slice the
        // ORIGINAL text.
        let line = "İstanbul cade here";
        let got = collect(line, "cade");
        assert_eq!(got.len(), 1);
        let (s, e) = got[0];
        assert_eq!(&line[s..e], "cade");
    }

    #[test]
    fn collect_line_skips_zero_width_matches() {
        // `a*` matches empty at every position while typing; none may land
        assert!(collect("bbb", "a*").is_empty());
        // but real (non-empty) hits of the same pattern still match
        assert_eq!(collect("baab", "a*"), vec![(1, 3)]);
    }

    #[test]
    fn collect_line_regex_class_matches() {
        assert_eq!(collect("Jack Cade", r"jack\Wcade"), vec![(0, 9)]);
    }

    #[test]
    fn valid_metachar_queries_are_regexes_not_literals() {
        // `what?` COMPILES as a regex (optional `t`) — no fallback occurs,
        // so it matches "wha"/"what", by design of always-regex mode.
        let re = build_matcher("what?");
        assert!(re.is_match("whale"));
    }
}
