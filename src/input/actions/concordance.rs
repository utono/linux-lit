use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::{EditableExt, WidgetExt};

use crate::app::AppState;
use crate::input::highlight::update_highlight_and_center;
use crate::input::navigation;
use crate::input::navigation::SEEK_PREROLL;

/// Handle concordance word selection: query all hits across the author's works,
/// store them in ConcordanceState, and jump to the first hit in the current work.
pub(crate) fn handle_word_selection(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    word: String,
) {
    let author = {
        let s = state.borrow();
        s.current_work.as_ref().map(|w| w.author.clone()).unwrap_or_default()
    };
    let state_clone = Rc::clone(state);
    let handle = tokio_handle.clone();
    let word_clone = word.clone();
    let author_clone = author.clone();
    glib::spawn_future_local(async move {
        let hits = handle
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                crate::db::concordance::find_word_occurrences(&conn, &word_clone, &author_clone)
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
        if hits.is_empty() {
            return;
        }

        let current_abbrev = state_clone
            .borrow()
            .current_work
            .as_ref()
            .map(|w| w.abbrev.clone())
            .unwrap_or_default();

        let all_hits: Vec<crate::concordance::ConcordanceHit> = hits
            .into_iter()
            .map(|h| crate::concordance::ConcordanceHit {
                work_abbrev: h.work_abbrev,
                work_title: h.title,
                author: h.author,
                line_mapping_id: h.line_mapping_id,
                div1: h.div1,
                div2: h.div2,
                line_in_div: h.line_in_div,
                canonical_text: h.canonical_text,
                has_audio: h.has_audio,
            })
            .collect();

        if all_hits.is_empty() {
            return;
        }

        let current_line_id = {
            let s = state_clone.borrow();
            s.current_work.as_ref().and_then(|w| {
                let work_idx = if let Some(ref lm) = s.line_map {
                    lm.buffer_to_work.get(s.current_line).copied().flatten()
                } else {
                    Some(s.current_line)
                };
                work_idx.and_then(|wi| w.lines.get(wi).map(|l| l.id))
            })
        };

        // Check if cursor line is itself a hit
        let cursor_on_hit = current_line_id.map(|cur_id| {
            all_hits.iter().any(|h| h.work_abbrev == current_abbrev && h.line_mapping_id == cur_id)
        }).unwrap_or(false);

        let start_index = if let Some(cur_id) = current_line_id {
            if cursor_on_hit {
                // Cursor is on a hit — park index there, don't jump
                all_hits.iter()
                    .position(|h| h.work_abbrev == current_abbrev && h.line_mapping_id == cur_id)
                    .unwrap_or(0)
            } else {
                // Cursor is not on a hit — find next hit forward in current work
                all_hits.iter()
                    .position(|h| h.work_abbrev == current_abbrev && h.line_mapping_id > cur_id)
                    .or_else(|| all_hits.iter().position(|h| h.work_abbrev == current_abbrev))
                    .unwrap_or(0)
            }
        } else {
            all_hits.iter()
                .position(|h| h.work_abbrev == current_abbrev)
                .unwrap_or(0)
        };

        let mut conc_state = crate::concordance::ConcordanceState::new(
            word.clone(),
            all_hits,
        );
        conc_state.current_index = start_index;

        {
            let mut s = state_clone.borrow_mut();
            if s.concordance_origin.is_none() {
                let lm_id = s.line_mapping_id_for_buffer(s.current_line);
                let abbrev = s.current_work.as_ref().map(|w| w.abbrev.clone());
                if let (Some(lm_id), Some(abbrev)) = (lm_id, abbrev) {
                    crate::logging::log(&format!(
                        "CONC_ORIGIN: saved {}@lm{}",
                        abbrev, lm_id,
                    ));
                    s.concordance_origin = Some(crate::concordance::ConcordanceOrigin {
                        work_abbrev: abbrev,
                        line_mapping_id: lm_id,
                    });
                }
            }
            s.concordance_bar.update(&conc_state.status_label(), &conc_state.status_work());
            s.title_bar.set_visible(false);
            s.concordance_state = Some(conc_state);
            if s.sync_enabled_before_concordance.is_none() {
                s.sync_enabled_before_concordance = Some(s.sync_enabled);
                s.sync_enabled = false;
                crate::logging::log("CONC_SYNC: disabled playback sync for concordance mode");
            }
        }

        if !cursor_on_hit {
            crate::logging::log(&format!(
                "CONC_SELECT: cursor not on hit, jumping to index={} line_id={}",
                start_index,
                state_clone.borrow().concordance_state.as_ref()
                    .and_then(|c| c.current_hit().map(|h| h.line_mapping_id))
                    .unwrap_or(-1),
            ));
            concordance_jump_to_current(&state_clone, &handle);
        } else {
            crate::logging::log(&format!(
                "CONC_SELECT: cursor on hit, staying at index={}", start_index
            ));
        }
    });
}

