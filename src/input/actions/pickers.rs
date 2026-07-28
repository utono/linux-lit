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
    SyntaxGloss,
    #[default]
    ReaderGloss,
}

impl GlossPickerFilter {
    pub(crate) fn gloss_type(self) -> &'static str {
        match self {
            GlossPickerFilter::TeacherGeneric => "teacher-generic",
            GlossPickerFilter::InnerMonologue => "inner-monologue",
            GlossPickerFilter::SyntaxGloss => "syntax-gloss",
            GlossPickerFilter::ReaderGloss => "reader-gloss",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            GlossPickerFilter::TeacherGeneric => GlossPickerFilter::InnerMonologue,
            GlossPickerFilter::InnerMonologue => GlossPickerFilter::SyntaxGloss,
            GlossPickerFilter::SyntaxGloss => GlossPickerFilter::ReaderGloss,
            GlossPickerFilter::ReaderGloss => GlossPickerFilter::TeacherGeneric,
        }
    }
}

/// Which slice of the journal the Q&A picker lists. Cycled in place with
/// Alt+t while the picker is open (the same bind the gloss picker uses for
/// its type filter). Tightest -> widest, wrapping.
///
/// NOT persisted: the picker always opens on `Work`, which is what it
/// listed before scopes existed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum JournalPickerScope {
    /// Entries anchored to the cursor's (div1, div2) band — division + passage.
    Division,
    /// Every entry for the current work. The default, and the pre-scope
    /// behavior.
    #[default]
    Work,
    /// Every entry from every work by this work's author, plus that
    /// author's corpus notes.
    Author,
}

impl JournalPickerScope {
    pub(crate) fn next(self) -> Self {
        match self {
            JournalPickerScope::Division => JournalPickerScope::Work,
            JournalPickerScope::Work => JournalPickerScope::Author,
            JournalPickerScope::Author => JournalPickerScope::Division,
        }
    }

