//! Plain `\` segment-overlay cycle: gloss → journal Q&A → back to the reader
//! (three presses, no wrap). The lap is anchored to the reader position where
//! it started — each advance closes the current overlay by RESTORING its saved
//! pre-open position (never the jump-to-source close), so both stops show the
//! same segment even after Ctrl+n/p traversal inside an overlay. Empty stops
//! keep their standalone fallbacks: the gloss stop toasts "No gloss on this
//! line" and the lap simply doesn't start; the journal stop toasts and lands
//! back in the reader at the anchor.
//! The synopsis overlay is NOT a stop (dropped from the lap 2026-07-21); its
//! `\` key is a consumed no-op in handle_synopsis_overlay_key.
//! Escape and each overlay's own close/flip keys are untouched.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Reader `\` (Action::CycleSegmentOverlays): start the lap at the gloss
/// stop. `open_gloss_at_cursor` records `gloss_return_pos` = the current
/// reader position, which becomes the lap's anchor (toasts and opens nothing
/// when no glossed passage covers the cursor).
pub(crate) fn cycle_from_reader(state: &Rc<RefCell<AppState>>) {
    crate::input::actions::gloss::open_gloss_at_cursor(state);
}

/// Gloss-overlay `\`: close restoring the anchor, then open the journal Q&A
/// stop for the anchor line.
pub(crate) fn cycle_from_gloss(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        // Every `\` advance silences TTS (a gloss block read with a/Space
        // must not keep speaking into the journal stop).
        s.tts.stop();
        crate::input::actions::chat::chat_loop_teardown(&mut s);
        s.gloss_overlay.hide();
        // Ctrl+Tab focus toggle: closing the overlay resets ask-card focus.
        s.ask_card_focus = true;
        s.gloss_opened_from_picker = false;
        crate::app::return_to_reader_mode(&mut s);
        // Restore, never jump_to_gloss_source_start — see module doc. Take the
        // entry stamp too so it can't leak into a later non-entry open.
        s.gloss_entry_citation.take();
        let pos = s.gloss_return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    crate::input::actions::journal::open_journal_scene(state);
}

/// Journal-overlay `\`: end the lap — close restoring the anchor, back to
/// the reading card. Opens nothing.
pub(crate) fn cycle_from_journal(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Silence TTS on the way out (matching the other advances).
    s.tts.stop();
    crate::input::actions::chat::chat_loop_teardown(&mut s);
    s.journal_overlay.hide();
    // Ctrl+Tab focus toggle: closing the overlay resets ask-card focus.
    s.ask_card_focus = true;
    // Recolor BEFORE restore's update_highlight, matching
    // journal::toggle_overlay's close half.
    crate::app::return_to_reader_mode(&mut s);
    // Take entry_page_id/return_pos so they don't leak into the next open,
    // but ALWAYS restore the saved position — never
    // jump_to_journal_source_start — so the lap ends on its entry segment
    // even after Ctrl+n/p traversal.
    s.journal.entry_page_id.take();
    let pos = s.journal.return_pos.take();
    crate::app::restore_saved_position_resnap(&mut s, pos);
}
