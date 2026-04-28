use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::input::navigation;

#[derive(Default)]
pub struct KeyState {
    pub pending_g: bool,
    pub pending_ctrl_slash: bool,
}

/// Handle a key press. Returns true if consumed.
pub fn handle_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    crate::logging::log(&format!("KEY: name={} ctrl={} shift={} alt={}", key_name, is_ctrl, is_shift, is_alt));
    let picker_visible = state.borrow().picker.is_visible();

    // Ctrl+n/Ctrl+p navigate picker list when visible
    if picker_visible && is_ctrl {
        match key_name {
            "n" => {
                state.borrow().picker.move_selection(1);
                return true;
            }
            "p" => {
                state.borrow().picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
    }

    // Ctrl+Shift+p: open concordance word picker (current work's vocab words only)
    if is_ctrl && is_shift && key_name == "P" {
        let words: Vec<(String, usize)> = {
            let s = state.borrow();
            let mut seen = std::collections::BTreeSet::new();
            for m in &s.vocab_matches {
                seen.insert(m.word.clone());
            }
            seen.into_iter().map(|w| (w, 0)).collect()
        };
        state.borrow_mut().concordance_word_picker.set_words(words);
        state.borrow().concordance_word_picker.show();
        return true;
    }

    // Ctrl+Alt+p: open concordance occurrence list
    if is_ctrl && is_alt && key_name == "p" {
        let s = state.borrow();
        if let Some(conc) = &s.concordance_state {
            s.concordance_list_picker
                .show(&conc.occurrences, conc.current_index);
        }
        return true;
    }

    // Ctrl+p: open picker when hidden (also clears concordance mode)
    if is_ctrl && key_name == "p" && !picker_visible
        && !state.borrow().bookmark_picker.is_visible()
        && !state.borrow().media_picker.is_visible()
    {
        {
            let mut s = state.borrow_mut();
            s.concordance_state = None;
            s.concordance_bar.hide();
        }
        state.borrow().correction_overlay.hide();
        state.borrow_mut().picker.show_prepare();
        state.borrow().picker.show_finish();
        return true;
    }

    // Picker-visible keys
    if picker_visible {
        match key_name {
            "Escape" => {
                let level = state.borrow().picker.level().clone();
                match level {
                    crate::ui::library_picker::PickerLevel::Works(_) => {
                        state.borrow_mut().picker.go_back_to_authors();
                        state.borrow().picker.refresh_after_level_change();
                    }
                    crate::ui::library_picker::PickerLevel::Authors => {
                        state.borrow().picker.hide();
                    }
                }
                return true;
            }
            "Return" => {
                let level = state.borrow().picker.level().clone();
                match level {
                    crate::ui::library_picker::PickerLevel::Authors => {
                        let selected_name = state
                            .borrow()
                            .picker
                            .list_box()
                            .selected_row()
                            .map(|r| r.widget_name().to_string());
                        if let Some(name) = selected_name {
                            if name.starts_with("author:") {
                                let author = name.trim_start_matches("author:").to_string();
                                state.borrow_mut().picker.enter_author(&author);
                                state.borrow().picker.refresh_after_level_change();
                            } else {
                                crate::input::actions::pickers::load_selected_work(state, tokio_handle);
                            }
                        }
                        return true;
                    }
                    crate::ui::library_picker::PickerLevel::Works(_) => {
                        crate::input::actions::pickers::load_selected_work(state, tokio_handle);
                        return true;
                    }
                }
            }
            "BackSpace" => {
                let level = state.borrow().picker.level().clone();
                if let crate::ui::library_picker::PickerLevel::Works(_) = level {
                    let text = state.borrow().picker.search_entry().text().to_string();
                    if text.is_empty() {
                        state.borrow_mut().picker.go_back_to_authors();
                        state.borrow().picker.refresh_after_level_change();
                        return true;
                    }
                }
                return false;
            }
            "Down" => {
                state.borrow().picker.move_selection(1);
                return true;
            }
            "Up" => {
                state.borrow().picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
        return false;
    }

    // Bookmark picker
    let bookmark_picker_visible = state.borrow().bookmark_picker.is_visible();

    if bookmark_picker_visible && is_ctrl {
        match key_name {
            "n" => {
                state.borrow().bookmark_picker.move_selection(1);
                return true;
            }
            "p" => {
                state.borrow().bookmark_picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
    }

    if bookmark_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().bookmark_picker.hide();
                return true;
            }
            "Return" => {
                let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
                if let Some(lm_id) = selected_id {
                    {
                        let s = state.borrow();
                        s.bookmark_picker.hide();
                    }
                    let mut s = state.borrow_mut();
                    let buffer_line = if let Some(ref lm) = s.line_map {
                        s.current_work.as_ref().and_then(|w| {
                            let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                            Some(lm.work_to_buffer[work_idx])
                        })
                    } else {
                        s.current_work.as_ref().and_then(|w| {
                            w.lines.iter().position(|l| l.id == lm_id)
                        })
                    };
                    if let Some(bl) = buffer_line {
                        navigation::jump_to_line(&mut s, bl);
                    }
                }
                return true;
            }
            "Delete" | "d" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Delete" || !is_search_focused {
                    crate::input::actions::pickers::delete_bookmark(state, tokio_handle);
                    return true;
                }
            }
            "Down" | "j" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Down" || !is_search_focused {
                    state.borrow().bookmark_picker.move_selection(1);
                    return true;
                }
            }
            "Up" | "k" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Up" || !is_search_focused {
                    state.borrow().bookmark_picker.move_selection(-1);
                    return true;
                }
            }
            _ => {}
        }
        return false;
    }

    // Media picker
    let media_picker_visible = state.borrow().media_picker.is_visible();

    // Ctrl+n/Ctrl+p navigate media picker list when visible
    if media_picker_visible && is_ctrl {
        match key_name {
            "n" => {
                state.borrow().media_picker.move_selection(1);
                return true;
            }
            "p" => {
                state.borrow().media_picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
    }

    if media_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().media_picker.hide();
                return true;
            }
            "Return" => {
                crate::input::actions::pickers::confirm_media_selection(state, tokio_handle);
                return true;
            }
            "Down" | "j" => {
                let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
                if key_name == "Down" || !is_search_focused {
                    state.borrow().media_picker.move_selection(1);
                    return true;
                }
            }
            "Up" | "k" => {
                let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
                if key_name == "Up" || !is_search_focused {
                    state.borrow().media_picker.move_selection(-1);
                    return true;
                }
            }
            "p" => {
                let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
                if !is_search_focused {
                    let selected_id = state.borrow().media_picker.selected_media_id();
                    let abbrev = state
                        .borrow()
                        .current_work
                        .as_ref()
                        .map(|w| w.abbrev.clone());
                    if let (Some(media_id), Some(abbrev)) = (selected_id, abbrev) {
                        match crate::db::queries::open_db_rw() {
                            Ok(conn) => {
                                if let Err(e) =
                                    crate::db::queries::set_media_priority(&conn, &abbrev, media_id)
                                {
                                    crate::logging::log(&format!(
                                        "MEDIA: set_media_priority failed: {}",
                                        e
                                    ));
                                } else {
                                    let max_pri: i64 = conn
                                        .query_row(
                                            "SELECT priority FROM work_media_associations \
                                             WHERE work_abbrev = ?1 AND media_id = ?2",
                                            rusqlite::params![&abbrev, media_id],
                                            |row| row.get(0),
                                        )
                                        .unwrap_or(20);
                                    state
                                        .borrow_mut()
                                        .media_picker
                                        .set_default(media_id, max_pri);
                                    crate::logging::log(&format!(
                                        "MEDIA: set default media_id={} for {}",
                                        media_id, abbrev
                                    ));
                                }
                            }
                            Err(e) => {
                                crate::logging::log(&format!(
                                    "MEDIA: open_db_rw failed: {}",
                                    e
                                ));
                            }
                        }
                    }
                    return true;
                }
            }
            _ => {}
        }
        return false;
    }

    // Ctrl+d: toggle debug logging
    if is_ctrl && key_name == "d" {
        let enabled = !crate::logging::debug_mode();
        crate::logging::set_debug_mode(enabled);
        crate::logging::log_always(&format!("DEBUG_MODE: {}", if enabled { "on" } else { "off" }));
        state.borrow().debug_icon.set_visible(enabled);
        return true;
    }

    // Ctrl+f: page forward
    if is_ctrl && key_name == "f" {
        navigation::page_forward(&mut state.borrow_mut());
        return true;
    }

    // Ctrl+b: page backward
    if is_ctrl && key_name == "b" {
        navigation::page_backward(&mut state.borrow_mut());
        return true;
    }

    // Ctrl+y: copy line_mapping.id and media_files.id to clipboard
    if is_ctrl && key_name == "y" {
        let s = state.borrow();
        let lm_id = s.line_mapping_id_for_buffer(s.current_line);
        let media_id = s.media_id;
        drop(s);
        let clip = match (lm_id, media_id) {
            (Some(l), Some(m)) => format!("{} {}", l, m),
            (Some(l), None) => format!("{}", l),
            (None, Some(m)) => format!("- {}", m),
            (None, None) => return true,
        };
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(clip.as_bytes());
            }
            let _ = child.wait();
        }
        crate::logging::log(&format!("CLIPBOARD: copied {}", clip));
        return true;
    }

    // Ctrl+Shift+M: open media picker
    if is_ctrl && is_shift && key_name == "M" {
        crate::input::actions::pickers::open_media_picker(state, tokio_handle);
        return true;
    }

    // Ctrl+m: open bookmark picker
    if is_ctrl && !is_shift && key_name == "m" {
        crate::input::actions::pickers::open_bookmark_picker(state, tokio_handle);
        return true;
    }

    // Settings overlay
    let settings_visible = state.borrow().settings_overlay.is_visible();

    // Ctrl+,: toggle settings overlay
    if is_ctrl && key_name == "comma" && !settings_visible && !picker_visible {
        state.borrow().correction_overlay.hide();
        let s = state.borrow();
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        let nm = s.config.navigation_mode;
        let ts = s.config.transition_style;
        let cl = s.config.show_cursor_line;
        drop(s);
        state.borrow_mut().settings_overlay.show(ls, cw, tm, nm, ts, cl);
        return true;
    }

    // Settings overlay visible — route keys
    if settings_visible {
        match key_name {
            "Escape" => {
                // Revert to snapshot values
                let (snap_ls, snap_cw, snap_tm, snap_ti, snap_nm, snap_ts, snap_cl) = state.borrow().settings_overlay.snapshot();
                {
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
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &snap_theme);
                    }
                    s.settings_overlay.hide();
                }
                return true;
            }
            "Return" => {
                // Confirm: persist config and close
                {
                    let s = state.borrow_mut();
                    crate::config::save(&s.config);
                    s.settings_overlay.hide();
                }
                return true;
            }
            "j" | "Down" => {
                state.borrow_mut().settings_overlay.move_selection(1);
                return true;
            }
            "k" | "Up" => {
                state.borrow_mut().settings_overlay.move_selection(-1);
                return true;
            }
            "h" | "Left" => {
                let (ls, cw, tm, nm, ts, cl) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style, s.config.show_cursor_line)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm, nm, ts, cl);
                crate::input::actions::settings::apply_settings_change(state, change);
                return true;
            }
            "l" | "Right" => {
                let (ls, cw, tm, nm, ts, cl) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style, s.config.show_cursor_line)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm, nm, ts, cl);
                crate::input::actions::settings::apply_settings_change(state, change);
                return true;
            }
            "r" => {
                // Reset to defaults
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
                return true;
            }
            _ => return true, // consume all other keys when settings visible
        }
    }

    // Search bar visible — route keys to search entry
    let search_visible = state.borrow().search_bar.is_visible();
    if search_visible {
        match key_name {
            "Escape" => {
                crate::input::search::clear_search(&mut state.borrow_mut());
                state.borrow().search_bar.hide();
                return true;
            }
            "Return" => {
                crate::input::search::execute_search(&state);
                state.borrow().search_bar.hide();
                return true;
            }
            "Tab" => {
                crate::input::search::toggle_playback(&mut state.borrow_mut());
                return true;
            }
            _ => return false, // let GTK route to the Entry
        }
    }

    // --- Gloss overlay (when visible) ---
    let gloss_visible = state.borrow().correction_overlay.is_visible();
    if gloss_visible {
        match key_name {
            "r" => {
                retry_gloss(state);
                return true;
            }
            "Escape" | "n" => {
                state.borrow().correction_overlay.hide();
                return true;
            }
            _ => return true, // consume all other keys while overlay is open
        }
    }

    // --- Gamepad overlay (when visible) ---
    let gamepad_visible = state.borrow().gamepad_overlay.is_visible();
    if gamepad_visible {
        match key_name {
            "Escape" => {
                state.borrow().gamepad_overlay.hide();
                return true;
            }
            _ => return true, // consume all other keys when gamepad overlay visible
        }
    }

    // --- Keybinds overlay (when visible) ---
    let keybinds_visible = state.borrow().keybinds_overlay.is_visible();
    if keybinds_visible {
        match key_name {
            "Escape" => {
                state.borrow().keybinds_overlay.hide();
                return true;
            }
            "g" if key_state.borrow().pending_ctrl_slash => {
                // C-/ g chord: swap to gamepad overlay
                key_state.borrow_mut().pending_ctrl_slash = false;
                let s = state.borrow();
                s.keybinds_overlay.hide();
                s.gamepad_overlay.show();
                return true;
            }
            "exclam" => {
                state.borrow_mut().keybinds_overlay.adjust_scale(-1);
                return true;
            }
            "bar" => {
                state.borrow_mut().keybinds_overlay.adjust_scale(1);
                return true;
            }
            "0" => {
                state.borrow_mut().keybinds_overlay.reset_scale();
                return true;
            }
            _ => return true, // consume all other keys when keybinds visible
        }
    }

    // --- Concordance picker overlay ---
    if state.borrow().concordance_picker.is_visible() {
        if is_ctrl && key_name == "n" {
            state.borrow().concordance_picker.move_selection(1);
            return true;
        }
        if is_ctrl && key_name == "p" {
            state.borrow().concordance_picker.move_selection(-1);
            return true;
        }
        match key_name {
            "Down" => {
                state.borrow().concordance_picker.move_selection(1);
                return true;
            }
            "Up" => {
                state.borrow().concordance_picker.move_selection(-1);
                return true;
            }
            "Return" => {
                let selected = state.borrow().concordance_picker.selected_word();
                state.borrow().concordance_picker.hide();
                if let Some(word) = selected {
                    crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
                }
                return true;
            }
            "Escape" => {
                state.borrow().concordance_picker.hide();
                return true;
            }
            _ => {}
        }
        return false;
    }

    // --- Concordance word picker (when visible) ---
    let conc_word_picker_visible = state.borrow().concordance_word_picker.is_visible();
    if conc_word_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().concordance_word_picker.hide();
                return true;
            }
            "Return" => {
                let selected = state.borrow().concordance_word_picker.selected_word();
                state.borrow().concordance_word_picker.hide();
                if let Some(word) = selected {
                    crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
                }
                return true;
            }
            _ => {
                if is_ctrl && key_name == "n" {
                    state.borrow().concordance_word_picker.move_selection(1);
                    return true;
                }
                if is_ctrl && key_name == "p" {
                    state.borrow().concordance_word_picker.move_selection(-1);
                    return true;
                }
                // Let entry handle text input
                return false;
            }
        }
    }

    // --- Concordance list picker (when visible) ---
    let conc_list_picker_visible = state.borrow().concordance_list_picker.is_visible();
    if conc_list_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().concordance_list_picker.hide();
                return true;
            }
            "Return" => {
                let selected = state.borrow().concordance_list_picker.selected_index();
                state.borrow().concordance_list_picker.hide();
                if let Some(idx) = selected {
                    {
                        let mut s = state.borrow_mut();
                        if let Some(conc) = &mut s.concordance_state {
                            conc.current_index = idx;
                        }
                    }
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
                return true;
            }
            "j" | "n" => {
                state.borrow().concordance_list_picker.move_selection(1);
                return true;
            }
            "k" | "p" => {
                if !is_ctrl {
                    state.borrow().concordance_list_picker.move_selection(-1);
                    return true;
                }
                return false;
            }
            _ => {
                if is_ctrl && key_name == "n" {
                    state.borrow().concordance_list_picker.move_selection(1);
                    return true;
                }
                if is_ctrl && key_name == "p" {
                    state.borrow().concordance_list_picker.move_selection(-1);
                    return true;
                }
                return false;
            }
        }
    }

    // --- Action popup (when visible) ---
    let action_popup_visible = state.borrow().action_popup.is_some();
    if action_popup_visible && is_ctrl {
        match key_name {
            "n" => {
                let mut s = state.borrow_mut();
                s.action_popup_widget.move_selection(1);
                let idx = s.action_popup_widget.selected_index();
                if let Some(ref mut popup) = s.action_popup {
                    popup.selected_index = idx;
                }
                return true;
            }
            "p" => {
                let mut s = state.borrow_mut();
                s.action_popup_widget.move_selection(-1);
                let idx = s.action_popup_widget.selected_index();
                if let Some(ref mut popup) = s.action_popup {
                    popup.selected_index = idx;
                }
                return true;
            }
            _ => {}
        }
    }
    if action_popup_visible {
        match key_name {
            "Return" => {
                let selected_idx = state.borrow().action_popup_widget.selected_index();
                crate::input::visual::close_action_popup(&mut state.borrow_mut());
                crate::input::visual::execute_action(state, selected_idx, tokio_handle);
                return true;
            }
            "Escape" => {
                crate::input::visual::close_action_popup(&mut state.borrow_mut());
                return true;
            }
            _ => return true, // consume all keys when popup visible
        }
    }

    // --- Visual mode ---
    let in_visual = state.borrow().visual_selection.is_some();
    if in_visual {
        match key_name {
            "j" => {
                crate::input::visual::move_selection_cursor(&mut state.borrow_mut(), 1);
                return true;
            }
            "k" => {
                crate::input::visual::move_selection_cursor(&mut state.borrow_mut(), -1);
                return true;
            }
            "G" => {
                crate::input::visual::extend_to_end(&mut state.borrow_mut());
                return true;
            }
            "g" => {
                // In visual mode, 'g' starts gg sequence to extend to start
                key_state.borrow_mut().pending_g = true;
                let ks = Rc::clone(key_state);
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    ks.borrow_mut().pending_g = false;
                });
                return true;
            }
            "Escape" | "V" => {
                crate::input::visual::exit_visual_mode(&mut state.borrow_mut());
                return true;
            }
            "y" => {
                crate::input::visual::yank_selection(&mut state.borrow_mut());
                return true;
            }
            "Return" => {
                crate::input::visual::open_action_popup(&mut state.borrow_mut());
                return true;
            }
            _ => {
                // Consume all other keys in visual mode
                return true;
            }
        }
    }

    // --- Normal mode (no picker) ---

    // gg sequence check
    if key_state.borrow().pending_g {
        key_state.borrow_mut().pending_g = false;
        if key_name == "g" {
            if state.borrow().visual_selection.is_some() {
                crate::input::visual::extend_to_start(&mut state.borrow_mut());
            } else {
                navigation::jump_to_start(&mut state.borrow_mut());
            }
            return true;
        } else if key_name == "semicolon" {
            // g; — jump to most recently created bookmark
            crate::input::actions::bookmarks::jump_to_recent_bookmark(state, tokio_handle);
            return true;
        }
    }

    // Ctrl+Alt+l: save position, quit MPV, and close window
    if is_ctrl && is_alt && key_name == "l" {
        crate::app::save_position(&mut state.borrow_mut());
        let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
        state.borrow().window.close();
        return true;
    }

    // Alt combos
    if is_alt && key_name == "backslash" {
        let mut s = state.borrow_mut();
        s.vocab_highlight_visible = !s.vocab_highlight_visible;
        if s.vocab_highlight_visible {
            crate::app::apply_vocab_highlighting(&s);
        } else {
            crate::app::remove_vocab_highlighting(&s);
        }
        s.config.vocab_highlight_visible = s.vocab_highlight_visible;
        crate::config::save(&s.config);
        crate::logging::log(&format!("VOCAB: highlighting {}", if s.vocab_highlight_visible { "on" } else { "off" }));
        return true;
    }

    if is_alt {
        match key_name {
            "d" => {
                let mut s = state.borrow_mut();
                s.dim_enabled = !s.dim_enabled;
                if !s.dim_enabled {
                    // Clear dim from entire buffer since only visible range was managed
                    let (start, end) = s.buffer.bounds();
                    s.buffer.remove_tag(&s.dim_tag, &start, &end);
                }
                navigation::update_highlight_only(&mut s);
                s.config.dim_enabled = s.dim_enabled;
                crate::config::save(&s.config);
                crate::logging::log(&format!("DIM: {}", if s.dim_enabled { "on" } else { "off" }));
                return true;
            }
            "f" => {
                crate::app::show_font_info(&state.borrow());
                return true;
            }
            "i" => {
                return crate::input::timestamps::set_end_time(&mut state.borrow_mut());
            }
            _ => return false,
        }
    }

    // Ctrl combos — page turn navigation (e-reader style)
    if is_ctrl {
        match key_name {
            "slash" => {
                let s = state.borrow();
                if s.keybinds_overlay.is_visible() || s.gamepad_overlay.is_visible() {
                    s.keybinds_overlay.hide();
                    s.gamepad_overlay.hide();
                } else {
                    // Hide other overlays before showing keybinds
                    s.picker.hide();
                    s.media_picker.hide();
                    s.settings_overlay.hide();
                    s.search_bar.hide();
                    s.correction_overlay.hide();
                    s.keybinds_overlay.show();
                }
                // Arm the chord so a following 'g' swaps keybinds → gamepad.
                key_state.borrow_mut().pending_ctrl_slash = true;
                let ks = Rc::clone(key_state);
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    ks.borrow_mut().pending_ctrl_slash = false;
                });
                return true;
            }
            "backslash" => {
                let abbrev = state
                    .borrow()
                    .current_work
                    .as_ref()
                    .map(|w| w.abbrev.clone());
                if let Some(abbrev) = abbrev {
                    let state_clone = Rc::clone(state);
                    let handle = tokio_handle.clone();
                    glib::spawn_future_local(async move {
                        let words = handle
                            .spawn_blocking(move || {
                                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                                crate::db::queries::load_vocab_word_list(&conn, &abbrev)
                                    .unwrap_or_default()
                            })
                            .await
                            .unwrap_or_default();
                        {
                            let mut s = state_clone.borrow_mut();
                            s.concordance_picker.set_words(words);
                            s.concordance_picker.show();
                        }
                        // set_text triggers connect_changed which borrows state,
                        // so the mutable borrow must be dropped first.
                        state_clone.borrow().concordance_picker.search_entry().set_text("");
                    });
                }
                return true;
            }
            "d" | "f" | "u" => {
                navigation::page_forward(&mut state.borrow_mut());
                return true;
            }
            "b" => {
                navigation::page_backward(&mut state.borrow_mut());
                return true;
            }
            "Up" => {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0));
                return true;
            }
            "Down" => {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0));
                return true;
            }
            _ => return false,
        }
    }

    // Vocab popup: backslash/numbersign show popup or cycle words, with 3s auto-hide
    if key_name == "backslash" || key_name == "numbersign" {
        let popup_visible = state.borrow().vocab_popup.is_visible();
        if popup_visible {
            if key_name == "backslash" {
                crate::app::vocab_popup_next(&mut state.borrow_mut());
            } else {
                crate::app::vocab_popup_prev(&mut state.borrow_mut());
            }
        } else {
            crate::app::open_vocab_popup(&mut state.borrow_mut());
        }
        // Reset the auto-hide timer: bump generation, schedule fade after 3s
        let gen = {
            let s = state.borrow();
            let next = s.vocab_popup_fade_gen.get() + 1;
            s.vocab_popup_fade_gen.set(next);
            next
        };
        let state_clone = Rc::clone(state);
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            let s = state_clone.borrow();
            if s.vocab_popup_fade_gen.get() != gen {
                return; // a newer press reset the timer
            }
            if !s.vocab_popup.is_visible() {
                return;
            }
            // Crossfade out the popup
            let widget = s.vocab_popup.widget().clone();
            let target = adw::CallbackAnimationTarget::new(move |value| {
                widget.set_opacity(value as f64);
                if value <= 0.0 {
                    widget.set_visible(false);
                    widget.set_opacity(1.0); // restore for next show
                }
            });
            let anim = adw::TimedAnimation::new(
                s.vocab_popup.widget(),
                1.0, 0.0, 500, target,
            );
            anim.set_easing(adw::Easing::EaseOutQuad);
            anim.play();
        });
        return true;
    }

    // Other vocab popup keys (when popup is visible)
    if state.borrow().vocab_popup.is_visible() {
        match key_name {
            "g" => {
                crate::app::vocab_popup_toggle_view(&mut state.borrow_mut());
                return true;
            }
            "Tab" => {
                crate::input::search::toggle_playback(&mut state.borrow_mut());
                return true;
            }
            // Let h, j, k, Escape, and other keys fall through to normal handling
            _ => {}
        }
    }

    // Single keys
    match key_name {
        "space" => {
            if is_shift {
                navigation::page_backward(&mut state.borrow_mut());
            } else {
                navigation::page_forward(&mut state.borrow_mut());
            }
            true
        }
        "j" => {
            navigation::cursor_next_dialogue(&mut state.borrow_mut());
            true
        }
        "k" => {
            navigation::cursor_prev_line(&mut state.borrow_mut());
            true
        }
        "Up" => {
            if is_shift {
                navigation::page_backward_bottom(&mut state.borrow_mut());
            } else {
                navigation::jump_to_prev_dialogue(&mut state.borrow_mut());
            }
            true
        }
        "Down" => {
            navigation::jump_to_next_dialogue(&mut state.borrow_mut());
            true
        }
        "g" => {
            key_state.borrow_mut().pending_g = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_g = false;
            });
            true
        }
        "G" => {
            navigation::jump_to_end(&mut state.borrow_mut());
            true
        }
        "comma" => {
            if is_shift {
                navigation::page_backward_bottom(&mut state.borrow_mut());
            } else {
                navigation::jump_to_prev_dialogue(&mut state.borrow_mut());
            }
            true
        }
        "q" => {
            navigation::jump_to_next_dialogue(&mut state.borrow_mut());
            true
        }
        "less" => {
            navigation::page_backward(&mut state.borrow_mut());
            true
        }
        "minus" => {
            let mut s = state.borrow_mut();
            s.config.show_cursor_line = !s.config.show_cursor_line;
            crate::input::navigation::update_highlight_only(&mut s);
            crate::config::save(&s.config);
            true
        }
        "Q" => {
            navigation::cursor_to_page_bottom(&mut state.borrow_mut());
            true
        }
        "o" | "e" | "O" | "E" => {
            let offset = match key_name {
                "o" => -3.5,
                "e" => 3.5,
                "O" => -60.0,
                "E" => 60.0,
                _ => unreachable!(),
            };
            let mut s = state.borrow_mut();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SeekRelative(offset));
            // Brief suppression while MPV processes the seek; sync resumes afterwards
            // so the cursor follows audio to the new position.
            s.suppress_sync_until = Some(
                std::time::Instant::now() + std::time::Duration::from_millis(500),
            );
            true
        }
        "a" => {
            crate::input::timestamps::play_current_line(&mut state.borrow_mut());
            true
        }
        "Tab" => {
            crate::input::search::toggle_playback(&mut state.borrow_mut());
            true
        }
        "s" => {
            let mut s = state.borrow_mut();
            s.sync_enabled = !s.sync_enabled;
            s.sync_icon.set_visible(!s.sync_enabled);
            crate::logging::log(&format!("SYNC: {}", if s.sync_enabled { "enabled" } else { "disabled" }));
            true
        }
        "exclam" => {
            crate::logging::log("FONT: exclam matched, decreasing");
            crate::app::adjust_font_size(&mut state.borrow_mut(), -1);
            crate::app::show_font_info(&state.borrow());
            true
        }
        "bar" => {
            crate::logging::log("FONT: bar matched, increasing");
            crate::app::adjust_font_size(&mut state.borrow_mut(), 1);
            crate::app::show_font_info(&state.borrow());
            true
        }
        "0" => {
            crate::app::reset_font_size(&mut state.borrow_mut());
            true
        }
        "f" => {
            crate::app::cycle_font(&mut state.borrow_mut(), true);
            true
        }
        "F" => {
            crate::app::cycle_font(&mut state.borrow_mut(), false);
            true
        }
        "plus" => {
            let mut s = state.borrow_mut();
            let new_speed = if s.playback_speed == 1.0 { 1.3 } else { 1.0 };
            s.playback_speed = new_speed;
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetSpeed(new_speed));
            crate::logging::log(&format!("SPEED: toggled to {}x", new_speed));
            true
        }
        "slash" => {
            let mut s = state.borrow_mut();
            crate::input::search::clear_search(&mut s);
            s.search_bar.show();
            true
        }
        "n" => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::next_match(&mut state.borrow_mut());
                true
            } else {
                false
            }
        }
        "N" => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::prev_match(&mut state.borrow_mut());
                true
            } else {
                false
            }
        }
        "u" | "Right" => {
            let ok = crate::input::timestamps::set_start_time(&mut state.borrow_mut());
            if ok {
                navigation::cursor_next_dialogue(&mut state.borrow_mut());
            }
            ok
        }
        "Left" => {
            let mut s = state.borrow_mut();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SeekRelative(-30.0));
            // Brief suppression while MPV processes the seek; sync resumes afterwards.
            s.suppress_sync_until = Some(
                std::time::Instant::now() + std::time::Duration::from_millis(500),
            );
            true
        }
        "period" => {
            crate::input::timestamps::set_chapter(&mut state.borrow_mut())
        }
        "bracketleft" => {
            {
                let mut s = state.borrow_mut();
                if s.translations_visible {
                    crate::app::toggle_translations(&mut s);
                }
                navigation::jump_to_prev_chapter(&mut s);
            }
            true
        }
        "braceleft" => {
            {
                let mut s = state.borrow_mut();
                if s.translations_visible {
                    crate::app::toggle_translations(&mut s);
                }
                navigation::jump_to_next_chapter(&mut s);
            }
            true
        }
        "2" => {
            let mut s = state.borrow_mut();
            let is_play = s
                .current_work
                .as_ref()
                .map(|w| w.work_type == "play")
                .unwrap_or(false);
            if is_play {
                crate::input::navigation::jump_to_prev_scene(&mut s);
            } else {
                crate::input::navigation::jump_to_prev_chapter(&mut s);
            }
            true
        }
        "3" => {
            let mut s = state.borrow_mut();
            let is_play = s
                .current_work
                .as_ref()
                .map(|w| w.work_type == "play")
                .unwrap_or(false);
            if is_play {
                crate::input::navigation::jump_to_next_scene(&mut s);
            } else {
                crate::input::navigation::jump_to_next_chapter(&mut s);
            }
            true
        }
        "i" => {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            drop(s);
            crate::app::toggle_translations(&mut state.borrow_mut());
            true
        }
        "BackSpace" => {
            crate::input::timestamps::delete_timestamp(&mut state.borrow_mut())
        }
        "p" => {
            crate::input::timestamps::nudge_start_backward(&mut state.borrow_mut())
        }
        "P" => {
            crate::input::timestamps::nudge_start_forward(&mut state.borrow_mut())
        }
        "U" => {
            crate::input::timestamps::undo_timestamp(&mut state.borrow_mut())
        }
        "l" => {
            crate::app::toggle_sign_column(&mut state.borrow_mut());
            true
        }
        "x" => {
            navigation::page_forward(&mut state.borrow_mut());
            true
        }
        "y" => {
            navigation::page_backward(&mut state.borrow_mut());
            true
        }
        "m" => {
            crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle);
            true
        }
        "semicolon" => {
            if is_shift {
                navigation::prev_bookmark(&mut state.borrow_mut());
            } else {
                navigation::next_bookmark(&mut state.borrow_mut());
            }
            true
        }
        "colon" => {
            navigation::prev_bookmark(&mut state.borrow_mut());
            true
        }
        "r" => {
            let has_concordance = state.borrow().concordance_state.is_some();
            if has_concordance {
                let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
                let advanced = {
                    let mut s = state.borrow_mut();
                    if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                        conc.advance_within_work(abbrev)
                    } else {
                        false
                    }
                };
                if advanced {
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
            } else {
                navigation::jump_to_next_vocab(&mut state.borrow_mut());
            }
            true
        }
        "R" => {
            let has_concordance = state.borrow().concordance_state.is_some();
            if has_concordance {
                let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
                let retreated = {
                    let mut s = state.borrow_mut();
                    if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                        conc.retreat_within_work(abbrev)
                    } else {
                        false
                    }
                };
                if retreated {
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
            } else {
                navigation::jump_to_prev_vocab(&mut state.borrow_mut());
            }
            true
        }
        "h" => {
            let mut s = state.borrow_mut();
            s.vocab_popup_auto = !s.vocab_popup_auto;
            if s.vocab_popup_auto {
                crate::app::open_vocab_popup(&mut s);
            } else {
                crate::app::close_vocab_popup(&mut s);
            }
            true
        }
        "Escape" => {
            // Clear concordance mode on Escape
            {
                let has_conc = state.borrow().concordance_state.is_some();
                if has_conc {
                    let mut s = state.borrow_mut();
                    s.concordance_state = None;
                    s.concordance_bar.hide();
                    return true;
                }
            }
            let mut s = state.borrow_mut();
            if s.ab_repeat.loop_active {
                let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
                s.ab_repeat.clear();
                s.ab_repeat.chunk_index = None;
                s.ab_a_line.set(None);
                s.ab_b_line.set(None);
                s.suppress_sync_until = None;
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
                crate::app::remove_ab_dim(&s);
                crate::logging::log("CHUNK: AB loop cleared");
                drop(s);
                crate::input::navigation::update_highlight_and_center(&mut state.borrow_mut());
                true
            } else if !s.search_matches.is_empty() {
                crate::input::search::clear_search(&mut s);
                true
            } else {
                false
            }
        }
        "V" => {
            crate::input::visual::enter_visual_mode(&mut state.borrow_mut());
            true
        }
        "w" => {
            navigation::word_cycle_copy(&mut state.borrow_mut());
            true
        }
        "W" => {
            navigation::word_collect_copy(&mut state.borrow_mut());
            true
        }
        _ => false,
    }
}

