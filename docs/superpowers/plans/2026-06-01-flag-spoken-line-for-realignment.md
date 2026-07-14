# Flag Spoken Lines for Targeted Re-alignment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user mark a missed-but-spoken line with `u` in linux-lit, then fill its timestamp on a targeted `align-forced --spoken-no-ts` wizard re-run that uses the manual mark as a search anchor and ±1.0s overwrite guard.

**Architecture:** Two repos. (1) linux-lit's `u` bind gains a `line_spoken_status` upsert (`is_spoken=1`) beside its existing manual-timestamp write; dual-media mirroring is handled by existing SQLite triggers. (2) whisper-transcript's `bin/align-forced` gains a `--spoken-no-ts` flag that scopes alignment to spoken-but-untimestamped lines, anchors on their manual timestamps via the existing windowed matcher, and overwrites a manual timestamp only when the aligned result is within 1.0s.

**Tech Stack:** Rust + rusqlite (linux-lit), Python 3 + sqlite3 + pytest (whisper-transcript), shared SQLite db at `~/utono/litdb/data/lit.db`.

**Spec:** `docs/superpowers/specs/2026-06-01-flag-spoken-line-for-realignment-design.md`

## File Structure

linux-lit (cwd `~/utono/linux-lit`):
- `src/db/queries.rs` — add `upsert_spoken_status`; add unit test in existing `#[cfg(test)] mod tests`.
- `src/input/timestamps.rs` — call `upsert_spoken_status` from `set_start_time`; set `line.is_spoken` in memory.

whisper-transcript (cwd `~/utono/whisper-transcript`):
- `whisper_transcriber/alignment_io.py` — add `load_spoken_no_ts_lm_ids` helper; extend `write_results` with `manual_anchors` + `tolerance` params.
- `bin/align-forced` — add `--spoken-no-ts` arg; in `cmd_aberrant`, filter the line set, build per-line manual anchors, thread anchors+tolerance into `write_results`.
- `tests/test_spoken_no_ts.py` — new pytest file for the helper + write guard.

wizard docs:
- `~/utono/litdb/.claude/skills/wizard-ambrose/SKILL.md` — add "Step 6.6".

---

## Task 1: `upsert_spoken_status` query (linux-lit)

**Files:**
- Modify: `src/db/queries.rs` (add function after `upsert_start_time` ~line 556; add test in `mod tests`)

- [ ] **Step 1: Write the failing test**

Add inside the `#[cfg(test)] mod tests { ... }` block in `src/db/queries.rs` (alongside the existing `line_start_time_reads_stored_value` test):

```rust
    #[test]
    fn upsert_spoken_status_inserts_then_updates() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_spoken_status (
                id INTEGER PRIMARY KEY,
                line_mapping_id INTEGER NOT NULL,
                media_id INTEGER NOT NULL,
                is_spoken INTEGER NOT NULL DEFAULT 1,
                confidence REAL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(line_mapping_id, media_id)
            );",
        )
        .unwrap();

        // Insert: row created with is_spoken=1, confidence=1.0
        upsert_spoken_status(&conn, 42, 7, true).unwrap();
        let (spoken, conf): (i64, f64) = conn
            .query_row(
                "SELECT is_spoken, confidence FROM line_spoken_status \
                 WHERE line_mapping_id = 42 AND media_id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(spoken, 1);
        assert_eq!(conf, 1.0);

        // Pre-existing not-spoken row gets flipped to spoken by upsert.
        conn.execute(
            "INSERT INTO line_spoken_status (line_mapping_id, media_id, is_spoken, confidence) \
             VALUES (99, 7, 0, 0.0)",
            [],
        )
        .unwrap();
        upsert_spoken_status(&conn, 99, 7, true).unwrap();
        let spoken2: i64 = conn
            .query_row(
                "SELECT is_spoken FROM line_spoken_status \
                 WHERE line_mapping_id = 99 AND media_id = 7",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(spoken2, 1);

        // No duplicate rows for the same (line, media).
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM line_spoken_status", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test upsert_spoken_status_inserts_then_updates 2>&1 | tail -20`
Expected: compile error — `cannot find function upsert_spoken_status in this scope`.

- [ ] **Step 3: Write minimal implementation**

