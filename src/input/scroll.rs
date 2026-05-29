use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;
use super::viewport::{
    visible_range, trim_visible_range, descender_guard_px, lines_per_page,
};

// ---------------------------------------------------------------------------
// PageDirection / PageTurnLock
// ---------------------------------------------------------------------------

/// Direction of a page turn, used by the Slide transition.
#[derive(Clone, Copy)]
#[derive(Debug)]
pub(crate) enum PageDirection {
    Forward,
    Backward,
}

/// Re-entrancy lock for animated page turns.
///
/// `set_page` calls `try_acquire` before mutating `page_top_line` or starting
/// an animation. The animation's `connect_done` callback calls `release`. While
/// locked, secondary turn requests (typically from MPV `CursorSync` arriving
/// mid-animation) are dropped so they don't compose with the in-flight turn.
///
/// `set_page_instant` does NOT consult the lock — it has no animation, so the
/// re-entrancy window doesn't exist for that path.
///
/// Mirrors foliate-js's `Paginator.#locked` (paginator.js:1060-1071).
///
/// Uses `Cell<bool>` rather than `bool` so a `&PageTurnLock` borrow can mutate
/// it from a `connect_done` closure without a `&mut AppState`.
pub(crate) struct PageTurnLock {
    locked: std::cell::Cell<bool>,
}

impl PageTurnLock {
    pub(crate) fn new() -> Self {
        Self { locked: std::cell::Cell::new(false) }
    }

    /// Attempt to take the lock. Returns true if acquired, false if already held.
    pub(crate) fn try_acquire(&self) -> bool {
        if self.locked.get() {
            false
        } else {
            self.locked.set(true);
            true
        }
    }

    /// Release the lock. Idempotent — releasing when unlocked is a no-op.
    pub(crate) fn release(&self) {
        self.locked.set(false);
    }

    /// Peek without mutating.
    pub(crate) fn is_locked(&self) -> bool {
        self.locked.get()
    }
}

// ---------------------------------------------------------------------------
// Page setting / scroll plumbing
// ---------------------------------------------------------------------------

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
pub(crate) fn set_page(state: &mut AppState, new_top: usize, direction: PageDirection) {
    // During work loading, GTK layout is invalid — skip page turns entirely.
    if state.loading_work.get() {
        log_fmt!("PAGE_TURN: SKIPPED (loading_work=true) new_top={}", new_top);
        return;
    }
    // F1: drop racing turns so MPV CursorSync arriving mid-animation can't
    // compose with a key-driven turn. set_page_instant does not go through
    // here — it has no animation window.
    if !state.page_turn_lock.try_acquire() {
        log_fmt!(
            "PAGE_TURN: SKIPPED (locked=true) new_top={} old_top={} requested_dir={:?}",
            new_top, state.page_top_line, direction
        );
        return;
    }
    crate::logging::log_always(&format!(
        "PAGE_TURN: new_top={} old_top={} current_line={} transition={:?}",
        new_top, state.page_top_line, state.current_line, state.config.transition_style
    ));

    match state.config.transition_style {
        crate::config::TransitionStyle::Instant => {
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.page_turn_lock.release();
        }
        crate::config::TransitionStyle::Crossfade => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                state.page_turn_lock.release();
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
            let lock = std::rc::Rc::clone(&state.page_turn_lock);
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                lock.release();
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
                state.page_turn_lock.release();
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
            let lock = std::rc::Rc::clone(&state.page_turn_lock);
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                card_cleanup.set_margin_start(0);
                card_cleanup.set_margin_end(0);
                lock.release();
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

/// Re-run `update_bottom_clip` against the current viewport state without
/// touching the scroll position. Use after font / line-height changes that
/// don't shift `page_top_line` — e.g. translation toggle, where the caller
/// has already restored a custom scroll value and a full `resnap_page`
/// would clobber it.
pub fn refresh_bottom_clip(state: &AppState) {
    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        state.page_top_line,
        state.effective_line_count(),
        state.is_prose(),
    );
}

/// Fire `update_bottom_clip` twice: once via `idle_add` (covers the common
/// case where layout is already settled), once via `timeout(100ms)` (covers
/// post-font-change where GTK's layout pass for new metrics hasn't completed
/// yet — `line_yrange` returns pre-layout heights on the first pass and the
/// clip ends up sized for the OLD font's line heights). The second fire reads
/// post-layout metrics and corrects.
fn schedule_bottom_clip_update(
    text_view: sourceview5::View,
    bottom_clip: gtk4::Box,
    scrolled_window: gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
    is_prose: bool,
) {
    let tv1 = text_view.clone();
    let bc1 = bottom_clip.clone();
    let sw1 = scrolled_window.clone();
    glib::idle_add_local_once(move || {
        update_bottom_clip(&tv1, &bc1, &sw1, page_top, line_count, is_prose);
    });
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, page_top, line_count, is_prose);
    });
}

