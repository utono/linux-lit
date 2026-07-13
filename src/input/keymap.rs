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
    /// First tap of the overloaded `r`: a second `r` within the chord window
    /// toggles the vocab popup's visibility (show when hidden, hide when
    /// visible). See Action::VocabPopupTap.
    PendingR,
    /// First tap of the overloaded `.`: a second `.` reverts the bookmark
    /// toggle and opens the picker. See Action::BookmarkTap.
    PendingPeriod,
    /// First tap of the overloaded `s`: a second `s` toggles playback sync
    /// (the single tap only toasts). See Action::PlaybackSyncTap.
    PendingS,
    /// First tap of the overloaded BackSpace: a second one deletes the
    /// line's timestamp (the single tap only toasts). See
    /// Action::DeleteTimestampTap.
    PendingBackspace,
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
    key_char: Option<char>,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    crate::logging::log(&format!("KEY: name={} ctrl={} shift={} alt={}", key_name, is_ctrl, is_shift, is_alt));

    // Shift+Ctrl+L: save position and quit from ANY mode — every overlay,
    // picker, and the vim editors included, so it sits ABOVE the JournalEdit/
    // GlossEdit routing (which otherwise owns all keys). GTK delivers the
    // shifted letter as the uppercase name "L" (with shift=true), so match
    // that; also accept "l" for layouts that report the unshifted name.
    if is_shift && is_ctrl && (key_name == "L" || key_name == "l") {
        crate::app::save_position(&mut state.borrow_mut());
        let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
        state.borrow().window.close();
        return true;
    }

    // Journal vim-edit mode owns ALL keys (including space, which Insert mode must
    // type literally), so route it BEFORE the global space / play-pause guards.
    // The emergency Shift+Ctrl+L quit above still wins. handle_journal_edit_key
    // translates the GTK key (+ key_char for printables) into a VimKey.
    if state.borrow().input_mode == crate::app::InputMode::JournalEdit {
        return handle_journal_edit_key(state, key_name, key_char, is_ctrl, is_shift, tokio_handle);
    }

    // GlossEdit owns ALL keys for the in-place gloss/synopsis vim editor, for the
    // same reasons as JournalEdit above (Insert mode must type space literally),
    // so route it BEFORE the global space / play-pause guards.
    if state.borrow().input_mode == crate::app::InputMode::GlossEdit {
        return handle_gloss_edit_key(state, key_name, key_char, is_ctrl, is_shift, tokio_handle);
    }

    // SegmentVim (the reader's `v` copy-only vim view) owns ALL keys for the
    // same reason as the two editors above.
    if state.borrow().input_mode == crate::app::InputMode::SegmentVim {
        return handle_segment_vim_key(state, key_name, key_char, is_ctrl);
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
            // Prose: space plays from the cursor line's start time; `a` is the
            // pure pause/resume toggle. Poetry/plays SWAP the two, so on those
            // works space is the pause toggle and `a` plays from the cursor
            // line (the `a` swap is handled in the Reader `a` intercept below).
            if reader_swaps_play_and_pause(state) {
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
            } else {
                let mut s = state.borrow_mut();
                if !crate::input::timestamps::play_current_line(&mut s) {
                    show_no_timestamp_toast(&s);
                }
            }
            return true;
        }
        // Non-editable, non-Reader, non-gloss (e.g. an overlay): fall through
        // to mode dispatch so the overlay's own space arm runs.
    }

    // Reader `a`: mirror of the space swap above. On prose, `a` is the pure
    // pause/resume toggle (handled by the TogglePause table bind → dispatch).
    // On poetry/plays, `a` instead plays from the cursor line's start time
    // (space takes the pause toggle). Intercepted here, before table dispatch,
    // so the swapped meaning wins over the compiled `a` → TogglePause bind.
    if key_name == "a"
        && !is_ctrl
        && !is_shift
        && !is_alt
        && state.borrow().input_mode == crate::app::InputMode::Reader
        && reader_swaps_play_and_pause(state)
    {
        let mut s = state.borrow_mut();
        if !crate::input::timestamps::play_current_line(&mut s) {
            show_no_timestamp_toast(&s);
        }
        return true;
    }

    // Global theme cycling — works in EVERY overlay, not just reader mode.
    // Ctrl+t / Ctrl+Shift+t cycle the reader theme regardless of the active
    // overlay. Resolved through the keymap so keymap.json overrides still apply;
    // scoped to the Theme* actions ONLY (we do NOT leak every reader bind into
    // overlays). No overlay handler binds these chords, so there is no conflict.
    // The vim editors (JournalEdit/GlossEdit/SegmentVim) are intercepted above
    // this point, so typing in them is unaffected.
    {
        use crate::input::actions::Action;
        // Bind on its own line so the `state.borrow()` temporary drops at the `;`
        // BEFORE dispatch_action → cycle_theme calls state.borrow_mut(). An
        // `if let` would hold the read borrow across the whole body → a RefCell
        // double-borrow → non-unwinding abort (mirrors the reader path at the
        // main keymap.lookup dispatch below).
        let theme_action = state.borrow().keymap.lookup(key_name, is_ctrl, is_shift, is_alt);
        if let Some(action @ (Action::ThemeNext | Action::ThemePrev)) = theme_action {
            dispatch_action(state, action, key_state, tokio_handle);
            return true;
        }
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
            | crate::app::InputMode::JournalPicker
            | crate::app::InputMode::JournalMovePicker
            | crate::app::InputMode::JournalTermInput
            | crate::app::InputMode::GlossPicker => handle_picker_key(state, key_name, is_ctrl, is_alt, tokio_handle, mode),
            crate::app::InputMode::Settings => handle_settings_key(state, key_name, is_ctrl),
            crate::app::InputMode::VocabLoop => handle_vocab_loop_key(state, key_name, is_ctrl),
            crate::app::InputMode::VoicePicker => handle_voice_picker_key(state, key_name, is_ctrl),
            crate::app::InputMode::Search => handle_search_key(state, key_name),
            crate::app::InputMode::OverlaySearchInput => handle_overlay_search_input_key(state, key_name),
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_state, key_name, key_char, is_ctrl, is_shift, is_alt, tokio_handle),
            crate::app::InputMode::GlossVisual => handle_block_visual_key(state, key_state, key_name, &GLOSS_VISUAL_CFG),
            crate::app::InputMode::JournalOverlay => handle_journal_key(state, key_state, key_name, key_char, is_ctrl, is_alt),
            // JournalEdit is intercepted at the top of handle_key (before the
            // global guards), so it never reaches this match.
            crate::app::InputMode::JournalEdit => unreachable!("JournalEdit handled before mode dispatch"),
            // GlossEdit is intercepted at the top of handle_key (before the
            // global guards), so it never reaches this match.
            crate::app::InputMode::GlossEdit => unreachable!("GlossEdit handled before mode dispatch"),
            crate::app::InputMode::SegmentVim => unreachable!("SegmentVim handled before mode dispatch"),
            crate::app::InputMode::JournalVisual => handle_journal_visual_key(state, key_state, key_name),
            crate::app::InputMode::SynopsisOverlay => handle_synopsis_overlay_key(state, key_state, key_name, key_char, is_ctrl, is_alt, is_shift),
            crate::app::InputMode::SynopsisVisual => handle_block_visual_key(state, key_state, key_name, &SYNOPSIS_VISUAL_CFG),
            crate::app::InputMode::TranslationOverlay => handle_translation_overlay_key(state, key_name, is_ctrl),
            crate::app::InputMode::DeleteConfirm => handle_delete_confirm_key(state, key_name),
            crate::app::InputMode::UndoConfirm => handle_undo_confirm_key(state, key_name),
            crate::app::InputMode::RewriteTargetChoice => handle_rewrite_target_key(state, key_name),
            crate::app::InputMode::EchoPicker => handle_echo_picker_key(state, key_name, tokio_handle),
            crate::app::InputMode::EchoTurnsPicker => handle_echo_turns_picker_key(state, key_name, tokio_handle),
            crate::app::InputMode::EchoesOverlay => handle_echoes_overlay_key(state, key_state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::GamepadOverlay => handle_gamepad_key(state, key_name),
            crate::app::InputMode::KeybindsOverlay => handle_keybinds_key(state, key_name),
            crate::app::InputMode::EchoKeybindsOverlay => handle_echo_keybinds_key(state, key_name, is_ctrl),
            crate::app::InputMode::GlossKeybindsOverlay => handle_overlay_keybinds_key(state, key_name, is_ctrl, OverlayLegend::Gloss),
            crate::app::InputMode::SynopsisKeybindsOverlay => handle_overlay_keybinds_key(state, key_name, is_ctrl, OverlayLegend::Synopsis),
            crate::app::InputMode::JournalKeybindsOverlay => handle_overlay_keybinds_key(state, key_name, is_ctrl, OverlayLegend::Journal),
            crate::app::InputMode::ActionPopup => handle_action_popup_key(state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name, is_ctrl, tokio_handle),
            crate::app::InputMode::PageCalibration => handle_page_calibration_key(state, key_state, key_name),
            crate::app::InputMode::ChatPrompt => handle_chat_prompt_key(state, key_name, key_char, is_ctrl),
            crate::app::InputMode::ChatTranscript => handle_chat_transcript_key(state, key_name, is_ctrl),
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

    // rr sequence check: a second quick `r` toggles the vocab popup's
    // visibility — show when hidden, hide when visible (the first tap
    // already cycled a word when it was visible; see Action::VocabPopupTap).
    // Any other key clears the pending tap and dispatches normally.
    if key_state.borrow().chord == ChordState::PendingR {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "r" && !is_ctrl && !is_shift && !is_alt {
            let mut s = state.borrow_mut();
            if s.vocab_popup.popup.is_visible() {
                s.vocab_popup.auto = false;
                crate::app::vocab_popup::close_vocab_popup(&mut s);
            } else {
                s.vocab_popup.auto = true;
                crate::app::vocab_popup::open_vocab_popup(&mut s);
            }
            return true;
        }
    }

    // .. sequence check: the first `.` toggled the bookmark
    // (Action::BookmarkTap); the second quick `.` reverts that toggle (net
    // zero) and opens the bookmark picker instead.
    if key_state.borrow().chord == ChordState::PendingPeriod {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "period" && !is_ctrl && !is_shift && !is_alt {
            crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle);
            crate::input::actions::pickers::open_bookmark_picker(state, tokio_handle);
            return true;
        }
    }

    // ss sequence check: the single `s` only toasted the sync state
    // (Action::PlaybackSyncTap); the second quick `s` performs the toggle.
    if key_state.borrow().chord == ChordState::PendingS {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "s" && !is_ctrl && !is_shift && !is_alt {
            toggle_playback_sync(&mut state.borrow_mut());
            return true;
        }
    }

    // BackSpace-BackSpace sequence check: the single tap only toasted the
    // timestamp (Action::DeleteTimestampTap); the second quick tap deletes.
    if key_state.borrow().chord == ChordState::PendingBackspace {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "BackSpace" && !is_ctrl && !is_shift && !is_alt {
            crate::input::timestamps::delete_timestamp(&mut state.borrow_mut());
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
            state.borrow().library_picker.move_selection(1);
            true
        }
        "p" if is_ctrl => {
            state.borrow().library_picker.move_selection(-1);
            true
        }
        "Escape" => {
            let level = state.borrow().library_picker.level().clone();
            match level {
                crate::ui::library_picker::PickerLevel::Works(_) => {
                    state.borrow_mut().library_picker.go_back_to_authors();
                    state.borrow().library_picker.refresh_after_level_change();
                }
                crate::ui::library_picker::PickerLevel::Authors => {
                    state.borrow().library_picker.hide();
                    state.borrow_mut().input_mode = crate::app::InputMode::Reader;
                }
            }
            true
        }
        "Return" => {
            let level = state.borrow().library_picker.level().clone();
            match level {
                crate::ui::library_picker::PickerLevel::Authors => {
                    let selected_name = state
                        .borrow()
                        .library_picker
                        .list_box()
                        .selected_row()
                        .map(|r| r.widget_name().to_string());
                    if let Some(name) = selected_name {
                        if name.starts_with("author:") {
                            let author = name.trim_start_matches("author:").to_string();
                            state.borrow_mut().library_picker.enter_author(&author);
                            state.borrow().library_picker.refresh_after_level_change();
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
            let level = state.borrow().library_picker.level().clone();
            if let crate::ui::library_picker::PickerLevel::Works(_) = level {
                let text = state.borrow().library_picker.search_entry().text().to_string();
                if text.is_empty() {
                    state.borrow_mut().library_picker.go_back_to_authors();
                    state.borrow().library_picker.refresh_after_level_change();
                    return true;
                }
            }
            false
        }
        "Down" => {
            state.borrow().library_picker.move_selection(1);
            true
        }
        "Up" => {
            state.borrow().library_picker.move_selection(-1);
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
                InputMode::GlossPicker => {
                    s.gloss_picker.hide();
                    // If the picker was opened from within the gloss overlay
                    // (Alt+g), the overlay is still visible behind it — return to
                    // it rather than dropping to the reader.
                    if s.gloss_picker_from_overlay {
                        s.gloss_picker_from_overlay = false;
                        s.input_mode = InputMode::GlossOverlay;
                    } else {
                        // Chokepoint: flashes the main-card cursor on return.
                        crate::app::return_to_reader_mode(&mut s);
                    }
                }
                InputMode::JournalPicker => {
                    s.journal_picker.hide();
                    // Opened from the reader (Alt+j, or Ctrl+j on a scene with
                    // no Q&A): nothing was revealed, so go back to the reader
                    // — through the chokepoint, so the cursor flashes — not to
                    // a hidden journal overlay. Opened from the overlay
                    // (Ctrl+\): return to it.
                    if s.journal.picker_from_reader {
                        s.journal.picker_from_reader = false;
                        s.journal.return_pos = None;
                        crate::app::return_to_reader_mode(&mut s);
                    } else {
                        s.input_mode = InputMode::JournalOverlay;
                    }
                }
                InputMode::JournalMovePicker => { s.journal_move_picker.hide(); s.input_mode = InputMode::JournalOverlay; }
                InputMode::JournalTermInput => { s.journal_term_input.hide(); s.input_mode = InputMode::JournalOverlay; }
                InputMode::EchoLinePicker => { drop(s); crate::input::actions::echoes::cancel_add_echo(state); }
                _ => {
                    if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
                        p.hide();
                        // Chokepoint: flashes the main-card cursor on return.
                        crate::app::return_to_reader_mode(&mut s);
                    }
                }
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
                        let buffer_line = navigation::buffer_line_for_line_id(&s, lm_id);
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
                        s.gloss_return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
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
                InputMode::JournalPicker => {
                    crate::input::actions::journal::confirm_picker(state);
                    true
                }
                InputMode::JournalMovePicker => {
                    crate::input::actions::journal::confirm_move_picker(state);
                    true
                }
                InputMode::JournalTermInput => {
                    crate::input::actions::journal::confirm_term_input(state);
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
            let s = state.borrow();
            if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
                p.move_selection(1);
            }
            true
        }
        PickerAction::MoveUp => {
            let s = state.borrow();
            if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
                p.move_selection(-1);
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
                // Escape commits the current live-search jump (same as Return):
                // keep the cursor on the matched line and highlight it, rather
                // than restoring the pre-search position. The live search has
                // already moved current_line/page_top_line to the match's
                // canonical spread; just drop the saved return pos and re-apply
                // the cursor-line highlight after clearing the search-match tags.
                s.search_return_pos = None;
                crate::input::search::clear_search(&mut s);
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
            // Zero matches: hiding the bar here made a failed search look like
            // the keypress was ignored. Keep the bar open (its [0/0] counter
            // stays visible, the query stays editable) and toast the failure;
            // Escape still cancels.
            let no_match = {
                let s = state.borrow();
                !s.search_bar.query().is_empty() && s.search_matches.is_empty()
            };
            if no_match {
                crate::input::search::no_match_toast(&state.borrow());
                return true;
            }
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

/// Keys while typing a regex into the `search_bar` to search the CURRENT
/// journal overlay entry (`InputMode::OverlaySearchInput`, opened by the
/// overlay `/`). Return confirms (sets the pattern on the overlay buffer and
/// returns to the overlay); Escape cancels back to the overlay leaving any
/// prior search untouched; every other key flows to the focused search-bar
/// Entry (return false). Tab is swallowed so it neither toggles playback nor
/// moves focus out of the Entry.
fn handle_overlay_search_input_key(state: &Rc<RefCell<AppState>>, key_name: &str) -> bool {
    // The same search bar serves the journal AND gloss overlays; route confirm
    // and cancel back to whichever overlay opened it (recorded in
    // `overlay_search_origin` by that overlay's `open_overlay_search`).
    let origin = state.borrow().overlay_search_origin;
    let is_gloss = origin == crate::app::InputMode::GlossOverlay;
    match key_name {
        "Return" => {
            if is_gloss {
                crate::input::actions::gloss::confirm_overlay_search(state);
            } else {
                crate::input::actions::journal::confirm_overlay_search(state);
            }
            true
        }
        "Escape" => {
            state.borrow().search_bar.hide();
            state.borrow_mut().input_mode = origin;
            true
        }
        "Tab" | "ISO_Left_Tab" => true,
        _ => false, // let GTK route to the Entry (including Space)
    }
}

/// Outcome of the shared ask-card key intercept.
enum AskIntercept {
    /// The helper consumed the key — the calling handler must `return true`.
    Consumed,
    /// The card is closed — the calling handler continues its own routing.
    NotHandled,
}

/// Route a key to an OPEN ask card that is a vim editor (the prompt input is
/// modal: NORMAL by default, `i`/`a` to type). `feed` drives the overlay's ask
/// engine; `submit`/`close` are the overlay's actions. Submit = `:w`/`:wq` (via
/// the engine action) OR Ctrl+Enter; close = double-Esc OR `:q`/`:q!`; a single
/// Esc goes to the engine (→ Normal mode). Returns `Consumed` while the card is
/// open (vim owns every key), else `NotHandled`.
#[allow(clippy::too_many_arguments)]
fn ask_vim_intercept(
    ask_open: bool,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    state: &Rc<RefCell<AppState>>,
    feed: impl Fn(&Rc<RefCell<AppState>>, crate::input::vim::VimKey) -> crate::input::vim::EditorAction,
    submit: impl Fn(&Rc<RefCell<AppState>>),
    close: impl Fn(&Rc<RefCell<AppState>>),
    paste: fn(&Rc<RefCell<AppState>>, &str),
) -> AskIntercept {
    use crate::input::vim::{EditorAction, VimKey};
    if !ask_open {
        return AskIntercept::NotHandled;
    }
    // Esc: single → Normal mode (engine); double (quick) → close the prompt.
    if key_name == "Escape" && !is_ctrl {
        if is_double_esc() {
            close(state);
        } else {
            feed(state, VimKey::Esc);
        }
        return AskIntercept::Consumed;
    }
    // Ctrl+Enter is an always-available submit (in addition to `:w`).
    if is_ctrl && key_name == "Return" {
        submit(state);
        return AskIntercept::Consumed;
    }
    // Ctrl+v / Ctrl+Shift+V: paste the system clipboard into the prompt.
    if is_ctrl && matches!(key_name, "v" | "V") {
        paste_clipboard(state, paste);
        return AskIntercept::Consumed;
    }
    if let Some(vk) = gtk_key_to_vim(key_name, key_char, is_ctrl) {
        match feed(state, vk) {
            EditorAction::Save | EditorAction::SaveQuit => submit(state),
            EditorAction::Cancel | EditorAction::CancelForce => close(state),
            // Visual `y`: copy the yanked selection to the system clipboard.
            EditorAction::CopyToClipboard(text) => copy_to_clipboard(&text),
            _ => {}
        }
    }
    // The prompt is a vim editor: consume EVERY key while it is open.
    AskIntercept::Consumed
}

/// Read the system clipboard and hand its text to `apply` (a paste sink on one
/// of the vim surfaces). GTK's clipboard API is async-only, so the paste lands
/// on the main loop a moment later; empty / non-text clipboards are ignored.
/// `apply` is a plain fn pointer so the 'static callback captures no borrows.
///
/// Line endings are normalized to `\n` here, for every sink: GDK's text
/// deserializer does NOT normalize (it is charset conversion only), so CRLF
/// from Windows/web sources would otherwise put literal `\r` chars into the
/// engine buffer — persisting into lit.db on `:w` and breaking the journal's
/// `\n\n` question/answer split.
fn paste_clipboard(state: &Rc<RefCell<AppState>>, apply: fn(&Rc<RefCell<AppState>>, &str)) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let state = state.clone();
    display.clipboard().read_text_async(
        gtk4::gio::Cancellable::NONE,
        move |res| {
            if let Ok(Some(text)) = res {
                if !text.is_empty() {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    apply(&state, &text);
                }
            }
        },
    );
}

/// Write `text` to the system clipboard (visual-mode `y` in the vim editors).
/// Uses the GDK clipboard, which on Wayland owns the selection for as long as
/// the app runs — the same channel `paste_clipboard` reads back.
fn copy_to_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(display) = gtk4::gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

thread_local! {
    /// Timestamp of the last Escape press in a vim editor (the journal page
    /// editor OR an ask-card prompt), for the double-Esc-to-exit gesture. A
    /// single Esc goes to vim Normal mode; two in quick succession (< DOUBLE_ESC_MS)
    /// exit/close. Shared by both vim surfaces (only one is active at a time).
    static LAST_EDIT_ESC: std::cell::Cell<Option<std::time::Instant>> =
        const { std::cell::Cell::new(None) };
}
const DOUBLE_ESC_MS: u128 = 400;

/// True when this Esc press is the SECOND within `DOUBLE_ESC_MS` of the last one
/// (and resets the timer on a double). Used for the double-Esc exit gesture.
fn is_double_esc() -> bool {
    let now = std::time::Instant::now();
    let double = LAST_EDIT_ESC.with(|c| {
        let prev = c.get();
        c.set(Some(now));
        prev.is_some_and(|p| now.duration_since(p).as_millis() < DOUBLE_ESC_MS)
    });
    if double {
        LAST_EDIT_ESC.with(|c| c.set(None));
    }
    double
}

/// Translate a GTK key (name + the printable `key_char` + ctrl) into a `VimKey`
/// for a vim surface. `None` = not editor input (swallow). Esc is handled by the
/// caller (double-Esc timing), so it is NOT mapped here.
fn gtk_key_to_vim(
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
) -> Option<crate::input::vim::VimKey> {
    use crate::input::vim::VimKey;
    match key_name {
        "Return" | "KP_Enter" => Some(VimKey::Enter),
        "BackSpace" => Some(VimKey::Backspace),
        "Tab" | "ISO_Left_Tab" => Some(VimKey::Tab),
        "r" | "R" if is_ctrl => Some(VimKey::CtrlR),
        _ => {
            if is_ctrl {
                None
            } else {
                key_char.filter(|c| !c.is_control()).map(VimKey::Char)
            }
        }
    }
}

/// The journal in-place vim editor's key handler (InputMode::JournalEdit).
/// Translates the GTK key (+ `key_char` for printables) into a `VimKey`, feeds
/// it to the engine via the overlay, and acts on the returned `EditorAction`.
fn handle_journal_edit_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    _is_shift: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    use crate::input::vim::{EditorAction, VimKey};

    // Esc: a SINGLE Esc returns to vim Normal mode (handled by the engine); TWO
    // in quick succession EXIT the editor. Timing lives here, not in the pure
    // engine (which has no clock).
    if key_name == "Escape" && !is_ctrl {
        if is_double_esc() {
            crate::input::actions::journal::vim_cancel(state, false);
            return true;
        }
        let _ = state.borrow().journal_overlay.feed_edit_key(VimKey::Esc);
        return true;
    }

    // Ctrl+v / Ctrl+Shift+V: paste the system clipboard at the cursor.
    if is_ctrl && matches!(key_name, "v" | "V") {
        paste_clipboard(state, |st, t| st.borrow().journal_overlay.paste_edit_text(t));
        return true;
    }

    let Some(vk) = gtk_key_to_vim(key_name, key_char, is_ctrl) else {
        // Swallow unmapped keys so they don't leak to other handlers while editing.
        return true;
    };

    let action = state.borrow().journal_overlay.feed_edit_key(vk);
    crate::logging::log(&format!("JOURNAL-EDIT: key={:?} -> action={:?}", vk, action));
    match action {
        EditorAction::Nop => true,
        EditorAction::Save => {
            crate::input::actions::journal::vim_save(state, false);
            true
        }
        EditorAction::SaveQuit => {
            crate::input::actions::journal::vim_save(state, true);
            true
        }
        EditorAction::Cancel => {
            crate::input::actions::journal::vim_cancel(state, false);
            true
        }
        EditorAction::CancelForce => {
            crate::input::actions::journal::vim_cancel(state, true);
            true
        }
        EditorAction::OpenRewrite => {
            crate::input::actions::journal::vim_open_rewrite(state, tokio_handle);
            true
        }
        // The engine already toggled the `<hi>` tags in its buffer and
        // feed_edit_key re-mirrored; the dirty-check sees the change on :w/:q.
        EditorAction::ToggleHighlight => true,
        // Visual `y`: engine yanked to its register; also copy to the system
        // clipboard so the selection can paste into other apps.
        EditorAction::CopyToClipboard(text) => {
            copy_to_clipboard(&text);
            true
        }
    }
}

/// Route a key to the gloss/synopsis in-place vim editor (`InputMode::GlossEdit`).
/// Near-clone of `handle_journal_edit_key`; `:w`/`:wq`/`:q`/`:q!`/`R` branch to the
/// gloss vs synopsis handler by `GlossOverlay::is_showing_synopsis`.
fn handle_gloss_edit_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    _is_shift: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    use crate::input::vim::{EditorAction, VimKey};

    // Esc: a SINGLE Esc returns to vim Normal mode (handled by the engine); TWO
    // in quick succession EXIT the editor (force cancel). Timing lives here.
    if key_name == "Escape" && !is_ctrl {
        if is_double_esc() {
            // Read the surface inline here: no save has run, so paginated_mode is
            // still the surface the editor was entered from.
            let synopsis = state.borrow().gloss_overlay.is_showing_synopsis();
            if synopsis {
                crate::input::actions::synopsis::vim_cancel(state, true);
            } else {
                crate::input::actions::gloss::vim_cancel(state, true);
            }
            return true;
        }
        let _ = state.borrow().gloss_overlay.feed_edit_key(VimKey::Esc);
        return true;
    }

    // Ctrl+v / Ctrl+Shift+V: paste the system clipboard at the cursor.
    if is_ctrl && matches!(key_name, "v" | "V") {
        paste_clipboard(state, |st, t| st.borrow().gloss_overlay.paste_edit_text(t));
        return true;
    }

    let Some(vk) = gtk_key_to_vim(key_name, key_char, is_ctrl) else {
        // Swallow unmapped keys so they don't leak to other handlers while editing.
        return true;
    };

    let action = state.borrow().gloss_overlay.feed_edit_key(vk);
    // Read the surface BEFORE the save/cancel call: the gloss/synopsis
    // vim_save/vim_cancel re-render and may flip paginated_mode (which backs
    // is_showing_synopsis), so capturing it after the match would route wrong.
    let synopsis = state.borrow().gloss_overlay.is_showing_synopsis();
    crate::logging::log(&format!(
        "GLOSS-EDIT: key={:?} -> action={:?} synopsis={}",
        vk, action, synopsis
    ));
    match action {
        EditorAction::Nop => true,
        EditorAction::Save => {
            if synopsis {
                crate::input::actions::synopsis::vim_save(state, false);
            } else {
                crate::input::actions::gloss::vim_save(state, false);
            }
            true
        }
        EditorAction::SaveQuit => {
            if synopsis {
                crate::input::actions::synopsis::vim_save(state, true);
            } else {
                crate::input::actions::gloss::vim_save(state, true);
            }
            true
        }
        EditorAction::Cancel => {
            if synopsis {
                crate::input::actions::synopsis::vim_cancel(state, false);
            } else {
                crate::input::actions::gloss::vim_cancel(state, false);
            }
            true
        }
        EditorAction::CancelForce => {
            if synopsis {
                crate::input::actions::synopsis::vim_cancel(state, true);
            } else {
                crate::input::actions::gloss::vim_cancel(state, true);
            }
            true
        }
        EditorAction::OpenRewrite => {
            if synopsis {
                crate::input::actions::synopsis::vim_open_rewrite(state, tokio_handle);
            } else {
                crate::input::actions::gloss::vim_open_rewrite(state, tokio_handle);
            }
            true
        }
        // The engine already toggled the `<hi>` tags in its buffer and
        // feed_edit_key re-mirrored; the dirty-check sees the change on :w/:q.
        EditorAction::ToggleHighlight => true,
        // Visual `y`: engine yanked to its register; also copy to the system
        // clipboard so the selection can paste into other apps.
        EditorAction::CopyToClipboard(text) => {
            copy_to_clipboard(&text);
            true
        }
    }
}

/// Key handler for the reader's `v` copy-only vim view (InputMode::SegmentVim).
/// Same engine + GlossOverlay edit buffer as GlossEdit, but every persistence
/// verb is refused: the surface exists to visually select text and copy it to
/// the system clipboard, never to edit the segment. `:q`/`:q!`/double-Esc
/// close; visual `y` copies with a toast and stays open for another selection.
fn handle_segment_vim_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
) -> bool {
    use crate::input::vim::{EditorAction, VimKey};

    // Esc: a SINGLE Esc returns to vim Normal mode (engine-handled); TWO in
    // quick succession close the view — same rhythm as the other vim editors.
    if key_name == "Escape" && !is_ctrl {
        if is_double_esc() {
            crate::input::actions::segment_vim::close(state);
            return true;
        }
        let _ = state.borrow().gloss_overlay.feed_edit_key(VimKey::Esc);
        return true;
    }

    let Some(vk) = gtk_key_to_vim(key_name, key_char, is_ctrl) else {
        // Swallow unmapped keys so they don't leak to other handlers.
        return true;
    };

    let action = state.borrow().gloss_overlay.feed_edit_key(vk);
    match action {
        // Copy-only: every save/rewrite verb is refused with a toast. The
        // engine's buffer may have been mutated (d/x/i are not blocked — they
        // can help whittle a selection) but the result is never persisted.
        EditorAction::Save | EditorAction::SaveQuit | EditorAction::OpenRewrite => {
            crate::input::actions::segment_vim::refuse_save(state);
            true
        }
        EditorAction::Cancel | EditorAction::CancelForce => {
            crate::input::actions::segment_vim::close(state);
            true
        }
        // Visual `y`: copy the selection to the system clipboard and stay open
        // so another span can be selected.
        EditorAction::CopyToClipboard(text) => {
            copy_to_clipboard(&text);
            let s = state.borrow();
            crate::ui::toast::show_transient(&s.chapter_toast, "Copied", 2);
            true
        }
        EditorAction::ToggleHighlight | EditorAction::Nop => true,
    }
}

/// Chat prompt focus: Tab cycles to the transcript BEFORE the vim editor can
/// consume it; Ctrl+Tab closes the panel; everything else feeds the embedded
/// AskCard vim editor via the shared ask_vim_intercept (Ctrl+Enter submits).
fn handle_chat_prompt_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
) -> bool {
    if (key_name == "Tab" || key_name == "ISO_Left_Tab") && is_ctrl {
        crate::input::actions::chat::close_chat_layout(&mut state.borrow_mut());
        return true;
    }
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        crate::input::actions::chat::focus_transcript(&mut state.borrow_mut());
        return true;
    }
    if key_name == "l" && is_ctrl {
        crate::input::actions::chat::flip_panel_side(&mut state.borrow_mut());
        return true;
    }
    match ask_vim_intercept(
        true,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().chat_panel.feed_input_vim_key(k),
        crate::input::actions::chat::submit_chat_prompt,
        // Esc in Normal mode (or :q) hides the input; `a` on the transcript
        // reopens it via focus_prompt.
        |st| {
            let mut s = st.borrow_mut();
            s.chat_panel.close_input();
            crate::input::actions::chat::focus_transcript(&mut s);
        },
        |st, t| st.borrow().chat_panel.paste_input_text(t),
    ) {
        AskIntercept::Consumed => true,
        AskIntercept::NotHandled => true, // prompt focus consumes everything
    }
}

