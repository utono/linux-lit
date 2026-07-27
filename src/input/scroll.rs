use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;
use super::viewport::{
    visible_range, trim_visible_range, descender_guard_px, ascent_ink_gap_px, lines_per_page,
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

    // A whole-line page turn always lands line-aligned; reset any over-tall
    // sub-line offset (a forward step WITHIN an over-tall paragraph does NOT go
    // through set_page — it uses set_page_instant_offset).
    state.page_top_offset = 0;

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
    // Offset-aware: a prose row-fill page top can sit mid-paragraph; snapping
    // to the line's row 0 re-anchored the page above its stored boundary.
    snap_scroll_to_line_offset(state, state.page_top_line, state.page_top_offset);
}

/// Exclusive `exact_end` for a single-column PROSE page in table mode: the
/// stored page's end, so the clip stops where the PAGINATION says the page
/// stops (2026-07-27).
///
/// Without this the prose clip is purely GEOMETRIC — it covers from the last
/// visual row that fits `usable_height` to the card edge, never consulting the
/// stored page. That is right while a page ends because it ran out of room,
/// and WRONG when it ends early by rule: the chapter clamp stops a page before
/// a heading, and the viewport then painted straight past the boundary. The
/// two-column path never had this bug because it always passed the stored
/// split as `exact_end`; this is the single-column counterpart.
///
/// `None` off-table (live engine, or a non-canonical top) leaves the geometric
/// clip in charge. `prose_table_last_line_for_top` gives the INCLUSIVE last
/// rendered line; `exact_end` is exclusive downstream (the play path passes
/// `cs.split`, the first line of the next column), hence the `+ 1`.
pub(crate) fn prose_exact_end_for_current_page(state: &AppState) -> Option<usize> {
    if !(state.is_prose() && state.column_count() == 1) {
        return None;
    }
    crate::input::prose_pages::prose_table_last_line_for_top(
        state,
        state.page_top_line,
        state.page_top_offset,
    )
    .map(|last| last + 1)
}

/// Re-run `update_bottom_clip` against the current viewport state without
/// touching the scroll position. Use after font / line-height changes that
/// don't shift `page_top_line` — e.g. translation toggle, where the caller
/// has already restored a custom scroll value and a full `resnap_page`
/// would clobber it.
pub fn refresh_bottom_clip(state: &AppState) {
    let left_exact_end = if state.column_count() == 2 {
        Some(super::viewport::column_split(state, state.page_top_line).split)
    } else {
        prose_exact_end_for_current_page(state)
    };
    state.left_clip_boundary.set(left_exact_end);
    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        state.page_top_line,
        state.effective_line_count(),
        state.is_prose(),
        left_exact_end,
        state.section_starts().map(|s| s.to_vec()),
        state.one_section_per_page(),
        Some(state.current_line),
    );
}

/// Re-run ONE column's paged bottom clip against its STORED boundary, with the
/// cursor's current line. Called by `update_highlight` when the cursor crosses
/// a column's clip boundary: the descender allowance must collapse to 0 while
/// the boundary line carries the cursor-highlight `paragraph_background` band
/// (whose paint starts at the box top, above where glyph ink can), and reopen
/// when the cursor leaves. Reuses the stored boundary rather than re-running
/// `column_split` so a cursor move can never shift the split itself.
pub(crate) fn reschedule_left_clip_for_cursor(state: &AppState) {
    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        state.page_top_line,
        state.effective_line_count(),
        state.is_prose(),
        state.left_clip_boundary.get(),
        state.section_starts().map(|s| s.to_vec()),
        state.one_section_per_page(),
        Some(state.current_line),
    );
}

/// Right-column analog of `reschedule_left_clip_for_cursor`. The right view's
/// page top is the spread's split (= the stored LEFT boundary).
pub(crate) fn reschedule_right_clip_for_cursor(state: &AppState) {
    let (Some(split), Some(right_end)) = (
        state.left_clip_boundary.get(),
        state.right_clip_boundary.get(),
    ) else {
        return;
    };
    schedule_bottom_clip_update(
        state.right_view.clone(),
        state.right_bottom_clip.clone(),
        state.right_scrolled_window.clone(),
        split,
        state.effective_line_count(),
        state.is_prose(),
        Some(right_end),
        None,
        false,
        Some(state.current_line),
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
    exact_end: Option<usize>,
    section_starts: Option<Vec<bool>>,
    one_section_per_page: bool,
    cursor_line: Option<usize>,
) {
    let tv1 = text_view.clone();
    let bc1 = bottom_clip.clone();
    let sw1 = scrolled_window.clone();
    let ss1 = section_starts.clone();
    glib::idle_add_local_once(move || {
        update_bottom_clip(&tv1, &bc1, &sw1, page_top, line_count, is_prose, exact_end, ss1.as_deref(), one_section_per_page, cursor_line);
    });
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, page_top, line_count, is_prose, exact_end, section_starts.as_deref(), one_section_per_page, cursor_line);
    });
}

/// Set the page top line and scroll instantly (no animation). For gg/G/restore.
pub(crate) fn set_page_instant(state: &mut AppState, new_top: usize) {
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    state.page_top_offset = 0;
    state.prose_flash_hold.set(true);
    snap_scroll_to_line(state, new_top);
    log_first_paint(state, new_top);
}

/// Instant page set to `new_top` with a sub-line scroll `offset` (the over-tall
/// prose paragraph case — restoring a mid-paragraph position on `y`). With
/// `offset == 0` this is identical to `set_page_instant`.
pub(crate) fn set_page_instant_offset(state: &mut AppState, new_top: usize, offset: i32) {
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    state.page_top_offset = offset;
    state.prose_flash_hold.set(true);
    snap_scroll_to_line_offset(state, new_top, offset);
    refresh_bottom_clip(state);
    log_first_paint(state, new_top);
}

/// One-shot paint-latency probe: logs when GTK draws the first frame after an
/// instant page set, splitting "the jump felt slow" into handler time
/// (KEY→SEEK lines, ~ms) vs layout/paint time (this line). One tick callback
/// per page set — negligible cost, and the only visibility into perceived
/// page-jump latency on the live session.
fn log_first_paint(state: &AppState, new_top: usize) {
    let set_at = std::time::Instant::now();
    state.text_view.add_tick_callback(move |_, _| {
        crate::logging::log(&format!(
            "PAINT: first frame for page_top={} after {}ms",
            new_top,
            set_at.elapsed().as_millis()
        ));
        glib::ControlFlow::Break
    });
}

/// Show a dim "Next: Act N, Scene M" label centered in an EMPTY right column
/// (the scene ended in the left column), naming the act/scene that opens the
/// next canonical spread; hide it in every other case. `cs` is the spread's
/// `ColumnSplit` (already computed by the caller). The next scene's act/scene
/// is read from `cs.next_page_top`'s DB `(div1, div2)` — authoritative metadata,
/// never inferred from buffer text.
fn update_next_scene_watermark(state: &AppState, cs: &super::viewport::ColumnSplit) {
    // Anthology works have no act/scene structure (div1 is an excerpt index),
    // and they pack excerpts into both columns rather than ending the spread at
    // each section break — so there is no "empty right column awaiting the next
    // scene" to annotate. Never show the watermark for them.
    if state.is_anthology() {
        state.next_scene_watermark.set_visible(false);
        return;
    }
    let line_count = state.effective_line_count();
    // The right column is VISUALLY empty when the spread ends at a scene break:
    // `column_split` leaves the scene's trailing exit/blank lines in the right
    // range `[split, page_end]` (the bottom-clip then hides them) and sets
    // `next_page_top` to the next scene's marker. The strict `page_end < split`
    // test alone is insufficient — when a single trailing line sits at the split
    // the engine returns `page_end == split` (observed on H8 1.3: split=804
    // page_end=804 next=805), so the right range is one clipped tail line, NOT
    // `< split`. Detect the scene-break case authoritatively: `next_page_top` is
    // a DB section start (the next spread opens a new scene) AND the right range
    // carries no dialogue — only the tail. `page_end < split` is kept as the
    // sufficient condition for the strict lone-tail geometry. Both arms require
    // `next_page_top < line_count`, which also excludes END-OF-WORK (there
    // `next_page_top == line_count`) and the empty-LEFT first-spread mirror
    // (`split == 0`, where the right column is non-empty and has dialogue).
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let next_opens_scene =
        cs.next_page_top < line_count && state.is_section_start(cs.next_page_top);
    let right_has_dialogue = (cs.split..=cs.page_end.min(line_count.saturating_sub(1)))
        .any(|l| super::viewport::is_dialogue_line(&state.buffer, l, state.is_prose(), &stage_lookup));
    let empty_right = (cs.page_end < cs.split || (next_opens_scene && !right_has_dialogue))
        && cs.next_page_top < line_count;
    if !empty_right {
        state.next_scene_watermark.set_visible(false);
        return;
    }
    let (div1, div2) = crate::app::scene_synopsis::divs_at_buffer_line(state, cs.next_page_top);
    let label = crate::app::scene_synopsis::scene_label_for(state, div1, div2);
    // ~120% of the reading font, in Pango units (1pt = 1024). Scales with the
    // user's configured `font_size` instead of a fragile relative keyword.
    let size_units = ((state.config.font_size as f64) * 1.2 * 1024.0).round() as i64;
    let markup = format!(
        "<span foreground=\"{}\" style=\"italic\" size=\"{}\">next: {}</span>",
        state.theme.dim_fg,
        size_units,
        glib::markup_escape_text(&label),
    );
    state.next_scene_watermark.set_markup(&markup);
    state.next_scene_watermark.set_visible(true);
}

