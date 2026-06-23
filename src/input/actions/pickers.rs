use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

/// Enter the given picker `mode`. The uniform tail of every picker-open path;
/// the caller does its own `show(...)` first (the show signature varies per
/// picker, so only the mode-set is shared).
pub(crate) fn open_picker_mode(s: &mut AppState, mode: crate::app::InputMode) {
    s.input_mode = mode;
}

/// Which gloss_type the Alt+g picker is currently filtered to. Cycled by Alt+t.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum GlossPickerFilter {
    TeacherGeneric,
    InnerMonologue,
    #[default]
    ReaderGloss,
}

impl GlossPickerFilter {
    pub(crate) fn gloss_type(self) -> &'static str {
        match self {
            GlossPickerFilter::TeacherGeneric => "teacher-generic",
            GlossPickerFilter::InnerMonologue => "inner-monologue",
            GlossPickerFilter::ReaderGloss => "reader-gloss",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            GlossPickerFilter::TeacherGeneric => GlossPickerFilter::InnerMonologue,
            GlossPickerFilter::InnerMonologue => GlossPickerFilter::ReaderGloss,
            GlossPickerFilter::ReaderGloss => GlossPickerFilter::TeacherGeneric,
        }
    }
}

/// Load the selected work in the library picker, hide the picker, and
/// display the new work. Spawns an async task to query the DB.
pub(crate) fn load_selected_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state.borrow().library_picker.selected_abbrev();
    if let Some(abbrev) = abbrev {
        crate::logging::log(&format!("PICKER: selected work '{}'", abbrev));
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::commands::MpvCommand::Pause);
            s.library_picker.hide();
            s.gloss_overlay.show_loading_message("Loading...");
        }
        state.borrow_mut().input_mode = crate::app::InputMode::Reader;
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        let handle_for_write = handle.clone();
        glib::spawn_future_local(async move {
            let t_db = std::time::Instant::now();
            let abbrev_for_log = abbrev.clone();
            let result = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev)?;
                    // Cache check — same pattern as build_window's MRU branch.
                    let t_read = std::time::Instant::now();
                    let (prepared, was_miss) = if let Some(snap) = crate::snapshot::read(&work) {
                        let bytes =
                            std::fs::metadata(crate::snapshot::cache_path(&work.abbrev))
                                .map(|m| m.len())
                                .unwrap_or(0);
                        crate::logging::log(&format!(
                            "SNAPSHOT: cache hit {} ({}ms, {} bytes)",
                            work.abbrev,
                            t_read.elapsed().as_millis(),
                            bytes
                        ));
                        let work_type = work.work_type.clone();
                        let prep = crate::app::PreparedText {
                            abbrev: snap.abbrev,
                            work_type: work_type.clone(),
                            file_lines_count: snap.filtered_contents.lines().count(),
                            cleaned_lines_count: snap.filtered_contents.lines().count(),
                            work_lines_count: work.lines.len(),
                            filtered_contents: snap.filtered_contents,
                            line_map: snap.line_map,
                            path: snap.text_file_path,
                            is_prose: crate::db::line_types::is_prose_work(&work_type),
                        };
                        (Some(prep), false)
                    } else {
                        if !crate::snapshot::cache_path(&work.abbrev).exists() {
                            crate::logging::log(&format!(
                                "SNAPSHOT: cache miss {} (file_missing)",
                                work.abbrev
                            ));
                        }
                        (crate::app::prepare_text_for_display(&work), true)
                    };
                    Ok::<_, rusqlite::Error>((work, prepared, was_miss))
                })
                .await;
            crate::logging::log(&format!("PICKER: load_work '{}' DB query {:.0}ms", abbrev_for_log, t_db.elapsed().as_millis()));
            match result {
                Ok(Ok((work, prepared, was_cache_miss))) => {
                    crate::logging::log(&format!(
                        "PICKER: loaded '{}' lines={} timestamps={} text_file={:?}",
                        work.abbrev, work.lines.len(), work.timestamps.len(), work.text_file.is_some()
                    ));
                    // Capture write inputs BEFORE display_work consumes prepared and work.
                    let write_inputs = if was_cache_miss {
                        prepared.as_ref().map(|p| {
                            (work.clone(), p.filtered_contents.clone(), p.line_map.clone())
                        })
                    } else {
                        None
                    };
                    {
                        let mut s = state_clone.borrow_mut();
                        s.gloss_overlay.hide();
                        crate::app::clear_display(&mut s);
                        crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                        crate::logging::log(&format!(
                            "PICKER: after display_work current_line={} page_top={} line_map={} effective_lines={}",
                            s.current_line, s.page_top_line, s.line_map.is_some(), s.effective_line_count()
                        ));
                    }
                    // After display_work: on cache miss with valid prep, write snapshot.
                    if let Some((w, filtered, line_map)) = write_inputs {
                        handle_for_write.spawn_blocking(move || {
                            let _ = crate::snapshot::write(&w, &filtered, &line_map);
                        });
                    }
                }
                Ok(Err(e)) => {
                    crate::logging::log(&format!("PICKER: load_work error: {}", e));
                    let s = state_clone.borrow();
                    s.gloss_overlay.hide();
                }
                Err(e) => {
                    crate::logging::log(&format!("PICKER: task join error: {}", e));
                    let s = state_clone.borrow();
                    s.gloss_overlay.hide();
                }
            }
        });
    }
}

