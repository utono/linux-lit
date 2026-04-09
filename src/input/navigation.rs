use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;

/// Seconds to seek before a line's start_time when navigating.
/// Provides audio context so playback doesn't start at a hard cut.
pub const SEEK_PREROLL: f64 = 0.2;

/// Seconds to highlight a line before playback actually reaches it.
/// Used by the MPV client's time-pos sync.
pub const SYNC_PREROLL: f64 = 0.0;

/// Move cursor by `delta` lines (j/k).
/// Going down: page turn when cursor reaches the last visible line.
/// Going up: smooth scroll to keep cursor visible.
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
    let top = target.saturating_sub(1);
    set_page_instant(state, top);
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

/// Find the next dialogue line at or after `from`.
fn next_dialogue_from(buffer: &sourceview5::Buffer, from: usize, line_count: usize) -> usize {
    use crate::db::line_types;
    for i in from..line_count {
        let text = buffer_line_text(buffer, i);
        if !line_types::is_blank(&text) && line_types::is_dialogue(&text, false) {
            return i;
        }
    }
    from
}

/// Find the last dialogue line in the range [from, from+count).
fn last_dialogue_in_page(buffer: &sourceview5::Buffer, from: usize, count: usize, line_count: usize) -> usize {
    use crate::db::line_types;
    let end = (from + count).min(line_count);
    let mut last = from;
    for i in from..end {
        let text = buffer_line_text(buffer, i);
        if !line_types::is_blank(&text) && line_types::is_dialogue(&text, false) {
            last = i;
        }
    }
    last
}

/// Given a dialogue line that will be the top of a page, back up one line
/// if immediately preceded by a speaker name so the speaker is visible.
fn back_up_for_speaker(buffer: &sourceview5::Buffer, line: usize) -> usize {
    use crate::db::line_types;
    if line == 0 {
        return line;
    }
    let prev = buffer_line_text(buffer, line - 1);
    if line_types::is_speaker(&prev) {
        line - 1
    } else {
        line
    }
}

/// Find the last buffer line that fits within the viewport, matching the
/// bottom clip calculation exactly. A line is included only if its full
/// height fits in the remaining usable space (widget height minus descender
/// guard). This ensures page_forward doesn't count clipped lines as "seen".
fn last_fully_visible_line(state: &AppState) -> usize {
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return state.page_top_line;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, state.page_top_line);
    let usable_height = widget_height - descender_guard;
    let mut total = 0;
    let mut last = state.page_top_line;
    for i in state.page_top_line..line_count {
        let Some(iter) = state.buffer.iter_at_line(i as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        // Match update_bottom_clip: line must fully fit in usable space
        if total + h > usable_height {
            break;
        }
        last = i;
        total += h;
    }
    // Back up past trailing speaker names and blank lines so a dangling
    // speaker at the bottom doesn't count as "visible" content.
    use crate::db::line_types;
    while last > state.page_top_line {
        let text = buffer_line_text(&state.buffer, last);
        if line_types::is_speaker(&text) || line_types::is_blank(&text) {
            last -= 1;
        } else {
            break;
        }
    }
    last
}

/// Page forward (Ctrl+d/f). The next page starts at the dialogue line
/// immediately after the last dialogue line visible on the current page,
/// backed up by one if preceded by a speaker name.
pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

    let last_visible = last_fully_visible_line(state);
    let last = last_dialogue_in_page(&state.buffer, state.page_top_line, last_visible.saturating_sub(state.page_top_line) + 1, line_count);
    let next = next_dialogue_from(&state.buffer, last + 1, line_count);

    // Debug: log page forward details
    {
        let lv_text = buffer_line_text(&state.buffer, last_visible);
        let ld_text = buffer_line_text(&state.buffer, last);
        let nx_text = if next < line_count { buffer_line_text(&state.buffer, next) } else { "(end)".into() };
        let widget_h = state.text_view.height();
        let desc_guard = descender_guard_px(&state.text_view, state.page_top_line);
        log_fmt!("PAGE_FWD: page_top={} last_visible={} last_dialogue={} next={}", state.page_top_line, last_visible, last, next);
        log_fmt!("PAGE_FWD: widget_h={} desc_guard={} usable_h={}", widget_h, desc_guard, widget_h - desc_guard);
        log_fmt!("PAGE_FWD: last_visible_text='{}'", lv_text.chars().take(60).collect::<String>());
        log_fmt!("PAGE_FWD: last_dialogue_text='{}'", ld_text.chars().take(60).collect::<String>());
        log_fmt!("PAGE_FWD: next_text='{}'", nx_text.chars().take(60).collect::<String>());
        // Log heights of lines near the boundary
        for i in last_visible.saturating_sub(2)..=(last_visible + 2).min(line_count - 1) {
            if let Some(iter) = state.buffer.iter_at_line(i as i32) {
                let (_y, h) = state.text_view.line_yrange(&iter);
                let t = buffer_line_text(&state.buffer, i);
                log_fmt!("PAGE_FWD: line {} h={} '{}'", i, h, t.chars().take(50).collect::<String>());
            }
        }
    }

    if next >= line_count {
        return; // already at end
    }
    let new_top = back_up_for_speaker(&state.buffer, next);

    // Remember current page so page_backward can return to it exactly
    state.page_history.push(state.page_top_line);

    state.current_line = next;
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top, PageDirection::Forward);
}

/// Page backward (Shift+,). Pop the previous page_top from the history
/// stack so we return to exactly the same page that page_forward came from.
pub fn page_backward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let Some(prev_top) = state.page_history.pop() else {
        return; // no history, already at first page
    };

    let line_count = state.effective_line_count();
    let next = next_dialogue_from(&state.buffer, prev_top, line_count);
    let new_top = back_up_for_speaker(&state.buffer, next);

    log_fmt!("PAGE_BWD: prev_top={} next={} new_top={} current_line={}", prev_top, next, new_top, state.current_line);

    state.current_line = next;
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top, PageDirection::Backward);
}

/// Move cursor to the last fully visible line on the current page (`Q` key).
pub fn cursor_to_page_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let last_vis = last_fully_visible_line(state);
    if state.current_line != last_vis {
        state.current_line = last_vis;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        update_highlight(state);
        seek_to_current_line(state);
    }
}

/// Go to previous page and place cursor on its last visible line (shift+comma).
pub fn page_backward_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let Some(prev_top) = state.page_history.pop() else {
        log_fmt!("NAV_BACK: no page_history to pop");
        return;
    };
    let new_top = back_up_for_speaker(&state.buffer, prev_top);
    // Set page first so last_fully_visible_line computes against the new page
    set_page(state, new_top, PageDirection::Backward);
    let last_vis = last_fully_visible_line(state);
    log_fmt!("NAV_BACK: Shift+comma prev_top={} new_top={} current_line={}", prev_top, new_top, last_vis);
    state.current_line = last_vis;
    state.pending_advance = None;
    state.pending_advance_ignore_bl = None;
    update_highlight(state);
    seek_to_current_line(state);
}