Add this function immediately after `upsert_start_time` (it ends at ~line 556) in `src/db/queries.rs`:

```rust
pub fn upsert_spoken_status(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    is_spoken: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_spoken_status \
         (line_mapping_id, media_id, is_spoken, confidence) \
         VALUES (?1, ?2, ?3, 1.0) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET is_spoken = ?3, confidence = 1.0",
        rusqlite::params![line_mapping_id, media_id, is_spoken as i64],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test upsert_spoken_status_inserts_then_updates 2>&1 | tail -20`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "db: add upsert_spoken_status query

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `u` sets is_spoken=1 (linux-lit)

**Files:**
- Modify: `src/input/timestamps.rs` — `set_start_time`, the in-memory update block (`match &mut line.timestamp` ~line 137) and the surrounding DB block.

There is no cheap unit test for `set_start_time` (it needs full `AppState` + GTK), so this task is verified by `cargo build`/`clippy` plus a documented manual check. The DB query it calls is already covered by Task 1.

- [ ] **Step 1: Add the spoken-status upsert after the timestamp write**

In `src/input/timestamps.rs`, inside `set_start_time`, find the block that does the timestamp upsert:

```rust
        if let Err(e) = crate::db::queries::upsert_start_time(&conn, line.id, media_id, &line.citation, time_pos) {
            crate::logging::log(&format!("TS: upsert_start_time failed: {}", e));
            return false;
        }
```

Immediately after that `if let Err` block (before the `// Update in-memory` comment / `match &mut line.timestamp`), insert:

```rust
        if let Err(e) = crate::db::queries::upsert_spoken_status(&conn, line.id, media_id, true) {
            // Non-fatal: the timestamp is already written; just log.
            crate::logging::log(&format!("TS: upsert_spoken_status failed: {}", e));
        }
```

- [ ] **Step 2: Set `is_spoken` in the in-memory line update**

In the same function, the in-memory update is:

```rust
        // Update in-memory
        match &mut line.timestamp {
            Some(ts) => {
                ts.start = time_pos;
                if end_time > 0.0 {
                    ts.end = end_time;
                }
            }
            None => line.timestamp = Some(TimeRange {
                start: time_pos,
                end: end_time,
                sentence_start: None,
                is_manual: true,
            }),
        }
```

Immediately AFTER that `match` (still inside the `{ let line = &mut work.lines[line_idx]; ... }` block, before `if end_time > 0.0 { ... update_end_time ... }`), add:

```rust
        line.is_spoken = Some(true);
```

- [ ] **Step 3: Build and lint**

Run: `cargo build 2>&1 | tail -20`
Expected: `Finished` with no errors.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new warnings introduced by this change.

- [ ] **Step 4: Document the manual verification (do NOT run the app)**

Per project rule, the user runs `cargo run`, not the agent. Record the manual-check recipe in the commit body (Step 5). The check, for the user to run later:

1. Open a production work with audio in linux-lit, cursor on a known untimestamped spoken line.
2. Press `u`.
3. In another terminal:
   ```bash
   sqlite3 ~/utono/litdb/data/lit.db \
     "SELECT lss.is_spoken, lss.confidence FROM line_spoken_status lss \
      JOIN line_mapping lm ON lm.id = lss.line_mapping_id \
      WHERE lm.work_abbrev='<WORK_ABBREV>' AND lss.media_id=<MKV_MEDIA_ID> \
      ORDER BY lss.id DESC LIMIT 1;"
   ```
   Expected: `1|1.0`.
4. Confirm the dual-media twin got mirrored:
   ```bash
   sqlite3 ~/utono/litdb/data/lit.db \
     "SELECT media_id, is_spoken FROM line_spoken_status \
      WHERE line_mapping_id=<that line's id>;"
   ```
   Expected: two rows (mkv + m4b), both `is_spoken=1`.

- [ ] **Step 5: Commit**

