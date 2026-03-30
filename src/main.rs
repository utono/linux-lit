mod ab_repeat;
mod app;
mod concordance;
mod config;
mod db;
mod gutter;
mod input;
mod logging;
mod mode;
mod ollama;
mod mpv;
mod text_file_map;
mod theme;
mod ui;

use gtk4::prelude::*;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    // Clear and set up log file
    let home = std::env::var("HOME").unwrap_or_default();
    let log_filename = if mode::is_dev_mode() {
        "linux-lit-dev.log"
    } else {
        "linux-lit-release.log"
    };
    let log_path = format!("{}/utono/linux-lit/{}", home, log_filename);
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);

    let app_id = if mode::is_dev_mode() {
        "com.utono.linux-lit.dev"
    } else {
        "com.utono.linux-lit"
    };

    let application = gtk4::Application::builder()
        .application_id(app_id)
        .build();

    application.connect_activate(|gtk_app| {
        // Channel: GTK → Tokio (commands)
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        // Channel: Tokio → GTK (events)
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel::<MpvEvent>(32);

        // Create Tokio runtime, clone handle for GTK thread, move runtime to background thread
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        let tokio_handle = rt.handle().clone();
        std::thread::spawn(move || {
            rt.block_on(async move {
                // SIGUSR1 listener for external theme changes
                let signal_evt_tx = evt_tx.clone();
                tokio::spawn(async move {
                    let mut sig = tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::user_defined1(),
                    )
                    .expect("Failed to register SIGUSR1 handler");
                    loop {
                        sig.recv().await;
                        let _ = signal_evt_tx.send(MpvEvent::ThemeChanged).await;
                    }
                });

                crate::mpv::client::run(cmd_rx, evt_tx).await;
            });
        });

        // Load works list from database (blocking is OK during startup — 133 works, sub-ms)
        let works = {
            let conn = db::queries::open_db().expect("Failed to open lit.db");
            db::queries::list_works(&conn).expect("Failed to list works")
        };

        // Load config
        let config = config::load();

        // Build the window with works list, Tokio handle, and config
        let state = app::build_window(gtk_app, works, tokio_handle, config, cmd_tx);

        // Process MPV events — CursorSync updates cursor position
        let state_for_events = std::rc::Rc::clone(&state);
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                match event {
                    MpvEvent::CursorSync(line_idx) => {
                        let mut s = state_for_events.borrow_mut();
                        if !s.search_matches.is_empty() {
                            continue;
                        }
                        // During chunk mode, don't let CursorSync move the view
                        if s.ab_repeat.chunk_index.is_some() {
                            continue;
                        }
                        if let Some(until) = s.suppress_sync_until {
                            if std::time::Instant::now() < until {
                                continue;
                            }
                            s.suppress_sync_until = None;
                        }
                        // Translate work-line index to buffer-line index if line_map present
                        let buffer_line = if let Some(ref lm) = s.line_map {
                            if line_idx < lm.work_to_buffer.len() {
                                let bl = lm.work_to_buffer[line_idx];
                                // Verify this is a real mapping (unmatched work lines default to 0)
                                if lm.buffer_to_work.get(bl) != Some(&Some(line_idx)) {
                                    continue;
                                }
                                bl
                            } else {
                                continue;
                            }
                        } else {
                            line_idx
                        };
                        if s.current_line != buffer_line {
                            s.current_line = buffer_line;

                            // Check if we're in prose sentence-group mode
                            let new_sg = s.line_map.as_ref().and_then(|lm| {
                                if lm.sentence_groups.is_empty() {
                                    return None;
                                }
                                crate::text_file_map::sentence_group_index(
                                    &lm.sentence_groups,
                                    buffer_line,
                                )
                            });

                            if new_sg.is_some() {
                                let old_sg = s.current_sentence_group;
                                s.current_sentence_group = new_sg;

                                // Always update highlight (sentence undimming)
                                crate::input::navigation::update_highlight_only(&mut s);

                                if old_sg != new_sg {
                                    // Sentence changed — schedule scroll for 0.2s before
                                    // the next sentence's start_time
                                    let scroll_target = buffer_line;
                                    let next_sg_start_time = new_sg.and_then(|sg_idx| {
                                        let lm = s.line_map.as_ref()?;
                                        let group = lm.sentence_groups.get(sg_idx)?;
                                        // Find the start_time of the first line in this group
                                        let work = s.current_work.as_ref()?;
                                        let wi = s.work_line_for_buffer(group.line_range.start)?;
                                        work.lines.get(wi)?.timestamp.as_ref().map(|ts| ts.start)
                                    });
                                    if let Some(start_time) = next_sg_start_time {
                                        let threshold = (start_time - 0.2).max(0.0);
                                        if s.current_time_pos >= threshold {
                                            // Already past threshold, scroll now
                                            crate::input::navigation::ensure_visible_no_highlight(&mut s);
                                        } else {
                                            s.pending_sentence_scroll =
                                                Some((threshold, scroll_target));
                                        }
                                    } else {
                                        // No timestamp info, scroll immediately
                                        crate::input::navigation::ensure_visible_no_highlight(&mut s);
                                    }
                                }
                            } else {
                                s.current_sentence_group = None;
                                crate::input::navigation::update_highlight_and_ensure_visible(
                                    &mut s,
                                );
                            }

                            crate::app::refresh_vocab_popup(&mut s);
                            s.config.last_line = buffer_line;
                            crate::config::save(&s.config);
                        }
                        // Check if the next dialogue line lacks a timestamp;
                        // if so, schedule an advance when the current line's audio ends.
                        s.pending_advance = None;
                        if let Some(ref work) = s.current_work {
                            if let Some(end_time) = work.lines.get(line_idx).and_then(|l| l.timestamp.as_ref()).map(|ts| ts.end) {
                                // Find next dialogue buffer line
                                let next_dialogue = if let Some(ref lm) = s.line_map {
                                    lm.dialogue_buffer_lines.iter().find(|&&bl| bl > buffer_line).copied()
                                } else {
                                    let lc = work.lines.len();
                                    ((buffer_line + 1)..lc).find(|&i| work.lines[i].is_dialogue)
                                };
                                if let Some(next_bl) = next_dialogue {
                                    // Check if the next dialogue line is untimestamped
                                    let next_wi = s.work_line_for_buffer(next_bl);
                                    let next_has_ts = next_wi.and_then(|wi| work.lines[wi].timestamp.as_ref()).is_some();
                                    if !next_has_ts {
                                        s.pending_advance = Some((end_time, next_bl));
                                    }
                                }
                            }
                        }
                    }
                    MpvEvent::ConnectionStatus(connected) => {
                        crate::logging::log(&format!("MPV connection: {}", connected));
                    }
                    MpvEvent::PlaybackState(playing) => {
                        crate::logging::log(&format!(
                            "MPV playback: {}",
                            if playing { "playing" } else { "paused" }
                        ));
                    }
                    MpvEvent::TimePos(pos) => {
                        let mut s = state_for_events.borrow_mut();
                        s.current_time_pos = pos;
                        // Advance to untimestamped next line when current line's audio ends
                        if let Some((end_time, next_bl)) = s.pending_advance {
                            if pos >= end_time {
                                s.pending_advance = None;
                                if s.current_line != next_bl {
                                    s.current_line = next_bl;
                                    // Suppress sync so cursor stays on this untimestamped line
                                    s.suppress_sync_until = Some(
                                        std::time::Instant::now() + std::time::Duration::from_secs(86400),
                                    );
                                    crate::input::navigation::update_highlight_and_ensure_visible(
                                        &mut s,
                                    );
                                    crate::app::refresh_vocab_popup(&mut s);
                                    s.config.last_line = next_bl;
                                    crate::config::save(&s.config);
                                }
                            }
                        }
                        // Deferred scroll for prose sentence mode
                        if let Some((threshold, _)) = s.pending_sentence_scroll {
                            if pos >= threshold {
                                s.pending_sentence_scroll = None;
                                crate::input::navigation::ensure_visible_no_highlight(&mut s);
                            }
                        }
                    }
                    MpvEvent::ThemeChanged => {
                        let mut s = state_for_events.borrow_mut();
                        let theme_name = crate::theme::current_theme_name();
                        let theme = if theme_name.is_empty() {
                            crate::theme::load_theme("gruvbox-material")
                        } else {
                            crate::theme::load_theme(&theme_name)
                        };
                        crate::input::keymap::apply_theme_to_state(&mut s, &theme);
                    }
                }
            }
        });

        // cmd_tx is stored in AppState — no need for std::mem::forget
        let _ = state;
    });

    application.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_blocking_via_handle() {
        // Reproduce the exact runtime pattern used in connect_activate:
        // Runtime on background thread, handle used from another thread for spawn_blocking
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        std::thread::spawn(move || {
            rt.block_on(async move { while let Some(_cmd) = cmd_rx.recv().await {} });
        });

        // spawn_blocking from outside the runtime, using the handle
        let result = handle.block_on(async {
            handle
                .spawn_blocking(|| {
                    let conn = db::queries::open_db().unwrap();
                    db::queries::load_work(&conn, "Ham").unwrap()
                })
                .await
                .unwrap()
        });

        assert_eq!(result.title, "Hamlet");
        assert!(result.lines.len() > 4000);

        // Clean up: drop cmd_tx so runtime shuts down
        drop(cmd_tx);
    }
}