/// Jump to the next vocab match. Always plain vocab jump (ignores concordance state).
pub(crate) fn jump_to_next_vocab(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    navigation::jump_to_next_vocab(&mut state.borrow_mut());
}

/// Jump to the previous vocab match. Always plain vocab jump (ignores concordance state).
pub(crate) fn jump_to_prev_vocab(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    navigation::jump_to_prev_vocab(&mut state.borrow_mut());
}

/// r: advance to next concordance hit. Shows toast if no concordance active.
pub(crate) fn concordance_next(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (old_index, old_work, total) = {
        let s = state.borrow();
        match &s.concordance_state {
            Some(c) => (
                c.current_index,
                c.current_hit().map(|h| h.work_abbrev.clone()).unwrap_or_default(),
                c.occurrences.len(),
            ),
            None => {
                show_no_concordance_toast(state);
                return;
            }
        }
    };
    let advanced = {
        let mut s = state.borrow_mut();
        s.concordance_state.as_mut().unwrap().advance()
    };
    if advanced {
        let (new_index, new_work, new_line_id) = {
            let s = state.borrow();
            let c = s.concordance_state.as_ref().unwrap();
            let h = c.current_hit().unwrap();
            (c.current_index, h.work_abbrev.clone(), h.line_mapping_id)
        };
        let cross_work = old_work != new_work;
        crate::logging::log(&format!(
            "CONC_NEXT: [{}/{}]->[{}/{}] work='{}'->'{}' line_id={} cross_work={}",
            old_index + 1, total, new_index + 1, total,
            old_work, new_work, new_line_id, cross_work,
        ));
        concordance_jump_to_current(state, tokio_handle);
    }
}

/// R: retreat to previous concordance hit. Shows toast if no concordance active.
pub(crate) fn concordance_prev(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (old_index, old_work, total) = {
        let s = state.borrow();
        match &s.concordance_state {
            Some(c) => (
                c.current_index,
                c.current_hit().map(|h| h.work_abbrev.clone()).unwrap_or_default(),
                c.occurrences.len(),
            ),
            None => {
                show_no_concordance_toast(state);
                return;
            }
        }
    };
    let retreated = {
        let mut s = state.borrow_mut();
        s.concordance_state.as_mut().unwrap().retreat()
    };
    if retreated {
        let (new_index, new_work, new_line_id) = {
            let s = state.borrow();
            let c = s.concordance_state.as_ref().unwrap();
            let h = c.current_hit().unwrap();
            (c.current_index, h.work_abbrev.clone(), h.line_mapping_id)
        };
        let cross_work = old_work != new_work;
        crate::logging::log(&format!(
            "CONC_PREV: [{}/{}]->[{}/{}] work='{}'->'{}' line_id={} cross_work={}",
            old_index + 1, total, new_index + 1, total,
            old_work, new_work, new_line_id, cross_work,
        ));
        concordance_jump_to_current(state, tokio_handle);
    }
}