```bash
git add src/input/timestamps.rs
git commit -m "input: u also marks the line is_spoken=1

set_start_time now upserts line_spoken_status (is_spoken=1, confidence=1.0)
beside the manual timestamp, so wizard-ambrose can later target
spoken-but-untimestamped lines. Dual-media twin is mirrored by existing
SQLite triggers.

Manual check: press u on a line, then
  sqlite3 lit.db 'SELECT is_spoken,confidence FROM line_spoken_status ...'
should return 1|1.0 for both media_ids.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `load_spoken_no_ts_lm_ids` helper (whisper-transcript)

Selects the line set for the targeted pass: lines that are `is_spoken=1` AND have no whisper timestamp (no row, or `source='manual'`).

**Files:**
- Modify: `whisper_transcriber/alignment_io.py` — add helper after `load_not_spoken_from_db` (~line 31).
- Create: `tests/test_spoken_no_ts.py`

Work from `~/utono/whisper-transcript`. Use the venv pytest: `.venv/bin/pytest`.

- [ ] **Step 1: Write the failing test**

Create `tests/test_spoken_no_ts.py`:

```python
"""Tests for the --spoken-no-ts targeted re-alignment helpers."""
import sqlite3
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from whisper_transcriber.alignment_io import load_spoken_no_ts_lm_ids


def _db_with_schema():
    db = sqlite3.connect(":memory:")
    db.executescript(
        """
        CREATE TABLE line_spoken_status (
            line_mapping_id INTEGER, media_id INTEGER,
            is_spoken INTEGER, confidence REAL,
            UNIQUE(line_mapping_id, media_id)
        );
        CREATE TABLE line_timestamps (
            line_mapping_id INTEGER, media_id INTEGER,
            start_time REAL, source TEXT,
            UNIQUE(line_mapping_id, media_id)
        );
        """
    )
    return db


def test_selects_spoken_lines_without_whisper_ts():
    db = _db_with_schema()
    mid = 7
    # line 1: spoken, no ts at all -> SELECTED
    db.execute("INSERT INTO line_spoken_status VALUES (1, ?, 1, 1.0)", (mid,))
    # line 2: spoken, manual ts -> SELECTED (manual is overwritable)
    db.execute("INSERT INTO line_spoken_status VALUES (2, ?, 1, 1.0)", (mid,))
    db.execute("INSERT INTO line_timestamps VALUES (2, ?, 100.0, 'manual')", (mid,))
    # line 3: spoken, already whisper-aligned -> EXCLUDED
    db.execute("INSERT INTO line_spoken_status VALUES (3, ?, 1, 0.9)", (mid,))
    db.execute(
        "INSERT INTO line_timestamps VALUES (3, ?, 200.0, 'whisper-align-aberrant')",
        (mid,),
    )
    # line 4: NOT spoken -> EXCLUDED
    db.execute("INSERT INTO line_spoken_status VALUES (4, ?, 0, 0.0)", (mid,))
    # line 5: spoken but for a DIFFERENT media_id -> EXCLUDED
    db.execute("INSERT INTO line_spoken_status VALUES (5, 99, 1, 1.0)")

    result = load_spoken_no_ts_lm_ids(db, mid)
    assert result == {1, 2}


def test_empty_when_no_spoken_status():
    db = _db_with_schema()
    assert load_spoken_no_ts_lm_ids(db, 7) == set()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_spoken_no_ts.py -q 2>&1 | tail -20`
Expected: `ImportError: cannot import name 'load_spoken_no_ts_lm_ids'`.

- [ ] **Step 3: Write minimal implementation**

In `whisper_transcriber/alignment_io.py`, add after `load_not_spoken_from_db` (ends ~line 31):

```python
def load_spoken_no_ts_lm_ids(db, media_id):
    """Line IDs to target for --spoken-no-ts: is_spoken=1 AND no whisper
    timestamp.

    A line qualifies when it is marked spoken for this media and either has
    no line_timestamps row, or has one whose source is 'manual' (i.e. set by
    the linux-lit `u` bind, which is overwritable by alignment). Lines already
    aligned by a whisper source are excluded so good timestamps are untouched.
    Returns a set of line_mapping_id (possibly empty).
    """
    rows = db.execute(
        """
        SELECT s.line_mapping_id
        FROM line_spoken_status s
        LEFT JOIN line_timestamps t
          ON t.line_mapping_id = s.line_mapping_id
         AND t.media_id = s.media_id
        WHERE s.media_id = ?
          AND s.is_spoken = 1
          AND (t.line_mapping_id IS NULL OR t.source = 'manual')
        """,
        (media_id,),
    ).fetchall()
    return {r[0] for r in rows}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `.venv/bin/pytest tests/test_spoken_no_ts.py -q 2>&1 | tail -20`
