use super::AppState;
use crate::app::layout::{apply_column_layout, overlay_card_size};
use crate::app::scene_synopsis::{current_scene_divs, synopsis_label};
use crate::app::font::{reapply_font, rebuild_line_number_gutter};
use gtk4::prelude::*;

/// Toggle translation lines below original text.
/// When showing: dims all lines, inserts translation text below matched lines.
/// When hiding: removes inserted lines and dim tag.
pub fn toggle_translations(state: &mut AppState) {
    if state.translations.is_empty() {
        crate::logging::log("TRANSLATIONS: no translations for this work");
        return;
    }

    crate::logging::log(&format!(
        "TRANSLATIONS: toggle entry visible={} buf_lines={} translations={} current_line={} page_top={} line_map={}",
        state.translations_visible,
        state.buffer.line_count(),
        state.translations.len(),
        state.current_line,
        state.page_top_line,
        state.line_map.is_some(),
    ));

    if state.translations_visible {
        hide_translations(state);
    } else {
        show_translations(state);
    }
}

fn show_translations(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => {
            crate::logging::log("TRANSLATIONS: show aborted — no current_work");
            return;
        }
    };

    state.card_vbox.set_opacity(0.0);

    // Capture the cursor's on-screen y-position BEFORE mutating the buffer.
    // The cursor is the user's visual anchor — keep it at the same screen
    // position after inserts so the viewport does not appear to scroll.
    let pre_adj_value = state.scrolled_window.vadjustment().value();
    let pre_adj_upper = state.scrolled_window.vadjustment().upper();
    let pre_adj_page = state.scrolled_window.vadjustment().page_size();
    let cursor_screen_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| {
            let (y, h) = state.text_view.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_SHOW: pre-insert cursor yrange y={} h={} adj_val={} screen_y={}",
                y, h, pre_adj_value as i64, (y as f64 - pre_adj_value) as i64,
            ));
            y as f64 - pre_adj_value
        });
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: pre-insert adj val={} upper={} page={} current_line={} page_top={}",
        pre_adj_value as i64, pre_adj_upper as i64, pre_adj_page as i64,
        state.current_line, state.page_top_line,
    ));

    // Build a list of (buffer_line, translation_text) pairs
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let line_count = state.buffer.line_count() as usize;
    let lm_len = state
        .line_map
        .as_ref()
        .map(|lm| lm.buffer_to_work.len())
        .unwrap_or(0);

    for buf_line in 0..line_count {
        let work_idx = state.work_line_for_buffer(buf_line);
        if let Some(wi) = work_idx {
            if let Some(line) = work.lines.get(wi) {
                if let Some(translation) = state.translations.get(&line.id) {
                    inserts.push((buf_line, translation.to_string()));
                }
            }
        }
    }

    crate::logging::log(&format!(
        "TRANSLATIONS: show scan buf_lines={} line_map_len={} work_lines={} inserts={}",
        line_count,
        lm_len,
        work.lines.len(),
        inserts.len(),
    ));

    // Insert bottom-to-top to avoid index shifting
    for (buf_line, text) in inserts.iter().rev() {
        let line_end = if let Some(mut iter) = state.buffer.iter_at_line(*buf_line as i32) {
            if !iter.ends_line() {
                iter.forward_to_line_end();
            }
            iter
        } else {
            continue;
        };
        state.buffer.insert(&mut line_end.clone(), &format!("\n    {}", text));
    }

    // Build translation_lines tracking vector
    let new_line_count = state.buffer.line_count() as usize;
    let mut tl = vec![false; new_line_count];

    let mut orig_idx = 0;
    let orig_line_count = line_count;
    let mut buf_idx = 0;
    let work_lines = &work.lines;
    while orig_idx < orig_line_count && buf_idx < new_line_count {
        tl[buf_idx] = false;
        let work_idx = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work.get(orig_idx).copied().flatten()
        } else if orig_idx < work_lines.len() {
            Some(orig_idx)
        } else {
            None
        };
        let has_translation = work_idx
            .and_then(|wi| work_lines.get(wi))
            .and_then(|line| state.translations.get(&line.id))
            .is_some();
        buf_idx += 1;
        if has_translation && buf_idx < new_line_count {
            tl[buf_idx] = true;
            buf_idx += 1;
        }
        orig_idx += 1;
    }
    state.translation_lines = tl;

    // Configure the translation gloss tag: the main card font, italic, 4pt
    // below the reader size. reapply_font (below) keeps this in sync on every
    // font change; set it here too so the first paint isn't a flash of the
    // previous size/family.
    let trans_size = state.config.font_size.saturating_sub(4);
    let desc = pango::FontDescription::from_string(
        &format!("{} Italic {}", state.config.font_family, trans_size),
    );
    state.translation_text_tag.set_font_desc(Some(&desc));

    // Apply translation-text tag to translation lines
    for (i, is_trans) in state.translation_lines.iter().enumerate() {
        if *is_trans {
            if let Some(line_start) = state.buffer.iter_at_line(i as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                state.buffer.apply_tag(&state.translation_text_tag, &line_start, &line_end);
            }
        }
    }

    // Ensure translation tag overrides the font-size tag
    let highest = state.buffer.tag_table().size() - 1;
    state.translation_text_tag.set_priority(highest);

    // Adjust current_line and page_top_line to account for inserted lines
    let old_current = state.current_line;
    let old_top = state.page_top_line;
    // Save the pre-toggle reader position so hide can restore it exactly.
    state.pre_translation_page = Some((old_current, old_top));
    state.current_line = map_line_after_insert(state.current_line, &inserts);
    state.page_top_line = map_line_after_insert(state.page_top_line, &inserts);

    // Remap the section-start bitmap onto the inflated buffer so the
    // section-break clamp (and the one-section-per-page clip for sonnets) lands
    // on the right physical line. Each original section start at index `i`
    // moves to `map_line_after_insert(i, &inserts)`; inserted translation lines
    // stay `false`. `section_starts()` returns this while translations show.
    if let Some(orig) = state.line_map.as_ref().map(|lm| lm.section_starts.clone()) {
        let mut remapped = vec![false; new_line_count];
        for (i, is_start) in orig.iter().enumerate() {
            if *is_start {
                let j = map_line_after_insert(i, &inserts);
                if j < remapped.len() {
                    remapped[j] = true;
                }
            }
        }
        state.translation_section_starts = remapped;
    } else {
        state.translation_section_starts = Vec::new();
    }

    let cursor_on_translation = state.current_line < state.translation_lines.len()
        && state.translation_lines[state.current_line];
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: line remap current {}→{} page_top {}→{} (inserts={}) cursor_on_translation={}",
        old_current, state.current_line, old_top, state.page_top_line, inserts.len(),
        cursor_on_translation,
    ));

    state.translations_visible = true;

    // Hide the sign column while translations show — the interleaved
    // translation lines make per-line signs misleading. Remember the prior
    // visibility so hide_translations can restore it.
    if state.sign_visible_before_translations.is_none() {
        state.sign_visible_before_translations = Some(state.sign_column_visible.get());
    }
    state.sign_column_visible.set(false);
    crate::input::timestamps::redraw_sign_gutters(state);

    // Translations force a single column (column_count() now returns 1 because
    // translations_visible is set). Reconfigure the layout to hide the right
    // column and widen the card before anchoring the viewport below.
    apply_column_layout(state);

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
    // The buffer's translation lines were just inserted/removed, so every cached
    // line index is stale. Drop the last-visible-range cache — otherwise
    // is_line_fully_visible compares the cursor against the old line numbers,
    // never fires a page turn, and the view scrolls off a line boundary
    // (clipping top and bottom).
    state.last_visible_range.set(None);

    let mid_adj = state.scrolled_window.vadjustment();
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: post-reapply_font adj val={} upper={} page={}",
        mid_adj.value() as i64, mid_adj.upper() as i64, mid_adj.page_size() as i64,
    ));

    // Repaint the cursor highlight but do NOT page-turn.
    crate::input::navigation::update_highlight_only(state);

    // Defer viewport anchor to an idle callback — GTK hasn't re-laid the
    // buffer yet so line_yrange and adjustment.upper are stale right now.
    //
    // The translation overlay scrolls continuously (cursor-following, vim
    // scrolloff) — it is NOT pinned to the old two-column page boundary. So
    // CENTER the highlighted cursor line: target = cursor_y - page_size*0.25
    // (the same ¼-down offset center_cursor uses). Snap that target to a
    // whole-line top so the continuously-scrolling overlay never lands between
    // line boundaries (which would clip the top/bottom lines); the deferred
    // refresh_bottom_clip below reads the aligned value and covers the partial
    // bottom line. (See the anti-clipping note in
    // docs/troubleshooting/page-turning-mechanics.md.)
    let cursor_line = state.current_line;
    let _ = cursor_screen_y; // no longer used for anchoring
    let tv = state.text_view.clone();
    let sw = state.scrolled_window.clone();
    let bc = state.bottom_clip.clone();
    let vbox = state.card_vbox.clone();
    gtk4::glib::idle_add_local_once(move || {
        let adj = sw.vadjustment();
        let cursor_y = tv.buffer().iter_at_line(cursor_line as i32).map(|iter| {
            let (y, h) = tv.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_SHOW: idle center cursor yrange y={} h={} line={}",
                y, h, cursor_line,
            ));
            y as f64
        });
        if let Some(cy) = cursor_y {
            let max_val = (adj.upper() - adj.page_size()).max(0.0);
            let offset = adj.page_size() * 0.25;
            let raw = (cy - offset).clamp(0.0, max_val);
            // Snap the centered target down to the top of whatever line begins
            // at or above it, so the viewport top sits on a whole-line edge.
            let (snap_line, _) = tv.line_at_y(raw as i32);
            let val = tv
                .buffer()
                .iter_at_line(snap_line.line())
                .map(|it| tv.line_yrange(&it).0 as f64)
                .unwrap_or(raw)
                .clamp(0.0, max_val);
            crate::logging::log(&format!(
                "TRANSLATIONS_SHOW: idle center cursor_y={} offset={} raw={} snapped={} upper={} page={}",
                cy as i64, offset as i64, raw as i64, val as i64,
                adj.upper() as i64, adj.page_size() as i64,
            ));
            adj.set_value(val);
            // page_top_line is left as-is: the overlay's j/k scrolloff path
            // (scroll_cursor_into_view_scrolloff) recomputes it from the live
            // adjustment on the first move, and the bottom-clip below is
            // scroll-aware (not page_top-relative), so it covers correctly now.
            // Cover the partial line at the bottom edge immediately on reveal —
            // the paged refresh_bottom_clip is page_top-relative and unreliable
            // here, so use the same scroll-aware clip the j/k path uses.
            crate::input::scroll::scrolloff_bottom_clip_widgets(&tv, &sw, &bc, val);
            // 100ms backstop: reapply_font changed line heights, so the FIRST
            // scroll-aware clip above can read pre-relayout metrics. Re-run it once
            // more against the settled layout at the SAME scroll value (mirrors
            // schedule_bottom_clip_update's idle+100ms pair, but scroll-aware — the
            // paged path is wrong here, see below).
            let (tv2, sw2, bc2) = (tv.clone(), sw.clone(), bc.clone());
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
                crate::input::scroll::scrolloff_bottom_clip_widgets(&tv2, &sw2, &bc2, val);
            });
        }
        vbox.set_opacity(1.0);
    });

    // NOTE: deliberately NOT calling refresh_bottom_clip(state) here. That is the
    // PAGED clip (page_top-relative), which assumes the scroll is snapped to
    // page_top. The translation reveal scrolls to a cursor-centered value that is
    // NOT page_top's top, so the paged clip computed a huge scroll_offset
    // (scroll_val - expected_y) and set the bottom clip to >2× the viewport height
    // — blanking the whole card until the first j/k. The scroll-aware
    // scrolloff_bottom_clip_widgets in the idle (and its 100ms backstop) is the
    // correct clip for the continuously-scrolled translation view.

    let new_buf_lines = state.buffer.line_count() as usize;
    let lm_len_after = state
        .line_map
        .as_ref()
        .map(|lm| lm.buffer_to_work.len())
        .unwrap_or(0);
    let line_map_stale = lm_len_after != new_buf_lines;
    let post_adj_value = state.scrolled_window.vadjustment().value();
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: FINAL inserted={} buf_lines {}->{} current {}->{} page_top {}->{} line_map_len={} stale={} adj {}->{} effective_line_count={}",
        inserts.len(),
        new_buf_lines.saturating_sub(inserts.len()),
        new_buf_lines,
        old_current,
        state.current_line,
        old_top,
        state.page_top_line,
        lm_len_after,
        line_map_stale,
        pre_adj_value as i64,
        post_adj_value as i64,
        state.effective_line_count(),
    ));

    rebuild_line_number_gutter(state);
}