pub(crate) fn toggle_previous_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    // Concordance-aware: toggle between origin work and current concordance hit
    {
        let s = state.borrow();
        if let (Some(ref conc), Some(ref origin)) = (&s.concordance_state, &s.concordance_origin) {
            let current_abbrev = s.current_work.as_ref().map(|w| w.abbrev.as_str());
            let on_origin = current_abbrev == Some(&origin.work_abbrev);
            let (target_abbrev, target_line_id) = if on_origin {
                if let Some(hit) = conc.current_hit() {
                    (hit.work_abbrev.clone(), Some(hit.line_mapping_id))
                } else {
                    crate::logging::log("CONC_TOGGLE: no current hit");
                    return;
                }
            } else {
                (origin.work_abbrev.clone(), Some(origin.line_mapping_id))
            };
            if current_abbrev == Some(&target_abbrev) {
                // Same work — just jump to the target line
                if let Some(target_lm) = target_line_id {
                    crate::logging::log(&format!(
                        "CONC_TOGGLE: same-work jump to lm{}",
                        target_lm,
                    ));
                    drop(s);
                    jump_to_line_mapping_id(state, target_lm);
                } else {
                    crate::logging::log("CONC_TOGGLE: same work, no target line");
                }
                return;
            }
            crate::logging::log(&format!(
                "CONC_TOGGLE: {} -> {} (lm {:?})",
                current_abbrev.unwrap_or("?"), target_abbrev, target_line_id,
            ));
            drop(s);
            load_work_at(state, tokio_handle, target_abbrev, target_line_id);
            return;
        }
    }

    // Standard previous-work toggle
    let abbrev = state.borrow().config.previous_work.clone();
    let abbrev = match abbrev {
        Some(a) => a,
        None => {
            crate::logging::log("TOGGLE_PREV: no previous work");
            return;
        }
    };
    if state.borrow().current_work.as_ref().map(|w| w.abbrev.as_str()) == Some(&abbrev) {
        crate::logging::log("TOGGLE_PREV: already viewing that work");
        return;
    }
    crate::logging::log(&format!("TOGGLE_PREV: switching to '{}'", abbrev));
    load_work_at(state, tokio_handle, abbrev, None);
}

fn jump_to_line_mapping_id(state: &Rc<RefCell<AppState>>, line_mapping_id: i64) {
    let mut s = state.borrow_mut();
    let buf_idx = s.current_work.as_ref().and_then(|w| {
        let work_idx = w.lines.iter().position(|l| l.id == line_mapping_id)?;
        if let Some(ref lm) = s.line_map {
            Some(lm.work_to_buffer[work_idx])
        } else {
            Some(work_idx)
        }
    });
    if let Some(idx) = buf_idx {
        s.current_line = idx;
        crate::input::highlight::update_highlight_and_center(&mut s);
    }
}