Expected: `2 passed`.

- [ ] **Step 5: Commit**

```bash
git add whisper_transcriber/alignment_io.py tests/test_spoken_no_ts.py
git commit -m "alignment_io: add load_spoken_no_ts_lm_ids for targeted re-align

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: ±1.0s manual-anchor overwrite guard in `write_results` (whisper-transcript)

`write_results` gains two optional params: `manual_anchors` (dict lm_id → manual start_time) and `tolerance`. When a result line is in `manual_anchors` and the existing row is `manual`, overwrite ONLY if the aligned start is within `tolerance` of the anchor; else keep the manual row.

**Files:**
- Modify: `whisper_transcriber/alignment_io.py` — `write_results` (~line 147).
- Modify: `tests/test_spoken_no_ts.py` — add write-guard tests.

- [ ] **Step 1: Write the failing test**

Append to `tests/test_spoken_no_ts.py`:

```python
from whisper_transcriber.alignment_io import write_results


def _ts_db():
    db = sqlite3.connect(":memory:")
    db.executescript(
        """
        CREATE TABLE line_timestamps (
            id INTEGER PRIMARY KEY,
            citation TEXT, line_mapping_id INTEGER, media_id INTEGER,
            start_time REAL, end_time REAL, source TEXT,
            updated_at TEXT,
            UNIQUE(line_mapping_id, media_id)
        );
        """
    )
    return db


def _row(db, lm_id, mid):
    return db.execute(
        "SELECT start_time, source FROM line_timestamps "
        "WHERE line_mapping_id=? AND media_id=?",
        (lm_id, mid),
    ).fetchone()


def test_overwrites_manual_within_tolerance():
    db = _ts_db()
    mid = 7
    db.execute(
        "INSERT INTO line_timestamps "
        "(citation,line_mapping_id,media_id,start_time,source) "
        "VALUES ('1.1.1',1,?,100.0,'manual')",
        (mid,),
    )
    # aligned at 100.5, anchor 100.0, tolerance 1.0 -> overwrite
    results = [(1, "1.1.1", 100.5, 101.0, 0.9, "matched")]
    written, _ = write_results(
        db, results, mid, "whisper-align-aberrant",
        keep_manual=False, dry_run=False,
        manual_anchors={1: 100.0}, tolerance=1.0,
    )
    assert written == 1
    start, source = _row(db, 1, mid)
    assert start == 100.5
    assert source == "whisper-align-aberrant"


def test_keeps_manual_outside_tolerance():
    db = _ts_db()
    mid = 7
    db.execute(
        "INSERT INTO line_timestamps "
        "(citation,line_mapping_id,media_id,start_time,source) "
        "VALUES ('1.1.1',1,?,100.0,'manual')",
        (mid,),
    )
    # aligned at 105.0, anchor 100.0, tolerance 1.0 -> keep manual
    results = [(1, "1.1.1", 105.0, 106.0, 0.9, "matched")]
    written, _ = write_results(
        db, results, mid, "whisper-align-aberrant",
        keep_manual=False, dry_run=False,
        manual_anchors={1: 100.0}, tolerance=1.0,
    )
    assert written == 0
    start, source = _row(db, 1, mid)
    assert start == 100.0
    assert source == "manual"


def test_no_anchor_dict_preserves_default_behavior():
    db = _ts_db()
    mid = 7
    # no existing row; plain insert still works with default params
    results = [(1, "1.1.1", 50.0, 51.0, 0.9, "matched")]
    written, _ = write_results(
        db, results, mid, "whisper-align-aberrant",
        keep_manual=False, dry_run=False,
    )
    assert written == 1
    start, source = _row(db, 1, mid)
    assert start == 50.0
    assert source == "whisper-align-aberrant"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_spoken_no_ts.py -q 2>&1 | tail -25`
Expected: failures — `write_results() got an unexpected keyword argument 'manual_anchors'`.

- [ ] **Step 3: Write minimal implementation**

In `whisper_transcriber/alignment_io.py`, change the `write_results` signature and add the guard. Current signature:

```python
def write_results(db, results, media_id, source_tag, keep_manual,
                  dry_run, interval=None):
