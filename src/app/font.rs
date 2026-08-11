use super::{AppState, TOP_SPACER_HEIGHT, SHOW_LINE_NUMBERS_TWO_COL};
use crate::app::layout::line_number_gutter_geometry;
use crate::logging::log;
use gtk4::prelude::{TextBufferExt, TextTagExt, TextViewExt, WidgetExt};

/// Keep top spacer at fixed TOP_SPACER_HEIGHT.
fn update_spacer_heights(state: &AppState) {
    state.top_spacer.set_height_request(TOP_SPACER_HEIGHT);
}

pub(crate) fn reapply_font(state: &AppState) {
    let tag_table = state.buffer.tag_table();
    // Remove old font tag if it exists
    if let Some(old) = tag_table.lookup("font-size") {
        tag_table.remove(&old);
    }
    // The translation overlay follows the main card font: same family and size
    // as the two-column reader, so changing the reader font (size via +/-, or
    // family via f/F) changes the overlay too. The interlinear paraphrase is
    // rendered 4pt smaller and italic (below), in the same family.
    let font_family = state.config.font_family.as_str();
    let font_size = state.config.font_size;
    let font_str = crate::ui::font_string(font_family, font_size as i32);
    let tag = gtk4::TextTag::builder()
        .name("font-size")
        .font(&font_str)
        .build();
    tag_table.add(&tag);
    let start = state.buffer.start_iter();
    let end = state.buffer.end_iter();
    state.buffer.apply_tag(&tag, &start, &end);
    // Also update CSS for consistency
    let divider_bottom = crate::input::scroll::two_column_divider_bottom_px(&state.text_view);
    let css = crate::theme::generate_css(
        &state.theme, &state.config.font_family, state.config.font_size, divider_bottom);
    state.css_provider.load_from_string(&css);
    // Keep translation tag in sync (italic, 4pt smaller) and ensure it
    // overrides the freshly re-added font-size tag.
    let trans_size = font_size.saturating_sub(4);
    // Comma form (see `ui::font_string`): a family ending in a style keyword
    // (Gentium *Book*, IBM Plex *Medium*…) would otherwise lose that word and
    // fall back to a default face. The style + size follow the comma.
    let trans_desc = pango::FontDescription::from_string(
        &format!("{}, Italic {}", font_family.trim(), trans_size),
    );
    state.translation_text_tag.set_font_desc(Some(&trans_desc));
    let highest = state.buffer.tag_table().size() - 1;
    state.translation_text_tag.set_priority(highest);
    state.authorship_tag.set_priority(highest.saturating_sub(1).max(1));
    // The gloss/synopsis and journal overlays follow the reader card's font
    // FAMILY and SIZE: synced here so a work load, a +/- size change, or f/F
    // family cycling keeps their body font matching the main card instead of
    // drifting to the fixed GLOSS_DEFAULT_FONT_FAMILY/SIZE.
    state.gloss_overlay.sync_reader_font(font_family, font_size as i32);
    state.journal_overlay.sync_reader_font(font_family, font_size as i32);
    log(&format!("FONT: reapply_font size={}pt via TextTag", state.config.font_size));
    update_spacer_heights(state);
}

pub(crate) fn rebuild_line_number_gutter(state: &mut AppState) {
    if let Some(old) = state.line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.text_view, old);
    }
    if let Some(old) = state.right_line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.right_view, old);
    }
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(true);
    // Line numbers are skipped in two-column mode (unless explicitly enabled),
    // matching the work-load gutter setup. Without this guard, rebuilding after
    // a font reapply (e.g. toggling translations off) re-added numbers in
    // two-column mode, which also ate column width and underfilled the right
    // column.
    // No verse line numbers in the translation overlay — the interleaved
    // original/translation lines make the right-gutter foliation noise.
    // No line numbers for a one-section-per-page work (sonnet_sequence): each
    // page is a single short poem with its own number heading, so the every-5th
    // foliation is noise.
    let show_numbers = (state.column_count() != 2 || SHOW_LINE_NUMBERS_TWO_COL)
        && !state.translations_visible
        && !state.one_section_per_page();
    if !is_prose && show_numbers {
        let base: Vec<Option<i64>> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx.and_then(|idx| {
                        state.current_work.as_ref()?.lines.get(idx).map(|l| l.line_in_div)
                    })
                })
                .collect()
        } else {
            state.current_work.as_ref()
                .map(|w| w.lines.iter().map(|l| Some(l.line_in_div)).collect())
                .unwrap_or_default()
        };
        let nums = if state.translations_visible && !state.translation_lines.is_empty() {
            let mut expanded = Vec::with_capacity(state.translation_lines.len());
            let mut orig_idx = 0;
            for &is_trans in &state.translation_lines {
                if is_trans {
                    expanded.push(None);
                } else {
                    expanded.push(base.get(orig_idx).copied().flatten());
                    orig_idx += 1;
                }
            }
            expanded
        } else {
            base
        };
        *state.line_numbers.borrow_mut() = nums;
        let (ln_width, ln_margin, ln_gap) = line_number_gutter_geometry(state.column_count());
        let renderer = crate::gutter::setup_line_number_gutter(
            &state.text_view,
            state.line_numbers.clone(),
            &state.theme.dim_fg,
            &state.config.font_family,
            state.config.font_size,
            ln_width,
            ln_margin,
        );
        state.text_view.set_right_margin(ln_gap);
        state.line_number_renderer = Some(renderer);
        let right_renderer = crate::gutter::setup_line_number_gutter(
            &state.right_view,
            state.line_numbers.clone(),
            &state.theme.dim_fg,
            &state.config.font_family,
            state.config.font_size,
            ln_width,
            ln_margin,
        );
        state.right_view.set_right_margin(ln_gap);
        state.right_line_number_renderer = Some(right_renderer);
    }
}

