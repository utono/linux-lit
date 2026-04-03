use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

// Page overlap: when turning pages, this many lines from the old page
// remain visible on the new page for reading continuity.
const PAGE_OVERLAP: usize = 0;

/// Seconds to seek before a line's start_time when navigating.
/// Provides audio context so playback doesn't start at a hard cut.
pub const SEEK_PREROLL: f64 = 0.2;

/// Seconds to highlight a line before playback actually reaches it.
/// Used by the MPV client's time-pos sync.
pub const SYNC_PREROLL: f64 = 0.0;

/// Move cursor by `delta` lines (j/k).
/// Going down: page turn when cursor reaches the last visible line.
/// Going up: smooth scroll to keep cursor visible.
#[allow(dead_code)]
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

    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                if state.current_line < state.page_top_line {
                    let lpp = lines_per_page(state);
                    let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
                    set_page(state, new_top);
                } else {
                    let next = state.current_line + 1;
                    let line_count = state.effective_line_count();
                    if next < line_count {
                        set_page(state, next);
                    } else {
                        set_page(state, state.current_line);
                    }
                }
            }
        }
    }

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
            let prev_line = state.current_line;
            state.current_line = target;
            // Clear stale pending_advance so old advance state doesn't bounce
            state.pending_advance = None;
            state.pending_advance_ignore_bl = None;
            update_highlight(state);
            scroll_after_jump_forward(state, prev_line);
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
        let prev_line = state.current_line;
        state.current_line = i;
        update_highlight(state);
        scroll_after_jump_forward(state, prev_line);
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
                if !is_line_fully_visible(state, line_idx) {
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

/// Check if a line is on screen at all (no padding requirement).
/// Used by playback sync to avoid premature page turns.
fn is_line_on_screen(state: &AppState, line: usize) -> bool {
    is_line_fully_visible(state, line)
}

/// Check whether a buffer line is fully visible in the viewport.
/// Uses buffer_to_window_coords to convert line_yrange into widget space,
/// then compares against the widget's allocated height.
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return false;
    };
    let (buf_y, h) = state.text_view.line_yrange(&iter);
    let (_, win_y) = state.text_view.buffer_to_window_coords(
        gtk4::TextWindowType::Widget,
        0,
        buf_y,
    );
    let widget_height = state.text_view.height();
    win_y >= 0 && win_y + h <= widget_height
}


/// Scroll just enough to keep the current line visible. No page turn.
fn scroll_to_cursor(state: &mut AppState) {
    center_cursor(state);
}

/// Mode-aware scroll after a forward jump (`q` / next paragraph or dialogue).
/// `prev_line` is the cursor position before the jump. In e-reader mode, if the
/// new position triggers a page turn, the previous line becomes the top of the
/// new page (continuity — last line of old page = first line of new page).
fn scroll_after_jump_forward(state: &mut AppState, prev_line: usize) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                // Start new page at the line after the last fully visible one
                let new_top = prev_line + 1;
                let line_count = state.effective_line_count();
                if new_top < line_count {
                    set_page(state, new_top);
                } else {
                    set_page(state, prev_line);
                }
            }
        }
    }
}

