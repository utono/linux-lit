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
    pub pending_z: bool,
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
        state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceWordPicker;
        return true;
    }

    // Ctrl+Alt+p: open concordance occurrence list
    if is_ctrl && is_alt && key_name == "p" {
        let s = state.borrow();
        if let Some(conc) = &s.concordance_state {
            s.concordance_list_picker
                .show(&conc.occurrences, conc.current_index);
        }
        drop(s);
        state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceListPicker;
        return true;
    }

    // Ctrl+p: open picker when hidden (also clears concordance mode)
    if is_ctrl && key_name == "p" && !picker_visible
        && !state.borrow().bookmark_picker.is_visible()
        && !state.borrow().media_picker.is_visible()
        && !state.borrow().settings_overlay.is_visible()
    {
        {
            let mut s = state.borrow_mut();
            s.concordance_state = None;
            s.concordance_bar.hide();
        }
        state.borrow().correction_overlay.hide();
        state.borrow_mut().picker.show_prepare();
        state.borrow().picker.show_finish();
        state.borrow_mut().input_mode = crate::app::InputMode::LibraryPicker;
        return true;
    }

    // Mode dispatch — delegate to per-mode handler functions
    let mode = state.borrow().input_mode;
    if mode != crate::app::InputMode::Reader {
        return match mode {
            crate::app::InputMode::LibraryPicker => handle_library_picker_key(state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::BookmarkPicker
            | crate::app::InputMode::MediaPicker
            | crate::app::InputMode::ConcordancePicker
            | crate::app::InputMode::ConcordanceWordPicker
            | crate::app::InputMode::ConcordanceListPicker => handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
            crate::app::InputMode::Settings => handle_settings_key(state, key_name, is_ctrl),
            crate::app::InputMode::Search => handle_search_key(state, key_name),
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_name),
            crate::app::InputMode::GamepadOverlay => handle_gamepad_key(state, key_name),
            crate::app::InputMode::KeybindsOverlay => handle_keybinds_key(state, key_state, key_name),
            crate::app::InputMode::ActionPopup => handle_action_popup_key(state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name),
            crate::app::InputMode::Reader => unreachable!(),
        };
    }

    // --- Reader mode (no overlay) ---

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

    // zt sequence check
    if key_state.borrow().pending_z {
        key_state.borrow_mut().pending_z = false;
        if key_name == "t" {
            navigation::scroll_cursor_top(&mut state.borrow_mut());
            return true;
        }
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

    // Shift+Tab: toggle gloss overlay for last-viewed gloss
    if key_name == "ISO_Left_Tab" {
        let has_gloss = state.borrow().gloss_saved.is_some();
        if has_gloss {
            let s = state.borrow();
            let ctx = s.gloss_context.as_ref().unwrap();
            let saved = s.gloss_saved.as_ref().unwrap();
            let h = s.scrolled_window.height();
            s.correction_overlay.show_gloss(&ctx.source_text, &saved.gloss_text, h);
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
            return true;
        }
        return false;
    }

    // Escape: special multi-state handler (concordance, AB loop, search clear).
    // Stays inline — multi-state preconditions don't fit the static Action model.
    if key_name == "Escape" {
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
            return true;
        } else if !s.search_matches.is_empty() {
            crate::input::search::clear_search(&mut s);
            return true;
        } else {
            return false;
        }
    }

    // Keymap-driven dispatch for everything else. Scope the borrow tightly
    // so it drops before dispatch_action borrows state itself.
    let action = state.borrow().keymap.lookup(key_name, is_ctrl, is_shift, is_alt);
    if let Some(action) = action {
        return dispatch_action(state, action, key_state, tokio_handle);
    }

    false
}

// ---------------------------------------------------------------------------
// Per-mode handler functions
// ---------------------------------------------------------------------------