/// n: next concordance hit in current work only. No-op if no hits in this work.
pub(crate) fn concordance_next_in_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
    let abbrev = match current_abbrev {
        Some(a) => a,
        None => return,
    };
    let advanced = {
        let mut s = state.borrow_mut();
        if let Some(conc) = s.concordance_state.as_mut() {
            conc.advance_in_work(&abbrev)
        } else {
            false
        }
    };
    if advanced {
        concordance_jump_to_current(state, tokio_handle);
    }
}

/// p: prev concordance hit in current work only. No-op if no hits in this work.
pub(crate) fn concordance_prev_in_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
    let abbrev = match current_abbrev {
        Some(a) => a,
        None => return,
    };
    let retreated = {
        let mut s = state.borrow_mut();
        if let Some(conc) = s.concordance_state.as_mut() {
            conc.retreat_in_work(&abbrev)
        } else {
            false
        }
    };
    if retreated {
        concordance_jump_to_current(state, tokio_handle);
    }
}

/// Open the concordance picker, populating it with all content words
/// from the current author's works (minus stopwords). Called from `Ctrl+\`.
pub(crate) fn open_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let author = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.author.clone());
    let author = match author {
        Some(a) => a,
        None => return,
    };

    let cached = state.borrow().concordance_word_cache.as_ref()
        .filter(|(a, _)| a == &author)
        .map(|(_, words)| words.clone());

    if let Some(words) = cached {
        let mut s = state.borrow_mut();
        s.concordance_picker.set_words(words);
        s.concordance_picker.show();
        crate::input::actions::pickers::open_picker_mode(&mut s, crate::app::InputMode::ConcordancePicker);
        drop(s);
        state.borrow().concordance_picker.search_entry().set_text("");
    } else {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        let author_clone = author.clone();
        glib::spawn_future_local(async move {
            let words = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::concordance::load_concordance_words(&conn, &author_clone)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.concordance_word_cache = Some((author.clone(), words.clone()));
                s.concordance_picker.set_words(words);
                s.concordance_picker.show();
                crate::input::actions::pickers::open_picker_mode(&mut s, crate::app::InputMode::ConcordancePicker);
            }
            state_clone.borrow().concordance_picker.search_entry().set_text("");
        });
    }
}

// ---------------------------------------------------------------------------
// Cross-work concordance navigation
// ---------------------------------------------------------------------------

