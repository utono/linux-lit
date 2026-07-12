//! Plain `\` segment-overlay cycle: journal Q&A → gloss → synopsis → journal
//! (wraps). The lap is anchored to the reader position where it started —
//! each advance closes the current overlay by RESTORING its saved pre-open
//! position (never the jump-to-source close), so every stop shows the same
//! segment even after Ctrl+n/p traversal inside an overlay. Empty stops keep
//! their standalone fallbacks: journal → work-wide Q&A picker (which ends the
//! lap), gloss/synopsis → toast, landing back in the reader at the anchor.
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