/// Chat transcript focus: j/k move the exchange cursor, s saves the selected
/// exchange, `a` re-shows the retired ask input, Tab cycles to the reader,
/// Ctrl+Tab closes.
fn handle_chat_transcript_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    match key_name {
        "Tab" | "ISO_Left_Tab" if is_ctrl => {
            crate::input::actions::chat::close_chat_layout(&mut state.borrow_mut());
            true
        }
        "Tab" | "ISO_Left_Tab" => {
            crate::input::actions::chat::focus_reader(&mut state.borrow_mut());
            true
        }
        "j" => {
            crate::input::actions::chat::transcript_cursor_move(&mut state.borrow_mut(), 1);
            true
        }
        "k" => {
            crate::input::actions::chat::transcript_cursor_move(&mut state.borrow_mut(), -1);
            true
        }
        "s" => {
            crate::input::actions::chat::save_selected_exchange(state);
            true
        }
        "a" => {
            // Ask: re-show the retired input and focus it.
            crate::input::actions::chat::focus_prompt(&mut state.borrow_mut());
            true
        }
        "l" if is_ctrl => {
            crate::input::actions::chat::flip_panel_side(&mut state.borrow_mut());
            true
        }
        "Escape" => {
            crate::input::actions::chat::focus_reader(&mut state.borrow_mut());
            true
        }
        _ => true,
    }
}

