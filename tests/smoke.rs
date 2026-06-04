//! End-to-end smoke test for linux-lit under a headless `cage` compositor.
//!
//! Marked `#[ignore]` so a normal `cargo test` on a box without cage/grim stays
//! green. Run explicitly (the env wrapper provides software GL + the a11y bus
//! the artifacts want; cage + grim are the hard requirement):
//!
//!     ./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture
//!
//! The harness runs the app inside its own isolated cage (temp XDG_RUNTIME_DIR),
//! so it never touches the live session. `GSK_RENDERER=cairo` (mandatory
//! headless) and the MPV-skip (`LIT_HEADLESS_TEST=1`) are handled by the harness
//! / app so the reader actually renders and nothing covers it.

mod harness;

use std::path::PathBuf;
use std::time::Duration;

use harness::Harness;

/// Resolve the binary under test. Override with LINUX_LIT_BIN, else use the path
/// Cargo injects for the `linux-lit` bin target, else fall back to debug.
fn app_binary() -> PathBuf {
    if let Ok(p) = std::env::var("LINUX_LIT_BIN") {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_linux-lit") {
        return PathBuf::from(p);
    }
    PathBuf::from("target/debug/linux-lit")
}

/// Launch linux-lit inside cage. No DB/work arg — it opens the MRU work from
/// `~/utono/litdb/data/lit.db`. LIT_DEV=1 → dev id + dev log; LIT_HEADLESS_TEST=1
/// → MPV is skipped (no window to cover the reader, no leaked process).
fn start() -> Harness {
    Harness::reset_dev_log();
    Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")],
    )
    .expect("launch linux-lit in cage")
}

#[test]
#[ignore = "needs cage + grim; run with --ignored"]
fn launches_and_renders_the_reader() {
    let h = start();

    // Gate on the app actually revealing its window (emits the viewport rect at
    // reveal). Falls back to a plain settle if the rect never lands.
    if h.wait_for_viewport_rect(Duration::from_secs(8)).is_err() {
        h.settle(Duration::from_secs(3));
    }

    let shot = PathBuf::from("target/e2e-shot.png");
    h.screenshot(&shot).expect("screenshot");

    // A blank/failed capture is tiny; a rendered reading card is ~150 KB. The
    // >50 KB floor distinguishes "reader painted" from "blank output" (the old
    // 2 KB floor passed even a solid-color desktop).
    let len = std::fs::metadata(&shot).expect("shot exists").len();
    assert!(
        len > 50_000,
        "screenshot {len} bytes — reader likely did not render (blank output). \
         Check GSK_RENDERER=cairo, that a work loaded, and that MPV was skipped."
    );
}