/// Set the page top line and scroll instantly (no animation). For gg/G/restore.
pub(crate) fn set_page_instant(state: &mut AppState, new_top: usize) {
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    snap_scroll_to_line(state, new_top);
}

/// Scroll so `line` is at the top of the viewport, then size the bottom clip
/// overlay to hide any partially-visible line at the bottom of the page.
pub(crate) fn snap_scroll_to_line(state: &mut AppState, line: usize) {
    let mut effective_top = line;

    if let Some(iter) = state.buffer.iter_at_line(line as i32) {
        let (y, _h) = state.text_view.line_yrange(&iter);
        let adj = state.scrolled_window.vadjustment();
        let max_value = (adj.upper() - adj.page_size()).max(0.0);
        let target = (y as f64).min(max_value);
        adj.set_value(target);

        // When the scroll was clamped, `line` can't appear at the viewport
        // top — earlier lines bleed in clipped. Walk backward to find the
        // line whose y <= the clamped scroll position and re-scroll to that
        // line's exact y so the page starts on a clean line boundary.
        if (y as f64) > max_value && line > 0 {
            for l in (0..line).rev() {
                if let Some(it) = state.buffer.iter_at_line(l as i32) {
                    let (ly, _) = state.text_view.line_yrange(&it);
                    if (ly as f64) <= max_value {
                        log_fmt!(
                            "SNAP_CLAMP: requested line {} (y={}) exceeds max_scroll={:.0}, corrected page_top to {} (y={})",
                            line, y, max_value, l, ly
                        );
                        adj.set_value(ly as f64);
                        effective_top = l;
                        state.page_top_line = l;
                        break;
                    }
                }
            }
        }
    }

    // F4: populate the cache synchronously so MPV sync handlers reading
    // is_line_fully_visible right after this call see the new range, not
    // stale state. The idle-scheduled update_bottom_clip below ALSO writes
    // the cache as a backstop for layout-not-yet-flushed cases.
    let widget_height = state.text_view.height();
    if widget_height > 0 {
        let descender_guard = descender_guard_px(&state.text_view, effective_top);
        let usable_height = widget_height - descender_guard - BASE_BOTTOM_MARGIN;
        let line_count = state.effective_line_count();
        let range = visible_range(&state.text_view, &state.buffer, effective_top, line_count, usable_height);
        state.last_visible_range.set(Some(range));
    } else {
        state.last_visible_range.set(None);
    }

    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        effective_top,
        state.effective_line_count(),
        state.is_prose(),
    );
}

/// Ensure the vadjustment's upper bound is large enough that any buffer line
/// can be scrolled to the viewport top. GTK computes upper from content height
/// + margins; near the end of the document that may not be enough. We extend
/// upper (via bottom_margin) so the last line is always reachable.
///
/// The minimum bottom_margin needed is `page_size` — this guarantees that even
/// the very last buffer line can appear at the viewport top with whitespace
/// below. We only increase, never decrease, to avoid fighting GTK's layout.
pub(crate) const BASE_BOTTOM_MARGIN: i32 = 40;
pub(crate) fn ensure_scroll_range(state: &AppState) {
    let page_size = state.scrolled_window.vadjustment().page_size();
    if page_size <= 0.0 {
        return;
    }
    let needed = (page_size as i32).max(BASE_BOTTOM_MARGIN);
    if state.text_view.bottom_margin() < needed {
        state.text_view.set_bottom_margin(needed);
    }
}