    /// Suffix for the picker header, so the active scope is always visible.
    ///
    /// The tightest scope is named with the WORK'S OWN division noun, taken
    /// from `gloss::genre_unit` (the single source for genre vocabulary), so a
    /// novel reads CHAPTER and an epic BOOK rather than the play-only "SCENE".
    /// This mirrors `division_label`, which already renders the rows that way.
    ///
    /// BOTH work-local scopes carry the abbrev (`CHAPTER — BH-Barrett`,
    /// `WORK — BH-Barrett`), because neither one's rows show a work column —
    /// only the cross-work ALL scope does. Without it the title says what KIND
    /// of division you are browsing but not which work's. An empty abbrev (no
    /// work loaded) degrades to the bare noun rather than a dangling dash.
    ///
    /// `Author` is titled ALL because that scope is the widest list the picker
    /// can show — every work by the author plus the corpus notes — and reads as
    /// a superset, not as a claim about authorship. It takes NO abbrev: its
    /// rows span works, so naming one would be wrong.
    pub(crate) fn label(self, work_type: &str, work_abbrev: &str) -> String {
        let with_work = |noun: String| {
            if work_abbrev.is_empty() {
                noun
            } else {
                format!("{noun} — {work_abbrev}")
            }
        };
        match self {
            JournalPickerScope::Division => {
                with_work(crate::gloss::genre_unit(work_type).1.to_uppercase())
            }
            JournalPickerScope::Work => with_work("WORK".to_string()),
            JournalPickerScope::Author => "ALL".to_string(),
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
                        crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
                        let prep = crate::app::text_prep::PreparedText {
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
                        (crate::app::text_prep::prepare_text_for_display(&work), true)
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
                            s.current_line, s.page_top.line(), s.line_map.is_some(), s.effective_line_count()
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
    let buf_idx = crate::input::navigation::buffer_line_for_line_id(&s, line_mapping_id);
    if let Some(idx) = buf_idx {
        s.current_line = idx;
        crate::input::highlight::update_highlight_and_center(&mut s);
    }
}

/// Load the Arkangel edition of `base_abbrev` (falling back to the base work
/// when no `-Arkangel` sibling exists) off the reader thread, then run
/// `on_ready`. When the reader is ALREADY on the resolved target edition
/// (`current_abbrev` == the resolved target), the work load is skipped entirely
/// and `on_ready` runs immediately. Otherwise the target work is loaded with
/// `skip_mpv_discovery = false` so MPV discovery starts the target edition's
/// media (the Arkangel `.m4b`), matching the Ctrl+\ library-picker load; then
/// `on_ready` runs. On DB/load error a "Could not load {base_abbrev}" toast is
/// shown and `on_ready` does NOT run.
///
/// `on_ready(&state)` is the caller's post-load action — open an overlay, seed a
/// search, jump the cursor to a source line — and runs in BOTH the same-work and
/// cross-work success paths, so a caller writes it once. It must re-borrow
/// `state` itself (no borrow is held when it runs).
///
/// Shared by the Ctrl+f corpus-search select and the term-filter Escape jump —
/// the two "open the Arkangel edition of a work I picked" surfaces.
pub(crate) fn load_arkangel_edition_then<F>(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    base_abbrev: String,
    current_abbrev: Option<String>,
    on_ready: F,
) where
    F: FnOnce(&Rc<RefCell<AppState>>) + 'static,
{
    let state_clone = Rc::clone(state);
    let handle = tokio_handle.clone();
    glib::spawn_future_local(async move {
        let base_for_load = base_abbrev.clone();
        let current_for_load = current_abbrev.clone();
        let result = handle
            .spawn_blocking(move || {
                let conn =
                    crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
                // Prefer the Arkangel edition; fall back to the base abbrev.
                let target = crate::db::queries::preferred_arkangel_abbrev(&conn, &base_for_load);
                // Already on the target edition -> skip the work load entirely.
                if current_for_load.as_deref() == Some(target.as_str()) {
                    return Ok::<_, rusqlite::Error>(None);
                }
                let work = crate::db::queries::load_work(&conn, &target)?;
                let prepared = crate::app::text_prep::prepare_text_for_display(&work);
                Ok::<_, rusqlite::Error>(Some((work, prepared)))
            })
            .await;
        match result {
            // Already on the target edition: no reload, run the ready action.
            Ok(Ok(None)) => on_ready(&state_clone),
            // Cross-work: load the target edition (MPV discovery loads its media),
            // then run the ready action.
            Ok(Ok(Some((work, prepared)))) => {
                {
                    let mut s = state_clone.borrow_mut();
                    s.skip_mpv_discovery = false;
                    crate::app::clear_display(&mut s);
                    crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                }
                on_ready(&state_clone);
            }
            _ => {
                let s = state_clone.borrow();
                crate::input::navigation::show_chapter_toast_secs(
                    &s,
                    &format!("Could not load {}", base_abbrev),
                    3,
                );
            }
        }
    });
}

pub(crate) fn load_work_at(
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
                let conn = crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
                    let prep = crate::app::text_prep::PreparedText {
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
                    (crate::app::text_prep::prepare_text_for_display(&work), true)
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
                        crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
            // Fetch the media list AND whether a lone item is a multi-work
            // bundle in the same blocking DB hop.
            let (items, single_is_bundle) = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
                    let items = crate::db::queries::list_media_for_work(&conn, &abbrev)
                        .unwrap_or_default();
                    let single_is_bundle = items.len() == 1
                        && crate::db::queries::is_bundle_media(&conn, items[0].media_id);
                    (items, single_is_bundle)
                })
                .await
                .unwrap_or_default();
            crate::logging::log(&format!(
                "MEDIA_PICKER: work='{}' found {} media files", abbrev_log, items.len()
            ));
            // Auto-select only a single DEDICATED media. A single multi-work
            // bundle (e.g. Rom's Hamlet+Macbeth+Romeo m4b) would auto-load the
            // wrong play, so fall through to showing the picker instead.
            if items.len() == 1 && single_is_bundle {
                crate::logging::log(
                    "MEDIA_PICKER: single media is a multi-work bundle — showing picker",
                );
            }
            if items.len() == 1 && !single_is_bundle {
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
                            crate::mpv::discovery::discover_or_launch_blocking(&path_for_discover)
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
                        crate::mpv::discovery::discover_or_launch_blocking(&path_for_discover)
                    })
                    .await
                    .unwrap_or_default()
            };