/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let buffer = &state.buffer;
    if let Some(target) = prev_dialogue_line(buffer, state.current_line) {
        let prev = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
        log_fmt!("NAV_PREV: comma from={} to={} page_top={}", prev, target, state.page_top_line);
        update_highlight(state);
        scroll_after_jump_backward(state);
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
}

/// Next dialogue line (`q` key).
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }
    let buffer = &state.buffer;
    if let Some(target) = next_dialogue_line(buffer, state.current_line, line_count) {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        log_fmt!("NAV_NEXT: q from={} to={} page_top={}", prev_line, target, state.page_top_line);
        update_highlight(state);
        scroll_after_jump_forward(state, prev_line);
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
}

/// Move cursor to previous line without seeking media (`j` key).
pub fn cursor_prev_line(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let target = state.current_line - 1;
    state.current_line = target;
    state.pending_advance = None;
    state.pending_advance_ignore_bl = None;
    state.prev_highlight_line.set(None);
    update_highlight(state);
    scroll_after_jump_backward(state);
    auto_show_vocab_popup(state);
}

/// Move cursor to next dialogue line without seeking media (`k` key).
pub fn cursor_next_dialogue(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }
    let buffer = &state.buffer;
    if let Some(target) = next_dialogue_line(buffer, state.current_line, line_count) {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        update_highlight(state);
        scroll_after_jump_forward(state, prev_line);
        auto_show_vocab_popup(state);
    }
}

/// Previous paragraph (`[` key).
/// Jump to the first non-blank line of the previous paragraph.
pub fn jump_to_prev_paragraph(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if state.current_line == 0 || line_count == 0 {
        return;
    }

    let buffer = &state.buffer;
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
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, line_idx);
            }
        }
        seek_to_current_line(state);
        auto_show_vocab_popup(state);
    }
}

/// Next paragraph (`{` key).
/// Jump to the first non-blank line of the next paragraph.
pub fn jump_to_next_paragraph(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }

    let buffer = &state.buffer;
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

/// Find the best page-top for a forward page turn targeting `target_line`.
/// Backs up from the target to include the speaker name and any blank line
/// between the speaker and the dialogue, so the new page starts with context.
fn page_turn_top(buffer: &sourceview5::Buffer, target_line: usize) -> usize {
    use crate::db::line_types;
    if target_line == 0 {
        return 0;
    }
    let mut top = target_line;
    // Walk backward over blank lines
    while top > 0 {
        let text = buffer_line_text(buffer, top - 1);
        if text.trim().is_empty() {
            top -= 1;
        } else {
            break;
        }
    }
    // If the line above is a speaker name, include it
    if top > 0 {
        let text = buffer_line_text(buffer, top - 1);
        if line_types::is_speaker(text.trim()) {
            top -= 1;
            // Also include a blank line above the speaker
            if top > 0 {
                let above = buffer_line_text(buffer, top - 1);
                if above.trim().is_empty() {
                    top -= 1;
                }
            }
        }
    }
    top
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
        let top = page_turn_top(&state.buffer, line_idx);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, top);
            }
        }
        seek_to_current_line(state);
    }
}

/// Next chapter line.
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
        let top = page_turn_top(&state.buffer, line_idx);
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page_instant(state, top);
            }
        }
        seek_to_current_line(state);
    }
}

/// Jump to the next bookmarked line (wraps around).
pub fn next_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || !is_bm.iter().any(|&b| b) {
        return;
    }
    let line_count = is_bm.len();
    for offset in 1..=line_count {
        let idx = (state.current_line + offset) % line_count;
        if is_bm[idx] {
            drop(is_bm);
            state.current_line = idx;
            update_highlight(state);
            let top = page_turn_top(&state.buffer, idx);
            match state.config.navigation_mode {
                crate::config::NavigationMode::Scroll => center_cursor(state),
                crate::config::NavigationMode::EReader => {
                    set_page_instant(state, top);
                }
            }
            seek_to_current_line(state);
            return;
        }
    }
}

/// Jump to the previous bookmarked line (wraps around).
pub fn prev_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || !is_bm.iter().any(|&b| b) {
        return;
    }
    let line_count = is_bm.len();
    for offset in 1..=line_count {
        let idx = (state.current_line + line_count - offset) % line_count;
        if is_bm[idx] {
            drop(is_bm);
            state.current_line = idx;
            update_highlight(state);
            let top = page_turn_top(&state.buffer, idx);
            match state.config.navigation_mode {
                crate::config::NavigationMode::Scroll => center_cursor(state),
                crate::config::NavigationMode::EReader => {
                    set_page_instant(state, top);
                }
            }
            seek_to_current_line(state);
            return;
        }
    }
}

/// Jump to a specific buffer line (used by bookmark jump-to-recent).
pub fn jump_to_line(state: &mut AppState, buffer_line: usize) {
    let line_count = state.effective_line_count();
    if buffer_line >= line_count {
        return;
    }
    state.current_line = buffer_line;
    update_highlight(state);
    let top = page_turn_top(&state.buffer, buffer_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            set_page_instant(state, top);
        }
    }
    seek_to_current_line(state);
}

// ---------------------------------------------------------------------------
// Page management
// ---------------------------------------------------------------------------

/// Check if a line is on screen at all (no padding requirement).
/// Used by playback sync to avoid premature page turns.
pub fn is_line_on_screen(state: &AppState, line: usize) -> bool {
    is_line_fully_visible(state, line)
}

/// Check whether a buffer line is fully visible in the viewport.
/// Uses buffer_to_window_coords to convert line_yrange into widget space,
/// then compares against the widget's allocated height.
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    // During work loading, GTK layout is stale — report all lines as visible
    // to prevent bogus page turns that crash the app.
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
    // Sum line heights from page_top to determine if `line` fits in the viewport.
    // Reserve a descender guard so the last line's descenders aren't clipped.
    let descender_guard = descender_guard_px(&state.text_view, state.page_top_line);
    let usable_height = state.text_view.height() - descender_guard;
    let buf = &state.buffer;
    let mut total_height = 0;
    for i in state.page_top_line..=line {
        let Some(iter) = buf.iter_at_line(i as i32) else { return false };
        let (_y, h) = state.text_view.line_yrange(&iter);
        total_height += h;
        if total_height > usable_height {
            return false;
        }
    }
    true
}


/// Scroll just enough to keep the current line visible. No page turn.
fn scroll_to_cursor(state: &mut AppState) {
    center_cursor(state);
}

