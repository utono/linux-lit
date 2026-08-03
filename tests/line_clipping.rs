//! The core invariant: the main reading card must never clip its first or last
//! visible line. Clipping commonly appears only at certain scroll offsets (top
//! of document, mid-scroll, end), so this checks all three.
//!
//! Run under the env wrapper so numpy/pillow are available:
//!     ./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
//!
//! The check fails closed, so a missing dep is a test failure, not a pass.
//!
//! Region: linux-lit's `sourceview5::View` exposes no AT-SPI Text interface, so
//! the detector can't auto-find the pane. Instead the app emits its reading
//! viewport rectangle to the dev log on reveal (under `LIT_HEADLESS_TEST`), and
//! `wait_for_viewport_rect` reads it — exact and resolution-independent.
//!
//! Scope: the MAIN reading card. The synopsis/gloss OVERLAY has its own scroll/
//! clip path and a different geometry; testing it would need an `h` open step
//! and its own region.

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
fn first_and_last_lines_never_clip() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")],
    )
    .expect("launch linux-lit in cage");

    // The viewport rect doubles as a readiness gate (logged at reveal).
    let region = h
        .wait_for_viewport_rect(Duration::from_secs(20))
        .expect("app reported its reading-viewport rect");

    // Keys land on the window's global capture-phase controller — no Tab-focus
    // step needed.

    // Top of document: `gg` is a sequential chord (g sets PendingG, the second
    // g triggers JumpToStart), NOT a modifier chord — two presses with a settle.
    h.key("g", 200).expect("g (pending)");
    h.key("g", 200).expect("g g -> top");
    h.settle(Duration::from_millis(500));
    h.assert_no_line_clipping("top", region)
        .expect("no clip at top");

    // A few pages in — `x` is page-forward in the RPD keymap.
    for _ in 0..3 {
        h.key("x", 150).expect("page forward");
    }
    h.settle(Duration::from_millis(500));
    h.assert_no_line_clipping("mid", region)
        .expect("no clip mid-scroll");

    // End of document: capital `G` (JumpToEnd) — shift+g as one chord.
    h.chord(&["shift"], "g").expect("shift+g -> end");
    h.settle(Duration::from_millis(500));
    h.assert_no_line_clipping("bottom", region)
        .expect("no clip at bottom");
}