/// Map an original buffer line index to its new position after translation inserts.
fn map_line_after_insert(orig_line: usize, inserts: &[(usize, String)]) -> usize {
    let mut offset = 0;
    for (buf_line, _) in inserts {
        if *buf_line < orig_line {
            offset += 1;
        } else {
            break;
        }
    }
    orig_line + offset
}

/// Strip translation lines from the buffer without repositioning the viewport.
/// Caller is responsible for scrolling/page-setting after this returns.
pub fn hide_translations_for_navigation(state: &mut AppState) {
    if !state.translations_visible {
        return;
    }
    strip_translation_lines(state);
}

fn hide_translations(state: &mut AppState) {
    state.card_vbox.set_opacity(0.0);

    // Capture the pre-toggle page BEFORE strip_translation_lines clears it.
    // These are pre-insert line indices, valid again after the strip restores
    // the original buffer numbering.
    let saved_pre_toggle = state.pre_translation_page.take();

    // Capture the cursor's on-screen y-position BEFORE removing lines so we
    // can restore it afterwards — the cursor is the user's visual anchor.
    let pre_adj_value = state.scrolled_window.vadjustment().value();
    let cursor_screen_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| {
            let (y, h) = state.text_view.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_HIDE: pre-remove cursor yrange y={} h={} adj_val={} screen_y={}",
                y, h, pre_adj_value as i64, (y as f64 - pre_adj_value) as i64,
            ));
            y as f64 - pre_adj_value
        });

    strip_translation_lines(state);

    // Repaint highlight but do NOT page-turn.
    crate::input::navigation::update_highlight_only(state);

    if state.column_count() == 2 {
        // Two-column work: translations were forcing a single column. Restore
        // the layout + page-position state, then defer the ENTIRE re-snap to
        // RESIZE_TICK. Do NOT call set_page_instant here: the left view still
        // has its single-column (over-wide, ~1408px) width, so column_split
        // would scroll the right view to a wrong split; the log showed that
        // pollutes the subsequent settled-width resnap (page_end 4215 vs the
        // correct 4219). The tick waits for the widths to settle (band check)
        // and produces the one correct resnap.
        apply_column_layout(state);
        let (cur, top) = saved_pre_toggle
            .unwrap_or((state.current_line, state.page_top_line));
        state.current_line = cur;
        state.page_top_line = top;
        // When we restored the FAITHFUL pre-toggle spread, tell the RESIZE_TICK
        // re-snap to trust it verbatim — skip snap_near_end_to_canonical, which
        // would re-derive the page from the saved cursor and (when the cursor is
        // the last line of the spread) land on the previous boundary, painting a
        // different, non-canonical spread. Only when no saved page existed do we
        // let the canonical snap correct the overlay-scrolled position.
        if saved_pre_toggle.is_some() {
            state.trust_restored_page.set(true);
        }
        crate::input::navigation::update_highlight_only(state);
        rebuild_line_number_gutter(state);
        state.needs_layout_refresh.set(true);
        state.card_vbox.set_opacity(1.0);
        return;
    }

    // Defer viewport anchor to an idle callback — GTK hasn't re-laid the
    // buffer yet so line_yrange and adjustment.upper are stale right now.
    let cursor_line = state.current_line;
    let screen_y = cursor_screen_y;
    let tv = state.text_view.clone();
    let sw = state.scrolled_window.clone();
    let vbox = state.card_vbox.clone();
    gtk4::glib::idle_add_local_once(move || {
        let adj = sw.vadjustment();
        let cur_y = tv.buffer().iter_at_line(cursor_line as i32).map(|iter| {
            let (y, h) = tv.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_HIDE: idle anchor cursor yrange y={} h={} line={}",
                y, h, cursor_line,
            ));
            y as f64
        });
        let adj_upper = adj.upper();
        let adj_page = adj.page_size();
        let new_adj = match (cur_y, screen_y) {
            (Some(y), Some(sy)) => Some((y - sy).max(0.0).min((adj_upper - adj_page).max(0.0))),
            _ => None,
        };
        crate::logging::log(&format!(
            "TRANSLATIONS_HIDE: idle anchor cur_y={:?} screen_y={:?} upper={} page={} new_adj={:?}",
            cur_y.map(|v| v as i64), screen_y.map(|v| v as i64),
            adj_upper as i64, adj_page as i64, new_adj.map(|v| v as i64),
        ));
        if let Some(val) = new_adj {
            adj.set_value(val);
        }
        vbox.set_opacity(1.0);
    });

    crate::input::navigation::refresh_bottom_clip(state);
    rebuild_line_number_gutter(state);
}

