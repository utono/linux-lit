//! Journal corpus-note Markdown rendering invariants, headless.
//!
//! Guards the three bug classes the first on-screen review found:
//!  1. TEXT ESCAPES THE COLUMN — a styled block (list item / quote) whose
//!     TextTag left-margin replaced the view's centering margin renders at the
//!     view's far-left edge, outside the inset panel. Asserted per page via
//!     the ink-band pixel check (`TEST_JOURNAL_CONTENT_BAND`).
//!  2. PHANTOM j PRESSES — the cursor unit (raw md paragraphs) disagreed with
//!     the rendered blocks, so j presses were swallowed by headings/rules.
//!     Asserted from the dev log: EVERY j press must log a NEW
//!     `JOURNAL-CURSOR:` line with a CHANGED cursor index, until the end.
//!  3. GROSS UNDERFILL — measured-vs-rendered height mismatch packed pages at
//!     a fraction of the budget. First page must fill ≥ 50% of the viewport.
//!
//! Depends on live lit.db content: the Shakespeare author-scope note (the
//! `import-corpus-note` skill's "Loading the Cry" import). Ham (Shakespeare)
//! is the entry work.
//!
//! Run:
//!     ./scripts/e2e-env.sh cargo test --test journal_markdown -- --ignored --nocapture

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

/// Parse every `JOURNAL-CURSOR: ... full#N ...` WHOLE-ENTRY cursor index from
/// the dev log, in order. (`cursor#` is page-local and legitimately repeats
/// across page turns; only `full#` detects a swallowed press.)
fn cursor_indices(log: &str) -> Vec<usize> {
    log.lines()
        .filter_map(|l| l.split(" full#").nth(1))
        .filter_map(|rest| {
            rest.split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
        })
        .collect()
}

#[test]
#[ignore = "needs cage + grim + wtype + python numpy/pillow; run with --ignored"]
fn corpus_note_markdown_renders_and_navigates() {
    Harness::reset_dev_log();
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "Ham"),
        ],
    )
    .expect("launch linux-lit in cage");

    // Run at the user's real display size: at cage's tiny 720p default the
    // heading-glue pagination correctly yields a title-only page 1, which
    // trips the fill assertion; 1920×1200 matches the device the layout is
    // tuned for.
    let _ = h.set_output_size(1920, 1200);

    let _ = h
        .wait_for_viewport_rect(Duration::from_secs(8))
        .expect("app reported its reading-viewport rect");

    // Advance into a scene first (front matter has no journal scene context)
    // and give the freshly-mapped window time to take focus — premature wtype
    // keys are dropped silently (same flow as tests/journal_clipping.rs).
    h.key("3", 250).expect("3 -> next chapter");
    h.settle(Duration::from_millis(400));

    // Open the journal overlay, then jump to the AUTHOR band (the corpus note).
    h.chord(&["ctrl"], "j").expect("Ctrl+j -> journal overlay");
    let _ = h
        .wait_for_journal_viewport_rect(Duration::from_secs(8))
        .expect("journal overlay opened (TEST_JOURNAL_VIEWPORT_RECT)");
    h.settle(Duration::from_millis(400));

    Harness::reset_dev_log(); // fresh log: the author band emits its own rects
    h.chord(&["alt"], "a").expect("Alt+a -> author band");
    let region = h
        .wait_for_journal_viewport_rect(Duration::from_secs(8))
        .expect("author band rendered (TEST_JOURNAL_VIEWPORT_RECT)");
    let band = h
        .wait_for_journal_content_band(Duration::from_secs(4))
        .expect("author band content band (TEST_JOURNAL_CONTENT_BAND)");
    h.settle(Duration::from_millis(600));

    // Page 1: all ink inside the panel band AND reasonably full (≥ 50%).
    h.assert_ink_within_band("journal_md_page01", region, band, 0.5)
        .expect("page 1: ink within column band + filled");

    // j-walk the whole note. Every press must move the bar (a NEW cursor log
    // line with a CHANGED index) — a press that logs nothing means we reached
    // the end; a press that logs the SAME index is a phantom stop (the bug).
    let mut seen = cursor_indices(&h.read_dev_log()).len();
    let mut last_idx = cursor_indices(&h.read_dev_log()).last().copied();
    let mut moves = 0usize;
    for press in 1..=80 {
        h.key("j", 120).expect("j");
        h.settle(Duration::from_millis(250));
        let log = h.read_dev_log();
        let all = cursor_indices(&log);
        if all.len() == seen {
            // No new cursor line — end of the note. Legitimate only after
            // at least a few successful moves.
            assert!(
                moves >= 5,
                "j stopped moving after only {moves} moves (press {press}) — phantom stops?"
            );
            break;
        }
        let new_idx = *all.last().unwrap();
        assert_ne!(
            Some(new_idx),
            last_idx,
            "press {press}: cursor logged but did NOT move (index {new_idx} repeated) — phantom stop"
        );
        seen = all.len();
        last_idx = Some(new_idx);
        moves += 1;

        // Every few moves (covers each page as j turns them): band check.
        if moves % 4 == 0 {
            h.assert_ink_within_band(
                &format!("journal_md_walk{moves:02}"),
                region,
                band,
                0.0,
            )
            .expect("mid-walk: ink within column band");
        }
    }
    assert!(moves >= 5, "expected a multi-block walk, got {moves} moves");

    // Final page: band check (no fill requirement — last page may be short).
    h.assert_ink_within_band("journal_md_last", region, band, 0.0)
        .expect("last page: ink within column band");
}
