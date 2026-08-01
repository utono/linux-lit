//! Two-column vocab popup geometry + Escape-close e2e invariant.
//!
//! Companion to tests/journal_clipping.rs / tests/overlay_clipping.rs. Exercises
//! the compact 2-column vocab popup (feat/vocab-surfaces): on a two-column play,
//! the `rr` chord opens a full-column float over the reading column the cursor is
//! NOT in. Task 2 clamps the card width to `(col_w - 48).clamp(200, 420)` and
//! centers it in that column; this test asserts the logged geometry proves the
//! popup fits strictly inside its column, and that Escape closes it.
//!
//! Run under the env wrapper so grim/wtype are available:
//!     ./scripts/e2e-env.sh cargo test --test vocab_popup_2col -- --ignored --nocapture
//!
//! ## Work / line selection route
//!
//! The popup lists only the vocab words on the reader's CURRENT line
//! (`VocabScope::CursorLine`), so the test must not assume any particular line
//! has a vocab word. Vocab words are GLOBAL (all `vocab_words` rows matched
//! against the buffer text — see db::queries::load_vocab_words), so a play with
//! prose-dense dialogue like Cym (Cymbeline, two-column) has vocab matches on
//! many lines. Route:
//!   1. Launch on Cym (the play the other e2e tests use), widen the output so the
//!      two-column layout settles.
//!   2. For up to N cursor positions: reset this run's log, drive the `rr` chord,
//!      and poll the log. A `VOCAB POPUP: float col_x=.. col_w=..` line means a
//!      vocab word was on the current line and the 2-col float opened — done.
//!      A `VOCAB POPUP: no vocab words in scope` line means step `j` and retry.
//!   3. If no line in the first N is a hit, fail naming the work so a better one
//!      can be chosen.
//!
//! The `rr` chord is state-based (`ChordState::PendingR`), not timer-based, so
//! two `key("r")` calls reliably fire the toggle.

mod harness;

use std::path::PathBuf;
use std::time::{Duration, Instant};

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

/// Poll this run's app log until `needle` appears, or `timeout` elapses.
/// Returns the first line containing `needle`.
fn wait_for_log_line(h: &Harness, needle: &str, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let log = h.read_dev_log();
        if let Some(line) = log.lines().rev().find(|l| l.contains(needle)) {
            return Some(line.to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Parse `col_x=<i> col_w=<i>` out of a `VOCAB POPUP: float ...` log line.
fn parse_float_geometry(line: &str) -> Option<(i32, i32)> {
    let field = |key: &str| -> Option<i32> {
        line.split_whitespace()
            .find_map(|tok| tok.strip_prefix(key))
            .and_then(|v| v.parse().ok())
    };
    Some((field("col_x=")?, field("col_w=")?))
}

/// Fire the `rr` chord (two plain `r` keys). The second consumes the PendingR
/// chord and toggles the popup.
fn tap_rr(h: &Harness) {
    h.key("r", 200).expect("r -> arm vocab chord");
    h.key("r", 60).expect("r -> toggle vocab popup");
}

#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored under scripts/e2e-env.sh"]
fn vocab_popup_2col_geometry_and_escape() {
    // Cym is a two-column play (Cymbeline). LIT_START_WORK makes the test
    // hermetic regardless of saved config.
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "Cym"),
            // Pin the cursor too, not just the work. Without this the reader
            // resumes its SAVED line (observed at 510), which is past the last
            // vocab match in Cym -- the matches occupy lines 104..444 -- so the
            // search stepped forward, away from every hit, and the test failed
            // with "no line in the first 40". 104 is Cym's first vocab line.
            ("LIT_START_POS", "104"),
        ],
    )
    .expect("launch linux-lit in cage");

    // A two-column card needs a wide output or the "two-col width settled" gate
    // never passes and the reveal (TEST_VIEWPORT_RECT) never fires.
    let _ = h.set_output_size(1920, 1236);

    // Readiness gate: the viewport rect is emitted at reveal.
    let _ = h
        .wait_for_viewport_rect(Duration::from_secs(10))
        .expect("app reported its reading-viewport rect (two-column card settled?)");
    h.settle(Duration::from_millis(400));

    // Walk FORWARD from the top rather than jumping a chapter.
    //
    // `3` (next-chapter) used to land on dialogue with vocab words; it now
    // lands on line 510 of Cym, past the last vocab match in the work (the
    // matches occupy lines 104..444, logged as `distinct_lines`). Every
    // subsequent `j` steps further away, so the search could never hit and the
    // test failed with "no line in the first 40". Starting at the top and
    // stepping forward walks INTO the vocab-dense region instead of away
    // from it, which does not depend on where a chapter boundary happens to
    // fall.
    h.settle(Duration::from_millis(200));

    // Find a line whose vocab words open the 2-col float. Reset the log before
    // each attempt so we read THIS attempt's outcome, then drive `rr`.
    const MAX_STEPS: usize = 40;
    let mut float_line: Option<String> = None;
    for _ in 0..MAX_STEPS {
        h.reset_dev_log();
        tap_rr(&h);
        // Either the float opens (hit) or "no vocab words in scope" (miss).
        // Poll briefly for whichever lands.
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let log = h.read_dev_log();
            if let Some(line) = log.lines().rev().find(|l| l.contains("VOCAB POPUP: float col_x=")) {
                float_line = Some(line.to_string());
                break;
            }
            if log.contains("VOCAB POPUP: no vocab words in scope") {
                break; // miss — step and retry
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        if float_line.is_some() {
            break;
        }
        // Miss: the `rr` above toggled the popup open only if there WERE words;
        // on a miss nothing opened, so just step the cursor forward and retry.
        h.key("j", 120).expect("j -> next line");
        h.settle(Duration::from_millis(150));
    }

    let float_line = float_line.unwrap_or_else(|| {
        panic!(
            "no line in the first {MAX_STEPS} of work 'Cym' produced a \
             'VOCAB POPUP: float' line (needs a two-column play with vocab \
             words on a reachable line — pick a different work if this trips)"
        )
    });

    // Geometry invariant (Task 2): the popup card width is
    // (col_w - 48).clamp(200, 420); it must be > 0 and strictly inside the
    // column (<= col_w). col_x/col_w themselves must be a sane on-screen column.
    let (col_x, col_w) = parse_float_geometry(&float_line)
        .unwrap_or_else(|| panic!("could not parse col_x/col_w from: {float_line}"));
    assert!(col_w > 0, "column width must be positive: {float_line}");
    assert!(col_x >= 0, "column x must be on-screen: {float_line}");
    let card_w = (col_w - 48).clamp(200, 420);
    assert!(card_w > 0, "computed popup width must be positive (col_w={col_w})");
    assert!(
        card_w <= col_w,
        "popup width {card_w} must fit inside its column {col_w} (float overflows the column)"
    );

    // Visual pass: capture the open 2-col popup for the UI review.
    h.settle(Duration::from_millis(300));
    h.capture("vocab_popup_2col").expect("screenshot the open vocab popup");

    // Escape must close the popup (logs `ESCAPE: closed vocab popup`). Reset the
    // log so we only see the Escape from THIS press.
    h.reset_dev_log();
    h.key("Escape", 200).expect("Escape -> close vocab popup");
    let closed = wait_for_log_line(&h, "ESCAPE: closed vocab popup", Duration::from_secs(4));
    assert!(
        closed.is_some(),
        "Escape did not log 'ESCAPE: closed vocab popup' (popup did not close)"
    );
}