fn strip_translation_lines(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    let pre_hide_buf_lines = line_count;

    // Remove translation lines from buffer bottom-to-top
    for i in (0..line_count).rev() {
        if i < state.translation_lines.len() && state.translation_lines[i] {
            let line_start = if i > 0 {
                if let Some(mut iter) = state.buffer.iter_at_line((i - 1) as i32) {
                    if !iter.ends_line() {
                        iter.forward_to_line_end();
                    }
                    iter
                } else {
                    continue;
                }
            } else {
                state.buffer.start_iter()
            };
            let line_end = if let Some(mut iter) = state.buffer.iter_at_line(i as i32) {
                if !iter.ends_line() {
                    iter.forward_to_line_end();
                }
                iter
            } else {
                continue;
            };
            state.buffer.delete(&mut line_start.clone(), &mut line_end.clone());
        }
    }

    // Remove translation tag from entire buffer
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.translation_text_tag, &buf_start, &buf_end);

    // Reverse-map current_line and page_top_line
    let old_current = state.current_line;
    let old_top = state.page_top_line;
    state.current_line = map_line_before_insert(old_current, &state.translation_lines);
    state.page_top_line = map_line_before_insert(old_top, &state.translation_lines);

    crate::logging::log(&format!(
        "TRANSLATIONS_HIDE: line remap current {}→{} page_top {}→{} buf_lines {}→{}",
        old_current, state.current_line, old_top, state.page_top_line,
        pre_hide_buf_lines, state.buffer.line_count(),
    ));

    state.translation_lines.clear();
    state.translation_section_starts = Vec::new();
    state.translations_visible = false;

    // Clear the saved pre-toggle page (covers navigation-driven hide and
    // single-column paths) so it does not leak into a later toggle.
    state.pre_translation_page = None;

    // Restore the sign column to its pre-translation visibility.
    if let Some(prev) = state.sign_visible_before_translations.take() {
        state.sign_column_visible.set(prev);
        crate::input::timestamps::redraw_sign_gutters(state);
    }

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
    // The buffer's translation lines were just inserted/removed, so every cached
    // line index is stale. Drop the last-visible-range cache — otherwise
    // is_line_fully_visible compares the cursor against the old line numbers,
    // never fires a page turn, and the view scrolls off a line boundary
    // (clipping top and bottom).
    state.last_visible_range.set(None);
    rebuild_line_number_gutter(state);
}

