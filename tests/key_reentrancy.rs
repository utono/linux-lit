//! Key dispatch must not panic when the app state is already borrowed.
//!
//! Prose pagination pumps the GTK main loop to force layout validation
//! (`input::prose_pages::record_prose_pages` and friends call
//! `glib::MainContext::iteration(false)` up to `MAX_LAYOUT_SPINS` times).
//! Those functions run while `app::mod`'s pagination call site holds
//! `st.borrow_mut()`. Pumping the loop DISPATCHES PENDING INPUT
//! RE-ENTRANTLY, so a keystroke arriving mid-pagination re-enters
//! `keymap::handle_key`, which used to open with a bare
//! `state.borrow()` -> `BorrowMutError`.
//!
//! A panic inside a `connect_key_pressed` trampoline cannot unwind across
//! GTK's C frames, so the process ABORTS rather than merely failing the
//! keystroke:
//!
//!     thread caused non-unwinding panic. aborting.
//!
//! Reported 2026-08-08 loading LoJ (21,520 rows) — the corpus's largest
//! prose work, whose pagination window is long enough to realistically
//! type into. This is a latent bug that work size exposes, not a data
//! problem.
//!
//! `AppState` owns GTK widgets and cannot be built off the main loop, so
//! these tests pin the REENTRANCY CONTRACT on a stand-in `RefCell` rather
//! than driving the real handler. The guarantee under test is the one the
//! guard in `handle_key` provides: a nested dispatch must return without
//! panicking, and must not observe a half-written borrow.

use std::cell::RefCell;
use std::rc::Rc;

/// Mirrors the guard `keymap::handle_key` applies before touching state:
/// if the state is already borrowed, the event is dropped, not forced.
fn dispatch_guarded(state: &Rc<RefCell<i32>>) -> bool {
    match state.try_borrow_mut() {
        Ok(mut s) => {
            *s += 1;
            true
        }
        // Re-entrant call during pagination: decline the key. Returning
        // false leaves it unconsumed, which is the correct outcome for a
        // keystroke the reader was not in a position to act on.
        Err(_) => false,
    }
}

#[test]
fn nested_dispatch_does_not_panic_while_state_is_borrowed() {
    let state = Rc::new(RefCell::new(0));
    // Simulate the pagination call site holding the mutable borrow.
    let held = state.borrow_mut();
    // A keystroke arrives via the pumped main loop.
    let consumed = dispatch_guarded(&state);
    assert!(
        !consumed,
        "a key arriving mid-pagination must be declined, not consumed"
    );
    drop(held);
}

#[test]
fn dispatch_works_normally_when_state_is_free() {
    let state = Rc::new(RefCell::new(0));
    assert!(dispatch_guarded(&state), "a normal key must be handled");
    assert_eq!(*state.borrow(), 1, "the handler must have run its effect");
}

#[test]
fn declined_key_leaves_state_untouched() {
    let state = Rc::new(RefCell::new(7));
    let held = state.borrow_mut();
    let _ = dispatch_guarded(&state);
    drop(held);
    assert_eq!(
        *state.borrow(),
        7,
        "a declined key must not have mutated state"
    );
}

/// The unguarded form is what shipped, and it panics. Kept as executable
/// documentation of WHY the guard exists: without it this is an abort, not
/// a catchable failure, because the real call site is a C trampoline.
#[test]
#[should_panic(expected = "already mutably borrowed")]
fn unguarded_dispatch_panics_the_old_way() {
    let state = Rc::new(RefCell::new(0));
    let _held = state.borrow_mut();
    let _boom = state.borrow(); // BorrowMutError -> abort in the GTK callback
}