/// Scroll so `line` is at the top of the viewport, then size the bottom clip
/// overlay to hide any partially-visible line at the bottom of the page.
pub(crate) fn snap_scroll_to_line(state: &mut AppState, line: usize) {
    snap_scroll_to_line_offset(state, line, 0);
}

/// Like `snap_scroll_to_line`, but scrolls `offset` pixels PAST `line`'s pixel
/// top (the over-tall prose paragraph sub-line case). With `offset == 0` this is
/// identical to `snap_scroll_to_line`. When `offset > 0` the caller has already
/// snapped `offset` to a real visual-row top within the (over-tall) line, so the
/// line-clamp/back-up correction below is skipped — we are deliberately
/// positioned mid-line and the paragraph is tall enough to scroll there.
pub(crate) fn snap_scroll_to_line_offset(state: &mut AppState, line: usize, offset: i32) {
    let mut effective_top = line;

    if let Some(iter) = state.buffer.iter_at_line(line as i32) {
        let (y, _h) = state.text_view.line_yrange(&iter);
        let adj = state.scrolled_window.vadjustment();
        let max_value = (adj.upper() - adj.page_size()).max(0.0);
        let target = ((y + offset) as f64).min(max_value);
        adj.set_value(target);

        // The clamp/back-up correction below only applies to a line-aligned snap
        // (`offset == 0`). With `offset > 0` we are deliberately positioned
        // mid-line inside an over-tall paragraph; the caller already snapped the
        // offset to a real visual-row top, so do not drag page_top backward.
        if offset == 0 {
        // When the scroll was clamped, `line` can't appear at the viewport
        // top — earlier lines bleed in clipped. Walk backward to find the
        // line whose y <= the clamped scroll position and re-scroll to that
        // line's exact y so the page starts on a clean line boundary.
        //
        // EXCEPT one-section-per-page (sonnet_sequence): the last sonnet sits
        // near the buffer end and may not be scrollable to the very top, but
        // walking back here would drag page_top into the PREVIOUS sonnet's tail
        // (the visible bug). Instead, extend the bottom margin so the section's
        // heading CAN reach the top, then scroll to it; do not back up.
        if (y as f64) > max_value && state.one_section_per_page() {
            let page_size = adj.page_size();
            let needed_upper = y as f64 + page_size;
            let extra = (needed_upper - adj.upper()).ceil().max(0.0) as i32;
            if extra > 0 {
                let cur = state.text_view.bottom_margin();
                state.text_view.set_bottom_margin(cur + extra);
            }
            // Re-read upper after extending; set_value again now that the line
            // can reach the top. A deferred re-scroll backstops the case where
            // upper hasn't recomputed synchronously yet.
            let new_max = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value((y as f64).min(new_max));
            let sw = state.scrolled_window.clone();
            let tv = state.text_view.clone();
            let target_y = y as f64;
            glib::idle_add_local_once(move || {
                let a = sw.vadjustment();
                let m = (a.upper() - a.page_size()).max(0.0);
                a.set_value(target_y.min(m));
                let _ = &tv;
            });
        } else if (y as f64) > max_value && line > 0 {
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
        } // end `if offset == 0` (line-aligned snap correction)
    }

    // F4: populate the cache synchronously so MPV sync handlers reading
    // is_line_fully_visible right after this call see the new range, not
    // stale state. The idle-scheduled update_bottom_clip below ALSO writes
    // the cache as a backstop for layout-not-yet-flushed cases.
    let widget_height = state.text_view.height();
    if widget_height > 0 {
        let descender_guard = descender_guard_px(&state.text_view, effective_top);
        let usable_height = widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN;
        let line_count = state.effective_line_count();
        let range = visible_range(&state.text_view, &state.buffer, effective_top, line_count, usable_height);
        state.last_visible_range.set(Some(range));
    } else {
        state.last_visible_range.set(None);
    }

    // In two-column mode the left column must stop exactly where the right
    // column begins (`cs.split`), with no re-trimming — column_split already
    // chose that boundary to fill the left column. Pass it as exact_end so
    // update_bottom_clip clips there verbatim instead of running its own trim
    // (which would underfill the column) or its fill-guard (which would render
    // past the split and duplicate the right column's top lines).
    let two_col = state.column_count() == 2;
    let table = crate::input::page_table::active_page_table(state);
    // A top that is not a stored `left_start` used to fall straight through to
    // the live engine, pairing a table-chosen top with a live-chosen end and
    // over-filling the column (see `last_page_top`). Prefer the spread that
    // CONTAINS the top, so table mode stays on its own grid; only a top the
    // table does not cover at all falls back to the live split.
    let table_spread = table.as_ref().and_then(|t| {
        crate::input::page_table::spread_for_top(t, effective_top)
            .copied()
            .or_else(|| {
                crate::input::page_table::page_for_line(t, effective_top)
                    .and_then(|i| t.get(i).copied())
            })
    });
    let cs = if two_col {
        if let Some(s) = table_spread {
            // Synthesize the ColumnSplit the downstream code expects from the
            // stored spread — column_split() is NOT consulted in table mode.
            Some(super::viewport::ColumnSplit {
                split: s.split.unwrap_or(s.end + 1),
                page_end: s.end,
                next_page_top: s.end + 1,
            })
        } else {
            Some(super::viewport::column_split(state, effective_top))
        }
    } else {
        None
    };
    let line_count = state.effective_line_count();
    // Prose single-column clips at the stored page end too; see
    // `prose_exact_end_for_current_page`.
    let left_exact_end = cs
        .map(|c| c.split)
        .or_else(|| prose_exact_end_for_current_page(state));
    // EMPTY LEFT COLUMN (first-spread short opening section → right column):
    // `cs.split == 0` means the left view renders nothing. The deferred
    // update_bottom_clip below would only apply the full-height clip an idle
    // tick later, so the left view paints the opening section for one frame
    // before it is hidden — a visible flash. Clip the full height SYNCHRONOUSLY
    // now, before the first paint, so the section only ever appears on the right.
    if left_exact_end == Some(0) {
        let h = state.text_view.height();
        if h > 0 {
            state.bottom_clip.set_height_request(h);
        }
    }
    state.left_clip_boundary.set(left_exact_end);
    schedule_bottom_clip_update(
        state.text_view.clone(),
        state.bottom_clip.clone(),
        state.scrolled_window.clone(),
        effective_top,
        line_count,
        state.is_prose(),
        left_exact_end,
        state.section_starts().map(|s| s.to_vec()),
        state.one_section_per_page(),
        Some(state.current_line),
    );

    if let Some(cs) = cs {
        update_next_scene_watermark(state, &cs);
        // Make sure the right view has enough bottom headroom that a near-the-end
        // split line can actually be scrolled to its top (otherwise the scroll
        // clamps low and the right column duplicates the start of the buffer).
        ensure_scroll_range(state);
        // Scroll the right view so its top line is `cs.split`. Do it BOTH
        // synchronously (so the common case where layout is already settled is
        // correct on the first paint) AND on an idle + 100ms backstop: right
        // after a structural jump (gg/G/jump_to_end) or work load, the right
        // view's `upper` may still be stale, so a synchronous set_value clamps
        // to the wrong (often 0) value and the right column duplicates the left.
        // Re-running post-layout lands it on cs.split.
        let split = cs.split;
        let rv = state.right_view.clone();
        let rsw = state.right_scrolled_window.clone();
        let buf = state.buffer.clone();
        scroll_right_view_to_split(&rv, &rsw, &buf, split);
        {
            let (rv, rsw, buf) = (rv.clone(), rsw.clone(), buf.clone());
            glib::idle_add_local_once(move || scroll_right_view_to_split(&rv, &rsw, &buf, split));
        }
        glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
            scroll_right_view_to_split(&rv, &rsw, &buf, split)
        });

        // Clip the right column exactly at its page end — column_split already
        // chose cs.page_end (trimmed), so clip there verbatim rather than
        // letting update_bottom_clip re-trim with a possibly different result.
        let right_end = (cs.page_end + 1).min(line_count);
        state.right_clip_boundary.set(Some(right_end));
        schedule_bottom_clip_update(
            state.right_view.clone(),
            state.right_bottom_clip.clone(),
            state.right_scrolled_window.clone(),
            cs.split,
            line_count,
            state.is_prose(),
            Some(right_end),
            None, // exact_end set → trim path (and section clamp) never runs
            false, // two-column right view: exact_end drives the clip, not the section fill-guard
            Some(state.current_line),
        );
    } else {
        // Single-column (prose) or layout-not-ready: never show the two-column
        // watermark.
        state.right_clip_boundary.set(None);
        state.next_scene_watermark.set_visible(false);
    }
}