/// Map a buffer line index (with translations) back to the original line index.
fn map_line_before_insert(buf_line: usize, translation_lines: &[bool]) -> usize {
    let mut orig = 0;
    for i in 0..=buf_line.min(translation_lines.len().saturating_sub(1)) {
        if i < translation_lines.len() && translation_lines[i] {
            // Skip translation lines
        } else if i == buf_line {
            return orig;
        } else {
            orig += 1;
        }
    }
    orig
}

/// Open the two-column speaker-grouped translation overlay for the current
/// scene, scrolled to the speaker block containing the cursor line.
pub fn show_translation_overlay(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    if rebuild_translation_overlay(state) {
        state.borrow_mut().input_mode = crate::app::InputMode::TranslationOverlay;
    }
}

/// Build (or rebuild) the two-column translation overlay for the cursor's
/// current scene and highlight/scroll to the cursor line. Idempotent:
/// `translation_overlay.show` clears prior content, so calling this again
/// after the cursor crosses into a new scene re-renders cleanly for that scene.
///
/// Returns `true` if the overlay was actually shown, `false` if it bailed early
/// (no current work / empty scene). Does NOT change `input_mode` — callers that
/// open the overlay (`show_translation_overlay`) set it themselves only on
/// success; the in-place rebuild path keeps the existing mode.
/// Make the translation overlay reflect the current reader cursor: if it's
/// open, either rebuild it (cursor crossed into a new scene) or just move the
/// highlight + follow-scroll. No-op when the overlay isn't visible. Takes the
/// `Rc` so it can manage its own borrows (rebuild needs an unborrowed handle).
pub fn sync_translation_overlay(
    state: &std::rc::Rc<std::cell::RefCell<AppState>>,
    scene_before: (i64, i64),
) {
    // Cheap visibility + scene check under a short borrow.
    let (visible, scene_after, cursor_w) = {
        let s = state.borrow();
        (
            s.translation_overlay.is_visible(),
            current_scene_divs(&s),
            s.work_line_for_buffer(s.current_line),
        )
    };
    if !visible {
        return;
    }
    if scene_after != scene_before {
        rebuild_translation_overlay(state);
        return;
    }
    if let Some(w) = cursor_w {
        let s = state.borrow();
        // Paginated overlay: turn to the page containing the cursor's block and
        // highlight it (synchronous — no scroll-settle timing).
        s.translation_overlay.show_for_cursor(w);
    }
}