fn handle_library_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
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
                    state.borrow_mut().input_mode = crate::app::InputMode::Reader;
                }
            }
            true
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
                    true
                }
                crate::ui::library_picker::PickerLevel::Works(_) => {
                    crate::input::actions::pickers::load_selected_work(state, tokio_handle);
                    true
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
            false
        }
        "Down" => {
            state.borrow().picker.move_selection(1);
            true
        }
        "Up" => {
            state.borrow().picker.move_selection(-1);
            true
        }
        _ => {
            // Ctrl+n/p already handled at top of handle_key; let GTK route
            // remaining keys to the search entry.
            if is_ctrl {
                // Ctrl combos not handled above — don't let GTK insert text
                false
            } else {
                false
            }
        }
    }
}

fn handle_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
    mode: crate::app::InputMode,
) -> bool {
    use crate::app::InputMode;
    use crate::input::picker_keys::{resolve_picker_key, PickerAction};

    match resolve_picker_key(key_name, is_ctrl) {
        PickerAction::Hide => {
            let mut s = state.borrow_mut();
            match mode {
                InputMode::BookmarkPicker => { s.bookmark_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::MediaPicker => { s.media_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordancePicker => { s.concordance_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordanceWordPicker => { s.concordance_word_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordanceListPicker => { s.concordance_list_picker.hide(); s.input_mode = InputMode::Reader; }
                _ => {}
            }
            true
        }
        PickerAction::Confirm => {
            match mode {
                InputMode::BookmarkPicker => {
                    let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
                    if let Some(lm_id) = selected_id {
                        {
                            let s = state.borrow();
                            s.bookmark_picker.hide();
                        }
                        let mut s = state.borrow_mut();
                        s.input_mode = InputMode::Reader;
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
                    true
                }
                InputMode::MediaPicker => {
                    crate::input::actions::pickers::confirm_media_selection(state, tokio_handle);
                    true
                }
                InputMode::ConcordancePicker => {
                    let selected = state.borrow().concordance_picker.selected_word();
                    state.borrow().concordance_picker.hide();
                    state.borrow_mut().input_mode = InputMode::Reader;
                    if let Some(word) = selected {
                        crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
                    }
                    true
                }
                InputMode::ConcordanceWordPicker => {
                    let selected = state.borrow().concordance_word_picker.selected_word();
                    state.borrow().concordance_word_picker.hide();
                    state.borrow_mut().input_mode = InputMode::Reader;
                    if let Some(word) = selected {
                        crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
                    }
                    true
                }
                InputMode::ConcordanceListPicker => {
                    let selected = state.borrow().concordance_list_picker.selected_index();
                    state.borrow().concordance_list_picker.hide();
                    state.borrow_mut().input_mode = InputMode::Reader;
                    if let Some(idx) = selected {
                        {
                            let mut s = state.borrow_mut();
                            if let Some(conc) = &mut s.concordance_state {
                                conc.current_index = idx;
                            }
                        }
                        navigation::concordance_jump_to_current(state, tokio_handle);
                    }
                    true
                }
                _ => true,
            }
        }
        PickerAction::MoveDown => {
            match mode {
                InputMode::BookmarkPicker => state.borrow().bookmark_picker.move_selection(1),
                InputMode::MediaPicker => state.borrow().media_picker.move_selection(1),
                InputMode::ConcordancePicker => state.borrow().concordance_picker.move_selection(1),
                InputMode::ConcordanceWordPicker => state.borrow().concordance_word_picker.move_selection(1),
                InputMode::ConcordanceListPicker => state.borrow().concordance_list_picker.move_selection(1),
                _ => {}
            }
            true
        }
        PickerAction::MoveUp => {
            match mode {
                InputMode::BookmarkPicker => state.borrow().bookmark_picker.move_selection(-1),
                InputMode::MediaPicker => state.borrow().media_picker.move_selection(-1),
                InputMode::ConcordancePicker => state.borrow().concordance_picker.move_selection(-1),
                InputMode::ConcordanceWordPicker => state.borrow().concordance_word_picker.move_selection(-1),
                InputMode::ConcordanceListPicker => state.borrow().concordance_list_picker.move_selection(-1),
                _ => {}
            }
            true
        }
        PickerAction::Unhandled => {
            // Per-mode extras
            match mode {
                InputMode::BookmarkPicker => {
                    if key_name == "Delete" || key_name == "d" {
                        crate::input::actions::pickers::delete_bookmark(state, tokio_handle);
                        return true;
                    }
                }
                InputMode::MediaPicker => {
                    if key_name == "p" {
                        let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
                        if !is_search_focused {
                            crate::input::actions::pickers::set_media_default(state, tokio_handle);
                            return true;
                        }
                    }
                }
                _ => {}
            }
            false
        }
    }
}

fn handle_settings_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    use crate::input::picker_keys::{resolve_picker_key, PickerAction};
    match resolve_picker_key(key_name, is_ctrl) {
        PickerAction::Hide => {
            crate::input::actions::settings::revert_to_snapshot(state);
            true
        }
        PickerAction::Confirm => {
            let mut s = state.borrow_mut();
            crate::config::save(&s.config);
            s.settings_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        PickerAction::MoveDown => {
            state.borrow_mut().settings_overlay.move_selection(1);
            true
        }
        PickerAction::MoveUp => {
            state.borrow_mut().settings_overlay.move_selection(-1);
            true
        }
        PickerAction::Unhandled => {
            match key_name {
                "h" | "Left" => {
                    let (ls, cw, tm, nm, ts, cl) = {
                        let s = state.borrow();
                        (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style, s.config.show_cursor_line)
                    };
                    let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm, nm, ts, cl);
                    crate::input::actions::settings::apply_settings_change(state, change);
                    true
                }
                "l" | "Right" => {
                    let (ls, cw, tm, nm, ts, cl) = {
                        let s = state.borrow();
                        (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style, s.config.show_cursor_line)
                    };
                    let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm, nm, ts, cl);
                    crate::input::actions::settings::apply_settings_change(state, change);
                    true
                }
                "r" => {
                    crate::input::actions::settings::reset_to_defaults(state);
                    true
                }
                _ => true, // consume all other keys when settings visible
            }
        }
    }
}

fn handle_search_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "Escape" => {
            crate::input::search::clear_search(&mut state.borrow_mut());
            state.borrow().search_bar.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        "Return" => {
            crate::input::search::execute_search(&state);
            state.borrow().search_bar.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        "Tab" => {
            crate::input::search::toggle_playback(&mut state.borrow_mut());
            true
        }
        _ => false, // let GTK route to the Entry
    }
}

fn handle_gloss_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "r" => {
            regenerate_gloss(state);
            true
        }
        "a" => {
            show_amend_dialog(state);
            true
        }
        "j" => {
            state.borrow().correction_overlay.scroll_gloss(1);
            true
        }
        "k" => {
            state.borrow().correction_overlay.scroll_gloss(-1);
            true
        }
        "Escape" | "n" => {
            {
                let mut s = state.borrow_mut();
                s.correction_overlay.hide();
                s.input_mode = crate::app::InputMode::Reader;
            }
            true
        }
        _ => true,
    }
}

fn handle_gamepad_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "Escape" => {
            state.borrow().gamepad_overlay.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        _ => true, // consume all other keys when gamepad overlay visible
    }
}

