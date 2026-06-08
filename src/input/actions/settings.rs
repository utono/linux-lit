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
            crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), val, cc, s.translations_visible);
            s.config.column_width = val;
        }
        SettingsChange::TextMargins(val) => {
            let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
            let is_verse = !crate::db::line_types::is_prose_work(work_type);
            let verse_bump = if is_verse { crate::app::verse_left_offset(s.window.width(), s.config.column_width) } else { 0 };
            s.text_view.set_left_margin(val as i32 + verse_bump);
            s.text_view.set_right_margin(val as i32 + crate::config::EXTRA_RIGHT_MARGIN);
            s.config.text_margins = val;
            if s.dialogue_formatting_active {
                crate::app::apply_dialogue_formatting(&mut s);
            }
        }
        SettingsChange::Theme(theme) => {
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
            open_voice_picker(state);
        }
        SettingsChange::None => {}
    }
}

/// Open the voice picker from the settings overlay's Voice row. Fetches the
/// account's voices asynchronously (showing "Loading voices…" meanwhile),
/// keeps the settings overlay underneath, and switches input to the picker.
pub(crate) fn open_voice_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    {
        let s = state.borrow();
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

/// Cancel the voice picker: return to the settings overlay without changing the
/// preferred voice.
pub(crate) fn cancel_voice_picker(state: &Rc<RefCell<crate::app::AppState>>) {
    let mut s = state.borrow_mut();
    s.voice_picker.hide();
    s.input_mode = crate::app::InputMode::Settings;
}

/// Apply a theme to AppState: load CSS, update tag colors, write
/// .current_theme. Called from settings overlay's theme cycling and from
/// revert_to_snapshot.
pub(crate) fn apply_theme_to_state(state: &mut crate::app::AppState, theme: &crate::theme::Theme) {
    let css = crate::theme::generate_css(theme, &state.config.font_family, state.config.font_size);
    state.css_provider.load_from_string(&css);

    // Update dim tag foreground
    state.dim_tag.set_property("foreground", &theme.dim_fg);
    state.ab_dim_tag.set_property("foreground", &theme.dim_fg);
    state.translation_dim_tag.set_property("foreground", &theme.dim_fg);
    state.selection_tag.set_property(
        "background",
        if theme.is_light {
            "rgba(38, 109, 211, 0.15)"
        } else {
            "rgba(68, 138, 255, 0.25)"
        },
    );

    // Update vocab tag foreground
    state.vocab_tag.set_property("foreground", &theme.vocab_fg);

    // Update cursor line highlight from root_color
    state.cursor_line_tag.set_property("paragraph-background", &theme.cursor_line_bg);

    // Write .current_theme file
    let home = std::env::var("HOME").unwrap_or_default();
    let theme_path = std::path::PathBuf::from(&home)
        .join("utono/themes/.config/themes/.current_theme");
    let _ = std::fs::write(&theme_path, &theme.name);

    state.theme = theme.clone();

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
    crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), snap_cw, cc, s.translations_visible);
    let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let is_verse = !crate::db::line_types::is_prose_work(work_type);
    let verse_bump = if is_verse { crate::app::verse_left_offset(s.window.width(), snap_cw) } else { 0 };
    s.text_view.set_left_margin(snap_tm as i32 + verse_bump);
    s.text_view.set_right_margin(snap_tm as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    s.config.line_spacing = snap_ls;
    s.config.column_width = snap_cw;
    s.config.text_margins = snap_tm;
    s.config.navigation_mode = snap_nm;
    s.config.transition_style = snap_ts;
    s.config.show_cursor_line = snap_cl;
    if s.dialogue_formatting_active {
        crate::app::apply_dialogue_formatting(&mut s);
    }
    crate::input::navigation::update_highlight_only(&mut s);
    // Revert theme if changed
    if let Some(snap_theme) = s.settings_overlay.themes().get(snap_ti) {
        let snap_theme = snap_theme.clone();
        s.settings_overlay.set_theme_index(snap_ti);
        apply_theme_to_state(&mut s, &snap_theme);
    }
    s.settings_overlay.hide();
    s.input_mode = crate::app::InputMode::Reader;
}

/// Open the settings overlay, reading current config values and passing them
/// to the overlay's show() method. Called from the OpenSettingsOverlay action.
pub(crate) fn open_settings(state: &Rc<RefCell<crate::app::AppState>>) {
    let s = state.borrow();
    if !s.settings_overlay.is_visible() && !s.picker.is_visible() {
        s.gloss_overlay.hide();
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        let nm = s.config.navigation_mode;
        let ts = s.config.transition_style;
        let cl = s.config.show_cursor_line;
        let voice = crate::elevenlabs::voice_label_for_id(&s.config.elevenlabs_voice_id);
        drop(s);
        state.borrow_mut().settings_overlay.show(ls, cw, tm, nm, ts, cl, &voice);
        state.borrow_mut().input_mode = crate::app::InputMode::Settings;
    }
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
    crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), cw, cc, s.translations_visible);
    let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let is_verse = !crate::db::line_types::is_prose_work(work_type);
    let verse_bump = if is_verse { crate::app::verse_left_offset(s.window.width(), cw) } else { 0 };
    s.text_view.set_left_margin(tm as i32 + verse_bump);
    s.text_view.set_right_margin(tm as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    s.config.line_spacing = ls;
    s.config.column_width = cw;
    s.config.text_margins = tm;
    s.config.navigation_mode = nm;
    s.config.transition_style = ts;
    s.config.show_cursor_line = false;
    if s.dialogue_formatting_active {
        crate::app::apply_dialogue_formatting(&mut s);
    }
    crate::input::navigation::update_highlight_only(&mut s);
    // Reset does not touch the preferred voice; keep the row showing it.
    let voice = crate::elevenlabs::voice_label_for_id(&s.config.elevenlabs_voice_id);
    s.settings_overlay.update_displayed_values(ls, cw, tm, nm, ts, false, &voice);
}