/// Scroll the right column's view so buffer line `split` sits at its top.
/// Clamped to the right view's scroll range; safe to call before or after
/// layout settles (a stale `upper` just clamps, and the post-layout re-call
/// corrects it).
fn scroll_right_view_to_split(
    right_view: &sourceview5::View,
    right_scrolled_window: &gtk4::ScrolledWindow,
    buffer: &sourceview5::Buffer,
    split: usize,
) {
    if let Some(iter) = buffer.iter_at_line(split as i32) {
        let (y, _h) = right_view.line_yrange(&iter);
        let radj = right_scrolled_window.vadjustment();
        let rmax = (radj.upper() - radj.page_size()).max(0.0);
        radj.set_value((y as f64).min(rmax));
    }
}

/// Ensure the vadjustment's upper bound is large enough that any buffer line
/// can be scrolled to the viewport top. GTK computes upper from content height
/// + margins; near the end of the document that may not be enough. We extend
/// upper (via bottom_margin) so the last line is always reachable.
///
/// The minimum bottom_margin needed is `page_size` — this guarantees that even
/// the very last buffer line can appear at the viewport top with whitespace
/// below. We only increase, never decrease, to avoid fighting GTK's layout.
pub(crate) const BASE_BOTTOM_MARGIN: i32 = 46;

/// Bottom reserve for the SINGLE-COLUMN paged FILL decision (and the matching
/// clip/visibility computations): `usable = widget_height - descender_guard -
/// SINGLE_COLUMN_BOTTOM_MARGIN`. Historically this was `BASE_BOTTOM_MARGIN`
/// (46, the scroll-headroom constant above), which reserved a near-line-height
/// band at the card bottom and left almost every single-column page one line
/// short of the two-column layout. Same empirical criterion as
/// `TWO_COLUMN_BOTTOM_MARGIN` below: the smallest reserve whose last line
/// keeps a clean descender at production geometry. Every single-column fill,
/// clip, and last-visible-line site must use this constant — a mismatch makes
/// the clip disagree with the fill (or the prose validator reject the grid).
pub(crate) const SINGLE_COLUMN_BOTTOM_MARGIN: i32 = 22;

/// Bottom reserve for a TWO-COLUMN paged column's FILL decision. Unlike the
/// single-column path (whose clip covers the full descender_guard +
/// SINGLE_COLUMN_BOTTOM_MARGIN band, see the single-col reserve in `update_bottom_clip`),
/// the two-column `exact_end` clip sums the actual line heights and reserves
/// only a descender allowance below the last line — so the fill only needs to
/// reserve that much. Reserving the full 40px `BASE_BOTTOM_MARGIN` wastes ~1
/// line per column. Value chosen empirically: the smallest reserve whose
/// last-column line keeps a clean descender at production geometry (see
/// docs/superpowers/plans/2026-07-15-two-column-fill-reserve.md). If a column's
/// last line slices its descenders, raise this; if columns underfill, it drifted
/// back toward 40. The two-column fill sites AND `validate_spreads` must both use
/// this constant or generation falls back to no-table.
pub(crate) const TWO_COLUMN_BOTTOM_MARGIN: i32 = 22;

/// The pixel reserve the two-column fill leaves BELOW the last possible line:
/// `descender_guard + TWO_COLUMN_BOTTOM_MARGIN` — exactly the band
/// `usable = card_h - guard - TWO_COLUMN_BOTTOM_MARGIN` withholds. The
/// `.column-divider` bottom margin is set to this so the divider ends at the
/// last-possible-line baseline, identically on every page. Reads the guard from
/// the live view (same font source as the fill) so it tracks font changes.
pub(crate) fn two_column_divider_bottom_px(text_view: &sourceview5::View) -> i32 {
    crate::input::viewport::descender_guard_px(text_view, 0) + TWO_COLUMN_BOTTOM_MARGIN
}

/// Bottom-clip height for a paged column whose content is a sum of
/// `line_yrange` LOGICAL line heights (`total`): `space` minus `total`, minus a
/// small `allowance` so the clip's top edge sits BELOW the last line's glyph
/// descenders (which render flush to the logical line bottom) instead of
/// slicing them. Floored at 0. Pure so the arithmetic is unit-tested without
/// GTK geometry.
pub(crate) fn paged_bottom_clip(space: i32, total: i32, allowance: i32) -> i32 {
    (space - total - allowance).max(0)
}

/// How far the paged clip's top edge may drop below the last line's logical
/// bottom: the font's descent (`guard`, what the descenders need), capped at
/// the blank strip above the next line's ink (`blank_budget`, see
/// `boundary_blank_budget`). The shared buffer renders the NEXT line
/// immediately below the logical bottom (hidden by the clip), and its ink
/// cannot start above `box_top + blank_budget` — so a reveal within that
/// budget shows only descender overhang and blank space, never the next
/// line's ascenders. Degrades to 0 (the pre-fix clip) when the budget is 0.
pub(crate) fn descender_allowance(guard: i32, blank_budget: i32) -> i32 {
    guard.min(blank_budget.max(0))
}

/// The guaranteed-blank strip at the top of `boundary_line`'s box (the first
/// line hidden below a paged bottom clip): its `pixels_above_lines` spacing
/// (the view's, unless one of the line's tags overrides it — verse uses 0)
/// plus the font's ascent internal leading (`ascent_ink_gap_px`) scaled by the
/// line's smallest tag scale (a 0.75-scale speaker label has a
/// proportionally smaller leading — probing only the base font over-revealed
/// by 1px and the line_clipping e2e caught the sliver). A whitespace-only
/// boundary line has no ink of its own, but its 0.25-scale box is only a few
/// px tall — the budget is capped at that box height so the reveal never
/// punches through it into the following line's ascenders (the e2e caught
/// exactly that 1px at a G-jump page ending on a blank line).
fn boundary_blank_budget(
    text_view: &sourceview5::View,
    buf_sv: &sourceview5::Buffer,
    boundary_line: usize,
) -> i32 {
    let Some(start) = buf_sv.iter_at_line(boundary_line as i32) else {
        // Past the buffer end: nothing is rendered below, reveal freely.
        return i32::MAX;
    };
    let mut end_it = start;
    if !end_it.ends_line() {
        end_it.forward_to_line_end();
    }
    if buf_sv.text(&start, &end_it, false).trim().is_empty() {
        let (_y, h) = text_view.line_yrange(&start);
        return h.max(0);
    }
    let tags = start.tags();
    let above = tags
        .iter()
        .filter(|t| t.is_pixels_above_lines_set())
        .map(|t| t.pixels_above_lines())
        .min()
        .unwrap_or_else(|| text_view.pixels_above_lines());
    let scale = tags
        .iter()
        .filter(|t| t.is_scale_set())
        .map(|t| t.scale())
        .fold(1.0_f64, f64::min);
    above.max(0) + ((ascent_ink_gap_px(text_view) as f64) * scale).floor() as i32
}

