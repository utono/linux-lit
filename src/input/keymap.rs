use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::AnimationExt;

use crate::app::AppState;
use crate::input::navigation;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ChordState {
    #[default]
    None,
    PendingG,
    PendingZ,
}

#[derive(Default)]
pub struct KeyState {
    pub chord: ChordState,
}

impl KeyState {
    pub fn start_chord(key_state: &Rc<RefCell<KeyState>>, chord: ChordState) {
        key_state.borrow_mut().chord = chord;
        let ks = Rc::clone(key_state);
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if ks.borrow().chord == chord {
                ks.borrow_mut().chord = ChordState::None;
            }
        });
    }
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

    // Shift+Ctrl+L: quit from any mode. GTK delivers the shifted letter as the
    // uppercase name "L" (with shift=true), so match that; also accept "l" for
    // layouts that report the unshifted name.
    if is_shift && is_ctrl && (key_name == "L" || key_name == "l") {
        crate::app::save_position(&mut state.borrow_mut());
        let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
        state.borrow().window.close();
        return true;
    }

    // Spacebar (no modifiers) toggles MPV play/pause from any mode, UNLESS a
    // text-input widget has focus (an Entry, or an editable TextView), in which
    // case space must type a literal space. The reader's main TextView is
    // non-editable, so it does not block this.
    // Exception: GlossOverlay intercepts Space to read the cursor's explication
    // paragraph aloud via handle_gloss_key — do not intercept there.
    // Spacebar (no modifiers) replicates the `a` bind on each surface:
    // begin playback from the cursor line's start time. This block handles the
    // Reader (main card) case; overlays handle space in their own arms so each
    // surface's space matches that surface's `a`. Guards stay here because they
    // gate Search and Gloss before their handlers run:
    //  - editable widget focus (Entry / editable TextView / Search) → space
    //    must type a literal space, so let GTK route it (return false);
    //  - GlossOverlay → its handler owns space (read-block), so skip here.
    // For any other non-editable mode, fall through to mode dispatch.
    if key_name == "space" && !is_ctrl && !is_shift && !is_alt {
        let s = state.borrow();
        let mode = s.input_mode;
        let gloss_open = mode == crate::app::InputMode::GlossOverlay;
        let focus_is_editable = mode == crate::app::InputMode::Search
            || gtk4::prelude::GtkWindowExt::focus(&s.window).is_some_and(|w| {
                w.is::<gtk4::Entry>()
                    || w.downcast_ref::<gtk4::TextView>()
                        .is_some_and(|tv| tv.is_editable())
            });
        drop(s);
        if focus_is_editable {
            return false; // type a literal space in the text field
        }
        if !gloss_open && mode == crate::app::InputMode::Reader {
            let mut s = state.borrow_mut();
            if !crate::input::timestamps::play_current_line(&mut s) {
                show_no_timestamp_toast(&s);
            }
            return true;
        }
        // Non-editable, non-Reader, non-gloss (e.g. an overlay): fall through
        // to mode dispatch so the overlay's own space arm runs.
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
            | crate::app::InputMode::EchoLinePicker
            | crate::app::InputMode::ConcordanceListPicker
            | crate::app::InputMode::ConcordanceWorksPicker
            | crate::app::InputMode::AuthorshipPicker
            | crate::app::InputMode::GlossPicker => handle_picker_key(state, key_name, is_ctrl, is_alt, tokio_handle, mode),
            crate::app::InputMode::Settings => handle_settings_key(state, key_name, is_ctrl),
            crate::app::InputMode::VoicePicker => handle_voice_picker_key(state, key_name, is_ctrl),
            crate::app::InputMode::Search => handle_search_key(state, key_name),
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_state, key_name, is_ctrl, is_shift, is_alt, tokio_handle),
            crate::app::InputMode::GlossVisual => handle_gloss_visual_key(state, key_state, key_name),
            crate::app::InputMode::SynopsisOverlay => handle_synopsis_overlay_key(state, key_state, key_name, is_ctrl, is_alt, is_shift),
            crate::app::InputMode::SynopsisVisual => handle_synopsis_visual_key(state, key_state, key_name),
            crate::app::InputMode::TranslationOverlay => handle_translation_overlay_key(state, key_name),
            crate::app::InputMode::DeleteConfirm => handle_delete_confirm_key(state, key_name),
            crate::app::InputMode::EchoPicker => handle_echo_picker_key(state, key_name, tokio_handle),
            crate::app::InputMode::EchoTurnsPicker => handle_echo_turns_picker_key(state, key_name, tokio_handle),
            crate::app::InputMode::EchoesOverlay => handle_echoes_overlay_key(state, key_state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::GamepadOverlay => handle_gamepad_key(state, key_name),
            crate::app::InputMode::KeybindsOverlay => handle_keybinds_key(state, key_name),
            crate::app::InputMode::EchoKeybindsOverlay => handle_echo_keybinds_key(state, key_name, is_ctrl),
            crate::app::InputMode::ActionPopup => handle_action_popup_key(state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name, tokio_handle),
            crate::app::InputMode::PageCalibration => handle_page_calibration_key(state, key_state, key_name),
            crate::app::InputMode::Reader => unreachable!(),
        };
    }

    // --- Reader mode (no overlay) ---

    // gg sequence check
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
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
    if key_state.borrow().chord == ChordState::PendingZ {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "t" {
            navigation::scroll_cursor_top(&mut state.borrow_mut());
            return true;
        }
    }

    // Keymap-driven dispatch. Scope the borrow tightly
    // so it drops before dispatch_action borrows state itself.
    let action = state.borrow().keymap.lookup(key_name, is_ctrl, is_shift, is_alt);
    if let Some(action) = action {
        dispatch_action(state, action, key_state, tokio_handle);
        // If the card is in page-image mode, keep the shown leaf in sync with the
        // cursor the action just moved (cheap no-op when image_mode is off).
        crate::app::refresh_page_image(state);
        return true;
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
        "n" if is_ctrl => {
            state.borrow().picker.move_selection(1);
            true
        }
        "p" if is_ctrl => {
            state.borrow().picker.move_selection(-1);
            true
        }
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
            // Let GTK route remaining keys to the search entry.
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
    is_alt: bool,
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
                InputMode::ConcordanceWorksPicker => { s.concordance_works_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::GlossPicker => {
                    s.gloss_picker.hide();
                    // If the picker was opened from within the gloss overlay
                    // (Alt+g), the overlay is still visible behind it — return to
                    // it rather than dropping to the reader.
                    if s.gloss_picker_from_overlay {
                        s.gloss_picker_from_overlay = false;
                        s.input_mode = InputMode::GlossOverlay;
                    } else {
                        s.input_mode = InputMode::Reader;
                    }
                }
                InputMode::AuthorshipPicker => { s.authorship_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::EchoLinePicker => { drop(s); crate::input::actions::echoes::cancel_add_echo(state); }
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
                        crate::input::actions::concordance::concordance_jump_to_current(state, tokio_handle);
                    }
                    true
                }
                InputMode::ConcordanceWorksPicker => {
                    let selected = state.borrow().concordance_works_picker.selected_abbrev();
                    state.borrow().concordance_works_picker.hide();
                    state.borrow_mut().input_mode = InputMode::Reader;
                    if let Some(abbrev) = selected {
                        let first_idx = state.borrow().concordance_state.as_ref()
                            .and_then(|c| c.occurrences.iter().position(|h| h.work_abbrev == abbrev));
                        if let Some(idx) = first_idx {
                            {
                                let mut s = state.borrow_mut();
                                if let Some(conc) = &mut s.concordance_state {
                                    conc.current_index = idx;
                                }
                            }
                            crate::input::actions::concordance::concordance_jump_to_current(state, tokio_handle);
                        }
                    }
                    true
                }
                InputMode::GlossPicker => {
                    let selected = state.borrow().gloss_picker.selected_index();
                    if let Some(idx) = selected {
                        let passage = state.borrow().gloss_picker.items[idx].clone();
                        {
                            let mut s = state.borrow_mut();
                            s.gloss_picker.hide();
                            // Confirming opens a fresh gloss overlay below, so the
                            // "from overlay" return path no longer applies.
                            s.gloss_picker_from_overlay = false;
                        }

                        let all_glosses = crate::db::queries::open_db()
                            .ok()
                            .and_then(|conn| {
                                crate::db::queries::find_glosses_by_start(
                                    &conn, &passage.work_abbrev,
                                    &passage.start_citation,
                                    &["teacher-generic", "inner-monologue", "reader-gloss"],
                                ).ok()
                            })
                            .unwrap_or_default();

                        if all_glosses.is_empty() {
                            state.borrow_mut().input_mode = InputMode::Reader;
                            return true;
                        }

                        let mut s = state.borrow_mut();
                        let passages = s.gloss_picker.items.clone();
                        // Remember where the reader was so Escape returns here
                        // (instead of jumping to the glossed passage).
                        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
                        // Shared open path (also used by the cursor open) — from
                        // the picker, so Escape uses the picker return path.
                        crate::input::actions::gloss::open_gloss_overlay(
                            &mut s, passages, idx, passage, all_glosses, true, None,
                        );
                    }
                    true
                }
                InputMode::AuthorshipPicker => {
                    crate::input::actions::authorship::confirm_attribution_selection(state);
                    true
                }
                InputMode::EchoLinePicker => {
                    crate::input::actions::echoes::confirm_add_echo(state);
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
                InputMode::ConcordanceWorksPicker => state.borrow().concordance_works_picker.move_selection(1),
                InputMode::GlossPicker => state.borrow().gloss_picker.move_selection(1),
                InputMode::AuthorshipPicker => state.borrow().authorship_picker.move_selection(1),
                InputMode::EchoLinePicker => state.borrow().echo_line_picker.move_selection(1),
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
                InputMode::ConcordanceWorksPicker => state.borrow().concordance_works_picker.move_selection(-1),
                InputMode::GlossPicker => state.borrow().gloss_picker.move_selection(-1),
                InputMode::AuthorshipPicker => state.borrow().authorship_picker.move_selection(-1),
                InputMode::EchoLinePicker => state.borrow().echo_line_picker.move_selection(-1),
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
                InputMode::GlossPicker => {
                    // Alt+t cycles the type filter (teacher-generic ->
                    // inner-monologue -> reader-gloss). Alt combos don't type
                    // into the search entry, so no focus guard is needed.
                    if is_alt && key_name == "t" {
                        crate::input::actions::pickers::toggle_gloss_picker_type(state, tokio_handle);
                        return true;
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
            crate::config::save(&state.borrow().config);
            crate::input::actions::settings::close_settings_to_return_mode(state);
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

/// Voice picker (opened from the settings overlay's Voice row). Typed text
/// reaches the focused search entry via GTK; here we handle nav/confirm/cancel.
/// Confirm and cancel both return to the settings overlay (still visible
/// underneath), NOT the reader.
fn handle_voice_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    use crate::input::picker_keys::{resolve_picker_key, PickerAction};
    match resolve_picker_key(key_name, is_ctrl) {
        PickerAction::Hide => {
            crate::input::actions::settings::cancel_voice_picker(state);
            true
        }
        PickerAction::Confirm => {
            crate::input::actions::settings::confirm_voice_picker(state);
            true
        }
        PickerAction::MoveDown => {
            state.borrow().voice_picker.move_selection(1);
            true
        }
        PickerAction::MoveUp => {
            state.borrow().voice_picker.move_selection(-1);
            true
        }
        // Let typed characters fall through to the focused GTK entry so the
        // fuzzy filter works; consume everything else.
        PickerAction::Unhandled => false,
    }
}

fn handle_search_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "Escape" => {
            {
                let mut s = state.borrow_mut();
                crate::input::search::clear_search(&mut s);
                // Escape cancels the search: restore the reader position saved
                // when search opened so the live-search jump does not affect
                // pagination. resnap_page re-tiles the original page cleanly
                // (two-column split, bottom clip, etc.).
                if let Some((line, top)) = s.search_return_pos.take() {
                    s.current_line = line;
                    s.page_top_line = top;
                } else {
                    s.page_top_line = s.current_line;
                }
                crate::input::scroll::resnap_page(&mut s);
                crate::input::highlight::update_highlight(&mut s);
            }
            state.borrow().search_bar.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        "Return" => {
            // execute_search lands the first match on its CANONICAL spread
            // (land_on_match_idx). Accepting the search just commits that jump —
            // do NOT recompute page_top_line here (the old top-align / keep-on-
            // page logic clobbered the canonical landing, dropping the match to
            // the top of the page).
            crate::input::search::execute_search(&state);
            // Accepting a match commits the jump; drop the saved return pos.
            state.borrow_mut().search_return_pos = None;
            state.borrow().search_bar.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        // Tab must not toggle playback while typing a search query. Consume it
        // so it neither triggers playback nor moves focus out of the Entry.
        "Tab" | "ISO_Left_Tab" => true,
        _ => false, // let GTK route to the Entry (including Space)
    }
}

fn handle_gloss_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    use crate::ui::gloss_overlay::AskFocus;

    // ---- Stacked add/edit input card (a / e) ------------------------------
    // When open it behaves like the synopsis ask card: Tab toggles focus,
    // Ctrl+Enter submits, Esc is two-stage (close card, then close overlay),
    // and typed characters fall through to the editable input while it holds
    // focus. These must be handled before the gloss navigation keys below.
    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.gloss_overlay.ask_is_open(), s.gloss_overlay.ask_focus())
    };
    if ask_open {
        if key_name == "Tab" || key_name == "ISO_Left_Tab" {
            state.borrow().gloss_overlay.toggle_ask_focus();
            return true;
        }
        if is_ctrl && key_name == "Return" {
            crate::input::actions::gloss::submit_gloss_prompt(state);
            return true;
        }
        if key_name == "Escape" {
            crate::input::actions::gloss::close_gloss_prompt(state);
            return true;
        }
        // Let typed characters through to the input while it holds focus.
        if ask_focus == AskFocus::Ask {
            return false;
        }
    }

    // Shift+Space: batch-synthesize all prose blocks (cache-only).
    if key_name == "space" && is_shift {
        crate::input::actions::gloss::synth_all_prose_blocks(state);
        return true;
    }

    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport to the top.
            // Result gloss: jump the block cursor to the first block.
            let has_blocks = state.borrow().gloss_overlay.current_block().is_some();
            if has_blocks {
                state.borrow().gloss_overlay.cursor_first_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss_to_top();
            }
        }
        return true;
    }
    if is_alt {
        match key_name {
            "n" => {
                // Silence audio on gloss nav (pause MPV + stop TTS), like j/k.
                crate::input::actions::gloss::stop_all_gloss_audio(state);
                crate::input::actions::gloss::navigate_gloss(state, -1);
                return true;
            }
            "p" => {
                crate::input::actions::gloss::stop_all_gloss_audio(state);
                crate::input::actions::gloss::navigate_gloss(state, 1);
                return true;
            }
            "g" => {
                // Same as the reader card's Alt+g: open the glosses picker, but
                // keep the gloss overlay open behind it. The flag tells
                // `open_gloss_picker` not to hide the overlay and the picker's
                // Escape handler to return to the overlay (not the reader).
                state.borrow_mut().gloss_picker_from_overlay = true;
                crate::input::actions::pickers::open_gloss_picker(state, tokio_handle);
                return true;
            }
            _ => {}
        }
    }
    if is_ctrl {
        match key_name {
            "n" => {
                // Silence audio on passage nav (pause MPV + stop TTS), like j/k.
                crate::input::actions::gloss::stop_all_gloss_audio(state);
                crate::input::actions::gloss::navigate_gloss_passage(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::gloss::stop_all_gloss_audio(state);
                crate::input::actions::gloss::navigate_gloss_passage(state, -1);
                return true;
            }
            // Ctrl+V cycles the active TTS voice (moved off plain V, which now
            // enters visual block-selection mode like the synopsis overlay).
            "v" => {
                crate::input::actions::gloss::cycle_active_voice(state);
                return true;
            }
            // Ctrl+, opens the settings overlay (same as the reading card),
            // keeping the gloss overlay visible underneath and returning to it
            // when settings closes.
            "comma" => {
                crate::input::actions::settings::open_settings_from_overlay(
                    state,
                    crate::app::InputMode::GlossOverlay,
                );
                return true;
            }
            // Ctrl+/ opens the keybinds overlay, returning to the gloss overlay
            // on close (same overlay-return pattern as Ctrl+, settings).
            "slash" => {
                crate::input::actions::pickers::open_keybinds_from_mode(
                    state,
                    crate::app::InputMode::GlossOverlay,
                );
                return true;
            }
            // Ctrl+Up/Ctrl+Down adjust volume, mirroring the reader's
            // VolumeUp/VolumeDown (and the echoes overlay).
            "Up" => {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0));
                return true;
            }
            "Down" => {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0));
                return true;
            }
            _ => {}
        }
    }
    match key_name {
        "a" => {
            crate::input::actions::gloss::begin_current_block(state);
            true
        }
        "A" => {
            crate::input::actions::gloss::show_amend_dialog(state);
            true
        }
        "c" => {
            crate::input::actions::gloss::copy_gloss_id(state);
            true
        }
        "d" => {
            crate::input::actions::gloss::show_delete_confirmation(state);
            true
        }
        "e" => {
            crate::input::actions::gloss::show_edit_dialog(state);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport to the bottom.
            // Result gloss: jump the block cursor to the last block.
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_last_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss_to_bottom();
            }
            true
        }
        "bar" => {
            state.borrow().gloss_overlay.adjust_font_size(1);
            true
        }
        "exclam" => {
            state.borrow().gloss_overlay.adjust_font_size(-1);
            true
        }
        "j" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport down.
            // Result gloss: step the block cursor to the next block.
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_next_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss(1);
            }
            true
        }
        "k" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_prev_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss(-1);
            }
            true
        }
        // Tab mirrors Space: read the cursor block aloud (ISO_Left_Tab is
        // Shift+Tab). The ask-card Tab guard above takes precedence when the
        // input is focused, so this only fires in normal overlay navigation.
        "space" | "Tab" | "ISO_Left_Tab" => {
            crate::input::actions::gloss::read_current_block(state);
            true
        }
        // `]` closes the overlay exactly like Escape/n (jump the cursor to the
        // glossed passage's source on close), NOT like toggle_overlay's
        // return-to-origin. It opens the overlay from the reader via table
        // dispatch (ToggleGlossOverlay), so `]` is a full open/close toggle.
        "Escape" | "n" | "bracketright" => {
            let mut s = state.borrow_mut();
            s.tts.stop();
            s.gloss_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            s.gloss_opened_from_picker = false;
            // A gloss may have just been created/edited in the overlay, adding a
            // new glossed passage. Recompute the main-card reader-gloss tint so
            // the newly-glossed lines color without needing a work reload.
            crate::app::apply_reader_gloss_highlighting(&mut s);
            // Jump the cursor to the first dialogue line of the glossed passage's
            // source text. If that can't be resolved, fall back to the exact page
            // the user was on before the gloss opened (saved by every open path).
            let jumped = crate::input::actions::gloss::jump_to_gloss_source_start(&mut s);
            let saved = s.gloss_return_pos.take();
            if !jumped {
                if let Some((line, top)) = saved {
                    s.current_line = line;
                    s.page_top_line = top;
                    crate::input::scroll::resnap_page(&mut s);
                    crate::input::highlight::update_highlight(&mut s);
                }
            }
            true
        }
        // Shift+V enters visual block-selection mode (j/k extend, gg/G ends,
        // y yank, Esc/V exit), mirroring the synopsis overlay. The old voice
        // cycle moved to Ctrl+V (handled in the is_ctrl block above).
        "V" => {
            let entered = state.borrow().gloss_overlay.enter_visual();
            if entered {
                let mut s = state.borrow_mut();
                s.input_mode = crate::app::InputMode::GlossVisual;
                s.gloss_overlay.set_gloss_visual_hint();
            }
            true
        }
        "v" => {
            crate::input::actions::settings::open_voice_picker(
                state,
                crate::app::VoicePickerOrigin::GlossOverlay,
            );
            true
        }
        "r" => {
            // Source verse only: play/stop the synthesized MP3 in the active /
            // default voice (pauses MPV first). No picker.
            crate::input::actions::gloss::toggle_source_tts(state);
            true
        }
        "R" => {
            // Source verse only: open the voice picker for the synthesized
            // reading (the only picker key); confirm sets active voice + plays.
            crate::input::actions::gloss::pick_source_voice(state);
            true
        }
        // `;` mirrors the reading card: show the chapter/scene toast for the
        // source line the overlay was opened from (state.current_line).
        "semicolon" => {
            navigation::show_current_chapter(&mut state.borrow_mut());
            true
        }
        _ => true,
    }
}

