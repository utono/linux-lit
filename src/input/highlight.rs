use crate::input::page_top::PageTop;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::log_fmt;
use super::viewport::{
    is_line_fully_visible, last_raw_visible_line,
    lines_per_page, page_turn_top_state,
};
use super::scroll::{
    set_page, set_page_instant, set_page_instant_offset, scroll_to_cursor, PageDirection,
};

// ---------------------------------------------------------------------------
// Highlight
// ---------------------------------------------------------------------------

/// Prose cursor-marking style switch. `false` (current): nav keybinds flash the
/// cursor paragraph's background with the phrase-highlight color, fading out;
/// no persistent marking. `true` (legacy): every OTHER visible paragraph is
/// dimmed via `dim_tag` so the current paragraph reads at full strength — flip
/// this back to `true` to restore the dimming behavior verbatim.
const PROSE_DIM_OTHER_PARAGRAPHS: bool = false;

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
            // Mid-paragraph pages: `current_line == page_top_line` means the
            // cursor's paragraph IS the one rendering at the top of this page
            // (via a nonzero `page_top_offset`), even though
            // `is_line_fully_visible` reports false for an over-tall
            // straddler (it charges the FULL wrapped height and gets
            // `count == 0`). Without this check, entering/extending visual
            // mode (or any other `update_highlight_and_ensure_visible` call)
            // on such a page fell through to `prose_table_boundary_for_line`,
            // which returns the page containing the paragraph's FIRST row —
            // yanking the view backward to an earlier page even though the
            // cursor's line was already visible right here. See Important #2
            // in final-review.md.
            let already_on_page = state.current_line == state.page_top.line();
            if !already_on_page && !is_line_fully_visible(state, state.current_line) {
                // Prose table mode is TABLE-AUTHORITATIVE: land on the stored
                // page (offset-aware) containing the cursor line. Only when no
                // prose table serves this line do we fall through to the play
                // table / live path below.
                if let Some((pt, po)) = crate::input::prose_pages::prose_table_boundary_for_line(
                    state, state.current_line,
                ) {
                    if (pt, po) != (state.page_top.line(), state.page_top.offset()) {
                        log_fmt!(
                            "PAGES_PROSE: ensure_visible current={} ({},{})->({},{})",
                            state.current_line, state.page_top.line(), state.page_top.offset(), pt, po
                        );
                        set_page_instant_offset(state, pt, po);
                    }
                } else {
                    // Table mode: land on the stored page containing the cursor so
                    // sync turns share x/y's page grid (the spoken line becomes the
                    // first dialogue of the new page in forward playback). Falls
                    // back to the live computation when no table serves this line.
                    let new_top = crate::input::page_table::table_top_for(state, state.current_line)
                        .unwrap_or_else(|| {
                            if state.current_line >= state.page_top.line() {
                                page_turn_top_state(state, state.current_line)
                            } else {
                                state.current_line
                            }
                        });
                    log_fmt!(
                        "SYNC_PAGE: ensure_visible triggered, current_line={} page_top={} new_top={}",
                        state.current_line, state.page_top.line(), new_top
                    );
                    let dir = if state.current_line >= state.page_top.line() { PageDirection::Forward } else { PageDirection::Backward };
                    set_page(state, new_top, dir);
                }
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
            // Prose table mode is TABLE-AUTHORITATIVE: decide whether a turn is
            // due from the CURRENT stored page's own last rendered line (never
            // re-walk the live engine — the play-table lesson `table_end_for_top`).
            // `prose_table_page_end` returns the stored page's exclusive end;
            // `end_off > 0` means `end_line` is partly on THIS page (last rendered
            // line = end_line), else the last rendered line is `end_line - 1`. A
            // turn is due only once the cursor passes that last rendered line —
            // then land on the stored page (offset-aware) that contains it.
            if state.is_prose()
                && state.column_count() == 1
                && crate::input::prose_pages::active_prose_page_table(state).is_some()
            {
                // A turn is due when the cursor's line is no longer ON the
                // current page. With leading-gap boundaries normalized (see
                // `prose_next_boundary`), the on-page invariant is exact and
                // OFFSET-AWARE: line L is on the page whose exclusive end is
                // `(end_line, end_off)` iff `(L, 0) < (end_line, end_off)`
                // lexicographically. So:
                //   * a page ending at `(L, 0)` (end_off == 0) does NOT contain L
                //     — L opens the next page (last rendered line = end_line - 1);
                //   * a page ending at `(L, end_off>0)` DOES contain L (its first
                //     `end_off` px have ink) — L is on-page (last rendered = end_line).
                // Expressed directly so a stray degenerate positive-offset end
                // (leading gap, no visible rows) can't read as on-page.
                // `prose_table_page_end` is None when the current top is off-grid
                // — then always re-land on the containing page (resnap + turn).
                let turn_due = match crate::input::prose_pages::prose_table_page_end(state) {
                    Some((end_line, end_off)) => {
                        // NOT on-page  <=>  (current_line, 0) >= (end_line, end_off).
                        (state.current_line, 0) >= (end_line, end_off)
                    }
                    None => true, // off-grid: re-land on the stored page
                };
                if turn_due {
                    if let Some((pt, po)) =
                        crate::input::prose_pages::prose_table_boundary_for_line(
                            state, state.current_line,
                        )
                    {
                        if (pt, po) != (state.page_top.line(), state.page_top.offset()) {
                            crate::logging::log_always(&format!(
                                "PAGES_PROSE: sync page-turn current={} ({},{})->({},{})",
                                state.current_line, state.page_top.line(),
                                state.page_top.offset(), pt, po
                            ));
                            set_page_instant_offset(state, pt, po);
                        }
                    }
                }
                auto_show_vocab_popup(state);
                return;
            }
            let last_vis = last_raw_visible_line(state, state.page_top.line());
            if state.current_line > last_vis {
                // Table mode: the stored page containing the spoken line IS the
                // landing page — same grid as x/y, and the table's final page
                // already is the canonical anchor spread, so the final-region
                // redirect below is only needed on the live path.
                let new_top = if let Some(t) =
                    crate::input::page_table::table_top_for(state, state.current_line)
                {
                    t
                } else {
                    let mut nt = page_turn_top_state(state, state.current_line);
                    // Don't turn onto a spread in the work's FINAL region (an empty
                    // right column for a lone EPILOGUE, or an underfilled too-early
                    // final spread) — redirect to the canonical final spread so the
                    // tail fills both columns. Shared with x and q/j; see
                    // `navigation::redirect_to_final_spread`.
                    if let Some(anchor) = super::navigation::redirect_to_final_spread(state, nt) {
                        nt = anchor;
                    }
                    nt
                };
                crate::logging::log_always(&format!(
                    "SYNC_PAGE_TURN: current={} last_vis={} old_top={} new_top={}",
                    state.current_line, last_vis, state.page_top.line(), new_top,
                ));
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
    // First-open anchor: keep page_top at 0 so the opening Act/Prologue header
    // stays at the top of the page; do not scroll down to the first dialogue
    // line. Consume the flag so subsequent navigation behaves normally.
    let top_anchored = state.pending_top_anchor.replace(false);
    if state.page_top.line() == 0
        && state.current_line > 0
        && !state.pending_synopsis.get()
        && !top_anchored
    {
        state.page_top = PageTop::at_line_start(state.current_line);
    }
    update_highlight(state);

    // Page label is intentionally NOT set here. text_view.height() is still
    // 0 (scrolled_window is hidden), so viewport_page_for_line would build
    // a degenerate page_tops index and return page 1, causing a visible
    // "# - 1" flash. The resize tick refreshes the label once layout
    // settles (see app.rs deferred-layout-refresh branch).

    let scroll_to = state.page_top.line();
    // A prose page top is `(line, row-offset px)`. Scrolling to the LINE's top
    // and dropping the offset would discard the cross-work jump landing's snap
    // (see display_work_at_with_prepared) and render a window the pagination
    // never chose. `+ 0` for every pre-existing caller — page_top_offset is 0
    // unless a prose grid put a boundary mid-paragraph.
    let scroll_off = state.page_top.offset();
    let line_count = state.effective_line_count();
    let is_prose = state.is_prose();
    let one_section_per_page = state.one_section_per_page();
    let cursor_line = Some(state.current_line);

    // Snap scroll position synchronously. line_yrange may return 0 if GTK
    // hasn't validated the layout yet, so we defer to an idle callback.
    let text_view = state.text_view.clone();
    let bottom_clip = state.bottom_clip.clone();
    let loading_flag = state.loading_work.clone();
    let refresh_flag = state.needs_layout_refresh.clone();
    let buffer = state.buffer.clone();
    let scrolled_window = state.scrolled_window.clone();
    let section_starts: Option<Vec<bool>> = state.section_starts().map(|s| s.to_vec());

    glib::idle_add_local_once(move || {
        if let Some(iter) = buffer.iter_at_line(scroll_to as i32) {
            let (y, _h) = text_view.line_yrange(&iter);
            let adj = scrolled_window.vadjustment();
            let max_scroll = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value(((y + scroll_off) as f64).max(0.0).min(max_scroll));
        }
        // Show the scrolled window so GTK can complete the layout pass.
        scrolled_window.set_visible(true);
        // Defer clip calculation to the next idle tick so line_yrange
        // returns accurate heights after the widget is visible and laid out.
        glib::idle_add_local_once(move || {
            super::scroll::update_bottom_clip_public(&text_view, &bottom_clip, &scrolled_window, scroll_to, line_count, is_prose, section_starts.as_deref(), one_section_per_page, cursor_line);
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
    // On a pinned prose grid the landing is the STORED page containing the
    // cursor, not a centring guess. `current_line - lpp/2` is off-grid by
    // construction and can even land on the PREVIOUS page's tail, which is how
    // a concordance/echo/vocab jump dropped the reader out of table mode
    // (2026-07-27 audit). Deliberate, approved consequence: on those works the
    // cursor is no longer vertically centred after a jump — it sits where the
    // stored page puts it, matching what page-turning already does.
    //
    // Reads `prose_table_boundary_for_line` rather than
    // `canonical_page_top_offset_for` on purpose: plays are NOT part of this
    // defect and must keep centring, so the play table must not be consulted.
    if let Some((top, off)) =
        crate::input::prose_pages::prose_table_boundary_for_line(state, state.current_line)
    {
        crate::input::scroll::set_page_instant_offset(state, top, off);
    } else {
        let lpp = lines_per_page(state);
        let new_top = state.current_line.saturating_sub(lpp / 2);
        set_page_instant(state, new_top);
    }
    auto_show_vocab_popup(state);
}

/// If vocab auto-popup is enabled, show/update the popup when the paragraph changes.
pub(crate) fn auto_show_vocab_popup(state: &mut AppState) {
    // If synopsis is currently showing, leave it alone (user toggled it on)
    if state.sidebar_mode == crate::app::SidebarMode::Synopsis && state.synopsis_visible {
        return;
    }

    if !state.vocab_popup.auto {
        return;
    }
    // Refresh whenever the current line changes; the refresh function
    // decides whether to show (line has vocab words) or hide (line has none).
    if state.vocab_popup.line != Some(state.current_line) {
        if state.vocab_popup.popup.is_visible() {
            crate::app::vocab_popup::refresh_vocab_popup(state);
        } else {
            crate::app::vocab_popup::open_vocab_popup(state);
        }
    }
}

/// Update visual state for the current line. Only applies dim/cursor tags
/// to the visible range (page_top_line +/- margin) for performance.
/// When dim is off, fades out the old cursor highlight smoothly.
pub(crate) fn update_highlight(state: &mut AppState) {
    // Cursor crossing a column's bottom-clip boundary line (the first line
    // hidden below the clip — for the left column that is the right column's
    // FIRST line, on-page and routinely highlighted): re-schedule that
    // column's clip. The paged clip's descender allowance must collapse to 0
    // while the boundary line carries the cursor-highlight band and reopen
    // when the cursor leaves — see `descender_allowance` in scroll.rs.
    {
        let old = state.prev_highlight_line.get();
        let new = Some(state.current_line);
        if old != new {
            // The `-`/`_` word underline belongs to the line it was made on.
            // `active_underline` already treats it as absent once the cursor
            // leaves, so nothing can ACT on a stale one — but the tag stays
            // painted until something removes it. This is that something: one
            // funnel every cursor move already passes through, rather than the
            // ~76 `current_line` write sites hooking it individually.
            if state.word_cycle.cycle_line.is_some()
                && state.word_cycle.cycle_line != new
                && !state.word_cycle.collect_ranges.is_empty()
            {
                crate::input::actions::word_copy::clear_word_underline(state);
            }
            let lb = state.left_clip_boundary.get();
            let rb = state.right_clip_boundary.get();
            if lb.is_some() && (old == lb || new == lb) {
                super::scroll::reschedule_left_clip_for_cursor(state);
            }
            if rb.is_some() && (old == rb || new == rb) {
                super::scroll::reschedule_right_clip_for_cursor(state);
            }
        }
    }
    // Prose cursor marking. Two styles, selected by PROSE_DIM_OTHER_PARAGRAPHS:
    //  * dim (legacy): mark the current paragraph by DIMMING every OTHER visible
    //    paragraph — the current one stays at full strength while the rest
    //    recede. Restore by flipping the const to true.
    //  * flash-off (current): no persistent marking and no nav flash — the
    //    moving karaoke tint (seek_to_current_line paints the new segment's
    //    first phrase) is the nav cue. flash_prose_cursor_line now fires ONLY
    //    from the self-contained overlay-close path (flash_reader_cursor arms
    //    + flushes in one call; it never routes through here).
    // Plays/verse instead tint the current line's background persistently.
    // Computed before the buffer borrow below.
    let prose_dim = PROSE_DIM_OTHER_PARAGRAPHS && state.is_prose();
    // BOTH classes: no persistent cursor tint while karaoke marks the cursor
    // line (the sweep is the marking) — the prose-only rule generalized now
    // that verse karaoke is on by default. karaoke_marks_cursor folds in the
    // untimestamped-line fallback described above, plus media capability and
    // the Alt+p axis.
    let karaoke_no_tint =
        !PROSE_DIM_OTHER_PARAGRAPHS && crate::input::phrase_highlight::karaoke_marks_cursor(state);
    // Consume any leftover flash/hold flags unconditionally so a flag set by
    // the page-set paths (prose_flash_hold) or a raced overlay-close arm can't
    // linger and leak into a later overlay-close flash.
    state.pending_prose_flash.replace(false);
    state.prose_flash_hold.replace(false);

    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let cl_tag = &state.cursor_line_tag;
    let fade_tag = &state.cursor_fade_tag;

    // Compute visible range with margin for scroll overshoot
    let lpp = lines_per_page(state);
    let margin = 5;
    let vis_start = state.page_top.line().saturating_sub(margin);
    let vis_end = (state.page_top.line() + lpp + margin)
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

        // Apply fade-out to the old cursor line (if it changed). Prose has no
        // persistent bg tint (dim or flash mode), so there is no fade for it.
        if !prose_dim && !karaoke_no_tint {
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
                // Fade toward the cursor-line highlight hue (cursor_line_bg),
                // not the window root color — otherwise the fade reads as the
                // theme's blue regardless of the configured highlight color.
                let (rc_r, rc_g, rc_b) = crate::theme::rgba_str_to_rgb(&state.theme.cursor_line_bg);
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
        } // cursor fade (skipped while karaoke marks the cursor line)

        // Mark the current paragraph. PROSE: dim every OTHER visible paragraph
        // (apply `dim_tag` across the visible range, then undim the current
        // paragraph), so the current/spoken paragraph reads at full strength
        // while the rest recede — the inverse of a highlight. PLAYS/VERSE: tint
        // the current line's background.
        if prose_dim {
            // dim the whole visible range, then clear it from the current line.
            buffer.apply_tag(tag, &vis_start_iter, &vis_end_iter);
        }
        {
            if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                if prose_dim {
                    buffer.remove_tag(tag, &line_start, &line_end);
                } else if !karaoke_no_tint {
                    // No persistent cursor tint while karaoke marks the
                    // cursor line — the sweep is the only marking.
                    buffer.apply_tag(cl_tag, &line_start, &line_end);
                }
                // Intentional observability hook (NOT stale per-keystroke trace):
                // the saved-position-resume regression test asserts the highlight
                // lands on the restored cursor line. Keep this log — removing it
                // orphaned that assertion once before (commit e7de29f). See
                // tests/saved_position_resume.rs.
                log_fmt!("CURSOR_LINE: applied tag to line {}", state.current_line);
            }
        }
        // (Nav binds no longer flash here; the overlay-close flash is armed +
        // flushed entirely inside flash_reader_cursor.)
        // When visual selection is active, apply highlight even when dim is off
        crate::input::visual::clear_selection_highlight(state);
        crate::input::visual::apply_selection_highlight(state);
        state.prev_highlight_line.set(Some(state.current_line));
        repaint_reader_gloss_visible(state);
        crate::app::scene_synopsis::update_title_bar_scene(state);
        crate::app::scene_synopsis::update_running_heads(state);
        crate::input::navigation::refresh_persistent_chapter_toast(state);
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

    repaint_reader_gloss_visible(state);
    crate::app::scene_synopsis::update_title_bar_scene(state);
    crate::app::scene_synopsis::update_running_heads(state);
    crate::input::navigation::refresh_persistent_chapter_toast(state);
}

/// Flush a nav-flash flag that is still armed after its action handler ran.
/// A nav press that ends as a no-op (e.g. `'` with no bookmark ahead, `;` at
/// the first bookmark) early-returns before update_highlight, so the armed
/// flag would linger and fire on the next repaint — including sync-driven
/// cursor moves, which must never flash. Consuming it here instead flashes the
/// line the cursor is on, so every nav keybind gives the same transient cue
/// whether or not the cursor moved.
pub(crate) fn flush_pending_prose_flash(state: &mut AppState) {
    if !state.pending_prose_flash.replace(false) {
        return;
    }
    // Flash only where karaoke is the marking (no persistent tint to look
    // at); with the axis on cursor-line mode — or any karaoke fallback — the
    // persistent tint already re-orients the eye.
    if PROSE_DIM_OTHER_PARAGRAPHS
        || !state.is_prose()
        || !crate::input::phrase_highlight::karaoke_marks_cursor(state)
    {
        return;
    }
    let hold = state.prose_flash_hold.replace(false);
    flash_prose_cursor_line(state, hold);
}

/// Flash the reader cursor line NOW — overlay-close feedback: when an overlay
/// (gloss / synopsis / journal) drops back to the reader, the eye needs
/// re-orienting to where the cursor sits in the main card. Same transient cue
/// as the nav flash; a no-op for verse (persistent cursor tint) and when the
/// cursor line is hidden.
pub(crate) fn flash_reader_cursor(state: &mut AppState) {
    state.pending_prose_flash.set(true);
    flush_pending_prose_flash(state);
}

/// Flash the cursor paragraph's background with the phrase-highlight color,
/// then fade it to transparent. Prose nav feedback: replaces the legacy
/// other-paragraph dimming (PROSE_DIM_OTHER_PARAGRAPHS) with a transient cue
/// that shares the karaoke phrase tint's visual language. The color is read
/// from the CURRENT theme at flash time, so theme switches need no tag refresh.
/// `hold` (a page turn preceded the flash): keep full strength for ~350ms
/// before fading, so the blink lands after the new page's first paint instead
/// of decaying while the page is still laying out.
fn flash_prose_cursor_line(state: &mut AppState, hold: bool) {
    // Cancel any in-flight flash; skip() drives its callback to the end value,
    // which clears the tag from the buffer.
    if let Some(prev) = state.prose_flash_anim.take() {
        prev.skip();
    }

    let buffer = state.buffer.clone();
    let flash_tag = state.prose_flash_tag.clone();
    let (buf_start, buf_end) = buffer.bounds();
    buffer.remove_tag(&flash_tag, &buf_start, &buf_end);

    let Some(line_start) = buffer.iter_at_line(state.current_line as i32) else {
        return;
    };
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    buffer.apply_tag(&flash_tag, &line_start, &line_end);
    log_fmt!("PROSE_FLASH: line {} hold={}", state.current_line, hold);

    // Start at the phrase tint's own color/alpha so the flash's first frame
    // matches the narration-sync phrase highlight exactly, then fade to 0.
    let rgba = gtk4::gdk::RGBA::parse(&state.theme.phrase_highlight_bg)
        .unwrap_or_else(|_| gtk4::gdk::RGBA::new(1.0, 1.0, 1.0, 0.22));
    let base_alpha = rgba.alpha();
    // Paint the tint at FULL strength now, before any animation frame runs —
    // the tag object keeps the 0-alpha rgba from the end of its last fade, so
    // a deferred start would otherwise show nothing until the first callback.
    {
        use gtk4::prelude::TextTagExt;
        flash_tag.set_paragraph_background_rgba(Some(&gtk4::gdk::RGBA::new(
            rgba.red(),
            rgba.green(),
            rgba.blue(),
            base_alpha,
        )));
    }
    let (duration, hold_frac) = if hold { (1050_u32, 350.0 / 1050.0) } else { (700, 0.0) };
    let target = adw::CallbackAnimationTarget::new(move |value| {
        use gtk4::prelude::TextTagExt;
        // `value` runs 1→0 linearly. Hold full alpha through `hold_frac` of
        // the run, then ease-out-quad to 0 — with hold_frac == 0 this is
        // exactly the original EaseOutQuad fade.
        let t = 1.0 - value;
        let f = if t <= hold_frac {
            1.0
        } else {
            let u = (t - hold_frac) / (1.0 - hold_frac);
            (1.0 - u) * (1.0 - u)
        };
        flash_tag.set_paragraph_background_rgba(Some(&gtk4::gdk::RGBA::new(
            rgba.red(),
            rgba.green(),
            rgba.blue(),
            f as f32 * base_alpha,
        )));
        if value <= 0.0 {
            let (s, e) = buffer.bounds();
            buffer.remove_tag(&flash_tag, &s, &e);
        }
    });
    let anim = adw::TimedAnimation::new(&state.text_view, 1.0, 0.0, duration, target);
    anim.set_easing(adw::Easing::Linear);
    if hold {
        // Page-turn flash: don't start the clock until the NEW page's first
        // painted frame. The first paint after an instant page set can take
        // ~800ms on the live display (observed PAINT: 821ms at 1920x1200), so
        // a wall-clock hold spent the whole flash before anything was visible.
        // The tint is already applied at full alpha above; this one-shot tick
        // (the same first-frame signal log_first_paint uses) starts the
        // hold+fade once the page is actually on screen.
        let a = anim.clone();
        state.text_view.add_tick_callback(move |_, _| {
            a.play();
            glib::ControlFlow::Break
        });
    } else {
        anim.play();
    }
    state.prose_flash_anim = Some(anim);
}

/// Reapply the reader-gloss tints to every glossed buffer line. Off-cursor
/// glossed lines get the contrast-guarded tint (reader-gloss-line /
/// theme.reader_gloss). The cursor's own glossed line gets the distinct
/// on-cursor color (reader-gloss-cursor-line / theme.reader_gloss_cursor) so
/// it reads differently from both normal body text and the off-cursor tint.
/// Called at the end of `update_highlight` after the dim and cursor tags are
/// set, because the dim path repaints `dim_tag` across the whole visible range
/// every move and would otherwise leave glossed lines un-tinted. Iterates the
/// `reader_gloss_lines` set directly (a handful of explicitly-glossed lines) so
/// it is correct in both single- and two-column layouts and still cheap.
fn repaint_reader_gloss_visible(state: &AppState) {
    if state.reader_gloss_lines.is_empty() {
        return;
    }
    for &buf_idx in &state.reader_gloss_lines {
        if buf_idx == state.current_line {
            // Cursor on a glossed line: show the distinct on-cursor color.
            crate::app::remove_reader_gloss_tag_from_line(state, buf_idx);
            crate::app::apply_reader_gloss_cursor_tag_to_line(state, buf_idx);
        } else {
            // Off-cursor glossed line: the gloss tint; clear any stale on-cursor color.
            crate::app::remove_reader_gloss_cursor_tag_from_line(state, buf_idx);
            crate::app::apply_reader_gloss_tag_to_line(state, buf_idx);
        }
    }
}