fn load_work_at(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    abbrev: String,
    target_line_id: Option<i64>,
) {
    {
        let s = state.borrow();
        let _ = s.cmd_tx.try_send(crate::mpv::commands::MpvCommand::Pause);
    }
    let state_clone = Rc::clone(state);
    let handle = tokio_handle.clone();
    let handle_for_write = handle.clone();
    glib::spawn_future_local(async move {
        let abbrev_for_log = abbrev.clone();
        let result = handle
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                let work = crate::db::queries::load_work(&conn, &abbrev)?;
                let t_read = std::time::Instant::now();
                let (prepared, was_miss) = if let Some(snap) = crate::snapshot::read(&work) {
                    let bytes = std::fs::metadata(crate::snapshot::cache_path(&work.abbrev))
                        .map(|m| m.len())
                        .unwrap_or(0);
                    crate::logging::log(&format!(
                        "SNAPSHOT: cache hit {} ({}ms, {} bytes)",
                        work.abbrev,
                        t_read.elapsed().as_millis(),
                        bytes
                    ));
                    let work_type = work.work_type.clone();
                    let prep = crate::app::PreparedText {
                        abbrev: snap.abbrev,
                        work_type: work_type.clone(),
                        file_lines_count: snap.filtered_contents.lines().count(),
                        cleaned_lines_count: snap.filtered_contents.lines().count(),
                        work_lines_count: work.lines.len(),
                        filtered_contents: snap.filtered_contents,
                        line_map: snap.line_map,
                        path: snap.text_file_path,
                        is_prose: crate::db::line_types::is_prose_work(&work_type),
                    };
                    (Some(prep), false)
                } else {
                    if !crate::snapshot::cache_path(&work.abbrev).exists() {
                        crate::logging::log(&format!(
                            "SNAPSHOT: cache miss {} (file_missing)",
                            work.abbrev
                        ));
                    }
                    (crate::app::prepare_text_for_display(&work), true)
                };
                Ok::<_, rusqlite::Error>((work, prepared, was_miss))
            })
            .await;
        crate::logging::log(&format!(
            "TOGGLE_PREV: load_work '{}' done",
            abbrev_for_log
        ));
        match result {
            Ok(Ok((work, prepared, was_cache_miss))) => {
                let write_inputs = if was_cache_miss {
                    prepared.as_ref().map(|p| {
                        (work.clone(), p.filtered_contents.clone(), p.line_map.clone())
                    })
                } else {
                    None
                };
                {
                    let mut s = state_clone.borrow_mut();
                    crate::app::clear_display(&mut s);
                    crate::app::display_work_at_with_prepared(&mut s, work, target_line_id, prepared);
                }
                if let Some((w, filtered, line_map)) = write_inputs {
                    handle_for_write.spawn_blocking(move || {
                        let _ = crate::snapshot::write(&w, &filtered, &line_map);
                    });
                }
            }
            Ok(Err(e)) => {
                crate::logging::log(&format!("TOGGLE_PREV: load error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("TOGGLE_PREV: task join error: {}", e));
            }
        }
    });
}

/// Open the bookmark picker, querying bookmarks for the current work.
/// Spawns an async task to query the DB.
pub(crate) fn open_bookmark_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
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
                    crate::db::queries::load_bookmarks_with_details(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.gloss_overlay.hide();
                s.bookmark_picker.set_items(items);
            }
            let mut s = state_clone.borrow_mut();
            s.bookmark_picker.show();
            open_picker_mode(&mut s, crate::app::InputMode::BookmarkPicker);
        });
    }
}

