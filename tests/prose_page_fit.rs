//! Machine-checked PIXEL-FIT e2e for prose pagination: every page the reader
//! actually renders must fit the card, with real breathing room at the bottom.
//!
//! This is the fit counterpart to `prose_row_fill.rs`. That test proves stored
//! pages TILE (no text lost between pages); this one proves each page FITS
//! (no page overflows the card, none is packed flush to the bottom rule). Both
//! failures observed on BH-Barrett 2026-08-05 were invisible to the tiling
//! test — the pages tiled perfectly and still rendered wrong:
//!
//!   1. OVERFLOW — a stored page measured TALLER than the card at render time,
//!      so `paged_bottom_clip` floored the clip to 0 and the last line poked
//!      out unmasked (`CLIP_WARN: main-card prose-1col OVERFLOW total=1188 >
//!      widget_h=1128 clip=0`). Reproduced on a FRESHLY generated, validated
//!      table: generation and render compute algebraically identical sums, so
//!      the per-line HEIGHTS differed between the two moments.
//!
//!      ROOT CAUSE (measured 2026-08-05): generation ran before the buffer-wide
//!      `font-size` TextTag was in effect, so `line_yrange` returned heights for
//!      a smaller face — every 4+ row paragraph short by ~29px. The deltas sum
//!      to the whole overflow. NOTE the census cannot see this: it reads the
//!      same wrong heights generation used, so it reports `over_usable=0` while
//!      the bug is fully present. Only a RENDER-side assertion catches it,
//!      which is what `deep_prose_pages_never_overflow_the_card` exists for.
//!   2. NO BREATHING ROOM — a page packed to `usable + fit_slack` with real ink
//!      instead of spending the slack on a blank inter-paragraph gap, leaving
//!      the final paragraph flush against the bottom rule (observed clip=48 on
//!      a page whose healthy neighbours clip 57-84).
//!
//! Why the log and not pixels: the app logs `BOTTOM_CLIP_EXACT` /
//! `BOTTOM_CLIP_ROWFILL` with the exact numbers the clip decision used
//! (`total`, `widget_h`, `clip`), which is strictly MORE precise than measuring
//! a screenshot — and it attributes a failure to the page that caused it. The
//! project's clip-prevention doc requires pixel verification for ACCEPTANCE of
//! a clipping fix; this test is a regression tripwire on the decision inputs,
//! so a failure here always names a concrete page to go look at.
//!
//! Run under the env wrapper, one cage test at a time:
//!     ./scripts/e2e-env.sh cargo test --test prose_page_fit -- --ignored --nocapture

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

