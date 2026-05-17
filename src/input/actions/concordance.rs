use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::EditableExt;

use crate::app::AppState;
use crate::input::highlight::update_highlight;
use crate::input::navigation;
use crate::input::navigation::SEEK_PREROLL;
use crate::input::scroll::center_cursor;

/// Handle concordance word selection: partition hits by work, set up same-work
/// concordance state, and spawn new instances for other works.
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

        // Partition hits by work
        let mut current_work_hits = Vec::new();
        let mut other_works: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();

        for h in hits {
            if h.work_abbrev == current_abbrev {
                current_work_hits.push(crate::concordance::ConcordanceHit {
                    work_abbrev: h.work_abbrev,
                    work_title: h.title,
                    author: h.author,
                    line_mapping_id: h.line_mapping_id,
                    div1: h.div1,
                    div2: h.div2,
                    line_in_div: h.line_in_div,
                    canonical_text: h.canonical_text,
                    has_audio: h.has_audio,
                });
            } else {
                // Keep only the first hit per other work
                other_works.entry(h.work_abbrev.clone()).or_insert(h.line_mapping_id);
            }
        }

        // Spawn a new instance for each other work
        if !other_works.is_empty() {
            let exe = std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("target/debug/linux-lit"));
            for (abbrev, line_id) in &other_works {
                crate::logging::log(&format!(
                    "CONC_SPAWN: work='{}' line_id={}", abbrev, line_id
                ));
                let _ = std::process::Command::new(&exe)
                    .env("LINUX_LIT_WORK", abbrev)
                    .env("LINUX_LIT_LINE_ID", line_id.to_string())
                    .env("LINUX_LIT_CONC_WORD", &word)
                    .spawn();
            }
        }

        // Set up concordance state for current work's hits (if any)
        if !current_work_hits.is_empty() {
            let conc_state = crate::concordance::ConcordanceState::new(
                word.clone(),
                current_work_hits,
            );
            let mut s = state_clone.borrow_mut();
            s.concordance_bar.update(&conc_state.status_label(), &conc_state.status_work());
            s.concordance_state = Some(conc_state);
            drop(s);
            concordance_jump_to_current(&state_clone, &handle);
        }
    });
}

/// Jump to the next vocab match, or advance the concordance within the
/// current work if concordance mode is active.
pub(crate) fn jump_to_next_vocab(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let advanced = {
            let mut s = state.borrow_mut();
            if let Some(conc) = s.concordance_state.as_mut() {
                conc.advance()
            } else {
                false
            }
        };
        if advanced {
            concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        navigation::jump_to_next_vocab(&mut state.borrow_mut());
    }
}

/// Jump to the previous vocab match, or retreat the concordance within the
/// current work if concordance mode is active.
pub(crate) fn jump_to_prev_vocab(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let retreated = {
            let mut s = state.borrow_mut();
            if let Some(conc) = s.concordance_state.as_mut() {
                conc.retreat()
            } else {
                false
            }
        };
        if retreated {
            concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        navigation::jump_to_prev_vocab(&mut state.borrow_mut());
    }
}

/// Open the concordance picker, populating it with the current work's vocab
/// words. Called from `Ctrl+\`.
pub(crate) fn open_picker(
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
            let words = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::load_vocab_word_list(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.concordance_picker.set_words(words);
                s.concordance_picker.show();
                s.input_mode = crate::app::InputMode::ConcordancePicker;
            }
            // set_text triggers connect_changed which borrows state, so the
            // mutable borrow must be dropped first.
            state_clone.borrow().concordance_picker.search_entry().set_text("");
        });
    }
}

// ---------------------------------------------------------------------------
// Cross-work concordance navigation
// ---------------------------------------------------------------------------

/// Jump to the current concordance occurrence.
/// Loads the work if different from current, positions cursor on the line.
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
        // Cross-work jump: spawn a new instance of linux-lit with the target work/line.
        // This avoids GTK lazy layout issues when loading large buffers in-process.
        crate::logging::log(&format!(
            "CONC_JUMP: spawning new instance for '{}' line_id={}", target_abbrev, target_line_id
        ));

        // Pause current MPV
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
        }

        let exe = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("target/debug/linux-lit"));
        let _ = std::process::Command::new(exe)
            .env("LINUX_LIT_WORK", &target_abbrev)
            .env("LINUX_LIT_LINE_ID", target_line_id.to_string())
            .spawn();
    } else {
        // Same work, just move cursor
        crate::logging::log("CONC_JUMP: same work, positioning cursor");
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
        crate::logging::log(&format!(
            "CONC_JUMP: positioned current_line={} page_top={}",
            s.current_line, s.page_top_line
        ));
        drop(s);
    }
}

/// Resolve the buffer index and sentence-start work index for a given line_mapping_id.
fn concordance_resolve_indices(state: &AppState, line_mapping_id: i64) -> Option<(usize, usize)> {
    let work = state.current_work.as_ref()?;
    let work_idx = work.lines.iter().position(|l| l.id == line_mapping_id)?;

    let buf_idx = if let Some(ref lm) = state.line_map {
        lm.work_to_buffer[work_idx]
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

/// Seek MPV to the sentence start for a concordance hit.
fn concordance_seek(state: &mut AppState, seek_work_idx: usize) {
    if let Some(work) = &state.current_work {
        if let Some(ts) = work.lines.get(seek_work_idx).and_then(|l| l.timestamp.as_ref()) {
            state.suppress_sync_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
            let seek_time = (ts.start - SEEK_PREROLL).max(0.0);
            let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
        }
    }
}

/// Position cursor on the line with the given line_mapping_id (same-work case).
/// The buffer layout is valid, so we scroll immediately.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    let (buf_idx, seek_work_idx) = match concordance_resolve_indices(state, line_mapping_id) {
        Some(v) => v,
        None => {
            crate::logging::log(&format!(
                "CONC_POS: resolve failed for line_mapping_id={}", line_mapping_id
            ));
            return;
        }
    };
    crate::logging::log(&format!(
        "CONC_POS: same-work buf_idx={} seek_work_idx={} line_mapping_id={}",
        buf_idx, seek_work_idx, line_mapping_id
    ));
    state.current_line = buf_idx;
    update_highlight(state);
    center_cursor(state);
    concordance_seek(state, seek_work_idx);
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
