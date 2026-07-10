mod ab_repeat;
mod app;
mod concordance;
mod config;
mod db;
mod gutter;
mod input;
mod instance;
mod logging;
mod mode;
mod claude;
mod elevenlabs;
mod gloss;
mod voyage;
mod mpv;
mod snapshot;
mod text_file_map;
mod theme;
mod tts;
mod ui;
mod scansion;

use gtk4::prelude::*;
use libadwaita as adw;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    // Clear and set up log file
    let home = std::env::var("HOME").unwrap_or_default();
    // LIT_LOG_PATH lets an isolated run (e.g. the headless nav-fuzz) write to its
    // own log instead of clobbering a live `cargo run` session's dev log.
    let log_path = if let Ok(p) = std::env::var("LIT_LOG_PATH") {
        p
    } else {
        let log_filename = if mode::is_dev_mode() {
            "linux-lit-dev.log"
        } else {
            "linux-lit-release.log"
        };
        format!("{}/utono/linux-lit/{}", home, log_filename)
    };
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
            let conn = db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
            db::queries::list_works(&conn).expect("Failed to list works")
        };

        // Load config
        let config = config::load();

        // Apply the configured startup volumes. The system sink is set once via
        // pactl (skipped under LIT_HEADLESS_TEST so isolated UI test runs never
        // touch the live session's audio sink); MPV's launch volume is recorded
        // for launch_mpv to read. Both default to the historical 70/100; change
        // them with the `set-startup-volume` skill (edits config JSON).
        if std::env::var("LIT_HEADLESS_TEST").is_err() {
            let pct = format!("{}%", config.system_volume);
            match std::process::Command::new("pactl")
                .args(["set-sink-volume", "@DEFAULT_SINK@", &pct])
                .spawn()
            {
                Ok(_) => crate::logging::log(&format!("STARTUP: set system volume to {}", pct)),
                Err(e) => crate::logging::log(&format!("STARTUP: failed to set volume: {}", e)),
            }
        }
        crate::mpv::discovery::set_mpv_volume(config.mpv_volume);

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
                        // The vocab loop pins the cursor; sync may not move it.
                        if s.vocab_loop.is_some() {
                            continue;
                        }
                        if !s.sync_enabled {
                            continue;
                        }
                        // Search matches present: normally we freeze the cursor
                        // so n/N navigation isn't disturbed by sync. But if
                        // playback is PLAYING (e.g. the user kept playing after a
                        // search submit / n / N), let sync track the audio so the
                        // highlight follows along — the whole point of seeking to
                        // the match while playing.
                        if !s.search_matches.is_empty() && !s.mpv_playing {
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
                        let cur_wi = s.work_line_for_buffer(s.current_line);
                        if let Some(cwi) = cur_wi {
                            // Task 1 makes CursorSync monotonic in citation order, so the old
                            // dist>50 magic guard is no longer load-bearing. Keep only a defensive
                            // clamp against a single event jumping more than the whole work (corrupt
                            // index), which should never happen.
                            let work_len = s.current_work.as_ref().map(|w| w.lines.len()).unwrap_or(usize::MAX);
                            let dist = (line_idx as isize - cwi as isize).unsigned_abs();
                            if dist >= work_len {
                                crate::logging::log(&format!(
                                    "CURSOR_SYNC: INSANE line_idx={} cur_work={} dist={} work_len={} — skipped",
                                    line_idx, cwi, dist, work_len,
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

                        // Capture the overlay's scene BEFORE the cursor moves so
                        // sync_translation_overlay (end of arm) can detect a
                        // cross-scene move and rebuild rather than just re-highlight.
                        let ov_scene_before = crate::app::scene_synopsis::current_scene_divs(&s);

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
                                        // Skip the scene snap when the new scene
                                        // is ALREADY visible on the current spread
                                        // (e.g. in the right column because it's
                                        // the work's last content): snapping would
                                        // re-page and empty the right column.
                                        let already_visible =
                                            crate::input::viewport::is_line_fully_visible(
                                                &s, buffer_line,
                                            );
                                        if old_scene.is_some()
                                            && old_scene != Some(scene)
                                            && !already_visible
                                        {
                                            // Table mode: the stored page containing
                                            // the scene's first spoken line IS the
                                            // scene page (the generator clamps at
                                            // section breaks and the loader restores
                                            // the heading chrome) — same grid as x/y.
                                            let mut top = crate::input::page_table::table_top_for(
                                                &s, buffer_line,
                                            )
                                            .unwrap_or_else(|| {
                                                crate::input::viewport::scene_header_top_state(
                                                    &s, buffer_line,
                                                )
                                            });
                                            // If the scene-header spread would leave
                                            // the right column empty (lone EPILOGUE),
                                            // redirect to the final-spread anchor so
                                            // the tail fills both columns. Mirrors
                                            // the q/j forward-nav rule.
                                            if s.column_count() == 2
                                                && crate::input::viewport::would_empty_right_column(&s, top)
                                            {
                                                top = crate::input::navigation::last_page_top(&s);
                                            }
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
                                crate::app::vocab_popup::refresh_vocab_popup(&mut s);
                            }
                        }
                        // Prose straddling paragraph: if the spoken paragraph
                        // continues onto the next stored prose page, schedule a
                        // TimePos-driven turn at the moment the narration crosses
                        // the page boundary (which falls INSIDE this paragraph).
                        // A whole-line sync turn would flip the page too early —
                        // as soon as the cursor lands on the paragraph — clipping
                        // the paragraph's first rows before they're spoken. This
                        // runs on every CursorSync; it re-derives the SAME target
                        // while the cursor stays on the straddling paragraph, so
                        // we skip the (DB-touching) recompute when the pending
                        // cross already targets the right continuation page, and
                        // clear it once the cursor is no longer on a straddle.
                        if s.is_prose() {
                            let mut target: Option<(f64, usize)> = None;
                            if let Some(table) =
                                crate::input::prose_pages::active_prose_page_table(&s)
                            {
                                if let Some(pi) = crate::input::prose_pages::prose_page_for_position(
                                    &table, s.page_top_line, s.page_top_offset)
                                {
                                    let p = table[pi];
                                    // Straddles: this page ENDS inside the cursor's
                                    // line and a next page exists. Deliberately NOT
                                    // gated on the line's first row being on THIS
                                    // page — that would only be true on the
                                    // paragraph's FIRST page. The generator
                                    // normalizes full-height ends to (L+1, 0), so
                                    // `p.end_line == buffer_line` here already means
                                    // the paragraph genuinely continues onto pi+1,
                                    // whether this is the paragraph's first, a
                                    // middle, or its last page before the turn. See
                                    // Important #1 in final-review.md — the
                                    // first-row restriction stalled the sync turn on
                                    // every middle page of a >=3-page paragraph.
                                    if pi + 1 < table.len() && p.end_line == buffer_line {
                                        let already = s.pending_prose_cross
                                            .map(|(_, tp)| tp == pi + 1)
                                            .unwrap_or(false);
                                        if already {
                                            target = s.pending_prose_cross; // unchanged; no recompute
                                        } else if let Some((t, ts_start)) = crate::input::navigation::prose_cross_time(&s, buffer_line, p.end_off) {
                                            let fire = (t - crate::input::navigation::SYNC_PREROLL).max(0.0);
                                            if t <= ts_start + 0.05 {
                                                // Belt-and-braces: the boundary is at
                                                // or before the line's spoken start —
                                                // a degenerate leading-gap boundary
                                                // (should not survive prose_next_
                                                // boundary's normalization, but if it
                                                // does the "cross" is already in the
                                                // past and would fire ~1.5s late on the
                                                // next TimePos tick). Do NOT schedule;
                                                // let update_highlight_and_advance_page
                                                // turn the page normally.
                                                crate::logging::log_always(&format!(
                                                    "SYNC_PROSE_CROSS: skip degenerate t={:.2} <= ts.start {:.2} (bl={} end_off={}) — normal advance",
                                                    t, ts_start, buffer_line, p.end_off));
                                                target = None;
                                            } else if fire <= s.current_time_pos {
                                                // Legitimate cross whose fire time is
                                                // already at/before the current
                                                // playback position: waiting for the
                                                // next TimePos would land the turn
                                                // ~1.5s late. Turn NOW (same body as
                                                // the TimePos fire path) instead of
                                                // scheduling.
                                                let np = table[pi + 1];
                                                if (s.page_top_line, s.page_top_offset)
                                                    != (np.start_line, np.start_off)
                                                {
                                                    crate::logging::log_always(&format!(
                                                        "SYNC_PROSE_CROSS: fired-immediately pos={:.2} fire={:.2} -> page {} top=({},{}) bl={} end_off={}",
                                                        s.current_time_pos, fire, pi + 2, np.start_line, np.start_off, buffer_line, p.end_off));
                                                    crate::input::scroll::set_page_instant_offset(
                                                        &mut s, np.start_line, np.start_off);
                                                    crate::input::navigation::after_page_change(
                                                        &mut s,
                                                        crate::input::navigation::PageChangeReason::MpvSync,
                                                    );
                                                }
                                                target = None; // handled; nothing pending
                                            } else {
                                                crate::logging::log_always(&format!(
                                                    "SYNC_PROSE_CROSS: scheduled t={:.2} fire={:.2} page {}->{} bl={} end_off={}",
                                                    t, fire, pi + 1, pi + 2, buffer_line, p.end_off));
                                                target = Some((fire, pi + 1));
                                            }
                                        }
                                    }
                                }
                            }
                            s.pending_prose_cross = target;
                        } else {
                            s.pending_prose_cross = None;
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
                        // Mirror the (possibly moved) cursor into the translation
                        // overlay so its highlight follows audio playback. Drop the
                        // borrow first — sync_translation_overlay re-borrows.
                        drop(s);
                        crate::app::translations::sync_translation_overlay(&state_for_events, ov_scene_before);
                    }
                    MpvEvent::ConnectionStatus(connected) => {
                        state_for_events.borrow_mut().mpv_connected = connected;
                        crate::logging::log(&format!("MPV connection: {}", connected));
                    }
                    MpvEvent::PlaybackState(playing) => {
                        let mut s = state_for_events.borrow_mut();
                        s.mpv_playing = playing;
                        // Starting playback revives sync. Navigating onto an
                        // UNTIMESTAMPED line suppresses CursorSync indefinitely
                        // (no seek is possible, the cursor must stay put) — but
                        // once the user resumes playback they expect the
                        // highlight to follow the audio again; without this the
                        // suppression outlives the pause and sync is dead for
                        // the rest of the session. Brief seek debounces (500ms)
                        // are left to expire on their own.
                        if playing {
                            if let Some(until) = s.suppress_sync_until {
                                if until.saturating_duration_since(std::time::Instant::now())
                                    > std::time::Duration::from_secs(60)
                                {
                                    s.suppress_sync_until = None;
                                    // Drop the paint-hold with it: a paused nav
                                    // seek sets phrase_paint_hold to the same
                                    // far-future instant, and a stale hold could
                                    // outlast a later, shorter suppression.
                                    s.phrase_paint_hold = None;
                                    crate::logging::log_always(
                                        "SYNC: cleared indefinite suppression on playback start",
                                    );
                                }
                            }
                        }
                        crate::logging::log(&format!(
                            "MPV playback: {}",
                            if playing { "playing" } else { "paused" }
                        ));
                    }
                    MpvEvent::TimePos(pos) => {
                        let mut s = state_for_events.borrow_mut();
                        s.current_time_pos = pos;

                        // Task 9: fire a scheduled prose page crossing once the
                        // narration reaches the boundary time. The cursor stays on
                        // the straddling paragraph (current_line unchanged) — only
                        // the visible page window advances to its continuation.
                        if s.sync_enabled && !s.loading_work.get() && s.vocab_loop.is_none() {
                            // Belt-and-braces: skip firing while an explicit seek
                            // path (search n/N, concordance r/R, etc.) has sync
                            // suppressed, mirroring the CursorSync suppression
                            // check above. Read-only here — do NOT consume the
                            // pending cross while suppressed; it stays armed and
                            // may still fire once suppression lapses IF the
                            // target is still valid (the explicit clears in
                            // land_on_match_idx / concordance_position_cursor
                            // handle the cases where it should NOT survive).
                            let suppressed = s.suppress_sync_until
                                .map(|until| std::time::Instant::now() < until)
                                .unwrap_or(false);
                            if let Some((fire_at, page_idx)) = s.pending_prose_cross {
                                if pos >= fire_at && s.mpv_playing && !suppressed {
                                    s.pending_prose_cross = None;
                                    if let Some(table) =
                                        crate::input::prose_pages::active_prose_page_table(&s)
                                    {
                                        if let Some(p) = table.get(page_idx).copied() {
                                            // Only turn if we're not already there
                                            // (a resnap may have advanced the page).
                                            if (s.page_top_line, s.page_top_offset)
                                                != (p.start_line, p.start_off)
                                            {
                                                crate::logging::log_always(&format!(
                                                    "SYNC_PROSE_CROSS: fired pos={:.2} -> page {} top=({},{})",
                                                    pos, page_idx + 1, p.start_line, p.start_off));
                                                crate::input::scroll::set_page_instant_offset(
                                                    &mut s, p.start_line, p.start_off);
                                                crate::input::navigation::after_page_change(
                                                    &mut s,
                                                    crate::input::navigation::PageChangeReason::MpvSync,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Set to the overlay's pre-move scene only when the
                        // pending_advance actually moves the cursor below.
                        let mut ov_moved: Option<(i64, i64)> = None;

                        // Advance to untimestamped next line when current line's audio ends
                        if s.sync_enabled && !s.loading_work.get() && s.vocab_loop.is_none() {
                            if let Some((end_time, next_bl, _source_wi)) = s.pending_advance {
                                if pos >= end_time {
                                    s.pending_advance_ignore_bl = Some(s.current_line);
                                    s.pending_advance = None;
                                    if s.current_line != next_bl {
                                        ov_moved = Some(crate::app::scene_synopsis::current_scene_divs(&s));
                                        s.current_line = next_bl;
                                        crate::input::navigation::update_highlight_and_advance_page(
                                            &mut s,
                                        );
                                    }
                                }
                            }
                        }

                        // Karaoke phrase highlight tracks the RAW time (the
                        // sync cursor above leads by SYNC_PREROLL; this must not).
                        crate::input::phrase_highlight::update_phrase_highlight(&mut s, pos);

                        // Mirror the advanced cursor into the translation overlay.
                        // Only when pending_advance moved it; drop the borrow first.
                        if let Some(scene_before) = ov_moved {
                            drop(s);
                            crate::app::translations::sync_translation_overlay(&state_for_events, scene_before);
                        } else {
                            drop(s);
                        }
                        // Keep the page-image card following playback (cheap no-op
                        // when image_mode is off or the page is unchanged).
                        crate::app::refresh_page_image(&state_for_events);
                    }
                    MpvEvent::ThemeChanged => {
                        // SIGUSR1 = "re-read MY config and re-apply". External
                        // control: edit config.json's theme (and/or
                        // theme_cycle), then kill -USR1.
                        let mut s = state_for_events.borrow_mut();
                        // Re-read from config.json ON DISK — the
                        // external-control contract is "edit config.json,
                        // then kill -USR1"; the in-memory config only knows
                        // the startup value. Adopt only `theme` and
                        // `theme_cycle` from disk (NOT the whole Config) —
                        // `load()` re-applies normalizations that would fight
                        // other in-memory runtime state. Without adopting
                        // theme_cycle here, apply_theme_to_state's config::save
                        // below would clobber a disk-edited theme_cycle with
                        // the stale in-memory one.
                        let disk = crate::config::load();
                        s.config.theme_cycle = disk.theme_cycle.clone();
                        let name = disk.theme_name().to_string();
                        let theme = crate::theme::load_theme_with_fallback(&name);
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &theme);
                    }
                }
            }
        });

        // cmd_tx is stored in AppState — no need for std::mem::forget
        let _ = state;
    });

    // Run with an EMPTY argv so GTK ignores our own markers (e.g.
    // `--headless-test`, a process-table tag used by the headless harness to
    // pgrep test instances without ever matching the live session). The marker
    // stays in the real process command line for `pgrep -f`; GTK never sees it.
    application.run_with_args::<&str>(&[]);
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