/// Poll the harness's dev log until `pred` matches or `timeout` elapses.
fn wait_for_log(h: &Harness, timeout: Duration, pred: impl Fn(&str) -> bool) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let log = h.read_dev_log();
        if pred(&log) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("timed out waiting for log condition; full log:\n{log}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Send `keysym`, retrying while the virtual-keyboard event gets dropped. Same
/// rationale as `prose_row_fill.rs`: the app runs a perpetual clip-recompute
/// tick, so `wtype` events sent during one are silently dropped (no `KEY:`
/// line at all). Counting `KEY: name=<n>` is the reliable landed signal.
#[must_use]
fn send_key_reliably(h: &Harness, keysym: &str, expect_name: &str) -> bool {
    let needle = format!("KEY: name={expect_name} ");
    let before = h.read_dev_log().matches(&needle).count();
    for _ in 0..10 {
        h.key(keysym, 100).expect("wtype key press");
        h.settle(Duration::from_millis(300));
        if h.read_dev_log().matches(&needle).count() > before {
            return true;
        }
    }
    false
}

/// One `BOTTOM_CLIP_EXACT` sighting — the numbers the paged clip decided from.
#[derive(Debug, Clone, Copy)]
struct ExactClip {
    widget_h: i64,
    total: i64,
    clip: i64,
    page_top: i64,
    top_off: i64,
    end: i64,
}

/// Pull `key=value` out of a whitespace-separated log tail.
fn field(rest: &str, key: &str) -> Option<i64> {
    rest.split_whitespace()
        .find_map(|tok| tok.strip_prefix(key)?.parse::<i64>().ok())
}

/// Parse every `BOTTOM_CLIP_EXACT: widget_h=.. total=.. allowance=.. clip=..
/// page_top=.. top_off=.. end=..` line, in chronological (file) order.
fn parse_exact_clips(log: &str) -> Vec<ExactClip> {
    log.lines()
        .filter_map(|line| {
            let rest = line.split("BOTTOM_CLIP_EXACT: ").nth(1)?;
            Some(ExactClip {
                widget_h: field(rest, "widget_h=")?,
                total: field(rest, "total=")?,
                clip: field(rest, "clip=")?,
                page_top: field(rest, "page_top=")?,
                top_off: field(rest, "top_off=")?,
                end: field(rest, "end=")?,
            })
        })
        .collect()
}

/// The single-column prose fill budget this run used, read from any
/// `BOTTOM_CLIP_ROWFILL: ... usable=N ...` line (constant for a given geometry).
fn parse_usable(log: &str) -> Option<i64> {
    log.lines()
        .find_map(|line| field(line.split("BOTTOM_CLIP_ROWFILL: ").nth(1)?, "usable="))
}

/// The generation-time fit census: `PAGES_PROSE_DRIFT: summary pages=N
/// over_usable=K worst=page P at Wpx usable=U slack=S`.
///
/// This covers EVERY page in the table, which is why the assertions below key
/// on it rather than only on the pages a drive happens to visit. Learned the
/// hard way: a 14-turn drive visited 14 of 764 pages and missed all 34
/// over-budget ones, so the suite passed while the table was demonstrably
/// over-packed. Driving cannot be made exhaustive (a novel is ~800 pages at
/// ~0.5s/turn), so the census is the only sound gate.
#[derive(Debug, Clone, Copy)]
struct DriftSummary {
    pages: i64,
    over_usable: i64,
    worst_page: i64,
    worst_px: i64,
    usable: i64,
    slack: i64,
}

fn parse_drift_summary(log: &str) -> Option<DriftSummary> {
    let rest = log
        .lines()
        .rev()
        .find_map(|l| l.split("PAGES_PROSE_DRIFT: summary ").nth(1))?;
    // "... worst=page 103 at 1114px usable=1107 slack=14"
    let worst_page = rest
        .split("worst=page ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let worst_px = rest
        .split(" at ")
        .nth(1)?
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .split_whitespace()
        .next()?
        .trim_end_matches("px")
        .parse()
        .ok()?;
    Some(DriftSummary {
        pages: field(rest, "pages=")?,
        over_usable: field(rest, "over_usable=")?,
        worst_page,
        worst_px,
        usable: field(rest, "usable=")?,
        slack: field(rest, "slack=")?,
    })
}

/// Drive BH forward through `turns` pages and return the dev log. Shared by
/// both fit assertions so one cage launch covers both (these tests contend
/// over the compositor — see CLAUDE.md's one-at-a-time rule).
fn drive_forward(turns: usize) -> String {
    drive_forward_from(turns, None)
}

/// `drive_forward`, optionally LANDING DEEP first via `LIT_START_SCENE`.
///
/// Sampling from page 1 is not sufficient coverage. The BH-Barrett overflow
/// lives at pages 335+; a 14-turn drive from the start visits pages 1-15 and
/// misses every over-budget page, which is why the original suite passed green
/// while the bug was fully present. A deep landing is the only way this test
/// exercises the pages the user actually reported.
fn drive_forward_from(turns: usize, start_scene: Option<&str>) -> String {
    let mut env: Vec<(&str, &str)> = vec![
        ("LIT_DEV", "1"),
        ("LIT_HEADLESS_TEST", "1"),
        ("LIT_START_WORK", "BH"),
        // Force generation at THIS run's headless geometry, so the table
        // under test is the one this card height actually needs.
        ("LIT_GEN_PAGE_TABLE", "1"),
    ];
    if let Some(scene) = start_scene {
        env.push(("LIT_START_SCENE", scene));
    }
    let h = Harness::start_app(&app_binary(), std::iter::empty::<&str>(), &env)
        .expect("launch linux-lit in cage");

    // PRODUCTION GEOMETRY IS LOAD-BEARING — do not drop this resize.
    // cage defaults to 1280x720, which yields a ~591px text view and a
    // COMPLETELY different page grid. Both bugs this test guards reproduce only
    // at the real card height (`widget_h=1128`, `usable=1071`): a 720p run
    // paginates fine and passes vacuously. The resize must happen BEFORE
    // waiting on generation, so the table is built at this geometry.
    //
    // 1200, not 1236 (corrected 2026-08-11). This comment used to cite
    // CLAUDE.md's "1236 (not 1200)" rule, which was itself wrong and has since
    // been corrected. Measured on BH, same build, generation env:
    //   1920x1236 -> text_view 1142, usable=1107
    //   1920x1200 -> text_view 1106, usable=1071  <- the target above
    // So 1236 ran this suite 36px off its OWN documented target, on a page
    // grid no real session ever sees. 1200 also matches BH's stored
    // fingerprint (v5|...|1920x1200|...|1106|uh1071), which is the grid the
    // reader actually loads.
    let resized = h
        .set_output_size(1920, 1200)
        .expect("wlr-randr resize must run");
    assert!(
        resized,
        "wlr-randr could not set 1920x1200 — without production geometry this \
         test passes vacuously (720p paginates fine). Refusing to run."
    );
    h.settle(Duration::from_secs(1));

    // `generated` (not `table hit`) is the only reliable "the table this run
    // uses is settled" signal under LIT_GEN_PAGE_TABLE — driving during the
    // regeneration window drops keypresses.
    wait_for_log(&h, Duration::from_secs(30), |log| {
        log.contains("PAGES_PROSE: generated")
    })
    .expect("prose table (re)generated for BH at this run's fingerprint");
    h.settle(Duration::from_secs(2));

    // Confirm the resize actually reached the text view, so a silent geometry
    // regression can never turn this suite green by accident.
    let widget_h = parse_exact_clips(&h.read_dev_log())
        .first()
        .map(|c| c.widget_h)
        .or_else(|| parse_usable(&h.read_dev_log()).map(|u| u + 57));
    if let Some(wh) = widget_h {
        assert!(
            wh > 900,
            "text view is only {wh}px tall — the 1920x1200 resize did not take \
             effect, so this run cannot reproduce the production page grid"
        );
    }

    assert!(send_key_reliably(&h, "g", "g"), "first `g` (PendingG) never registered");
    h.settle(Duration::from_millis(400));
    assert!(send_key_reliably(&h, "g", "g"), "second `g` (jump to start) never registered");
    h.settle(Duration::from_millis(600));

    for _ in 0..turns {
        // A dropped `x` just means one fewer page sampled — not a failure. The
        // assertions below gate on how many pages we actually observed.
        let _ = send_key_reliably(&h, "x", "x");
        h.settle(Duration::from_millis(400));
    }
    h.settle(Duration::from_millis(600));
    let log = h.read_dev_log();
    // Preserve the run's log: the harness tempdir is deleted on Drop, and
    // without a copy a failure (or a suspicious PASS) can't be diagnosed.
    if let Ok(dir) = std::env::var("PROSE_FIT_LOG_DIR") {
        let _ = std::fs::write(PathBuf::from(dir).join("prose-fit-run.log"), &log);
    }
    log
}

/// No rendered prose page may overflow the card. This is bug #1: the render
/// side measured a stored page TALLER than `widget_h`, so the clip floored to
/// 0 and the overflowing last line rendered unmasked.
///
/// Asserted two ways, because each catches the failure at a different layer:
/// the app's own `CLIP_WARN` tripwire (which names the offending page), and the
/// raw `total <= widget_h` relation from `BOTTOM_CLIP_EXACT` (which holds even
/// if the warning's gating ever changes).
#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored"]
fn prose_pages_never_overflow_the_card() {
    let log = drive_forward(14);

    let clips = parse_exact_clips(&log);
    assert!(
        !clips.is_empty(),
        "no `BOTTOM_CLIP_EXACT` lines found — is single-column prose table mode \
         active? full log:\n{log}"
    );

    // The app's own tripwire. Every occurrence is a real overflow.
    let warns: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("CLIP_WARN") && l.contains("prose-1col OVERFLOW"))
        .collect();
    assert!(
        warns.is_empty(),
        "{} prose page(s) overflowed the card (clip floored to 0 — the last line \
         renders unmasked). This is clip-prevention.md #12 on the PROSE path; if \
         the table was freshly generated this run, the cause is a render-vs-\
         generation height disagreement, NOT a stale table. Offending lines:\n{}",
        warns.len(),
        warns.join("\n")
    );

    // Independent of the warning's own gating: the measured page must fit.
    for c in &clips {
        assert!(
            c.total <= c.widget_h,
            "page at top=({},{}) end={} measured total={}px against widget_h={}px \
             (over by {}px, clip={}). A page taller than the card cannot be masked.",
            c.page_top,
            c.top_off,
            c.end,
            c.total,
            c.widget_h,
            c.total - c.widget_h,
            c.clip
        );
    }
}

/// The SAME overflow invariant, exercised on the DEEP pages where the reported
/// bug actually lives (BH-Barrett ch. 26, pages ~335+).
///
/// This is the regression guard for the generation-time FONT TIMING defect:
/// prose generation measured per-line heights before the buffer-wide
/// `font-size` TextTag was in effect, so every multi-row paragraph was charged
/// ~29px short. Pages built from those heights tile perfectly and satisfy the
/// generation-time census (`over_usable=0` — it reads the same wrong heights,
/// so it agrees with itself), then render 44-127px too tall and floor the clip
/// to 0.
///
/// Landing deep is load-bearing: the front of the book is short dialogue lines
/// (1-3 rows) that are unaffected by a per-paragraph re-wrap, so a drive from
/// page 1 cannot see this no matter how many turns it takes.
#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored"]
fn deep_prose_pages_never_overflow_the_card() {
    // ch. 26 == buffer lines ~2960-3020 == the pages in the user's report.
    let log = drive_forward_from(8, Some("26.0"));

    let clips = parse_exact_clips(&log);
    assert!(
        !clips.is_empty(),
        "no `BOTTOM_CLIP_EXACT` lines found at the deep landing — did \
         LIT_START_SCENE=26.0 resolve? full log:\n{log}"
    );

    let warns: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("CLIP_WARN") && l.contains("prose-1col OVERFLOW"))
        .collect();
    assert!(
        warns.is_empty(),
        "{} DEEP prose page(s) overflowed the card. The table was generated \
         this run, so this is a generation-vs-render height disagreement — \
         check that prose generation measures AFTER the font tag is applied \
         (PAGES_PROSE_PANGO / GEN_HEIGHTS under LIT_TRACE_*). Offending:\n{}",
        warns.len(),
        warns.join("\n")
    );

    for c in &clips {
        assert!(
            c.total <= c.widget_h,
            "deep page at top=({},{}) end={} measured total={}px against \
             widget_h={}px (over by {}px, clip={})",
            c.page_top,
            c.top_off,
            c.end,
            c.total,
            c.widget_h,
            c.total - c.widget_h,
            c.clip
        );
    }
}