/// Jump to the current concordance occurrence.
/// Loads the work in-place if different from current, positions cursor on the line.
pub fn concordance_jump_to_current(
    state: &Rc<RefCell<AppState>>,
    _handle: &tokio::runtime::Handle,
) {
    let (target_abbrev, target_line_id) = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        let hit = match conc.current_hit() {
            Some(h) => h,
            None => return,
        };
        (hit.work_abbrev.clone(), hit.line_mapping_id)
    };

    let current_abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());

    crate::logging::log(&format!(
        "CONC_JUMP: target_abbrev='{}' target_line_id={} current_abbrev={:?}",
        target_abbrev, target_line_id, current_abbrev
    ));

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        crate::logging::log(&format!(
            "CONC_JUMP: CROSS-WORK from '{}' to '{}' line_id={} — saving position",
            current_abbrev.as_deref().unwrap_or("?"), target_abbrev, target_line_id
        ));

        {
            let mut s = state.borrow_mut();
            s.concordance_resume_playback = s.mpv_playing;
            crate::app::save_position(&mut s);
        }

        let state_clone = Rc::clone(state);
        let abbrev_for_load = target_abbrev.clone();
        let target_abbrev_log = target_abbrev.clone();
        let handle = state.borrow().tokio_handle.clone();
        glib::spawn_future_local(async move {
            crate::logging::log(&format!(
                "CONC_JUMP: loading work '{}' from database...", target_abbrev_log
            ));
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev_for_load)?;
                    let prepared = crate::app::text_prep::prepare_text_for_display(&work);
                    Ok::<_, rusqlite::Error>((work, prepared))
                })
                .await;
            match result {
                Ok(Ok((work, prepared))) => {
                    let work_title = work.title.clone();
                    let lines_count = work.lines.len();
                    let ts_count = work.timestamps.len();
                    {
                        let mut s = state_clone.borrow_mut();
                        s.skip_mpv_discovery = true;
                        crate::app::clear_display(&mut s);
                        crate::app::display_work_at_with_prepared(
                            &mut s,
                            work,
                            Some(target_line_id),
                            prepared,
                        );
                        update_highlight_and_center(&mut s);
                    }
                    // Try to auto-select Arkangel media for Shakespeare
                    let auto_media = {
                        let s = state_clone.borrow();
                        s.current_work.as_ref().and_then(|w| {
                            w.media_paths.iter().zip(w.media_ids.iter())
                                .find(|(p, _)| p.contains("/aax-Arkangel/"))
                                .map(|(p, id)| (p.clone(), *id))
                        })
                    };

                    {
                        let s = state_clone.borrow();
                        concordance_update_bar(&s);
                        crate::logging::log(&format!(
                            "CONC_JUMP: CROSS-WORK loaded '{}' lines={} timestamps={} \
                             current_line={} page_top={} auto_media={:?}",
                            work_title, lines_count, ts_count,
                            s.current_line, s.page_top_line,
                            auto_media.as_ref().map(|(_, id)| id),
                        ));
                    }

                    if let Some((path, media_id)) = auto_media {
                        let already_connected = state_clone.borrow().mpv_connected;
                        let should_resume = state_clone.borrow().concordance_resume_playback;
                        let seek_time = if should_resume {
                            concordance_current_seek_time(&state_clone.borrow())
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
                                "CONC_JUMP: auto-selected Arkangel media_id={} path='{}'", media_id, path
                            ));
                        }
                        {
                            let mut s = state_clone.borrow_mut();
                            s.media_id = Some(media_id);
                            s.concordance_resume_playback = false;
                        }
                    } else {
                        let handle_for_picker = state_clone.borrow().tokio_handle.clone();
                        crate::input::actions::pickers::open_media_picker(
                            &state_clone, &handle_for_picker,
                        );
                    }
                }
                Ok(Err(e)) => {
                    crate::logging::log(&format!("CONC_JUMP: load_work error: {}", e));
                }
                Err(e) => {
                    crate::logging::log(&format!("CONC_JUMP: spawn_blocking error: {}", e));
                }
            }
        });
    } else {
        crate::logging::log("CONC_JUMP: same work, positioning cursor");
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
    }
}

/// Resolve the buffer index and sentence-start work index for a given line_mapping_id.
fn concordance_resolve_indices(state: &AppState, line_mapping_id: i64) -> Option<(usize, usize)> {
    let work = state.current_work.as_ref()?;
    let work_idx = work.lines.iter().position(|l| l.id == line_mapping_id)?;

    let buf_idx = if let Some(ref lm) = state.line_map {
        let bi = lm.work_to_buffer[work_idx];
        if lm.buffer_to_work.get(bi) != Some(&Some(work_idx)) {
            return None;
        }
        bi
    } else {
        work_idx
    };

    let seek_work_idx = if let Some(ref lm) = state.line_map {
        if let Some(sg) = crate::text_file_map::sentence_group_for(&lm.sentence_groups, buf_idx) {
            lm.buffer_to_work.get(sg.line_range.start)
                .copied()
                .flatten()
                .unwrap_or(work_idx)
        } else {
            find_sentence_start_by_timestamp(work, work_idx)
        }
    } else {
        find_sentence_start_by_timestamp(work, work_idx)
    };

    Some((buf_idx, seek_work_idx))
}

/// Compute the seek time for the current concordance hit without side effects.
pub(crate) fn concordance_current_seek_time(state: &AppState) -> Option<f64> {
    let line_id = state.concordance_state.as_ref()
        .and_then(|c| c.current_hit().map(|h| h.line_mapping_id))?;
    let work = state.current_work.as_ref()?;
    let line = work.lines.iter().find(|l| l.id == line_id)?;
    let ts = line.timestamp.as_ref()?;
    Some((ts.start - SEEK_PREROLL).max(0.0))
}



