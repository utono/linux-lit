//! End-to-end guard for the journal-qa clipboard blob, headless.
//!
//! Drives the REAL keybind in a cage compositor and reads back the Wayland
//! clipboard, covering what the `journal_blob_tests` unit tests cannot: that
//! the bind is reachable, resolves live reader state, and puts the v1 contract
//! the litdb `journal-qa` skill parses onto the clipboard.
//!
//! Scope here is the DIVISION blob (`Ctrl+y` with no selection). The PASSAGE
//! blob rides the same cap in visual mode, but `V` cannot be delivered through
//! wtype (see the note in the test body), so its construction is unit-tested
//! instead — it shares `selection_passage_args` with the Ctrl+a ask card.
//!
//! Also pins the reassignment: `CopyLineMappingId` moved to `Ctrl+Shift+y`
//! (2026-07-29), so that chord must still copy the bare `<id> <media_id>` pair
//! and NOT a JSON blob.
//!
//! Run:
//!     ./scripts/e2e-env.sh cargo test --test journal_qa_blob -- --ignored --nocapture

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

/// The entry work: a Shakespeare play, so the blob's work_type is `play` and
/// the division label is an `act.scene` pair.
const WORK: &str = "Rom";

#[test]
#[ignore] // needs ./scripts/e2e-env.sh (cage + wtype + wl-paste)
fn ctrl_y_copies_division_blob_and_shift_keeps_id_bind() {
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", WORK),
        ],
    )
    .expect("start app in cage");

    h.wait_for_viewport_rect(Duration::from_secs(30))
        .expect("reader viewport never appeared");
    h.settle(Duration::from_millis(600));

    // --- Division blob: Ctrl+y with NO selection ------------------------------
    // Retry the press until the app logs the copy: a chord sent while the work
    // is still loading is dropped before it reaches the key handler.
    let mut copied = false;
    for _ in 0..8 {
        h.chord(&["ctrl"], "y").expect("ctrl+y");
        h.settle(Duration::from_millis(600));
        if h.read_dev_log().contains("JOURNAL-QA: copied division blob") {
            copied = true;
            break;
        }
    }
    assert!(
        copied,
        "Ctrl+y never produced a division blob; dev log tail:\n{}",
        tail_log(&h, 12)
    );
    let div = read_clipboard(&h);
    let dv: serde_json::Value =
        serde_json::from_str(&div).unwrap_or_else(|e| panic!("division blob is not JSON: {e}\n{div}"));

    assert_eq!(dv["v"], 1, "blob contract version");
    assert_eq!(dv["scope"], "division", "no selection => division scope");
    assert_eq!(dv["work_abbrev"], WORK);
    assert_eq!(dv["work_type"], "play");
    assert!(
        dv["start_citation"].is_null() && dv["end_citation"].is_null(),
        "a division blob carries no citations: {dv}"
    );
    assert!(dv["source_text"].is_null(), "a division blob has no excerpt");
    assert!(
        !dv["division_text"].as_str().unwrap_or("").is_empty(),
        "division_text must carry the scene context the user message needs"
    );

    // `division_label` is the reader's DISPLAY label (a play reads
    // "Act 3, Scene 1", not "3.1"); the skill puts it in the user message's
    // "<Unit>: <label>" line, so it must be non-empty but is not a citation.
    assert!(
        !dv["division_label"].as_str().unwrap_or("").is_empty(),
        "division_label must carry the reader's display label"
    );

    // NOTE — the PASSAGE blob (visual-mode Ctrl+y) is deliberately NOT driven
    // here. Entering visual mode requires the `V` keysym, and wtype cannot
    // deliver a shifted letter the way GTK does: `-k V` arrives as lowercase
    // ("v", shift=false) and fires OpenSegmentVim, while `-M shift -k v`
    // arrives as ("v", shift=true) — a pairing real GTK never emits, which
    // `KeyMap::lookup` matches to nothing (it strips shift only for keys whose
    // name is ALREADY uppercase). So the selection can't be established from
    // this harness. The passage blob's construction is covered instead by the
    // `journal_blob_tests` unit tests beside `journal_blob_json`, and its
    // context builder is the very same `selection_passage_args` that Ctrl+a
    // uses in production.

    // --- The reassigned debug bind still works -------------------------------
    let mut id_copied = false;
    for _ in 0..8 {
        h.chord(&["ctrl", "shift"], "y").expect("ctrl+shift+y");
        h.settle(Duration::from_millis(600));
        if h.read_dev_log().contains("CLIPBOARD: copied ") {
            id_copied = true;
            break;
        }
    }
    assert!(
        id_copied,
        "Ctrl+Shift+y never ran CopyLineMappingId; dev log tail:\n{}",
        tail_log(&h, 12)
    );
    let ids = read_clipboard(&h);
    // `<line_mapping_id>` alone when the work has no active media, else
    // `<line_mapping_id> <media_id>`. Every whitespace-separated token is an
    // integer — which a JSON blob's `{"v": 1, ...}` never is. (Don't test this
    // with serde_json: a bare `1131947` IS valid JSON.)
    let toks: Vec<&str> = ids.split_whitespace().collect();
    assert!(
        !toks.is_empty() && toks.iter().all(|t| t.parse::<i64>().is_ok()),
        "Ctrl+Shift+y must copy '<line_mapping_id> [media_id]', got {ids:?}"
    );
    assert!(
        !ids.contains("\"v\""),
        "Ctrl+Shift+y must NOT copy a journal-qa blob: {ids}"
    );

    assert!(
        h.read_dev_log().contains("JOURNAL-QA: copied division blob"),
        "division copy must be logged"
    );
}

/// Last `n` lines of the dev log, for diagnosing a bind that did not fire.
fn tail_log(h: &Harness, n: usize) -> String {
    let log = h.read_dev_log();
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Read the cage session's clipboard via `wl-paste`, retrying briefly.
///
/// `wl-copy` serves the selection from a background process, so a read issued
/// immediately after the bind fires can land before that process owns the
/// selection ("No selection" on stderr). Retry rather than flake.
fn read_clipboard(h: &Harness) -> String {
    let mut last = String::new();
    for _ in 0..10 {
        match h.clipboard_text() {
            Ok(s) if !s.trim().is_empty() => return s,
            Ok(_) => last = "clipboard empty".into(),
            Err(e) => last = e.to_string(),
        }
        h.settle(Duration::from_millis(300));
    }
    panic!("clipboard never became readable: {last}");
}