fn handle_translation_overlay_key(state: &Rc<RefCell<AppState>>, key_name: &str) -> bool {
    // i (the same bind that opened the overlay) toggles it closed, matching
    // Escape. Without this, a second i would be swallowed by the catch-all.
    if key_name == "i" {
        let mut s = state.borrow_mut();
        s.translation_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        return true;
    }
    match key_name {
        "Escape" => {
            let mut s = state.borrow_mut();
            s.translation_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        // Dialogue navigation: drive the REAL cursor (same fns as the main
        // card), which also seeks MPV, then mirror the highlight + follow in
        // the overlay.
        "comma" => { overlay_nav(state, navigation::jump_to_prev_dialogue); true }
        "q" => { overlay_nav(state, navigation::jump_to_next_dialogue); true }
        "j" => { overlay_nav(state, navigation::cursor_next_dialogue); true }
        "k" => { overlay_nav(state, navigation::cursor_prev_line); true }
        // Playback (same as the main card): Tab toggles play/pause, a plays
        // from the current line. Neither moves the cursor, so no re-highlight.
        "Tab" | "ISO_Left_Tab" => {
            crate::input::search::toggle_playback(&mut state.borrow_mut());
            true
        }
        "a" | "space" => {
            let mut s = state.borrow_mut();
            if !crate::input::timestamps::play_current_line(&mut s) {
                show_no_timestamp_toast(&s);
            }
            true
        }
        // Toggle playback sync (same as the main card): identical state +
        // toast. When enabled, MPV events drive the overlay highlight too.
        "s" => {
            toggle_playback_sync(&mut state.borrow_mut());
            true
        }
        // Swallow everything else so stray keys don't leak to the reader.
        _ => true,
    }
}

/// Toggle MPV playback sync on/off, mirroring concordance state and showing the
/// bottom-center "Sync: on/off" toast. Shared by the main-card `TogglePlaybackSync`
/// dispatch and the translation overlay's `s` bind so both behave identically.
fn toggle_playback_sync(s: &mut AppState) {
    s.sync_enabled = !s.sync_enabled;
    if s.sync_enabled {
        s.suppress_sync_until = None;
    }
    if s.sync_enabled_before_concordance.is_some() {
        s.sync_enabled_before_concordance = Some(s.sync_enabled);
    }
    let label = if s.sync_enabled { "Sync: on" } else { "Sync: off" };
    crate::logging::log(&format!("SYNC: {}", if s.sync_enabled { "enabled" } else { "disabled" }));
    // Bottom-center (same place as the act/scene pill); reset margins in
    // case a prior corner toast moved the shared widget.
    s.speed_toast.set_halign(gtk4::Align::Center);
    s.speed_toast.set_margin_start(0);
    s.speed_toast.set_margin_end(0);
    s.speed_toast.set_text(label);
    s.speed_toast.set_visible(true);
    let toast = s.speed_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
}

/// Show a transient bottom-center toast (reuses `chapter_toast`, 3s auto-hide).
/// Used when a play attempt is a no-op because the line has no timestamp.
fn show_no_timestamp_toast(s: &AppState) {
    s.chapter_toast.set_text("No timestamp on this line");
    s.chapter_toast.set_visible(true);
    let toast = s.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
}

/// Run a main-card navigation function (moves `current_line` + seeks MPV via
/// `after_page_change`), then re-highlight and follow in the translation overlay.
fn overlay_nav(state: &Rc<RefCell<AppState>>, nav_fn: fn(&mut AppState)) {
    let scene_before = crate::app::current_scene_divs(&state.borrow());
    nav_fn(&mut state.borrow_mut());
    crate::app::sync_translation_overlay(state, scene_before);
}

fn handle_synopsis_overlay_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    is_alt: bool,
    is_shift: bool,
) -> bool {
    use crate::ui::gloss_overlay::AskFocus;

    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.gloss_overlay.ask_is_open(), s.gloss_overlay.ask_focus())
    };

    // ---- Keys that apply whether or not the ask card is open --------------

    // Tab toggles focus between the synopsis and ask cards (only meaningful
    // while the ask card is open). Consume so it never reaches playback toggle
    // or the editable input.
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        if ask_open {
            state.borrow().gloss_overlay.toggle_ask_focus();
        }
        return true;
    }

    // Ctrl+Enter submits the question when the ask card is open.
    if ask_open && is_ctrl && key_name == "Return" {
        crate::input::actions::synopsis::submit_amend_prompt(state);
        return true;
    }

    // Escape is two-stage: close the ask card first (back to full synopsis),
    // then close the whole overlay on a second press.
    if key_name == "Escape" {
        if ask_open {
            crate::input::actions::synopsis::close_amend_prompt(state);
        } else {
            let mut s = state.borrow_mut();
            s.gloss_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
        }
        return true;
    }

    // While the ask card holds focus, let typed characters through to the
    // editable input. Only the explicit shortcuts above (Tab/Ctrl+Enter/Esc)
    // are intercepted; everything else falls through to GTK so the field works.
    if ask_open && ask_focus == AskFocus::Ask {
        return false;
    }

    // gg: jump to the first block.
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.cursor_first_block();
        }
        return true;
    }

    // Shift+Space: batch-synthesize all synopsis paragraphs (cache-only).
    if key_name == "space" && is_shift {
        crate::input::actions::gloss::synth_all_synopsis_blocks(state);
        return true;
    }

    // ---- Synopsis-focused (or ask card closed) navigation -----------------

    match key_name {
        "h" => {
            let mut s = state.borrow_mut();
            s.gloss_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        // `a`: always (re)start the cursor paragraph's TTS from the start,
        // mirroring the gloss-overlay `a` (`begin_current_block`). Plain
        // Space/Tab below is the play/pause toggle.
        "a" => {
            crate::input::actions::gloss::begin_current_synopsis_block(state);
            true
        }
        "A" => {
            crate::input::actions::synopsis::show_amend_prompt(state);
            true
        }
        "E" => {
            crate::input::actions::synopsis::show_edit_prompt(state);
            true
        }
        "V" => {
            let entered = state.borrow().gloss_overlay.enter_visual();
            if entered {
                let mut s = state.borrow_mut();
                s.input_mode = crate::app::InputMode::SynopsisVisual;
                s.gloss_overlay.set_synopsis_visual_hint();
            }
            true
        }
        "U" => {
            crate::input::actions::synopsis::undo_amend(state);
            true
        }
        "bar" => {
            state.borrow().gloss_overlay.adjust_font_size(1);
            true
        }
        "exclam" => {
            state.borrow().gloss_overlay.adjust_font_size(-1);
            true
        }
        "n" => {
            crate::app::cycle_synopsis(state, 1);
            true
        }
        "p" => {
            crate::app::cycle_synopsis(state, -1);
            true
        }
        "g" if is_alt => {
            crate::input::actions::synopsis::open_work_glosses(state);
            true
        }
        // Ctrl+, opens the settings overlay (same as the reading card), keeping
        // the synopsis overlay visible underneath and returning to it on close.
        "comma" if is_ctrl => {
            crate::input::actions::settings::open_settings_from_overlay(
                state,
                crate::app::InputMode::SynopsisOverlay,
            );
            true
        }
        // Ctrl+/ opens the keybinds overlay, returning to the synopsis overlay
        // on close (same overlay-return pattern as Ctrl+, settings).
        "slash" if is_ctrl => {
            crate::input::actions::pickers::open_keybinds_from_mode(
                state,
                crate::app::InputMode::SynopsisOverlay,
            );
            true
        }
        // Ctrl+Up/Ctrl+Down adjust volume, mirroring the reader's
        // VolumeUp/VolumeDown (and the echoes overlay).
        "Up" if is_ctrl => {
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0));
            true
        }
        "Down" if is_ctrl => {
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0));
            true
        }
        "j" => {
            state.borrow().gloss_overlay.cursor_next_block();
            true
        }
        "k" => {
            state.borrow().gloss_overlay.cursor_prev_block();
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            state.borrow().gloss_overlay.cursor_last_block();
            true
        }
        // Plain Space: play/stop the cursor paragraph's TTS (Shift+Space, the
        // batch-synth, is handled by the guard above before this match).
        // Tab mirrors Space (ISO_Left_Tab is Shift+Tab).
        "space" | "Tab" | "ISO_Left_Tab" => {
            crate::input::actions::gloss::read_current_synopsis_block(state);
            true
        }
        // `;` mirrors the reading card: show the chapter/scene toast for the
        // source line the overlay was opened from (state.current_line).
        "semicolon" => {
            navigation::show_current_chapter(&mut state.borrow_mut());
            true
        }
        _ => true,
    }
}

