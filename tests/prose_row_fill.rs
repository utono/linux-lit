//! Machine-checked no-text-loss e2e for prose visual-row pagination: driving
//! `x` through Bleak House (BH) must land on a sequence of pages whose stored
//! `prose_pages` rows tile EXACTLY — page N's exclusive end must equal page
//! N+1's start, with zero gap and zero overlap. This is the same invariant
//! `validate_prose_pages` checks at generation time, re-checked here from the
//! outside against the persisted lit.db rows, driven through the real app.
//!
//! Does NOT depend on `TEST_VIEWPORT_RECT`: in this environment the app's
//! reveal currently takes its 5s "load may be stuck" fallback path, which never
//! emits that rect (a known pre-existing issue, unrelated to prose pagination).
//! Instead this test settles for a fixed duration and parses the `PAGES_PROSE:`
//! log lines the app writes under `LIT_DEV` regardless of which reveal path ran.
//!
//! Run under the env wrapper:
//!     ./scripts/e2e-env.sh cargo test --test prose_row_fill -- --ignored --nocapture

mod harness;

use std::path::PathBuf;
use std::time::Duration;

use harness::Harness;
use rusqlite::Connection;

fn app_binary() -> PathBuf {
    if let Ok(p) = std::env::var("LINUX_LIT_BIN") {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_linux-lit") {
        return PathBuf::from(p);
    }
    PathBuf::from("target/debug/linux-lit")
}

fn lit_db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("utono/litdb/data/lit.db")
}

/// Poll the harness's dev log until `pred` matches or `timeout` elapses.
fn wait_for_log(
    h: &Harness,
    timeout: Duration,
    pred: impl Fn(&str) -> bool,
) -> Result<(), String> {
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

/// Send `keysym` and confirm the app actually logged `KEY: name=<expect_name>`
/// one more time than before — retrying up to 10 times if the virtual-
/// keyboard event was dropped (observed: the app runs a perpetual ~700ms
/// `BOTTOM_CLIP_ROWFILL` recompute tick even at rest, so there is no true
/// "quiet" window to wait for; occasional `wtype` events sent during a
/// recompute are simply dropped — no queueing, no KEY: line at all — but
/// most attempts land fine). Returns whether it landed; caller decides
/// severity (the `gg` calibration step must succeed, but a handful of `x`
/// presses failing to land is tolerable — the test's own `>= 10 transitions`
/// assertion is the real gate on that).
#[must_use]
fn send_key_reliably(h: &Harness, keysym: &str, expect_name: &str) -> bool {
    let needle = format!("KEY: name={expect_name} ");
    let before = h.read_dev_log().matches(&needle).count();
    for _ in 0..10 {
        h.key(keysym, 100).expect("wtype key press");
        h.settle(Duration::from_millis(300));
        let after = h.read_dev_log().matches(&needle).count();
        if after > before {
            return true;
        }
    }
    false
}

/// A `PAGES_PROSE: page K/N top=(l,o)` (or `(G) top=(l,o)`) sighting, in the
/// order visited.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Visited {
    page_no: i64, // K, 1-based — matches prose_pages.page_no directly
    line: i64,
    off: i64,
}

/// Parse every `PAGES_PROSE: page K/N ... top=(l,o)` line from the dev log, in
/// file order (== chronological order, since the harness truncates before
/// launch and the app only appends).
fn parse_visited_pages(log: &str) -> Vec<Visited> {
    let mut out = Vec::new();
    for line in log.lines() {
        let Some(rest) = line.split("PAGES_PROSE: page ").nth(1) else {
            continue;
        };
        // rest looks like "3/1583 top=(120,45)" or "3/1583 (G) top=(120,45)".
        let Some(slash) = rest.find('/') else { continue };
        let Ok(page_no) = rest[..slash].parse::<i64>() else {
            continue;
        };
        let Some(top_idx) = rest.find("top=(") else {
            continue; // "(at end)" / "(at start)" lines carry no position
        };
        let inner = &rest[top_idx + "top=(".len()..];
        let Some(close) = inner.find(')') else { continue };
        let mut parts = inner[..close].splitn(2, ',');
        let (Some(l), Some(o)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(line_v), Ok(off_v)) = (l.trim().parse::<i64>(), o.trim().parse::<i64>()) else {
            continue;
        };
        out.push(Visited { page_no, line: line_v, off: off_v });
    }
    out
}