/// Mode-aware scroll after a backward jump (`,` / prev paragraph or dialogue).
/// In e-reader mode, page-turns when cursor reaches the top line of the page.
fn scroll_after_jump_backward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if state.current_line < state.page_top_line {
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

/// Clear dim tags from the old visible range before a page turn.
/// Called before page_top_line is updated.
fn clear_old_page_dim(state: &AppState) {
    if !state.dim_enabled {
        return;
    }
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let lpp = lines_per_page(state);
    let margin = 5;
    let old_start = state.page_top_line.saturating_sub(margin);
    let old_end = (state.page_top_line + lpp + margin)
        .min(state.effective_line_count());
    let start_iter = buffer.iter_at_line(old_start as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let end_iter = buffer.iter_at_line(old_end as i32)
        .unwrap_or_else(|| buffer.end_iter());
    buffer.remove_tag(tag, &start_iter, &end_iter);
}

/// Crossfade duration in milliseconds.
const CROSSFADE_MS: f64 = 650.0;

/// Set the page top line with a fade-in transition. Sets text view opacity
/// to 0, scrolls to new position, then fades opacity back to 1.
fn set_page(state: &mut AppState, new_top: usize) {
    // Fade out: set opacity to 0 before scrolling
    state.text_view.set_opacity(0.0);

    // Scroll to new position
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    snap_scroll_to_line(state, new_top);

    // Fade in: animate opacity from 0 to 1
    let start_time = std::cell::Cell::new(None::<f64>);
    state.text_view.add_tick_callback(move |widget, clock| {
        let now = clock.frame_time() as f64 / 1_000.0;
        let t0 = start_time.get();
        let t0 = match t0 {
            Some(t) => t,
            None => {
                start_time.set(Some(now));
                now
            }
        };
        let elapsed = now - t0;
        let progress = (elapsed / CROSSFADE_MS).min(1.0);
        widget.set_opacity(progress);

        if progress >= 1.0 {
            widget.set_opacity(1.0);
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

/// Set the page top line and scroll instantly (no animation). For gg/G/restore.
fn set_page_instant(state: &mut AppState, new_top: usize) {
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    snap_scroll_to_line(state, new_top);
}

/// Scroll so `line` is at the top of the viewport, then size the bottom clip
/// overlay to hide any partially-visible line at the bottom of the page.
fn snap_scroll_to_line(state: &mut AppState, line: usize) {
    // Position the target line at the very top of the viewport
    if let Some(mut iter) = state.buffer.iter_at_line(line as i32) {
        state.text_view.scroll_to_iter(&mut iter, 0.0, true, 0.0, 0.0);
    }

    // Schedule the clip height update for the next frame, after GTK has
    // completed the scroll and updated line layout positions.
    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let page_top = line;
    let line_count = state.effective_line_count();
    glib::idle_add_local_once(move || {
        update_bottom_clip(&text_view, &bottom_clip, page_top, line_count);
    });
}

/// Compute the gap between the last fully visible line and the viewport bottom,
/// then set the bottom_clip overlay height to cover it.
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    page_top: usize,
    line_count: usize,
) {
    let widget_height = text_view.height();
    let buffer = text_view.buffer();
    let mut used = 0;
    for i in page_top..line_count {
        let Some(iter) = buffer.iter_at_line(i as i32) else { break };
        let (buf_y, h) = text_view.line_yrange(&iter);
        let (_, win_y) = text_view.buffer_to_window_coords(
            gtk4::TextWindowType::Widget,
            0,
            buf_y,
        );
        if win_y + h > widget_height {
            break;
        }
        used = win_y + h;
    }
    let gap = (widget_height - used).max(0);
    bottom_clip.set_height_request(gap);
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
/// Uses `line_yrange` which includes `pixels_above_lines` in the y coordinate,
/// so the line's full visual extent (including top spacing) is visible.
fn scroll_value_for_line(state: &AppState, line: usize) -> f64 {
    let adj = state.scrolled_window.vadjustment();
    let max = adj.upper() - adj.page_size();

    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return 0.0;
    };
    let (y, _h) = state.text_view.line_yrange(&iter);
    (y as f64).max(0.0).min(max)
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
            // Only page-turn if the paragraph start is off-screen
            if !is_line_on_screen(state, para_start) {
                crate::logging::log(&format!(
                    "PARA_SCROLL: para_start={} page_top={}",
                    para_start, state.page_top_line
                ));
                set_page(state, para_start);
            }
        }
    }
}


pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                set_page(state, state.current_line);
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

/// Duration for cursor line highlight crossfade in milliseconds.
const HIGHLIGHT_FADE_MS: f64 = 500.0;

/// Update visual state for the current line. Only applies dim/cursor tags
/// to the visible range (page_top_line +/- margin) for performance.
/// When dim is off, fades out the old cursor highlight smoothly.
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let cl_tag = &state.cursor_line_tag;
    let fade_tag = &state.cursor_fade_tag;

    // Compute visible range with margin for scroll overshoot
    let lpp = lines_per_page(state);
    let margin = 5;
    let vis_start = state.page_top_line.saturating_sub(margin);
    let vis_end = (state.page_top_line + lpp + margin)
        .min(state.effective_line_count());

    // Get iters for visible range
    let vis_start_iter = buffer.iter_at_line(vis_start as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let vis_end_iter = buffer.iter_at_line(vis_end as i32)
        .unwrap_or_else(|| buffer.end_iter());

    // Clear cursor line tag from full buffer (lightweight — only one line has it)
    let (buf_start, buf_end) = buffer.bounds();
    buffer.remove_tag(cl_tag, &buf_start, &buf_end);

    if !state.dim_enabled {
        // Remove dimming in visible range
        buffer.remove_tag(tag, &vis_start_iter, &vis_end_iter);

        // Apply fade-out to the old cursor line (if it changed)
        if let Some(old_line) = state.prev_highlight_line.get() {
            if old_line != state.current_line {
                // Remove any existing fade, then apply to old line
                buffer.remove_tag(fade_tag, &buf_start, &buf_end);
                if let Some(old_start) = buffer.iter_at_line(old_line as i32) {
                    let mut old_end = old_start;
                    if !old_end.ends_line() {
                        old_end.forward_to_line_end();
                    }
                    buffer.apply_tag(fade_tag, &old_start, &old_end);
                }
                // Start fade-out animation on the fade tag
                let fade_tag_clone = fade_tag.clone();
                let start_time = std::cell::Cell::new(None::<f64>);
                let buf_clone = buffer.clone();
                state.text_view.add_tick_callback(move |_widget, clock| {
                    let now = clock.frame_time() as f64 / 1_000.0;
                    let t0 = start_time.get();
                    let t0 = match t0 {
                        Some(t) => t,
                        None => {
                            start_time.set(Some(now));
                            now
                        }
                    };
                    let elapsed = now - t0;
                    let progress = (elapsed / HIGHLIGHT_FADE_MS).min(1.0);
                    let alpha = (1.0 - progress) as f32 * 0.10; // max alpha 0.10
                    use gtk4::prelude::TextTagExt;
                    fade_tag_clone.set_paragraph_background_rgba(Some(
                        &gtk4::gdk::RGBA::new(0.0, 0.3, 0.86, alpha),
                    ));
                    if progress >= 1.0 {
                        let (s, e) = buf_clone.bounds();
                        buf_clone.remove_tag(&fade_tag_clone, &s, &e);
                        return glib::ControlFlow::Break;
                    }
                    glib::ControlFlow::Continue
                });
            }
        }

        // Apply cursor line background to new line
        if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            buffer.apply_tag(cl_tag, &line_start, &line_end);
        }
        // When visual selection is active, apply highlight even when dim is off
        crate::input::visual::clear_selection_highlight(state);
        crate::input::visual::apply_selection_highlight(state);
        state.prev_highlight_line.set(Some(state.current_line));
        return;
    }

    // Dim visible range
    buffer.apply_tag(tag, &vis_start_iter, &vis_end_iter);

    // Undim the current line
    if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.remove_tag(tag, &line_start, &line_end);
    }

    // When a chunk is active, undim lines within the chunk range
    // (only the portion that overlaps with visible range)
    if state.ab_repeat.chunk_index.is_some() {
        if let (Some(a), Some(b)) = (state.ab_repeat.a_line, state.ab_repeat.b_line) {
            let chunk_start = a.max(vis_start);
            let chunk_end = b.min(vis_end.saturating_sub(1));
            for line_idx in chunk_start..=chunk_end {
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
    state.prev_highlight_line.set(Some(state.current_line));
}


// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count how many buffer lines are fully visible starting from `page_top_line`.
/// Uses buffer_to_window_coords to accurately detect clipped lines.
fn lines_per_page(state: &AppState) -> usize {
    let line_count = state.effective_line_count();
    let start = state.page_top_line;

    if line_count == 0 || start >= line_count {
        return 15;
    }

    let mut count = 0;
    for i in start..line_count {
        if !is_line_fully_visible(state, i) {
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
            if !is_line_fully_visible(state, target_line) {
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
            if !is_line_fully_visible(state, target_line) {
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