/// Mode-aware scroll after a forward jump (`q` / next paragraph or dialogue).
/// `prev_line` is the cursor position before the jump. In e-reader mode, if the
/// new position triggers a page turn, the previous line becomes the top of the
/// new page (continuity — last line of old page = first line of new page).
fn scroll_after_jump_forward(state: &mut AppState, _prev_line: usize) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                // Remember current page for backward navigation
                state.page_history.push(state.page_top_line);
                // Put the dialogue line at the top, backing up to include speaker name
                let new_top = page_turn_top(&state.buffer, state.current_line);
                log_fmt!("NAV_PAGE_FWD: current={} old_top={} new_top={} history_len={}", state.current_line, state.page_top_line, new_top, state.page_history.len());
                set_page_instant(state, new_top);
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
                state.page_history.push(state.page_top_line);
                let lpp = lines_per_page(state);
                let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
                log_fmt!("NAV_PAGE_BACK: current={} old_top={} new_top={} lpp={} history_len={}", state.current_line, state.page_top_line, new_top, lpp, state.page_history.len());
                set_page_instant(state, new_top);
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
pub fn seek_to_current_line(state: &mut AppState) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    let work_idx = match state.work_line_for_buffer(state.current_line) {
        Some(wi) => wi,
        None => return,
    };

    if let Some(ts) = &work.lines[work_idx].timestamp {
        // Exact timestamp — brief suppression while MPV processes the seek.
        // Don't shorten an existing longer suppression (e.g. from display_work).
        let new_until = std::time::Instant::now() + std::time::Duration::from_millis(500);
        if state.suppress_sync_until.map_or(true, |existing| new_until > existing) {
            state.suppress_sync_until = Some(new_until);
        }
        let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
        log_fmt!("SEEK: line={} work_idx={} start={:.2} seek={:.2} suppress=500ms", state.current_line, work_idx, ts.start, seek_time);
        let _ = state
            .cmd_tx
            .try_send(crate::mpv::MpvCommand::Seek(seek_time));
    } else {
        // No timestamp — suppress indefinitely so cursor stays put
        log_fmt!("SEEK: line={} work_idx={} NO_TIMESTAMP suppress=86400s", state.current_line, work_idx);
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

/// Direction of a page turn, used by the Slide transition.
#[derive(Clone, Copy)]
enum PageDirection {
    Forward,
    Backward,
}


/// Capture the entire card (spacers + text) as a static Picture overlay.
/// Uses WidgetPaintable → Snapshot → RenderNode → Texture to freeze the frame.
/// Returns the Picture (already added to page_turn_overlay) or None if capture fails.
fn capture_page_snapshot(state: &AppState) -> Option<gtk4::Picture> {
    let widget = &state.card_vbox;
    let w = widget.width();
    let h = widget.height();
    if w <= 0 || h <= 0 {
        return None;
    }

    // Try the texture approach first (frozen snapshot, immune to opacity changes)
    let paintable = gtk4::WidgetPaintable::new(Some(widget));
    let snapshot = gtk4::Snapshot::new();
    use gtk4::prelude::PaintableExt;
    paintable.snapshot(&snapshot, w as f64, h as f64);

    if let Some(node) = snapshot.to_node() {
        if let Some(native) = widget.native() {
            if let Some(renderer) = native.renderer() {
                let viewport = gtk4::graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
                let texture = renderer.render_texture(&node, Some(&viewport));
                let pic = gtk4::Picture::for_paintable(&texture);
                pic.set_content_fit(gtk4::ContentFit::Fill);
                pic.set_size_request(w, h);
                state.page_turn_overlay.add_overlay(&pic);
                return Some(pic);
            }
        }
    }

    // Fallback: create a WidgetPaintable, then immediately disconnect it
    // from the widget so it becomes a frozen snapshot of the current frame.
    // Texture path failed (e.g. stale GPU state after suspend/resume).
    let wp = gtk4::WidgetPaintable::new(Some(widget));
    wp.set_widget(gtk4::Widget::NONE);
    let pic = gtk4::Picture::for_paintable(&wp);
    pic.set_content_fit(gtk4::ContentFit::Fill);
    pic.set_size_request(w, h);
    state.page_turn_overlay.add_overlay(&pic);
    Some(pic)
}

/// Set the page top line with an animated transition based on config.transition_style.
fn set_page(state: &mut AppState, new_top: usize, direction: PageDirection) {
    // During work loading, GTK layout is invalid — skip page turns entirely.
    if state.loading_work.get() {
        log_fmt!("PAGE_TURN: SKIPPED (loading_work=true) new_top={}", new_top);
        return;
    }
    log_fmt!(
        "PAGE_TURN: new_top={} old_top={} current_line={} transition={:?}",
        new_top, state.page_top_line, state.current_line, state.config.transition_style
    );

    match state.config.transition_style {
        crate::config::TransitionStyle::Instant => {
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
        }
        crate::config::TransitionStyle::Crossfade => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                return;
            };

            // Cancel any in-flight page animation
            if let Some(prev) = state.page_turn_anim.take() {
                prev.skip();
            }

            // Hide the card while GTK scrolls to the new position.
            // The snapshot is a sibling overlay in page_turn_overlay (which wraps
            // card_vbox), so hiding card_vbox doesn't hide the snapshot.
            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);

            // Gentle crossfade: value goes 1.0 → 0.0 over 800ms.
            // New content fades in fast (quadratic), snapshot fades out slow
            // (inverse quadratic). Keeps combined brightness near 100%.
            let overlay = state.page_turn_overlay.clone();
            let card = state.card_vbox.clone();
            let snap = snapshot_pic.clone();
            let target = adw::CallbackAnimationTarget::new(move |value| {
                let t = 1.0 - value as f64; // progress 0→1
                // Smoothstep S-curve for gentle timing
                let s = t * t * (3.0 - 2.0 * t);
                // Use sine/cosine on the smoothstepped value to preserve
                // total brightness: sin²+cos² = 1, no dark dip at midpoint.
                card.set_opacity((s * std::f64::consts::FRAC_PI_2).sin());
                snap.set_opacity((s * std::f64::consts::FRAC_PI_2).cos());
            });
            let anim = adw::TimedAnimation::new(
                &snapshot_pic,
                1.0,  // from
                0.0,  // to
                700,  // duration ms
                target,
            );
            anim.set_easing(adw::Easing::Linear);

            let snap_cleanup = snapshot_pic.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
            });

            anim.play();
            state.page_turn_anim = Some(anim);
        }
        crate::config::TransitionStyle::Slide => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                return;
            };

            // Cancel any in-flight page animation
            if let Some(prev) = state.page_turn_anim.take() {
                prev.skip();
            }

            let width = state.card_vbox.width() as f64;

            // Hide card while GTK scrolls to the new position
            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.card_vbox.set_margin_start(0);
            state.card_vbox.set_margin_end(0);

            // Animate snapshot sliding out: 0.0 → 1.0 progress, 250ms, ease-out-cubic.
            // Reveals card on the first frame after scroll settles.
            let overlay = state.page_turn_overlay.clone();
            let card = state.card_vbox.clone();
            let snap = snapshot_pic.clone();
            let is_forward = matches!(direction, PageDirection::Forward);
            let revealed = std::cell::Cell::new(false);
            let target = adw::CallbackAnimationTarget::new(move |progress| {
                if !revealed.get() {
                    card.set_opacity(1.0);
                    revealed.set(true);
                }
                let offset = (width * progress) as i32;
                if is_forward {
                    snap.set_margin_start(0);
                    snap.set_margin_end(offset);
                    card.set_margin_start((width as i32) - offset);
                    card.set_margin_end(0);
                } else {
                    snap.set_margin_start(offset);
                    snap.set_margin_end(0);
                    card.set_margin_start(0);
                    card.set_margin_end((width as i32) - offset);
                }
            });
            let anim = adw::TimedAnimation::new(
                &snapshot_pic,
                0.0,  // from
                1.0,  // to
                250,  // duration ms
                target,
            );
            anim.set_easing(adw::Easing::EaseOutCubic);

            let snap_cleanup = snapshot_pic.clone();
            let card_cleanup = state.card_vbox.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                card_cleanup.set_margin_start(0);
                card_cleanup.set_margin_end(0);
            });

            anim.play();
            state.page_turn_anim = Some(anim);
        }
    }
}