/// Final bottom-clip height for the over-tall single-paragraph case in
/// `update_bottom_clip`: the per-visual-row clip from `bottom_clip_height`
/// (computed against a `usable_height`-tall viewport) plus the reserved bottom
/// band `widget_height - usable_height` (= `descender_guard + SINGLE_COLUMN_BOTTOM_MARGIN`)
/// that the rest of the pagination keeps off a full page. Clamped to
/// `[0, widget_height]` so the clip never goes negative or taller than the card.
/// Pure so the arithmetic is unit-tested without GTK geometry.
pub(crate) fn overtall_clip(row_clip: i32, widget_height: i32, usable_height: i32) -> i32 {
    let reserve = (widget_height - usable_height).max(0);
    (row_clip + reserve).clamp(0, widget_height)
}
pub(crate) fn ensure_scroll_range(state: &AppState) {
    let page_size = state.scrolled_window.vadjustment().page_size();
    if page_size <= 0.0 {
        return;
    }
    let needed = (page_size as i32).max(BASE_BOTTOM_MARGIN);
    if state.text_view.bottom_margin() < needed {
        state.text_view.set_bottom_margin(needed);
    }
    // Two-column mode: the right view needs the same headroom so a near-the-end
    // split line (e.g. EPILOGUE on the final spread) can be scrolled to its top.
    // Without this the right view's `upper` is too small, set_value clamps to a
    // low value, and the right column shows the start of the buffer (duplicating
    // the left column) instead of cs.split.
    if state.column_count() == 2 {
        let rpage = state.right_scrolled_window.vadjustment().page_size();
        if rpage > 0.0 {
            let rneeded = (rpage as i32).max(BASE_BOTTOM_MARGIN);
            if state.right_view.bottom_margin() < rneeded {
                state.right_view.set_bottom_margin(rneeded);
            }
        }
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
    section_starts: Option<&[bool]>,
    one_section_per_page: bool,
    cursor_line: Option<usize>,
) {
    update_bottom_clip(text_view, bottom_clip, scrolled_window, page_top, line_count, is_prose, None, section_starts, one_section_per_page, cursor_line);
}

/// Set the bottom clip to hide everything below the last fully-visible line.
/// Uses buffer line_yrange (absolute coords) to sum heights from page_top,
/// avoiding buffer_to_window_coords which may be stale after scroll_to_iter.
/// `exact_end`, when `Some(end)`, clips so the last visible line is `end - 1`
/// with NO trimming applied. Used for the left column of a two-column spread,
/// where `column_split` already decided the boundary (`cs.split`); re-running
/// the trim here would undo column_split's generous left-column fill and leave
/// the column badly underfilled.
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
    is_prose: bool,
    exact_end: Option<usize>,
    section_starts: Option<&[bool]>,
    one_section_per_page: bool,
    cursor_line: Option<usize>,
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
            widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN
        }
    };

    // Two-column column with a pre-decided boundary: clip exactly at
    // `end - 1` with no trimming and no descender-guard reduction.
    // column_split already chose this boundary (and verified it fits in
    // `widget_height - guard - margin`), so re-running visible_range here with
    // the guard-reduced usable_height would fit fewer lines and underfill.
    // Sum the real heights of [page_top, end-1] directly.
    if let Some(end) = exact_end {
        // An EMPTY column: column_split chose `split == 0` for the first spread
        // (short opening section moved to the RIGHT column), so the LEFT view
        // renders nothing. `end <= page_top` means no line belongs here — clip
        // the whole height. Without this, the `.max(page_top)` below would force
        // line `page_top` to render and duplicate the right column's first line.
        if end <= page_top {
            if bottom_clip.height_request() != widget_height {
                bottom_clip.set_height_request(widget_height);
            }
            return;
        }
        let last = end.saturating_sub(1).max(page_top);
        let mut total = 0i32;
        for i in page_top..=last {
            if let Some(iter) = buf_sv.iter_at_line(i as i32) {
                let (_y, h) = text_view.line_yrange(&iter);
                total += h;
            }
        }
        // Drop the clip's top edge below the last line's logical bottom so its
        // flush descender ink (g/y/p/comma tails) isn't sliced — capped at the
        // strip guaranteed free of the NEXT line's ink (the shared buffer
        // renders it right below the split; see `boundary_blank_budget`).
        // EXCEPT while the boundary line carries the cursor highlight: its
        // paragraph_background band paints from the box TOP (above where ink
        // can start), so any reveal would show the band's edge as a colored
        // sliver. update_highlight re-schedules this clip when the cursor
        // crosses the boundary.
        let allowance = if cursor_line == Some(end) {
            0
        } else {
            descender_allowance(descender_guard, boundary_blank_budget(text_view, &buf_sv, end))
        };
        let clip = paged_bottom_clip(widget_height, total, allowance);
        if bottom_clip.height_request() != clip {
            crate::logging::log(&format!(
                "BOTTOM_CLIP_EXACT: widget_h={} total={} allowance={} clip={} page_top={} end={}",
                widget_height, total, allowance, clip, page_top, end
            ));
            bottom_clip.set_height_request(clip);
        }
        // Clip tripwire (debug only): the left column [page_top, end-1] measured
        // TALLER than the card, so paged_bottom_clip floored to 0 and the
        // overflowing last line pokes out with nothing to mask it — the stale
        // play_pages / over-tall-split overflow (clip-prevention.md #12). Its own
        // decisive tell is `total > widget_h` with `clip=0`.
        if crate::logging::debug_mode() && widget_height > 0 && total > widget_height {
            crate::log_fmt!(
                "CLIP_WARN: main-card two-col OVERFLOW total={} > widget_h={} clip={} page_top={} end={} (clip-prevention.md #12)",
                total, widget_height, clip, page_top, end
            );
        }
        return;
    }

    let range = visible_range(text_view, &buf_sv, page_top, line_count, usable_height);

    // Single-column PROSE pages fill by visual row and may start AND end
    // mid-paragraph on ANY line (design: full row-fill). Route every such page
    // through the per-visual-row clip path — not just the over-tall
    // (`range.count == 0`) case — because the engine guarantees each prose page
    // top is a snapped visual-row top and each page bottom is the snap of
    // `top + usable`, so the row-based clip (last fully-visible display row at
    // the live scroll value, then the reserve band) is exactly correct for ALL
    // prose pages: over-tall, mid-paragraph, or clean line top. `exact_end` is
    // never set for single-column paths (only the two-column columns pass it),
    // so it discriminates cleanly. Non-prose and two-column pages keep the
    // whole-line path below byte-for-byte.
    let prose_single_col = is_prose && exact_end.is_none();

    if prose_single_col || range.count == 0 || range.total_height == 0 {
        // For non-prose, this branch still only triggers on an over-tall line
        // (`range.count == 0`): a single buffer line at page_top TALLER than
        // usable_height, so visible_range (which counts whole BUFFER lines) fit
        // nothing. Previously this clipped to 0, letting the over-tall paragraph
        // render flush to the card's bottom edge — exposed once the narrower
        // prose column made long paragraphs wrap past the viewport height.
        //
        // Clip at a clean VISUAL-ROW boundary instead: the paragraph's wrapped
        // rows are normal-height, so cover from the bottom of the last full row
        // that fits within `usable_height` down to the card edge. This routes
        // through the shared descender-correct `display_rows`/`bottom_clip_height`
        // helpers — the SAME per-visual-row mechanism scroll-mode uses on the main
        // card (sanctioned in clip-prevention.md), so it never cuts through a glyph
        // row the way `line_yrange` (whole-paragraph) would. Paging forward
        // continues the paragraph from the next row.
        //
        // bottom_clip_height clips relative to a `usable_height`-tall viewport
        // anchored at the scroll value, returning the gap from the last full row's
        // bottom up to `scroll_val + usable_height`. The REAL viewport is
        // `widget_height`, so add the reserved bottom band
        // (`widget_height - usable_height` = descender_guard + SINGLE_COLUMN_BOTTOM_MARGIN)
        // the clip must also cover, giving the same bottom gap a normal full page
        // keeps.
        // Rows are seeked around the LIVE viewport (not walked from the
        // document start): `display_rows` caps at 8192 visual rows, so past
        // ~8192 rows into a long prose work it returned an empty window and a
        // truncated content height, and the clip covered the ENTIRE card —
        // every page near the end of TrollopeBBC rendered blank (`G`, then
        // each `y`, 2026-07-10). `content_ink_height` reads the true document
        // bottom straight from the last line's yrange, so the doc-end guard in
        // `bottom_clip_height` only fires on the real last page.
        let tv: &gtk4::TextView = text_view.upcast_ref();
        let scroll_val_now = scrolled_window.vadjustment().value();
        let rows = crate::ui::display_rows_window(tv, scroll_val_now, usable_height as f64);
        let content_h = crate::ui::content_ink_height(tv);
        let row_clip = crate::ui::bottom_clip_height(
            &rows,
            scroll_val_now,
            usable_height as f64,
            content_h,
        );
        let clip = overtall_clip(row_clip, widget_height, usable_height);
        let reserve = (widget_height - usable_height).max(0);
        if bottom_clip.height_request() != clip {
            bottom_clip.set_height_request(clip);
        }
        crate::logging::log(&format!(
            "BOTTOM_CLIP_ROWFILL: prose_single_col={} page_top={} scroll_val={:.1} usable={} widget_h={} row_clip={} reserve={} -> clip={}",
            prose_single_col, page_top, scroll_val_now, usable_height, widget_height, row_clip, reserve, clip
        ));
        return;
    }

    let display_range = {
        let bf = section_starts.map(super::viewport::section_break_fn);
        let is_break = bf.as_ref().map(|f| f as &dyn Fn(usize) -> bool);
        let trimmed = trim_visible_range(range, page_top, text_view, &buf_sv, is_prose, is_break);

        // One-section-per-page works (sonnet_sequence): the section-break clamp is
        // the intended page boundary even when the section is short (a 14-line
        // sonnet fills well under 85% of the viewport). Skip the fill-guard so the
        // page stops at the end of this sonnet rather than packing the next ones.
        if one_section_per_page {
            trimmed
        }
        // Viewport fill guard for display: if trimming left the page less than
        // ~85% full, the dangling-speaker/block trims created too much empty
        // space. Fall back to the raw visible range which only clips the partial
        // bottom line. A dangling speaker at the bottom looks better than 15%+
        // empty space.
        else if widget_height > 0
            && trimmed.last_fit < range.last_fit
            && trimmed.total_height * 20 < widget_height * 17
        {
            range
        } else {
            trimmed
        }
    };

    let scroll_val = scrolled_window.vadjustment().value();
    let expected_y = if let Some(iter) = buf_sv.iter_at_line(page_top as i32) {
        let (y, _h) = text_view.line_yrange(&iter);
        y as f64
    } else {
        -1.0
    };
    // When the viewport is scrolled PAST page_top's pixel top (offset > 0) —
    // which happens during line-by-line navigation in translation mode, where
    // the scroll tracks the cursor rather than snapping to a page boundary —
    // the displayed content sits `offset` higher on screen, exposing `offset`
    // more space (and a partial line) at the bottom. The clip box (valign=End)
    // must grow by that offset to keep covering the partial bottom line. In
    // normal page-turn mode the scroll is snapped to page_top so offset == 0
    // and this is a no-op.
    let scroll_offset = if expected_y >= 0.0 {
        (scroll_val - expected_y).max(0.0)
    } else {
        0.0
    };
    // Same flush-descender allowance (and cursor-on-boundary gate) as the
    // exact_end branch: the clip's top edge otherwise lands at the last line's
    // logical bottom and slices its descender ink.
    let boundary = display_range.last_fit + 1;
    let allowance = if cursor_line == Some(boundary) {
        0
    } else {
        descender_allowance(descender_guard, boundary_blank_budget(text_view, &buf_sv, boundary))
    };
    let clip = paged_bottom_clip(
        widget_height + scroll_offset.round() as i32,
        display_range.total_height,
        allowance,
    );
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
    // Clip tripwire (debug only): a clip as tall as the card blanks it — the
    // "whole card goes BLANK after a cursor-scrolled reveal" failure, where a
    // paged clip ran against a scroll value far from page_top so `scroll_offset`
    // inflated the clip past the viewport (clip-prevention.md #7).
    if crate::logging::debug_mode() && widget_height > 0 && clip >= widget_height {
        crate::log_fmt!(
            "CLIP_WARN: main-card single-col clip={} >= widget_h={} (card would blank; offset={:.0}; clip-prevention.md #7)",
            clip, widget_height, scroll_offset
        );
    }
    bottom_clip.set_height_request(clip);
}

