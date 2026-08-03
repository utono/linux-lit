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

/// The app must request fullscreen ITSELF, not rely on a compositor rule.
///
/// Regression guard for the dwl → niri migration (2026-08-01): fullscreen had
/// always been the compositor's job (dwl-mlj's tag rule in config.h, then
/// niri's `open-fullscreen true` window-rule), and the app only ever asked for
/// `.default_width(1000).default_height(800)`. An open-time WM rule is silent
/// when it is absent, so the reader mapped at 1000x800 on a niri session whose
/// config predated the rule. Pagination keys off the text-view height, so a
/// windowed reader is not a smaller reading experience — it is a different and
/// untested page grid.
///
/// **Deliberately a LOG assertion, not a pixel one.** A pixel check cannot
/// decide this: with a single window and `gaps 0`, niri tiles the window to
/// fill the entire output, which is pixel-identical to fullscreen (the harness
/// says as much on `window_is_fullscreen`). That was verified the hard way here
/// — a cream-at-row-0 version of this test passed identically against a build
/// with the `window.fullscreen()` call compiled out, i.e. it was green either
/// way and guarded nothing. Geometry is no better: niri 26.04 reports no
/// fullscreen flag, and the tiled window measures the same as a fullscreen one.
/// What IS decidable is whether the app made the request at all.
#[test]
#[ignore = "needs niri + cage; run with --ignored"]
fn app_requests_fullscreen_itself() {
    let h = start();
    h.settle(Duration::from_secs(3));

    let log = std::fs::read_to_string(h.log_path()).expect("read app log");

    // `start()` sets LIT_HEADLESS_TEST, which is exactly what suppresses the
    // request — so under the harness the suppression branch is the correct
    // one, and seeing it proves the gate is wired and evaluated. Asserting the
    // request fires would require dropping that env, which changes MPV and
    // reveal behavior for no gain.
    assert!(
        log.contains("STARTUP: fullscreen suppressed (LIT_HEADLESS_TEST)"),
        "the app-side fullscreen gate did not run. build_window must decide \
         fullscreen at startup (src/app/mod.rs). Log:\n{log}"
    );

    // The suppression must be keyed on LIT_HEADLESS_TEST ALONE. If it were
    // folded into the broader `hermetic_env` used elsewhere in that file — which
    // also includes `is_dev_mode()` — then LIT_DEV=1 would suppress it too, and
    // LIT_DEV=1 is how the app is launched for real desktop use (`crll`). That
    // would leave the dev build windowed: precisely the bug this guards.
    assert!(
        !log.contains("STARTUP: requested fullscreen (app-side)"),
        "fullscreen was requested despite LIT_HEADLESS_TEST — the niri \
         decoration test drives the window tiled and would be fought by this. \
         Log:\n{log}"
    );
}

