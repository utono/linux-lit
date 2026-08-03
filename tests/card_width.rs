//! The main reading card must be allocated the width `apply_card_sizing`
//! COMPUTES, not the natural (minimum) width of its wrapping text view.
//!
//! Run under the env wrapper, single-threaded, one binary at a time:
//!     ./scripts/e2e-env.sh cargo test --test card_width -- --ignored --nocapture
//!
//! ## The bug this guards
//!
//! `content_hbox` is `halign: Center`, and every widget from it down to the
//! text view carried `width_request = -1`. A centered GTK4 box is allocated its
//! NATURAL width, and `hexpand` on a descendant does not override an ancestor's
//! centering. For a `WrapMode::Word` text view inside a ScrolledWindow with
//! `hscrollbar-policy = Never`, that natural width collapses to the child's
//! MINIMUM — the widest unbreakable word. Single-column prose rendered one word
//! per line in a ~280px card on a 1920px window (2026-08-03, BH-Barrett).
//!
//! `apply_card_sizing` computed `card_w=1050` correctly and logged it, then
//! discarded it: it set margins and `hexpand` but never applied the width to
//! any widget.
//!
//! ## Why the oracle is the VIEWPORT RECT, not the log
//!
//! `CARD_SIZING: card_w=…` reports what the code INTENDED. That number was
//! already correct while the bug was on screen, so asserting on it passes on
//! the broken build. `TEST_VIEWPORT_RECT` is the text view's real allocation,
//! which is the thing that was wrong. Assert on the allocation, never on the
//! intent.
//!
//! This is the same trap as backlog #13: a green test against the wrong
//! surface. See CLAUDE.md, "Verify against the VISIBLE surface".

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

/// Production geometry for the CAGE harness (the reader is `.decorated(false)`).
/// See CLAUDE-activeContext.md: 1920x1200 -> text_view 1128. Do NOT port the
/// 1236/1098 pair from the cargo harness — it is decorated and measures
/// differently.
const OUT_W: u32 = 1920;
const OUT_H: u32 = 1200;

#[test]
#[ignore = "needs cage + grim; run with --ignored --test-threads=1"]
fn single_column_card_fills_its_computed_width() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")],
    )
    .expect("launch linux-lit in cage");

    h.set_output_size(OUT_W, OUT_H).expect("resize output");

    let (_x, _y, w, _hgt) = h
        .wait_for_viewport_rect(Duration::from_secs(12))
        .expect("app reported its reading-viewport rect");

    // A BAND, not a floor. Both failure modes are real and they are opposite,
    // so a one-sided assertion misses one of them:
    //
    //   281px  — collapsed (no width set anywhere; the 2026-08-03 bug)
    //   1872px — filled the whole window (halign: Fill with capped margins;
    //            caught only because this assertion gained an upper bound)
    //   ~1050  — correct, the configured 1-column reading measure
    //
    // The viewport is the text view's allocation, so it sits a little under
    // `card_w`; the band is wide enough to absorb margins and gutter without
    // admitting either failure.
    assert!(
        (900..=1200).contains(&w),
        "reading viewport is {w}px wide on a {OUT_W}px output; expected ~1050 \
         (the configured 1-column measure). Below the band the card collapsed \
         to the text view's near-zero minimum width (nothing in the chain from \
         content_hbox down to the text view set a width). Above it the card \
         filled the window instead of holding its computed width. See \
         docs/superpowers/specs/2026-08-03-card-width-collapse-design.md"
    );

    // Degenerate wrapping also shows up as an absurd content height: 17 lines
    // measured 8566px against a 1128px widget when each word took its own row.
    // The clip tripwire caught it, so treat a fresh OVERFLOW as a failure too.
    let log = h.read_dev_log();
    let overflow = log
        .lines()
        .filter(|l| l.contains("CLIP_WARN") && l.contains("OVERFLOW"))
        .count();
    assert_eq!(
        overflow, 0,
        "clip tripwire fired {overflow} OVERFLOW warning(s) — a `total` an \
         order of magnitude over widget_h means degenerate one-word-per-line \
         wrapping (near-zero layout width), not an overfull page. \
         See docs/troubleshooting/clip-prevention.md"
    );
}
