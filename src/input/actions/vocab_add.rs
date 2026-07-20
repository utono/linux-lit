use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Ctrl+Alt+\ on the main card: open an EMPTY vim-input card to type a vocab
/// word. Reuses the gloss_overlay edit buffer, exactly like segment_vim, but
/// starts blank and pre-seeded into Insert mode so the reader can type
/// immediately. On :w the word is looked up + inserted; :q/Esc cancels.
pub(crate) fn open(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    s.gloss_overlay
        .show_gloss_with_color("Add vocab word", "", cw, h, Some(&s.theme.root_color), &[]);
    let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.gloss_overlay.set_edit_copy_only(false); // saving IS allowed here
    s.gloss_overlay.enter_edit_buffer("", &fill, &fg);
    // Seed Insert mode so the reader can type immediately.
    let _ = s
        .gloss_overlay
        .feed_edit_key(crate::input::vim::VimKey::Char('i'));
    s.input_mode = crate::app::InputMode::AddVocab;
    crate::logging::log("VOCAB ADD: opened input card");
}

/// Close the input card without saving and return to the reader.
pub(crate) fn close(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.gloss_overlay.exit_edit_buffer();
    s.gloss_overlay.hide();
    crate::app::return_to_reader_mode(&mut s);
    crate::logging::log("VOCAB ADD: cancelled");
}

/// :w in the input card: normalize, look up locally, insert + refresh. On a
/// local miss, fall back to the Claude API (async) with an in-flight guard.
pub(crate) fn submit(state_rc: &Rc<RefCell<AppState>>) {
    let raw = state_rc.borrow().gloss_overlay.edit_buffer_text();
    let word = crate::vocab_lookup::normalize_vocab_word(&raw);
    close(state_rc);

    if word.is_empty() {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "nothing to add", 2);
        return;
    }

    // Duplicate in-flight guard.
    if state_rc.borrow().vocab_add_pending.as_deref() == Some(word.as_str()) {
        return;
    }

    // Local ladder first (synchronous).
    if let Some((definition, source)) = crate::vocab_lookup::lookup_local(&word) {
        insert_and_refresh(state_rc, &word, &definition, &source);
        return;
    }

    // Local miss → Claude fallback (async).
    let model = state_rc.borrow().config.claude_model.clone();
    state_rc.borrow_mut().vocab_add_pending = Some(word.clone());
    {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("looking up \u{201c}{word}\u{201d}\u{2026}"),
            2,
        );
    }
    crate::logging::log(&format!("VOCAB ADD: local miss, asking Claude for '{word}'"));

    let system = "You are a concise dictionary. Given a single English word, \
                  reply with ONE clear dictionary-style definition of it — a \
                  single sentence, no headword, no part-of-speech tag, no \
                  numbering, no quotation marks."
        .to_string();
    let user = word.clone();
    let word_ok = word.clone();
    let word_err = word;
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system,
        user,
        model,
        move |st, answer| {
            // Clear the guard regardless of the UI state.
            if st.borrow().vocab_add_pending.as_deref() == Some(word_ok.as_str()) {
                st.borrow_mut().vocab_add_pending = None;
            }
            let definition = answer.trim().to_string();
            insert_and_refresh(st, &word_ok, &definition, "claude");
        },
        move |st, msg| {
            if st.borrow().vocab_add_pending.as_deref() == Some(word_err.as_str()) {
                st.borrow_mut().vocab_add_pending = None;
            }
            let s = st.borrow();
            crate::input::navigation::show_chapter_toast_secs(
                &s,
                &format!("no definition for \u{201c}{word_err}\u{201d}: {msg}"),
                3,
            );
            crate::logging::log(&format!("VOCAB ADD: claude failed for '{word_err}': {msg}"));
        },
    );
}

/// Insert the word and run the shared view refresh. Used by both the sync
/// local path and the async Claude success callback.
fn insert_and_refresh(state_rc: &Rc<RefCell<AppState>>, word: &str, definition: &str, source: &str) {
    let outcome = match crate::db::queries::open_db_rw() {
        Ok(conn) => crate::db::queries::insert_vocab_word(&conn, word, definition, source),
        Err(e) => Err(e),
    };
    let mut s = state_rc.borrow_mut();
    match outcome {
        Ok(o) => {
            let added = matches!(o, crate::db::queries::VocabInsertOutcome::Added);
            crate::app::apply_after_add(&mut s, word, added, source);
        }
        Err(e) => {
            crate::logging::log(&format!("VOCAB ADD: db write failed for '{word}': {e}"));
            crate::input::navigation::show_chapter_toast_secs(
                &s,
                &format!("couldn't save \u{201c}{word}\u{201d}"),
                3,
            );
        }
    }
}