fn handle_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "Escape" => {
            state.borrow().keybinds_overlay.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        "g" if key_state.borrow().pending_ctrl_slash => {
            // C-/ g chord: swap to gamepad overlay
            key_state.borrow_mut().pending_ctrl_slash = false;
            let s = state.borrow();
            s.keybinds_overlay.hide();
            s.gamepad_overlay.show();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GamepadOverlay;
            true
        }
        "exclam" => {
            state.borrow_mut().keybinds_overlay.adjust_scale(-1);
            true
        }
        "bar" => {
            state.borrow_mut().keybinds_overlay.adjust_scale(1);
            true
        }
        "0" => {
            state.borrow_mut().keybinds_overlay.reset_scale();
            true
        }
        _ => true, // consume all other keys when keybinds visible
    }
}

fn handle_action_popup_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    if is_ctrl {
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
    match key_name {
        "Return" => {
            let selected_idx = state.borrow().action_popup_widget.selected_index();
            crate::input::visual::close_action_popup(&mut state.borrow_mut());
            crate::input::visual::execute_action(state, selected_idx, tokio_handle);
            true
        }
        "Escape" => {
            crate::input::visual::close_action_popup(&mut state.borrow_mut());
            true
        }
        _ => true, // consume all keys when popup visible
    }
}