fn handle_journal_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    is_alt: bool,
) -> bool {
    // The `e` editor is an in-place vim mode (InputMode::JournalEdit, handled at
    // the top of handle_key). The ask card (the `r` create + `R` rewrite prompts)
    // is itself a vim editor, intercepted here.

    // ---- Ask input card (a vim editor) intercepts all keys while open ----
    let ask_open = state.borrow().journal_overlay.ask_is_open();
    match ask_vim_intercept(
        ask_open,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().journal_overlay.feed_ask_vim_key(k),
        crate::input::actions::journal::submit_prompt,
        crate::input::actions::journal::close_prompt,
        |st, t| st.borrow().journal_overlay.paste_ask_text(t),
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::NotHandled => {}
    }

    // gg chord -> first block (mirrors the gloss/synopsis overlays' block cursor)
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        // A ctrl-chord within the gg window cancels the pending `g` and
        // dispatches normally below (Ctrl+g arrives as key_name "g" too, so
        // without this `g` then Ctrl+g would run the gg jump instead of
        // reaching the Ctrl+g consumed no-op arm below).
        if !is_ctrl {
            if key_name == "g" {
                crate::input::actions::gloss::stop_all_gloss_audio(state);
                state.borrow().journal_overlay.cursor_first_block();
                crate::input::actions::gloss::recolor_journal_cached_blocks_rc(state);
            }
            return true;
        }
    }

    // Ctrl+\: open the work-wide Q&A picker. Checked BEFORE the plain Alt/Ctrl
    // blocks so the chord wins over any single-modifier meaning of `\`.
    // Lists every page in the work; confirm lands on the chosen page's band,
    // Escape returns to the journal overlay.
    if is_ctrl && key_name == "backslash" {
        crate::input::actions::journal::open_picker(state);
        return true;
    }

    if is_alt {
        match key_name {
            "n" => {
                crate::input::actions::journal::nav_scene(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::journal::nav_scene(state, -1);
                return true;
            }
            "w" => {
                crate::input::actions::journal::nav_to_work_band(state);
                return true;
            }
            // Alt+s: jump to the Scene band for the main card's current line —
            // a direct jump (like Alt+w/Alt+a), the same band the overlay opens
            // on. Returns to the reading position's scene from the author/work
            // band without closing and reopening the overlay.
            "s" => {
                crate::input::actions::journal::nav_to_scene_band(state);
                return true;
            }
            // Alt+a: jump to the author/corpus band (scope='author' pages for
            // the current work's author). A jump target, not part of the
            // sequential band walk (Alt+n/p scenes, Alt+w work).
            "a" => {
                crate::input::actions::journal::nav_to_author_band(state);
                return true;
            }
            // Alt+g: dropped (cross-create: reader-gloss from the journal
            // passage). Consumed so Alt+g can't start a gg chord below.
            "g" => return true,
            _ => {}
        }
    }

    if is_ctrl {
        match key_name {
            "n" => {
                crate::input::actions::journal::nav_page(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::journal::nav_page(state, -1);
                return true;
            }
            // Ctrl+j: Escape-only close policy — consumed no-op (was: close
            // the journal). Consumed so Ctrl+j can't fall through to the
            // plain j block-nav arm.
            "j" => return true,
            // Ctrl+Tab: dropped inside overlays (reader-side Ctrl+Tab still
            // reopens the last overlay). Consumed so it can't fall through
            // to the plain Tab TTS arm.
            "Tab" | "ISO_Left_Tab" => return true,
            // Ctrl+Shift+J: open the "move this Q&A to another band" picker.
            // Arrives as key_name "J" (shifted), distinct from Ctrl+j.
            "J" => {
                crate::input::actions::journal::open_move_picker(state);
                return true;
            }
            // Ctrl+g: dropped (cross-jump to the gloss view — the \ cycle is
            // the only overlay-to-overlay navigation). Consumed so it can't
            // start a gg chord below.
            "g" => return true,
            // Ctrl+s: always (re)start the cursor paragraph's TTS from the
            // beginning (cache hit plays the stored MP3, miss synthesizes via
            // ElevenLabs). Moved off plain `s` so synthesis requires the Ctrl
            // modifier.
            "s" => {
                crate::input::actions::gloss::begin_current_journal_block(state);
                return true;
            }
            // Ctrl+/ opens the JOURNAL-specific keybind legend (its full keybind
            // set), returning to the journal overlay on close.
            "slash" => {
                open_overlay_legend(&mut state.borrow_mut(), OverlayLegend::Journal);
                return true;
            }
            _ => {}
        }
    }

    // While a term filter is active the overlay shows a read-only cross-work
    // entry via show_page; s.journal.pages/page_index still point at the origin
    // band, so the mutating and block-nav arms below would act on the WRONG
    // (origin) entry — risking edits/deletes on the wrong lit.db row. Swallow
    // them with a hint. `f` (re-search), `Escape` (clears the filter), and the
    // Ctrl+n/p stepping (handled in the is_ctrl block above via nav_page's
    // filter branch) stay live; `s` is a consumed no-op below.
    // While a term filter is active the overlay shows a cross-work match via
    // `render_filtered_match`. LIVE keys operate on the DISPLAYED entry
    // (displayed_journal_page → the filter match) and re-render the filtered
    // view: block-nav j/k/x/y/g/G (overlay buffer only), the entry ops R/e/D/c,
    // undo u (reverts the displayed entry's edit), and visual-select V (selects
    // in the overlay buffer, yanks to clipboard — no DB/origin read). Still
    // GATED — semantically ambiguous or origin-bound under a cross-work browse:
    // new-Q&A (r, no clear home band), overlay-cycle (backslash, would switch
    // works), TTS (space/a — the audio cache is keyed by entry_id + work_abbrev
    // of the ORIGIN, so cross-work playback would mis-path the cache). Clear the
    // filter (Esc) to use these. Ctrl+n/p step the subset; f re-searches.
    if state.borrow().journal.filter.is_some()
        && matches!(key_name, "r" | "space" | "a" | "backslash")
    {
        crate::ui::toast::show_transient(
            &state.borrow().chapter_toast,
            "Clear the term filter (Esc) for this key",
            3,
        );
        return true;
    }

    match key_name {
        // `r` opens the ask card to create a new Q&A in the current band,
        // matching the gloss + synopsis overlays where `r` opens a journal Q&A.
        // (Moved from A to r across all three overlays.)
        "r" => {
            crate::input::actions::journal::begin_ask(state);
            true
        }
        // `R` opens the rewrite TARGET chooser (q question / a answer / b both):
        // a single key then routes to improve-question, the answer-only rewrite
        // prompt, or both. Works directly from the Q&A view (no `e` editor).
        "R" => {
            crate::input::actions::journal::open_rewrite_target(state);
            true
        }
        "e" => {
            crate::input::actions::journal::begin_edit(state);
            true
        }
        // u: undo the last `e` edit (single-level), behind a y/Esc confirmation.
        "u" => {
            crate::input::actions::gloss::show_undo_confirmation(
                state,
                crate::app::InputMode::JournalOverlay,
            );
            true
        }
        "D" => {
            crate::input::actions::gloss::show_delete_confirmation(
                state,
                crate::app::InputMode::JournalOverlay,
            );
            true
        }
        "V" => {
            let entered = state.borrow().journal_overlay.enter_visual();
            if entered {
                let mut s = state.borrow_mut();
                s.input_mode = crate::app::InputMode::JournalVisual;
                s.journal_overlay.set_journal_visual_hint();
            }
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            state.borrow().journal_overlay.cursor_last_block();
            crate::input::actions::gloss::recolor_journal_cached_blocks_rc(state);
            true
        }
        // j/k step the paragraph block cursor (the left accent bar), mirroring
        // the gloss/synopsis overlays; Space/Tab read the cursor block aloud and
        // `a` restarts it. Silence audio on nav, like the other overlays' j/k.
        // `q` aliases j and `,` aliases k, matching the reading card's
        // dialogue-nav keys (q = next, comma = prev) so the same fingers move the
        // block cursor here.
        "j" | "q" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Block-less render (pending-passage source): scroll the viewport.
            if state.borrow().journal_overlay.has_nav_blocks() {
                state.borrow().journal_overlay.cursor_next_block();
                // A page turn re-rendered the buffer; recolor cached blocks.
                crate::input::actions::gloss::recolor_journal_cached_blocks_rc(state);
            } else {
                state.borrow().journal_overlay.scroll_view(1);
            }
            true
        }
        "k" | "comma" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().journal_overlay.has_nav_blocks() {
                state.borrow().journal_overlay.cursor_prev_block();
                crate::input::actions::gloss::recolor_journal_cached_blocks_rc(state);
            } else {
                state.borrow().journal_overlay.scroll_view(-1);
            }
            true
        }
        // x/y: turn to the next/prev RENDER page of the current Q&A (same entry),
        // landing the cursor on the first block of that page — a whole-page jump,
        // unlike j/k which step one block. No-op at the first/last page.
        "x" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().journal_overlay.has_nav_blocks() {
                state.borrow().journal_overlay.page_turn(1);
                crate::input::actions::gloss::recolor_journal_cached_blocks_rc(state);
            } else {
                state.borrow().journal_overlay.scroll_view(1);
            }
            true
        }
        "y" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().journal_overlay.has_nav_blocks() {
                state.borrow().journal_overlay.page_turn(-1);
                crate::input::actions::gloss::recolor_journal_cached_blocks_rc(state);
            } else {
                state.borrow().journal_overlay.scroll_view(-1);
            }
            true
        }
        // Space: play/stop the cursor paragraph's TTS (cache hit plays the
        // stored MP3, miss synthesizes via ElevenLabs). Tab/ISO_Left_Tab no
        // longer synthesize — dropped by request.
        "space" => {
            crate::input::actions::gloss::read_current_journal_block(state);
            true
        }
        // `a`: toggle play/pause of the cursor block's TTS (starts it only
        // when a cached MP3 exists; never synthesizes).
        "a" => {
            crate::input::actions::gloss::toggle_pause_current_journal_block(state);
            true
        }
        // `s`: dropped — TTS (re)start moved to Ctrl+s (handled in the is_ctrl
        // block above) so plain `s` no longer synthesizes. Consumed no-op.
        "s" => true,
        "c" => {
            crate::input::actions::journal::copy_current_id(state);
            true
        }
        // `\`: advance the segment-overlay cycle → gloss for the lap's entry
        // segment (Ctrl+\ = work-wide picker, handled above; Alt+\ excluded).
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::overlay_cycle::cycle_from_journal(state);
            true
        }
        // `f`: open the term-browse input (cross-work journal filter by tag/term).
        // Suggests existing distinct tags; Enter searches the typed/selected term.
        "f" => {
            crate::input::actions::journal::open_term_input(state);
            true
        }
        // `/`: open the search bar to type a regex for the CURRENT overlay entry.
        // Acts only on the overlay buffer, so it is SAFE under an active term
        // filter (excluded from the mutating-key gate above).
        "slash" => {
            crate::input::actions::journal::open_overlay_search(state);
            true
        }
        // n / N: step the current entry's search matches (revive the MRU pattern
        // when no live search but a prior one exists). Buffer-only → safe under
        // a filter.
        "n" => {
            crate::input::actions::journal::step_overlay_search(state, true);
            true
        }
        "N" => {
            crate::input::actions::journal::step_overlay_search(state, false);
            true
        }
        // Escape precedence: an active overlay search clears first (stay in the
        // overlay); else an active term filter clears (stay); else close.
        "Escape" => {
            if crate::input::actions::journal::clear_overlay_search(state) {
                // cleared a live search; stay in the overlay
            } else if state.borrow().journal.filter.is_some() {
                crate::input::actions::journal::clear_filter(state);
            } else {
                crate::input::actions::journal::close_overlay(state);
            }
            true
        }
        _ => false,
    }
}

