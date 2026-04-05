use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;

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
    state.prev_highlight_line.set(None);
    update_highlight(state);

    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, state.current_line) {
                if state.current_line < state.page_top_line {
                    let lpp = lines_per_page(state);
                    let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
                    set_page(state, new_top, PageDirection::Backward);
                } else {
                    let new_top = page_turn_top(&state.buffer, state.current_line);
                    set_page(state, new_top, PageDirection::Forward);
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
    set_page(state, new_top, PageDirection::Forward);
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
    set_page(state, new_top, PageDirection::Backward);
}

/// Previous dialogue line (`,` key).
/// If cursor is at the top line of the page, just page backward (don't move cursor).
/// Previous dialogue line (`,` key).
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let buffer = &state.buffer;
    if let Some(target) = prev_dialogue_line(buffer, state.current_line) {
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
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
        update_highlight(state);
        scroll_after_jump_forward(state, prev_line);
        seek_to_current_line(state);
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
                set_page(state, line_idx, PageDirection::Backward);
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
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                set_page(state, line_idx, PageDirection::Backward);
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
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => center_cursor(state),
            crate::config::NavigationMode::EReader => {
                if !is_line_fully_visible(state, line_idx) {
                    let dir = if line_idx >= state.page_top_line { PageDirection::Forward } else { PageDirection::Backward };
                    set_page(state, line_idx, dir);
                }
            }
        }
        seek_to_current_line(state);
    }
}

/// Restore cursor position after loading a work (used on startup with MRU).
pub fn restore_cursor(state: &mut AppState) {
    state.page_top_line = state.current_line;
    update_highlight(state);

    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let scrolled_window = state.scrolled_window.clone();
    let loading_flag = state.loading_work.clone();
    let line = state.current_line;
    let line_count = state.effective_line_count();
    let buffer = state.buffer.clone();
    let is_ereader = matches!(
        state.config.navigation_mode,
        crate::config::NavigationMode::EReader
    );
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        if is_ereader {
            if let Some(mut iter) = buffer.iter_at_line(line as i32) {
                text_view.scroll_to_iter(&mut iter, 0.0, true, 0.0, 0.0);
            }
            update_bottom_clip(&text_view, &bottom_clip, line, line_count);

            // Second clip update after layout has fully settled
            let tv2 = text_view.clone();
            let bc2 = bottom_clip.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                update_bottom_clip(&tv2, &bc2, line, line_count);
            });
        } else {
            let adj = scrolled_window.vadjustment();
            let max_scroll = adj.upper() - adj.page_size();
            if max_scroll > 0.0 {
                let offset = adj.page_size() * 0.25;
                // Estimate scroll position from line number
                let frac = line as f64 / line_count.max(1) as f64;
                let val = (frac * adj.upper() - offset).max(0.0).min(max_scroll);
                adj.set_value(val);
            }
        }
        loading_flag.set(false);
    });
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
    // During work loading, GTK layout is stale — report all lines as visible
    // to prevent bogus page turns that crash the app.
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
    // Sum line heights from page_top to determine if `line` fits in the viewport.
    let widget_height = state.text_view.height();
    let buf = &state.buffer;
    let mut total_height = 0;
    for i in state.page_top_line..=line {
        let Some(iter) = buf.iter_at_line(i as i32) else { return false };
        let (_y, h) = state.text_view.line_yrange(&iter);
        total_height += h;
        if total_height > widget_height {
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
                // Put the dialogue line at the top, backing up to include speaker name
                let new_top = page_turn_top(&state.buffer, state.current_line);
                set_page(state, new_top, PageDirection::Forward);
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
                set_page(state, new_top, PageDirection::Backward);
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

/// Set the bottom clip to hide everything below 34 lines of content.
/// Uses buffer line_yrange (absolute coords) to sum heights from page_top,
/// avoiding buffer_to_window_coords which may be stale after scroll_to_iter.
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
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
    // The clip hides everything past the last line that fits entirely.
    let mut total_height = 0;
    for i in page_top..line_count {
        let Some(iter) = buf.iter_at_line(i as i32) else { break };
        let (_y, h) = text_view.line_yrange(&iter);
        if total_height + h > widget_height {
            break;
        }
        total_height += h;
    }

    let clip = (widget_height - total_height).max(0);
    bottom_clip.set_height_request(clip);
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

/// Set page_top and apply highlight tags, then defer the actual scroll to a
/// timeout callback so GTK has time to lay out the new buffer content. Used by
/// display_work after replacing the entire buffer text.
pub fn update_highlight_deferred_scroll(state: &mut AppState) {
    state.page_top_line = state.current_line;
    update_highlight(state);

    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let loading_flag = state.loading_work.clone();
    let line = state.current_line;
    let line_count = state.effective_line_count();
    let buffer = state.buffer.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        if let Some(mut iter) = buffer.iter_at_line(line as i32) {
            text_view.scroll_to_iter(&mut iter, 0.0, true, 0.0, 0.0);
        }
        update_bottom_clip(&text_view, &bottom_clip, line, line_count);
        loading_flag.set(false);

        // Schedule a second clip update after layout has fully settled,
        // to catch cases where line heights weren't final at 50ms.
        let tv2 = text_view.clone();
        let bc2 = bottom_clip.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
            update_bottom_clip(&tv2, &bc2, line, line_count);
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
    let widget_height = state.text_view.height();
    let buf = &state.buffer;
    let mut total_height = 0;
    let mut count = 0;
    for i in start..line_count {
        let Some(iter) = buf.iter_at_line(i as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        if total_height + h > widget_height {
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
