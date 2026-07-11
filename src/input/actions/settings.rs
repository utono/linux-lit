use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

/// Apply a SettingsChange variant to AppState in-place. Called from the
/// settings overlay's h/l/j/k key handlers.
pub(crate) fn apply_settings_change(
    state: &Rc<RefCell<crate::app::AppState>>,
    change: crate::ui::settings_overlay::SettingsChange,
) {
    use crate::ui::settings_overlay::SettingsChange;
    let mut s = state.borrow_mut();
    match change {
        SettingsChange::LineSpacing(val) => {
            if s.dialogue_formatting_active {
                let tag_table = s.buffer.tag_table();
                if let Some(tag) = tag_table.lookup("speaker-gap") {
                    tag.set_property("pixels-above-lines", val.max(1) as i32 * 5);
                }
            } else {
                s.text_view.set_pixels_above_lines((val as i32).max(0));
                s.text_view.set_pixels_below_lines((val as i32).max(0));
            }
            s.config.line_spacing = val;
        }
        SettingsChange::ColumnWidth(val) => {
            let cc = s.column_count();
            // Store first so the prose floor (effective_column_width = max of
            // the 78-char measure and the configured width) sees the new value.
            s.config.column_width = val;
            let cw = crate::app::layout::effective_column_width(&s);
            crate::app::layout::apply_card_sizing(&s.content_hbox, s.window.width(), cw, cc, s.translations_visible, s.chat_layout_open);
        }
        SettingsChange::TextMargins(val) => {
            let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
            let is_verse = !crate::db::line_types::is_prose_work(work_type);
            let verse_bump = if is_verse { crate::app::layout::verse_left_offset(s.window.width(), s.config.column_width) } else { 0 };
            s.text_view.set_left_margin(val as i32 + verse_bump);
            s.text_view.set_right_margin(val as i32 + crate::config::EXTRA_RIGHT_MARGIN);
            s.config.text_margins = val;
            if s.dialogue_formatting_active {
                crate::app::formatting::apply_dialogue_formatting(&mut s);
            }
        }
        SettingsChange::Theme(theme) => {
            let v = s.config.root_variant_for(&theme.name);
            let theme = if v == 0 {
                theme
            } else {
                Box::new(crate::theme::load_theme_with_fallback(&theme.name, v))
            };
            apply_theme_to_state(&mut s, &theme);
        }
        SettingsChange::Navigation(mode) => {
            s.config.navigation_mode = mode;
        }
        SettingsChange::Transition(style) => {
            s.config.transition_style = style;
        }
        SettingsChange::CursorLine(val) => {
            s.config.show_cursor_line = val;
            crate::input::navigation::update_highlight_only(&mut s);
        }
        SettingsChange::OpenVoicePicker => {
            drop(s);
            open_voice_picker(state, crate::app::VoicePickerOrigin::Settings);
        }
        SettingsChange::None => {}
    }
}

/// Open the voice picker from the settings overlay's Voice row. Fetches the
/// account's voices asynchronously (showing "Loading voices…" meanwhile),
/// keeps the settings overlay underneath, and switches input to the picker.
pub(crate) fn open_voice_picker(
    state: &Rc<RefCell<crate::app::AppState>>,
    origin: crate::app::VoicePickerOrigin,
) {
    {
        let mut s = state.borrow_mut();
        s.voice_picker_origin = origin;
        // Seed ✓ badges with the current gloss's associated voices (gloss origin).
        let assoc: Vec<String> = if origin != crate::app::VoicePickerOrigin::Settings {
            let gid = s.gloss_list.get(s.gloss_index).map(|g| g.gloss_id);
            match (gid, crate::db::queries::open_db()) {
                (Some(gid), Ok(conn)) => crate::db::queries::get_gloss_voices(&conn, gid)
                    .into_iter().map(|(vid, _)| vid).collect(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        s.voice_picker.set_associated(assoc);
        s.voice_picker.set_status("Loading voices\u{2026}");
        s.voice_picker.show();
    }
    state.borrow_mut().input_mode = crate::app::InputMode::VoicePicker;

    // Fetch voices off the GTK main thread via the Tokio runtime.
    let tokio_handle = state.borrow().tokio_handle.clone();
    let state_for_result = Rc::clone(state);
    gtk4::glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move { crate::elevenlabs::list_voices().await })
            .await;
        match result {
            Ok(Ok(voices)) => {
                // The account returns dozens of generic premade voices; the
                // picker should only offer the user's custom narration voices.
                let voices: Vec<_> = voices
                    .into_iter()
                    .filter(|v| crate::elevenlabs::is_custom_voice(&v.voice_id))
                    .collect();
                let s = state_for_result.borrow();
                // Ignore if the user already closed the picker.
                if s.input_mode == crate::app::InputMode::VoicePicker {
                    if voices.is_empty() {
                        s.voice_picker.set_status("No voices found");
                    } else {
                        drop(s);
                        state_for_result.borrow_mut().voice_picker.set_voices(voices);
                    }
                }
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                if s.input_mode == crate::app::InputMode::VoicePicker {
                    s.voice_picker.set_status(&format!("Error: {}", e));
                }
            }
            Err(e) => {
                crate::log_fmt!("VOICE: list_voices join error: {}", e);
                let s = state_for_result.borrow();
                if s.input_mode == crate::app::InputMode::VoicePicker {
                    s.voice_picker.set_status("Error loading voices");
                }
            }
        }
    });
}

