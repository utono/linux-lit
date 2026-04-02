use std::cell::RefCell;
use std::rc::Rc;

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

    let mut new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    // Skip over translation lines
    if state.translations_visible && !state.translation_lines.is_empty() {
        let direction = if delta > 0 { 1i32 } else { -1i32 };
        while new_line < state.translation_lines.len()
            && state.translation_lines[new_line]
        {
            let next = new_line as i32 + direction;
            if next < 0 || next >= line_count as i32 {
                break;
            }
            new_line = next as usize;
        }
    }

    if new_line == state.current_line {
        return;
    }

    state.current_line = new_line;
    update_highlight(state);

    scroll_to_cursor(state);

    auto_show_vocab_popup(state);
}

/// Jump to the first line.
pub fn jump_to_start(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    let target = if let Some(ref lm) = state.line_map {
        lm.dialogue_buffer_lines.first().copied().unwrap_or(0)
    } else {
        work.lines
            .iter()
            .position(|l| l.is_dialogue)
            .unwrap_or(0)
    };

    state.current_line = target;
    update_highlight(state);
    set_page_instant(state, target);
    seek_to_current_line(state);
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
    seek_to_current_line(state);
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

    // Move cursor to center of the new page
    state.current_line = (new_top + lpp / 2).min(line_count - 1);
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

    let line_count = state.effective_line_count();
    // Move cursor to center of the new page
    state.current_line = (new_top + lpp / 2).min(line_count.saturating_sub(1));
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top);
}