/// Re-scroll to the current page_top and recalculate the bottom clip.
/// Called after font/size changes that invalidate line heights.
pub fn resnap_page(state: &mut AppState) {
    snap_scroll_to_line(state, state.page_top_line);
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
    // Position the target line at the very top of the viewport using
    // vadjustment for pixel-perfect positioning (scroll_to_iter is imprecise).
    if let Some(iter) = state.buffer.iter_at_line(line as i32) {
        let (y, _h) = state.text_view.line_yrange(&iter);
        let adj = state.scrolled_window.vadjustment();
        adj.set_value(y as f64);
    }

    // Update page line indicator with line_mapping.id
    if let Some(lm_id) = state.line_mapping_id_for_buffer(line) {
        state.page_line_label.set_text(&format!("{}", lm_id));
        state.page_line_label.set_visible(true);
    }

    // Schedule the clip height update for the next frame, after GTK has
    // completed the scroll and updated line layout positions.
    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let scrolled_window = state.scrolled_window.clone();
    let page_top = line;
    let line_count = state.effective_line_count();
    glib::idle_add_local_once(move || {
        update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, page_top, line_count);
    });
}

/// Compute a descender guard in pixels from the first visible line's height.
/// Uses ~20% of line height, which safely covers font descenders at any size.
fn descender_guard_px(text_view: &sourceview5::View, page_top: usize) -> i32 {
    let buf = text_view.buffer();
    if let Some(iter) = buf.iter_at_line(page_top as i32) {
        let (_y, h) = text_view.line_yrange(&iter);
        if h > 0 {
            return (h / 5).max(6);
        }
    }
    8 // fallback
}

/// Set the bottom clip to hide everything below the last fully-visible line.
/// Uses buffer line_yrange (absolute coords) to sum heights from page_top,
/// avoiding buffer_to_window_coords which may be stale after scroll_to_iter.
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
) {
    let widget_height = text_view.height();
    if widget_height <= 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    let buf = text_view.buffer();

    // Walk lines from page_top, summing heights until we exceed the viewport.
    // Reserve a descender guard based on the actual font descent so the clip
    // doesn't eat into the last visible line's descenders (g, p, y, q, j).
    let descender_guard = descender_guard_px(text_view, page_top);
    let usable_height = widget_height - descender_guard;

    let mut total_height = 0;
    let mut any_nonzero = false;
    let mut last_fit = page_top;
    for i in page_top..line_count {
        let Some(iter) = buf.iter_at_line(i as i32) else { break };
        let (_y, h) = text_view.line_yrange(&iter);
        if h > 0 {
            any_nonzero = true;
        }
        if total_height + h > usable_height {
            break;
        }
        total_height += h;
        last_fit = i;
    }

    if !any_nonzero {
        bottom_clip.set_height_request(0);
        return;
    }

    // Hide trailing speaker names (and blank lines before them) so a speaker
    // name never dangles at the bottom of a page without its dialogue.
    {
        use crate::db::line_types;
        let mut trim = last_fit;
        while trim > page_top {
            let text = {
                let Some(start) = buf.iter_at_line(trim as i32) else { break };
                let mut end = start;
                if !end.ends_line() { end.forward_to_line_end(); }
                buf.text(&start, &end, false).to_string()
            };
            if line_types::is_speaker(&text) || line_types::is_blank(&text) {
                if let Some(iter) = buf.iter_at_line(trim as i32) {
                    let (_y, h) = text_view.line_yrange(&iter);
                    total_height -= h;
                }
                trim -= 1;
            } else {
                break;
            }
        }
    }

    let clip = (widget_height - total_height).max(0);
    let scroll_val = scrolled_window.vadjustment().value();
    let expected_y = if let Some(iter) = buf.iter_at_line(page_top as i32) {
        let (y, _h) = text_view.line_yrange(&iter);
        y as f64
    } else {
        -1.0
    };
    let scroll_offset = scroll_val - expected_y;
    crate::logging::log(&format!(
        "BOTTOM_CLIP: widget_h={} total_h={} clip={} page_top={} scroll_val={:.1} expected_y={:.1} offset={:.1}",
        widget_height, total_height, clip, page_top, scroll_val, expected_y, scroll_offset
    ));
    bottom_clip.set_height_request(clip);
}

/// Scroll the viewport by a fixed step without moving the cursor or seeking audio.
/// `delta` is +1 for down, -1 for up.  Scrolls by ~3 line heights per step,
/// similar to browser-style scrolling.
pub fn scroll_viewport(state: &mut AppState, delta: i32) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    // Scroll by 3 line heights per keypress
    let line_height = state.buffer.iter_at_line(state.current_line as i32)
        .map(|iter| {
            let rect = state.text_view.iter_location(&iter);
            rect.height() as f64
        })
        .unwrap_or(30.0)
        .max(20.0);
    let step = line_height * 3.0;
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
                let dir = if para_start >= state.page_top_line { PageDirection::Forward } else { PageDirection::Backward };
                set_page(state, para_start, dir);
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
                let new_top = if state.current_line >= state.page_top_line {
                    page_turn_top(&state.buffer, state.current_line)
                } else {
                    state.current_line
                };
                log_fmt!(
                    "SYNC_PAGE: ensure_visible triggered, current_line={} page_top={} new_top={}",
                    state.current_line, state.page_top_line, new_top
                );
                let dir = if state.current_line >= state.page_top_line { PageDirection::Forward } else { PageDirection::Backward };
                set_page(state, new_top, dir);
            }
        }
    }
    auto_show_vocab_popup(state);
}