fn handle_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "j" => {
            crate::input::visual::move_selection_cursor(&mut state.borrow_mut(), 1);
            true
        }
        "k" => {
            crate::input::visual::move_selection_cursor(&mut state.borrow_mut(), -1);
            true
        }
        "G" => {
            crate::input::visual::extend_to_end(&mut state.borrow_mut());
            true
        }
        "g" => {
            // In visual mode, 'g' starts gg sequence to extend to start
            key_state.borrow_mut().pending_g = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_g = false;
            });
            true
        }
        "Escape" | "V" => {
            crate::input::visual::exit_visual_mode(&mut state.borrow_mut());
            true
        }
        "y" => {
            crate::input::visual::yank_selection(&mut state.borrow_mut());
            true
        }
        "Return" => {
            crate::input::visual::open_action_popup(&mut state.borrow_mut());
            true
        }
        _ => {
            // Consume all other keys in visual mode
            true
        }
    }
}

/// Execute an Action by calling its corresponding verb. Returns true if the
/// action was dispatched (currently always true for known actions; false
/// branch reserved for future "action exists but precondition failed" cases).
fn dispatch_action(
    state: &Rc<RefCell<AppState>>,
    action: crate::input::actions::Action,
    key_state: &Rc<RefCell<KeyState>>,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    crate::logging::log(&format!("ACTION: {}", action.name()));
    use crate::input::actions::Action::*;
    match action {
        // Page navigation
        PageForward => { navigation::page_forward(&mut state.borrow_mut()); true }
        PageBackward => { navigation::page_backward(&mut state.borrow_mut()); true }
        PageBackwardBottom => { navigation::page_backward_bottom(&mut state.borrow_mut()); true }
        JumpToStart => { navigation::jump_to_start(&mut state.borrow_mut()); true }
        JumpToEnd => { navigation::jump_to_end(&mut state.borrow_mut()); true }

        // Cursor / dialogue
        CursorNextDialogue => { navigation::cursor_next_dialogue(&mut state.borrow_mut()); true }
        CursorPrevLine => { navigation::cursor_prev_line(&mut state.borrow_mut()); true }
        CursorToPageBottom => { navigation::cursor_to_page_bottom(&mut state.borrow_mut()); true }
        JumpToNextDialogue => { navigation::jump_to_next_dialogue(&mut state.borrow_mut()); true }
        JumpToPrevDialogue => { navigation::jump_to_prev_dialogue(&mut state.borrow_mut()); true }
        JumpToNextChapter => { navigation::jump_to_next_chapter(&mut state.borrow_mut()); true }
        JumpToPrevChapter => { navigation::jump_to_prev_chapter(&mut state.borrow_mut()); true }
        JumpToNextScene => { navigation::jump_to_next_section(&mut state.borrow_mut()); true }
        JumpToPrevScene => { navigation::jump_to_prev_section(&mut state.borrow_mut()); true }

        // Bookmarks
        ToggleBookmark => { crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle); true }
        NextBookmark => { navigation::next_bookmark(&mut state.borrow_mut()); true }
        PrevBookmark => { navigation::prev_bookmark(&mut state.borrow_mut()); true }
        JumpToRecentBookmark => { crate::input::actions::bookmarks::jump_to_recent_bookmark(state, tokio_handle); true }
        OpenBookmarkPicker => { crate::input::actions::pickers::open_bookmark_picker(state, tokio_handle); true }

        // Pickers / overlays
        OpenLibraryPicker => {
            let s = state.borrow();
            if !s.picker.is_visible()
                && !s.bookmark_picker.is_visible()
                && !s.media_picker.is_visible()
                && !s.settings_overlay.is_visible()
            {
                drop(s);
                let mut sm = state.borrow_mut();
                sm.concordance_state = None;
                sm.concordance_bar.hide();
                drop(sm);
                state.borrow().correction_overlay.hide();
                state.borrow_mut().picker.show_prepare();
                state.borrow().picker.show_finish();
                state.borrow_mut().input_mode = crate::app::InputMode::LibraryPicker;
            }
            true
        }
        OpenMediaPicker => { crate::input::actions::pickers::open_media_picker(state, tokio_handle); true }
        OpenConcordancePicker => { crate::input::actions::concordance::open_picker(state, tokio_handle); true }
        OpenConcordanceWordPicker => {
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
            state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceWordPicker;
            true
        }
        OpenConcordanceListPicker => {
            let s = state.borrow();
            if let Some(conc) = &s.concordance_state {
                s.concordance_list_picker.show(&conc.occurrences, conc.current_index);
            }
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceListPicker;
            true
        }
        OpenSettingsOverlay => {
            let s = state.borrow();
            if !s.settings_overlay.is_visible() && !s.picker.is_visible() {
                s.correction_overlay.hide();
                let ls = s.config.line_spacing;
                let cw = s.config.column_width;
                let tm = s.config.text_margins;
                let nm = s.config.navigation_mode;
                let ts = s.config.transition_style;
                let cl = s.config.show_cursor_line;
                drop(s);
                state.borrow_mut().settings_overlay.show(ls, cw, tm, nm, ts, cl);
                state.borrow_mut().input_mode = crate::app::InputMode::Settings;
            }
            true
        }
        OpenKeybindsOverlay => {
            let s = state.borrow();
            if s.keybinds_overlay.is_visible() || s.gamepad_overlay.is_visible() {
                s.keybinds_overlay.hide();
                s.gamepad_overlay.hide();
                drop(s);
                state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            } else {
                s.picker.hide();
                s.media_picker.hide();
                s.settings_overlay.hide();
                s.search_bar.hide();
                s.correction_overlay.hide();
                s.keybinds_overlay.show();
                drop(s);
                state.borrow_mut().input_mode = crate::app::InputMode::KeybindsOverlay;
            }
            key_state.borrow_mut().pending_ctrl_slash = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_ctrl_slash = false;
            });
            true
        }
        OpenSearch => {
            let mut s = state.borrow_mut();
            crate::input::search::clear_search(&mut s);
            s.search_bar.show();
            s.input_mode = crate::app::InputMode::Search;
            true
        }

        // MPV / media
        TogglePlaybackSync => {
            let mut s = state.borrow_mut();
            s.sync_enabled = !s.sync_enabled;
            s.sync_icon.set_visible(!s.sync_enabled);
            crate::logging::log(&format!("SYNC: {}", if s.sync_enabled { "enabled" } else { "disabled" }));
            true
        }
        TogglePlayback => { crate::input::search::toggle_playback(&mut state.borrow_mut()); true }
        SeekShortBackward => { do_mpv_seek(state, -3.5); true }
        SeekShortForward => { do_mpv_seek(state, 3.5); true }
        SeekLongBackward => { do_mpv_seek(state, -60.0); true }
        SeekLongForward => { do_mpv_seek(state, 60.0); true }
        SeekBackward30 => { do_mpv_seek(state, -30.0); true }
        VolumeUp => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0)); true }
        VolumeDown => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0)); true }
        TogglePlaybackSpeed => {
            let mut s = state.borrow_mut();
            let new_speed = if s.playback_speed == 1.0 { 1.3 } else { 1.0 };
            s.playback_speed = new_speed;
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetSpeed(new_speed));
            crate::logging::log(&format!("SPEED: toggled to {}x", new_speed));
            true
        }

        // Vocab / glossing
        ToggleVocabPopup => {
            let mut s = state.borrow_mut();
            s.vocab_popup_auto = !s.vocab_popup_auto;
            if s.vocab_popup_auto {
                crate::app::open_vocab_popup(&mut s);
            } else {
                crate::app::close_vocab_popup(&mut s);
            }
            true
        }
        VocabPopupNext => {
            // Same inline logic as the original "backslash" / "numbersign"
            // arms; the auto-hide timer handling is preserved.
            handle_vocab_popup_key(state, true);
            true
        }
        VocabPopupPrev => {
            handle_vocab_popup_key(state, false);
            true
        }
        JumpToNextVocab => {
            let has_concordance = state.borrow().concordance_state.is_some();
            if has_concordance {
                let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
                let advanced = {
                    let mut s = state.borrow_mut();
                    if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                        conc.advance_within_work(abbrev)
                    } else { false }
                };
                if advanced {
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
            } else {
                navigation::jump_to_next_vocab(&mut state.borrow_mut());
            }
            true
        }
        JumpToPrevVocab => {
            let has_concordance = state.borrow().concordance_state.is_some();
            if has_concordance {
                let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
                let retreated = {
                    let mut s = state.borrow_mut();
                    if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                        conc.retreat_within_work(abbrev)
                    } else { false }
                };
                if retreated {
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
            } else {
                navigation::jump_to_prev_vocab(&mut state.borrow_mut());
            }
            true
        }
        ToggleVocabHighlight => {
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
            true
        }

        // Visual / selection
        EnterVisualMode => { crate::input::visual::enter_visual_mode(&mut state.borrow_mut()); true }
        WordCycleCopy => { navigation::word_cycle_copy(&mut state.borrow_mut()); true }
        WordCollectCopy => { navigation::word_collect_copy(&mut state.borrow_mut()); true }

        // Translations
        ToggleTranslations => {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            drop(s);
            crate::app::toggle_translations(&mut state.borrow_mut());
            true
        }

        // Settings (in reader)
        AdjustFontSizeUp => { crate::app::adjust_font_size(&mut state.borrow_mut(), 1); crate::app::show_font_info(&state.borrow()); true }
        AdjustFontSizeDown => { crate::app::adjust_font_size(&mut state.borrow_mut(), -1); crate::app::show_font_info(&state.borrow()); true }
        ResetFontSize => { crate::app::reset_font_size(&mut state.borrow_mut()); true }
        CycleFontForward => { crate::app::cycle_font(&mut state.borrow_mut(), true); true }
        CycleFontBackward => { crate::app::cycle_font(&mut state.borrow_mut(), false); true }
        ToggleSignColumn => { crate::app::toggle_sign_column(&mut state.borrow_mut()); true }
        ToggleCursorLine => {
            let mut s = state.borrow_mut();
            s.config.show_cursor_line = !s.config.show_cursor_line;
            crate::input::navigation::update_highlight_only(&mut s);
            crate::config::save(&s.config);
            true
        }
        ToggleDim => {
            let mut s = state.borrow_mut();
            s.dim_enabled = !s.dim_enabled;
            if !s.dim_enabled {
                let (start, end) = s.buffer.bounds();
                s.buffer.remove_tag(&s.dim_tag, &start, &end);
            }
            navigation::update_highlight_only(&mut s);
            s.config.dim_enabled = s.dim_enabled;
            crate::config::save(&s.config);
            crate::logging::log(&format!("DIM: {}", if s.dim_enabled { "on" } else { "off" }));
            true
        }
        ShowFontInfo => { crate::app::show_font_info(&state.borrow()); true }

        // Timestamps
        SetStartTime => { crate::input::timestamps::set_start_time(&mut state.borrow_mut()); true }
        SetEndTime => crate::input::timestamps::set_end_time(&mut state.borrow_mut()),
        SetChapter => crate::input::timestamps::set_chapter(&mut state.borrow_mut()),
        DeleteTimestamp => crate::input::timestamps::delete_timestamp(&mut state.borrow_mut()),
        NudgeStartBackward => crate::input::timestamps::nudge_start_backward(&mut state.borrow_mut()),
        NudgeStartForward => crate::input::timestamps::nudge_start_forward(&mut state.borrow_mut()),
        UndoTimestamp => crate::input::timestamps::undo_timestamp(&mut state.borrow_mut()),
        PlayCurrentLine => { crate::input::timestamps::play_current_line(&mut state.borrow_mut()); true }

        // App
        SaveAndQuit => {
            crate::app::save_position(&mut state.borrow_mut());
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
            state.borrow().window.close();
            true
        }
        ToggleDebugLogging => {
            let enabled = !crate::logging::debug_mode();
            crate::logging::set_debug_mode(enabled);
            crate::logging::log_always(&format!("DEBUG_MODE: {}", if enabled { "on" } else { "off" }));
            let icon = state.borrow().debug_icon.clone();
            icon.set_label(if enabled { "⚙" } else { "⊘" });
            icon.set_visible(true);
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                icon.set_visible(false);
            });
            true
        }
        CopyLineMappingId => {
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
            true
        }

        // Multi-key chord entry
        PendingG => {
            key_state.borrow_mut().pending_g = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_g = false;
            });
            true
        }
        PendingZ => {
            key_state.borrow_mut().pending_z = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_z = false;
            });
            true
        }

        // Search (in reader, when matches present)
        SearchNextMatch => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::next_match(&mut state.borrow_mut());
                true
            } else {
                false
            }
        }
        SearchPrevMatch => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::prev_match(&mut state.borrow_mut());
                true
            } else {
                false
            }
        }
    }
}