/// The SAME overflow invariant, exercised on chapter 37 — the chapter the user
/// actually reported (text sliced mid-glyph, no masking band). A PRIOR fix's
/// acceptance test (`deep_prose_pages_never_overflow_the_card` above) lands on
/// chapter 26 and is green; chapter 37 has 22 pages overflowing the card by
/// 13-114px on the pre-fix sweep. Chapter 26 happening to be healthy is exactly
/// how that defect stayed green — a deep sample of ONE chapter is not coverage,
/// this pins the chapter that actually failed.
///
/// The known-bad pages ran continuously from buffer line ~4223 to 4402+ (22
/// pages); 20 turns from the chapter-37 landing is enough to walk into and
/// through that span at BH's prose density, but the census below is the real
/// gate (it covers every stored page, not just the ones this drive visits) —
/// see the file-level doc comment on why a sampled drive alone is insufficient.
#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored"]
fn chapter_37_pages_never_overflow_the_card() {
    let log = drive_forward_from(20, Some("37.0"));

    let clips = parse_exact_clips(&log);
    assert!(
        !clips.is_empty(),
        "no `BOTTOM_CLIP_EXACT` lines found at the deep landing — did \
         LIT_START_SCENE=37.0 resolve? full log:\n{log}"
    );

    let overflows: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("CLIP_WARN") && l.contains("OVERFLOW"))
        .collect();
    assert!(
        overflows.is_empty(),
        "chapter 37 overflowed {} page(s):\n{}",
        overflows.len(),
        overflows.join("\n")
    );

    // Independent of the warning's own gating: the measured page must fit.
    for c in &clips {
        assert!(
            c.total <= c.widget_h,
            "ch37 page at top=({},{}) end={} measured total={}px against \
             widget_h={}px (over by {}px, clip={})",
            c.page_top,
            c.top_off,
            c.end,
            c.total,
            c.widget_h,
            c.total - c.widget_h,
            c.clip
        );
    }
}

