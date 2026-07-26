//! Plain `\` segment-overlay cycle: gloss → journal Q&A → syntax → back to
//! the reader (four presses, no wrap). The lap is anchored to the reader
//! position where it started — each advance closes the current overlay by
//! RESTORING its saved pre-open position (never the jump-to-source close), so
//! every stop shows the same segment even after Ctrl+n/p traversal inside an
//! overlay. The syntax stop is not a separate surface — a syntax gloss is a
//! gloss_type rendered by the same gloss overlay as the first stop, so its
//! "open" is a filtered lookup (`try_open_syntax_gloss_at_cursor`) rather than
//! a distinct widget. Empty stops keep their standalone fallbacks: the gloss
//! stop toasts "No gloss on this line" and the lap simply doesn't start; the
//! journal stop toasts and continues to the syntax stop regardless; the syntax
//! stop opens nothing when no syntax-gloss covers the cursor and the lap ends
//! in the reader at the anchor either way.
//! The synopsis overlay is NOT a stop (dropped from the lap 2026-07-21); its
//! `\` key is a consumed no-op in handle_synopsis_overlay_key. This decision
//! still stands — synopsis was one stop too many, and adding the syntax stop
//! does not reopen that question.
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

/// Journal-overlay `\`: close restoring the anchor, then open the syntax
/// stop for the anchor line. The syntax stop opens nothing (and the lap
/// simply ends in the reader) when no syntax-gloss covers the cursor — see
/// `cycle_from_syntax`.
pub(crate) fn cycle_from_journal(state: &Rc<RefCell<AppState>>) {
    {
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
        // jump_to_journal_source_start — so the lap continues from its entry
        // segment even after Ctrl+n/p traversal.
        s.journal.entry_page_id.take();
        let pos = s.journal.return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    crate::input::actions::gloss::try_open_syntax_gloss_at_cursor(state);
}

/// Syntax-stop (gloss overlay filtered to `syntax-gloss`) `\`: end the lap —
/// close restoring the anchor, back to the reading card. Opens nothing.
/// Shares the gloss overlay's close shape with `cycle_from_gloss` (it's the
/// same widget, just showing a different gloss_type).
pub(crate) fn cycle_from_syntax(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Silence TTS on the way out (matching the other advances).
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