/// MPV seek with brief sync suppression. Common pattern for o/e/O/E/Left.
fn do_mpv_seek(state: &Rc<RefCell<AppState>>, offset: f64) {
    let mut s = state.borrow_mut();
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SeekRelative(offset));
    s.suppress_sync_until = Some(
        std::time::Instant::now() + std::time::Duration::from_millis(500),
    );
}

/// Vocab popup key handler with auto-hide timer reset.
fn handle_vocab_popup_key(state: &Rc<RefCell<AppState>>, forward: bool) {
    let popup_visible = state.borrow().vocab_popup.is_visible();
    if popup_visible {
        if forward {
            crate::app::vocab_popup_next(&mut state.borrow_mut());
        } else {
            crate::app::vocab_popup_prev(&mut state.borrow_mut());
        }
    } else {
        crate::app::open_vocab_popup(&mut state.borrow_mut());
    }
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
            return;
        }
        if !s.vocab_popup.is_visible() {
            return;
        }
        let widget = s.vocab_popup.widget().clone();
        let target = adw::CallbackAnimationTarget::new(move |value| {
            widget.set_opacity(value as f64);
            if value <= 0.0 {
                widget.set_visible(false);
                widget.set_opacity(1.0);
            }
        });
        let anim = adw::TimedAnimation::new(
            s.vocab_popup.widget(),
            1.0, 0.0, 500, target,
        );
        anim.set_easing(adw::Easing::EaseOutQuad);
        anim.play();
    });
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

