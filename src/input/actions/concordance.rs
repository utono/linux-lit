use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::EditableExt;

use crate::app::AppState;
use crate::input::navigation;

/// Handle concordance word selection: partition hits by work, set up same-work
/// concordance state, and spawn new instances for other works.
pub(crate) fn handle_word_selection(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    word: String,
) {
    let state_clone = Rc::clone(state);
    let handle = tokio_handle.clone();
    let word_clone = word.clone();
    glib::spawn_future_local(async move {
        let hits = handle
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                crate::db::concordance::find_word_occurrences(&conn, &word_clone)
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
            navigation::concordance_jump_to_current(&state_clone, &handle);
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
        let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
        let advanced = {
            let mut s = state.borrow_mut();
            if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                conc.advance_within_work(abbrev)
            } else { false }
        };
        if advanced {
            navigation::concordance_jump_to_current(state, tokio_handle);
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
        let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
        let retreated = {
            let mut s = state.borrow_mut();
            if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                conc.retreat_within_work(abbrev)
            } else { false }
        };
        if retreated {
            navigation::concordance_jump_to_current(state, tokio_handle);
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
