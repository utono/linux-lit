use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::input::navigation;

#[derive(Default)]
pub struct KeyState {
    pub pending_g: bool,
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

    // Ctrl+p: open picker when hidden
    if is_ctrl && key_name == "p" && !picker_visible {
        state.borrow().picker.show();
        return true;
    }

    // Picker-visible keys
    if picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().picker.hide();
                return true;
            }
            "Return" => {
                let abbrev = state.borrow().picker.selected_abbrev();
                if let Some(abbrev) = abbrev {
                    let state_clone = Rc::clone(state);
                    let handle = tokio_handle.clone();
                    glib::spawn_future_local(async move {
                        let work = handle
                            .spawn_blocking(move || {
                                let conn =
                                    crate::db::queries::open_db().expect("Failed to open lit.db");
                                crate::db::queries::load_work(&conn, &abbrev)
                            })
                            .await;
                        match work {
                            Ok(Ok(work)) => {
                                let mut s = state_clone.borrow_mut();
                                s.picker.hide();
                                crate::app::display_work(&mut s, work);
                            }
                            Ok(Err(e)) => eprintln!("Failed to load work: {}", e),
                            Err(e) => eprintln!("Task join error: {}", e),
                        }
                    });
                }
                return true;
            }
            "Down" => {
                state.borrow().picker.move_selection(1);
                return true;
            }
            "Up" => {
                state.borrow().picker.move_selection(-1);
                return true;
            }
            "j" => {
                if !state.borrow().picker.search_entry().has_focus() {
                    state.borrow().picker.move_selection(1);
                    return true;
                }
            }
            "k" => {
                if !state.borrow().picker.search_entry().has_focus() {
                    state.borrow().picker.move_selection(-1);
                    return true;
                }
            }
            _ => {}
        }
        return false;
    }

    // Media picker
    let media_picker_visible = state.borrow().media_picker.is_visible();

    if media_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().media_picker.hide();
                return true;
            }
            "Return" => {
                let selected_path = state.borrow().media_picker.selected_media_path();
                let selected_id = state.borrow().media_picker.selected_media_id();
                if let (Some(path), Some(media_id)) = (selected_path, selected_id) {
                    let state_clone = Rc::clone(state);
                    let handle = tokio_handle.clone();
                    glib::spawn_future_local(async move {
                        let socket_path = handle
                            .spawn_blocking(move || {
                                if let Some(sock) =
                                    crate::mpv::discovery::find_socket_for_work(&[path.clone()])
                                {
                                    return sock.to_string_lossy().to_string();
                                }
                                let launched = crate::mpv::discovery::launch_mpv(&path);
                                for _ in 0..60 {
                                    std::thread::sleep(std::time::Duration::from_millis(50));
                                    if std::path::Path::new(&launched).exists() {
                                        return launched;
                                    }
                                }
                                launched
                            })
                            .await
                            .unwrap_or_default();

                        if !socket_path.is_empty() {
                            let mut s = state_clone.borrow_mut();
                            s.media_id = Some(media_id);
                            let _ = s
                                .cmd_tx
                                .try_send(crate::mpv::MpvCommand::Connect(socket_path));
                            s.media_picker.hide();
                            crate::logging::log(&format!(
                                "MEDIA: switched to media_id={}",
                                media_id
                            ));
                        }
                    });
                }
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
            _ => {}
        }
        return false;
    }

    // Settings overlay
    let settings_visible = state.borrow().settings_overlay.is_visible();

    // Ctrl+,: toggle settings overlay
    if is_ctrl && key_name == "comma" && !settings_visible && !picker_visible {
        let s = state.borrow();
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        drop(s);
        state.borrow_mut().settings_overlay.show(ls, cw, tm);
        return true;
    }

    // Settings overlay visible — route keys
    if settings_visible {
        match key_name {
            "Escape" => {
                // Revert to snapshot values
                let (snap_ls, snap_cw, snap_tm, snap_ti) = state.borrow().settings_overlay.snapshot();
                {
                    let mut s = state.borrow_mut();
                    if s.dialogue_formatting_active {
                        let tag_table = s.buffer.tag_table();
                        if let Some(tag) = tag_table.lookup("speaker-gap") {
                            tag.set_property("pixels-above-lines", snap_ls.max(1) as i32 * 5);
                        }
                    } else {
                        s.text_view.set_pixels_above_lines(snap_ls as i32);
                        s.text_view.set_pixels_below_lines(snap_ls as i32);
                    }
                    s.scrolled_window.set_width_request(snap_cw as i32);
                    s.text_view.set_left_margin(snap_tm as i32);
                    s.text_view.set_right_margin(snap_tm as i32);
                    s.config.line_spacing = snap_ls;
                    s.config.column_width = snap_cw;
                    s.config.text_margins = snap_tm;
                    // Revert theme if changed
                    if let Some(snap_theme) = s.settings_overlay.themes().get(snap_ti) {
                        let snap_theme = snap_theme.clone();
                        s.settings_overlay.set_theme_index(snap_ti);
                        apply_theme_to_state(&mut s, &snap_theme);
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
                let (ls, cw, tm) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm);
                apply_settings_change(state, change);
                return true;
            }
            "l" | "Right" => {
                let (ls, cw, tm) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm);
                apply_settings_change(state, change);
                return true;
            }
            "r" => {
                // Reset to defaults
                let mut s = state.borrow_mut();
                let ls = crate::config::DEFAULT_LINE_SPACING;
                let cw = crate::config::DEFAULT_COLUMN_WIDTH;
                let tm = crate::config::DEFAULT_TEXT_MARGINS;
                if s.dialogue_formatting_active {
                    let tag_table = s.buffer.tag_table();
                    if let Some(tag) = tag_table.lookup("speaker-gap") {
                        tag.set_property("pixels-above-lines", ls.max(1) as i32 * 5);
                    }
                } else {
                    s.text_view.set_pixels_above_lines(ls as i32);
                    s.text_view.set_pixels_below_lines(ls as i32);
                }
                s.scrolled_window.set_width_request(cw as i32);
                s.text_view.set_left_margin(tm as i32);
                s.text_view.set_right_margin(tm as i32);
                s.config.line_spacing = ls;
                s.config.column_width = cw;
                s.config.text_margins = tm;
                s.settings_overlay.update_displayed_values(ls, cw, tm);
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
                state.borrow().search_bar.hide();
                return true;
            }
            "Return" => {
                crate::input::search::execute_search(&state);
                state.borrow().search_bar.hide();
                return true;
            }
            "Tab" => {
                crate::input::search::toggle_playback(&state.borrow());
                return true;
            }
            _ => return false, // let GTK route to the Entry
        }
    }

    // --- Normal mode (no picker) ---

    // gg sequence check
    if key_state.borrow().pending_g {
        key_state.borrow_mut().pending_g = false;
        if key_name == "g" {
            navigation::jump_to_start(&mut state.borrow_mut());
            return true;
        }
    }

    // Ctrl+Shift+l: save position and quit
    if is_ctrl && is_shift && key_name == "L" {
        crate::app::save_position(&mut state.borrow_mut());
        state.borrow().window.close();
        return true;
    }

    // Alt combos
    if is_alt {
        match key_name {
            "f" => {
                crate::app::show_font_info(&state.borrow());
                return true;
            }
            _ => return false,
        }
    }

    // Ctrl combos — page turn navigation (e-reader style)
    if is_ctrl {
        match key_name {
            "d" | "f" => {
                navigation::page_forward(&mut state.borrow_mut());
                return true;
            }
            "u" | "b" => {
                navigation::page_backward(&mut state.borrow_mut());
                return true;
            }
            _ => return false,
        }
    }

    // Single keys
    match key_name {
        "j" => {
            navigation::move_cursor(&mut state.borrow_mut(), 1);
            true
        }
        "k" => {
            navigation::move_cursor(&mut state.borrow_mut(), -1);
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
            navigation::jump_to_prev_dialogue(&mut state.borrow_mut());
            true
        }
        "q" => {
            navigation::jump_to_next_dialogue(&mut state.borrow_mut());
            true
        }
        "Tab" => {
            crate::input::search::toggle_playback(&state.borrow());
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
            crate::input::timestamps::set_start_time(&mut state.borrow_mut())
        }
        "i" => {
            crate::input::timestamps::set_end_time(&mut state.borrow_mut())
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
        "l" => {
            crate::app::toggle_sign_column(&mut state.borrow_mut());
            true
        }
        "x" => {
            // Next chunk forward: find chunk at cursor, or advance to next
            {
                let mut s = state.borrow_mut();
                let lines = s.current_work.as_ref().map(|w| &w.lines[..]);
                if let Some(lines) = lines {
                    // If no chunk index yet, find chunk at current line
                    if s.ab_repeat.chunk_index.is_none() {
                        let work_idx = s.work_line_for_buffer(s.current_line);
                        s.ab_repeat.chunk_index = work_idx.and_then(|idx| {
                            s.ab_repeat.find_chunk_at_line(idx, lines)
                        });
                    } else {
                        // Advance to next chunk
                        s.ab_repeat.next_chunk();
                    }
                    // Activate loop for current chunk
                    if let Some(idx) = s.ab_repeat.chunk_index {
                        if let Some(chunk) = s.ab_repeat.chunks.get(idx).cloned() {
                            if let (Some(a), Some(b)) = (chunk.a_time, chunk.b_time) {
                                let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetAbLoop { a, b });
                                s.ab_repeat.a_time = Some(a);
                                s.ab_repeat.b_time = Some(b);
                                s.ab_repeat.loop_active = true;
                                // Resolve chunk lines to buffer line indices
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
                                    // Translate to buffer indices if line_map present
                                    if let Some(ref lm) = s.line_map {
                                        a_buf = a_buf.map(|i| lm.work_to_buffer[i]);
                                        b_buf = b_buf.map(|i| lm.work_to_buffer[i]);
                                    }
                                    s.ab_repeat.a_line = a_buf;
                                    s.ab_repeat.b_line = b_buf;
                                    s.ab_a_line.set(a_buf);
                                    s.ab_b_line.set(b_buf);
                                }
                                crate::logging::log(&format!("CHUNK: looping chunk {} ({:.1}s - {:.1}s)", idx, a, b));
                            }
                        }
                    }
                }
            } // drop borrow_mut before immutable borrow
            {
                let s = state.borrow();
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
            }
            crate::app::apply_ab_dim(&state.borrow());
            true
        }
        "y" => {
            // Previous chunk backward
            {
                let mut s = state.borrow_mut();
                if s.ab_repeat.prev_chunk().is_some() {
                    if let Some(idx) = s.ab_repeat.chunk_index {
                        if let Some(chunk) = s.ab_repeat.chunks.get(idx).cloned() {
                            if let (Some(a), Some(b)) = (chunk.a_time, chunk.b_time) {
                                let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetAbLoop { a, b });
                                s.ab_repeat.a_time = Some(a);
                                s.ab_repeat.b_time = Some(b);
                                s.ab_repeat.loop_active = true;
                                // Resolve chunk lines to buffer line indices
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
                                    // Translate to buffer indices if line_map present
                                    if let Some(ref lm) = s.line_map {
                                        a_buf = a_buf.map(|i| lm.work_to_buffer[i]);
                                        b_buf = b_buf.map(|i| lm.work_to_buffer[i]);
                                    }
                                    s.ab_repeat.a_line = a_buf;
                                    s.ab_repeat.b_line = b_buf;
                                    s.ab_a_line.set(a_buf);
                                    s.ab_b_line.set(b_buf);
                                }
                                crate::logging::log(&format!("CHUNK: looping chunk {} ({:.1}s - {:.1}s)", idx, a, b));
                            }
                        }
                    }
                }
            } // drop borrow_mut before immutable borrow
            {
                let s = state.borrow();
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
            }
            crate::app::apply_ab_dim(&state.borrow());
            true
        }
        "m" => {
            let abbrev = state
                .borrow()
                .current_work
                .as_ref()
                .map(|w| w.abbrev.clone());
            if let Some(abbrev) = abbrev {
                let state_clone = Rc::clone(state);
                let handle = tokio_handle.clone();
                glib::spawn_future_local(async move {
                    let items = handle
                        .spawn_blocking(move || {
                            let conn =
                                crate::db::queries::open_db().expect("Failed to open lit.db");
                            crate::db::queries::list_media_for_work(&conn, &abbrev)
                                .unwrap_or_default()
                        })
                        .await
                        .unwrap_or_default();
                    let mut s = state_clone.borrow_mut();
                    s.media_picker.set_items(items);
                    s.media_picker.show();
                });
            }
            true
        }
        "Escape" => {
            let mut s = state.borrow_mut();
            if s.ab_repeat.loop_active {
                let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
                s.ab_repeat.clear();
                s.ab_a_line.set(None);
                s.ab_b_line.set(None);
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
                crate::app::remove_ab_dim(&s);
                crate::logging::log("CHUNK: AB loop cleared");
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn apply_settings_change(
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
                s.text_view.set_pixels_above_lines(val as i32);
                s.text_view.set_pixels_below_lines(val as i32);
            }
            s.config.line_spacing = val;
        }
        SettingsChange::ColumnWidth(val) => {
            s.scrolled_window.set_width_request(val as i32);
            s.config.column_width = val;
        }
        SettingsChange::TextMargins(val) => {
            s.text_view.set_left_margin(val as i32);
            s.text_view.set_right_margin(val as i32);
            s.config.text_margins = val;
        }
        SettingsChange::Theme(theme) => {
            apply_theme_to_state(&mut s, &theme);
        }
        SettingsChange::None => {}
    }
}

fn apply_theme_to_state(state: &mut crate::app::AppState, theme: &crate::theme::Theme) {
    let css = crate::theme::generate_css(theme, &state.config.font_family, state.config.font_size);
    state.css_provider.load_from_string(&css);

    // Update dim tag foreground
    state.dim_tag.set_property("foreground", &theme.dim_fg);
    state.ab_dim_tag.set_property("foreground", &theme.dim_fg);

    // Write .current_theme file
    let home = std::env::var("HOME").unwrap_or_default();
    let theme_path = std::path::PathBuf::from(&home)
        .join("utono/themes/.config/themes/.current_theme");
    let _ = std::fs::write(&theme_path, &theme.name);

    state.theme = theme.clone();

    crate::logging::log(&format!("SETTINGS: theme changed to {}", theme.display_name));
}