/// The window must be able to SHRINK to whatever width the compositor gives it.
///
/// Regression guard for the fullscreen-collapse bug found on 2026-08-01: the
/// reader opened 1585x1200 on a 1920x1200 output, leaving a strip of wallpaper
/// down the right-hand side. `app_requests_fullscreen_itself` above was green
/// throughout — it only proves the *request* is made at startup (145ms in the
/// log), never that the resulting surface actually ends up output-sized.
///
/// The cause was a hard GTK minimum: `content_hbox.set_width_request(
/// config.column_width)` in `build_window` (src/app/mod.rs), plus
/// `target_card_width`'s `.max(cw_cfg)` floor (src/app/layout.rs). A child's
/// `width_request` propagates up as the WINDOW's minimum size, which GTK then
/// advertises to the compositor. A window whose minimum exceeds the space it is
/// given cannot honour fullscreen, and no amount of asking will fix it.
///
/// **Deliberately a GEOMETRY assertion, and note this contradicts the guidance
/// on the two tests above** — for this bug specifically, geometry is the only
/// oracle that works, because the failure IS a size:
///   - A pixel check is useless: 1585 of 1920 columns still paints a perfectly
///     normal-looking cream card, and `window_is_fullscreen` probes row 0 at the
///     horizontal CENTRE, which is cream in both the healthy and the broken
///     case. It reports `true` while a 335px strip of wallpaper sits off to the
///     right.
///   - A fullscreen-flag check is useless for the reason given above: niri 26.04
///     reports no such flag.
///
/// The probe is to shrink the column far below the suspected floor and see
/// whether the window follows. A healthy window tracks its tile; a
/// minimum-pinned one refuses and sits at its floor. Verified by hand against
/// the live session before writing this: `set-column-width 33%` on a 1920 output
/// (asking for 640px) left the window at exactly 1585.
/// KNOWN FAILING (2026-08-03) — see
/// `docs/superpowers/specs/2026-08-03-card-width-collapse-design.md`.
///
/// The card must carry a `width_request` or single-column prose collapses to one
/// word per line, and in GTK a `width_request` IS the window's minimum width —
/// measured in cage, every alternative (on `scrolled_overlay`, via
/// `min-content-width`, via margins) pins it identically. So the reader's
/// minimum is now 1098 and niri will not tile it narrower.
///
/// This guard passed on master only because master had no width at all: it was
/// green *because of* the collapse bug it does not test for. Fullscreen — the
/// thing the 2026-08-01 bug actually broke — is unaffected, because 1098 is far
/// below the output; that bug was a 1585 minimum.
///
/// Un-ignore when the custom `GtkLayoutManager` lands (report a small minimum,
/// allocate `card_w`), which is the only known way to satisfy this AND
/// `tests/card_width.rs` at once.
#[test]
#[ignore = "KNOWN FAILING: card width_request pins the window minimum at 1098 \
            (2026-08-03). Needs the custom LayoutManager; see the card-width \
            collapse spec. Also needs niri + cage."]
fn window_can_shrink_below_its_card_width() {
    let h = start();
    h.set_output_size(1920, 1236).expect("resize outer output");
    let _ = h.wait_for_viewport_rect(Duration::from_secs(12));

    // Fullscreen pins the window to the output, which would mask a minimum-size
    // floor below the output width. Tiled, the column governs the width.
    h.unfullscreen_window().expect("leave fullscreen");
    h.settle(Duration::from_millis(800));

    // 33% of 1920 = 640px, far below the ~1585 floor the bug produced and below
    // the default 1050 `column_width`. Chosen to be unambiguous: no plausible
    // rounding, gap, or chrome allowance explains a window this much too wide.
    h.niri_msg(&["action", "set-column-width", "33%"])
        .expect("set column width to 33%");
    h.settle(Duration::from_millis(900));

    let (_tile, win) = h
        .focused_tile_and_window_size()
        .expect("query tile/window size");
    let (out_w, _out_h) = h.output_size().expect("query output size");

    // Compare the window against the OUTPUT, never against its own tile.
    //
    // niri SHRINKS THE TILE TO FIT the window's advertised minimum: on the
    // broken build the probe reads `tile=(1585,1204) win=(1585,1204)` on a 1912
    // output, so `win <= tile` holds trivially and a tile-relative assertion is
    // green on exactly the bug it is meant to catch (confirmed by running it
    // that way first). The tile conforms to the window, not the reverse.
    //
    // The output does not conform to anything, which is what makes it a valid
    // oracle. After asking for a 33% column the window should be ~1/3 of the
    // output; anything near full width means it never shrank at all.
    let ceiling = out_w / 2;
    assert!(
        win.0 <= ceiling,
        "asked niri for a 33% column on a {out_w}px output (~{}px), but the \
         window is {}x{} — it did not shrink past ~{}px. A child \
         `width_request` (`content_hbox` in `apply_card_sizing`, and \
         `target_card_width`'s `.max(cw_cfg)` floor, both src/app/layout.rs) is \
         propagating up as the window's MINIMUM width, and niri shrinks the \
         TILE to match it rather than the other way round. NOTE (2026-08-03): \
         this is EXPECTED to fail at ~1098 while the card carries its \
         width_request — that request is what stops single-column prose \
         collapsing to one word per line. Only a failure ABOVE ~1098, or a \
         regression to ~1585 (the two-column floor, which breaks fullscreen \
         because it exceeds the output), is a new bug.",
        out_w / 3,
        win.0,
        win.1,
        ceiling
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