/// Key handling for synopsis visual mode (Shift+V from the synopsis overlay).
/// Mirrors the reader's `handle_visual_key`: j/k extend the block selection,
/// gg/G jump the cursor end, y yanks the selected paragraphs and exits, Esc/V
/// exits without copying. All other keys are consumed.
fn handle_synopsis_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    // gg: extend to the first block.
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.visual_to_end(false);
        }
        return true;
    }

    match key_name {
        "j" => {
            state.borrow().gloss_overlay.visual_step(1);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.visual_step(-1);
            true
        }
        "G" => {
            state.borrow().gloss_overlay.visual_to_end(true);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "y" => {
            let (text, n) = {
                let s = state.borrow();
                (s.gloss_overlay.visual_selection_text(), s.gloss_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("SYNOPSIS: copied {} blocks", n));
            }
            {
                let mut s = state.borrow_mut();
                s.gloss_overlay.exit_visual();
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
                s.gloss_overlay.set_synopsis_hint();
                s.chapter_toast.set_text("Copied");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    toast.set_visible(false);
                });
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            s.gloss_overlay.exit_visual();
            s.input_mode = crate::app::InputMode::SynopsisOverlay;
            s.gloss_overlay.set_synopsis_hint();
            true
        }
        _ => true,
    }
}