```

Change to:

```python
def write_results(db, results, media_id, source_tag, keep_manual,
                  dry_run, interval=None, manual_anchors=None,
                  tolerance=1.0):
```

Then, inside the `for lm_id, cit, st, et, conf, status in results:` loop, in the `if existing:` branch, AFTER the `keep_manual` skip block and BEFORE the `if interval is not None and source == source_tag:` block, insert:

```python
            # --spoken-no-ts anchor guard: when a manual mark exists for this
            # line, only overwrite it if the aligned start is within tolerance
            # of the human's mark; otherwise keep the manual timestamp.
            if (manual_anchors is not None
                    and lm_id in manual_anchors
                    and source == 'manual'):
                if abs(st - manual_anchors[lm_id]) > tolerance:
                    skipped_manual += 1
                    continue
```

(The existing `written += 1` at the end of the loop body still counts the overwrites that pass the guard. The default `manual_anchors=None` leaves all current callers unchanged.)

- [ ] **Step 4: Run test to verify it passes**

Run: `.venv/bin/pytest tests/test_spoken_no_ts.py -q 2>&1 | tail -20`
Expected: `5 passed`.

- [ ] **Step 5: Run the full suite to confirm no regressions**

Run: `.venv/bin/pytest -q 2>&1 | tail -20`
Expected: all pre-existing tests (incl. `test_windowed_matching.py`) still pass.

- [ ] **Step 6: Commit**

```bash
git add whisper_transcriber/alignment_io.py tests/test_spoken_no_ts.py
git commit -m "alignment_io: write_results ±tolerance guard for manual anchors

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `--spoken-no-ts` flag wiring in `align-forced` (whisper-transcript)

Wire the flag, the line-set filter, the per-line manual anchors, and the threaded write params into `cmd_aberrant`. This is CLI glue over Tasks 3–4; verified by a `--dry-run` against a real flagged work plus an `argparse` smoke check (no pure unit test, since `cmd_aberrant` needs a transcript + audio).

**Files:**
- Modify: `bin/align-forced` — `argparse` (~line 491, near `--delete-all-ts`) and `cmd_aberrant` (~lines 299–409).

- [ ] **Step 1: Add the argparse flag**

In `bin/align-forced`, in `main()`'s parser, after the `--delete-all-ts` argument block (~line 494), add:

```python
    parser.add_argument(
        "--spoken-no-ts", action="store_true",
        help="Targeted pass: align ONLY lines flagged is_spoken=1 that have "
             "no whisper timestamp (no row or source=manual). Uses each "
             "line's manual mark as a search anchor and overwrites it only "
             "if the aligned start is within 1.0s. Non-destructive; existing "
             "whisper timestamps are preserved. Implies --strategy aberrant.")
```

Then, where `args.strategy` is dispatched at the end of `main()`:

```python
    if args.strategy == "aberrant":
        cmd_aberrant(args)
    else:
        cmd_align(args)
```

change to force aberrant when the flag is set:

```python
    if args.spoken_no_ts:
        args.strategy = "aberrant"
    if args.strategy == "aberrant":
        cmd_aberrant(args)
    else:
        cmd_align(args)
```

- [ ] **Step 2: Filter the line set in `cmd_aberrant`**

In `cmd_aberrant`, immediately after the existing:

```python
    sentences = load_sentences(db, args.work)
    if not sentences:
        print(f"No sentences found for work '{args.work}'", file=sys.stderr)
        sys.exit(1)
    print(f"Loaded {len(sentences)} sentences for {args.work}")
```

add (use `getattr` so older callers without the attr still work):

```python
    if getattr(args, "spoken_no_ts", False):
        from whisper_transcriber.alignment_io import load_spoken_no_ts_lm_ids
        target_ids = load_spoken_no_ts_lm_ids(db, args.media_id)
        if not target_ids:
            print("--spoken-no-ts: no spoken-but-untimestamped lines for "
                  f"media_id {args.media_id}; nothing to do.")
            db.close()
            return
        sentences = [s for s in sentences if s[0] in target_ids]
        print(f"--spoken-no-ts: targeting {len(sentences)} flagged line(s)")
```