const CHUNK_PREROLL: f64 = 0.5;

/// Activate a chunk by index: set AB loop (with preroll), resolve buffer lines.
fn activate_chunk(s: &mut AppState, idx: usize) {
    if let Some(chunk) = s.ab_repeat.chunks.get(idx).cloned() {
        if let (Some(a), Some(b)) = (chunk.a_time, chunk.b_time) {
            let loop_a = (a - CHUNK_PREROLL).max(0.0);
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetAbLoop { a: loop_a, b });
            s.ab_repeat.a_time = Some(a);
            s.ab_repeat.b_time = Some(b);
            s.ab_repeat.loop_active = true;
            if let Some(ref work) = s.current_work {
                let mut a_buf = None;
                let mut b_buf = None;
                for (i, line) in work.lines.iter().enumerate() {
                    if line.div1 == chunk.div1 && Some(line.div2) == chunk.div2 {
                        if line.line_in_div == chunk.a_line {
                            a_buf = Some(i);
                        }
                        if line.line_in_div == chunk.b_line {
                            b_buf = Some(i);
                        }
                    }
                }
                if let Some(ref lm) = s.line_map {
                    a_buf = a_buf.map(|i| lm.work_to_buffer[i]);
                    b_buf = b_buf.map(|i| lm.work_to_buffer[i]);
                }
                s.ab_repeat.a_line = a_buf;
                s.ab_repeat.b_line = b_buf;
                s.ab_a_line.set(a_buf);
                s.ab_b_line.set(b_buf);
            }
            crate::logging::log(&format!("CHUNK: looping chunk {} ({:.1}s - {:.1}s, preroll {:.1}s)", idx, a, b, loop_a));
        }
    }
}

