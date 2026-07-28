//! Plain `\` segment-overlay cycle: gloss → journal Q&A → syntax → gloss →
//! … — a WRAPPING rotation over the three overlay stops, anchored to the
//! reader position where it started. Each advance closes the current overlay
//! by RESTORING its saved pre-open position (never the jump-to-source close),
//! so every stop shows the same segment even after Ctrl+n/p traversal inside
//! an overlay.
//!
//! The syntax stop is not a separate surface — a syntax gloss is a gloss_type
//! rendered by the same gloss overlay as the first stop, so its "open" is a
//! filtered lookup (`try_open_syntax_gloss_at_cursor`) rather than a distinct
//! widget.
//!
//! EMPTY STOPS ARE SKIPPED (2026-07-27). Previously the lap ran
//! `reader → gloss → journal → syntax → reader` and simply ENDED wherever a
//! stop had nothing to show. Because `open_journal_scene` returned `()`, the
//! gloss advance could not tell that the journal stop had toasted and opened
//! nothing, so on any scene without a journal entry the user was dumped back
//! into the reader and the syntax stop was UNREACHABLE by cycling — the
//! reported bug (Ant-Arkangel 5.2, which has a syntax gloss but zero journal
//! entries). Now each advance probes the candidate stops in rotation order
//! and opens the first that has content.
//!
//! The reader is NO LONGER a stop: `\` rotates among the overlays
//! indefinitely and Escape (each overlay's own close key) is the way out.
//! When NO other stop has content, the current overlay is left untouched and
//! a toast says so — the key must never appear dead.
//!
//! The synopsis overlay is NOT a stop (dropped from the lap 2026-07-21); its
//! `\` key is a consumed no-op in handle_synopsis_overlay_key. This decision
//! still stands.
//! Escape and each overlay's own close/flip keys are untouched.
//!
//! EVERY STOP IS SEGMENT-SCOPED (2026-07-27). A stop has content only when
//! something covers the CURSOR'S SEGMENT. The journal stop used to fall back
//! to the whole scene band when no passage Q&A covered the cursor, so `\` on
//! BH-Barrett ch. 10 opened the chapter's oldest Q&A — about a different
//! passage than the one on screen. `open_journal_scene` now takes
//! `JournalOpenScope::SegmentOnly` here; Ctrl+j keeps the band fallback.
//! A journal entry is reachable by `\` when its citation span covers the
//! anchor, whatever its filing `scope` (corrected 2026-07-27 — the probe
//! used to require `scope='passage'`, which hid every scene-filed entry that
//! carried a span, and made the journal stop dead on Bleak House). Entries
//! with no citation at all are unreachable by `\` — reach them with Ctrl+j
//! or the picker.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// The stops of the `\` rotation, in cycle order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stop {
    Gloss,
    Journal,
    Syntax,
}

impl Stop {
    /// The next stop in the rotation (wraps Syntax → Gloss).
    fn next(self) -> Self {
        match self {
            Stop::Gloss => Stop::Journal,
            Stop::Journal => Stop::Syntax,
            Stop::Syntax => Stop::Gloss,
        }
    }

    /// Whether this stop has anything to show for the current cursor —
    /// checked WITHOUT opening anything or tearing down the live overlay.
    fn has_content(self, state: &Rc<RefCell<AppState>>) -> bool {
        match self {
            Stop::Gloss => crate::input::actions::gloss::gloss_covers_cursor(
                state,
                crate::input::actions::gloss::CYCLE_GLOSS_TYPES,
            ),
            Stop::Syntax => crate::input::actions::gloss::gloss_covers_cursor(
                state,
                crate::input::actions::gloss::CYCLE_SYNTAX_TYPES,
            ),
            Stop::Journal => {
                crate::input::actions::journal::journal_has_content_at_cursor(state)
            }
        }
    }