/// Launch, resize, land at `start_scene` (if given), force generation, wait
/// for `PAGES_PROSE: generated`, and return the dev log. Unlike
/// `drive_forward_from` this does NOT drive any page turns — the determinism
/// property under test is about GENERATION itself, not the live walk, so the
/// only thing that matters is that the table gets (re)built once per launch.
///
/// A fresh `Harness` per call is deliberate: each call is a completely
/// separate process launch, which is the axis the determinism bug lives on
/// (scroll history / GTK validation state carried over from a PRIOR run
/// cannot explain what's observed — the bug is that a SINGLE cold generation
/// depends on incidental main-loop timing).
fn generate_table_at(w: u32, h: u32, work: &str, start_scene: Option<&str>) -> String {
    let mut env: Vec<(&str, &str)> = vec![
        ("LIT_DEV", "1"),
        ("LIT_HEADLESS_TEST", "1"),
        ("LIT_START_WORK", work),
        ("LIT_GEN_PAGE_TABLE", "1"),
    ];
    if let Some(scene) = start_scene {
        env.push(("LIT_START_SCENE", scene));
    }
    let h_app = Harness::start_app(&app_binary(), std::iter::empty::<&str>(), &env)
        .expect("launch linux-lit in cage");

    let resized = h_app
        .set_output_size(w, h)
        .expect("wlr-randr resize must run");
    assert!(
        resized,
        "wlr-randr could not set {w}x{h} — without production geometry this \
         run cannot reproduce the bug"
    );
    h_app.settle(Duration::from_secs(1));

    wait_for_log(&h_app, Duration::from_secs(30), |log| {
        log.contains("PAGES_PROSE: generated") || log.contains("VALIDATE_FAIL")
    })
    .expect("prose table generation must settle (generated or VALIDATE_FAIL) within 30s");
    h_app.settle(Duration::from_secs(1));

    h_app.read_dev_log()
}