/// The `layout_fingerprint` this run's prose table was generated/loaded at —
/// read from the most recent `PAGES_PROSE: generated ... fp=<fp>` or
/// `PAGES_PROSE: table hit (...) for <abbrev>` companion line. Generation logs
/// the fingerprint directly; a `table hit` doesn't, so on a hit we fall back to
/// querying lit.db for BH's newest `prose_pages_meta` row (there is exactly one
/// fingerprint per (work, geometry), and a freshly-launched harness is the only
/// writer touching BH in this run).
fn active_fingerprint(log: &str, conn: &Connection) -> String {
    if let Some(fp) = log
        .lines()
        .rev()
        .find_map(|l| l.split("PAGES_PROSE: generated ").nth(1))
        .and_then(|rest| rest.split("fp=").nth(1))
    {
        // Take only the fingerprint TOKEN, not the rest of the line. `fp=` is
        // no longer last: generation now appends `record_prose_pages_ms=…
        // total_ms=…` after it, and swallowing those into the fingerprint made
        // every lookup miss and this test report "generation did not persist"
        // when the rows were in fact present under the correct fingerprint.
        return fp.split_whitespace().next().unwrap_or("").to_string();
    }
    conn.query_row(
        "SELECT layout_fingerprint FROM prose_pages_meta \
         WHERE work_abbrev = 'BH' ORDER BY generated_at DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .expect("a prose_pages_meta row for BH must exist after this run (generated or table hit)")
}

#[derive(Debug, Clone, Copy)]
struct StoredRow {
    start_line: i64,
    start_off: i64,
    end_line: i64,
    end_off: i64,
}

fn load_stored_rows(conn: &Connection, fingerprint: &str) -> std::collections::HashMap<i64, StoredRow> {
    let mut stmt = conn
        .prepare(
            "SELECT page_no, start_line_id, start_row_offset, end_line_id, end_row_offset \
             FROM prose_pages WHERE work_abbrev = 'BH' AND layout_fingerprint = ?1",
        )
        .expect("prepare prose_pages query");
    let rows = stmt
        .query_map([fingerprint], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                StoredRow {
                    start_line: row.get(1)?,
                    start_off: row.get(2)?,
                    end_line: row.get(3)?,
                    end_off: row.get(4)?,
                },
            ))
        })
        .expect("query prose_pages rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect prose_pages rows");
    rows.into_iter().collect()
}

