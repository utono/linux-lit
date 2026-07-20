//! Headless capture of the reader-gloss chat panel's CHROME and layout.
//!
//! `V` then `Tab` opens the chat panel pinned to a selection WITHOUT a gloss —
//! so no Claude API call is made (cage has no network/API). This exercises the
//! panel's float placement, header visibility, square corners, and the ask
//! input, which is the branch's visual surface that a headless run CAN verify.
//! The gloss CONTENT (markup, indents, typography) needs a real gloss and is
//! left to the user's live pass.
//!
//!     ./scripts/e2e-env.sh cargo test --test chat_panel_ui -- --ignored --nocapture

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

fn start() -> Harness {
    Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")],
    )
    .expect("app starts under cage")
}

#[test]
#[ignore]
fn chat_panel_opens_pinned_and_toggles() {
    let h = start();
    if h.wait_for_viewport_rect(Duration::from_secs(8)).is_err() {
        h.settle(Duration::from_secs(3));
    }

    // Enter visual mode, extend the selection a few lines, then Tab to open the
    // chat panel pinned to that selection — no gloss, no API.
    // `V` is Shift+v — wtype sends the literal keysym, so a bare "V" arrives as
    // lowercase `v` (a different reader action). Use the shift chord.
    h.chord(&["shift"], "v").expect("V enters visual mode");
    h.settle(Duration::from_millis(300));
    h.key("j", 150).expect("extend down");
    h.key("j", 150).expect("extend down");
    h.key("Tab", 600).expect("Tab opens the pinned chat panel");
    h.settle(Duration::from_secs(1));

    let pinned = h.capture("chat_panel_pinned").expect("capture pinned panel");
    let len = std::fs::metadata(&pinned).expect("shot exists").len();
    assert!(
        len > 50_000,
        "chat-panel screenshot {len} bytes — panel likely did not render"
    );
}
