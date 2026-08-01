//! End-to-end smoke test for linux-lit under the **niri** window manager.
//!
//! Counterpart to `smoke.rs` (which uses cage). Cage is a kiosk compositor: it
//! force-fullscreens its single client, which papers over anything that depends
//! on real window-manager behavior. niri is what the user actually runs, so
//! these tests catch what cage cannot — window decorations, tiling geometry,
//! and whether the app still reveals when the WM does NOT hand it a fullscreen
//! surface unasked.
//!
//! niri has no headless backend (it is Smithay, not wlroots), so the harness
//! nests it inside cage: `cage → niri → linux-lit`. See `tests/harness/niri.rs`.
//!
//! Marked `#[ignore]` so a normal `cargo test` on a box without niri/cage/grim
//! stays green. Run explicitly:
//!
//!     ./scripts/e2e-env.sh cargo test --test niri_smoke -- --ignored --nocapture

mod harness;

use std::path::PathBuf;
use std::time::Duration;

use harness::niri::NiriHarness;

/// Resolve the binary under test. Override with LINUX_LIT_BIN, else use the path
/// Cargo injects for the `linux-lit` bin target, else fall back to debug.
fn app_binary() -> PathBuf {
    if let Ok(p) = std::env::var("LINUX_LIT_BIN") {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_linux-lit") {
        return PathBuf::from(p);
    }
    PathBuf::from("target/debug/linux-lit")
}

/// Launch linux-lit inside nested niri. LIT_DEV=1 → dev id + dev log;
/// LIT_HEADLESS_TEST=1 → MPV is skipped (no window to cover the reader).
fn start() -> NiriHarness {
    NiriHarness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")],
    )
    .expect("launch linux-lit inside nested niri")
}

#[test]
#[ignore = "needs niri + cage + grim; run with --ignored"]
fn launches_and_renders_the_reader_under_niri() {
    let h = start();

    // Production geometry. Resizes the OUTER cage output — niri's winit output
    // inherits its size from the parent, so its own `mode` directive is inert.
    let _ = h.set_output_size(1920, 1236);

    // Gate on the app actually revealing (it emits the viewport rect at reveal).
    if h.wait_for_viewport_rect(Duration::from_secs(12)).is_err() {
        h.settle(Duration::from_secs(4));
    }

    let shot = h.capture("niri-smoke").expect("screenshot under niri");

    // A blank/failed capture is tiny; a rendered reading card is large. Same
    // floor the cage smoke test uses to distinguish "painted" from "blank".
    let bytes = std::fs::metadata(&shot).expect("stat screenshot").len();
    assert!(
        bytes > 50_000,
        "screenshot {} is only {bytes} bytes — the reader almost certainly did not paint under niri",
        shot.display()
    );
}

/// niri reports the real output geometry over IPC, so we can assert the resize
/// actually took rather than trusting wlr-randr's exit status. This guards the
/// harness's own load-bearing quirk: a `mode` in niri's config does nothing,
/// and only resizing the outer cage output moves the app's viewport.
#[test]
#[ignore = "needs niri + cage; run with --ignored"]
fn output_resize_reaches_niri() {
    let h = start();

    let (w0, h0) = h.output_size().expect("query niri outputs before resize");
    assert!(
        w0 > 0 && h0 > 0,
        "niri reported a degenerate output size {w0}x{h0}"
    );

    h.set_output_size(1920, 1236).expect("resize outer cage output");
    h.settle(Duration::from_millis(800));

    let (w, h_px) = h.output_size().expect("query niri outputs after resize");
    assert!(
        w >= 1900,
        "niri's output is {w}x{h_px} after requesting 1920x1236 — the resize did not \
         reach the nested compositor, so two-column layout will never settle"
    );
}

