//! Journal Q&A overlay clip invariant: the journal overlay must not clip/occlude
//! its last visible line when the ask card is open. Companion to
//! tests/overlay_clipping.rs (synopsis/gloss overlay) and tests/line_clipping.rs
//! (main reading card).
//!
//! NOTE on what this asserts: the reported "text behind the ask card" bug is an
//! OCCLUSION/layout bug — the ask card overflows the fixed-height container and
//! overlaps an UNCHANGED-height scroll viewport (page_size does NOT shrink on
//! open; proven by runtime diag, see docs/troubleshooting/clip-prevention.md
//! "occlusion is not clipping"). The BottomClipGuard refactor does NOT fix it;
//! the structural layout fix (make the scroll viewport shrink for the ask card)
//! does. This test is the on-screen guard for that structural fix: once the
//! viewport shrinks, no answer row may sit behind the ask card.
//!
//! Run under the env wrapper so numpy/pillow are available:
//!     ./scripts/e2e-env.sh cargo test --test journal_clipping -- --ignored --nocapture
//!
//! Regions: the app emits two rects to the dev log under `LIT_HEADLESS_TEST`:
//!   - TEST_JOURNAL_VIEWPORT_RECT — scrolled window bounds when overlay opens
//!   - TEST_JOURNAL_ASK_VIEWPORT_RECT — same after ask card is revealed
//! The test asserts on the ask-card rect (the viewport with the ask card open),
//! which is where the occluded rows would show until the structural fix lands.

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
    // Use Cym — a work whose journal has pages of its own. Ctrl+j on a work
    // with NO pages only toasts ("No journal pages yet"), so the overlay would
    // be unreachable (46941ce6). LIT_START_WORK makes the test hermetic
    // regardless of saved config.
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "Cym"),
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

    // Open the journal Q&A overlay: Ctrl+j (ToggleJournalOverlay). On an empty
    // scene band this opens the work-wide picker instead — confirm the first
    // row (Return) to reveal the overlay.
    h.chord(&["ctrl"], "j").expect("Ctrl+j -> journal overlay/picker");

    // Wait for the overlay's scrolled viewport rect (emitted from show_page once
    // the vadjustment gets a non-zero range — after the first layout pass).
    if h
        .wait_for_journal_viewport_rect(Duration::from_secs(4))
        .is_err()
    {
        h.key("Return", 250).expect("Return -> confirm picker row");
        let _ = h
            .wait_for_journal_viewport_rect(Duration::from_secs(8))
            .expect("journal overlay opened after picker confirm (TEST_JOURNAL_VIEWPORT_RECT)");
    }
    h.settle(Duration::from_millis(500));

    // Press Ctrl+r to open the ask card — the reported-bug path (the opener
    // moved from `A` to `r`, then to Ctrl+r when plain `r` became the vocab
    // popup tap; `A` is TTS now). Today the ask card overlaps the
    // (unchanged-height) scroll viewport and occludes the lower rows; the
    // structural fix must make the viewport shrink so no row sits behind the
    // card. The overlay emits TEST_JOURNAL_ASK_VIEWPORT_RECT from
    // open_ask_card so the harness reads the with-ask-card-open rect.
    h.chord(&["ctrl"], "r").expect("Ctrl+r -> open ask card");

    let ask_region = h
        .wait_for_journal_ask_viewport_rect(Duration::from_secs(8))
        .expect("journal overlay reported ask-card rect (TEST_JOURNAL_ASK_VIEWPORT_RECT)");
    h.settle(Duration::from_millis(500));

    // Assert no occluded/clipped row with the ask card open (the reported bug;
    // passes once the structural layout fix shrinks the viewport for the card).
    h.assert_no_line_clipping("journal_overlay_ask_open", ask_region)
        .expect("no row behind the journal ask card WITH it open (occlusion/layout fix)");
}