/// Like update_highlight_and_ensure_visible, but turns the page when the
/// highlight advances past the last visible line. The new page starts with
/// the new line at the top — no overlap with the previous page's content.
/// Used by playback sync.
pub fn update_highlight_and_advance_page(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            let last_vis = last_fully_visible_line(state);
            log_fmt!(
                "SYNC_ADVANCE: current={} last_vis={} page_top={}",
                state.current_line, last_vis, state.page_top_line
            );
            if state.current_line > last_vis {
                let new_top = page_turn_top(&state.buffer, state.current_line);
                state.page_history.push(state.page_top_line);
                set_page(state, new_top, PageDirection::Forward);
            }
        }
    }
    auto_show_vocab_popup(state);
}

/// Apply highlight tags, snap scroll to page_top_line, apply bottom clip,
/// then show the scrolled window. Called at the end of display_work_at
/// after the buffer and cursor position are fully set up.
pub fn update_highlight_and_show(state: &mut AppState) {
    if state.page_top_line == 0 && state.current_line > 0 {
        state.page_top_line = state.current_line;
    }
    update_highlight(state);

    let scroll_to = state.page_top_line;
    let line_count = state.effective_line_count();

    // Snap scroll position synchronously. line_yrange may return 0 if GTK
    // hasn't validated the layout yet, so we defer to an idle callback.
    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let loading_flag = state.loading_work.clone();
    let buffer = state.buffer.clone();
    let scrolled_window = state.scrolled_window.clone();

    glib::idle_add_local_once(move || {
        if let Some(iter) = buffer.iter_at_line(scroll_to as i32) {
            let (y, _h) = text_view.line_yrange(&iter);
            let adj = scrolled_window.vadjustment();
            let max_scroll = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value((y as f64).max(0.0).min(max_scroll));
        }
        // Show the scrolled window so GTK can complete the layout pass.
        scrolled_window.set_visible(true);
        // Defer clip calculation to the next idle tick so line_yrange
        // returns accurate heights after the widget is visible and laid out.
        glib::idle_add_local_once(move || {
            update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, scroll_to, line_count);
            loading_flag.set(false);
        });
    });
}

/// Update highlight and center the current line on screen.
pub fn update_highlight_and_center(state: &mut AppState) {
    update_highlight(state);
    let lpp = lines_per_page(state);
    let new_top = state.current_line.saturating_sub(lpp / 2);
    set_page_instant(state, new_top);
    auto_show_vocab_popup(state);
}

/// If vocab auto-popup is enabled, show/update the popup when the paragraph changes.
fn auto_show_vocab_popup(state: &mut AppState) {
    if !state.vocab_popup_auto {
        return;
    }
    if state.vocab_popup.is_visible() {
        // Only refresh when paragraph changes — avoid resetting word/view
        // position on every line advance within the same paragraph.
        let para = state.current_paragraph_range();
        if state.current_paragraph_start != Some(para.start) {
            crate::app::refresh_vocab_popup(state);
        }
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

/// Update visual state for the current line. Only applies dim/cursor tags
/// to the visible range (page_top_line +/- margin) for performance.
/// When dim is off, fades out the old cursor highlight smoothly.
fn update_highlight(state: &mut AppState) {
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
        if state.config.show_cursor_line {
        if let Some(old_line) = state.prev_highlight_line.get() {
            if old_line != state.current_line {
                // Cancel any in-flight cursor fade
                if let Some(prev) = state.cursor_fade_anim.take() {
                    prev.skip();
                }

                // Remove any existing fade, then apply to old line
                buffer.remove_tag(fade_tag, &buf_start, &buf_end);
                if let Some(old_start) = buffer.iter_at_line(old_line as i32) {
                    let mut old_end = old_start;
                    if !old_end.ends_line() {
                        old_end.forward_to_line_end();
                    }
                    buffer.apply_tag(fade_tag, &old_start, &old_end);
                }

                // Animate fade-out: alpha from 1.0 → 0.0, 150ms, ease-out-quad
                let fade_tag_clone = fade_tag.clone();
                let buf_clone = buffer.clone();
                let (rc_r, rc_g, rc_b) = crate::theme::root_color_rgb(&state.theme.root_color);
                let fade_alpha_max = if state.theme.is_light { 0.13_f32 } else { 0.15_f32 };
                let target = adw::CallbackAnimationTarget::new(move |value| {
                    let alpha = value as f32 * fade_alpha_max;
                    use gtk4::prelude::TextTagExt;
                    fade_tag_clone.set_paragraph_background_rgba(Some(
                        &gtk4::gdk::RGBA::new(rc_r, rc_g, rc_b, alpha),
                    ));
                    if value <= 0.0 {
                        let (s, e) = buf_clone.bounds();
                        buf_clone.remove_tag(&fade_tag_clone, &s, &e);
                    }
                });
                // Need a widget to attach the animation to — use text_view
                let anim = adw::TimedAnimation::new(
                    &state.text_view,
                    1.0,  // from
                    0.0,  // to
                    700,  // duration ms
                    target,
                );
                anim.set_easing(adw::Easing::EaseOutQuad);
                anim.play();
                state.cursor_fade_anim = Some(anim);
            }
        }
        } // show_cursor_line

        // Apply cursor line background to new line (if enabled)
        if state.config.show_cursor_line {
            if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                buffer.apply_tag(cl_tag, &line_start, &line_end);
            }
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
    // During work loading, GTK layout is invalid — return a reasonable estimate.
    if state.loading_work.get() {
        return 35;
    }

    let line_count = state.effective_line_count();
    let start = state.page_top_line;

    if line_count == 0 || start >= line_count {
        return 15;
    }

    // Sum line heights to count how many fit in the viewport.
    // Reserve a descender guard so the last line's descenders aren't clipped.
    let descender_guard = descender_guard_px(&state.text_view, start);
    let usable_height = state.text_view.height() - descender_guard;
    let buf = &state.buffer;
    let mut total_height = 0;
    let mut count = 0;
    for i in start..line_count {
        let Some(iter) = buf.iter_at_line(i as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        if total_height + h > usable_height {
            break;
        }
        total_height += h;
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
                set_page(state, target_line, PageDirection::Forward);
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
    _handle: &tokio::runtime::Handle,
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

    crate::logging::log(&format!(
        "CONC_JUMP: target_abbrev='{}' target_line_id={} current_abbrev={:?}",
        target_abbrev, target_line_id, current_abbrev
    ));

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        // Cross-work jump: spawn a new instance of linux-lit with the target work/line.
        // This avoids GTK lazy layout issues when loading large buffers in-process.
        crate::logging::log(&format!(
            "CONC_JUMP: spawning new instance for '{}' line_id={}", target_abbrev, target_line_id
        ));

        // Pause current MPV
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
        }

        let exe = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("target/debug/linux-lit"));
        let _ = std::process::Command::new(exe)
            .env("LINUX_LIT_WORK", &target_abbrev)
            .env("LINUX_LIT_LINE_ID", target_line_id.to_string())
            .spawn();
    } else {
        // Same work, just move cursor
        crate::logging::log("CONC_JUMP: same work, positioning cursor");
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
        crate::logging::log(&format!(
            "CONC_JUMP: positioned current_line={} page_top={}",
            s.current_line, s.page_top_line
        ));
        drop(s);
    }
}

/// Resolve the buffer index and sentence-start work index for a given line_mapping_id.
fn concordance_resolve_indices(state: &AppState, line_mapping_id: i64) -> Option<(usize, usize)> {
    let work = state.current_work.as_ref()?;
    let work_idx = work.lines.iter().position(|l| l.id == line_mapping_id)?;

    let buf_idx = if let Some(ref lm) = state.line_map {
        lm.work_to_buffer[work_idx]
    } else {
        work_idx
    };

    let seek_work_idx = if let Some(ref lm) = state.line_map {
        if let Some(sg) = crate::text_file_map::sentence_group_for(&lm.sentence_groups, buf_idx) {
            lm.buffer_to_work.get(sg.line_range.start)
                .copied()
                .flatten()
                .unwrap_or(work_idx)
        } else {
            find_sentence_start_by_timestamp(work, work_idx)
        }
    } else {
        find_sentence_start_by_timestamp(work, work_idx)
    };

    Some((buf_idx, seek_work_idx))
}

/// Seek MPV to the sentence start for a concordance hit.
fn concordance_seek(state: &mut AppState, seek_work_idx: usize) {
    if let Some(work) = &state.current_work {
        if let Some(ts) = work.lines.get(seek_work_idx).and_then(|l| l.timestamp.as_ref()) {
            state.suppress_sync_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
            let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
            let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
        }
    }
}

/// Position cursor on the line with the given line_mapping_id (same-work case).
/// The buffer layout is valid, so we scroll immediately.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    let (buf_idx, seek_work_idx) = match concordance_resolve_indices(state, line_mapping_id) {
        Some(v) => v,
        None => {
            crate::logging::log(&format!(
                "CONC_POS: resolve failed for line_mapping_id={}", line_mapping_id
            ));
            return;
        }
    };
    crate::logging::log(&format!(
        "CONC_POS: same-work buf_idx={} seek_work_idx={} line_mapping_id={}",
        buf_idx, seek_work_idx, line_mapping_id
    ));
    state.current_line = buf_idx;
    update_highlight(state);
    center_cursor(state);
    concordance_seek(state, seek_work_idx);
}