/// Confirm the voice picker selection: persist the chosen voice id, refresh the
/// settings Voice row, and return to the settings overlay.
pub(crate) fn confirm_voice_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let selected = state.borrow().voice_picker.selected_voice();
    let origin = state.borrow().voice_picker_origin;
    match origin {
        crate::app::VoicePickerOrigin::Settings => {
            let mut s = state.borrow_mut();
            s.voice_picker.hide();
            if let Some((voice_id, name, _free)) = selected {
                s.config.elevenlabs_voice_id = voice_id.clone();
                crate::config::save(&s.config);
                // Show the live name from the picker list on the settings Voice row.
                s.settings_overlay.set_voice_label(&name);
                crate::log_fmt!("VOICE: preferred voice set to {} ({})", name, voice_id);
            }
            // Return to the settings overlay (still visible underneath).
            s.input_mode = crate::app::InputMode::Settings;
        }
        // GlossOverlay: pressing v in the overlay — toggle voice ASSOCIATION.
        crate::app::VoicePickerOrigin::GlossOverlay => {
            let gloss_id = {
                let s = state.borrow();
                s.gloss_list.get(s.gloss_index).map(|g| g.gloss_id)
            };
            state.borrow().voice_picker.hide();
            if let (Some((voice_id, name, _free)), Some(gid)) = (selected, gloss_id) {
                let model = crate::elevenlabs::OP_MODEL_ID.to_string();
                match crate::db::queries::open_db_rw() {
                    Ok(conn) => {
                        let added =
                            crate::db::queries::toggle_gloss_voice(&conn, gid, &voice_id, &model);
                        crate::log_fmt!(
                            "VOICE: gloss {} {} voice {} ({})",
                            gid, if added { "associated" } else { "removed" }, voice_id, name
                        );
                        crate::input::actions::gloss::voice_picker_toast(
                            state, if added { "Associated" } else { "Removed" }, &name,
                        );
                    }
                    Err(e) => {
                        crate::log_fmt!("VOICE: could not open db to toggle gloss voice: {}", e);
                        crate::input::actions::gloss::voice_picker_toast(
                            state, "Could not update", &name,
                        );
                    }
                }
            }
            state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
        }
        // GlossPlay: opened by R on a Source block — set the picked voice as the
        // gloss's ACTIVE voice and play the source verse (pausing MPV first).
        crate::app::VoicePickerOrigin::GlossPlay => {
            let gloss_id = {
                let s = state.borrow();
                s.gloss_list.get(s.gloss_index).map(|g| g.gloss_id)
            };
            // The Source block index to play (R is only reachable on a Source block).
            let source_index = {
                let s = state.borrow();
                match s.gloss_overlay.current_block() {
                    Some((crate::ui::gloss_block::BlockKind::Source, idx)) => Some(idx),
                    _ => None,
                }
            };
            state.borrow().voice_picker.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;

            if let (Some((voice_id, name, _free)), Some(gid)) = (selected, gloss_id) {
                let model = crate::elevenlabs::OP_MODEL_ID.to_string();
                match crate::db::queries::open_db_rw() {
                    Ok(conn) => {
                        // Associate the voice if it isn't already (do NOT blindly
                        // toggle — that would REMOVE an already-associated voice).
                        let existing = crate::db::queries::get_gloss_voices(&conn, gid);
                        let already = existing.iter().any(|(vid, _)| vid == &voice_id);
                        if !already {
                            let _ = crate::db::queries::toggle_gloss_voice(
                                &conn, gid, &voice_id, &model,
                            );
                        }
                        // Set this voice as the active one: its index in the re-read list.
                        let voices = crate::db::queries::get_gloss_voices(&conn, gid);
                        if let Some(pos) = voices.iter().position(|(vid, _)| vid == &voice_id) {
                            state.borrow_mut().gloss_active_voice = pos;
                        }
                        crate::log_fmt!(
                            "VOICE: gloss {} active voice {} ({})", gid, voice_id, name
                        );
                        crate::input::actions::gloss::voice_picker_toast(state, "Voice", &name);
                    }
                    Err(e) => {
                        // Match the GlossOverlay arm: log + toast so a failed pick
                        // isn't silently played in the previously-active voice.
                        crate::log_fmt!(
                            "VOICE: could not open db to set gloss active voice: {}", e
                        );
                        crate::input::actions::gloss::voice_picker_toast(
                            state, "Could not update", &name,
                        );
                    }
                }
            }

            // Play the source verse in the now-active voice (pauses MPV first).
            if let Some(idx) = source_index {
                crate::input::actions::gloss::play_source_tts_pausing_mpv(state, idx);
            }
        }
    }
}

