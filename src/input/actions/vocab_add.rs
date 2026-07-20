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

/// :w in the input card. Task 6 fills in lookup + insert + refresh. For now,
/// just read the word, close, and toast so the wiring is testable.
pub(crate) fn submit(state_rc: &Rc<RefCell<AppState>>) {
    let raw = state_rc.borrow().gloss_overlay.edit_buffer_text();
    let word = crate::vocab_lookup::normalize_vocab_word(&raw);
    close(state_rc);
    let s = state_rc.borrow();
    if word.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "nothing to add", 2);
    } else {
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("(stub) would add \u{201c}{word}\u{201d}"),
            2,
        );
    }
}