/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
/// Previous paragraph (`,` key).
/// Jump to the first non-blank line of the previous paragraph (separated by blank lines).
pub fn jump_to_prev_paragraph(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if state.current_line == 0 || line_count == 0 {
        return;
    }

    let buffer = &state.buffer;

    // In plays, jump to the previous dialogue line
    if state.dialogue_formatting_active {
        if let Some(target) = prev_dialogue_line(buffer, state.current_line) {
            state.current_line = target;
            // Clear stale pending_advance so old advance state doesn't bounce
            state.pending_advance = None;
            state.pending_advance_ignore_bl = None;
            update_highlight(state);
            scroll_after_jump_backward(state);
            seek_to_current_line(state);
            auto_show_vocab_popup(state);
        }
        return;
    }

    let mut i = state.current_line.saturating_sub(1);

    // Skip blank lines immediately above
    while i > 0 && is_blank_buffer_line(buffer, i) {
        i -= 1;
    }
    // Skip non-blank lines (current paragraph body)
    while i > 0 && !is_blank_buffer_line(buffer, i) {
        i -= 1;
    }
    // Now i is on a blank line (or 0). Find the first non-blank line of the paragraph above.
    // If i is 0 and non-blank, that's the target. Otherwise advance past the blank.
    let target = if is_blank_buffer_line(buffer, i) {
        let mut start = i + 1;
        while start < line_count && is_blank_buffer_line(buffer, start) {
            start += 1;
        }
        if start < line_count && start < state.current_line {
            Some(start)
        } else {
            Some(0)
        }
    } else {
        Some(0)
    };

    if let Some(line_idx) = target {
        state.current_line = line_idx;
        update_highlight(state);
        scroll_after_jump_backward(state);
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
}

/// Next paragraph (`q` key).
/// Jump to the first non-blank line of the next paragraph (separated by blank lines).
/// In plays, jump to the next dialogue line instead.
pub fn jump_to_next_paragraph(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }

    let buffer = &state.buffer;

    // In plays, jump to the next dialogue line
    if state.dialogue_formatting_active {
        if let Some(target) = next_dialogue_line(buffer, state.current_line, line_count) {
            state.current_line = target;
            // Clear stale pending_advance so old advance state doesn't bounce
            state.pending_advance = None;
            state.pending_advance_ignore_bl = None;
            update_highlight(state);
            scroll_after_jump_forward(state);
            seek_to_current_line(state);
            auto_show_vocab_popup(state);
        }
        return;
    }

    let mut i = state.current_line + 1;

    // Skip remaining lines of current paragraph
    while i < line_count && !is_blank_buffer_line(buffer, i) {
        i += 1;
    }
    // Skip blank lines between paragraphs
    while i < line_count && is_blank_buffer_line(buffer, i) {
        i += 1;
    }

    if i < line_count {
        state.current_line = i;
        update_highlight(state);
        scroll_after_jump_forward(state);
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
}

/// Check if a buffer line is blank (empty or whitespace only).
fn is_blank_buffer_line(buffer: &sourceview5::Buffer, line: usize) -> bool {
    let text = buffer_line_text(buffer, line);
    text.trim().is_empty()
}

/// Get the text content of a buffer line.
fn buffer_line_text(buffer: &sourceview5::Buffer, line: usize) -> String {
    let Some(start) = buffer.iter_at_line(line as i32) else {
        return String::new();
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(&start, &end, false).to_string()
}

/// Check if a buffer line is a dialogue line (not blank, speaker, stage direction, or marker).
fn is_dialogue_line(buffer: &sourceview5::Buffer, line: usize) -> bool {
    use crate::db::line_types;
    let text = buffer_line_text(buffer, line);
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !line_types::is_speaker(trimmed)
        && !line_types::is_stage_direction(trimmed)
        && !line_types::is_act_scene_marker(trimmed)
        && !line_types::is_separator(trimmed)
}

/// Find the next dialogue line after `current`.
fn next_dialogue_line(buffer: &sourceview5::Buffer, current: usize, line_count: usize) -> Option<usize> {
    let mut i = current + 1;
    while i < line_count {
        if is_dialogue_line(buffer, i) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the previous dialogue line before `current`.
fn prev_dialogue_line(buffer: &sourceview5::Buffer, current: usize) -> Option<usize> {
    if current == 0 {
        return None;
    }
    let mut i = current - 1;
    loop {
        if is_dialogue_line(buffer, i) {
            return Some(i);
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

/// Previous chapter line (`[` key).
pub fn jump_to_prev_chapter(state: &mut AppState) {
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if state.current_line == 0 {
            return;
        }
        if let Some(ref lm) = state.line_map {
            let mut found = None;
            for bl in (0..state.current_line).rev() {
                if let Some(Some(wi)) = lm.buffer_to_work.get(bl) {
                    if work.lines[*wi].is_chapter {
                        found = Some(bl);
                        break;
                    }
                }
            }
            found
        } else {
            let mut found = None;
            for i in (0..state.current_line).rev() {
                if work.lines[i].is_chapter {
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
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => scroll_to_cursor(state),
        }
        seek_to_current_line(state);
    }
}

/// Next chapter line (`{` key).
pub fn jump_to_next_chapter(state: &mut AppState) {
    let line_count = state.effective_line_count();
    let target = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        if let Some(ref lm) = state.line_map {
            let mut found = None;
            for bl in (state.current_line + 1)..lm.buffer_to_work.len() {
                if let Some(Some(wi)) = lm.buffer_to_work.get(bl) {
                    if work.lines[*wi].is_chapter {
                        found = Some(bl);
                        break;
                    }
                }
            }
            found
        } else {
            let mut found = None;
            for i in (state.current_line + 1)..line_count {
                if work.lines[i].is_chapter {
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
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                if needs_page_turn_down(state, line_idx) {
                    set_page(state, line_idx);
                }
            }
        }
        seek_to_current_line(state);
    }
}

/// Restore cursor position after loading a work (used on startup with MRU).
pub fn restore_cursor(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => {
            let adj = state.scrolled_window.vadjustment();
            let max_scroll = adj.upper() - adj.page_size();
            let line_y = scroll_value_for_line(state, state.current_line);
            let offset = adj.page_size() * 0.25;
            let centered = (line_y - offset).max(0.0).min(max_scroll.max(0.0));
            adj.set_value(centered);
        }
        crate::config::NavigationMode::EReader => {
            let new_top = state.current_line.saturating_sub(PAGE_OVERLAP);
            set_page_instant(state, new_top);
        }
    }
}

// ---------------------------------------------------------------------------
// Page management
// ---------------------------------------------------------------------------

/// Ensure cursor is on the current page. If not, page turn to show it.
/// For backward movement, cursor appears near page bottom.
/// For forward movement, cursor appears near page top.
fn ensure_cursor_on_page(state: &mut AppState) {
    crate::logging::log(&format!(
        "ENSURE: cursor={} page_top={}",
        state.current_line, state.page_top_line
    ));

    if state.current_line < state.page_top_line {
        // Went above — new page with cursor near bottom
        let lpp = lines_per_page(state);
        let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
        set_page(state, new_top);
    } else if needs_page_turn_down(state, state.current_line) {
        // At or past last fully visible line — new page with this line at top
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
    center_cursor(state);
}

/// Mode-aware scroll after a forward jump (`q` / next paragraph or dialogue).
/// In scroll mode, centers the cursor. In e-reader mode, moves cursor down
/// the page; when it reaches the last line, page-turns with that line at top.
fn scroll_after_jump_forward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => ensure_cursor_on_page(state),
    }
}

/// Mode-aware scroll after a backward jump (`,` / prev paragraph or dialogue).
/// In scroll mode, centers the cursor. In e-reader mode, cursor moves up the
/// page; when it reaches the first line (or above), page-turns with the cursor
/// line at the bottom of the new page (inverse of `q` forward behavior).
fn scroll_after_jump_backward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            let page_top = state.page_top_line;
            if state.current_line <= page_top {
                // At or above the first line — new page with cursor near bottom
                let lpp = lines_per_page(state);
                let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
                set_page(state, new_top);
            }
        }
    }
}

/// Scroll the viewport so the current line is vertically centered.
/// Near document edges, clamps so no blank space appears (scrolloff behavior).
fn center_cursor(state: &mut AppState) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    let line_y = scroll_value_for_line(state, state.current_line);
    let offset = adj.page_size() * 0.25;
    let centered = (line_y - offset).max(0.0).min(max_scroll);
    crate::logging::log(&format!(
        "CENTER: line={} line_y={:.0} offset={:.0} centered={:.0} max={:.0} old_val={:.0}",
        state.current_line, line_y, offset, centered, max_scroll, adj.value()
    ));
    adj.set_value(centered);
}

/// Seek MPV to the current line's start time (with preroll).
/// Called on every cursor movement so audio follows the reader.
/// When the target line has a timestamp, suppresses CursorSync briefly while MPV
/// processes the seek. When it has no timestamp, suppresses indefinitely so the
/// cursor stays where the user put it.
fn seek_to_current_line(state: &mut AppState) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    let work_idx = match state.work_line_for_buffer(state.current_line) {
        Some(wi) => wi,
        None => return,
    };

    if let Some(ts) = &work.lines[work_idx].timestamp {
        // Exact timestamp — brief suppression while MPV processes the seek
        state.suppress_sync_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
        let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
        let _ = state
            .cmd_tx
            .try_send(crate::mpv::MpvCommand::Seek(seek_time));
    } else {
        // No timestamp — suppress indefinitely so cursor stays put
        state.suppress_sync_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(86400));
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

/// Scroll the viewport by a fixed step without moving the cursor or seeking audio.
/// `delta` is +1 for down, -1 for up.  Scrolls by approximately one wrapped line height.
pub fn scroll_viewport(state: &mut AppState, delta: i32) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    // Use the height of the current line as the scroll step
    let step = state.buffer.iter_at_line(state.current_line as i32)
        .map(|iter| {
            let rect = state.text_view.iter_location(&iter);
            rect.height() as f64
        })
        .unwrap_or(30.0)
        .max(20.0);
    let new_val = (adj.value() + step * delta as f64).max(0.0).min(max_scroll);
    adj.set_value(new_val);
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
/// Update highlight only (no scrolling). Used for prose sentence mode where
/// scrolling is deferred until the next sentence is about to start.
pub fn update_highlight_only(state: &mut AppState) {
    update_highlight(state);
    auto_show_vocab_popup(state);
}

/// Scroll so that a paragraph's first line lands at the cursor position
/// (~35% down the viewport). Used when playback crosses a paragraph boundary.
pub fn scroll_paragraph_to_top(state: &mut AppState, para_start: usize) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => {
            let adj = state.scrolled_window.vadjustment();
            let max_scroll = adj.upper() - adj.page_size();
            if max_scroll <= 0.0 {
                return;
            }
            let line_y = scroll_value_for_line(state, para_start);
            let offset = adj.page_size() * 0.25;
            let val = (line_y - offset).max(0.0).min(max_scroll);
            adj.set_value(val);
        }
        crate::config::NavigationMode::EReader => {
            let lpp = lines_per_page(state);
            let cursor_offset = lpp * 35 / 100;
            let new_top = para_start.saturating_sub(cursor_offset);
            set_page(state, new_top);
        }
    }
}