/// Gloss-overlay visual block selection (entered with Shift+V). Mirrors
/// `handle_synopsis_visual_key` but returns to the GLOSS overlay on exit and
/// yanks the full block text (source verse + gloss, as displayed) read from the
/// gloss buffer — not `current_synopsis`, which is empty in a gloss.
fn handle_gloss_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    // gg: extend to the first block.
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.visual_to_end(false);
        }
        return true;
    }

    match key_name {
        "j" => {
            state.borrow().gloss_overlay.visual_step(1);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.visual_step(-1);
            true
        }
        "G" => {
            state.borrow().gloss_overlay.visual_to_end(true);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "y" => {
            let (text, n) = {
                let s = state.borrow();
                (s.gloss_overlay.visual_selection_buffer_text(), s.gloss_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("GLOSS: copied {} blocks", n));
            }
            {
                let mut s = state.borrow_mut();
                s.gloss_overlay.exit_visual_to_start();
                s.input_mode = crate::app::InputMode::GlossOverlay;
                s.gloss_overlay.set_gloss_hint();
                s.chapter_toast.set_text("Copied");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    toast.set_visible(false);
                });
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            s.gloss_overlay.exit_visual();
            s.input_mode = crate::app::InputMode::GlossOverlay;
            s.gloss_overlay.set_gloss_hint();
            true
        }
        _ => true,
    }
}