/// Open the media picker, querying media files for the current work.
/// Spawns an async task to query the DB.
pub(crate) fn open_media_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (abbrev, work_title) = {
        let s = state.borrow();
        match s.current_work.as_ref() {
            Some(w) => (w.abbrev.clone(), w.title.clone()),
            None => return,
        }
    };
    {
        state.borrow().media_picker.set_title(&abbrev, &work_title);
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        let handle_for_confirm = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let abbrev_log = abbrev.clone();
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::list_media_for_work(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            crate::logging::log(&format!(
                "MEDIA_PICKER: work='{}' found {} media files", abbrev_log, items.len()
            ));
            if items.len() == 1 {
                let item = &items[0];
                crate::logging::log(&format!(
                    "MEDIA_PICKER: auto-selecting single media: id={} path='{}'",
                    item.media_id, item.path
                ));
                let path = item.path.clone();
                let media_id = item.media_id;
                let already_connected = state_clone.borrow().mpv_connected;
                let should_resume = state_clone.borrow().concordance_resume_playback;
                let seek_time = if should_resume {
                    let s = state_clone.borrow();
                    crate::input::actions::concordance::concordance_current_seek_time(&s)
                } else {
                    None
                };
                if already_connected {
                    let s = state_clone.borrow();
                    if let Some(t) = seek_time {
                        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileAndSeek(path.clone(), t));
                    } else {
                        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFile(path.clone()));
                        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
                    }
                    crate::logging::log(&format!(
                        "MEDIA_PICKER: loadfile '{}' into existing MPV", path
                    ));
                } else {
                    let path_for_discover = path.clone();
                    let socket_path = handle_for_confirm
                        .spawn_blocking(move || {
                            if let Some((sock, _)) =
                                crate::mpv::discovery::find_socket_for_work(&[path_for_discover.clone()])
                            {
                                return sock.to_string_lossy().to_string();
                            }
                            let launched = crate::mpv::discovery::launch_mpv(&path_for_discover);
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
                        let s = state_clone.borrow();
                        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Connect(socket_path));
                    }
                }
                {
                    let mut s = state_clone.borrow_mut();
                    s.media_id = Some(media_id);
                    s.concordance_resume_playback = false;
                    crate::logging::log(&format!(
                        "MEDIA_PICKER: set media_id={}", media_id
                    ));
                }
                return;
            }
            {
                let mut s = state_clone.borrow_mut();
                s.gloss_overlay.hide();
                s.media_picker.set_items(items);
            }
            let mut s = state_clone.borrow_mut();
            s.media_picker.show();
            open_picker_mode(&mut s, crate::app::InputMode::MediaPicker);
        });
    }
}