fn handle_gloss_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    is_shift: bool,
    is_alt: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    // ---- Stacked add/edit input card (a vim editor) -----------------------
    // The prompt is modal vim: NORMAL by default, `i`/`a` to type; `:w`/Ctrl+Enter
    // submits; double-Esc / `:q` closes. Handled before gloss nav keys.
    let ask_open = state.borrow().gloss_overlay.ask_is_open();
    match ask_vim_intercept(
        ask_open,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().gloss_overlay.feed_ask_vim_key(k),
        crate::input::actions::gloss::submit_gloss_prompt,
        crate::input::actions::gloss::close_gloss_prompt,
        |st, t| st.borrow().gloss_overlay.paste_ask_text(t),
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::NotHandled => {}
    }

    // Shift+Space: batch-synthesize all prose blocks (cache-only).
    if key_name == "space" && is_shift {
        crate::input::actions::gloss::synth_all_prose_blocks(state);
        return true;
    }

    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        // A ctrl-chord within the gg window cancels the pending `g` and
        // dispatches normally below — otherwise `g` then Ctrl+g would run the
        // gg jump instead of reaching the Ctrl+g/Ctrl+j consumed no-op arms
        // below (Ctrl+g arrives as key_name "g" too).
        if !is_ctrl {
            if key_name == "g" {
                crate::input::actions::gloss::stop_all_gloss_audio(state);
                // Loading card (no blocks): scroll the viewport to the top.
                // Result gloss: jump the block cursor to the first block.
                let has_blocks = state.borrow().gloss_overlay.current_block().is_some();
                if has_blocks {
                    state.borrow().gloss_overlay.cursor_first_block();
                    // A page turn re-rendered the buffer; recolor cached blocks.
                    crate::input::actions::gloss::recolor_cached_blocks_rc(state);
                } else {
                    state.borrow().gloss_overlay.scroll_gloss_to_top();
                }
            }
            return true;
        }
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
            // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the
            // only overlay-to-overlay navigation). Consumed no-op.
            "j" => return true,
            // Ctrl+g: Escape-only close policy — consumed no-op (was: close
            // same as Escape). Consumed so it can't start a gg chord below.
            "g" => return true,
            // Ctrl+Tab: dropped inside overlays (reader-side Ctrl+Tab still
            // reopens the last overlay). Consumed so it can't fall through
            // to the plain Tab TTS arm.
            "Tab" | "ISO_Left_Tab" => return true,
            // Ctrl+/ opens the GLOSS-specific keybind legend (its full keybind
            // set), returning to the gloss overlay on close.
            "slash" => {
                open_overlay_legend(&mut state.borrow_mut(), OverlayLegend::Gloss);
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
        "c" => {
            crate::input::actions::gloss::copy_gloss_id(state);
            true
        }
        "D" => {
            crate::input::actions::gloss::show_delete_confirmation(
                state,
                crate::app::InputMode::GlossOverlay,
            );
            true
        }
        "e" => {
            crate::input::actions::gloss::begin_edit(state);
            true
        }
        // R: ask-Claude rewrite of the displayed gloss, straight from the read
        // view (same prompt the vim editor's `R` opens — no need to enter `e`
        // first). Opens in INSERT. Mirrors journal `R`.
        "R" => {
            crate::input::actions::gloss::begin_rewrite(state);
            true
        }
        // u: undo the last `e` edit (single-level), behind a y/Esc confirmation.
        "u" => {
            crate::input::actions::gloss::show_undo_confirmation(
                state,
                crate::app::InputMode::GlossOverlay,
            );
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
                // A page turn re-rendered the buffer; recolor cached blocks.
                crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            } else {
                state.borrow().gloss_overlay.scroll_gloss_to_bottom();
            }
            true
        }
        // (Font-size adjust removed: the gloss + journal overlays are locked to
        // GLOSS_DEFAULT_FONT_SIZE so they always render at the same size.)
        // `q` aliases j and `,` aliases k, matching the reading card's
        // dialogue-nav keys (and the journal overlay) so the same fingers move
        // the block cursor here.
        "j" | "q" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport down.
            // Result gloss: step the block cursor to the next block.
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_next_block();
                // A page turn re-rendered the buffer; recolor cached blocks.
                crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            } else {
                state.borrow().gloss_overlay.scroll_gloss(1);
            }
            true
        }
        "k" | "comma" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_prev_block();
                // A page turn re-rendered the buffer; recolor cached blocks.
                crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            } else {
                state.borrow().gloss_overlay.scroll_gloss(-1);
            }
            true
        }
        // x/y: turn to the next/prev RENDER page of the current gloss, landing
        // the cursor on the first block of that page — a whole-page jump, unlike
        // j/k which step one block. No-op at the first/last page. On the loading
        // card (no blocks) fall back to the scroll, mirroring j/k.
        "x" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.page_turn(1);
                crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            } else {
                state.borrow().gloss_overlay.scroll_gloss(1);
            }
            true
        }
        "y" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.page_turn(-1);
                crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            } else {
                state.borrow().gloss_overlay.scroll_gloss(-1);
            }
            true
        }
        // Space reads the cursor block aloud (cache hit plays, miss
        // synthesizes). Tab/ISO_Left_Tab no longer synthesize — dropped by
        // request. The Ctrl+Tab guard above already consumes the chord.
        "space" => {
            crate::input::actions::gloss::read_current_block(state);
            true
        }
        // `\`: advance the segment-overlay cycle → synopsis for the lap's
        // entry segment (restores the pre-open page, unlike Escape's
        // jump-to-source close).
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::overlay_cycle::cycle_from_gloss(state);
            true
        }
        // `/`: open the search bar to type a regex for the CURRENT gloss buffer
        // (mirrors the journal overlay's `/`). Ctrl+/ (the gloss keybind legend)
        // is handled in the is_ctrl block above, so this plain-`/` arm is safe.
        "slash" => {
            crate::input::actions::gloss::open_overlay_search(state);
            true
        }
        // Escape precedence: an active overlay search clears first (stay in the
        // overlay); else close to the reader, landing on the glossed passage's
        // source line (recomputes the reader-gloss tint so a just-created gloss
        // colors without a reload; falls back to the pre-open page). Gloss has no
        // journal-style term filter, so the precedence is simpler.
        "Escape" => {
            if crate::input::actions::gloss::clear_overlay_search(state) {
                // cleared a live search; stay in the overlay
            } else {
                crate::input::actions::gloss::close_gloss_to_reader(state);
            }
            true
        }
        // n / N: step the current gloss's search matches (revive the MRU pattern
        // when no live search but a prior one exists). Buffer-only. Was a
        // consumed no-op (Escape-only close policy retired `n` as a close alias).
        "n" => {
            crate::input::actions::gloss::step_overlay_search(state, true);
            true
        }
        "N" => {
            crate::input::actions::gloss::step_overlay_search(state, false);
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
        // r: dropped (cross-create: journal ask card for the gloss passage;
        // asking happens from the reader). Consumed no-op.
        "r" => true,
        "v" => {
            crate::input::actions::settings::open_voice_picker(
                state,
                crate::app::VoicePickerOrigin::GlossOverlay,
            );
            true
        }
        "l" => {
            // Source verse only: play/stop the synthesized MP3 in the active /
            // default voice (pauses MPV first). No picker. (Moved off `r`, which
            // now opens a journal Q&A for the passage.)
            crate::input::actions::gloss::toggle_source_tts(state);
            true
        }
        "L" => {
            // Source verse only: open the voice picker for the synthesized
            // reading (the only picker key); confirm sets active voice + plays.
            // (Moved off `R` when `r` was repurposed for the journal Q&A.)
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

fn handle_translation_overlay_key(state: &Rc<RefCell<AppState>>, key_name: &str, is_ctrl: bool) -> bool {
    // i (the same bind that opened the overlay) toggles it closed, matching
    // Escape. Without this, a second i would be swallowed by the catch-all.
    if key_name == "i" {
        let mut s = state.borrow_mut();
        s.translation_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        return true;
    }
    // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the only
    // overlay-to-overlay navigation). Still checked before the plain-`j`
    // dialogue-step so Ctrl+j can't step the cursor.
    if key_name == "j" && is_ctrl {
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
        // x / y turn the OVERLAY page forward / backward (same roles as the main
        // reading card), moving the reader cursor onto the first dialogue line of
        // the new page so the highlight + MPV sync follow.
        "x" => { overlay_page_turn(state, true); true }
        "y" => { overlay_page_turn(state, false); true }
        // Playback (same as the main card): `a` and Space play from the current
        // line (no cursor move, so no re-highlight). Tab is dropped by request —
        // `a` now owns MPV media playback.
        "a" | "space" => {
            let mut s = state.borrow_mut();
            if !crate::input::timestamps::play_current_line(&mut s) {
                show_no_timestamp_toast(&s);
            }
            true
        }
        // Tab/ISO_Left_Tab: dropped — consumed no-op so they don't leak.
        "Tab" | "ISO_Left_Tab" => true,
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
    crate::ui::toast::show_transient(&s.speed_toast, label, 3);
}

/// Show a transient bottom-center toast (reuses `chapter_toast`, 3s auto-hide).
/// Used when a play attempt is a no-op because the line has no timestamp.
fn show_no_timestamp_toast(s: &AppState) {
    crate::ui::toast::show_transient(&s.chapter_toast, "No timestamp on this line", 3);
}

/// Whether the current work swaps the main-card `a` / Space media binds.
/// Prose keeps the default (Space plays from the cursor line, `a` pause-toggles);
/// poetry/plays (any non-prose work type) swap them so `a` plays from the cursor
/// line and Space pause-toggles. False when no work is loaded.
fn reader_swaps_play_and_pause(state: &Rc<RefCell<AppState>>) -> bool {
    let s = state.borrow();
    s.current_work
        .as_ref()
        .is_some_and(|w| !crate::db::line_types::is_prose_work(&w.work_type))
}

/// Run a main-card navigation function (moves `current_line` + seeks MPV via
/// `after_page_change`), then re-highlight and follow in the translation overlay.
fn overlay_nav(state: &Rc<RefCell<AppState>>, nav_fn: fn(&mut AppState)) {
    let scene_before = crate::app::scene_synopsis::current_scene_divs(&state.borrow());
    nav_fn(&mut state.borrow_mut());
    crate::app::translations::sync_translation_overlay(state, scene_before);
}

/// Turn the translation overlay's page forward/backward (x/y), moving the reader
/// cursor onto the FIRST line of the adjacent overlay page so the highlight and
/// MPV sync follow. No-op at the first/last page. The overlay owns its own
/// pagination, so it computes the target work line; we move the real cursor there
/// (via `jump_to_line`, which seeks MPV) and then let `sync_translation_overlay`
/// re-page + re-highlight the overlay.
fn overlay_page_turn(state: &Rc<RefCell<AppState>>, forward: bool) {
    let target_work_idx = state.borrow().translation_overlay.page_turn_target(forward);
    let Some(work_idx) = target_work_idx else { return };
    let scene_before = crate::app::scene_synopsis::current_scene_divs(&state.borrow());
    {
        let mut s = state.borrow_mut();
        // work index -> buffer line (line_map present for plays; identity otherwise).
        let buffer_line = match s.line_map.as_ref() {
            Some(lm) => lm.work_to_buffer.get(work_idx).copied(),
            None => Some(work_idx),
        };
        if let Some(bl) = buffer_line {
            navigation::jump_to_line(&mut s, bl);
        }
    }
    crate::app::translations::sync_translation_overlay(state, scene_before);
}

fn handle_synopsis_overlay_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    is_alt: bool,
    is_shift: bool,
) -> bool {
    let ask_open = state.borrow().gloss_overlay.ask_is_open();

    // The edit prompt is modal vim (NORMAL by default, `i`/`a` to type; `:w`/
    // Ctrl+Enter submits; double-Esc / `:q` closes).
    match ask_vim_intercept(
        ask_open,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().gloss_overlay.feed_ask_vim_key(k),
        crate::input::actions::synopsis::submit_amend_prompt,
        crate::input::actions::synopsis::close_amend_prompt,
        |st, t| st.borrow().gloss_overlay.paste_ask_text(t),
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::NotHandled => {}
    }

    // Closed-card overlay-level semantics (preserved verbatim):
    // Tab is always consumed so it never reaches playback toggle.
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        return true;
    }
    // Escape with the card closed hides the overlay and returns to Reader.
    if key_name == "Escape" {
        let mut s = state.borrow_mut();
        s.gloss_overlay.hide();
        crate::app::return_to_reader_mode(&mut s);
        return true;
    }

    // Ctrl+g: Escape-only close policy — consumed no-op (was: close, same
    // as Escape). Still clears a pending gg chord so a held Ctrl isn't
    // swallowed as the second g.
    if key_name == "g" && is_ctrl {
        key_state.borrow_mut().chord = ChordState::None;
        return true;
    }

    // gg: jump to the first block.
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.cursor_first_block();
            // A page turn re-rendered the buffer; recolor cached blocks.
            crate::input::actions::gloss::recolor_cached_blocks_rc(state);
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
        // h: Escape-only close policy — consumed no-op (was: close; h still
        // OPENS the synopsis from the reader).
        "h" => true,
        // `\`: advance the segment-overlay cycle → wrap to the journal Q&A
        // stop for the lap's entry segment.
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::overlay_cycle::cycle_from_synopsis(state);
            true
        }
        // `a`: always (re)start the cursor paragraph's TTS from the start,
        // mirroring the gloss-overlay `a` (`begin_current_block`). Plain
        // Space/Tab below is the play/pause toggle.
        "a" => {
            crate::input::actions::gloss::begin_current_synopsis_block(state);
            true
        }
        // r: dropped (cross-create: scene ask card; asking happens from the
        // reader). Consumed no-op.
        "r" => true,
        "e" => {
            crate::input::actions::synopsis::begin_edit(state);
            true
        }
        // R: ask-Claude rewrite of the displayed synopsis, straight from the
        // read view (same prompt the vim editor's `R` opens — no need to enter
        // `e` first). Opens in INSERT. Mirrors journal/gloss `R`.
        "R" => {
            crate::input::actions::synopsis::begin_rewrite(state);
            true
        }
        // c: copy the current scene's scene_synopses.id to the clipboard + toast,
        // mirroring gloss `c` (gloss_id) and journal `c` (page id).
        "c" => {
            crate::input::actions::synopsis::copy_synopsis_id(state);
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
        // u: undo the last `e` edit (single-level), behind a y/Esc confirmation.
        // (Was `U`; now lowercased + gated by the confirm like gloss/journal.)
        "u" => {
            crate::input::actions::gloss::show_undo_confirmation(
                state,
                crate::app::InputMode::SynopsisOverlay,
            );
            true
        }
        // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the only
        // overlay-to-overlay navigation). Consumed no-op.
        "j" if is_ctrl => true,
        // (Font-size adjust removed: overlays are locked to GLOSS_DEFAULT_FONT_SIZE.)
        "n" if is_ctrl => {
            crate::app::scene_synopsis::cycle_synopsis(state, 1);
            true
        }
        "p" if is_ctrl => {
            crate::app::scene_synopsis::cycle_synopsis(state, -1);
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
        // Ctrl+/ opens the SYNOPSIS-specific keybind legend (its full keybind
        // set), returning to the synopsis overlay on close.
        "slash" if is_ctrl => {
            open_overlay_legend(&mut state.borrow_mut(), OverlayLegend::Synopsis);
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
            // A page turn re-rendered the buffer; recolor cached blocks.
            crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.cursor_prev_block();
            // A page turn re-rendered the buffer; recolor cached blocks.
            crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            state.borrow().gloss_overlay.cursor_last_block();
            // A page turn re-rendered the buffer; recolor cached blocks.
            crate::input::actions::gloss::recolor_cached_blocks_rc(state);
            true
        }
        // Plain Space: play/stop the cursor paragraph's TTS (Shift+Space, the
        // batch-synth, is handled by the guard above before this match).
        // Tab/ISO_Left_Tab no longer synthesize — dropped by request (the
        // Tab consumed-no-op guard near the top already swallows them).
        "space" => {
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
/// Per-mode variance for the unified visual-block key handler. Plain `fn`
/// pointers over `&GlossOverlay` (both the synopsis and gloss overlays use the
/// GlossOverlay widget) — no trait, no generic. See `handle_block_visual_key`.
struct BlockVisualCfg {
    /// Yank text source: synopsis reads the rendered selection text; gloss reads
    /// the raw buffer block text (source verse + gloss as displayed).
    yank_text: fn(&crate::ui::gloss_overlay::GlossOverlay) -> String,
    /// Log prefix ("SYNOPSIS" / "GLOSS").
    log_tag: &'static str,
    /// Exit on yank (synopsis: exit_visual; gloss: exit_visual_to_start).
    yank_exit: fn(&crate::ui::gloss_overlay::GlossOverlay),
    /// Exit on Escape/V — returns the cursor to the anchor block (where Shift+V
    /// was entered) via `exit_visual_to_anchor`, so cancelling lands back where
    /// the selection started. Separate from `yank_exit` (gloss yank uses
    /// `exit_visual_to_start`).
    escape_exit: fn(&crate::ui::gloss_overlay::GlossOverlay),
    /// InputMode to return to on exit.
    return_mode: crate::app::InputMode,
    /// Hint setter for the returned-to overlay.
    set_hint: fn(&crate::ui::gloss_overlay::GlossOverlay),
}

const SYNOPSIS_VISUAL_CFG: BlockVisualCfg = BlockVisualCfg {
    yank_text: crate::ui::gloss_overlay::GlossOverlay::visual_selection_text,
    log_tag: "SYNOPSIS",
    yank_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual,
    escape_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual_to_anchor,
    return_mode: crate::app::InputMode::SynopsisOverlay,
    set_hint: crate::ui::gloss_overlay::GlossOverlay::set_synopsis_hint,
};

const GLOSS_VISUAL_CFG: BlockVisualCfg = BlockVisualCfg {
    yank_text: crate::ui::gloss_overlay::GlossOverlay::visual_selection_buffer_text,
    log_tag: "GLOSS",
    yank_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual_to_start,
    escape_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual_to_anchor,
    return_mode: crate::app::InputMode::GlossOverlay,
    set_hint: crate::ui::gloss_overlay::GlossOverlay::set_gloss_hint,
};

/// Visual block selection in the synopsis/gloss overlays (entered with Shift+V).
/// gg/G jump the cursor end, j/k extend the selection, y yanks the selected
/// blocks and exits, Esc/V exits without copying. All other keys are consumed.
/// `cfg` carries the per-mode variance (yank text source, log tag, exit fn,
/// return mode, hint fn) — see `SYNOPSIS_VISUAL_CFG` / `GLOSS_VISUAL_CFG`. Escape
/// returns the cursor to the anchor block (`exit_visual_to_anchor`, both modes),
/// while the gloss yank collapses to the selection start (`exit_visual_to_start`);
/// the two separate `*_exit` slots carry that difference.
fn handle_block_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    cfg: &BlockVisualCfg,
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
        // q aliases j and `,` aliases k (extend the selection), matching the
        // normal-mode block-nav aliases in both overlays.
        "j" | "q" => {
            state.borrow().gloss_overlay.visual_step(1);
            true
        }
        "k" | "comma" => {
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
                ((cfg.yank_text)(&s.gloss_overlay), s.gloss_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("{}: copied {} blocks", cfg.log_tag, n));
            }
            {
                let mut s = state.borrow_mut();
                (cfg.yank_exit)(&s.gloss_overlay);
                s.input_mode = cfg.return_mode;
                (cfg.set_hint)(&s.gloss_overlay);
                crate::ui::toast::show_transient(&s.chapter_toast, "Copied", 2);
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            (cfg.escape_exit)(&s.gloss_overlay);
            s.input_mode = cfg.return_mode;
            (cfg.set_hint)(&s.gloss_overlay);
            true
        }
        _ => true,
    }
}

/// Visual block selection in the journal Q&A overlay (entered with Shift+V).
/// gg/G jump the cursor end, j/k extend, y yanks the selected blocks to the
/// clipboard and exits, Esc/V cancel. All other keys are consumed. Parallel to
/// `handle_block_visual_key` but calls `JournalOverlay` (a different type, so it
/// cannot share `BlockVisualCfg`, which is fixed to `GlossOverlay`).
fn handle_journal_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().journal_overlay.visual_to_end(false);
        }
        return true;
    }
    match key_name {
        // q aliases j and `,` aliases k (extend the selection down/up), matching
        // the normal journal-overlay block-nav aliases.
        "j" | "q" => {
            state.borrow().journal_overlay.visual_step(1);
            true
        }
        "k" | "comma" => {
            state.borrow().journal_overlay.visual_step(-1);
            true
        }
        "G" => {
            state.borrow().journal_overlay.visual_to_end(true);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "y" => {
            let (text, n) = {
                let s = state.borrow();
                (s.journal_overlay.visual_selection_text(), s.journal_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("JOURNAL: copied {} blocks", n));
            }
            {
                let mut s = state.borrow_mut();
                s.journal_overlay.exit_visual();
                s.input_mode = crate::app::InputMode::JournalOverlay;
                s.journal_overlay.set_journal_hint();
                crate::ui::toast::show_transient(&s.chapter_toast, "Copied", 2);
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            s.journal_overlay.exit_visual_to_anchor();
            s.input_mode = crate::app::InputMode::JournalOverlay;
            s.journal_overlay.set_journal_hint();
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
            // Read the origin before closing (close clears it), then run that
            // overlay's delete. close_delete_confirmation restores the origin mode.
            let origin = state.borrow().delete_confirm_origin;
            crate::input::actions::gloss::close_delete_confirmation(state);
            match origin {
                Some(crate::app::InputMode::JournalOverlay) => {
                    crate::input::actions::journal::delete_current(state);
                }
                _ => {
                    crate::input::actions::gloss::delete_current_gloss(state);
                }
            }
            true
        }
        "Escape" | "n" => {
            crate::input::actions::gloss::close_delete_confirmation(state);
            true
        }
        _ => true,
    }
}

fn handle_undo_confirm_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "y" => {
            // Read the origin before closing (close clears it), then run that
            // overlay's single-level undo. close_undo_confirmation has already
            // restored the originating overlay mode.
            let origin = state.borrow().undo_confirm_origin;
            crate::input::actions::gloss::close_undo_confirmation(state);
            match origin {
                Some(crate::app::InputMode::GlossOverlay) => {
                    crate::input::actions::gloss::undo_gloss_edit(state);
                }
                Some(crate::app::InputMode::SynopsisOverlay) => {
                    crate::input::actions::synopsis::undo_amend(state);
                }
                Some(crate::app::InputMode::JournalOverlay) => {
                    crate::input::actions::journal::undo_journal_edit(state);
                }
                _ => {}
            }
            true
        }
        "Escape" | "n" => {
            crate::input::actions::gloss::close_undo_confirmation(state);
            true
        }
        _ => true,
    }
}