/// Cancel the voice picker: return to the origin (settings or gloss overlay)
/// without changing anything.
pub(crate) fn cancel_voice_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let origin = state.borrow().voice_picker_origin;
    let mut s = state.borrow_mut();
    s.voice_picker.hide();
    s.input_mode = match origin {
        crate::app::VoicePickerOrigin::Settings => crate::app::InputMode::Settings,
        // GlossPlay routes back like GlossOverlay until the later task refines it.
        crate::app::VoicePickerOrigin::GlossOverlay
        | crate::app::VoicePickerOrigin::GlossPlay => crate::app::InputMode::GlossOverlay,
    };
}

/// Apply a theme to AppState: load CSS, update tag colors, persist
/// config.theme. Called from settings overlay's theme cycling and from
/// revert_to_snapshot.
pub(crate) fn apply_theme_to_state(state: &mut crate::app::AppState, theme: &crate::theme::Theme) {
    let css = crate::theme::generate_css(theme, &state.config.font_family, state.config.font_size);
    state.css_provider.load_from_string(&css);

    // Update dim tag foreground
    state.dim_tag.set_property("foreground", &theme.dim_fg);
    state.ab_dim_tag.set_property("foreground", &theme.dim_fg);
    state.translation_dim_tag.set_property("foreground", &theme.dim_fg);
    let selection_bg = crate::theme::selection_bg(theme);
    state.selection_tag.set_property("background", selection_bg);
    // The `<hi>` marker in the overlays uses the SAME selection blue, so a theme
    // change must repaint both overlays' highlight color (responsive to dwl theme).
    state.gloss_overlay.set_highlight_color(selection_bg);
    state.journal_overlay.set_highlight_color(selection_bg);
    // Page-marker glyph color follows theme dim foreground.
    state.gloss_overlay.set_marker_color(&theme.dim_fg);
    state.journal_overlay.set_marker_color(&theme.dim_fg);
    state.gloss_overlay.set_panel_color(&theme.overlay_panel_bg);
    state.journal_overlay.set_panel_color(&theme.overlay_panel_bg);
    state.journal_overlay.set_bar_color(&theme.root_color);

    // Update vocab tag foreground
    state.vocab_tag.set_property("foreground", &theme.vocab_fg);

    // Update reader-gloss tints to the new theme's contrast-guarded gloss colors.
    state.reader_gloss_tag.set_property("foreground", &theme.reader_gloss);
    state.reader_gloss_cursor_tag.set_property("foreground", &theme.reader_gloss_cursor);

    // Update cursor line highlight from root_color
    state.cursor_line_tag.set_property("paragraph-background", &theme.cursor_line_bg);
    state.phrase_tag.set_property("background", &theme.phrase_highlight_bg);
    state
        .vocab_sentence_tag
        .set_property("background", &theme.vocab_sentence_bg());

    // Persist the per-app theme. linux-lit no longer reads or writes the
    // system-wide .current_theme — its theme is independent (default
    // kindle-sepia). config::save is atomic and a no-op under
    // LIT_HEADLESS_TEST.
    state.config.theme = Some(theme.name.clone());
    crate::config::save(&state.config);

    state.theme = theme.clone();

    // Resync the settings overlay's own theme_index to whatever theme was just
    // applied. Without this, a theme change made OUTSIDE the overlay's own
    // row-0 cycling (Alt+t/Alt+Shift+T `cycle_theme`, or the SIGUSR1 handler)
    // leaves the overlay's index stale: opening settings shows the old theme
    // name, and Escape's `revert_to_snapshot` re-applies + persists that STALE
    // theme, silently undoing the change. This is a no-op for the overlay's
    // own cycle/revert paths — they already set the index to what it becomes
    // here.
    if let Some(idx) = state.settings_overlay.themes().iter().position(|t| t.name == theme.name) {
        state.settings_overlay.set_theme_index(idx);
    }

    // The gutter sign renderer captures its color (theme.sign_fg) by value in a
    // closure at build time, so a live theme switch leaves the signs painted in
    // the OLD theme's color. Rebuild the gutter so the signs pick up the new
    // theme's sign_fg (and dim line-number color). Only worth it when the sign
    // column is actually built.
    if state.gutter_renderer.is_some() {
        crate::app::setup_gutter(state);
    }

    crate::logging::log(&format!("SETTINGS: theme changed to {}", theme.display_name));
}