/// Confirm the selected media file: discover or launch the MPV socket,
/// re-send filtered timestamps, and connect MPV. Called from the media
/// picker's Return key.
pub(crate) fn confirm_media_selection(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected_path = state.borrow().media_picker.selected_media_path();
    let selected_id = state.borrow().media_picker.selected_media_id();
    crate::logging::log(&format!(
        "MEDIA_CONFIRM: path={:?} media_id={:?}", selected_path, selected_id
    ));
    if let (Some(path), Some(media_id)) = (selected_path, selected_id) {
        let already_connected = state.borrow().mpv_connected;
        let should_resume = state.borrow().concordance_resume_playback;
        let seek_time = if should_resume {
            crate::input::actions::concordance::concordance_current_seek_time(&state.borrow())
        } else {
            None
        };
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        if already_connected {
            if let Some(t) = seek_time {
                let _ = state.borrow().cmd_tx.try_send(
                    crate::mpv::MpvCommand::LoadFileAndSeek(path.clone(), t),
                );
            } else {
                let _ = state.borrow().cmd_tx.try_send(
                    crate::mpv::MpvCommand::LoadFile(path.clone()),
                );
                let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            }
            crate::logging::log(&format!("MEDIA_PICKER: loadfile '{}' into existing MPV", path));
        } else {
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
        }
        glib::spawn_future_local(async move {
            let need_connect = if already_connected {
                String::new()
            } else {
                let path_for_discover = path.clone();
                handle
                    .spawn_blocking(move || {
                        if let Some((sock, _)) =
                            crate::mpv::discovery::find_socket_for_work(&[path_for_discover.clone()])
                        {
                            return sock.to_string_lossy().to_string();
                        }
                        let launched = crate::mpv::discovery::launch_mpv(&path_for_discover);
                        for _ in 0..60 {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            if std::path::Path::new(&launched).exists() {
                                return launched;
                            }
                        }
                        launched
                    })
                    .await
                    .unwrap_or_default()
            };

            {
                let mut s = state_clone.borrow_mut();
                s.media_id = Some(media_id);
                // Rebuild per-line timestamps for the new media_id
                if let Some(ref mut work) = s.current_work {
                    // Build timestamp + chapter lookups from work.timestamps
                    let mut ts_map: std::collections::HashMap<i64, crate::db::models::TimeRange> =
                        std::collections::HashMap::new();
                    let mut chapter_set: std::collections::HashSet<i64> =
                        std::collections::HashSet::new();
                    for ts in &work.timestamps {
                        if ts.media_id == media_id {
                            ts_map.entry(ts.line_id).or_insert(crate::db::models::TimeRange {
                                start: ts.start,
                                end: ts.end,
                                sentence_start: ts.sentence_start,
                                is_manual: ts.is_manual,
                            });
                            if ts.is_chapter {
                                chapter_set.insert(ts.line_id);
                            }
                        }
                    }
                    for line in &mut work.lines {
                        line.timestamp = ts_map.get(&line.id).copied();
                        line.is_chapter = chapter_set.contains(&line.id);
                    }
                    // Re-send timestamps to MPV
                    let mut ts_data: Vec<(i64, f64, f64)> = work
                        .timestamps
                        .iter()
                        .filter(|t| t.media_id == media_id)
                        .map(|t| (t.line_id, t.start, t.end))
                        .collect();
                    ts_data.sort_by(|a, b| {
                        a.1.partial_cmp(&b.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let mut id_to_idx: std::collections::HashMap<i64, usize> =
                        std::collections::HashMap::new();
                    for (i, line) in work.lines.iter().enumerate() {
                        id_to_idx.insert(line.id, i);
                    }
                    let _ = s.cmd_tx.try_send(
                        crate::mpv::MpvCommand::SetTimestamps {
                            timestamps: ts_data,
                            line_id_to_index: id_to_idx,
                        },
                    );
                }
                // Rebuild gutter sign vecs for new media
                let new_has_ts: Vec<bool> = if let Some(ref lm) = s.line_map {
                    lm.buffer_to_work
                        .iter()
                        .map(|opt_idx| {
                            opt_idx
                                .and_then(|idx| s.current_work.as_ref()?.lines.get(idx)?.timestamp.as_ref())
                                .is_some()
                        })
                        .collect()
                } else {
                    s.current_work
                        .as_ref()
                        .map(|w| w.lines.iter().map(|l| l.timestamp.is_some()).collect())
                        .unwrap_or_default()
                };
                *s.has_timestamp.borrow_mut() = new_has_ts;
                let new_is_manual: Vec<bool> = if let Some(ref lm) = s.line_map {
                    lm.buffer_to_work
                        .iter()
                        .map(|opt_idx| {
                            opt_idx
                                .and_then(|idx| {
                                    Some(s.current_work.as_ref()?.lines.get(idx)?.timestamp.as_ref()?.is_manual)
                                })
                                .unwrap_or(false)
                        })
                        .collect()
                } else {
                    s.current_work
                        .as_ref()
                        .map(|w| w.lines.iter().map(|l| l.timestamp.as_ref().map_or(false, |t| t.is_manual)).collect())
                        .unwrap_or_default()
                };
                *s.is_manual.borrow_mut() = new_is_manual;
                let new_is_ch: Vec<bool> = if let Some(ref lm) = s.line_map {
                    lm.buffer_to_work
                        .iter()
                        .map(|opt_idx| {
                            opt_idx
                                .and_then(|idx| Some(s.current_work.as_ref()?.lines.get(idx)?.is_chapter))
                                .unwrap_or(false)
                        })
                        .collect()
                } else {
                    s.current_work
                        .as_ref()
                        .map(|w| w.lines.iter().map(|l| l.is_chapter).collect())
                        .unwrap_or_default()
                };
                *s.is_chapter_line.borrow_mut() = new_is_ch;
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
                if let Some(ref renderer) = s.right_gutter_renderer {
                    renderer.queue_draw();
                }
                if !need_connect.is_empty() {
                    let _ = s
                        .cmd_tx
                        .try_send(crate::mpv::MpvCommand::Connect(need_connect));
                }
                s.media_picker.hide();
                s.input_mode = crate::app::InputMode::Reader;
                crate::logging::log(&format!(
                    "MEDIA: switched to media_id={}",
                    media_id
                ));
                s.concordance_resume_playback = false;
                drop(s);
            }
        });
    }
}

/// Set the selected media as the default (highest priority) for the current
/// work. Spawns an async task to write to the DB, then updates the picker
/// widget on completion. Called from the media picker's `p` key.
pub(crate) fn set_media_default(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected_id = state.borrow().media_picker.selected_media_id();
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let (Some(media_id), Some(abbrev)) = (selected_id, abbrev) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db_rw()?;
                    crate::db::queries::set_media_priority(&conn, &abbrev, media_id)?;
                    let max_pri: i64 = conn
                        .query_row(
                            "SELECT priority FROM work_media_associations \
                             WHERE work_abbrev = ?1 AND media_id = ?2",
                            rusqlite::params![&abbrev, media_id],
                            |row| row.get(0),
                        )
                        .unwrap_or(20);
                    crate::logging::log(&format!(
                        "MEDIA: set default media_id={} for {} (pri={})",
                        media_id, abbrev, max_pri
                    ));
                    Ok::<_, rusqlite::Error>((media_id, max_pri))
                })
                .await;
            match result {
                Ok(Ok((media_id, max_pri))) => {
                    state_clone
                        .borrow_mut()
                        .media_picker
                        .set_default(media_id, max_pri);
                }
                Ok(Err(e)) => {
                    crate::logging::log(&format!(
                        "MEDIA: set_media_default DB error: {}",
                        e
                    ));
                }
                Err(e) => {
                    crate::logging::log(&format!(
                        "MEDIA: set_media_default join error: {}",
                        e
                    ));
                }
            }
        });
    }
}