/// The journal `R` target chooser: route a single key to one of the three
/// rewrite paths, tearing down the chooser box first (which restores the journal
/// overlay mode). `Esc` and any non-matching key just dismiss the chooser.
/// Mirrors the delete/undo confirm handlers' close-then-act order.
fn handle_rewrite_target_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "a" => {
            crate::input::actions::journal::close_rewrite_target(state);
            crate::input::actions::journal::begin_rewrite(state);
            true
        }
        "q" => {
            crate::input::actions::journal::close_rewrite_target(state);
            crate::input::actions::journal::rewrite_question_path(state, false);
            true
        }
        "b" => {
            crate::input::actions::journal::close_rewrite_target(state);
            crate::input::actions::journal::rewrite_question_path(state, true);
            true
        }
        "Escape" => {
            crate::input::actions::journal::close_rewrite_target(state);
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
            // Ctrl+g: Escape-only close policy — consumed no-op (was: close,
            // same as Escape).
            "g" => return true,
            // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the
            // only overlay-to-overlay navigation). Consumed no-op.
            "j" => return true,
            // Ctrl+Tab: dropped inside overlays (reader-side Ctrl+Tab still
            // reopens the last overlay). Consumed no-op (plain Tab is also a
            // dropped no-op now).
            "Tab" | "ISO_Left_Tab" => return true,
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
        // `a`: AB-loop the SOURCE turn's recorded media (moved off Tab, which
        // is dropped by request).
        "a" => {
            crate::input::actions::echoes::play_source_turn(state);
            true
        }
        // Tab/ISO_Left_Tab: dropped — consumed no-op so they don't leak.
        "Tab" | "ISO_Left_Tab" => true,
        // Space: play the SELECTED echo's recorded media.
        "space" => {
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
            // Escape: close the echoes overlay, clear the echo session + any turn
            // AB-loop, return to reader.
            crate::input::actions::echoes::close_echoes_to_reader(state);
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

/// Which per-overlay keybind legend is open, so the shared modal handler hides
/// the right widget and returns to the right parent overlay.
#[derive(Clone, Copy)]
enum OverlayLegend {
    Gloss,
    Synopsis,
    Journal,
}

/// Open the gloss/synopsis/journal Ctrl+/ keybind legend for `which`: show the
/// legend widget and switch to its modal InputMode. The open-side mirror of the
/// close path in `handle_overlay_keybinds_key` (audit #51), keyed by the same
/// `OverlayLegend` enum so the per-overlay field+mode mapping lives in ONE place.
fn open_overlay_legend(s: &mut AppState, which: OverlayLegend) {
    match which {
        OverlayLegend::Gloss => {
            s.gloss_keybinds_overlay.show();
            s.input_mode = crate::app::InputMode::GlossKeybindsOverlay;
        }
        OverlayLegend::Synopsis => {
            s.synopsis_keybinds_overlay.show();
            s.input_mode = crate::app::InputMode::SynopsisKeybindsOverlay;
        }
        OverlayLegend::Journal => {
            s.journal_keybinds_overlay.show();
            s.input_mode = crate::app::InputMode::JournalKeybindsOverlay;
        }
    }
}

/// Modal handler for the gloss/synopsis/journal Ctrl+/ keybind legends: Esc or
/// Ctrl+/ closes the legend and returns to its parent overlay; all other keys are
/// swallowed. Mirrors `handle_echo_keybinds_key`.
fn handle_overlay_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
    which: OverlayLegend,
) -> bool {
    if key_name == "Escape" || (is_ctrl && key_name == "slash") {
        let mut s = state.borrow_mut();
        match which {
            OverlayLegend::Gloss => {
                s.gloss_keybinds_overlay.hide();
                s.input_mode = crate::app::InputMode::GlossOverlay;
            }
            OverlayLegend::Synopsis => {
                s.synopsis_keybinds_overlay.hide();
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
            }
            OverlayLegend::Journal => {
                s.journal_keybinds_overlay.hide();
                s.input_mode = crate::app::InputMode::JournalOverlay;
            }
        }
    }
    true // consume all keys while the legend is up (modal)
}

/// Fully modal vocab-sentence loop keys: n/p step, a/Space toggles pause,
/// Escape or Ctrl+r exits. EVERYTHING else is swallowed (returns true) so
/// no reader bind can fire mid-drill.
fn handle_vocab_loop_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    match key_name {
        "n" if !is_ctrl => crate::input::vocab_loop::advance(state, true),
        "p" if !is_ctrl => crate::input::vocab_loop::advance(state, false),
        "a" | "space" if !is_ctrl => {
            let _ = state
                .borrow()
                .cmd_tx
                .try_send(crate::mpv::MpvCommand::TogglePause);
        }
        "Escape" => crate::input::vocab_loop::exit_vocab_loop(&mut state.borrow_mut()),
        // Exit on the loop's own entry keys (Ctrl+- forward, Ctrl+Shift+-
        // backward — shifted minus may arrive as "underscore"); Ctrl+r/R
        // kept as a legacy exit.
        "r" | "R" | "minus" | "underscore" if is_ctrl => {
            crate::input::vocab_loop::exit_vocab_loop(&mut state.borrow_mut())
        }
        _ => {}
    }
    true
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
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    match key_name {
        // Ctrl+a — open the Journal Q&A ask card for the selection directly
        // (skips the Action menu). Works for ask-entered AND V-entered
        // selections, so the menu is never required for Journal Q&A.
        "a" if is_ctrl => {
            crate::input::visual::action_journal_qa(state);
            true
        }
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
            // Ask-entered selection (Ctrl+a): Return is a direct confirm.
            // V-entered selection: Return opens the Action menu (unchanged).
            let pending_ask = state
                .borrow()
                .visual_selection
                .as_ref()
                .is_some_and(|s| s.pending_ask);
            if pending_ask {
                crate::input::visual::action_journal_qa(state);
            } else {
                crate::input::visual::open_action_popup(&mut state.borrow_mut());
            }
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

/// Locate the large-model whisperX transcript for a media file:
/// `<media dir>/whisperx-cache/<stem>.whisperX-transcript-large-v3*.json`
/// (the trailing `*` covers language suffixes like `.en`). Returns the first
/// match sorted by name, or None — smaller models (medium) never substitute.
fn find_large_whisperx_json(media_path: &str) -> Option<String> {
    let p = std::path::Path::new(media_path);
    let stem = p.file_stem()?.to_str()?;
    let cache = p.parent()?.join("whisperx-cache");
    let prefix = format!("{}.whisperX-transcript-large-v3", stem);
    let mut hits: Vec<String> = std::fs::read_dir(&cache)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".json")
        })
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    hits.sort();
    hits.into_iter().next()
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
    // Nav keybinds no longer flash the cursor paragraph — the karaoke tint
    // moving to the new segment's first phrase (seek_to_current_line) is the
    // nav cue. flash_reader_cursor remains for overlay-close re-orientation.
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
        BookmarkTap => {
            // Overloaded `.`: single tap toggles the bookmark and arms the
            // .. chord; the second quick tap reverts the toggle and opens
            // the picker (PendingPeriod check in handle_key_inner).
            crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle);
            KeyState::start_chord(key_state, ChordState::PendingPeriod);
        }
        ToggleChapterStart => crate::input::actions::chapters::toggle_chapter_start(state, tokio_handle),
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
            s.search_return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
            s.search_bar.show();
            s.input_mode = crate::app::InputMode::Search;
        }

        // MPV / media
        TogglePlaybackSync => toggle_playback_sync(&mut state.borrow_mut()),
        PlaybackSyncTap => {
            // Overloaded s: the single tap only TOASTS the sync state (an
            // accidental press must not silently kill sync); ss toggles
            // (PendingS check in handle_key_inner).
            {
                let s = state.borrow();
                let msg = if s.sync_enabled { "Sync: on (ss toggles)" } else { "Sync: off (ss toggles)" };
                navigation::show_chapter_toast(&s, msg);
            }
            KeyState::start_chord(key_state, ChordState::PendingS);
        }
        TogglePlaybackFromTimestamp => {
            crate::input::search::toggle_playback_from_timestamp(&mut state.borrow_mut())
        }
        // Pure pause/resume: no seek to the cursor line (unlike
        // TogglePlaybackFromTimestamp).
        TogglePause => {
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
        }
        SeekShortBackward => {
            // Sync + karaoke on: o steps by PHRASE (restart the current
            // phrase, or the previous one when near its start); otherwise
            // the raw ±seconds seek.
            if !crate::input::phrase_highlight::phrase_step_seek(&mut state.borrow_mut(), false) {
                do_mpv_seek(state, -3.5)
            }
        }
        SeekShortForward => {
            if !crate::input::phrase_highlight::phrase_step_seek(&mut state.borrow_mut(), true) {
                do_mpv_seek(state, 3.5)
            }
        }
        SeekLongBackward => do_mpv_seek(state, -60.0),
        SeekLongForward => do_mpv_seek(state, 60.0),
        SeekBackward30 => do_mpv_seek(state, -30.0),
        VolumeUp => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0)); }
        VolumeDown => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0)); }
        TogglePlaybackSpeed => {
            let mut s = state.borrow_mut();
            let new_speed = next_playback_speed(s.playback_speed);
            s.playback_speed = new_speed;
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetSpeed(new_speed));
            crate::logging::log(&format!("SPEED: cycled to {}x", new_speed));
            let label = format!("Speed: {:.1}x", new_speed);
            // Speed toast sits bottom-center (same place as the act/scene pill);
            // reset margins in case a prior "Sync:"/"Copied" moved the shared
            // toast to a corner.
            s.speed_toast.set_halign(gtk4::Align::Center);
            s.speed_toast.set_margin_start(0);
            s.speed_toast.set_margin_end(0);
            crate::ui::toast::show_transient(&s.speed_toast, &label, 3);
        }

        // Vocab / glossing
        ToggleVocabPopup => {
            let mut s = state.borrow_mut();
            s.vocab_popup.auto = !s.vocab_popup.auto;
            if s.vocab_popup.auto {
                crate::app::vocab_popup::open_vocab_popup(&mut s);
            } else {
                crate::app::vocab_popup::close_vocab_popup(&mut s);
            }
        }
        VocabPopupTap => {
            // Overloaded r: a single tap cycles the segment's words while
            // the popup is visible (same as Ctrl+r) and arms the rr chord;
            // the second quick tap toggles visibility (the PendingR check in
            // handle_key_inner consumes it before dispatch).
            if state.borrow().vocab_popup.popup.is_visible() {
                handle_vocab_popup_key(state, true);
            }
            KeyState::start_chord(key_state, ChordState::PendingR);
        }
        VocabPopupNext => {
            handle_vocab_popup_key(state, true);
        }
        VocabPopupPrev => {
            handle_vocab_popup_key(state, false);
        }
        HideVocabPopup => {
            fade_out_vocab_popup(state);
        }
        VocabJournalAsk => crate::input::actions::vocab_journal::vocab_journal_ask(state),
        VocabJournalPageNext => crate::input::actions::vocab_journal::vocab_journal_page(state, 1),
        VocabJournalPagePrev => crate::input::actions::vocab_journal::vocab_journal_page(state, -1),
        JumpToNextVocab => crate::input::actions::concordance::jump_to_next_vocab(state, tokio_handle),
        JumpToPrevVocab => crate::input::actions::concordance::jump_to_prev_vocab(state, tokio_handle),
        ConcordanceNext => crate::input::actions::concordance::concordance_next(state, tokio_handle),
        ConcordancePrev => crate::input::actions::concordance::concordance_prev(state, tokio_handle),
        TogglePhraseHighlight => {
            let mut s = state.borrow_mut();
            let is_prose = s.is_prose();
            let mode = if is_prose {
                s.config.phrase_highlight_prose = s.config.phrase_highlight_prose.cycle();
                s.config.phrase_highlight_prose
            } else {
                s.config.phrase_highlight_verse = s.config.phrase_highlight_verse.cycle();
                s.config.phrase_highlight_verse
            };
            crate::config::save(&s.config);
            // Clear on EVERY transition (not just Off) so a stale phrase-width
            // tint never lingers when entering LINE mode; the next TimePos
            // tick repaints at the new mode's width.
            crate::input::phrase_highlight::clear_phrase_highlight(&mut s);
            let text = format!(
                "Phrase highlight {} ({})",
                mode.label(),
                if is_prose { "prose" } else { "plays/poetry" },
            );
            crate::input::navigation::show_chapter_toast(&s, &text);
            crate::logging::log(&format!("PHRASE_HL: toggled {}", text));
        }
        ToggleVocabHighlight => {
            let mut s = state.borrow_mut();
            s.vocab_highlight_visible = !s.vocab_highlight_visible;
            if s.vocab_highlight_visible {
                crate::app::apply_vocab_highlighting(&s);
            } else {
                crate::app::remove_vocab_highlighting(&s);
            }
            // Persist per-work to lit.db (the column keyed by this work's abbrev),
            // not to config. Source of truth is now per-work. Log on failure so a
            // silent revert (locked/read-only DB) is greppable, not invisible.
            if let Some(abbrev) = s.current_work.as_ref().map(|w| w.abbrev.clone()) {
                match crate::db::queries::open_db_rw().and_then(|conn| {
                    crate::db::queries::set_vocab_highlight(&conn, &abbrev, s.vocab_highlight_visible)
                }) {
                    Ok(()) => {}
                    Err(e) => crate::logging::log(&format!("VOCAB: persist failed for {}: {}", abbrev, e)),
                }
            }
            crate::logging::log(&format!("VOCAB: highlighting {}", if s.vocab_highlight_visible { "on" } else { "off" }));
        }
        ToggleGlossOverlay => crate::input::actions::gloss::toggle_overlay(state),
        ToggleJournalOverlay => crate::input::actions::journal::toggle_overlay(state),
        ToggleLastOverlay => {
            let panel_open = state.borrow().chat_layout_open;
            if panel_open {
                crate::input::actions::chat::close_chat_layout(&mut state.borrow_mut());
            } else {
                crate::input::actions::gloss::toggle_last_overlay(state)
            }
        }
        CycleSegmentOverlays => crate::input::actions::overlay_cycle::cycle_from_reader(state),
        OpenJournalPicker => crate::input::actions::journal::open_picker_from_reader(state),
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
        AskPassage => crate::input::visual::enter_visual_block_mode(&mut state.borrow_mut()),
        WordCycleCopy => crate::input::actions::word_copy::word_cycle_copy(&mut state.borrow_mut()),
        WordCollectCopy => crate::input::actions::word_copy::word_collect_copy(&mut state.borrow_mut()),
        OpenSegmentVim => crate::input::actions::segment_vim::open(state),

        // Translations
        ToggleTranslations => {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            drop(s);
            crate::app::translations::toggle_translations(&mut state.borrow_mut());
        }

        // Settings (in reader)
        AdjustFontSizeUp => { crate::app::font::adjust_font_size(&mut state.borrow_mut(), 1); crate::app::font::show_font_info(&state.borrow()); }
        AdjustFontSizeDown => { crate::app::font::adjust_font_size(&mut state.borrow_mut(), -1); crate::app::font::show_font_info(&state.borrow()); }
        ResetFontSize => crate::app::font::reset_font_size(&mut state.borrow_mut()),
        CycleFontForward => crate::app::font::cycle_font(&mut state.borrow_mut(), true),
        CycleFontBackward => crate::app::font::cycle_font(&mut state.borrow_mut(), false),
        ToggleSignColumn => crate::app::toggle_sign_column(&mut state.borrow_mut()),
        ToggleColumnLayout => navigation::toggle_column_layout(&mut state.borrow_mut()),
        TogglePreviousWork => {
            crate::input::actions::pickers::toggle_previous_work(state, tokio_handle);
        }
        ThemeNext => crate::input::actions::settings::cycle_theme(state, true),
        ThemePrev => crate::input::actions::settings::cycle_theme(state, false),
        RootVariantNext => crate::input::actions::settings::cycle_root_variant(state, true),
        RootVariantPrev => crate::input::actions::settings::cycle_root_variant(state, false),
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
        ToggleChatLayout => crate::input::actions::chat::toggle_chat_layout(state),
        ChatPanelFlipSide => {
            crate::input::actions::chat::flip_panel_side(&mut state.borrow_mut())
        }
        CycleScansion => {
            let mut s = state.borrow_mut();
            // Populate the cache on first use (or for a freshly loaded work).
            if s.scansion.data.is_empty() {
                if let Some(work) = s.current_work.as_ref() {
                    let abbrev = work.abbrev.clone();
                    if let Ok(conn) = crate::db::queries::open_db() {
                        match crate::db::queries::load_scansion_for_work(&conn, &abbrev) {
                            Ok(map) => s.scansion.data = map,
                            Err(e) => crate::logging::log(&format!("SCANSION: load failed: {}", e)),
                        }
                    }
                }
            }
            if s.scansion.data.is_empty() {
                s.scansion.level = crate::scansion::ScanLevel::Off;
                // Reuse the chapter-toast widget for a transient reader message
                // (same pattern as show_chapter_toast in navigation.rs:1670).
                crate::ui::toast::show_transient(&s.chapter_toast, "No scansion for this work", 3);
                crate::logging::log("SCANSION: no scansion for this work");
                return;
            }
            s.scansion.level = s.scansion.level.next();
            s.config.scansion_level = s.scansion.level.as_str().to_string();
            crate::config::save(&s.config);
            crate::logging::log(&format!("SCANSION: level -> {:?}", s.scansion.level));
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
                crate::app::scene_synopsis::update_title_bar_scene(&s);
            }
            crate::config::save(&s.config);
        }
        ShowFontInfo => crate::app::font::show_font_info(&state.borrow()),
        ShowCurrentChapter => navigation::show_current_chapter(&mut state.borrow_mut()),

        // Timestamps
        SetStartTime => { crate::input::timestamps::set_start_time(&mut state.borrow_mut()); }
        SetEndTime => { crate::input::timestamps::set_end_time(&mut state.borrow_mut()); }
        SetChapter => { crate::input::timestamps::set_chapter(&mut state.borrow_mut()); }
        DeleteTimestamp => { crate::input::timestamps::delete_timestamp(&mut state.borrow_mut()); }
        DeleteTimestampTap => {
            // Overloaded BackSpace: the single tap only previews the line's
            // timestamp; the second quick tap deletes it (PendingBackspace
            // check in handle_key_inner).
            {
                let s = state.borrow();
                let ts = s.current_work.as_ref().and_then(|w| {
                    s.work_line_for_buffer(s.current_line)
                        .and_then(|wi| w.lines.get(wi))
                        .and_then(|l| l.timestamp.as_ref())
                        .map(|t| t.start)
                });
                let msg = match ts {
                    Some(t) => format!("ts {:.2}s (BackSpace again deletes)", t),
                    None => "no timestamp on this line".to_string(),
                };
                navigation::show_chapter_toast(&s, &msg);
            }
            KeyState::start_chord(key_state, ChordState::PendingBackspace);
        }
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
            crate::ui::toast::show_transient(&s.speed_toast, &format!("Copied {}", clip), 3);
        }
        CopyWorkInfo => {
            let s = state.borrow();
            let Some(work) = s.current_work.as_ref() else { return };
            let abbrev = work.abbrev.clone();
            // Active media: the path whose media_id matches the playing one,
            // else the work's primary media.
            let media_path = s
                .media_id
                .and_then(|mid| {
                    work.media_ids
                        .iter()
                        .position(|&m| m == mid)
                        .and_then(|i| work.media_paths.get(i))
                })
                .or_else(|| work.media_paths.first())
                .cloned();
            drop(s);
            let whisperx = media_path.as_deref().and_then(find_large_whisperx_json);
            let mut clip = abbrev.clone();
            if let Some(ref mp) = media_path {
                clip.push('\n');
                clip.push_str(mp);
            }
            if let Some(ref wx) = whisperx {
                clip.push('\n');
                clip.push_str(wx);
            }
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
            crate::logging::log(&format!("CLIPBOARD: copied work info {}", clip.replace('\n', " | ")));
            let msg = match (media_path.is_some(), whisperx.is_some()) {
                (true, true) => format!("Copied {} + media + whisperX", abbrev),
                (true, false) => format!("Copied {} + media (no large whisperX)", abbrev),
                (false, _) => format!("Copied {} (no media)", abbrev),
            };
            let s = state.borrow();
            s.speed_toast.set_halign(gtk4::Align::Center);
            s.speed_toast.set_margin_start(0);
            s.speed_toast.set_margin_end(0);
            crate::ui::toast::show_transient(&s.speed_toast, &msg, 3);
        }

        // Multi-key chord entry
        PendingG => {
            if state.borrow().vocab_popup.popup.is_visible() {
                crate::app::vocab_popup::vocab_popup_toggle_view(&mut state.borrow_mut());
            } else {
                KeyState::start_chord(key_state, ChordState::PendingG);
            }
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
        ToggleSynopsis => crate::app::scene_synopsis::toggle_synopsis(&mut state.borrow_mut()),
        ShowSynopsisOverlay => crate::app::scene_synopsis::show_synopsis_overlay(state),
        ShowTranslationOverlay => crate::app::translations::show_translation_overlay(state),
        ToggleImageView => crate::app::toggle_image_view(state),
        EnterPageCalibration => crate::app::enter_page_calibration(state),

        // Authorship display
        ToggleAuthorship => {
            let mut s = state.borrow_mut();
            if s.authorship_sets.is_empty() {
                crate::ui::toast::show_transient(&s.chapter_toast, "No authorship data for this work", 3);
                return;
            }
            s.authorship_enabled = !s.authorship_enabled;
            crate::app::formatting::apply_authorship_formatting(&mut s);
            let label = if s.authorship_enabled { "Authorship: on" } else { "Authorship: off" };
            crate::ui::toast::show_transient(&s.chapter_toast, label, 3);
        }
        PickAttributionSet => {
            let s = state.borrow();
            if s.authorship_sets.is_empty() {
                crate::ui::toast::show_transient(&s.chapter_toast, "No authorship data for this work", 3);
                return;
            }
            if s.authorship_sets.len() == 1 {
                crate::ui::toast::show_transient(&s.chapter_toast, "Only one attribution set available", 3);
                return;
            }
            drop(s);
            crate::input::actions::authorship::open_attribution_picker(state);
        }
    }

}

