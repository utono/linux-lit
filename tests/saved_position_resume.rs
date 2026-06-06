//! Regression: reopening a two-column play must restore the SAVED reading
//! position, highlight intact — it must NOT jump to the work's final (EPILOGUE)
//! spread.
//!
//! The bug: `snap_near_end_to_canonical` fired on reopen whenever the restored
//! page was within a few forward spreads of the work's end (true for any
//! late-Act-5 position at a tall viewport) and forced both `page_top` and the
//! cursor onto the canonical FINAL spread — the EPILOGUE — even though the saved
//! cursor sat several spreads earlier (H8 Scene 3, `I'll peck o'er the pales`,
//! buffer line 4192, page_top 4191). Result: reopen showed the EPILOGUE with no
//! highlight instead of the page the user quit on. The fix early-returns when the
//! restored cursor is already fully visible on the restored spread.
//!
//! VIEWPORT HEIGHT MATTERS. The bug only reproduces when the text view is tall
//! enough (~1112px on the real monitor) that the forward-spread walk reaches the
//! work's end within `NEAR_END_SPREADS`. cage's default 1280x720 (and even a
//! 1920x1080 output, which yields only a ~956px text view after chrome) is too
//! short to trigger it. This test sets a tall output so the text view matches the
//! real ~1112px session, then asserts the achieved height is in range so the test
//! fails loudly if the chrome offset ever drifts.
//!
//! Run under the env wrapper:
//!     ./scripts/e2e-env.sh cargo test --test saved_position_resume -- --ignored --nocapture

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
#[ignore = "needs cage + grim + wlr-randr; run with --ignored"]
fn reopen_restores_saved_position_not_epilogue() {
    Harness::reset_dev_log();
    // Resume H8 at the Porter's line in Scene 3 (buffer 4192) — the exact saved
    // position the live bug reproduced from. Scene 4 and the EPILOGUE follow it,
    // so the canonical final spread (which the bug wrongly jumped to) is several
    // spreads ahead.
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LINUX_LIT_WORK", "H8"),
            ("LIT_START_POS", "4192"),
        ],
    )
    .expect("launch linux-lit in cage");

    // Match the real ~1112px text view: cage's 1280x720 default is far too short
    // to trigger the bug, and even 1920x1080 yields only ~956px after chrome.
    // 1920x1236 (≈1112 + ~124px chrome) reproduces the tall-viewport pagination
    // where the bug appeared.
    let _ = h.set_output_size(1920, 1236);

    // Readiness gate + viewport rect (its 4th value is the text-view height).
    let (_, _, _, view_h) = h
        .wait_for_viewport_rect(Duration::from_secs(10))
        .expect("app reported its reading-viewport rect");
    h.settle(Duration::from_millis(400));

    // Guard: if the chrome offset drifts and the text view is no longer ~1112px,
    // the test would silently stop exercising the tall-viewport path. Fail loudly.
    assert!(
        (1060..=1160).contains(&view_h),
        "text view height {view_h}px is outside the ~1112px band this regression \
         needs; adjust set_output_size (the bug only reproduces at a tall viewport)"
    );

    let log = h.read_dev_log();

    assert!(
        log.contains("resumed saved position current_line=4192"),
        "expected the saved Scene-3 position to be restored; log:\n{log}"
    );

    // THE REGRESSION: the canonical snap must NOT have fired. Before the fix the
    // log showed `STARTUP: snap near-end page_top 4191 -> canonical <epilogue>`.
    assert!(
        !log.contains("STARTUP: snap near-end"),
        "reopen wrongly snapped the saved Scene-3 position to the final/EPILOGUE \
         spread (the saved-position-override regression); log:\n{log}"
    );

    // And the highlight must be on the saved cursor line (4192), not moved.
    assert!(
        log.contains("CURSOR_LINE: applied tag to line 4192"),
        "expected the highlight to stay on the saved cursor line 4192; log:\n{log}"
    );
}