#[test]
#[ignore = "needs cage + grim + wtype; run with --ignored"]
fn prose_pages_tile_without_gaps() {
    // 1. Launch BH via the harness (LIT_HEADLESS_TEST, isolated log). Force
    //    generation at the harness's own headless geometry so a stored table
    //    exists for whatever fingerprint this run settles at, even if BH's
    //    lit.db table was previously stored at a different (e.g. real-monitor)
    //    geometry.
    let h = Harness::start_app(
        &app_binary(),
        std::iter::empty::<&str>(),
        &[
            ("LIT_DEV", "1"),
            ("LIT_HEADLESS_TEST", "1"),
            ("LIT_START_WORK", "BH"),
            ("LIT_GEN_PAGE_TABLE", "1"),
        ],
    )
    .expect("launch linux-lit in cage");

    // No TEST_VIEWPORT_RECT dependency (broken in this env — 5s reveal
    // fallback never emits it; even on a run where it DOES appear, waiting on
    // it isn't enough — see below). Poll the log instead of a fixed sleep.
    //
    // `LIT_GEN_PAGE_TABLE=1` forces `generate_and_store_prose` to run
    // regardless of whether a table was already loaded from lit.db, so the
    // ONE reliable "the table this run will use is now settled" signal is the
    // `PAGES_PROSE: generated ...` line specifically — NOT `table hit`, which
    // fires much earlier (a stale table loaded on startup, often before the
    // deferred-layout tick even runs) and would let the driving code proceed
    // while the real (possibly still-running) regeneration is still
    // competing for the main loop, silently dropping wtype's virtual-keyboard
    // events. Confirmed empirically: waiting only for `table hit` let `gg`/`x`
    // fire during the 2057ms-9023ms regeneration window and drop presses.
    wait_for_log(&h, Duration::from_secs(20), |log| {
        log.contains("PAGES_PROSE: generated")
    })
    .expect("prose table (re)generated for BH at this run's fingerprint");
    h.settle(Duration::from_secs(2));

    // 2. Send: "gg" then "x" (16 attempts, tolerating a few drops — see
    //    below). `wtype`'s virtual-keyboard events are transient (not queued
    //    by the compositor), so a keypress sent while
    //    the app's main loop is busy (observed: bursts of BOTTOM_CLIP_ROWFILL
    //    recompute right after generation, and after each page turn) is
    //    silently dropped — no KEY: line at all, not even a no-op. `KEY:
    //    name=<n>` is logged unconditionally at the top of `handle_key`, so
    //    counting occurrences of that exact line is a reliable "did it land"
    //    signal. `send_key_reliably` retries a press until the count goes up.
    assert!(send_key_reliably(&h, "g", "g"), "first `g` (PendingG) never registered");
    h.settle(Duration::from_millis(400));
    assert!(send_key_reliably(&h, "g", "g"), "second `g` (jump to start) never registered");
    h.settle(Duration::from_millis(600));

    // A handful of `x` presses occasionally failing to land (dropped virtual-
    // keyboard event during the app's perpetual recompute tick) is tolerable —
    // send extra attempts and let step 6's `>= 10 transitions` assertion be
    // the real gate, rather than treating every single press as load-bearing.
    let mut dropped_x = 0u32;
    for _ in 0..16 {
        if !send_key_reliably(&h, "x", "x") {
            dropped_x += 1;
            eprintln!("an `x` press failed to register after 10 attempts (dropped_x={dropped_x})");
        }
        h.settle(Duration::from_millis(500));
    }
    h.settle(Duration::from_millis(500));

    // 3. Read the log; collect PAGES_PROSE "top=(l,o)" tuples in order.
    let log = h.read_dev_log();
    let visited = parse_visited_pages(&log);
    assert!(
        !visited.is_empty(),
        "no `PAGES_PROSE: page K/N top=(l,o)` lines found — is the prose table \
         active? full log:\n{log}"
    );

    // 4. Open lit.db read-only; load prose_pages rows for BH at the run's
    //    fingerprint; map page_no -> (start,end).
    let conn = Connection::open_with_flags(
        lit_db_path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("open lit.db read-only");
    let fingerprint = active_fingerprint(&log, &conn);
    let stored = load_stored_rows(&conn, &fingerprint);
    assert!(
        !stored.is_empty(),
        "no prose_pages rows stored for BH at fingerprint {fingerprint:?} — \
         table generation did not persist; full log:\n{log}"
    );

    // Sanity: the visited page_no must have a stored row (the log's `top=(l,o)`
    // uses in-memory BUFFER line indices, while the stored row's
    // start_line_id/end_line_id are `line_mapping.id`s — not directly
    // comparable — so cross-check what IS comparable across both encodings:
    // the pixel offset within the line, which page_for_position resolves
    // identically either way).
    for v in &visited {
        let row = stored.get(&v.page_no).unwrap_or_else(|| {
            panic!(
                "visited page_no={} has no stored row at fingerprint {fingerprint:?}",
                v.page_no
            )
        });
        assert_eq!(
            row.start_off, v.off,
            "visited page {} logged top=({},{}) but the stored row's start_off is {} \
             (page_no match but offset mismatch would mean we're reading the wrong row)",
            v.page_no, v.line, v.off, row.start_off
        );
    }

    // 5. For each consecutive visited pair (a, b): the STORED row whose
    //    page_no == a must have end == the STORED row whose page_no == b's
    //    start (exclusive-end tiling — zero gap, zero overlap). Compared
    //    entirely in DB-native `line_mapping.id` + offset space (both sides of
    //    this comparison come from `stored`, so the buffer-vs-id encoding
    //    mismatch from step 4 doesn't apply here).
    let mut transitions = 0usize;
    for w in visited.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Repeated turns can log the SAME page twice (e.g. an at-end no-op) —
        // only count genuine forward transitions.
        if a.page_no == b.page_no {
            continue;
        }
        let row_a = stored[&a.page_no];
        let row_b = stored[&b.page_no];
        assert_eq!(
            (row_a.end_line, row_a.end_off),
            (row_b.start_line, row_b.start_off),
            "no-text-loss violation: stored page {} ends at ({},{}) but the \
             next visited stored page {} starts at ({},{}) — gap or overlap",
            a.page_no,
            row_a.end_line,
            row_a.end_off,
            b.page_no,
            row_b.start_line,
            row_b.start_off
        );
        transitions += 1;
        println!(
            "tiled OK: page {} end=({},{}) == page {} start=({},{})",
            a.page_no, row_a.end_line, row_a.end_off, b.page_no, row_b.start_line, row_b.start_off
        );
    }

    // 6. Assert at least 10 turns actually happened (guards silent no-ops).
    assert!(
        transitions >= 10,
        "only {transitions} real page-forward transitions observed (need >= 10) \
         — turns may be silently no-op'ing; visited={visited:?}\nfull log:\n{log}"
    );
}