/// Playback-speed cycle for TogglePlaybackSpeed (keyboard and gamepad):
/// 1.0 → 1.3 → 0.9 → 1.0. Any off-cycle value snaps back to 1.0.
pub(crate) fn next_playback_speed(current: f64) -> f64 {
    if current == 1.0 {
        1.3
    } else if current == 1.3 {
        0.9
    } else {
        1.0
    }
}

/// MPV seek with brief sync suppression. Common pattern for o/e/O/E/Left.
/// With karaoke on, the phrase at the seek target is tinted immediately so
/// the highlight tracks the timecode sent to MPV; the paint is held through
/// the suppression window (post-seek TimePos ticks would otherwise clear it).
fn do_mpv_seek(state: &Rc<RefCell<AppState>>, offset: f64) {
    let mut s = state.borrow_mut();
    let target = (s.current_time_pos + offset).max(0.0);
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SeekRelative(offset));
    s.suppress_sync_until =
        Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
    if crate::input::phrase_highlight::paint_pending_phrase(&mut s, target) {
        s.phrase_paint_hold = s.suppress_sync_until;
    }
}

/// Vocab popup key handler (Ctrl+r cycles the segment's vocab words). The
/// popup is STICKY and FOLLOWS the cursor and playback line (same
/// `vocab_popup.auto` follow hook the r/Shift+H visibility toggles use — see
/// `update_highlight` in src/input/highlight.rs): it stays up, tracking the
/// current line, until a visibility toggle dismisses it. The old 3-second
/// auto-hide timer is gone by design.
fn handle_vocab_popup_key(state: &Rc<RefCell<AppState>>, forward: bool) {
    let popup_visible = state.borrow().vocab_popup.popup.is_visible();
    if popup_visible {
        if forward {
            crate::app::vocab_popup::vocab_popup_next(&mut state.borrow_mut());
        } else {
            crate::app::vocab_popup::vocab_popup_prev(&mut state.borrow_mut());
        }
    } else {
        crate::app::vocab_popup::open_vocab_popup(&mut state.borrow_mut());
    }
    let mut s = state.borrow_mut();
    // A popup opened (or cycled) from the keyboard follows the cursor and
    // playback line exactly like Shift+H's auto mode.
    s.vocab_popup.auto = true;
    // Invalidate any pending fade (defensive; nothing arms one anymore).
    s.vocab_popup.fade_gen.set(s.vocab_popup.fade_gen.get() + 1);
}

/// Action::HideVocabPopup (unbound by default; kept for user keymaps): fade
/// the vocab popup out (500ms, EaseOutQuad — the same animation the old
/// auto-hide used). Idempotent: no-op when the popup isn't visible. Also
/// clears the follow flag — leaving `auto` set would let the highlight hook
/// reopen the popup on the very next cursor move.
fn fade_out_vocab_popup(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.vocab_popup.auto = false;
    s.vocab_popup.fade_gen.set(s.vocab_popup.fade_gen.get() + 1);
    let s = &*s;
    if !s.vocab_popup.popup.is_visible() {
        return;
    }
    let widget = s.vocab_popup.popup.widget().clone();
    let target = adw::CallbackAnimationTarget::new(move |value| {
        widget.set_opacity(value as f64);
        if value <= 0.0 {
            widget.set_visible(false);
            widget.set_opacity(1.0);
        }
    });
    let anim = adw::TimedAnimation::new(
        s.vocab_popup.popup.widget(),
        1.0, 0.0, 500, target,
    );
    anim.set_easing(adw::Easing::EaseOutQuad);
    anim.play();
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

