//! Journal Q&A overlay clip invariant: the journal overlay must not clip its
//! last visible line when the ask card is open — the exact regression that
//! Tasks 1-5 fixed. Companion to tests/overlay_clipping.rs (synopsis/gloss
//! overlay) and tests/line_clipping.rs (main reading card).
//!
//! The regression: opening the ask card shrank the scrolled viewport, pushing
//! the bottom clip guard up, so the last visible answer row was clipped behind
//! the ask card. This test reproduces that exact path and asserts it stays fixed.
//!
//! Run under the env wrapper so numpy/pillow are available:
//!     ./scripts/e2e-env.sh cargo test --test journal_clipping -- --ignored --nocapture
//!
//! Regions: the app emits two rects to the dev log under `LIT_HEADLESS_TEST`:
//!   - TEST_JOURNAL_VIEWPORT_RECT — scrolled window bounds when overlay opens
//!   - TEST_JOURNAL_ASK_VIEWPORT_RECT — same after ask card is revealed
//! The test uses the ask-card rect for the regression assertion because the ask
//! card shrinks the scrolled viewport from its initial height.

mod harness;

use std::path::PathBuf;
use std::time::Duration;

use harness::Harness;

fn app_binary() -> PathBuf {
    if let Ok(p) = std::env::var("LINUX_LIT_BIN") {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_linux-lit") {
        return PathBuf::from(p);
    }
    PathBuf::from("target/debug/linux-lit")
}

#[test]
#[ignore = "needs cage + grim + wtype + python numpy/pillow; run with --ignored"]
fn journal_overlay_ask_card_never_clips() {
    Harness::reset_dev_log();
    // Use Ham (Hamlet) — always present, single-column prose work with journal
    // support. LIT_START_WORK makes the test hermetic regardless of saved config.
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "Ham"),
        ],
    )
    .expect("launch linux-lit in cage");

    // Wait for the main card to be ready (viewport rect is emitted at reveal).
    let _ = h
        .wait_for_viewport_rect(Duration::from_secs(8))
        .expect("app reported its reading-viewport rect");

    // Advance into a scene so the journal overlay has scene context. Front
    // matter (scene 0) is valid but may have no content; a chapter-forward gives
    // a scene with a real title in the overlay. `3` is next-chapter in RPD keymap.
    h.key("3", 250).expect("3 -> next chapter");
    h.settle(Duration::from_millis(400));

    // Open the journal Q&A overlay: Ctrl+j (ToggleJournalOverlay).
    h.chord(&["ctrl"], "j").expect("Ctrl+j -> journal overlay");

    // Wait for the overlay's scrolled viewport rect (emitted from show_page once
    // the vadjustment gets a non-zero range — after the first layout pass).
    let _region = h
        .wait_for_journal_viewport_rect(Duration::from_secs(8))
        .expect("journal overlay reported its viewport rect (TEST_JOURNAL_VIEWPORT_RECT)");
    h.settle(Duration::from_millis(500));

    // Type literal `A` to open the ask card. This is the exact regression path
    // from Tasks 1-5: opening the ask card shrinks the scrolled viewport, and the
    // bottom clip guard must recompute to keep the last row visible.
    // The overlay emits TEST_JOURNAL_ASK_VIEWPORT_RECT from open_ask_card so the
    // harness can read the UPDATED (smaller) rect for the clip assertion.
    // Using type_text (wtype text mode) rather than chord to ensure the uppercase
    // keysym (GDK_KEY_A) reaches the journal overlay — wtype's -M shift / -k a
    // leaves the keyval as lowercase `a`, which the journal keymap doesn't match.
    h.type_text("A", 200).expect("A -> open ask card");

    let ask_region = h
        .wait_for_journal_ask_viewport_rect(Duration::from_secs(8))
        .expect("journal overlay reported ask-card rect (TEST_JOURNAL_ASK_VIEWPORT_RECT)");
    h.settle(Duration::from_millis(500));

    // Assert no line clipping with the ask card open (the Tasks 1-5 regression).
    h.assert_no_line_clipping("journal_overlay_ask_open", ask_region)
        .expect("no clip at journal overlay bottom WITH ask card open (Task 1-5 regression)");
}
