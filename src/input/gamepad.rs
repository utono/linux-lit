//! 8BitDo Micro gamepad support.
//!
//! Reads BTN_*/ABS_* events from the evdev node in a background thread and
//! dispatches linux-lit actions on the GTK main thread. The D-pad is
//! reported as ABS_X/ABS_Y (0 = left/up, 127 = center, 255 = right/down).

use std::cell::RefCell;
use std::rc::Rc;

use evdev::{AbsoluteAxisType, Device, InputEventKind, Key};
use tokio::sync::mpsc;

use crate::app::AppState;
use crate::input::navigation;

#[derive(Debug, Clone, Copy)]
pub enum GamepadAction {
    PlayCurrentLine,
    NextDialogue,
    PrevDialogue,
    TogglePlayback,
    SetStartTime,
    ToggleChapter,
    TogglePlaybackSpeed,
    ToggleTranslations,
    SeekBackwardShort,
    PrevChapter,
    NextChapter,
}

const DEVICE_NAME: &str = "8BitDo Micro gamepad";
const AXIS_LOW: i32 = 64;
const AXIS_HIGH: i32 = 192;

fn find_device() -> Option<Device> {
    for (path, dev) in evdev::enumerate() {
        if dev.name().map(|n| n == DEVICE_NAME).unwrap_or(false) {
            crate::logging::log(&format!("GAMEPAD: found at {:?}", path));
            return Some(dev);
        }
    }
    None
}

/// Spawn the reader thread. Events are forwarded to the GTK main loop via a
/// tokio mpsc channel read inside a local future.
pub fn spawn(state: Rc<RefCell<AppState>>) {
    let (tx, mut rx) = mpsc::channel::<GamepadAction>(32);

    std::thread::spawn(move || loop {
        let Some(mut device) = find_device() else {
            crate::logging::log("GAMEPAD: device not found; retrying in 5s");
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        };

        let mut x_state: i8 = 0;
        let mut y_state: i8 = 0;

        loop {
            let events = match device.fetch_events() {
                Ok(e) => e,
                Err(err) => {
                    crate::logging::log(&format!(
                        "GAMEPAD: fetch_events error ({err}); reopening"
                    ));
                    break;
                }
            };

            for ev in events {
                match ev.kind() {
                    InputEventKind::Key(key) if ev.value() == 1 => {
                        if let Some(action) = key_to_action(key) {
                            let _ = tx.blocking_send(action);
                        }
                    }
                    InputEventKind::AbsAxis(axis) => {
                        let v = ev.value();
                        match axis {
                            AbsoluteAxisType::ABS_X => {
                                let new_state = axis_state(v);
                                if new_state != x_state {
                                    if new_state < 0 {
                                        let _ = tx.blocking_send(GamepadAction::SeekBackwardShort);
                                    } else if new_state > 0 {
                                        let _ = tx.blocking_send(GamepadAction::SetStartTime);
                                    }
                                    x_state = new_state;
                                }
                            }
                            AbsoluteAxisType::ABS_Y => {
                                let new_state = axis_state(v);
                                if new_state != y_state {
                                    if new_state < 0 {
                                        let _ = tx.blocking_send(GamepadAction::TogglePlaybackSpeed);
                                    } else if new_state > 0 {
                                        let _ = tx.blocking_send(GamepadAction::ToggleTranslations);
                                    }
                                    y_state = new_state;
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    glib::spawn_future_local(async move {
        while let Some(action) = rx.recv().await {
            dispatch(&state, action);
        }
    });
}

fn axis_state(v: i32) -> i8 {
    if v <= AXIS_LOW {
        -1
    } else if v >= AXIS_HIGH {
        1
    } else {
        0
    }
}

fn key_to_action(key: Key) -> Option<GamepadAction> {
    Some(match key {
        Key::BTN_SOUTH => GamepadAction::ToggleChapter,
        Key::BTN_EAST => GamepadAction::NextDialogue,
        Key::BTN_NORTH => GamepadAction::PrevDialogue,
        Key::BTN_WEST => GamepadAction::SetStartTime,
        Key::BTN_SELECT => GamepadAction::PlayCurrentLine,
        Key::BTN_START => GamepadAction::PrevChapter,
        Key::BTN_MODE => GamepadAction::NextChapter,
        _ => return None,
    })
}

fn dispatch(state: &Rc<RefCell<AppState>>, action: GamepadAction) {
    crate::logging::log(&format!("GAMEPAD: action={:?}", action));
    match action {
        GamepadAction::PlayCurrentLine => {
            crate::input::timestamps::play_current_line(&mut state.borrow_mut());
        }
        GamepadAction::NextDialogue => {
            navigation::jump_to_next_dialogue(&mut state.borrow_mut());
        }
        GamepadAction::PrevDialogue => {
            navigation::jump_to_prev_dialogue(&mut state.borrow_mut());
        }
        GamepadAction::TogglePlayback => {
            crate::input::search::toggle_playback(&mut state.borrow_mut());
        }
        GamepadAction::SetStartTime => {
            let ok = crate::input::timestamps::set_start_time(&mut state.borrow_mut());
            if ok {
                navigation::cursor_next_dialogue(&mut state.borrow_mut());
            }
        }
        GamepadAction::ToggleChapter => {
            let _ = crate::input::timestamps::set_chapter(&mut state.borrow_mut());
        }
        GamepadAction::TogglePlaybackSpeed => {
            let mut s = state.borrow_mut();
            let new_speed = if s.playback_speed == 1.0 { 1.3 } else { 1.0 };
            s.playback_speed = new_speed;
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetSpeed(new_speed));
            crate::logging::log(&format!("SPEED: toggled to {}x", new_speed));
        }
        GamepadAction::ToggleTranslations => {
            crate::app::toggle_translations(&mut state.borrow_mut());
        }
        GamepadAction::SeekBackwardShort => {
            let mut s = state.borrow_mut();
            let _ = s
                .cmd_tx
                .try_send(crate::mpv::MpvCommand::SeekRelative(-3.5));
            s.suppress_sync_until = Some(
                std::time::Instant::now() + std::time::Duration::from_millis(500),
            );
        }
        GamepadAction::PrevChapter => {
            navigation::jump_to_prev_chapter(&mut state.borrow_mut());
        }
        GamepadAction::NextChapter => {
            navigation::jump_to_next_chapter(&mut state.borrow_mut());
        }
    }
}