/// Page-image calibration keys: j/k move the cursor one line, Enter marks the
/// cursor line as the current page's start (+advance), n/p step pages without
/// marking, gg/G jump to the first/last page, Esc finishes (recompute ranges +
/// save). Every key refreshes the caption so the displayed "start line" tracks
/// the cursor.
fn handle_page_calibration_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    // gg: jump to the first page (mirrors the reader/overlay gg chord).
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            crate::app::calibration_jump_page(state, false);
        }
        return true;
    }
    match key_name {
        "Return" => {
            crate::app::calibration_mark(state);
        }
        "Escape" => {
            crate::app::exit_page_calibration(state, true);
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
        }
        "G" => crate::app::calibration_jump_page(state, true),
        "n" => crate::app::calibration_step_page(state, 1),
        "p" => crate::app::calibration_step_page(state, -1),
        "j" | "k" => {
            {
                let mut s = state.borrow_mut();
                let n_lines = s.buffer.line_count().max(1) as usize;
                // Step to the next/previous buffer line that HAS a work-line
                // mapping, skipping chrome/blank/unmapped rows. This keeps the
                // cursor on a real line so Enter can always record a
                // line_mapping.id and the caption shows actual text (not
                // "(cursor on an unmapped line)").
                let cur = s.current_line;
                let forward = key_name == "j";
                let mut probe = cur;
                let mut landed = None;
                loop {
                    if forward {
                        if probe + 1 >= n_lines {
                            break;
                        }
                        probe += 1;
                    } else {
                        if probe == 0 {
                            break;
                        }
                        probe -= 1;
                    }
                    if s.work_line_for_buffer(probe).is_some() {
                        landed = Some(probe);
                        break;
                    }
                }
                if let Some(next) = landed {
                    s.current_line = next;
                    crate::input::highlight::update_highlight_and_center(&mut s);
                }
            }
            // Refresh the caption to show the new cursor line as the start line.
            crate::app::calibration_show_page(state);
        }
        _ => {}
    }
    true
}

fn handle_delete_confirm_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "y" => {
            crate::input::actions::gloss::close_delete_confirmation(state);
            crate::input::actions::gloss::delete_current_gloss(state);
            true
        }
        "Escape" | "n" => {
            crate::input::actions::gloss::close_delete_confirmation(state);
            true
        }
        _ => true,
    }
}

fn handle_echo_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    match key_name {
        "j" | "Down" => {
            state.borrow().echo_picker.move_selection(1);
            true
        }
        "k" | "Up" => {
            state.borrow().echo_picker.move_selection(-1);
            true
        }
        "Return" => {
            let selected = {
                let s = state.borrow();
                s.echo_picker.selected_index()
                    .and_then(|idx| s.echo_picker.items.get(idx).cloned())
            };
            state.borrow().echo_picker.hide();
            crate::input::visual::run_pending_inner_monologue(state, tokio_handle, selected);
            true
        }
        "n" => {
            // Skip the suggested echo; let Claude find its own.
            state.borrow().echo_picker.hide();
            crate::input::visual::run_pending_inner_monologue(state, tokio_handle, None);
            true
        }
        "Escape" => {
            // Cancel glossing entirely.
            crate::input::visual::cancel_pending_inner_monologue(state);
            true
        }
        _ => true,
    }
}

fn handle_echo_turns_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    match key_name {
        "j" | "Down" => {
            state.borrow().echo_turns_picker.move_selection(1);
            true
        }
        "k" | "Up" => {
            state.borrow().echo_turns_picker.move_selection(-1);
            true
        }
        "Return" => {
            crate::input::actions::echoes::confirm_echo_turns_pick(state, tokio_handle);
            true
        }
        "Escape" => {
            let s = state.borrow();
            s.echo_turns_picker.hide();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            true
        }
        _ => true,
    }
}