/// Revert AppState to the snapshot taken when the settings overlay opened,
/// then hide the overlay. Called from Escape in settings overlay.
pub(crate) fn revert_to_snapshot(state: &Rc<RefCell<crate::app::AppState>>) {
    let (snap_ls, snap_cw, snap_tm, snap_ti, snap_nm, snap_ts, snap_cl) = state.borrow().settings_overlay.snapshot();
    let mut s = state.borrow_mut();
    if s.dialogue_formatting_active {
        let tag_table = s.buffer.tag_table();
        if let Some(tag) = tag_table.lookup("speaker-gap") {
            tag.set_property("pixels-above-lines", snap_ls.max(1) as i32 * 5);
        }
    } else {
        s.text_view.set_pixels_above_lines((snap_ls as i32).max(0));
        s.text_view.set_pixels_below_lines((snap_ls as i32).max(0));
    }
    let cc = s.column_count();
    crate::app::layout::apply_card_sizing(&s.content_hbox, s.window.width(), snap_cw, cc, s.translations_visible, s.chat_layout_open);
    let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let is_verse = !crate::db::line_types::is_prose_work(work_type);
    let verse_bump = if is_verse { crate::app::layout::verse_left_offset(s.window.width(), snap_cw) } else { 0 };
    s.text_view.set_left_margin(snap_tm as i32 + verse_bump);
    s.text_view.set_right_margin(snap_tm as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    s.config.line_spacing = snap_ls;
    s.config.column_width = snap_cw;
    s.config.text_margins = snap_tm;
    s.config.navigation_mode = snap_nm;
    s.config.transition_style = snap_ts;
    s.config.show_cursor_line = snap_cl;
    if s.dialogue_formatting_active {
        crate::app::formatting::apply_dialogue_formatting(&mut s);
    }
    crate::input::navigation::update_highlight_only(&mut s);
    // Revert theme if changed
    if let Some(snap_theme) = s.settings_overlay.themes().get(snap_ti) {
        let snap_theme = snap_theme.clone();
        let v = s.config.root_variant_for(&snap_theme.name);
        let snap_theme = if v == 0 {
            snap_theme
        } else {
            crate::theme::load_theme_with_fallback(&snap_theme.name, v)
        };
        s.settings_overlay.set_theme_index(snap_ti);
        apply_theme_to_state(&mut s, &snap_theme);
    }
    drop(s);
    // Return to wherever settings was opened from (reader, or the gloss /
    // synopsis overlay still visible underneath).
    close_settings_to_return_mode(state);
}

/// Open the settings overlay, reading current config values and passing them
/// to the overlay's show() method. Called from the OpenSettingsOverlay action.
pub(crate) fn open_settings(state: &Rc<RefCell<crate::app::AppState>>) {
    let s = state.borrow();
    if !s.settings_overlay.is_visible() && !s.library_picker.is_visible() {
        s.gloss_overlay.hide();
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        let nm = s.config.navigation_mode;
        let ts = s.config.transition_style;
        let cl = s.config.show_cursor_line;
        let voice = crate::elevenlabs::voice_label_for_id(&s.config.elevenlabs_voice_id);
        drop(s);
        let mut s = state.borrow_mut();
        s.settings_return_mode = crate::app::InputMode::Reader;
        s.settings_overlay.show(ls, cw, tm, nm, ts, cl, &voice);
        s.input_mode = crate::app::InputMode::Settings;
    }
}

/// Open settings from the gloss or synopsis overlay (`Ctrl+,`). Unlike
/// `open_settings`, the gloss-overlay widget is left VISIBLE underneath the
/// settings scrim, and `settings_return_mode` records which overlay to restore
/// when settings closes (so Enter/Escape return to the gloss/synopsis overlay,
/// not the reader). `return_mode` must be `GlossOverlay` or `SynopsisOverlay`.
pub(crate) fn open_settings_from_overlay(
    state: &Rc<RefCell<crate::app::AppState>>,
    return_mode: crate::app::InputMode,
) {
    let s = state.borrow();
    if s.settings_overlay.is_visible() || s.library_picker.is_visible() {
        return;
    }
    // The gloss overlay stays VISIBLE behind the settings overlay: the settings
    // scrim + card are add_overlay panels on the outermost overlay (see app.rs),
    // so they render ABOVE the gloss overlay. We just record the return mode and
    // show settings on top. close_settings_to_return_mode restores the mode.
    let ls = s.config.line_spacing;
    let cw = s.config.column_width;
    let tm = s.config.text_margins;
    let nm = s.config.navigation_mode;
    let ts = s.config.transition_style;
    let cl = s.config.show_cursor_line;
    let voice = crate::elevenlabs::voice_label_for_id(&s.config.elevenlabs_voice_id);
    drop(s);
    let mut s = state.borrow_mut();
    s.settings_return_mode = return_mode;
    s.settings_overlay.show(ls, cw, tm, nm, ts, cl, &voice);
    s.input_mode = crate::app::InputMode::Settings;
}

/// Close the settings overlay and return to wherever it was opened from
/// (`settings_return_mode`): the reader, or the gloss/synopsis overlay left
/// visible underneath. Shared by Confirm (Enter) and the revert path.
pub(crate) fn close_settings_to_return_mode(state: &Rc<RefCell<crate::app::AppState>>) {
    let mut s = state.borrow_mut();
    s.settings_overlay.hide();
    let return_mode = s.settings_return_mode;
    match return_mode {
        crate::app::InputMode::GlossOverlay | crate::app::InputMode::SynopsisOverlay => {
            // The gloss overlay stayed visible behind settings — just restore
            // the input mode so its key handler takes over again.
            s.input_mode = return_mode;
        }
        _ => {
            s.input_mode = crate::app::InputMode::Reader;
        }
    }
    s.settings_return_mode = crate::app::InputMode::Reader;
}

/// Reset AppState to default settings. Called from `r` in settings overlay.
pub(crate) fn reset_to_defaults(state: &Rc<RefCell<crate::app::AppState>>) {
    let mut s = state.borrow_mut();
    let ls = crate::config::DEFAULT_LINE_SPACING;
    let cw = crate::config::DEFAULT_COLUMN_WIDTH;
    let tm = crate::config::DEFAULT_TEXT_MARGINS;
    let nm = crate::config::NavigationMode::default();
    let ts = crate::config::TransitionStyle::default();
    if s.dialogue_formatting_active {
        let tag_table = s.buffer.tag_table();
        if let Some(tag) = tag_table.lookup("speaker-gap") {
            tag.set_property("pixels-above-lines", ls.max(1) as i32 * 5);
        }
    } else {
        s.text_view.set_pixels_above_lines((ls as i32).max(0));
        s.text_view.set_pixels_below_lines((ls as i32).max(0));
    }
    let cc = s.column_count();
    crate::app::layout::apply_card_sizing(&s.content_hbox, s.window.width(), cw, cc, s.translations_visible, s.chat_layout_open);
    let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let is_verse = !crate::db::line_types::is_prose_work(work_type);
    let verse_bump = if is_verse { crate::app::layout::verse_left_offset(s.window.width(), cw) } else { 0 };
    s.text_view.set_left_margin(tm as i32 + verse_bump);
    s.text_view.set_right_margin(tm as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    s.config.line_spacing = ls;
    s.config.column_width = cw;
    s.config.text_margins = tm;
    s.config.navigation_mode = nm;
    s.config.transition_style = ts;
    s.config.show_cursor_line = false;
    if s.dialogue_formatting_active {
        crate::app::formatting::apply_dialogue_formatting(&mut s);
    }
    crate::input::navigation::update_highlight_only(&mut s);
    // Reset does not touch the preferred voice; keep the row showing it.
    let voice = crate::elevenlabs::voice_label_for_id(&s.config.elevenlabs_voice_id);
    s.settings_overlay.update_displayed_values(ls, cw, tm, nm, ts, false, &voice);
}

/// Index of the next theme in a cycle list of length `len`.
/// `current` = position of the active theme in the list, if it is in the
/// list at all; when it is not (e.g. set via the settings overlay), both
/// directions jump to the first entry.
pub(crate) fn next_cycle_index(len: usize, current: Option<usize>, forward: bool) -> usize {
    match current {
        Some(i) if forward => (i + 1) % len,
        Some(i) => (i + len - 1) % len,
        None => 0,
    }
}

/// Alt+t / Alt+Shift+T: cycle through the curated theme list in config.
pub(crate) fn cycle_theme(state: &Rc<RefCell<crate::app::AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    let cycle = s.config.theme_cycle();
    if cycle.is_empty() {
        return;
    }
    let current = cycle.iter().position(|t| *t == s.theme.name);
    let next = next_cycle_index(cycle.len(), current, forward);
    let variant = s.config.root_variant_for(&cycle[next]);
    let theme = crate::theme::load_theme_with_fallback(&cycle[next], variant);
    apply_theme_to_state(&mut s, &theme);
    // Desktop notification, mirroring the font-cycle pattern (font.rs). The
    // synchronous hint replaces a still-visible previous theme toast instead
    // of stacking.
    let position = format!("{}/{}", next + 1, cycle.len());
    let _ = std::process::Command::new("notify-send")
        .args(["-t", "1500", "-h", "string:x-canonical-private-synchronous:linux-lit-theme",
               &format!("Theme [{}]", position), &s.theme.display_name])
        .spawn();
}

/// Ctrl+t / Ctrl+Shift+T: cycle the current theme's ROOT-color variant
/// forward (0 → 1 → ... → 4 → 0) or backward. The index persists per theme
/// (config.root_variants); the theme is re-resolved so root_color (and its
/// derivative scrim_bg) follow the new root while the card background and
/// reading tints stay pinned. See
/// docs/plans/2026-07-10-bg-variant-cycling-design.md.
pub(crate) fn cycle_root_variant(state: &Rc<RefCell<crate::app::AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    let name = s.theme.name.clone();
    let count = crate::theme::ROOT_VARIANT_COUNT;
    let next = if forward {
        (s.theme.root_variant + 1) % count
    } else {
        (s.theme.root_variant + count - 1) % count
    };
    // Insert BEFORE apply_theme_to_state — it saves the config snapshot.
    s.config.root_variants.insert(name.clone(), next);
    let theme = crate::theme::load_theme_with_fallback(&name, next);
    apply_theme_to_state(&mut s, &theme);
    let _ = std::process::Command::new("notify-send")
        .args(["-t", "1500", "-h",
               "string:x-canonical-private-synchronous:linux-lit-theme",
               &format!("Root [{}/{}]", next + 1, crate::theme::ROOT_VARIANT_COUNT),
               &s.theme.root_color])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::next_cycle_index;

    #[test]
    fn cycle_forward_wraps() {
        assert_eq!(next_cycle_index(4, Some(0), true), 1);
        assert_eq!(next_cycle_index(4, Some(3), true), 0);
    }

    #[test]
    fn cycle_backward_wraps() {
        assert_eq!(next_cycle_index(4, Some(0), false), 3);
        assert_eq!(next_cycle_index(4, Some(2), false), 1);
    }

    #[test]
    fn current_not_in_list_jumps_to_first() {
        assert_eq!(next_cycle_index(4, None, true), 0);
        assert_eq!(next_cycle_index(4, None, false), 0);
    }
}
