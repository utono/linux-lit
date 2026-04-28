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
            crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), val);
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
        SettingsChange::None => {}
    }
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
    crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), snap_cw);
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
    crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), cw);
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
    s.settings_overlay.update_displayed_values(ls, cw, tm, nm, ts, false);
}