/// Scroll to make the current line visible without re-applying highlight.
/// Used for deferred prose sentence scrolling.
pub fn ensure_visible_no_highlight(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, state.current_line) {
                set_page(state, state.current_line);
            } else {
                ensure_cursor_on_page(state);
            }
        }
    }
}

pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, state.current_line) {
                set_page(state, state.current_line);
            } else {
                ensure_cursor_on_page(state);
            }
        }
    }
    auto_show_vocab_popup(state);
}

/// Update highlight and center the current line on screen.
pub fn update_highlight_and_center(state: &mut AppState) {
    update_highlight(state);
    let lpp = lines_per_page(state);
    let new_top = state.current_line.saturating_sub(lpp / 2);
    set_page_instant(state, new_top);
    auto_show_vocab_popup(state);
}

/// If vocab auto-popup is enabled, show/update the popup for the current line.
fn auto_show_vocab_popup(state: &mut AppState) {
    if !state.vocab_popup_auto {
        return;
    }
    // If popup is already visible, refresh it for the new line
    if state.vocab_popup.is_visible() {
        crate::app::refresh_vocab_popup(state);
    } else {
        crate::app::open_vocab_popup(state);
    }
}

/// Position chunk's first line ~5 lines from top, move cursor there, update highlight.
pub fn position_chunk(state: &mut AppState) {
    if let Some(a_line) = state.ab_repeat.a_line {
        state.current_line = a_line;
        update_highlight(state);
        let new_top = a_line.saturating_sub(5);
        set_page_instant(state, new_top);
    }
}