fn handle_echoes_overlay_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            crate::input::actions::echoes::select_first_echo(state);
        }
        return true;
    }
    // Ctrl+Up/Ctrl+Down adjust volume, mirroring the reader's VolumeUp/Down.
    if is_ctrl {
        match key_name {
            "Up" => {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0));
                return true;
            }
            "Down" => {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0));
                return true;
            }
            "slash" => {
                let mut s = state.borrow_mut();
                s.echo_keybinds_overlay.show();
                s.input_mode = crate::app::InputMode::EchoKeybindsOverlay;
                return true;
            }
            _ => {}
        }
    }
    match key_name {
        "n" => {
            crate::input::actions::echoes::move_echo_selection(state, 1, tokio_handle);
            true
        }
        "p" => {
            crate::input::actions::echoes::move_echo_selection(state, -1, tokio_handle);
            true
        }
        "Return" => {
            crate::input::actions::echoes::jump_to_selected_echo(state, tokio_handle);
            true
        }
        "c" => {
            crate::input::actions::echoes::copy_selected_echo(state);
            true
        }
        "Tab" => {
            crate::input::actions::echoes::play_source_turn(state);
            true
        }
        "a" | "space" => {
            crate::input::actions::echoes::play_selected_echo(state, tokio_handle);
            true
        }
        "s" => {
            crate::input::actions::echoes::toggle_curated(state);
            true
        }
        "d" => {
            crate::input::actions::echoes::delete_selected_echo(state);
            true
        }
        "D" => {
            crate::input::actions::echoes::delete_all_echoes(state);
            true
        }
        "R" => {
            crate::input::actions::echoes::refresh_echoes(state, tokio_handle);
            true
        }
        "Up" => {
            crate::input::actions::echoes::reorder_selected_echo(state, -1);
            true
        }
        "Down" => {
            crate::input::actions::echoes::reorder_selected_echo(state, 1);
            true
        }
        "A" => {
            crate::input::actions::echoes::open_add_echo_picker(state);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            crate::input::actions::echoes::select_last_echo(state);
            true
        }
        "j" => {
            state.borrow().gloss_overlay.scroll_gloss(1);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.scroll_gloss(-1);
            true
        }
        // `;` mirrors the reading card: show the chapter/scene toast for the
        // source line the overlay was opened from (state.current_line).
        "semicolon" => {
            navigation::show_current_chapter(&mut state.borrow_mut());
            true
        }
        "Escape" => {
            let mut s = state.borrow_mut();
            s.gloss_overlay.hide();
            s.echo_overlay_links.clear();
            s.echo_overlay_turn_id = None;
            s.echo_overlay_turn_key = None;
            // Clear any turn AB-loop so normal reading isn't stuck looping.
            if s.ab_repeat.loop_active {
                let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
                s.ab_repeat.loop_active = false;
                s.ab_repeat.a_time = None;
                s.ab_repeat.b_time = None;
            }
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        _ => true,
    }
}

fn handle_gamepad_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    // The gamepad overlay is the 6th screen in the keybinds cycle.
    match key_name {
        "Escape" => {
            state.borrow().gamepad_overlay.hide();
            // Honor the keybinds-overlay return mode (the gamepad screen is part
            // of the Ctrl+/ cycle), so closing returns to the overlay it opened
            // from rather than always the reader.
            let back = state.borrow().keybinds_return_mode;
            crate::input::actions::pickers::restore_mode_after_keybinds(state, back);
            true
        }
        "n" | "Up" => {
            // Past the gamepad → wrap to the first keyboard row.
            let s = state.borrow();
            s.gamepad_overlay.hide();
            s.keybinds_overlay.show();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::KeybindsOverlay;
            true
        }
        "p" | "Down" => {
            // Back from the gamepad → the last keyboard row.
            let s = state.borrow();
            s.gamepad_overlay.hide();
            s.keybinds_overlay.show_last_row();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::KeybindsOverlay;
            true
        }
        _ => true,
    }
}

fn handle_echo_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    // Esc or Ctrl+/ closes the legend, returning to the echoes overlay.
    if key_name == "Escape" || (is_ctrl && key_name == "slash") {
        let mut s = state.borrow_mut();
        s.echo_keybinds_overlay.hide();
        s.input_mode = crate::app::InputMode::EchoesOverlay;
    }
    true // consume all keys while the legend is up (modal)
}

fn handle_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    // Advance a row; past the last keyboard row hands off to the gamepad screen.
    fn next_row_or_gamepad(state: &Rc<RefCell<AppState>>) {
        let advanced = state.borrow().keybinds_overlay.next_row();
        if !advanced {
            let s = state.borrow();
            s.keybinds_overlay.hide();
            s.gamepad_overlay.show();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GamepadOverlay;
        }
    }
    // Previous row; before the first keyboard row hands off to the gamepad screen.
    fn prev_row_or_gamepad(state: &Rc<RefCell<AppState>>) {
        let moved = state.borrow().keybinds_overlay.prev_row();
        if !moved {
            let s = state.borrow();
            s.keybinds_overlay.hide();
            s.gamepad_overlay.show();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GamepadOverlay;
        }
    }

    match key_name {
        "Escape" => {
            state.borrow().keybinds_overlay.hide();
            let back = state.borrow().keybinds_return_mode;
            crate::input::actions::pickers::restore_mode_after_keybinds(state, back);
            return true;
        }
        "Tab" => {
            state.borrow().keybinds_overlay.toggle_mode();
            return true;
        }
        // Arrows navigate in BOTH modes.
        "Up" => {
            next_row_or_gamepad(state);
            return true;
        }
        "Down" => {
            prev_row_or_gamepad(state);
            return true;
        }
        "Right" => {
            state.borrow().keybinds_overlay.move_selection(1);
            return true;
        }
        "Left" => {
            state.borrow().keybinds_overlay.move_selection(-1);
            return true;
        }
        _ => {}
    }

    if state.borrow().keybinds_overlay.is_jump_mode() {
        // Jump mode: any other key jumps the highlight to its cap (no-op if no
        // matching cap). Always consume so nothing leaks to the reader.
        state.borrow().keybinds_overlay.jump_to_key(key_name);
        return true;
    }

    // Nav mode: the classic n/p rows, j/k highlight.
    match key_name {
        "n" => next_row_or_gamepad(state),
        "p" => prev_row_or_gamepad(state),
        "j" => {
            state.borrow().keybinds_overlay.move_selection(1);
        }
        "k" => {
            state.borrow().keybinds_overlay.move_selection(-1);
        }
        _ => {}
    }
    true // consume all other keys while keybinds visible
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
    tokio_handle: &tokio::runtime::Handle,
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
            KeyState::start_chord(key_state, ChordState::PendingG);
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
        "i" => {
            crate::input::actions::echoes::show_echoes_for_selection(state, crate::db::echo_channel::EchoChannel::Shakespeare, tokio_handle);
            true
        }
        _ => {
            // Consume all other keys in visual mode
            true
        }
    }
}

