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
        .wait_for_viewport_rect(Duration::from_secs(20))
        .expect("app reported its reading-viewport rect");

    // Front matter (before a scene with synopsis data) may show nothing, so
    // advance one scene first — `{` (braceleft) is JumpToNextScene in the RPD
    // keymap. (`h` used to open the synopsis and `3` used to jump a scene; both
    // binds moved — `h` is now CursorNextDialogueNoSeek and the synopsis opener
    // is Ctrl+h. See src/input/keymap_config.rs.)
    h.key("braceleft", 250).expect("{ -> next scene");
    h.settle(Duration::from_millis(400));

    // Open the synopsis overlay — Ctrl+h (ShowSynopsisOverlay).
    h.chord(&["ctrl"], "h").expect("Ctrl+h -> synopsis overlay");
    let region = h
        .wait_for_overlay_viewport_rect(Duration::from_secs(20))
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