/// Update visual state for the current line (visual selection highlighting).
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let (buf_start, buf_end) = buffer.bounds();

    // Always clear cursor line background from previous position
    let cl_tag = &state.cursor_line_tag;
    buffer.remove_tag(cl_tag, &buf_start, &buf_end);

    if !state.dim_enabled {
        // Remove all dimming when disabled
        buffer.remove_tag(tag, &buf_start, &buf_end);
        // Apply cursor line background so the current line is still visible
        if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            buffer.apply_tag(cl_tag, &line_start, &line_end);
        }
        return;
    }

    // Dim the entire buffer
    buffer.apply_tag(tag, &buf_start, &buf_end);

    // Undim the current line
    if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.remove_tag(tag, &line_start, &line_end);
    }

    // When a chunk is active, undim all lines within the chunk range
    if state.ab_repeat.chunk_index.is_some() {
        if let (Some(a), Some(b)) = (state.ab_repeat.a_line, state.ab_repeat.b_line) {
            for line_idx in a..=b {
                if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
                    let mut line_end = line_start;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    buffer.remove_tag(tag, &line_start, &line_end);
                }
            }
        }
    }

    // When visual selection is active, clear stale highlight then re-apply
    crate::input::visual::clear_selection_highlight(state);
    crate::input::visual::apply_selection_highlight(state);
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

/// Jump to the next vocab word occurrence after current position.
pub fn jump_to_next_vocab(state: &mut AppState) {
    if state.vocab_matches.is_empty() {
        return;
    }

    let next_idx = match state.vocab_match_idx {
        Some(idx) => {
            if idx + 1 < state.vocab_matches.len() {
                idx + 1
            } else {
                0
            }
        }
        None => {
            state.vocab_matches
                .iter()
                .position(|m| m.line_index > state.current_line)
                .unwrap_or(0)
        }
    };

    state.vocab_match_idx = Some(next_idx);
    let target_line = state.vocab_matches[next_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, target_line) {
                set_page(state, target_line);
            }
        }
    }
    seek_to_current_line(state);
}

/// Jump to a specific vocab match index. Used by concordance picker.
pub fn jump_to_vocab_at(state: &mut AppState, match_idx: usize) {
    if match_idx >= state.vocab_matches.len() {
        return;
    }
    state.vocab_match_idx = Some(match_idx);
    let target_line = state.vocab_matches[match_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if needs_page_turn_down(state, target_line) {
                set_page(state, target_line);
            }
        }
    }
    seek_to_current_line(state);
}