/// Public entry point for `update_bottom_clip`, used by `highlight::update_highlight_and_show`
/// which needs to call it from an idle callback.
pub(crate) fn update_bottom_clip_public(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
    is_prose: bool,
) {
    update_bottom_clip(text_view, bottom_clip, scrolled_window, page_top, line_count, is_prose);
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
    is_prose: bool,
) {
    let widget_height = text_view.height();
    if widget_height <= 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    let buf = text_view.buffer();
    let buf_sv: sourceview5::Buffer = match buf.downcast::<sourceview5::Buffer>() {
        Ok(b) => b,
        Err(_) => {
            bottom_clip.set_height_request(0);
            return;
        }
    };

    // When page_top would render the last buffer line within widget_height,
    // skip the descender_guard reservation — there's no next page below,
    // so no risk of half-clipping a "partial line" descender. Just fit
    // content into widget_height and let bottom_clip cover any whitespace.
    let descender_guard = descender_guard_px(text_view, page_top);
    let usable_height = {
        let probe = visible_range(text_view, &buf_sv, page_top, line_count, widget_height);
        if probe.last_fit + 1 >= line_count {
            widget_height
        } else {
            widget_height - descender_guard - BASE_BOTTOM_MARGIN
        }
    };

    let range = visible_range(text_view, &buf_sv, page_top, line_count, usable_height);

    if range.count == 0 || range.total_height == 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    let trimmed = trim_visible_range(range, page_top, text_view, &buf_sv, is_prose);

    // Viewport fill guard for display: if trimming left the page less than
    // ~85% full, the dangling-speaker/block trims created too much empty
    // space. Fall back to the raw visible range which only clips the partial
    // bottom line. A dangling speaker at the bottom looks better than 15%+
    // empty space.
    let display_range = if widget_height > 0
        && trimmed.last_fit < range.last_fit
        && trimmed.total_height * 20 < widget_height * 17
    {
        range
    } else {
        trimmed
    };

    let clip = (widget_height - display_range.total_height).max(0);
    let scroll_val = scrolled_window.vadjustment().value();
    let expected_y = if let Some(iter) = buf_sv.iter_at_line(page_top as i32) {
        let (y, _h) = text_view.line_yrange(&iter);
        y as f64
    } else {
        -1.0
    };
    let scroll_offset = scroll_val - expected_y;
    // Skip the redundant set_height_request when the clip value would be
    // unchanged. Calling set_height_request even with the same value forces
    // GTK to revalidate layout, which can re-shape Pango glyphs by 1-2px
    // (subpixel positioning). Visible as a "shift then snap back" 100ms
    // after the work first appears (the post-snap timeout backstop).
    let cur = bottom_clip.height_request();
    if cur == clip {
        crate::logging::log(&format!(
            "BOTTOM_CLIP: widget_h={} total_h={} clip={} page_top={} scroll_val={:.1} expected_y={:.1} offset={:.1} (unchanged, skipped)",
            widget_height, display_range.total_height, clip, page_top, scroll_val, expected_y, scroll_offset
        ));
        return;
    }
    crate::logging::log(&format!(
        "BOTTOM_CLIP: widget_h={} total_h={} clip={} page_top={} scroll_val={:.1} expected_y={:.1} offset={:.1}",
        widget_height, display_range.total_height, clip, page_top, scroll_val, expected_y, scroll_offset
    ));
    bottom_clip.set_height_request(clip);
}

// ---------------------------------------------------------------------------
// Scroll helpers
// ---------------------------------------------------------------------------

/// Scroll just enough to keep the current line visible. No page turn.
pub(crate) fn scroll_to_cursor(state: &mut AppState) {
    center_cursor(state);
}

/// Mode-aware scroll after a forward jump (`q` / next paragraph or dialogue).
/// `prev_line` is the cursor position before the jump. In e-reader mode, if the
/// new position triggers a page turn, the previous line becomes the top of the
/// new page (continuity — last line of old page = first line of new page).
pub(crate) fn scroll_after_jump_forward(state: &mut AppState, _prev_line: usize) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !state.is_prose()
                && super::viewport::is_first_dialogue_of_scene(
                    &state.buffer, &state.translation_lines, state.current_line,
                )
            {
                let new_top = super::viewport::back_up_for_speaker(&state.buffer, state.current_line);
                if new_top != state.page_top_line {
                    log_fmt!("NAV_SCENE_FWD: current={} old_top={} new_top={}", state.current_line, state.page_top_line, new_top);
                    set_page_instant(state, new_top);
                }
            } else if !super::viewport::is_line_fully_visible(state, state.current_line) {
                let new_top = super::viewport::page_turn_top(&state.buffer, state.current_line);
                log_fmt!("NAV_PAGE_FWD: current={} old_top={} new_top={}", state.current_line, state.page_top_line, new_top);
                set_page_instant(state, new_top);
            }
        }
    }
}

