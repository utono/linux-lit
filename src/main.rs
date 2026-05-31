mod ab_repeat;
mod app;
mod concordance;
mod config;
mod db;
mod gutter;
mod input;
mod logging;
mod mode;
mod claude;
mod gloss;
mod voyage;
mod mpv;
mod snapshot;
mod text_file_map;
mod theme;
mod ui;

use gtk4::prelude::*;
use libadwaita as adw;
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
    crate::logging::log("STARTUP: main entry");

    // Handle --clear-cache: delete all snapshot files and exit immediately.
    // Maintenance command; doesn't proceed to launch the window.
    if std::env::args().any(|a| a == "--clear-cache") {
        match snapshot::delete_all() {
            Ok(()) => {
                println!("Cleared snapshot cache.");
                crate::logging::log("STARTUP: --clear-cache invoked; cache cleared; exiting");
            }
            Err(e) => {
                eprintln!("Failed to clear snapshot cache: {}", e);
                crate::logging::log(&format!("STARTUP: --clear-cache failed: {}", e));
                std::process::exit(1);
            }
        }
        return;
    }

    let app_id = if mode::is_dev_mode() {
        "com.utono.linux-lit.dev"
    } else {
        "com.utono.linux-lit"
    };

    adw::init().expect("Failed to initialize libadwaita");

    let application = gtk4::Application::builder()
        .application_id(app_id)
        .build();

    application.connect_activate(|gtk_app| {
        crate::logging::log("STARTUP: connect_activate fired");
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

        // Start background reader for the 8BitDo Micro gamepad (no-op if absent).
        crate::input::gamepad::spawn(std::rc::Rc::clone(&state));

        // Process MPV events — CursorSync updates cursor position
        let state_for_events = std::rc::Rc::clone(&state);
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                match event {
                    MpvEvent::CursorSync(line_idx) => {
                        let mut s = state_for_events.borrow_mut();
                        if s.loading_work.get() {
                            crate::logging::log(&format!("CURSOR_SYNC: SKIP loading_work line_idx={}", line_idx));
                            continue;
                        }
                        if !s.sync_enabled {
                            continue;
                        }
                        if !s.search_matches.is_empty() {
                            continue;
                        }
                        // During chunk mode, don't let CursorSync move the view
                        if s.ab_repeat.chunk_index.is_some() {
                            continue;
                        }
                        if let Some(until) = s.suppress_sync_until {
                            if std::time::Instant::now() < until {
                                crate::logging::log(&format!(
                                    "CURSOR_SYNC: SUPPRESSED line_idx={} remaining={:.1}s",
                                    line_idx,
                                    until.duration_since(std::time::Instant::now()).as_secs_f64()
                                ));
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
                        // Guard against aberrant timestamps: if the sync target
                        // is far from the current position, skip it. Also skip
                        // if current_line has no work mapping (e.g. on a stage
                        // direction after a bad resume).
                        let cur_wi = s.work_line_for_buffer(s.current_line);
                        if let Some(cwi) = cur_wi {
                            let dist = (line_idx as isize - cwi as isize).unsigned_abs();
                            if dist > 50 {
                                crate::logging::log(&format!(
                                    "CURSOR_SYNC: ABERRANT line_idx={} cur_work={} dist={} — skipped",
                                    line_idx, cwi, dist,
                                ));
                                continue;
                            }
                        } else {
                            crate::logging::log(&format!(
                                "CURSOR_SYNC: SKIP no work mapping for current_line={}", s.current_line
                            ));
                            continue;
                        }
                        // After a pending_advance, ignore CursorSync that would
                        // pull the cursor back to the source timestamped line.
                        if s.pending_advance_ignore_bl == Some(buffer_line) {
                            continue;
                        }
                        // CursorSync targets a different line — clear the guard
                        s.pending_advance_ignore_bl = None;

                        if s.current_line != buffer_line {
                            crate::logging::log_always(&format!(
                                "CURSOR_SYNC: line_idx={} buffer_line={} current={} page_top={} translations_visible={} buf_lines={}",
                                line_idx,
                                buffer_line,
                                s.current_line,
                                s.page_top_line,
                                s.translations_visible,
                                s.buffer.line_count(),
                            ));
                            s.current_line = buffer_line;

                            // Detect scene transition (plays only): when
                            // playback crosses into a new scene, snap the
                            // viewport so the scene header is at the top.
                            let mut scene_scrolled = false;
                            if let Some(ref work) = s.current_work {
                                if !crate::db::line_types::is_prose_work(&work.work_type) {
                                    let wi = s.work_line_for_buffer(buffer_line).unwrap_or(line_idx);
                                    if let Some(line) = work.lines.get(wi) {
                                        let scene = (line.div1, line.div2);
                                        let old_scene = s.current_sync_scene;
                                        s.current_sync_scene = Some(scene);
                                        if old_scene.is_some() && old_scene != Some(scene) {
                                            let top = crate::input::viewport::scene_header_top(
                                                &s.buffer, buffer_line,
                                            );
                                            if top != s.page_top_line {
                                                crate::logging::log_always(&format!(
                                                    "SYNC_SCENE_SCROLL: {:?}->{:?} top={} current={} page_top={}",
                                                    old_scene, scene, top, buffer_line, s.page_top_line
                                                ));
                                                crate::input::scroll::set_page_instant(&mut s, top);
                                                scene_scrolled = true;
                                            }
                                        }
                                    }
                                }
                            }

                            // Detect paragraph transition before scrolling:
                            // when playback crosses into a new paragraph, scroll
                            // so its first line lands at the cursor position.
                            let para = s.current_paragraph_range();
                            let old_para_start = s.current_paragraph_start;
                            s.current_paragraph_start = Some(para.start);
                            let paragraph_changed = old_para_start.is_some()
                                && old_para_start != Some(para.start);

                            if paragraph_changed && !scene_scrolled {
                                crate::logging::log_always(&format!(
                                    "SYNC_PARA_SCROLL: para_start={} page_top={} on_screen={}",
                                    para.start, s.page_top_line, crate::input::navigation::is_line_on_screen(&s, para.start)
                                ));
                                crate::input::navigation::scroll_paragraph_to_top(
                                    &mut s, para.start,
                                );
                            }
                            // Highlight + advance page if current line is past last visible.
                            // This runs for both paragraph-change (where scroll_paragraph_to_top
                            // may have skipped the turn because para_start was on-screen) and
                            // normal line-by-line sync.
                            crate::input::navigation::update_highlight_and_advance_page(
                                &mut s,
                            );
                            crate::input::navigation::after_page_change(
                                &mut s,
                                crate::input::navigation::PageChangeReason::MpvSync,
                            );

                            if paragraph_changed {
                                crate::app::refresh_vocab_popup(&mut s);
                            }
                        }
                        // Schedule a pending_advance when the current line's
                        // audio ends and the next dialogue line is
                        // untimestamped. Scene boundaries are handled
                        // naturally by CursorSync's scene-transition
                        // detection — no pending_advance needed.
                        s.pending_advance = None;
                        if let Some(ref work) = s.current_work {
                            if let Some(ts) = work.lines.get(line_idx).and_then(|l| l.timestamp.as_ref()) {
                                let next_dialogue = if let Some(ref lm) = s.line_map {
                                    lm.dialogue_buffer_lines.iter().find(|&&bl| bl > buffer_line).copied()
                                } else {
                                    let lc = work.lines.len();
                                    ((buffer_line + 1)..lc).find(|&i| work.lines[i].is_dialogue)
                                };
                                if let Some(next_bl) = next_dialogue {
                                    let next_wi = s.work_line_for_buffer(next_bl);
                                    let next_has_ts = next_wi.and_then(|wi| work.lines[wi].timestamp.as_ref()).is_some();
                                    if !next_has_ts {
                                        let end_time = if ts.end > ts.start {
                                            ts.end
                                        } else {
                                            let next_start = next_wi
                                                .and_then(|wi| work.lines[wi].timestamp.as_ref())
                                                .map(|nts| (nts.start - 0.2).max(ts.start));
                                            next_start.unwrap_or(ts.start + 5.0)
                                        };
                                        s.pending_advance = Some((end_time, next_bl, line_idx));
                                    }
                                }
                            }
                        }
                    }
                    MpvEvent::ConnectionStatus(connected) => {
                        state_for_events.borrow_mut().mpv_connected = connected;
                        crate::logging::log(&format!("MPV connection: {}", connected));
                    }
                    MpvEvent::PlaybackState(playing) => {
                        state_for_events.borrow_mut().mpv_playing = playing;
                        crate::logging::log(&format!(
                            "MPV playback: {}",
                            if playing { "playing" } else { "paused" }
                        ));
                    }
                    MpvEvent::TimePos(pos) => {
                        let mut s = state_for_events.borrow_mut();
                        s.current_time_pos = pos;

                        // Advance to untimestamped next line when current line's audio ends
                        if s.sync_enabled && !s.loading_work.get() {
                            if let Some((end_time, next_bl, _source_wi)) = s.pending_advance {
                                if pos >= end_time {
                                    s.pending_advance_ignore_bl = Some(s.current_line);
                                    s.pending_advance = None;
                                    if s.current_line != next_bl {
                                        s.current_line = next_bl;
                                        crate::input::navigation::update_highlight_and_advance_page(
                                            &mut s,
                                        );
                                    }
                                }
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
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &theme);
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