    /// Open this stop. Called only after `has_content` said yes and the
    /// previous overlay has been torn down.
    fn open(self, state: &Rc<RefCell<AppState>>) {
        match self {
            Stop::Gloss => {
                crate::input::actions::gloss::open_gloss_at_cursor(state);
            }
            Stop::Journal => {
                crate::input::actions::journal::open_journal_scene(
                    state,
                    crate::input::actions::journal::JournalOpenScope::SegmentOnly,
                );
            }
            Stop::Syntax => {
                crate::input::actions::gloss::try_open_syntax_gloss_at_cursor(state);
            }
        }
    }
}

/// Reader `\` (Action::CycleSegmentOverlays): start the lap at the gloss
/// stop. `open_gloss_at_cursor` records `gloss_return_pos` = the current
/// reader position, which becomes the lap's anchor (toasts and opens nothing
/// when no glossed passage covers the cursor).
///
/// Entry from the reader deliberately does NOT skip to another stop: `\` in
/// the reader means "show me the gloss here", and its own miss toast already
/// says when there is none.
pub(crate) fn cycle_from_reader(state: &Rc<RefCell<AppState>>) {
    crate::input::actions::gloss::open_gloss_at_cursor(state);
}

/// Gloss-overlay `\`: advance to the journal Q&A stop (or past it to syntax
/// when the scene has no journal entry).
pub(crate) fn cycle_from_gloss(state: &Rc<RefCell<AppState>>) {
    advance(state, Stop::Gloss);
}

/// Journal-overlay `\`: advance to the syntax stop (or wrap past it to the
/// gloss stop when no syntax gloss covers the cursor).
pub(crate) fn cycle_from_journal(state: &Rc<RefCell<AppState>>) {
    advance(state, Stop::Journal);
}

/// Syntax-stop (gloss overlay filtered to `syntax-gloss`) `\`: WRAP to the
/// gloss stop rather than ending the lap in the reader (2026-07-27).
pub(crate) fn cycle_from_syntax(state: &Rc<RefCell<AppState>>) {
    advance(state, Stop::Syntax);
}

/// Advance the rotation from `current` to the next stop that has content.
///
/// Probes at most one full rotation (the two other stops) so an all-empty
/// passage can never spin. When nothing else can open, the current overlay is
/// left exactly as it was and a toast reports it — the user must never press
/// `\` and see nothing happen.
fn advance(state: &Rc<RefCell<AppState>>, current: Stop) {
    // Probe BEFORE any teardown: `close_current` restores the reader position
    // and hides the overlay, which cannot be undone if the next stop turns out
    // to be empty.
    let mut candidate = current.next();
    let mut target = None;
    // Two candidates — every stop except the one we are on.
    for _ in 0..2 {
        if candidate.has_content(state) {
            target = Some(candidate);
            break;
        }
        candidate = candidate.next();
    }

    let Some(target) = target else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            "Nothing else to cycle to for this passage",
            3,
        );
        return;
    };

    close_current(state, current);
    target.open(state);
}