/// Execute an Action by calling its corresponding verb. The key is always
/// consumed when a mapped action is dispatched.
fn dispatch_action(
    state: &Rc<RefCell<AppState>>,
    action: crate::input::actions::Action,
    key_state: &Rc<RefCell<KeyState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    crate::logging::log(&format!("ACTION: {}", action.name()));
    use crate::input::actions::Action::*;
    match action {
        // Page navigation
        PageForward => navigation::page_forward(&mut state.borrow_mut()),
        PageBackward => navigation::page_backward(&mut state.borrow_mut()),
        PageBackwardBottom => navigation::page_backward_bottom(&mut state.borrow_mut()),
        JumpToStart => navigation::jump_to_start(&mut state.borrow_mut()),
        JumpToEnd => navigation::jump_to_end(&mut state.borrow_mut()),

        // Cursor / dialogue
        CursorNextDialogue => navigation::cursor_next_dialogue(&mut state.borrow_mut()),
        CursorPrevLine => navigation::cursor_prev_line(&mut state.borrow_mut()),
        CursorToPageBottom => navigation::cursor_to_page_bottom(&mut state.borrow_mut()),
        JumpToNextDialogue => navigation::jump_to_next_dialogue(&mut state.borrow_mut()),
        JumpToPrevDialogue => navigation::jump_to_prev_dialogue(&mut state.borrow_mut()),
        JumpToNextSpeaker => navigation::jump_to_next_speaker(&mut state.borrow_mut()),
        JumpToPrevSpeaker => navigation::jump_to_prev_speaker(&mut state.borrow_mut()),
        JumpToNextChapter => navigation::jump_to_next_chapter(&mut state.borrow_mut()),
        JumpToPrevChapter => navigation::jump_to_prev_chapter(&mut state.borrow_mut()),
        JumpToNextScene => navigation::jump_to_next_section(&mut state.borrow_mut()),
        JumpToPrevScene => navigation::jump_to_prev_section(&mut state.borrow_mut()),

        // Bookmarks
        ToggleBookmark => crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle),
        NextBookmark => navigation::next_bookmark(&mut state.borrow_mut()),
        PrevBookmark => navigation::prev_bookmark(&mut state.borrow_mut()),
        JumpToRecentBookmark => crate::input::actions::bookmarks::jump_to_recent_bookmark(state, tokio_handle),
        OpenBookmarkPicker => crate::input::actions::pickers::open_bookmark_picker(state, tokio_handle),

        // Pickers / overlays
        OpenLibraryPicker => crate::input::actions::pickers::open_library_picker_from_reader(state),
        OpenRecentPicker => crate::input::actions::pickers::open_recent_picker(state),
        OpenMediaPicker => crate::input::actions::pickers::open_media_picker(state, tokio_handle),
        OpenConcordancePicker => crate::input::actions::concordance::open_picker(state, tokio_handle),
        OpenConcordanceWordPicker => crate::input::actions::pickers::open_concordance_word_picker(state),
        OpenConcordanceListPicker => crate::input::actions::pickers::open_concordance_list_picker(state),
        OpenConcordanceWorksPicker => crate::input::actions::pickers::open_concordance_works_picker(state),
        OpenSettingsOverlay => crate::input::actions::settings::open_settings(state),
        OpenKeybindsOverlay => {
            crate::input::actions::pickers::open_keybinds_overlay(state);
        }
        OpenSearch | OpenSearchBackward => {
            let mut s = state.borrow_mut();
            crate::input::search::clear_search(&mut s);
            // `/` searches forward (first match at/after cursor); `?` searches
            // backward (last match at/before cursor). execute_search reads this.
            s.search_backward = matches!(action, OpenSearchBackward);
            // Remember where the reader was so Escape can restore it (live
            // search moves current_line/page_top_line as the user types).
            s.search_return_pos = Some((s.current_line, s.page_top_line));
            s.search_bar.show();
            s.input_mode = crate::app::InputMode::Search;
        }

        // MPV / media
        TogglePlaybackSync => toggle_playback_sync(&mut state.borrow_mut()),
        TogglePlayback => crate::input::search::toggle_playback(&mut state.borrow_mut()),
        SeekShortBackward => do_mpv_seek(state, -3.5),
        SeekShortForward => do_mpv_seek(state, 3.5),
        SeekLongBackward => do_mpv_seek(state, -60.0),
        SeekLongForward => do_mpv_seek(state, 60.0),
        SeekBackward30 => do_mpv_seek(state, -30.0),
        VolumeUp => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0)); }
        VolumeDown => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0)); }
        TogglePlaybackSpeed => {
            let mut s = state.borrow_mut();
            let new_speed = if s.playback_speed == 1.0 { 1.3 } else { 1.0 };
            s.playback_speed = new_speed;
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetSpeed(new_speed));
            crate::logging::log(&format!("SPEED: toggled to {}x", new_speed));
            let label = format!("Speed: {:.1}x", new_speed);
            // Speed toast sits bottom-center (same place as the act/scene pill);
            // reset margins in case a prior "Sync:"/"Copied" moved the shared
            // toast to a corner.
            s.speed_toast.set_halign(gtk4::Align::Center);
            s.speed_toast.set_margin_start(0);
            s.speed_toast.set_margin_end(0);
            s.speed_toast.set_text(&label);
            s.speed_toast.set_visible(true);
            let toast = s.speed_toast.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                toast.set_visible(false);
            });
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
        }
        VocabPopupNext => {
            // Same inline logic as the original "backslash" / "numbersign"
            // arms; the auto-hide timer handling is preserved.
            handle_vocab_popup_key(state, true);
        }
        VocabPopupPrev => {
            handle_vocab_popup_key(state, false);
        }
        JumpToNextVocab => crate::input::actions::concordance::jump_to_next_vocab(state, tokio_handle),
        JumpToPrevVocab => crate::input::actions::concordance::jump_to_prev_vocab(state, tokio_handle),
        ConcordanceNext => crate::input::actions::concordance::concordance_next(state, tokio_handle),
        ConcordancePrev => crate::input::actions::concordance::concordance_prev(state, tokio_handle),
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
        }
        ToggleGlossOverlay => crate::input::actions::gloss::toggle_overlay(state),
        OpenGlossPicker => crate::input::actions::pickers::open_gloss_picker(state, tokio_handle),
        OpenLastGloss => crate::input::actions::gloss::open_last_gloss(state),
        ShowEchoesBcp => crate::input::actions::echoes::show_echoes_for_cursor_line(state, crate::db::echo_channel::EchoChannel::Bcp, tokio_handle),
        ReopenEchoesBcp => crate::input::actions::echoes::reopen_echoes(state, crate::db::echo_channel::EchoChannel::Bcp, tokio_handle),
        ShowEchoTurnsBcp => crate::input::actions::echoes::open_echo_turns_picker(state, crate::db::echo_channel::EchoChannel::Bcp),
        ShowEchoesShx => crate::input::actions::echoes::show_echoes_for_cursor_line(state, crate::db::echo_channel::EchoChannel::Shakespeare, tokio_handle),
        ReopenEchoesShx => crate::input::actions::echoes::reopen_echoes(state, crate::db::echo_channel::EchoChannel::Shakespeare, tokio_handle),
        ShowEchoTurnsShx => crate::input::actions::echoes::open_echo_turns_picker(state, crate::db::echo_channel::EchoChannel::Shakespeare),

        // Visual / selection
        EnterVisualMode => crate::input::visual::enter_visual_mode(&mut state.borrow_mut()),
        WordCycleCopy => crate::input::actions::word_copy::word_cycle_copy(&mut state.borrow_mut()),
        WordCollectCopy => crate::input::actions::word_copy::word_collect_copy(&mut state.borrow_mut()),

        // Translations
        ToggleTranslations => {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            drop(s);
            crate::app::toggle_translations(&mut state.borrow_mut());
        }

        // Settings (in reader)
        AdjustFontSizeUp => { crate::app::adjust_font_size(&mut state.borrow_mut(), 1); crate::app::show_font_info(&state.borrow()); }
        AdjustFontSizeDown => { crate::app::adjust_font_size(&mut state.borrow_mut(), -1); crate::app::show_font_info(&state.borrow()); }
        ResetFontSize => crate::app::reset_font_size(&mut state.borrow_mut()),
        CycleFontForward => crate::app::cycle_font(&mut state.borrow_mut(), true),
        CycleFontBackward => crate::app::cycle_font(&mut state.borrow_mut(), false),
        ToggleSignColumn => crate::app::toggle_sign_column(&mut state.borrow_mut()),
        ToggleColumnLayout => navigation::toggle_column_layout(&mut state.borrow_mut()),
        TogglePreviousWork => {
            crate::input::actions::pickers::toggle_previous_work(state, tokio_handle);
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
        }
        CycleScansion => {
            let mut s = state.borrow_mut();
            // Populate the cache on first use (or for a freshly loaded work).
            if s.scansion_data.is_empty() {
                if let Some(work) = s.current_work.as_ref() {
                    let abbrev = work.abbrev.clone();
                    if let Ok(conn) = crate::db::queries::open_db() {
                        match crate::db::queries::load_scansion_for_work(&conn, &abbrev) {
                            Ok(map) => s.scansion_data = map,
                            Err(e) => crate::logging::log(&format!("SCANSION: load failed: {}", e)),
                        }
                    }
                }
            }
            if s.scansion_data.is_empty() {
                s.scansion_level = crate::scansion::ScanLevel::Off;
                // Reuse the chapter-toast widget for a transient reader message
                // (same pattern as show_chapter_toast in navigation.rs:1670).
                s.chapter_toast.set_text("No scansion for this work");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                crate::logging::log("SCANSION: no scansion for this work");
                return;
            }
            s.scansion_level = s.scansion_level.next();
            s.config.scansion_level = s.scansion_level.as_str().to_string();
            crate::config::save(&s.config);
            crate::logging::log(&format!("SCANSION: level -> {:?}", s.scansion_level));
            crate::app::rebuild_buffer_text(&mut s);
            // Scansion marks change every verse line's content and height, so the
            // cached page-tops and the left/right column split are now stale.
            // Do NOT resnap synchronously: GTK has not re-laid-out the
            // just-replaced buffer yet, so line_yrange/adjustment.upper are stale
            // and a synchronous snap_scroll_to_line lands on a garbage offset and
            // blanks the spread (the top-of-document `i` blank). Instead drop the
            // stale page-tops and defer the resnap to the RESIZE_TICK via
            // needs_layout_refresh — the same mechanism hide_translations uses for
            // its two-column buffer change, which waits for layout to settle and
            // produces the one correct resnap.
            navigation::invalidate_page_tops(&mut s);
            navigation::update_highlight_only(&mut s);
            // Hold the CURRENT spread verbatim across the deferred refresh.
            // Without this, the RESIZE_TICK runs snap_near_end_to_canonical,
            // which re-derives the page from the cursor and (e.g. at the top of
            // a play, where page_top=0 "ACT 1" but the canonical first spread
            // begins at the first dialogue boundary, line 1) shifts page_top off
            // the spread the user is looking at. A scansion toggle must not move
            // the page — only re-render it with/without marks. (Same one-shot
            // flag hide_translations sets to keep its restored spread.)
            s.trust_restored_page.set(true);
            s.needs_layout_refresh.set(true);
        }
        ToggleTitleBar => {
            let mut s = state.borrow_mut();
            let visible = s.title_bar.is_visible();
            s.title_bar.set_visible(!visible);
            s.config.title_bar_visible = !visible;
            if !visible {
                crate::app::update_title_bar_scene(&s);
            }
            crate::config::save(&s.config);
        }
        ShowFontInfo => crate::app::show_font_info(&state.borrow()),
        ShowCurrentChapter => navigation::show_current_chapter(&mut state.borrow_mut()),

        // Timestamps
        SetStartTime => { crate::input::timestamps::set_start_time(&mut state.borrow_mut()); }
        SetEndTime => { crate::input::timestamps::set_end_time(&mut state.borrow_mut()); }
        SetChapter => { crate::input::timestamps::set_chapter(&mut state.borrow_mut()); }
        DeleteTimestamp => { crate::input::timestamps::delete_timestamp(&mut state.borrow_mut()); }
        NudgeStartBackward => { crate::input::timestamps::nudge_start_backward(&mut state.borrow_mut()); }
        NudgeStartForward => { crate::input::timestamps::nudge_start_forward(&mut state.borrow_mut()); }
        UndoTimestamp => { crate::input::timestamps::undo_timestamp(&mut state.borrow_mut()); }
        PlayCurrentLine => { crate::input::timestamps::play_current_line(&mut state.borrow_mut()); }

        // App
        EscapeReaderMode => crate::input::actions::escape::escape_reader_mode(state),
        SaveAndQuit => {
            crate::app::save_position(&mut state.borrow_mut());
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
            state.borrow().window.close();
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
        }
        ToggleNavTest => {
            crate::input::nav_test::toggle(state);
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
                (None, None) => return,
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
            // Confirm with a bottom-center toast (same place as the act/scene
            // pill); reset margins in case a prior corner toast moved the widget.
            let s = state.borrow();
            s.speed_toast.set_halign(gtk4::Align::Center);
            s.speed_toast.set_margin_start(0);
            s.speed_toast.set_margin_end(0);
            s.speed_toast.set_text(&format!("Copied {}", clip));
            s.speed_toast.set_visible(true);
            let toast = s.speed_toast.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                toast.set_visible(false);
            });
        }

        // Multi-key chord entry
        PendingG => {
            if state.borrow().vocab_popup.is_visible() {
                crate::app::vocab_popup_toggle_view(&mut state.borrow_mut());
            } else {
                KeyState::start_chord(key_state, ChordState::PendingG);
            }
        }
        PendingZ => {
            KeyState::start_chord(key_state, ChordState::PendingZ);
        }

        // Search / concordance-in-work (n/p)
        SearchNextMatch => {
            if state.borrow().concordance_state.is_some() {
                crate::input::actions::concordance::concordance_next_in_work(state, tokio_handle);
            } else {
                crate::input::search::reactivate_and_step(state, true);
            }
        }
        SearchPrevMatch => {
            if state.borrow().concordance_state.is_some() {
                crate::input::actions::concordance::concordance_prev_in_work(state, tokio_handle);
            } else {
                crate::input::search::reactivate_and_step(state, false);
            }
        }
        ToggleSynopsis => crate::app::toggle_synopsis(&mut state.borrow_mut()),
        ShowSynopsisOverlay => crate::app::show_synopsis_overlay(state),
        ShowTranslationOverlay => crate::app::show_translation_overlay(state),
        ToggleImageView => crate::app::toggle_image_view(state),
        EnterPageCalibration => crate::app::enter_page_calibration(state),

        // Authorship display
        ToggleAuthorship => {
            let mut s = state.borrow_mut();
            if s.authorship_sets.is_empty() {
                s.chapter_toast.set_text("No authorship data for this work");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                return;
            }
            s.authorship_enabled = !s.authorship_enabled;
            crate::app::apply_authorship_formatting(&mut s);
            let label = if s.authorship_enabled { "Authorship: on" } else { "Authorship: off" };
            s.chapter_toast.set_text(label);
            s.chapter_toast.set_visible(true);
            let toast = s.chapter_toast.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                toast.set_visible(false);
            });
        }
        PickAttributionSet => {
            let s = state.borrow();
            if s.authorship_sets.is_empty() {
                s.chapter_toast.set_text("No authorship data for this work");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                return;
            }
            if s.authorship_sets.len() == 1 {
                s.chapter_toast.set_text("Only one attribution set available");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                return;
            }
            drop(s);
            crate::input::actions::authorship::open_attribution_picker(state);
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