fn regenerate_gloss(state_rc: &Rc<RefCell<AppState>>) {
    let (ctx, model, tokio_handle, old_gloss_id) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let old_id = state.gloss_saved.as_ref().map(|g| g.gloss_id);
        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), old_id)
    };

    if let Some(gid) = old_gloss_id {
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::delete_gloss(&conn, gid);
        }
    }

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_saved = None;
        s.correction_overlay.show_loading();
    }

    let user_msg = crate::gloss::build_user_message(&ctx, None, None);
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude(&user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn,
                        &ctx.hash,
                        &ctx.work_abbrev,
                        &ctx.start_citation,
                        &ctx.end_citation,
                        ctx.act,
                        ctx.scene,
                        &ctx.speaker,
                        &ctx.source_text,
                        &gloss_text,
                    );
                }

                let saved = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_existing_gloss(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                        ).ok().flatten()
                    });

                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                s.correction_overlay.show_gloss(&ctx.source_text, &gloss_text, h);
                s.gloss_saved = saved;
                crate::logging::log("GLOSS: regenerated");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.correction_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: regenerate error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("GLOSS: tokio join error: {}", e));
            }
        }
    });
}

fn show_amend_dialog(state_rc: &Rc<RefCell<AppState>>) {
    let window = {
        let s = state_rc.borrow();
        s.text_view.root().and_then(|r| r.downcast::<gtk4::Window>().ok())
    };
    let parent_window = match window {
        Some(w) => w,
        None => return,
    };

    let dialog = gtk4::Window::builder()
        .title("Enhancement prompt")
        .transient_for(&parent_window)
        .modal(true)
        .default_width(450)
        .default_height(160)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let label = gtk4::Label::new(Some("Enhancement prompt:"));
    label.set_halign(gtk4::Align::Start);
    vbox.append(&label);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_min_content_height(80);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_top_margin(6);
    text_view.set_bottom_margin(6);
    text_view.set_left_margin(6);
    text_view.set_right_margin(6);
    scrolled.set_child(Some(&text_view));
    vbox.append(&scrolled);

    let hint = gtk4::Label::new(Some("Ctrl+Enter to submit  \u{00b7}  Esc to cancel"));
    hint.add_css_class("dim-label");
    hint.set_halign(gtk4::Align::Start);
    vbox.append(&hint);

    dialog.set_child(Some(&vbox));

    let state_for_key = Rc::clone(state_rc);
    let dialog_weak = dialog.downgrade();
    let tv_clone = text_view.clone();

    let key_controller = gtk4::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_ctrl, keyval, _code, modifier| {
        let key_name = keyval.name().unwrap_or_default();
        if key_name == "Escape" {
            if let Some(d) = dialog_weak.upgrade() {
                d.close();
            }
            return glib::Propagation::Stop;
        }
        if key_name == "Return" && modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
            let d = match dialog_weak.upgrade() {
                Some(d) => d,
                None => return glib::Propagation::Stop,
            };
            let buf = tv_clone.buffer();
            let prompt = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            d.close();
            if !prompt.trim().is_empty() {
                amend_gloss(&state_for_key, &prompt);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dialog.add_controller(key_controller);

    dialog.present();
}

fn amend_gloss(state_rc: &Rc<RefCell<AppState>>, prompt: &str) {
    let (ctx, model, tokio_handle, existing) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let existing = match &state.gloss_saved {
            Some(g) => g.clone(),
            None => return,
        };
        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), existing)
    };

    state_rc.borrow().correction_overlay.show_loading();

    let prompt_owned = prompt.to_string();
    let user_msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), Some(&existing.gloss_text));
    let state_for_result = Rc::clone(state_rc);
    let gloss_id = existing.gloss_id;

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude(&user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::update_gloss(&conn, gloss_id, &gloss_text);
                }

                let saved = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_existing_gloss(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                        ).ok().flatten()
                    });

                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                s.correction_overlay.show_gloss(&ctx.source_text, &gloss_text, h);
                s.gloss_saved = saved;
                crate::logging::log("GLOSS: amended");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.correction_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: amend error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("GLOSS: tokio join error: {}", e));
            }
        }
    });
}