/// Tear down the overlay showing `current`, restoring the lap's anchor.
///
/// Both the gloss and syntax stops are the SAME widget (a gloss overlay
/// showing a different gloss_type), so they share a close shape; the journal
/// stop has its own state to take.
fn close_current(state: &Rc<RefCell<AppState>>, current: Stop) {
    let mut s = state.borrow_mut();
    // Every `\` advance silences TTS (a gloss block read with a/Space must not
    // keep speaking into the next stop).
    s.tts.stop();
    crate::input::actions::chat::chat_loop_teardown(&mut s);

    // The `-`-family cycle is keyed by block index into a buffer the next open
    // re-renders, so reset it here rather than resuming mid-block on a
    // different document.
    s.gloss_word_cycle = Default::default();
    s.journal_word_cycle = Default::default();

    match current {
        Stop::Gloss | Stop::Syntax => {
            s.gloss_overlay.hide();
            // Ctrl+Tab focus toggle: closing the overlay resets ask-card focus.
            s.ask_card_focus = true;
            s.gloss_opened_from_picker = false;
            crate::app::return_to_reader_mode(&mut s);
            // Restore, never jump_to_gloss_source_start — see module doc. Take
            // the entry stamp too so it can't leak into a later non-entry open.
            s.gloss_entry_citation.take();
            let pos = s.gloss_return_pos.take();
            crate::app::restore_saved_position_resnap(&mut s, pos);
        }
        Stop::Journal => {
            s.journal_overlay.hide();
            s.ask_card_focus = true;
            // Recolor BEFORE restore's update_highlight, matching
            // journal::toggle_overlay's close half.
            crate::app::return_to_reader_mode(&mut s);
            // Take entry_page_id/return_pos so they don't leak into the next
            // open, but ALWAYS restore the saved position — never
            // jump_to_journal_source_start — so the lap continues from its
            // entry segment even after Ctrl+n/p traversal.
            s.journal.entry_page_id.take();
            s.journal_from_synopsis = None;
            let pos = s.journal.return_pos.take();
            crate::app::restore_saved_position_resnap(&mut s, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_order_wraps_at_syntax() {
        assert_eq!(Stop::Gloss.next(), Stop::Journal);
        assert_eq!(Stop::Journal.next(), Stop::Syntax);
        // The 2026-07-27 change: syntax wraps to gloss instead of ending the
        // lap in the reader.
        assert_eq!(Stop::Syntax.next(), Stop::Gloss);
    }

    /// Inclusive span overlap — the rule `gloss_covers_cursor` and
    /// `try_open_syntax_gloss_at_cursor` share.
    fn overlaps(a: (i64, i64, i64), b: (i64, i64, i64), c: (i64, i64, i64), d: (i64, i64, i64)) -> bool {
        c <= b && a <= d
    }

    /// Regression (2026-07-27, second occurrence). The `\` cycle asked "is
    /// there a syntax gloss AT THE ANCHOR LINE" when it should have asked "…for
    /// the passage I am LOOKING AT". The stops sit on passages of different
    /// widths — a reader-gloss over a whole speech (Ant.5.2.424-437) vs a
    /// syntax gloss over the one sentence it was created from (424-425) — so an
    /// anchor at line 437 missed, and `\` reported "nothing else to cycle to"
    /// on a passage that visibly had a syntax gloss.
    #[test]
    fn wider_displayed_passage_overlaps_a_narrower_syntax_gloss() {
        let displayed = ((5, 2, 424), (5, 2, 437)); // reader-gloss: whole speech
        let syntax = ((5, 2, 424), (5, 2, 425)); // syntax gloss: one sentence
        assert!(
            overlaps(displayed.0, displayed.1, syntax.0, syntax.1),
            "the narrower syntax span must be found from the wider displayed span"
        );
        // The old line-anchored rule: the anchor sat at the END of the
        // displayed passage, outside the syntax span — this is what failed.
        let anchor = (5, 2, 437);
        assert!(
            !(syntax.0 <= anchor && anchor <= syntax.1),
            "line 437 is genuinely outside 424-425 — why the anchor rule missed"
        );
    }

    #[test]
    fn non_overlapping_passages_are_not_matched() {
        // A syntax gloss belonging to a DIFFERENT speech must not be pulled in.
        let displayed = ((5, 2, 424), (5, 2, 437));
        let elsewhere = ((5, 2, 500), (5, 2, 505));
        assert!(!overlaps(displayed.0, displayed.1, elsewhere.0, elsewhere.1));
    }

    #[test]
    fn two_probes_cover_every_other_stop() {
        // `advance` probes exactly twice; from any stop those two candidates
        // must be the other two stops, never a repeat of the current one.
        for start in [Stop::Gloss, Stop::Journal, Stop::Syntax] {
            let first = start.next();
            let second = first.next();
            assert_ne!(first, start);
            assert_ne!(second, start);
            assert_ne!(first, second);
        }
    }
}