/// Headless UI test harness support: when `LIT_HEADLESS_TEST` is set, log the
/// reading viewport's rectangle in toplevel-window coordinates (which equal
/// screenshot pixels under the cage fullscreen output). The harness greps this
/// line and passes it to `check_line_clipping.py --region`, because linux-lit's
/// `sourceview5::View` exposes no AT-SPI Text interface for auto-detection.
///
/// Format (stable, parsed by tests/harness): `TEST_VIEWPORT_RECT x y w h`.
pub(crate) fn emit_test_viewport_rect(state: &AppState) {
    if std::env::var_os("LIT_HEADLESS_TEST").is_none() {
        return;
    }
    // Bounds of the scrolled text viewport in the window's coordinate space.
    if let Some(r) = state.scrolled_window.compute_bounds(&state.window) {
        crate::logging::log_viewport_rect("TEST_VIEWPORT_RECT", &r);
    } else {
        crate::logging::log("TEST_VIEWPORT_RECT unavailable (compute_bounds returned None)");
    }
    // Also report the FIRST spread's column split so tests can assert the
    // short-opening-section-to-right-column rule (split=0 → empty left column).
    // Format (stable, parsed by tests): `FIRST_SPREAD_SPLIT split=S page_end=E next=N`.
    if state.column_count() == 2 {
        let cs = super::viewport::column_split(state, 0);
        log_fmt!(
            "FIRST_SPREAD_SPLIT split={} page_end={} next={}",
            cs.split,
            cs.page_end,
            cs.next_page_top
        );
    }
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
/// Whether a dialogue/segment jump may TURN THE PAGE (2026-07-27).
///
/// User rule: the dialogue/segment binds (`;` `'` `,` `q` `j` `k`) never
/// effect a page turn — on any work type. They move the cursor within what is
/// already on screen; crossing to another page is the job of the explicit
/// page binds (`x` `y` `[` `{`). When one of these binds would need a turn,
/// the caller holds the cursor where it was instead.
///
/// Returns true when the cursor's new line is already visible, i.e. the jump
/// needs no turn. Mirrors the `already_visible` test the scroll helpers use,
/// including the prose `is_line_start_visible` exception (an over-tall
/// paragraph whose opening row IS on screen must not read as "not visible").
pub(crate) fn jump_stays_on_page(state: &AppState) -> bool {
    if state.config.navigation_mode != crate::config::NavigationMode::EReader {
        // Scroll mode centers the cursor; there is no page to leave.
        return true;
    }
    if state.translations_visible {
        // The translation overlay scrolls by scrolloff, never by page.
        return true;
    }
    if state.is_prose() && state.column_count() == 1 {
        super::viewport::is_line_start_visible(state, state.current_line)
    } else {
        super::viewport::is_line_fully_visible(state, state.current_line)
    }
}

pub(crate) fn scroll_after_jump_forward(state: &mut AppState, _prev_line: usize) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            // If the cursor's new line is ALREADY on the current spread (e.g. it
            // moved into the right column), do nothing — the caller updated the
            // highlight; no re-page needed.
            //
            // Prose uses `is_line_start_visible`, not `is_line_fully_visible`:
            // the latter charges a buffer line its FULL wrapped height, so an
            // over-tall paragraph starting exactly at page_top_line reads as
            // "not visible" even though its opening row is on screen — that
            // false negative is what caused the original bug (a redundant page
            // turn skipped the paragraph's own first page). Play/table paths
            // keep the original whole-line check.
            let already_visible = if state.is_prose() && state.column_count() == 1 {
                super::viewport::is_line_start_visible(state, state.current_line)
            } else {
                super::viewport::is_line_fully_visible(state, state.current_line)
            };
            if already_visible {
                return;
            }
            // Prose visual-row fill: advance one boundary at a time (the same
            // rule as x — Task 3's prose_next_boundary) until the cursor
            // line's page is reached. This is what fixes the j-past-an-
            // over-tall-paragraph tail skip: the intermediate sub-line pages
            // are stepped through, never jumped over.
            //
            // Stop condition is `page_top_line >= target`, NOT
            // `is_line_start_visible(target)`: for an over-tall paragraph,
            // `prose_next_boundary` steps INTO its middle (offset > 0) before
            // ever landing exactly on its start — `is_line_start_visible`
            // requires offset == 0 at `line == page_top_line`, so it would
            // never go true once the walk is already past the paragraph's
            // fresh start, and the loop would run away through the rest of
            // the document (observed: guard-bounded runaway to page_top≈1500
            // in a document of ~2000 lines). Landing with `page_top_line ==
            // target` (any offset) already means `target` is showing on this
            // page — that IS "reached," regardless of whether it started
            // fresh or mid-paragraph.
            if state.is_prose() && state.column_count() == 1 {
                let target = state.current_line;
                // Table-authoritative: if a prose table is active, land directly
                // on the stored page containing `target` (offset-aware). Never
                // re-walk the live boundaries while a grid is pinned — the
                // established play-table lesson (table_end_for_top): visibility
                // and landing checks must read the SAME source that renders.
                if let Some((pt, po)) =
                    crate::input::prose_pages::prose_table_boundary_for_line(state, target)
                {
                    let from = (state.page_top_line, state.page_top_offset);
                    if (pt, po) != from {
                        state.page_back_stack.clear();
                        state.page_back_stack.push(from);
                        log_fmt!(
                            "PAGES_PROSE: jump-fwd table current={} ({},{})->({},{})",
                            target, from.0, from.1, pt, po
                        );
                        set_page_instant_offset(state, pt, po);
                    }
                    return;
                }
                let mut guard = 0usize;
                let from = (state.page_top_line, state.page_top_offset);
                while state.page_top_line < target {
                    let Some((nt, no)) = super::navigation::prose_next_boundary(state) else {
                        break;
                    };
                    // Advance the live page state directly; one back entry
                    // for the whole jump (matches the single-entry rule below).
                    state.page_top_line = nt;
                    state.page_top_offset = no;
                    guard += 1;
                    if guard > state.effective_line_count().max(64) {
                        break; // safety: never loop forever
                    }
                }
                if (state.page_top_line, state.page_top_offset) != from {
                    let (nt, no) = (state.page_top_line, state.page_top_offset);
                    // Restore pre-jump state for the back stack, then land.
                    state.page_top_line = from.0;
                    state.page_top_offset = from.1;
                    state.page_back_stack.clear();
                    state.page_back_stack.push(from);
                    log_fmt!(
                        "NAV_PAGE_FWD: prose row-fill current={} ({},{})->({},{})",
                        state.current_line, from.0, from.1, nt, no
                    );
                    set_page_instant_offset(state, nt, no);
                }
                return;
            }
            // Compute where a turn/snap would land. Table mode: the stored page
            // containing the new cursor line IS the landing page — same grid as
            // x/y and playback sync (see update_highlight_and_advance_page).
            let new_top = if let Some(t) =
                crate::input::page_table::table_top_for(state, state.current_line)
            {
                t
            } else if !state.is_prose()
                && super::viewport::is_first_dialogue_of_scene_state(
                    state, &state.translation_lines, state.current_line,
                )
            {
                super::viewport::back_up_for_speaker_state(state, state.current_line)
            } else {
                super::viewport::page_turn_top_state(state, state.current_line)
            };
            // If turning to `new_top` lands on the work's FINAL spread region,
            // redirect to the canonical final spread so the tail fills BOTH columns
            // (the EPILOGUE in the right column) — covers both the empty-right
            // lone-EPILOGUE case and the underfilled too-early final spread. Shared
            // with x (page_forward) and playback sync; see
            // `navigation::redirect_to_final_spread`.
            let new_top = match super::navigation::redirect_to_final_spread(state, new_top) {
                Some(anchor) => {
                    log_fmt!("NAV_FWD_LASTPAGE: current={} page_top={} new_top={} -> anchor={}",
                             state.current_line, state.page_top_line, new_top, anchor);
                    anchor
                }
                None => new_top,
            };
            if new_top != state.page_top_line {
                // A dialogue-nav forward step turned the page. Record the page we
                // came from so `y` returns to it (a single return entry, like a
                // structural jump) — otherwise `y` pops a STALE older entry and
                // jumps somewhere unrelated.
                state.page_back_stack.clear();
                state.page_back_stack.push((state.page_top_line, state.page_top_offset));
                log_fmt!("NAV_PAGE_FWD: current={} old_top={} new_top={}", state.current_line, state.page_top_line, new_top);
                set_page_instant(state, new_top);
            } else {
                // No page turn (the "next page" anchors back to the current top —
                // the work's short tail can't scroll further). The cursor was
                // advanced to a line that is NOT visible on this clamped final
                // spread, which would leave NO highlight on screen. Cap it at the
                // last fully-visible dialogue line so a line is always highlighted.
                let visible_end = super::viewport::last_fully_visible_line(state, state.page_top_line);
                if state.current_line > visible_end {
                    let capped = {
                        let stage_lookup = |bi: usize| -> Option<i64> {
                            state.work_line_for_buffer(bi)
                                .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                                .map(|l| l.sub_line)
                        };
                        super::viewport::prev_dialogue_line(
                            &state.buffer, &state.translation_lines, visible_end + 1, state.is_prose(), &stage_lookup,
                        )
                        .filter(|&d| d >= state.page_top_line && d <= visible_end)
                        .unwrap_or(visible_end)
                    };
                    log_fmt!("NAV_FWD_LASTPAGE: cursor {} off-screen, capped to last visible {}",
                             state.current_line, capped);
                    state.current_line = capped;
                }
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
            // Already on the current spread (cursor moved up within it)? Nothing
            // to do — just the highlight moved. (Per-dialogue backward nav does
            // NOT scene-snap; that is the `2`/`3` jump's job.)
            if super::viewport::is_line_fully_visible(state, state.current_line) {
                return;
            }
            // Prose table mode is TABLE-AUTHORITATIVE — mirror of the jump-fwd
            // branch above: land on the stored page (offset-aware) containing
            // the target paragraph's FIRST row. This also covers the
            // mid-paragraph page top (`current_line == page_top_line` with
            // `page_top_offset > 0`): the `,` target's tail renders at the top
            // of this page, so the strict `current_line < page_top_line` check
            // below never fires and the paragraph's start stayed off-screen —
            // `,` after a `q` page turn looked like a no-op and the seek
            // anchored to the paragraph's last visible phrase.
            if state.is_prose() && state.column_count() == 1 {
                if let Some((pt, po)) = crate::input::prose_pages::prose_table_boundary_for_line(
                    state, state.current_line,
                ) {
                    let from = (state.page_top_line, state.page_top_offset);
                    if (pt, po) != from {
                        state.page_back_stack.clear();
                        state.page_back_stack.push(from);
                        log_fmt!(
                            "PAGES_PROSE: jump-bwd table current={} ({},{})->({},{})",
                            state.current_line, from.0, from.1, pt, po
                        );
                        set_page_instant_offset(state, pt, po);
                    }
                    return;
                }
            }
            if state.current_line < state.page_top_line {
                // The cursor stepped above the current page top (it was on the
                // left column's top line). Turn to the PREVIOUS full page and put
                // the highlight on the BOTTOM of that page's right column — the
                // previous dialogue line in reading order, which is what a reader
                // expects from `,`/`k` at the page top.
                // Walk back one spread at a time until we reach the page that
                // actually CONTAINS the target dialogue (`state.current_line`,
                // already set to the correct prev dialogue by the caller). A single
                // `prev_page_top` step can land on a legitimately dialogue-less
                // spread that doesn't hold the target — e.g. H8 1.4's scene-header
                // spread `[1701,1703]` ('Scene 4'/'====='/blank fill the whole
                // page at a short viewport, its dialogue pushed to the next spread).
                // Anchoring there leaves the cursor off-page and the bottom-right
                // fallback below would snap it onto the blank. Skip past such
                // spreads to the one that shows the cursor.
                let old_top = state.page_top_line;
                let target = state.current_line;
                // Table mode: the stored page containing the target dialogue IS
                // the landing page (same grid as x/y); the live walk below can
                // land off the canonical grid.
                if let Some(t) = crate::input::page_table::table_top_for(state, target) {
                    state.page_back_stack.clear();
                    state.page_back_stack.push((old_top, 0));
                    log_fmt!("NAV_PAGE_BACK: table new_top={} old_top={} cursor={}",
                             t, old_top, target);
                    set_page_instant(state, t);
                    return;
                }
                let mut new_top = super::viewport::prev_page_top(state, old_top).new_top;
                let mut guard = 0;
                // Keep walking back while this spread starts AFTER the target (so it
                // can't contain it): a single prev_page_top can land on a
                // legitimately dialogue-less spread whose page_top is still past the
                // target (H8 1.4: target=1697 in scene 3, but prev_page_top(1704)
                // lands on the scene-4 header spread at 1701 > 1697). Step back until
                // page_top <= target, i.e. the spread that actually holds it.
                while new_top > target {
                    let prev = super::viewport::prev_page_top(state, new_top).new_top;
                    if prev >= new_top {
                        break;
                    }
                    new_top = prev;
                    guard += 1;
                    if guard > 64 {
                        break;
                    }
                }
                // Record the page we came from (single return entry) so a later
                // `x`/`y` round-trips, and so `y` never pops a stale older entry.
                state.page_back_stack.clear();
                // center-cursor lands line-aligned, so the return entry has offset 0.
                state.page_back_stack.push((old_top, 0));
                set_page_instant(state, new_top);
                // If the target now sits on this spread, keep it (it is a real
                // dialogue line). Otherwise fall back to the spread's bottom-right
                // dialogue line, backing up off any trailing non-dialogue.
                let cursor = if super::viewport::is_line_fully_visible(state, target) {
                    target
                } else {
                    let last_vis = super::viewport::last_fully_visible_line(state, new_top);
                    let stage_lookup = |bi: usize| -> Option<i64> {
                        state.work_line_for_buffer(bi)
                            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                            .map(|l| l.sub_line)
                    };
                    super::viewport::prev_dialogue_line(
                        &state.buffer, &state.translation_lines, last_vis + 1, state.is_prose(), &stage_lookup,
                    )
                    .filter(|&d| d >= new_top)
                    .unwrap_or(last_vis)
                };
                log_fmt!("NAV_PAGE_BACK: new_top={} old_top={} cursor {}->{} (bottom-right)",
                         new_top, old_top, state.current_line, cursor);
                state.current_line = cursor;
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

/// Keep the current line visible with a vim-style `scrolloff` margin: the
/// cursor never sits within `SCROLLOFF` lines of the top or bottom edge unless
/// at a document boundary. Scrolls the minimum needed; if the cursor is already
/// within the comfortable band, does nothing. Used for j/k in the translation
/// view, where the highlight follows the cursor line.
pub(crate) fn scroll_cursor_into_view_scrolloff(state: &mut AppState) {
    const SCROLLOFF: i32 = 3;
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = (adj.upper() - adj.page_size()).max(0.0);
    if max_scroll <= 0.0 {
        return;
    }
    let Some(iter) = state.buffer.iter_at_line(state.current_line as i32) else { return };
    let (y, h) = state.text_view.line_yrange(&iter);
    let line_h = (h as f64).max(1.0);
    let margin = SCROLLOFF as f64 * line_h;
    let top = adj.value();
    let bottom = top + adj.page_size();
    let cursor_top = y as f64;
    let cursor_bottom = (y + h) as f64;
    let raw_val = if cursor_top - margin < top {
        // Too close to the top edge — scroll up so the margin is restored.
        (cursor_top - margin).max(0.0)
    } else if cursor_bottom + margin > bottom {
        // Too close to the bottom edge — scroll down.
        (cursor_bottom + margin - adj.page_size()).min(max_scroll)
    } else {
        return; // already within the comfortable band
    };
    // Snap the target to a whole-line top so the viewport never lands between
    // line boundaries — otherwise (notably in the translation view, where this
    // scrolloff path is used instead of page turns) the top and bottom lines
    // render half-clipped against the card edges.
    let new_val = snap_value_to_line_top(state, raw_val.clamp(0.0, max_scroll))
        .clamp(0.0, max_scroll);
    adj.set_value(new_val);
    // Keep page_top_line consistent with the new scroll position.
    if let Some(top_line) = line_at_value(state, new_val) {
        state.page_top_line = top_line;
    }
    // Cover the partial line straddling the bottom edge. The main card's paged
    // update_bottom_clip is page_top-relative and assumes a snapped page; the
    // translation view scrolls continuously, so compute the clip from the ACTUAL
    // viewport like the gloss overlay does: find the last whole line fully above
    // the viewport bottom and clip from its bottom down.
    update_scrolloff_bottom_clip(state, new_val);
}

/// Scroll-position-aware bottom clip for the continuously-scrolling translation
/// view. `top_val` is the current vadjustment value. Sets `state.bottom_clip`
/// to cover from the bottom of the last fully-visible line to the viewport
/// bottom, so a partial line never shows clipped against the card's lower edge.
fn update_scrolloff_bottom_clip(state: &AppState, top_val: f64) {
    scrolloff_bottom_clip_widgets(
        &state.text_view, &state.scrolled_window, &state.bottom_clip, top_val,
    );
}

/// Widget-level core of `update_scrolloff_bottom_clip`, callable from an idle
/// closure (e.g. the translation toggle reveal) that holds the widgets but not
/// `AppState`. Computes the clip purely from the current scroll position so the
/// bottom partial line is covered immediately on toggle, before any j/k.
pub(crate) fn scrolloff_bottom_clip_widgets(
    text_view: &sourceview5::View,
    scrolled_window: &gtk4::ScrolledWindow,
    bottom_clip: &gtk4::Box,
    top_val: f64,
) {
    let adj = scrolled_window.vadjustment();
    let viewport_h = adj.page_size();
    if viewport_h <= 0.0 {
        if bottom_clip.height_request() != 0 {
            bottom_clip.set_height_request(0);
        }
        return;
    }
    // Share the overlays' single covering algorithm: logical-line rows fed to the
    // pure bottom_clip_height. (Scroll-mode uses line_yrange rows, not the wrapped
    // display_rows the overlays use — same algorithm, different row source.)
    let tv = text_view.upcast_ref::<gtk4::TextView>();
    let rows = crate::ui::line_yrange_rows(tv, top_val, viewport_h);
    let clip_h = crate::ui::bottom_clip_height(&rows, top_val, viewport_h, adj.upper());
    if bottom_clip.height_request() != clip_h {
        bottom_clip.set_height_request(clip_h);
    }
}

/// The buffer line whose top is at or just above `target_y`, found by GTK's own
/// y→line mapping (O(1), no per-line walk). Returns the line index.
fn line_at_value(state: &AppState, target_y: f64) -> Option<usize> {
    let (iter, _top) = state.text_view.line_at_y(target_y.max(0.0) as i32);
    Some(iter.line() as usize)
}

/// Greatest line-top y at or below `target_y` (snaps a raw scroll target down to
/// a whole-line boundary), using GTK's y→line mapping.
fn snap_value_to_line_top(state: &AppState, target_y: f64) -> f64 {
    let Some(line) = line_at_value(state, target_y) else { return target_y };
    match state.buffer.iter_at_line(line as i32) {
        Some(iter) => {
            let (y, _h) = state.text_view.line_yrange(&iter);
            y as f64
        }
        None => target_y,
    }
}

/// Greatest VISUAL-ROW top y at or below `target_y` on the main reading card —
/// the per-wrapped-row analogue of `snap_value_to_line_top`, for paging WITHIN an
/// over-tall prose paragraph (one buffer line wrapping to many rows, so there are
/// no intermediate line tops to snap to).
///
/// Seeks to the BUFFER LINE containing `target_y` (`line_at_y`) and walks only
/// THAT line's wrapped display rows, rather than scanning every visual row from
/// the document start. The document-start scan (`crate::ui::display_rows`) caps
/// at 8192 visual rows: on a long prose work (e.g. Bleak House wraps to far more
/// than 8192 rows) a `target_y` beyond that cap found NO row at/below it and
/// snapped back to `lower` — which, during page-table generation, made
/// `prose_next_boundary` see `snapped <= y0`, return `None` mid-document, and
/// emit one giant final page (VALIDATE_FAIL fit). Seeking makes the snap correct
/// at ANY depth, and O(rows-in-one-line) instead of O(rows-from-start). Clamped
/// to the scroll range.
/// Prose row-fit correction for a fill boundary. `snapped` is the greatest
/// display-row top at/below the raw boundary pixel `raw` (from
/// `snap_value_to_display_row`). When `raw` falls in the whitespace BETWEEN
/// that row's ink bottom and the next row's top (inter-paragraph spacing, or
/// a blank separator's leading), the snapped row FULLY FITS on the page —
/// excluding it desynchronizes the stored page grid from the live bottom
/// clip (`bottom_clip_height` admits any row whose BOTTOM fits the budget),
/// so the reader SEES the row on this page while the table assigns it to the
/// next: the sync page turn fired one visible row early and the next page
/// re-showed a row already read (BH "at a loss how to receive it. I hinted
/// that the climate—", 2026-07-09). Returns `Some(next display row top)`
/// when the snapped row's bottom is within `raw`; `None` when the row
/// genuinely straddles `raw` (keep `snapped`), can't be located, or is the
/// document's last row.
pub(crate) fn next_row_top_if_row_fits(state: &AppState, snapped: f64, raw: f64) -> Option<f64> {
    use gtk4::prelude::TextViewExt;
    let tv = state.text_view.upcast_ref::<gtk4::TextView>();
    let top_margin = tv.top_margin() as f64;
    let (mut iter, _) = tv.line_at_y((snapped - top_margin).max(0.0) as i32);
    for _ in 0..1024 {
        let rect = tv.iter_location(&iter);
        let row_top = rect.y() as f64 + top_margin;
        if rect.height() > 0 && (row_top - snapped).abs() <= 0.5 {
            if row_top + rect.height() as f64 > raw + 0.5 {
                return None; // straddles the budget: the snap is correct
            }
            // Fits: the boundary belongs at the NEXT display row's top.
            let mut next = iter;
            for _ in 0..64 {
                if !tv.forward_display_line(&mut next) {
                    return None; // document tail
                }
                let r = tv.iter_location(&next);
                if r.height() > 0 {
                    return Some(r.y() as f64 + top_margin);
                }
            }
            return None;
        }
        if row_top > snapped + 0.5 || !tv.forward_display_line(&mut iter) {
            return None;
        }
    }
    None
}

pub(crate) fn snap_value_to_display_row(state: &AppState, target_y: f64) -> f64 {
    use gtk4::prelude::{TextViewExt, TextBufferExt};
    let adj = state.scrolled_window.vadjustment();
    let lower = adj.lower();
    let max_value = (adj.upper() - adj.page_size()).max(lower);
    let target = target_y.clamp(lower, max_value);
    let tv = state.text_view.upcast_ref::<gtk4::TextView>();
    let top_margin = tv.top_margin() as f64;
    // The buffer line whose vertical extent contains `target`. line_at_y takes
    // buffer-y (no top margin); iter_location returns buffer-y too, so we add
    // top_margin to both row tops to match display_rows' coordinate space.
    let (line_iter, _) = tv.line_at_y((target - top_margin).max(0.0) as i32);
    let line_no = line_iter.line();
    let buffer = tv.buffer();
    // Walk the wrapped display rows from ONE logical line BEFORE the target's
    // line through the target's line, keeping the greatest row top at/below
    // `target`. Starting a line early is load-bearing: `line_at_y` returns the
    // line whose box contains `target`, but that box begins with `pixels_above_
    // lines` leading, so its FIRST row top can sit a few px BELOW `target` when
    // `target` falls in that leading gap. In that case the correct snap is the
    // previous line's LAST row (the greatest row top not exceeding `target`) —
    // scanning only the target's own line would wrongly snap FORWARD to a row
    // top past `target`, over-filling the page by those few px (observed: a
    // 1px fit overshoot at a page boundary landing in a line's leading gap).
    let start_line = line_no.saturating_sub(1);
    let mut iter = buffer.iter_at_line(start_line).unwrap_or(line_iter);
    let mut best = lower;
    loop {
        let rect = tv.iter_location(&iter);
        if rect.height() > 0 {
            let row_top = rect.y() as f64 + top_margin;
            if row_top <= target + 0.5 {
                best = best.max(row_top);
            } else {
                break;
            }
        }
        if !tv.forward_display_line(&mut iter) || iter.line() > line_no {
            break;
        }
    }
    best.clamp(lower, max_value)
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
                // Table mode: land on the stored page containing the paragraph
                // start — same grid as x/y and the sync advance path. A raw
                // para_start top would put the page off the canonical grid.
                let new_top = crate::input::page_table::table_top_for(state, para_start)
                    .unwrap_or(para_start);
                crate::logging::log_always(&format!(
                    "SYNC_PARA_TURN: para_start={} page_top={} new_top={}",
                    para_start, state.page_top_line, new_top
                ));
                set_page(state, new_top, PageDirection::Forward);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod overtall_clip_tests {
    use super::overtall_clip;

    #[test]
    fn adds_the_reserve_band_to_the_row_clip() {
        // widget 1112, usable 1052 -> reserve 60. A row_clip of 20 (small partial
        // row above usable bottom) plus the 60px reserve = 80px total clip, so the
        // over-tall paragraph keeps a full-page-sized bottom gap.
        assert_eq!(overtall_clip(20, 1112, 1052), 80);
    }

    #[test]
    fn zero_row_clip_still_reserves_the_band() {
        // Even when the last row lands exactly on usable_height (row_clip 0), the
        // reserve alone keeps the paragraph off the card edge (the flush-bottom fix).
        assert_eq!(overtall_clip(0, 1112, 1052), 60);
    }

    #[test]
    fn clamps_to_widget_height_and_non_negative() {
        // Absurd row_clip can't push the clip past the card height.
        assert_eq!(overtall_clip(9000, 1112, 1052), 1112);
        // usable >= widget (no reserve) with a 0 row_clip -> 0, never negative.
        assert_eq!(overtall_clip(0, 1112, 1112), 0);
        assert_eq!(overtall_clip(0, 1112, 1200), 0);
    }
}

#[cfg(test)]
mod paged_bottom_clip_tests {
    use super::{descender_allowance, paged_bottom_clip};

    #[test]
    fn subtracts_descender_allowance_from_slack() {
        // widget 1112, content sums to 1000 → 112px of slack below the last
        // line. A 6px descender allowance moves the clip top edge 6px below
        // the logical line bottom, revealing the flush descender ink.
        assert_eq!(paged_bottom_clip(1112, 1000, 6), 106);
    }

    #[test]
    fn never_negative_when_content_fills_widget() {
        // Degenerate: content already equals/overflows the space (the last
        // page, or pre-layout heights). Clip floors at 0.
        assert_eq!(paged_bottom_clip(1112, 1112, 6), 0);
        assert_eq!(paged_bottom_clip(1112, 1110, 6), 0);
    }

    #[test]
    fn zero_allowance_is_the_old_behavior() {
        // With allowance 0 the helper reduces to space - total, the pre-fix
        // clip — proves the change is purely additive.
        assert_eq!(paged_bottom_clip(1112, 1000, 0), 112);
    }

    #[test]
    fn allowance_is_capped_at_the_inter_line_spacing() {
        // The next line's ink can start as little as pixels_above_lines below
        // the clip's top edge (measured ~5px on the failing MND spread), so
        // the reveal must never exceed that spacing — a full descender_guard
        // (up to 24) would expose the next line's ascender tops.
        assert_eq!(descender_allowance(12, 6), 6);
        assert_eq!(descender_allowance(4, 6), 4);
        // line_spacing 0 → no reveal budget → old behavior, never negative.
        assert_eq!(descender_allowance(12, 0), 0);
        assert_eq!(descender_allowance(12, -3), 0);
    }
}

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