/// Open the recent works picker: same as library picker but filtered to the
/// last 10 viewed works from config.recent_works. The alternate work
/// (previous_work, toggled via minus key) is always included.
pub(crate) fn open_recent_picker(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    if !s.library_picker.is_visible()
        && !s.bookmark_picker.is_visible()
        && !s.media_picker.is_visible()
        && !s.settings_overlay.is_visible()
    {
        let mut recent = s.config.recent_works.clone();
        if let Some(ref prev) = s.config.previous_work {
            if !recent.contains(prev) {
                recent.push(prev.clone());
            }
        }
        drop(s);
        let mut sm = state.borrow_mut();
        sm.concordance_state = None;
        sm.concordance_bar.hide();
        crate::input::actions::concordance::restore_sync_after_concordance(&mut sm);
        drop(sm);
        state.borrow().gloss_overlay.hide();
        state.borrow_mut().library_picker.show_prepare_recent(&recent);
        state.borrow().library_picker.show_finish();
        state.borrow_mut().input_mode = crate::app::InputMode::LibraryPicker;
    }
}

/// Open the library picker from reader mode: hide other overlays, tear down
/// concordance state, and switch to LibraryPicker input mode.
pub(crate) fn open_library_picker_from_reader(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    if !s.library_picker.is_visible()
        && !s.bookmark_picker.is_visible()
        && !s.media_picker.is_visible()
        && !s.settings_overlay.is_visible()
    {
        drop(s);
        let mut sm = state.borrow_mut();
        sm.concordance_state = None;
        sm.concordance_bar.hide();
        crate::input::actions::concordance::restore_sync_after_concordance(&mut sm);
        drop(sm);
        state.borrow().gloss_overlay.hide();
        state.borrow_mut().library_picker.show_prepare();
        state.borrow().library_picker.show_finish();
        state.borrow_mut().input_mode = crate::app::InputMode::LibraryPicker;
    }
}