/// Mode-aware scroll after a backward jump (`,` / prev paragraph or dialogue).
/// In e-reader mode, page-turns when cursor reaches the top line of the page.
pub(crate) fn scroll_after_jump_backward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !state.is_prose()
                && super::viewport::is_first_dialogue_of_scene(
                    &state.buffer, &state.translation_lines, state.current_line,
                )
            {
                let new_top = super::viewport::back_up_for_speaker(&state.buffer, state.current_line);
                if new_top != state.page_top_line {
                    log_fmt!("NAV_SCENE_BACK: current={} old_top={} new_top={}", state.current_line, state.page_top_line, new_top);
                    set_page_instant(state, new_top);
                }
            } else if state.current_line < state.page_top_line {
                let new_top = super::viewport::page_turn_top(&state.buffer, state.current_line);
                log_fmt!("NAV_PAGE_BACK: page_turn_top new_top={} current={} old_top={}",
                         new_top, state.current_line, state.page_top_line);
                set_page_instant(state, new_top);
            }
        }
    }
}

/// Scroll the viewport so the current line is vertically centered.
/// Near document edges, clamps so no blank space appears (scrolloff behavior).
pub(crate) fn center_cursor(state: &mut AppState) {
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
pub(crate) fn scroll_value_for_line(state: &AppState, line: usize) -> f64 {
    let adj = state.scrolled_window.vadjustment();
    let max = adj.upper() - adj.page_size();

    let Some(iter) = state.buffer.iter_at_line(line as i32) else {
        return 0.0;
    };
    let (y, _h) = state.text_view.line_yrange(&iter);
    (y as f64).max(0.0).min(max)
}

/// Scroll so that a paragraph's first line lands at the cursor position
/// (~35% down the viewport). Used when playback crosses a paragraph boundary.
pub fn scroll_paragraph_to_top(state: &mut AppState, para_start: usize) {
    // F1: if a page-turn animation is in flight, drop this sync request.
    // The next CursorSync after release will pick up the new state.
    // Mirrors foliate Paginator.goTo's #locked early-return (paginator.js:1023).
    if state.page_turn_lock.is_locked() {
        crate::logging::log(&format!(
            "PARA_SCROLL: SKIP (page_turn_locked) para_start={} page_top={}",
            para_start, state.page_top_line
        ));
        return;
    }
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
            // Only page-turn if the paragraph start is off-screen AND ahead
            // of the current page. Never scroll backward to a paragraph that
            // started on a previous page — that would undo a user page turn.
            if para_start >= state.page_top_line
                && !super::viewport::is_line_on_screen(state, para_start)
            {
                crate::logging::log_always(&format!(
                    "SYNC_PARA_TURN: para_start={} page_top={}",
                    para_start, state.page_top_line
                ));
                set_page(state, para_start, PageDirection::Forward);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod page_turn_lock_tests {
    use super::PageTurnLock;

    #[test]
    fn try_acquire_succeeds_when_unlocked() {
        let lock = PageTurnLock::new();
        assert!(lock.try_acquire(), "first acquire should succeed");
        assert!(lock.is_locked(), "lock should be held after acquire");
    }

    #[test]
    fn try_acquire_fails_when_locked() {
        let lock = PageTurnLock::new();
        assert!(lock.try_acquire());
        assert!(!lock.try_acquire(), "second acquire should fail");
        assert!(lock.is_locked(), "lock should still be held after rejected acquire");
    }

    #[test]
    fn release_clears_the_lock() {
        let lock = PageTurnLock::new();
        lock.try_acquire();
        lock.release();
        assert!(!lock.is_locked(), "release should clear the lock");
        assert!(lock.try_acquire(), "acquire should succeed after release");
    }

    #[test]
    fn release_when_unlocked_is_a_noop() {
        let lock = PageTurnLock::new();
        lock.release();
        lock.release();
        assert!(!lock.is_locked());
        assert!(lock.try_acquire());
    }

    #[test]
    fn double_release_does_not_re_lock() {
        let lock = PageTurnLock::new();
        lock.try_acquire();
        lock.release();
        lock.release();
        assert!(!lock.is_locked());
    }
}