/// Adjust font size by delta, clamp to 8..=72, reapply CSS and repaginate.
/// Adjust the reader font size. The translation overlay follows this size
/// (reapply_font reads `config.font_size` whether or not the overlay is open),
/// so adjusting while the overlay is visible scales both the original lines and
/// the interlinear paraphrase.
pub fn adjust_font_size(state: &mut AppState, delta: i32) {
    let new_size = (state.config.font_size as i32 + delta).clamp(8, 72) as u32;
    if new_size == state.config.font_size {
        return;
    }
    state.config.font_size = new_size;
    reapply_font(state);
    rebuild_line_number_gutter(state);
    // Font metrics are in the page-table layout fingerprint: drop a
    // now-stale table (and reload a matching one) BEFORE resnapping, so the
    // resnap paginates with the correct engine.
    crate::input::page_table::revalidate_on_resize(state);
    crate::input::prose_pages::revalidate_prose_on_resize(state);
    // Mirror the resize tick (src/app/mod.rs): a font change invalidates line
    // heights, so any scheduled prose cross and the current `page_top_offset`
    // are stale — cancel the cross (it re-schedules on the next CursorSync if
    // still warranted) and let `resnap_prose_to_table` re-derive an on-grid
    // offset instead of leaving `resnap_page`'s line-top scroll paired with a
    // stale pixel offset (Minor #3, final-review.md).
    state.pending_prose_cross = None;
    crate::input::navigation::resnap_page(state);
    crate::input::prose_pages::resnap_prose_to_table(state);
    crate::input::navigation::invalidate_page_tops(state);
    crate::config::save(&state.config);
}

/// Reset font size to default (16pt).
pub fn reset_font_size(state: &mut AppState) {
    let default = 16u32;
    if state.config.font_size == default {
        return;
    }
    state.config.font_size = default;
    reapply_font(state);
    rebuild_line_number_gutter(state);
    crate::input::page_table::revalidate_on_resize(state);
    crate::input::prose_pages::revalidate_prose_on_resize(state);
    // See adjust_font_size: mirror the resize tick's cross-cancel + prose
    // resnap so a stale pending cross / page_top_offset doesn't survive the
    // font change (Minor #3, final-review.md).
    state.pending_prose_cross = None;
    crate::input::navigation::resnap_page(state);
    crate::input::prose_pages::resnap_prose_to_table(state);
    crate::input::navigation::invalidate_page_tops(state);
    crate::config::save(&state.config);
}

/// Cycle font family forward (f) or backward (F).
pub fn cycle_font(state: &mut AppState, forward: bool) {
    let cycle = crate::config::FONT_CYCLE;
    let current = &state.config.font_family;
    let idx = cycle.iter().position(|f| *f == current).unwrap_or(0);
    let next = if forward {
        (idx + 1) % cycle.len()
    } else {
        (idx + cycle.len() - 1) % cycle.len()
    };
    state.config.font_family = cycle[next].to_string();
    // Record the choice for THIS work, not as the global default. Both `F`
    // and `Ctrl+Shift+f` land here, so both directions of the cycle set the
    // per-work face. A work with no entry opens on the global default
    // (Charis); changing the font in one work never affects another.
    //
    // `font_family` still carries the live value so every downstream consumer
    // — reapply_font, the prose layout fingerprint, the translation overlay —
    // reads ONE field and needs no per-work awareness. `work_fonts` is the
    // durable record; `font_family` is the resolved current value.
    if let Some(w) = state.current_work.as_ref() {
        state
            .config
            .work_fonts
            .insert(w.abbrev.clone(), cycle[next].to_string());
    }
    reapply_font(state);
    crate::input::page_table::revalidate_on_resize(state);
    crate::input::prose_pages::revalidate_prose_on_resize(state);
    // See adjust_font_size: mirror the resize tick's cross-cancel + prose
    // resnap so a stale pending cross / page_top_offset doesn't survive the
    // font change (Minor #3, final-review.md).
    state.pending_prose_cross = None;
    crate::input::navigation::resnap_page(state);
    crate::input::prose_pages::resnap_prose_to_table(state);
    crate::input::navigation::invalidate_page_tops(state);
    crate::config::save(&state.config);
    log(&format!("FONT: cycled to {}", state.config.font_family));
    show_font_info(state);
}

/// Show current font info in the in-app chapter toast.
///
/// In-app toast rather than `notify-send`: the desktop notification depended on
/// an external daemon that is not guaranteed to be running or visible above a
/// fullscreen window, so the feedback silently vanished. The toast renders in
/// our own window and is always seen. Single line — name plus position.
pub fn show_font_info(state: &AppState) {
    let cycle = crate::config::FONT_CYCLE;
    let idx = cycle.iter().position(|f| *f == state.config.font_family).unwrap_or(0);
    let text = format!(
        "Font [{}/{}] {} {}pt",
        idx + 1,
        cycle.len(),
        state.config.font_family,
        state.config.font_size
    );
    crate::input::navigation::show_chapter_toast_secs(
        state,
        &text,
        crate::input::navigation::SETTINGS_TOAST_SECS,
    );
}