/// Seek MPV to the hit line's own start time (not sentence start).
/// Brief sync suppression to let the seek command reach MPV before
/// CursorSync tries to advance the cursor.
fn concordance_seek(state: &mut AppState, line_mapping_id: i64) {
    if let Some(work) = &state.current_work {
        if let Some(line) = work.lines.iter().find(|l| l.id == line_mapping_id) {
            if let Some(ts) = line.timestamp.as_ref() {
                state.suppress_sync_until =
                    Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
                let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
                let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
            }
        }
    }
}

/// Position cursor on the line with the given line_mapping_id (same-work case).
/// The buffer layout is valid, so we scroll immediately.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    let (buf_idx, _seek_work_idx) = match concordance_resolve_indices(state, line_mapping_id) {
        Some(v) => v,
        None => {
            crate::logging::log(&format!(
                "CONC_POS: resolve failed for line_mapping_id={}", line_mapping_id
            ));
            return;
        }
    };

    let conc_word = state.concordance_state.as_ref()
        .map(|c| c.word.clone())
        .unwrap_or_default();
    let line_text = state.current_work.as_ref()
        .and_then(|w| w.lines.iter().find(|l| l.id == line_mapping_id))
        .map(|l| l.text.clone())
        .unwrap_or_default();
    let contains_word = line_text.to_lowercase().contains(&conc_word.to_lowercase());

    state.current_line = buf_idx;
    update_highlight_and_center(state);

    let hit_seek_time = state.current_work.as_ref()
        .and_then(|w| w.lines.iter().find(|l| l.id == line_mapping_id))
        .and_then(|l| l.timestamp.as_ref())
        .map(|ts| (ts.start - SEEK_PREROLL).max(0.0));

    let lpp = crate::input::viewport::lines_per_page(state);
    let cursor_offset = state.current_line.saturating_sub(state.page_top_line);
    crate::logging::log(&format!(
        "CONC_POS: word='{}' line_id={} buf_idx={} contains_word={} \
         seek_time={} page_top={} cursor={} lpp={} offset_from_top={} \
         text='{}'",
        conc_word, line_mapping_id, buf_idx, contains_word,
        hit_seek_time.map(|t| format!("{:.1}", t)).unwrap_or_else(|| "NONE".to_string()),
        state.page_top_line, state.current_line, lpp, cursor_offset,
        if line_text.len() > 80 { &line_text[..80] } else { &line_text },
    ));

    concordance_seek(state, line_mapping_id);
}

/// Find the first work-line index sharing the same sentence_start_time as `work_idx`.
/// Falls back to `work_idx` itself if no sentence time data is available.
fn find_sentence_start_by_timestamp(work: &crate::db::models::Work, work_idx: usize) -> usize {
    let target_ss = work.lines[work_idx]
        .timestamp
        .as_ref()
        .and_then(|t| t.sentence_start);
    let target_ss = match target_ss {
        Some(ss) => ss,
        None => return work_idx,
    };
    // Scan backwards to find the first line with the same sentence_start_time
    for i in (0..work_idx).rev() {
        let ss = work.lines[i]
            .timestamp
            .as_ref()
            .and_then(|t| t.sentence_start);
        match ss {
            Some(s) if (s - target_ss).abs() < 0.001 => continue,
            _ => return i + 1,
        }
    }
    0
}

/// Update the concordance status bar from current state.
fn concordance_update_bar(state: &AppState) {
    if let Some(conc) = &state.concordance_state {
        state
            .concordance_bar
            .update(&conc.status_label(), &conc.status_work());
    }
}

/// Restore playback sync to its pre-concordance state.
/// Call when clearing concordance_state.
pub(crate) fn restore_sync_after_concordance(state: &mut AppState) {
    if let Some(was_enabled) = state.sync_enabled_before_concordance.take() {
        state.sync_enabled = was_enabled;
        crate::logging::log(&format!(
            "CONC_SYNC: restored playback sync to {}",
            if was_enabled { "enabled" } else { "disabled" }
        ));
    }
}

fn show_no_concordance_toast(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    crate::ui::toast::show_transient(&s.chapter_toast, "No concordance active — press Ctrl+\\ to pick a word", 3);
}