/// Find the first work-line index sharing the same sentence_start_time as `work_idx`.
/// Falls back to `work_idx` itself if no sentence time data is available.
fn find_sentence_start_by_timestamp(work: &crate::db::models::Work, work_idx: usize) -> usize {
    let target_ss = work.lines[work_idx]
        .timestamp
        .as_ref()
        .and_then(|t| t.sentence_start);
    let target_ss = match target_ss {
        Some(ss) => ss,
        None => return work_idx,
    };
    // Scan backwards to find the first line with the same sentence_start_time
    for i in (0..work_idx).rev() {
        let ss = work.lines[i]
            .timestamp
            .as_ref()
            .and_then(|t| t.sentence_start);
        match ss {
            Some(s) if (s - target_ss).abs() < 0.001 => continue,
            _ => return i + 1,
        }
    }
    0
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
/// Cycle through words on the current line, copying each to the system clipboard.
/// Each press advances to the next word; wraps after the last word.
/// Briefly bolds the word in the buffer for 2 seconds.
pub fn word_cycle_copy(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    // Reset index if we moved to a different line
    let idx = if state.word_cycle_line == Some(state.current_line) {
        state.word_cycle_index % words.len()
    } else {
        0
    };

    let (ref word, char_start, char_end) = words[idx];

    // Copy to clipboard via wl-copy
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(word.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COPY: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!("WORD_COPY: copied '{}' (word {}/{})", word, idx + 1, words.len());

    // Update cycle state
    state.word_cycle_line = Some(state.current_line);
    state.word_cycle_index = idx + 1;

    // Clear multi-word collect state (w is single-word mode)
    state.word_collect_words.clear();
    state.word_collect_ranges.clear();

    // Remove any previous underline tag, then apply to the current word
    apply_word_underline(state, &[(char_start, char_end)]);
}

/// Collect words on the current line, accumulating across presses.
/// Each W press advances to the next word, appends it to the collection,
/// and copies all collected words (space-separated) to the clipboard.
/// Underlines all collected words. Resets on line change.
pub fn word_collect_copy(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    // Reset if we moved to a different line
    let idx = if state.word_cycle_line == Some(state.current_line) {
        state.word_cycle_index % words.len()
    } else {
        state.word_collect_words.clear();
        state.word_collect_ranges.clear();
        0
    };

    let (ref word, char_start, char_end) = words[idx];

    // Append to collection
    state.word_collect_words.push(word.clone());
    state.word_collect_ranges.push((char_start, char_end));

    // Copy all collected words to clipboard
    let phrase = state.word_collect_words.join(" ");
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(phrase.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COLLECT: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!("WORD_COLLECT: copied '{}' ({} words)", phrase, state.word_collect_words.len());

    // Update cycle state
    state.word_cycle_line = Some(state.current_line);
    state.word_cycle_index = idx + 1;

    // Underline all collected words
    let ranges: Vec<(usize, usize)> = state.word_collect_ranges.clone();
    apply_word_underline(state, &ranges);
}

/// Extract words from the current buffer line with their char offsets.
fn extract_buffer_line_words(state: &AppState) -> Vec<(String, usize, usize)> {
    let buf_line_start = state.buffer.iter_at_line(state.current_line as i32).unwrap();
    let buf_line_end = {
        let mut it = buf_line_start;
        if !it.ends_line() { it.forward_to_line_end(); }
        it
    };
    let buf_line_text = state.buffer.text(&buf_line_start, &buf_line_end, false).to_string();

    let mut words: Vec<(String, usize, usize)> = Vec::new();
    for token in buf_line_text.split_whitespace() {
        let token_byte_start = token.as_ptr() as usize - buf_line_text.as_ptr() as usize;
        let token_char_start = buf_line_text[..token_byte_start].chars().count();
        let stripped = token.trim_matches(|c: char| !c.is_alphanumeric());
        if stripped.is_empty() {
            continue;
        }
        let strip_byte_offset = stripped.as_ptr() as usize - token.as_ptr() as usize;
        let char_start = token_char_start + token[..strip_byte_offset].chars().count();
        let char_end = char_start + stripped.chars().count();
        words.push((stripped.to_string(), char_start, char_end));
    }
    words
}

/// Apply the underline tag to the given char ranges on the current line,
/// removing any previous underline first. Auto-removes after 2 seconds.
fn apply_word_underline(state: &mut AppState, ranges: &[(usize, usize)]) {
    let buf = &state.buffer;
    let tag = &state.word_bold_tag;
    let (buf_start, buf_end) = (buf.start_iter(), buf.end_iter());
    buf.remove_tag(tag, &buf_start, &buf_end);

    let line_start = buf.iter_at_line(state.current_line as i32).unwrap();
    for &(char_start, char_end) in ranges {
        let mut word_start = line_start;
        word_start.forward_chars(char_start as i32);
        let mut word_end = word_start;
        word_end.forward_chars((char_end - char_start) as i32);
        buf.apply_tag(tag, &word_start, &word_end);
    }

    // Auto-remove underline after 2 seconds
    let gen = state.word_bold_gen.get() + 1;
    state.word_bold_gen.set(gen);
    let gen_rc = state.word_bold_gen.clone();
    let buf_clone = buf.clone();
    let tag_clone = tag.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        if gen_rc.get() == gen {
            let (s, e) = (buf_clone.start_iter(), buf_clone.end_iter());
            buf_clone.remove_tag(&tag_clone, &s, &e);
        }
    });
}

#[cfg(test)]
mod page_turn_tests {
    use crate::db::line_types;

    /// Load the cleaned Troilus text, stripping ## prefixes like the app does.
    fn load_troilus_lines() -> Vec<String> {
        let path = std::path::Path::new(
            "/home/mlj/utono/literature/shakespeare-william/folger-cleaned/troilus-and-cressida.txt",
        );
        if !path.exists() {
            panic!("Troilus cleaned file not found at {:?}", path);
        }
        let contents = std::fs::read_to_string(path).expect("Failed to read Troilus file");
        contents
            .lines()
            .map(|line| {
                if let Some(stripped) = line.strip_prefix("## ") {
                    stripped.to_string()
                } else {
                    line.to_string()
                }
            })
            .collect()
    }

    fn is_dialogue_line(text: &str) -> bool {
        !line_types::is_blank(text) && line_types::is_dialogue(text, false)
    }

    /// Collect all dialogue line indices in the file.
    fn dialogue_indices(lines: &[String]) -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, text)| is_dialogue_line(text))
            .map(|(i, _)| i)
            .collect()
    }

    /// Simulate next_dialogue_from on plain strings.
    fn next_dialogue(lines: &[String], from: usize) -> Option<usize> {
        for i in from..lines.len() {
            if is_dialogue_line(&lines[i]) {
                return Some(i);
            }
        }
        None
    }

    /// Simulate last_dialogue_in_page on plain strings.
    fn last_dialogue_in_range(lines: &[String], from: usize, count: usize) -> usize {
        let end = (from + count).min(lines.len());
        let mut last = from;
        for i in from..end {
            if is_dialogue_line(&lines[i]) {
                last = i;
            }
        }
        last
    }

    /// Simulate back_up_for_speaker on plain strings.
    fn back_up_for_speaker(lines: &[String], line: usize) -> usize {
        if line == 0 {
            return line;
        }
        if line_types::is_speaker(&lines[line - 1]) {
            line - 1
        } else {
            line
        }
    }

    /// Test page-forward through entire Troilus: every page turn must advance
    /// to the next dialogue line with no gaps or repeats.
    #[test]
    fn test_page_forward_no_gaps_or_repeats() {
        let lines = load_troilus_lines();
        let all_dialogue = dialogue_indices(&lines);
        assert!(
            all_dialogue.len() > 100,
            "Expected 100+ dialogue lines, got {}",
            all_dialogue.len()
        );

        let page_size = 30; // approximate lines per page
        let line_count = lines.len();

        // Track which dialogue lines we've highlighted, in order
        let mut highlighted: Vec<usize> = Vec::new();

        // Start: first dialogue line
        let first = all_dialogue[0];
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut current_line = first;
        highlighted.push(current_line);

        // Page forward until end
        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 500 {
                panic!("Page forward seems stuck after {} iterations", iterations);
            }

            // Simulate last_fully_visible_line: page_top + page_size
            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            let last = last_dialogue_in_range(&lines, page_top, last_visible - page_top + 1);
            let next = match next_dialogue(&lines, last + 1) {
                Some(n) => n,
                None => break, // reached end
            };
            if next >= line_count {
                break;
            }

            let new_top = back_up_for_speaker(&lines, next);
            page_top = new_top;
            current_line = next;
            highlighted.push(current_line);
        }

        // Verify: highlighted lines should be strictly increasing
        for i in 1..highlighted.len() {
            assert!(
                highlighted[i] > highlighted[i - 1],
                "Page forward: highlighted line {} (line {}) is not after {} (line {}), page {}",
                highlighted[i],
                lines[highlighted[i]].chars().take(50).collect::<String>(),
                highlighted[i - 1],
                lines[highlighted[i - 1]].chars().take(50).collect::<String>(),
                i
            );
        }

        // Verify: every highlighted line is a dialogue line
        for &h in &highlighted {
            assert!(
                is_dialogue_line(&lines[h]),
                "Highlighted line {} is not dialogue: '{}'",
                h,
                &lines[h]
            );
        }

        // Verify: no dialogue lines were skipped between consecutive highlights
        // (i.e., every dialogue line between two highlighted lines was on the same page)
        for i in 1..highlighted.len() {
            let prev = highlighted[i - 1];
            let curr = highlighted[i];
            // Find all dialogue lines between prev and curr
            let between: Vec<usize> = all_dialogue
                .iter()
                .filter(|&&d| d > prev && d < curr)
                .copied()
                .collect();
            // These should all be on the same page as prev (between page_top and last_visible)
            // We can't verify exact page boundaries without GTK, but we can verify
            // the gap isn't larger than page_size (which would mean we skipped a whole page)
            if !between.is_empty() {
                let gap = curr - prev;
                assert!(
                    gap <= page_size + 10, // allow some slack for speaker/blank lines
                    "Gap too large between highlights: line {} to {} (gap={}, {} dialogue lines between). Page {}",
                    prev, curr, gap, between.len(), i
                );
            }
        }

        println!(
            "Page forward test passed: {} pages, {} to {} ({} dialogue lines total)",
            highlighted.len(),
            highlighted[0],
            highlighted.last().unwrap(),
            all_dialogue.len()
        );
    }

    /// Test page-backward uses history stack: forward all the way, then backward
    /// all the way using the recorded history. Each backward step must return to
    /// the exact previous page_top, and the round-trip must reach the start.
    #[test]
    fn test_page_backward_via_history() {
        let lines = load_troilus_lines();
        let all_dialogue = dialogue_indices(&lines);

        let page_size = 30;
        let line_count = lines.len();

        // Page forward to the end, recording history (simulating page_history)
        let first = all_dialogue[0];
        let mut page_top = back_up_for_speaker(&lines, first);
        let mut history: Vec<usize> = Vec::new();
        let mut forward_tops: Vec<usize> = vec![page_top];

        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 500 { break; }
            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            let last = last_dialogue_in_range(&lines, page_top, last_visible - page_top + 1);
            let next = match next_dialogue(&lines, last + 1) {
                Some(n) => n,
                None => break,
            };
            if next >= line_count { break; }
            // Push current page_top before advancing (like the real code)
            history.push(page_top);
            let new_top = back_up_for_speaker(&lines, next);
            page_top = new_top;
            forward_tops.push(page_top);
        }

        // Now page backward using history (like the real code)
        let mut backward_tops: Vec<usize> = vec![page_top];
        while let Some(prev_top) = history.pop() {
            page_top = prev_top;
            backward_tops.push(page_top);
        }

        // Verify: backward tops are strictly decreasing
        for i in 1..backward_tops.len() {
            assert!(
                backward_tops[i] < backward_tops[i - 1],
                "History backward: top {} is not before {} at step {}",
                backward_tops[i], backward_tops[i - 1], i
            );
        }

        // Verify: backward reaches the first page
        assert_eq!(
            *backward_tops.last().unwrap(), forward_tops[0],
            "Backward didn't reach the first page"
        );

        // Verify: backward is exact reverse of forward
        assert_eq!(
            backward_tops.len(), forward_tops.len(),
            "Forward {} pages but backward {} pages",
            forward_tops.len(), backward_tops.len()
        );
        for i in 0..forward_tops.len() {
            assert_eq!(
                forward_tops[i],
                backward_tops[backward_tops.len() - 1 - i],
                "Round-trip mismatch at page {}: forward={} backward={}",
                i, forward_tops[i], backward_tops[backward_tops.len() - 1 - i]
            );
        }

        println!(
            "Page backward (history) test passed: {} pages, {} down to {}",
            backward_tops.len(),
            backward_tops[0],
            backward_tops.last().unwrap(),
        );
    }

    // --- Prose tests ---

    /// Helper: find next non-blank line at or after `from` (prose mode).
    fn next_nonblank(lines: &[String], from: usize) -> Option<usize> {
        for i in from..lines.len() {
            if !line_types::is_blank(&lines[i]) {
                return Some(i);
            }
        }
        None
    }

    /// Load a prose file's lines.
    fn load_prose_lines(path: &str) -> Vec<String> {
        let p = std::path::Path::new(path);
        if !p.exists() {
            panic!("Prose file not found at {}", path);
        }
        std::fs::read_to_string(p)
            .expect("read")
            .lines()
            .map(String::from)
            .collect()
    }

    /// Test page-forward through Bleak House: every page turn advances
    /// to the next non-blank line with no repeats.
    #[test]
    fn test_page_forward_prose_bleak_house() {
        let path = "/home/mlj/utono/literature/dickens-charles/bleak-house-prepared.txt";
        if !std::path::Path::new(path).exists() {
            eprintln!("SKIP: Bleak House not found");
            return;
        }
        let lines = load_prose_lines(path);
        let page_size = 30;
        let line_count = lines.len();

        let first = next_nonblank(&lines, 0).expect("no non-blank lines");
        let mut page_top = first;
        let mut current_line = first;
        let mut highlighted: Vec<usize> = vec![current_line];

        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 5000 { break; }

            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            // Last non-blank in page range
            let mut last = page_top;
            for i in page_top..=last_visible {
                if i < line_count && !line_types::is_blank(&lines[i]) {
                    last = i;
                }
            }
            let next = match next_nonblank(&lines, last + 1) {
                Some(n) => n,
                None => break,
            };

            page_top = next;
            current_line = next;
            highlighted.push(current_line);
        }

        for i in 1..highlighted.len() {
            assert!(
                highlighted[i] > highlighted[i - 1],
                "Prose forward: line {} not after {} at page {}",
                highlighted[i], highlighted[i - 1], i
            );
        }

        for &h in &highlighted {
            assert!(
                !line_types::is_blank(&lines[h]),
                "Highlighted line {} is blank", h
            );
        }

        println!(
            "Bleak House forward: {} pages, line {} to {} ({} total lines)",
            highlighted.len(), highlighted[0], highlighted.last().unwrap(), line_count
        );
    }

    /// Test page-backward through Bleak House via history stack:
    /// forward all the way recording history, then backward pops history.
    /// Round-trip must be exact.
    #[test]
    fn test_page_backward_prose_bleak_house() {
        let path = "/home/mlj/utono/literature/dickens-charles/bleak-house-prepared.txt";
        if !std::path::Path::new(path).exists() {
            eprintln!("SKIP: Bleak House not found");
            return;
        }
        let lines = load_prose_lines(path);
        let page_size = 30;
        let line_count = lines.len();

        let first = next_nonblank(&lines, 0).expect("no non-blank lines");
        let mut page_top = first;
        let mut history: Vec<usize> = Vec::new();
        let mut forward_tops: Vec<usize> = vec![page_top];

        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 5000 { break; }
            let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
            let mut last = page_top;
            for i in page_top..=last_visible {
                if i < line_count && !line_types::is_blank(&lines[i]) {
                    last = i;
                }
            }
            let next = match next_nonblank(&lines, last + 1) {
                Some(n) => n,
                None => break,
            };
            history.push(page_top);
            page_top = next;
            forward_tops.push(page_top);
        }

        // Backward via history
        let mut backward_tops: Vec<usize> = vec![page_top];
        while let Some(prev_top) = history.pop() {
            page_top = prev_top;
            backward_tops.push(page_top);
        }

        // Verify exact round-trip
        assert_eq!(
            backward_tops.len(), forward_tops.len(),
            "Forward {} pages but backward {} pages",
            forward_tops.len(), backward_tops.len()
        );
        for i in 0..forward_tops.len() {
            assert_eq!(
                forward_tops[i],
                backward_tops[backward_tops.len() - 1 - i],
                "Round-trip mismatch at page {}",
                i
            );
        }

        println!(
            "Bleak House backward (history): {} pages, {} down to {}",
            backward_tops.len(), backward_tops[0], backward_tops.last().unwrap()
        );
    }
}