- [ ] **Step 3: Build per-line manual anchors and thread them into matching + write**

In `cmd_aberrant`, the existing anchor block selects only `is_chapter`/`is_scene_start` rows:

```python
    anchor_rows = db.execute("""
        SELECT lt.line_mapping_id, lt.start_time,
               lm.div1, lm.div2, lm.line_in_div
        FROM line_timestamps lt
        JOIN line_mapping lm ON lt.line_mapping_id = lm.id
        WHERE lt.media_id = ? AND lm.work_abbrev = ?
          AND (lt.is_chapter = 1 OR lt.is_scene_start = 1)
        ORDER BY lm.div1, lm.div2, lm.line_in_div
    """, (args.media_id, args.work)).fetchall()

    anchors = []
    if anchor_rows:
        lm_id_to_sent_idx = {s[0]: i for i, s in enumerate(sentences)}
        for lm_id, start_time, d1, d2, lid in anchor_rows:
            sent_idx = lm_id_to_sent_idx.get(lm_id)
            if sent_idx is not None and start_time is not None:
                anchors.append((sent_idx, start_time))
```

Immediately AFTER that block, add the spoken-no-ts manual anchors. These use the manual `start_time` of every targeted line that has one, both to scope matching and (via `manual_anchors`) to drive the ±1.0s write guard:

```python
    manual_anchors = None
    if getattr(args, "spoken_no_ts", False):
        man_rows = db.execute("""
            SELECT line_mapping_id, start_time
            FROM line_timestamps
            WHERE media_id = ? AND source = 'manual' AND start_time IS NOT NULL
        """, (args.media_id,)).fetchall()
        manual_anchors = {lm_id: st for lm_id, st in man_rows}
        # Restrict to lines in the current (filtered) sentence set.
        sent_lm_ids = {s[0] for s in sentences}
        manual_anchors = {k: v for k, v in manual_anchors.items()
                          if k in sent_lm_ids}
        lm_id_to_sent_idx = {s[0]: i for i, s in enumerate(sentences)}
        for lm_id, st in manual_anchors.items():
            sent_idx = lm_id_to_sent_idx.get(lm_id)
            if sent_idx is not None:
                anchors.append((sent_idx, st))
        anchors.sort(key=lambda a: a[0])
        print(f"--spoken-no-ts: {len(manual_anchors)} manual anchor(s)")
```

- [ ] **Step 4: Thread `manual_anchors` + tolerance into the `write_results` call**

The existing call in `cmd_aberrant`:

```python
    written, skipped_manual = write_results(
        db, all_results, args.media_id, source_tag,
        args.keep_manual, args.dry_run)
```

Change to:

```python
    written, skipped_manual = write_results(
        db, all_results, args.media_id, source_tag,
        args.keep_manual, args.dry_run,
        manual_anchors=manual_anchors, tolerance=1.0)
```

(`manual_anchors` is `None` in the normal path — unchanged behavior — and the dict in the `--spoken-no-ts` path.)

- [ ] **Step 5: Smoke-check the CLI parses and dispatches**

Run: `.venv/bin/python bin/align-forced --spoken-no-ts --help 2>&1 | grep -A3 spoken-no-ts`
Expected: the `--spoken-no-ts` help text prints (argparse accepts the flag).

Run (no media → expect the early "nothing to do" or required-arg error, NOT a Python traceback about an unknown attr):
`.venv/bin/python bin/align-forced --work NoSuchWork --media-id 999999 --media-path /nonexistent --spoken-no-ts 2>&1 | tail -5`
Expected: a clean error/exit (e.g. "Audio file not found" or "nothing to do"), no `AttributeError`.

- [ ] **Step 6: Full test suite**

Run: `.venv/bin/pytest -q 2>&1 | tail -20`
Expected: all pass (this task adds no unit tests but must not break Tasks 3–4 or existing tests).

- [ ] **Step 7: Commit**

