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
            s.correction_overlay.show_loading_message("Loading...");
        }
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let t_db = std::time::Instant::now();
            let abbrev_for_log = abbrev.clone();
            let result = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev)?;
                    let prepared = crate::app::prepare_text_for_display(&work);
                    Ok::<_, rusqlite::Error>((work, prepared))
                })
                .await;
            crate::logging::log(&format!("PICKER: load_work '{}' DB query {:.0}ms", abbrev_for_log, t_db.elapsed().as_millis()));
            match result {
                Ok(Ok((work, prepared))) => {
                    crate::logging::log(&format!(
                        "PICKER: loaded '{}' lines={} timestamps={} text_file={:?}",
                        work.abbrev, work.lines.len(), work.timestamps.len(), work.text_file.is_some()
                    ));
                    {
                        let mut s = state_clone.borrow_mut();
                        s.correction_overlay.hide();
                        crate::app::clear_display(&mut s);
                        crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                        crate::logging::log(&format!(
                            "PICKER: after display_work current_line={} page_top={} line_map={} effective_lines={}",
                            s.current_line, s.page_top_line, s.line_map.is_some(), s.effective_line_count()
                        ));
                    }
                }
                Ok(Err(e)) => {
                    crate::logging::log(&format!("PICKER: load_work error: {}", e));
                    let s = state_clone.borrow();
                    s.correction_overlay.hide();
                }
                Err(e) => {
                    crate::logging::log(&format!("PICKER: task join error: {}", e));
                    let s = state_clone.borrow();
                    s.correction_overlay.hide();
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
                s.correction_overlay.hide();
                s.bookmark_picker.set_items(items);
            }
            state_clone.borrow().bookmark_picker.show();
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
                s.correction_overlay.hide();
                s.media_picker.set_items(items);
            }
            state_clone.borrow().media_picker.show();
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
                crate::logging::log(&format!(
                    "MEDIA: switched to media_id={}",
                    media_id
                ));
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
                }
            }
        });
    }
}