/// Jump to the previous vocab word occurrence before current position.
pub fn jump_to_prev_vocab(state: &mut AppState) {
    if state.vocab_matches.is_empty() {
        return;
    }

    let prev_idx = match state.vocab_match_idx {
        Some(idx) => {
            if idx > 0 {
                idx - 1
            } else {
                state.vocab_matches.len() - 1
            }
        }
        None => {
            state.vocab_matches
                .iter()
                .rposition(|m| m.line_index < state.current_line)
                .unwrap_or(state.vocab_matches.len() - 1)
        }
    };

    state.vocab_match_idx = Some(prev_idx);
    let target_line = state.vocab_matches[prev_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => scroll_to_cursor(state),
    }
    seek_to_current_line(state);
}

// --- Cross-work concordance navigation ---

/// Jump to the current concordance occurrence.
/// Loads the work if different from current, positions cursor on the line.
pub fn concordance_jump_to_current(
    state: &Rc<RefCell<AppState>>,
    handle: &tokio::runtime::Handle,
) {
    let (target_abbrev, target_line_id) = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        let hit = match conc.current_hit() {
            Some(h) => h,
            None => return,
        };
        (hit.work_abbrev.clone(), hit.line_mapping_id)
    };

    let current_abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        // Need to load a different work
        let state_clone = Rc::clone(state);
        let handle_clone = handle.clone();
        let abbrev = target_abbrev.clone();

        // Check if preloaded work matches
        let preloaded = {
            let mut s = state_clone.borrow_mut();
            if let Some(conc) = &mut s.concordance_state {
                if conc
                    .preloaded_work
                    .as_ref()
                    .map(|p| p.work_abbrev == abbrev)
                    .unwrap_or(false)
                {
                    conc.preloaded_work.take()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(preloaded) = preloaded {
            // Use preloaded work
            let mut s = state_clone.borrow_mut();
            crate::app::display_work(&mut s, preloaded.work);
            concordance_position_cursor(&mut s, target_line_id);
            concordance_update_bar(&s);
            drop(s);
            concordance_preload_next(&state_clone, &handle_clone);
        } else {
            // Async load via spawn_blocking
            glib::spawn_future_local(async move {
                let work = handle_clone
                    .spawn_blocking(move || {
                        let conn =
                            crate::db::queries::open_db().expect("Failed to open lit.db");
                        crate::db::queries::load_work(&conn, &abbrev).ok()
                    })
                    .await
                    .unwrap_or(None);
                if let Some(work) = work {
                    let mut s = state_clone.borrow_mut();
                    crate::app::display_work(&mut s, work);
                    concordance_position_cursor(&mut s, target_line_id);
                    concordance_update_bar(&s);
                    drop(s);
                    concordance_preload_next(&state_clone, &handle_clone);
                }
            });
        }
    } else {
        // Same work, just move cursor
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
        drop(s);
        concordance_preload_next(state, handle);
    }
}

/// Position cursor on the line with the given line_mapping_id.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    if let Some(work) = &state.current_work {
        if let Some(idx) = work.lines.iter().position(|l| l.id == line_mapping_id) {
            state.current_line = idx;
            update_highlight(state);
            center_cursor(state);
            seek_to_current_line(state);
        }
    }
}

/// Update the concordance status bar from current state.
fn concordance_update_bar(state: &AppState) {
    if let Some(conc) = &state.concordance_state {
        state
            .concordance_bar
            .update(&conc.status_label(), &conc.status_work());
    }
}

/// Kick off background preload of the next work in the concordance direction.
fn concordance_preload_next(
    state: &Rc<RefCell<AppState>>,
    handle: &tokio::runtime::Handle,
) {
    let next_abbrev = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        // Preload in forward direction
        match conc.next_work_abbrev(1) {
            Some(a) if Some(a) != s.current_work.as_ref().map(|w| w.abbrev.as_str()) => {
                a.to_string()
            }
            _ => return,
        }
    };

    let state_clone = Rc::clone(state);
    let handle_clone = handle.clone();
    let abbrev = next_abbrev;
    glib::spawn_future_local(async move {
        let work = handle_clone
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                crate::db::queries::load_work(&conn, &abbrev).ok()
            })
            .await
            .unwrap_or(None);
        if let Some(work) = work {
            let mut s = state_clone.borrow_mut();
            if let Some(conc) = &mut s.concordance_state {
                conc.preloaded_work = Some(crate::concordance::PreloadedWork {
                    work_abbrev: work.abbrev.clone(),
                    work,
                });
            }
        }
    });
}
