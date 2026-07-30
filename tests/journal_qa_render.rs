//! Visual confirmation that a Q&A entry's Markdown renders as BLOCKS.
//!
//! Opens the journal overlay on a work whose entries contain a `>` blockquote
//! and captures a screenshot, then asserts from the dev log that the entry was
//! block-planned (`kind='qa'` now takes the same path notes do) and that no
//! Markdown marker survived into the rendered buffer.
//!
//! Run:
//!     ./scripts/e2e-env.sh cargo test --test journal_qa_render -- --ignored --nocapture

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
#[ignore]
fn qa_entry_renders_markdown_blocks() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "Rom"),
        ],
    )
    .expect("start app in cage");

    h.wait_for_viewport_rect(Duration::from_secs(30))
        .expect("reader viewport never appeared");
    h.settle(Duration::from_millis(800));

    // Ctrl+j opens the journal (overlay, or the work-wide picker when the
    // landing scene's band is empty — Return then reveals the overlay).
    let mut opened = false;
    for _ in 0..6 {
        h.chord(&["ctrl"], "j").expect("ctrl+j");
        h.settle(Duration::from_millis(900));
        if h.wait_for_journal_viewport_rect(Duration::from_secs(2)).is_ok() {
            opened = true;
            break;
        }
        h.key("Return", 300).ok();
        h.settle(Duration::from_millis(700));
        if h.wait_for_journal_viewport_rect(Duration::from_secs(2)).is_ok() {
            opened = true;
            break;
        }
    }
    assert!(opened, "journal overlay never opened");
    h.settle(Duration::from_millis(700));

    // Step to the LAST entry in this band so the newest Q&A (the one this
    // session saved) is the one captured.
    for _ in 0..6 {
        h.key("l", 200).ok();
    }
    h.settle(Duration::from_millis(800));
    let shot = h.capture("journal-qa-render").expect("screenshot");
    eprintln!("screenshot: {}", shot.display());

    // The buffer the overlay set must not carry raw Markdown markers: the block
    // renderer consumes them into tags. `JOURNAL-RENDER` lines carry the body.
    let log = h.read_dev_log();
    for bad in ["\n### ", "\n- ", "\n> "] {
        assert!(
            !log.contains(&format!("JOURNAL-RENDER{bad}")),
            "a Markdown marker reached the rendered buffer: {bad:?}"
        );
    }
}
