//! Journal passage-Q&A verse-source spacing, headless capture.
//!
//! A passage Q&A on a VERSE work prepends the quoted source (speaker, verse
//! block, citation) above the question. The verse lines must render at
//! pure line-height — consecutive, no blank line between them — matching the
//! main reading card, while blank-line gaps still separate the speaker, quote,
//! and citation blocks. Regression guard for the "verse lines double
//! spaced in the overlay" bug (each `<verse>` line used to be its own paragraph,
//! joined by "\n\n").
//!
//! This test drives Cym (entry Cym.1.1.1, a 3-line FIRST GENTLEMAN speech) into
//! the journal overlay and CAPTURES the page for visual review — the spacing is
//! a judgment call best confirmed by eye. It also asserts ink stays within the
//! content band (no tag-margin escape).
//!
//! Run:
//!     ./scripts/e2e-env.sh cargo test --test journal_verse_source -- --ignored --nocapture

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
fn passage_verse_source_lines_render_tight() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "Cym"),
        ],
    )
    .expect("launch linux-lit in cage");

    let _ = h.set_output_size(1920, 1200);
    let _ = h
        .wait_for_viewport_rect(Duration::from_secs(20))
        .expect("app reported its reading-viewport rect");

    // Into Act 1 Scene 1 (front matter has no journal scene context).
    h.key("3", 250).expect("3 -> next chapter");
    h.settle(Duration::from_millis(400));

    // Open the journal overlay for the cursor's scene band. On an empty scene
    // band Ctrl+j opens the work-wide picker instead — confirm the first row.
    h.chord(&["ctrl"], "j").expect("Ctrl+j -> journal overlay/picker");
    if h
        .wait_for_journal_viewport_rect(Duration::from_secs(4))
        .is_err()
    {
        h.key("Return", 250).expect("Return -> confirm picker row");
        let _ = h
            .wait_for_journal_viewport_rect(Duration::from_secs(8))
            .expect("journal overlay opened after picker confirm");
    }
    h.settle(Duration::from_millis(500));

    let region = h
        .wait_for_journal_viewport_rect(Duration::from_secs(8))
        .expect("journal overlay viewport rect");
    let band = h
        .wait_for_journal_content_band(Duration::from_secs(4))
        .expect("journal content band");

    // Page across the scene band's entries (Ctrl+n) to land on a passage Q&A
    // whose source is a verse block. Capture each so the reviewer can pick the
    // verse-source page; assert the ink stays inside the column band throughout.
    h.capture("verse_src_entry00").expect("capture entry 0");
    h.assert_ink_within_band("verse_src_band00", region, band, 0.0)
        .expect("entry 0: ink within band");
    for n in 1..=8u32 {
        h.chord(&["ctrl"], "n").expect("Ctrl+n -> next entry");
        h.settle(Duration::from_millis(600));
        h.capture(&format!("verse_src_entry{n:02}"))
            .expect("capture entry");
        h.assert_ink_within_band(&format!("verse_src_band{n:02}"), region, band, 0.0)
            .expect("entry: ink within band");
    }
}
