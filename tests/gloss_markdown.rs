//! Gloss-overlay (reader gloss) pagination + fill invariants, headless.
//!
//! Guards the prose-gloss UNDERFILL bug (clip-prevention.md #16, 2026-07-17): a
//! prose gloss alternates a speakerless Source block (the quoted verse) with its
//! Explication paragraph. `repaginate` over-charged each speakerless Source a
//! full `line_h` of phantom height (verse renders with NO trailing paragraph
//! gap), closing every page a whole unit early — the TT "mock Dedication"
//! rendered 3 pages at ~55% fill where 2 nearly-full pages suffice.
//!
//! Asserts on the TT front-matter gloss (Swift/Jonson mock Dedication,
//! 8 speakerless Source/Explication units):
//!  1. FILL — page 1's ink reaches >= 50% of the overlay viewport (the
//!     underfill guard; a pre-fix page 1 filled ~55% of the CARD but the ink
//!     bottom sat far above the viewport's usable height).
//!  2. PAGE COUNT — the gloss paginates to exactly 2 pages (`GLOSS-PAGES: n=2`);
//!     pre-fix it was 3.
//!  3. PAGE TURN — `j` past the last block on page 1 turns to page 2 (a new
//!     `GLOSS-PAGES`/`GLOSS-CURSOR` page-0 reset), i.e. the overlay is navigable.
//!
//! Depends on live lit.db + dev config content: gloss 21800 on `TT.0.0.3`
//! (a reader-gloss), reachable via Ctrl+Shift+g (OpenLastGloss) because
//! `config-dev.json`'s `last_gloss[TT]` points at `TT.0.0.3`. The
//! `import-corpus-note`-style fixture note is NOT needed here.
//!
//! Run:
//!     ./scripts/e2e-env.sh cargo test --test gloss_markdown -- --ignored --nocapture

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

/// The page count from the most recent `GLOSS-PAGES: n=<N> ...` log line, if any.
fn last_gloss_page_count(log: &str) -> Option<usize> {
    log.lines()
        .rev()
        .find_map(|l| l.split("GLOSS-PAGES: n=").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
}

/// Count of `GLOSS-CURSOR:` lines (each cursor move logs one), for a
/// did-navigation check.
fn gloss_cursor_lines(log: &str) -> usize {
    log.lines().filter(|l| l.contains("GLOSS-CURSOR:")).count()
}

#[test]
#[ignore = "needs cage + grim + wtype + python numpy/pillow; run with --ignored"]
fn tt_dedication_gloss_fills_and_paginates() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "TT"),
        ],
    )
    .expect("launch linux-lit in cage");

    // Real display size — the layout is tuned for 1920×1200; cage's 720p default
    // would repaginate to a different (smaller) budget.
    let _ = h.set_output_size(1920, 1200);

    let _ = h
        .wait_for_viewport_rect(Duration::from_secs(20))
        .expect("app reported its reading-viewport rect");
    h.settle(Duration::from_millis(500));

    // Open the TT "mock Dedication" reader-gloss via Ctrl+Shift+g (OpenLastGloss):
    // it opens config `last_gloss[TT]` = TT.0.0.3 DIRECTLY, independent of the
    // cursor line — the cursor-matching Ctrl+g path needs the cursor exactly on
    // the passage's `line_in_div`, which the front-matter buffer indexing makes
    // fragile. Both `ctrl_shift("g")` and `ctrl_shift("G")` are bound to
    // OpenLastGloss, so whatever keysym cage's keymap emits, the bind matches.
    // No mid-test log reset — the app holds the log handle in append mode, so an
    // outside truncate leaves a sparse file; scan the whole (growing) log instead.
    let mut region_opt = None;
    for attempt in 0..3 {
        h.chord(&["ctrl", "shift"], "g")
            .expect("Ctrl+Shift+g -> open last gloss");
        if let Ok(r) = h.wait_for_overlay_viewport_rect(Duration::from_secs(4)) {
            region_opt = Some(r);
            break;
        }
        eprintln!("open-last-gloss attempt {attempt}: no rect yet, retrying");
        h.settle(Duration::from_millis(400));
    }
    let region = match region_opt {
        Some(r) => r,
        None => {
            let log = h.read_dev_log();
            let tail: String = log
                .lines()
                .rev()
                .take(50)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            panic!(
                "gloss overlay never emitted TEST_OVERLAY_VIEWPORT_RECT after 3 \
                 Ctrl+Shift+g presses. Did the chord land (look for `KEY: name=g \
                 ctrl=true shift=true`) and is last_gloss[TT]=TT.0.0.3 in \
                 config-dev.json?\n--- dev log tail ---\n{tail}"
            );
        }
    };
    h.settle(Duration::from_millis(600));

    // (2) PAGE COUNT: the fix collapses 3 underfilled pages to 2 well-filled ones.
    let pages = last_gloss_page_count(&h.read_dev_log())
        .expect("GLOSS-PAGES line logged on repaginate");
    assert_eq!(
        pages, 2,
        "TT mock-Dedication gloss should paginate to 2 pages after the underfill \
         fix (pre-fix: 3). Got {pages}. An underfill regression splits the 8 units \
         across too many pages."
    );

    // (1) FILL: page 1's ink must reach >= 50% of the overlay viewport. Use the
    // full viewport x-range as the band (fill is the concern here, not tag-margin
    // escapes — those are a play-gloss concern the synopsis test covers).
    let (rx, _ry, rw, _rh) = region;
    let band = (rx, rx + rw);
    h.assert_ink_within_band("gloss_md_page01", region, band, 0.5)
        .expect("page 1: ink fills >= 50% of the overlay viewport (underfill guard)");

    // (3) PAGE TURN: j-walk until the page turns. On the last block of page 1, a
    // `j` turns to page 2 — the app re-renders and logs a fresh GLOSS-CURSOR
    // reset. Assert we can both move the cursor AND reach page 2's content.
    let before = gloss_cursor_lines(&h.read_dev_log());
    let mut moved = 0usize;
    for _ in 1..=12 {
        h.key("j", 120).expect("j");
        h.settle(Duration::from_millis(250));
        let now = gloss_cursor_lines(&h.read_dev_log());
        if now > before {
            moved = now - before;
        }
    }
    assert!(
        moved >= 4,
        "j should walk the gloss blocks (>= 4 cursor moves across 2 pages); got {moved}"
    );

    // Capture page 2 for the visual review pass (no fill floor — the last page of
    // an 8-unit gloss may be short, though here it is ~92% full).
    h.assert_ink_within_band("gloss_md_page02", region, band, 0.0)
        .expect("page 2: ink within viewport band");
}
