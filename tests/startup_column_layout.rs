//! Regression: a two-column play must lay out two-column from the FIRST card
//! pass, with no visible 1→2-column reflow on startup.
//!
//! The glitch: `column_count()` returns 1 while `current_work` is still None
//! (early startup, before the work loads), so the first card-sizing/formatting
//! pass is single-column; once the work loads it switches to two-column and
//! re-formats. The reveal is opacity-gated, but the reflow lands close enough to
//! reveal to be visible.
//!
//! Fix: seed the column count from the last session's resolved value
//! (`config.last_column_count`, or `LIT_START_COLUMNS` for tests) so the first
//! pass already matches. This test sets `LIT_START_COLUMNS=2` and asserts the
//! FIRST `CARD_SIZING` line reports `cols=2` — i.e. no 1→2 swap.
//!
//! Run under the env wrapper:
//!     ./scripts/e2e-env.sh cargo test --test startup_column_layout -- --ignored --nocapture

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
fn two_column_play_starts_two_column_no_reflow() {
    Harness::reset_dev_log();
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LINUX_LIT_WORK", "H8"),
            // Seed the early-startup column guess (config writeback is suppressed
            // under LIT_HEADLESS_TEST, so the persisted value can't be relied on).
            ("LIT_START_COLUMNS", "2"),
        ],
    )
    .expect("launch linux-lit in cage");

    // Two-column needs the wide output or the layout never settles.
    let _ = h.set_output_size(1920, 1236);

    h.wait_for_viewport_rect(Duration::from_secs(10))
        .expect("app reported its reading-viewport rect");
    h.settle(Duration::from_millis(400));

    let log = h.read_dev_log();

    // The FIRST CARD_SIZING line decides the initial layout. With the count
    // seeded it must already be two-column — a `cols=1` first line is the reflow
    // glitch (single-column built first, then swapped).
    let first_card_sizing = log
        .lines()
        .find(|l| l.contains("CARD_SIZING:"))
        .unwrap_or("<none>");
    assert!(
        first_card_sizing.contains("cols=2"),
        "first CARD_SIZING pass was not two-column — the startup 1→2 reflow is \
         still present. First line: {first_card_sizing}\n\nfull log:\n{log}"
    );
}

/// H8 opens with a Prologue that fits in one column. On the FIRST spread it must
/// move to the RIGHT column (empty left): `column_split(state, 0).split == 0`.
/// The app logs `FIRST_SPREAD_SPLIT split=S page_end=E next=N` on reveal (under
/// LIT_HEADLESS_TEST). Assert `split=0` — the start-of-document mirror of the
/// EPILOGUE short-tail rule. (See docs/troubleshooting/page-turning-mechanics.md
/// → "Section-break clamping".)
#[test]
#[ignore = "needs cage + grim + wlr-randr; run with --ignored"]
fn short_prologue_fills_first_spread_right_column() {
    Harness::reset_dev_log();
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LINUX_LIT_WORK", "H8"),
            ("LIT_START_COLUMNS", "2"),
        ],
    )
    .expect("launch linux-lit in cage");

    let _ = h.set_output_size(1920, 1236);

    h.wait_for_viewport_rect(Duration::from_secs(10))
        .expect("app reported its reading-viewport rect");
    h.settle(Duration::from_millis(400));

    let log = h.read_dev_log();
    let split_line = log
        .lines()
        .find(|l| l.contains("FIRST_SPREAD_SPLIT"))
        .unwrap_or("<none>");
    assert!(
        split_line.contains("split=0"),
        "H8's short Prologue did not move to the first spread's RIGHT column \
         (expected `split=0`, i.e. empty left). Line: {split_line}\n\nfull log:\n{log}"
    );
}