fn retry_gloss(state_rc: &Rc<RefCell<AppState>>) {
    let (original, endpoint, model, tokio_handle) = {
        let state = state_rc.borrow();
        let original = match &state.gloss_original_text {
            Some(t) => t.clone(),
            None => return,
        };
        (
            original,
            state.config.ollama_endpoint.clone(),
            state.config.ollama_model.clone(),
            state.tokio_handle.clone(),
        )
    };

    crate::logging::log("VISUAL: retrying LLM gloss");
    state_rc.borrow().correction_overlay.show_loading();

    let state_for_result = Rc::clone(state_rc);
    let original_for_display = original.clone();

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::ollama::gloss_text(&endpoint, &model, &original).await
            })
            .await;

        let state = state_for_result.borrow();

        match result {
            Ok(Ok(gloss)) => {
                state.correction_overlay.show(&original_for_display, &gloss);
                crate::logging::log("VISUAL: gloss overlay refreshed with retry");
            }
            Ok(Err(e)) => {
                crate::logging::log(&format!("VISUAL: LLM gloss retry error: {}", e));
                state.correction_overlay.show(&format!("Error: {}", e), "");
            }
            Err(e) => {
                crate::logging::log(&format!("VISUAL: tokio join error on gloss retry: {}", e));
            }
        }
    });
}