/// The app builds its window with `.decorated(false)`, so no titlebar should be
/// drawn. cage cannot test this at all — it force-fullscreens its single client,
/// and a fullscreen window has no decorations by definition.
///
/// **This must be a PIXEL check, not a geometry check.** GTK's titlebar is
/// CLIENT-side: it is painted inside the window's own surface, so niri reports
/// identical `tile_size` and `window_size` either way and its IPC cannot see it.
/// Verified during bring-up by temporarily rebuilding with `.decorated(true)`:
/// an IPC-geometry assertion stayed green, while the pixel scan below caught a
/// 37px white titlebar (dark title text at rows ~17-23) above the cream card.
///
/// Note the harness config deliberately does NOT set `prefer-no-csd` — that
/// would suppress the titlebar regardless of what the app asks for, making this
/// test vacuous. See `tests/harness/niri-test.kdl`.
#[test]
#[ignore = "needs niri + cage + grim; run with --ignored"]
fn window_has_no_client_side_titlebar() {
    let h = start();
    let _ = h.set_output_size(1920, 1236);
    let _ = h.wait_for_viewport_rect(Duration::from_secs(12));

    // CRITICAL: leave fullscreen. Both `start_app` and `set_output_size`
    // fullscreen the window (the app's reveal needs it), and a fullscreen
    // window is undecorated by definition — so a decoration check made while
    // fullscreen passes no matter what. This was verified twice during
    // bring-up: with `.decorated(true)` temporarily restored, the test stayed
    // green until this call was added.
    h.unfullscreen_window().expect("leave fullscreen");
    h.settle(Duration::from_millis(800));

    let png = h.capture("niri-undecorated").expect("capture");

    // Scan a vertical strip down the window's horizontal centre. Row 0 is NOT a
    // useful probe on its own: the app paints its own teal root margin around
    // the reading card, so the top row is legitimately dark whether decorated
    // or not (an early version of this test compared row 0 to the card colour
    // and would have failed on correct code).
    //
    // The titlebar's distinguishing signature is a BRIGHT, near-neutral band at
    // the very top — measured at ~(255,255,255) for 37 rows, with dark title
    // text inside it. The undecorated build shows teal root (~(46,89,99)) from
    // row 0 until the cream card begins. So: look for a bright neutral band in
    // the top 60 rows. Neither the teal root nor the cream card qualifies as
    // "bright neutral" (cream is warm: R-B ≈ 21).
    let strip = column_strip(&png, 60).expect("read capture strip");
    let titlebar_rows = strip.iter().filter(|&&c| is_bright_neutral(c)).count();

    assert!(
        titlebar_rows < 8,
        "found {titlebar_rows} bright neutral rows in the top 60 of the window \
         — a client-side title bar appears to be painted above the reading \
         card. The window should be undecorated. Strip sample: {:?}. See {}",
        &strip[..strip.len().min(45)],
        png.display()
    );
}

/// Colours down the vertical centre line of a PNG, rows `0..n`.
fn column_strip(png: &std::path::Path, n: u32) -> Option<Vec<(u8, u8, u8)>> {
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(
            "import sys;from PIL import Image;im=Image.open(sys.argv[1]).convert('RGB');\
             x=im.size[0]//2;\
             print(' '.join('%d,%d,%d'%im.getpixel((x,y)) for y in range(min(int(sys.argv[2]),im.size[1]))))",
        )
        .arg(png)
        .arg(n.to_string())
        .output()
        .ok()?;
    let v: Vec<(u8, u8, u8)> = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|t| {
            let mut p = t.split(',').filter_map(|n| n.parse::<u8>().ok());
            Some((p.next()?, p.next()?, p.next()?))
        })
        .collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// A GTK titlebar is bright and colour-neutral (~#ffffff / #f6f5f4). The teal
/// root is dark; the cream card is bright but warm (R noticeably above B).
fn is_bright_neutral((r, g, b): (u8, u8, u8)) -> bool {
    let min = r.min(g).min(b);
    let spread = r.max(g).max(b) - min;
    min >= 235 && spread <= 8
}