pub fn rebuild_translation_overlay(state: &std::rc::Rc<std::cell::RefCell<AppState>>) -> bool {
    let s = state.borrow();

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };

    let (div1, div2) = current_scene_divs(&s);

    // Collect this scene's lines (preserving order) with their work indices.
    let scene_lines: Vec<crate::db::models::Line> = work
        .lines
        .iter()
        .filter(|l| l.div1 == div1 && l.div2 == div2)
        .cloned()
        .collect();
    if scene_lines.is_empty() {
        return false;
    }
    // Index of the first scene line within work.lines, for idx_of mapping.
    let base = work
        .lines
        .iter()
        .position(|l| l.div1 == div1 && l.div2 == div2)
        .unwrap_or(0);

    let blocks = crate::ui::translation_overlay::group_scene_into_blocks(
        &scene_lines,
        |i| base + i,
        |id| s.translations.get(&id).cloned(),
    );

    let (card_width, card_height) = overlay_card_size(&s);
    let text_fg = s.theme.text_fg.clone();
    let dim_fg = s.theme.dim_fg.clone();
    let body_font_size = s.config.font_size as i32;
    let font_family = s.config.font_family.clone();
    let cursor_line_bg = s.theme.cursor_line_bg.clone();
    // Mirror the main card's per-line spacing rule (display_work): non-prose
    // works (plays/poems — the only works with translations) use tight 0px
    // spacing; only prose uses the configured `line_spacing`. Passing the config
    // value unconditionally made the overlay looser than the reading card, which
    // renders verse with no extra per-line spacing.
    let work_is_prose = s
        .current_work
        .as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(false);
    let line_spacing = if work_is_prose {
        s.config.line_spacing as i32
    } else {
        0
    };
    let label = synopsis_label(&s, div1, div2);

    // Cursor's work index, to pick the block to anchor on.
    let cursor_idx = s.work_line_for_buffer(s.current_line);

    s.translation_overlay.show(
        &label,
        &blocks,
        card_width,
        card_height,
        &text_fg,
        &dim_fg,
        body_font_size,
        &font_family,
        &cursor_line_bg,
        line_spacing,
        cursor_idx,
    );
    drop(s);

    true
}