```bash
git add bin/align-forced
git commit -m "align-forced: add --spoken-no-ts targeted re-alignment pass

Aligns only is_spoken=1 lines lacking a whisper timestamp, anchors on
their manual marks via the windowed matcher, and overwrites a manual
timestamp only within 1.0s. Non-destructive; preserves existing whisper
timestamps.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Wizard documentation — Step 6.6 (litdb skill)

**Files:**
- Modify: `~/utono/litdb/.claude/skills/wizard-ambrose/SKILL.md` — insert a new section after "Step 6.5: Deduce Performed Scene Order" (ends just before "## Step 7: Open in linux-lit").

This is a docs-only task; verification is a read-back, not a test run.

- [ ] **Step 1: Insert the new step**

Add this section immediately before the `## Step 7: Open in linux-lit` heading:

```markdown
## Step 6.6: Fill Manually-Flagged Lines (`--spoken-no-ts`)

Forced alignment sometimes leaves a genuinely-spoken line with no
timestamp. While reading the work in linux-lit, press `u` on each such
line: that sets a manual timestamp AND marks the line `is_spoken=1` in
`line_spoken_status` (mirrored to the dual-media twin automatically).

Later, run a **targeted, non-destructive** pass that aligns ONLY those
flagged lines. It does NOT delete or touch existing whisper timestamps:

```bash
cd /home/mlj/utono/whisper-transcript && .venv/bin/python bin/align-forced \
    --work <WORK_ABBREV> --media-id <MEDIA_ID> \
    --media-path <full-media-path> \
    --spoken-no-ts
```

For Ambrose dual-media, use `<MKV_MEDIA_ID>` and the `.mkv` path (the
mirror triggers propagate to the `.m4b`).

Behavior:

- Line set = lines where `is_spoken=1` AND there is no whisper timestamp
  (no `line_timestamps` row, or one with `source='manual'`).
- Each flagged line's manual mark is used as a search anchor (windowed
  matching) AND as a validity check: the aligned timestamp overwrites the
  manual one **only if within ±1.0s**; otherwise your manual mark is kept.
- `--spoken-no-ts` implies `--strategy aberrant`.

You do NOT need to re-run Step 5 (populate-spoken-status) afterward — the
flagged lines are already `is_spoken=1`, and the Step-6 step-5 sweep only
marks lines with NO timestamp as not-spoken, so any line newly filled here
is safe.

Verify a flagged line got a timestamp within tolerance:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
  SELECT lm.div1||'.'||lm.div2||'.'||lm.line_in_div AS loc,
         lt.start_time, lt.source
  FROM line_mapping lm
  JOIN line_timestamps lt ON lt.line_mapping_id = lm.id
  JOIN line_spoken_status s
    ON s.line_mapping_id = lm.id AND s.media_id = lt.media_id
  WHERE lm.work_abbrev='<WORK_ABBREV>' AND lt.media_id=<MEDIA_ID>
    AND s.is_spoken=1
  ORDER BY lm.div1, lm.div2, lm.line_in_div;"
```
```

- [ ] **Step 2: Verify the insertion reads correctly**

Run: `rg -n "Step 6.6|spoken-no-ts" ~/utono/litdb/.claude/skills/wizard-ambrose/SKILL.md`
Expected: the new heading and at least one `--spoken-no-ts` reference appear, positioned before "## Step 7".

- [ ] **Step 3: Commit (in the litdb repo)**

```bash
cd ~/utono/litdb && git add .claude/skills/wizard-ambrose/SKILL.md
git commit -m "wizard-ambrose: document Step 6.6 --spoken-no-ts targeted re-align

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final verification (all tasks)

- [ ] **linux-lit:** `cd ~/utono/linux-lit && cargo test 2>&1 | tail -15` → all pass; `cargo build` → Finished; `cargo clippy 2>&1 | tail -15` → no new warnings.
- [ ] **whisper-transcript:** `cd ~/utono/whisper-transcript && .venv/bin/pytest -q 2>&1 | tail -15` → all pass.
- [ ] **End-to-end (manual, user-run):** in linux-lit press `u` on a known missed line of a production work; confirm `line_spoken_status` row = `1|1.0` for both media_ids; then run `align-forced --spoken-no-ts` and confirm the line receives a whisper timestamp (or keeps the manual one if no match within 1.0s).