            {
                let mut s = state_clone.borrow_mut();
                s.media_id = Some(media_id);
                // Rebuild per-line timestamps for the new media_id
                if let Some(ref mut work) = s.current_work {
                    // Build the timestamp lookup from work.timestamps for the new media.
                    let mut ts_map: std::collections::HashMap<i64, crate::db::models::TimeRange> =
                        std::collections::HashMap::new();
                    for ts in &work.timestamps {
                        if ts.media_id == media_id {
                            ts_map.entry(ts.line_id).or_insert(crate::db::models::TimeRange {
                                start: ts.start,
                                end: ts.end,
                                sentence_start: ts.sentence_start,
                                is_manual: ts.is_manual,
                            });
                        }
                    }
                    for line in &mut work.lines {
                        line.timestamp = ts_map.get(&line.id).copied();
                    }
                    // Structural chapter flags follow div1 boundaries, NOT the new
                    // media's track marks — re-mark so switching media never moves them.
                    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
                    crate::text_file_map::mark_chapter_starts(&mut work.lines, is_prose);
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
        s.vocab_popup.popup.hide();
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
    // Glosses are stored under the canonical base abbrev, so key the picker by
    // it too — otherwise the picker is empty for variant editions.
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let gloss_type = GlossPickerFilter::default().gloss_type();
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
    // Canonical base abbrev so the picker shares gloss rows across editions.
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let gloss_type = filter.gloss_type();
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
                let buffer_line =
                    crate::input::navigation::buffer_line_for_line_id(&s, lm_id);
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

/// The author column's display form: the LAST WORD of the stored author name.
///
/// `"Charles Dickens"` → `"Dickens"`; a mononym like `"Shakespeare"` is its
/// own surname. Deliberately simple — correct for every author in lit.db, and
/// the picker needs the horizontal space for the entry's identifying line. It
/// would mis-split a particle surname ("van Gogh"), which does not occur.
pub(crate) fn author_surname(author: &str) -> &str {
    author.split_whitespace().last().unwrap_or("")
}

/// The picker's division column, in the DIVISION NOUN THE WORK ITSELF USES.
///
/// A prose work's chapters live in `div1` alone — `div2` is always 0 in lit.db
/// and carries no meaning — so showing "2.0" was noise; it reads "Ch. 2".
/// Plays keep `act.scene` numerals, where both numbers are meaningful.
///
/// `div1 <= 0` on a prose work is the FRONT MATTER, not a chapter zero: it
/// renders "Preface", the same convention `division_synopsis::chapter_label`
/// already uses for the synopsis overlay. Six live entries have `div1 = 0`
/// and would otherwise read "Ch. 0".
///
/// Takes the ROW'S OWN `work_type`, not the loaded work's: author scope is
/// cross-work, so a play row and a novel row appear in the same list and must
/// be labelled by their own kind. That is why this is a pure string predicate
/// rather than `division_synopsis::is_chapter_work`, which reads `AppState`.
///
/// The noun comes from `gloss::genre_unit`, the SAME source the picker header
/// uses (`JournalPickerScope::label`), so a row and the title above it can
/// never disagree — an epic's header reading BOOK while its rows read "3.0"
/// was the split this unification removed.
///
/// TWO-LEVEL vs ONE-LEVEL is decided by whether the work type actually uses
/// `div2`, measured against lit.db: only `play` (530804 non-zero div2 lines),
/// `bible_book` (1754) and `anthology` (1334) do. Every other type has `div2`
/// identically 0, so printing it was pure noise. A two-level work keeps bare
/// `act.scene` numerals, where both numbers carry meaning and a noun would
/// crowd the column.
pub(crate) fn division_label(work_type: &str, div1: i64, div2: i64) -> String {
    // Sentinel divisions have no numeric address to show: (-1,-1) is a
    // whole-work entry, (-2,-2) a corpus note. Printing "-2.-2" is noise —
    // their TYPE column already says `work` / `author`.
    if div1 < 0 {
        return String::new();
    }
    if uses_two_level_divisions(work_type) {
        return format!("{div1}.{div2}");
    }
    // Division 0 on a one-level work is the FRONT MATTER, not a chapter zero —
    // the same convention `division_synopsis::chapter_label` already uses.
    if div1 == 0 {
        return "Preface".to_string();
    }
    // Title-case the genre noun for display: "chapter" -> "Chapter 2".
    let unit = crate::gloss::genre_unit(work_type).1;
    let mut noun = unit.chars();
    let titled: String = match noun.next() {
        Some(c) => c.to_uppercase().collect::<String>() + noun.as_str(),
        None => String::new(),
    };
    format!("{titled} {div1}")
}

/// Whether `work_type` addresses its lines with BOTH div1 and div2.
///
/// Measured against lit.db rather than assumed: only plays (act.scene), bible
/// books (chapter.verse) and anthologies have any non-zero `div2`. Everything
/// else is single-level, so showing the second number is noise.
fn uses_two_level_divisions(work_type: &str) -> bool {
    matches!(work_type, "play" | "bible_book" | "anthology")
}

/// Alt+t inside the Q&A picker: advance the scope, rebuild the list, retitle
/// the header. Selection resets to the first row and the filter is cleared —
/// the row sets differ between scopes, so preserving either is meaningless,
/// and a stale filter silently hiding a scope's contents reads as "empty".
pub(crate) fn cycle_journal_picker_scope(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal_picker_scope = s.journal_picker_scope.next();
    crate::input::actions::journal::repopulate_picker_for_scope(&mut s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_filter_cycles_through_every_type_including_syntax() {
        // Alt+t must reach syntax-gloss, and the cycle must return to its
        // start — a filter that cannot be cycled back to is unreachable.
        let start = GlossPickerFilter::default();
        let mut seen = vec![start.gloss_type()];
        let mut f = start.next();
        while f != start {
            seen.push(f.gloss_type());
            f = f.next();
        }
        assert!(seen.contains(&"syntax-gloss"), "cycle must reach it: {seen:?}");
        assert!(seen.contains(&"reader-gloss"));
        assert!(seen.contains(&"teacher-generic"));
        assert!(seen.contains(&"inner-monologue"));
        assert_eq!(seen.len(), 4, "one entry per type, no duplicates: {seen:?}");
    }

    #[test]
    fn author_surname_takes_the_last_word() {
        use super::author_surname;
        assert_eq!(author_surname("Charles Dickens"), "Dickens");
        assert_eq!(author_surname("Diarmaid MacCulloch"), "MacCulloch");
        // A mononym is its own surname — Shakespeare is the dominant corpus.
        assert_eq!(author_surname("Shakespeare"), "Shakespeare");
        assert_eq!(author_surname(""), "");
        assert_eq!(author_surname("  Jonathan  Swift  "), "Swift");
    }

    /// The division column uses the work's OWN division noun.
    #[test]
    fn division_label_uses_the_works_own_noun() {
        use super::division_label;
        // Two-level works keep bare numerals: both numbers carry meaning.
        assert_eq!(division_label("play", 1, 4), "1.4");
        assert_eq!(division_label("play", 4, 10), "4.10");
        assert_eq!(division_label("bible_book", 3, 16), "3.16");
        assert_eq!(division_label("anthology", 2, 5), "2.5");
        // One-level works take their OWN genre noun, from the same
        // genre_unit the picker header uses — so a row can never disagree
        // with the title above it.
        assert_eq!(division_label("prose", 2, 0), "Chapter 2");
        assert_eq!(division_label("prose_book", 10, 0), "Chapter 10");
        assert_eq!(division_label("epic", 3, 0), "Book 3");
        assert_eq!(division_label("sonnet_sequence", 18, 0), "Sonnet 18");
        assert_eq!(division_label("essay_collection", 4, 0), "Essay 4");
        // Unknown type degrades to the neutral noun, never a wrong specific one.
        assert_eq!(division_label("", 3, 0), "Section 3");
        // Division 0 on a one-level work is FRONT MATTER, never "Chapter 0".
        // Six live entries hit this.
        assert_eq!(division_label("prose", 0, 0), "Preface");
        assert_eq!(division_label("epic", 0, 0), "Preface");
        // Sentinel divisions carry no numeric address: a whole-work entry
        // (-1,-1) and a corpus note (-2,-2) show nothing rather than "-2.-2".
        // Their TYPE column already reads `work` / `author`.
        assert_eq!(division_label("prose", -1, -1), "");
        assert_eq!(division_label("", -2, -2), "");
        assert_eq!(division_label("play", -1, -1), "");
    }

    /// Tightest -> widest, wrapping. Mirrors GlossPickerFilter's cycle test.
    #[test]
    fn journal_picker_scope_cycles_scene_work_author() {
        use super::JournalPickerScope as S;
        assert_eq!(S::Division.next(), S::Work);
        assert_eq!(S::Work.next(), S::Author);
        assert_eq!(S::Author.next(), S::Division);
    }

    /// The header names the tightest scope with the WORK'S OWN division noun,
    /// so a novel reads CHAPTER where a play reads SCENE. Work scope names the
    /// work; the widest scope is ALL.
    #[test]
    fn journal_picker_scope_label_uses_the_works_own_noun() {
        use super::JournalPickerScope as S;
        // Prose: the screenshot case — Bleak House is chaptered, not scened.
        assert_eq!(S::Division.label("prose", "BH-Barrett"), "CHAPTER — BH-Barrett");
        assert_eq!(S::Division.label("prose_book", "BH-Barrett"), "CHAPTER — BH-Barrett");
        // Plays keep SCENE, the pre-change noun.
        assert_eq!(S::Division.label("play", "Cym"), "SCENE — Cym");
        // Other genres follow genre_unit rather than a prose/verse binary.
        assert_eq!(S::Division.label("epic", "Il"), "BOOK — Il");
        assert_eq!(S::Division.label("sonnet_sequence", "Son"), "SONNET — Son");
        // Unknown/blank work_type degrades to the neutral noun, never a
        // wrong-but-specific one.
        assert_eq!(S::Division.label("", "X"), "SECTION — X");
        // No work loaded: the bare noun, never a dangling dash.
        assert_eq!(S::Division.label("prose", ""), "CHAPTER");

        // Work scope names the work, whose column the rows omit in this scope.
        assert_eq!(S::Work.label("prose", "BH-Barrett"), "WORK — BH-Barrett");
        assert_eq!(S::Work.label("play", "Cym"), "WORK — Cym");
        // No abbrev (no work loaded) degrades to a bare WORK, not "WORK — ".
        assert_eq!(S::Work.label("prose", ""), "WORK");

        // The widest scope is ALL regardless of work or genre.
        assert_eq!(S::Author.label("prose", "BH-Barrett"), "ALL");
        assert_eq!(S::Author.label("play", "Cym"), "ALL");
        assert_eq!(S::Author.label("", ""), "ALL");
    }

    /// The picker always OPENS on Work — today's behavior, so existing
    /// muscle memory is untouched.
    #[test]
    fn journal_picker_scope_defaults_to_work() {
        assert_eq!(super::JournalPickerScope::default(), super::JournalPickerScope::Work);
    }

    /// The cycle handler advances the scope: default (Work) -> Author.
    /// The bind reaches Author scope, proving the handler wiring is intact.
    #[test]
    fn journal_picker_scope_default_next_is_author() {
        let start = super::JournalPickerScope::default();
        assert_eq!(start.next(), super::JournalPickerScope::Author);
    }
}
