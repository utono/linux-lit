use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;
use super::viewport::{
    is_line_fully_visible, last_fully_visible_line,
    lines_per_page, page_turn_top,
};
use super::scroll::{
    set_page, set_page_instant, scroll_to_cursor, PageDirection,
};

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

/// Update highlight and ensure cursor is visible, scrolling or page-turning
/// as needed.
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
            let last_vis = last_fully_visible_line(state, state.page_top_line);
            log_fmt!(
                "SYNC_ADVANCE: current={} last_vis={} page_top={}",
                state.current_line, last_vis, state.page_top_line
            );
            if state.current_line > last_vis {
                let new_top = page_turn_top(&state.buffer, state.current_line);
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
    if state.page_top_line == 0 && state.current_line > 0 && !state.pending_synopsis.get() {
        state.page_top_line = state.current_line;
    }
    update_highlight(state);

    // Page label is intentionally NOT set here. text_view.height() is still
    // 0 (scrolled_window is hidden), so viewport_page_for_line would build
    // a degenerate page_tops index and return page 1, causing a visible
    // "# - 1" flash. The resize tick refreshes the label once layout
    // settles (see app.rs deferred-layout-refresh branch).

    let scroll_to = state.page_top_line;
    let line_count = state.effective_line_count();
    let is_prose = state.is_prose();

    // Snap scroll position synchronously. line_yrange may return 0 if GTK
    // hasn't validated the layout yet, so we defer to an idle callback.
    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let loading_flag = state.loading_work.clone();
    let refresh_flag = state.needs_layout_refresh.clone();
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
            super::scroll::update_bottom_clip_public(&text_view, &bottom_clip, &scrolled_window, scroll_to, line_count, is_prose);
            loading_flag.set(false);
            // Signal the resize tick to refresh layout once line metrics
            // are valid (may take one or more frames after the scrolled
            // window becomes visible and GTK reflows the text).
            refresh_flag.set(true);
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
pub(crate) fn auto_show_vocab_popup(state: &mut AppState) {
    // If synopsis is currently showing, leave it alone (user toggled it on)
    if state.sidebar_mode == crate::app::SidebarMode::Synopsis && state.synopsis_visible {
        return;
    }

    if !state.vocab_popup_auto {
        return;
    }
    // Refresh whenever the current line changes; the refresh function
    // decides whether to show (line has vocab words) or hide (line has none).
    if state.vocab_popup_line != Some(state.current_line) {
        if state.vocab_popup.is_visible() {
            crate::app::refresh_vocab_popup(state);
        } else {
            crate::app::open_vocab_popup(state);
        }
    }
}

/// Update visual state for the current line. Only applies dim/cursor tags
/// to the visible range (page_top_line +/- margin) for performance.
/// When dim is off, fades out the old cursor highlight smoothly.
pub(crate) fn update_highlight(state: &mut AppState) {
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
                log_fmt!("CURSOR_LINE: applied tag to line {} bg={}", state.current_line, state.theme.cursor_line_bg);
            }
        } else {
            log_fmt!("CURSOR_LINE: show_cursor_line is OFF");
        }
        // When visual selection is active, apply highlight even when dim is off
        crate::input::visual::clear_selection_highlight(state);
        crate::input::visual::apply_selection_highlight(state);
        state.prev_highlight_line.set(Some(state.current_line));
        crate::app::update_title_bar_scene(state);
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

    crate::app::update_title_bar_scene(state);
}
