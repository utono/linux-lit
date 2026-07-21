//! Plain `\` segment-overlay cycle: journal Q&A → gloss → synopsis → journal
//! (wraps). The lap is anchored to the reader position where it started —
//! each advance closes the current overlay by RESTORING its saved pre-open
//! position (never the jump-to-source close), so every stop shows the same
//! segment even after Ctrl+n/p traversal inside an overlay. Empty stops keep
//! their standalone fallbacks: every stop (journal, gloss, synopsis) toasts,
//! landing back in the reader at the anchor.
//! Escape and each overlay's own close/flip keys are untouched.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Reader `\` (Action::CycleSegmentOverlays): start the lap at the journal
/// Q&A stop. `open_journal_scene` records `journal.return_pos` = the current
/// reader position, which becomes the lap's anchor.
pub(crate) fn cycle_from_reader(state: &Rc<RefCell<AppState>>) {
    crate::input::actions::journal::open_journal_scene(state);
}

/// Journal-overlay `\`: close restoring the anchor, then open the gloss stop
/// for the anchor line.
pub(crate) fn cycle_from_journal(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        // Every `\` advance silences TTS (a journal block read with s/Space
        // must not keep speaking into the gloss stop).
        s.tts.stop();
        crate::input::actions::chat::chat_loop_teardown(&mut s);
        s.journal_overlay.hide();
        // Recolor BEFORE restore's update_highlight, matching
        // journal::toggle_overlay's close half.
        crate::app::return_to_reader_mode(&mut s);
        // Take entry_page_id/return_pos so they don't leak into the next
        // open, but ALWAYS restore the saved position — never
        // jump_to_journal_source_start — so the lap stays on its entry
        // segment even after Ctrl+n/p traversal.
        s.journal.entry_page_id.take();
        let pos = s.journal.return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    crate::input::actions::gloss::open_gloss_at_cursor(state);
}

/// Gloss-overlay `\`: close restoring the anchor, then open the synopsis stop.
pub(crate) fn cycle_from_gloss(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.tts.stop();
        crate::input::actions::chat::chat_loop_teardown(&mut s);
        s.gloss_overlay.hide();
        s.gloss_opened_from_picker = false;
        crate::app::return_to_reader_mode(&mut s);
        // Restore, never jump_to_gloss_source_start — see module doc. Take the
        // entry stamp too so it can't leak into a later non-entry open.
        s.gloss_entry_citation.take();
        let pos = s.gloss_return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    crate::app::scene_synopsis::show_synopsis_overlay(state);
}

/// Synopsis-overlay `\`: wrap back to the journal Q&A stop. The synopsis
/// never moves the reader cursor, so its close (hide + return to reader,
/// mirroring its `h`/Escape arms) already leaves the anchor current.
pub(crate) fn cycle_from_synopsis(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        // Every `\` advance silences TTS (matching the other two advances).
        s.tts.stop();
        crate::input::actions::chat::chat_loop_teardown(&mut s);
        s.gloss_overlay.hide();
        crate::app::return_to_reader_mode(&mut s);
    }
    crate::input::actions::journal::open_journal_scene(state);
}