/// Open the keybinds overlay, toggling visibility. If already visible (or
/// gamepad overlay is visible), hide both and return to Reader mode.
/// Otherwise, hide other overlays and show the keybinds overlay.
/// Note: the chord start (KeyState::start_chord) must be called separately
/// by the dispatch arm since it touches key_state.
pub(crate) fn open_keybinds_overlay(state: &Rc<RefCell<AppState>>) {
    open_keybinds_from_mode(state, crate::app::InputMode::Reader);
}

/// Open the Ctrl+/ keybinds overlay, recording `return_mode` so its close paths
/// restore that mode (the overlay it was opened from) instead of always
/// dropping to the reader. Called by `open_keybinds_overlay` (reader, return
/// mode `Reader`) and from the per-overlay Ctrl+/ handlers with their own mode.
/// Toggles closed (back to `return_mode`) if already visible.
pub(crate) fn open_keybinds_from_mode(
    state: &Rc<RefCell<AppState>>,
    return_mode: crate::app::InputMode,
) {
    let s = state.borrow();
    if s.keybinds_overlay.is_visible() || s.gamepad_overlay.is_visible() {
        s.keybinds_overlay.hide();
        s.gamepad_overlay.hide();
        let back = s.keybinds_return_mode;
        drop(s);
        restore_mode_after_keybinds(state, back);
    } else {
        s.library_picker.hide();
        s.media_picker.hide();
        s.settings_overlay.hide();
        s.search_bar.hide();
        s.gloss_overlay.hide();
        s.vocab_popup.hide();
        s.keybinds_overlay.show();
        drop(s);
        let mut s = state.borrow_mut();
        s.keybinds_return_mode = return_mode;
        s.input_mode = crate::app::InputMode::KeybindsOverlay;
    }
}

/// Restore the input mode (and re-show the overlay widget) after the keybinds
/// overlay closes. For gloss/synopsis the overlay widget was hidden on open, so
/// it must be shown again; other modes just need the input mode restored. Always
/// resets `keybinds_return_mode` to `Reader`.
pub(crate) fn restore_mode_after_keybinds(
    state: &Rc<RefCell<AppState>>,
    return_mode: crate::app::InputMode,
) {
    let mut s = state.borrow_mut();
    match return_mode {
        crate::app::InputMode::GlossOverlay | crate::app::InputMode::SynopsisOverlay => {
            s.gloss_overlay.show_again();
            s.input_mode = return_mode;
        }
        crate::app::InputMode::Reader => {
            s.input_mode = crate::app::InputMode::Reader;
        }
        // Any other recorded overlay: its widget is a persistent add_overlay
        // panel that was not hidden on open, so only the input mode needs
        // restoring.
        other => {
            s.input_mode = other;
        }
    }
    s.keybinds_return_mode = crate::app::InputMode::Reader;
}

/// Open the concordance word picker with the current work's vocab words.
pub(crate) fn open_concordance_word_picker(state: &Rc<RefCell<AppState>>) {
    let words: Vec<(String, usize)> = {
        let s = state.borrow();
        let mut seen = std::collections::BTreeSet::new();
        for m in &s.vocab_matches {
            seen.insert(m.word.clone());
        }
        seen.into_iter().map(|w| (w, 0)).collect()
    };
    state.borrow_mut().concordance_word_picker.set_words(words);
    let mut s = state.borrow_mut();
    s.concordance_word_picker.show();
    open_picker_mode(&mut s, crate::app::InputMode::ConcordanceWordPicker);
}

