//! Overlay clip invariant: the synopsis/gloss overlay must not clip its first or
//! last line. Companion to tests/line_clipping.rs (which covers the MAIN reading
//! card and explicitly scopes overlays out). Closes that coverage gap so the
//! shared free-scroll clip path can't regress silently.
//!
//! Run under the env wrapper so numpy/pillow are available:
//!     ./scripts/e2e-env.sh cargo test --test overlay_clipping -- --ignored --nocapture
//!
//! Region: the app emits the overlay's scrolled viewport rect to the dev log on
//! reveal (under `LIT_HEADLESS_TEST`) as `TEST_OVERLAY_VIEWPORT_RECT`, read by
//! `wait_for_overlay_viewport_rect`.

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
fn synopsis_overlay_never_clips() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")],
    )
    .expect("launch linux-lit in cage");

    // Wait for the main card to be ready first (its rect doubles as readiness).
    let _ = h
        .wait_for_viewport_rect(Duration::from_secs(8))
        .expect("app reported its reading-viewport rect");

    // Front matter (before Chapter 1) has no synopsis, so `h` would show nothing.
    // Advance into a chapter first — `3` is next-chapter in the RPD keymap.
    h.key("3", 250).expect("3 -> next chapter");
    h.settle(Duration::from_millis(400));

    // Open the synopsis overlay.
    h.key("h", 300).expect("h -> synopsis overlay");
    let region = h
        .wait_for_overlay_viewport_rect(Duration::from_secs(8))
        .expect("overlay reported its viewport rect");
    h.settle(Duration::from_millis(500));

    // Top of the overlay: assert the first line isn't clipped.
    h.assert_no_line_clipping("synopsis_overlay_top", region)
        .expect("no clip at overlay top");

    // Scroll to the bottom so the last line sits at the viewport edge — `j`
    // scrolls the open overlay (not the reading buffer) per the keymap.
    for _ in 0..40 {
        h.key("j", 60).expect("j -> scroll overlay down");
    }
    h.settle(Duration::from_millis(500));
    h.assert_no_line_clipping("synopsis_overlay_bottom", region)
        .expect("no clip at overlay bottom");
}
