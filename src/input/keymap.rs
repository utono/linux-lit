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
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
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
            let cmd_tx = state.borrow().cmd_tx.clone();
            let _ = cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
            true
        }
        _ => false,
    }
}
