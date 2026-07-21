use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Ctrl+Alt+\ from any surface: open a dedicated compact vim-input card to type
/// a vocab word. Its own `AskCard` (attached above the whole overlay chain), so
/// it opens OVER the gloss/journal overlays and the chat transcript — the old
/// gloss-overlay reuse could not, since the gloss overlay was either busy or
/// below the journal. Opens in INSERT so the reader can type immediately. On :w
/// the word is looked up + inserted; :q/Esc cancels and restores the prior mode.
pub(crate) fn open(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    // Remember the surface we opened from so close() returns to it (Reader,
    // either overlay, or the chat transcript).
    let prior = s.input_mode;
    s.vocab_add_return_mode = Some(prior);
    place_away_from_cursor(&s);
    let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    // `open_insert` starts the vim engine in INSERT (type immediately, no `i`).
    // card_width = 0 keeps the card's fixed 560px input-strip request and its
    // centered float rather than re-insetting to an overlay prose column.
    s.vocab_add_card.open_insert(
        "Add vocab word",
        ":w add \u{b7} Esc cancel",
        "",
        0,
        &fill,
        &fg,
    );
    s.input_mode = crate::app::InputMode::AddVocab;
    crate::logging::log("VOCAB ADD: opened input card");
}

/// Anchor the floating card to whichever half of the window the cursor segment
/// is NOT in, so the card never covers the line being read: cursor on-screen in
/// the top half → card sits low (End + bottom margin); bottom half → card sits
/// high (Start + top margin). Falls back to the old centered float when the
/// cursor's geometry is unavailable (e.g. pre-layout).
fn place_away_from_cursor(s: &AppState) {
    use gtk4::prelude::*;
    const EDGE_MARGIN: i32 = 120;
    let container = s.vocab_add_card.container();
    let adj = s.scrolled_window.vadjustment();
    let placed = s
        .buffer
        .iter_at_line(s.current_line as i32)
        .map(|iter| {
            let (y, h) = s.text_view.line_yrange(&iter);
            let screen_y = y as f64 + h as f64 / 2.0 - adj.value();
            screen_y < adj.page_size() / 2.0
        })
        .map(|cursor_in_top_half| {
            if cursor_in_top_half {
                container.set_valign(gtk4::Align::End);
                container.set_margin_top(0);
                container.set_margin_bottom(EDGE_MARGIN);
            } else {
                container.set_valign(gtk4::Align::Start);
                container.set_margin_top(EDGE_MARGIN);
                container.set_margin_bottom(0);
            }
        });
    if placed.is_none() {
        container.set_valign(gtk4::Align::Center);
        container.set_margin_top(0);
        container.set_margin_bottom(0);
    }
}

/// Close the input card without saving and restore the surface it opened from.
pub(crate) fn close(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.vocab_add_card.close();
    let back = s
        .vocab_add_return_mode
        .take()
        .unwrap_or(crate::app::InputMode::Reader);
    if back == crate::app::InputMode::Reader {
        crate::app::return_to_reader_mode(&mut s);
    } else {
        s.input_mode = back;
    }
    crate::logging::log("VOCAB ADD: cancelled");
}

/// :w in the input card: normalize, look up locally, insert + refresh. On a
/// local miss, fall back to the Claude API (async) with an in-flight guard.
pub(crate) fn submit(state_rc: &Rc<RefCell<AppState>>) {
    let raw = state_rc.borrow().vocab_add_card.take_text();
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
