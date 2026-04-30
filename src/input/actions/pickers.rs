use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

/// Load the selected work in the library picker, hide the picker, and
/// display the new work. Spawns an async task to query the DB.
pub(crate) fn load_selected_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state.borrow().picker.selected_abbrev();
    if let Some(abbrev) = abbrev {
        crate::logging::log(&format!("PICKER: selected work '{}'", abbrev));
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::commands::MpvCommand::Pause);
            s.picker.hide();
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
            state_clone.borrow().bookmark_picker.show();
            state_clone.borrow_mut().input_mode = crate::app::InputMode::BookmarkPicker;
        });
    }
}

/// Open the media picker, querying media files for the current work.
/// Spawns an async task to query the DB.
pub(crate) fn open_media_picker(
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
                    crate::db::queries::list_media_for_work(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.gloss_overlay.hide();
                s.media_picker.set_items(items);
            }
            state_clone.borrow().media_picker.show();
            state_clone.borrow_mut().input_mode = crate::app::InputMode::MediaPicker;
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
    if let (Some(path), Some(media_id)) = (selected_path, selected_id) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let socket_path = handle
                .spawn_blocking(move || {
                    if let Some((sock, _)) =
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
                // Re-send timestamps filtered by new media_id
                if let Some(ref work) = s.current_work {
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
                let _ = s
                    .cmd_tx
                    .try_send(crate::mpv::MpvCommand::Connect(socket_path));
                s.media_picker.hide();
                s.input_mode = crate::app::InputMode::Reader;
                crate::logging::log(&format!(
                    "MEDIA: switched to media_id={}",
                    media_id
                ));
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