/// Pull the page count out of the LAST `PAGES_PROSE: generated N pages for
/// ...` line in the log (there is exactly one per successful generation in
/// these single-launch runs).
fn parse_generated_page_count(log: &str) -> Option<usize> {
    log.lines()
        .rev()
        .find_map(|l| l.split("PAGES_PROSE: generated ").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
}

/// Generation must be a pure function of content + geometry. Before the fix
/// the same fingerprint produced 801, 806 and 808 pages on three runs,
/// because the height sweep read whatever GTK happened to have validated —
/// a provisional estimate for lines far from the viewport at generation time.
///
/// One cage launch per iteration (not one launch driven 3 times) is
/// deliberate: each launch is a fresh cold generation, which is the exact
/// circumstance the race was observed in (freshly opened work, viewport
/// nowhere near the deep chapter being measured).
#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored"]
fn prose_generation_is_deterministic_across_runs() {
    let mut counts = Vec::new();
    for _ in 0..3 {
        // 1200 to match the rest of the suite (see the geometry note in
        // `run_fit_census`). Determinism only needs the geometry held CONSTANT
        // across the three runs, but there is no reason to measure it on a
        // grid no session ever renders.
        let log = generate_table_at(1920, 1200, "BH", Some("37.0"));
        counts.push(
            parse_generated_page_count(&log)
                .unwrap_or_else(|| panic!("PAGES_PROSE: generated N pages must appear; full log:\n{log}")),
        );
        std::thread::sleep(Duration::from_secs(3));
    }
    assert!(
        counts.iter().all(|c| *c == counts[0]),
        "generation is not deterministic: {counts:?} — same content, same \
         geometry, different tables"
    );
}

/// Every rendered prose page must leave real breathing room below its last
/// line. This is bug #2: a page spent the `prose_fit_slack` tolerance on INK
/// rather than on a blank inter-paragraph gap, so the final paragraph sat
/// flush against the card's bottom rule.
///
/// The slack exists to let a boundary advance past a fully-fitting row into
/// the following ink-free gap (`prose_fit_slack`'s doc comment). Spending it on
/// ink is what produces the flush page, so the invariant is stated against the
/// fill budget `usable` — NOT `usable + slack`: a page whose content exceeds
/// `usable` has, by definition, consumed space the fill decision reserved.
#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored"]
fn prose_pages_keep_bottom_breathing_room() {
    let log = drive_forward(14);

    // Gate on the WHOLE-TABLE census, not just the pages this drive visited.
    // A 14-turn drive samples 14 of ~760 pages; the first version of this test
    // did exactly that and passed while 34 pages were over-packed.
    let s = parse_drift_summary(&log).unwrap_or_else(|| {
        panic!(
            "no `PAGES_PROSE_DRIFT: summary` line — the generation-time fit census \
             is the gate for this test (it needs LIT_DEV/debug logging and a run \
             that actually GENERATES the table); full log:\n{log}"
        )
    });
    assert!(
        s.pages > 100,
        "census covered only {} pages — expected the full BH table; full log:\n{log}",
        s.pages
    );

    assert_eq!(
        s.over_usable, 0,
        "{} of {} stored prose pages are packed past the fill budget usable={}px \
         (worst: page {} at {}px, {}px over). Those pages render with the final \
         paragraph flush against the card's bottom rule and no breathing room — \
         the reader sees text touching the edge.\n\n\
         `prose_fit_slack` ({}px) exists so a boundary may advance past a fully-\
         fitting row into the following INK-FREE inter-paragraph gap; the \
         validator's `usable + slack` tolerance assumes that overshoot carries no \
         ink. When the overshoot is real text instead, the page is genuinely \
         overfull and the clip shrinks to nothing. Fix the boundary decision so \
         the slack is only spent on blank space, not the tolerance that hides it.",
        s.over_usable, s.pages, s.usable, s.worst_page, s.worst_px,
        s.worst_px - s.usable, s.slack
    );
}