/// Open the concordance occurrence list picker for the current concordance state.
pub(crate) fn open_concordance_list_picker(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    if let Some(conc) = &s.concordance_state {
        s.concordance_list_picker.show(&conc.occurrences, conc.current_index);
    }
    drop(s);
    open_picker_mode(&mut state.borrow_mut(), crate::app::InputMode::ConcordanceListPicker);
}

pub(crate) fn open_concordance_works_picker(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let conc = match &s.concordance_state {
        Some(c) => c,
        None => return,
    };
    let mut works: Vec<(String, String, usize)> = Vec::new();
    for hit in &conc.occurrences {
        if let Some(entry) = works.iter_mut().find(|(a, _, _)| a == &hit.work_abbrev) {
            entry.2 += 1;
        } else {
            works.push((hit.work_abbrev.clone(), hit.work_title.clone(), 1));
        }
    }
    if works.is_empty() {
        return;
    }
    s.concordance_works_picker.show(&works);
    drop(s);
    open_picker_mode(&mut state.borrow_mut(), crate::app::InputMode::ConcordanceWorksPicker);
}

pub(crate) fn open_gloss_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    // Opening always starts on the default type (teacher-generic).
    state.borrow_mut().gloss_picker_filter = GlossPickerFilter::default();
    // Glosses are stored under the base abbrev (`-Amb` stripped), so normalize
    // here to match — otherwise the picker is empty for `-Amb` variants.
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| crate::gloss::normalize_abbrev(&w.abbrev).to_string());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let gloss_type = GlossPickerFilter::default().gloss_type();
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::find_glossed_passages(&conn, &abbrev, &[gloss_type])
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                // When opened from within the gloss overlay (Alt+g), keep the
                // overlay visible behind the picker; otherwise hide it so the
                // picker reads as a modal over the reader.
                if !s.gloss_picker_from_overlay {
                    s.gloss_overlay.hide();
                }
                s.gloss_picker.set_items(items);
                s.gloss_picker.set_type_label(gloss_type);
            }
            let mut s = state_clone.borrow_mut();
            s.gloss_picker.show();
            open_picker_mode(&mut s, crate::app::InputMode::GlossPicker);
        });
    }
}

/// Flip the gloss-picker type filter (teacher-generic <-> inner-monologue) and
/// re-query the current work's glossed passages for the newly selected type.
/// Called from the GlossPicker's Alt+t handler.
pub(crate) fn toggle_gloss_picker_type(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let filter = {
        let mut s = state.borrow_mut();
        s.gloss_picker_filter = s.gloss_picker_filter.next();
        s.gloss_picker_filter
    };
    // Normalize `-Amb` so the picker shares gloss rows with the base work.
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| crate::gloss::normalize_abbrev(&w.abbrev).to_string());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let gloss_type = filter.gloss_type();
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::find_glossed_passages(&conn, &abbrev, &[gloss_type])
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            let mut s = state_clone.borrow_mut();
            s.gloss_picker.set_items(items);
            s.gloss_picker.set_type_label(gloss_type);
        });
    }
}

/// Delete the selected bookmark from DB and update AppState's is_bookmarked
/// vec + gutter renderer. Called from the bookmark picker's Delete/d key.
pub(crate) fn delete_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let (Some(lm_id), Some(abbrev)) = (selected_id, abbrev) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db_rw()
                        .expect("Failed to open lit.db rw");
                    crate::db::queries::delete_bookmark(&conn, &abbrev, lm_id)
                })
                .await;
            if let Ok(Ok(())) = result {
                let mut s = state_clone.borrow_mut();
                // Update is_bookmarked vec
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
                    let mut bm = s.is_bookmarked.borrow_mut();
                    if bl < bm.len() {
                        bm[bl] = false;
                    }
                }
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
                s.bookmark_picker.remove_selected();
                if !s.bookmark_picker.has_items() {
                    s.bookmark_picker.hide();
                    s.input_mode = crate::app::InputMode::Reader;
                }
            }
        });
    }
}
