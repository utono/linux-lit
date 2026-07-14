# Rename `is_chapter` → `is_track_mark` (linux-lit — Plan B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update linux-lit's SQL so it reads/writes the renamed `line_timestamps` column `is_track_mark` (formerly `is_chapter`), matching the litdb migration — a column-mapping-only change in `src/db/queries.rs`, no Rust struct/field/action/snapshot changes.

**Architecture:** The reader holds a `Line.is_chapter` in-memory concept ("this line is a chapter boundary") that is SOURCED FROM the per-media audio flag. Only the SQL strings that name the DB column change from `is_chapter` to `is_track_mark`. Every Rust identifier — `Line.is_chapter`, `Timestamp.is_chapter`, `is_chapter_line`, `is_chapter_work`, `Action::SetChapter`, the gutter, chapter-jump nav, `SNAPSHOT_VERSION` — is UNCHANGED. The column maps INTO `Timestamp.is_chapter` exactly as before; only the column's name on the wire differs.

**Tech Stack:** Rust, rusqlite (raw SQL strings), `cargo test --bins`, `cargo clippy`.

## Global Constraints

- **Scope is column-mapping ONLY** (decided): rename `is_chapter` → `is_track_mark` ONLY inside SQL string literals in `src/db/queries.rs`. Do NOT rename any Rust identifier, struct field, enum variant, state map, function, keybind, overlay label, or bump `SNAPSHOT_VERSION`. This NARROWS the design spec `~/utono/litdb/docs/specs/2026-06-26-rename-is-chapter-to-track-mark-design.md` (which proposed a fuller rename) — the narrowing is the user's explicit decision; the spec's struct/action/snapshot items are superseded and out of scope here.
- The 7 SQL-string occurrences to change are ALL in `src/db/queries.rs`: lines 146, 1371, 1374, 1378, 1432, 1461, 1464 (verified). No other source file contains an `is_chapter` SQL string — every other `is_chapter` in `src/` is a Rust field/identifier and STAYS.
- **`Timestamp.is_chapter` and `Line.is_chapter` Rust fields STAY** — line 161 (`is_chapter: row.get(6)?…`) keeps its Rust field name; only the SELECT's column name (line 146) changes. The struct field reads column-index 6 positionally, so the field name is independent of the column name.
- Do NOT run the app (`cargo run`); only `cargo build` / `cargo test` / `cargo clippy`. (CLAUDE.md)
- `cargo test --bins` stays green; `cargo clippy` warning count must NOT increase (baseline **119**).
- **Cross-repo coupling (critical):** this build only works against a lit.db whose column IS `is_track_mark`. The litdb migration (Plan A, `rename_is_chapter_to_track_mark.py`) is committed but **NOT yet applied to the live DB** — the live column is still `is_chapter`. So after this change, an un-migrated live DB makes the reader's timestamp queries fail. The rollout (Task 2) applies the litdb migration and this build together, in one sitting.
- Commit messages end with the Co-Authored-By / Claude-Session trailer per CLAUDE.md.

---

## File Structure

- `src/db/queries.rs` — the only file changed. Rename the column in 7 SQL strings; add one test that builds a `line_timestamps` schema with `is_track_mark` and round-trips through the affected queries.

---

### Task 1: Rename the column in queries.rs SQL + a round-trip test

**Files:**
- Modify: `src/db/queries.rs` (SQL strings at lines 146, 1371, 1374, 1378, 1432, 1461, 1464)
- Test: `src/db/queries.rs` `#[cfg(test)]` module (add one test)

**Interfaces:**
- No public signature changes. `load_work`, the chapter-toggle upsert (`set_chapter`'s query, ~1365-1383), `get_timestamp_snapshot` (~1429-1453), and `restore_timestamp` (~1455-1467) keep their signatures; only their embedded SQL column name changes.

- [ ] **Step 1: Write the failing test.** Add to the `#[cfg(test)] mod tests` in `src/db/queries.rs` (near line 2297). This test builds the renamed schema and exercises the toggle upsert + read-back, which is the tightest round-trip over the renamed column:

```rust
#[test]
fn track_mark_column_roundtrips() {
    use rusqlite::Connection;
    let conn = Connection::open_in_memory().unwrap();
    // Schema mirrors the MIGRATED lit.db: the column is is_track_mark.
    conn.execute_batch(
        "CREATE TABLE line_timestamps (
            id INTEGER PRIMARY KEY, citation TEXT, line_mapping_id INTEGER,
            media_id INTEGER, start_time REAL, end_time REAL, source TEXT,
            is_track_mark INTEGER DEFAULT 0, is_scene_start INTEGER DEFAULT 0,
            sentence_start_time REAL, sentence_end_time REAL,
            created_at TEXT, updated_at TEXT,
            UNIQUE(line_mapping_id, media_id)
        );",
    ).unwrap();

    // First toggle: inserts with is_track_mark=1 -> returns true.
    let on = set_chapter_timestamp(&conn, 7, 100, "W.1.0.1", 1.5).unwrap();
    assert!(on);
    let v: i64 = conn.query_row(
        "SELECT is_track_mark FROM line_timestamps WHERE line_mapping_id=7 AND media_id=100",
        [], |r| r.get(0)).unwrap();
    assert_eq!(v, 1);

    // Second toggle: flips back to 0 -> returns false.
    let off = set_chapter_timestamp(&conn, 7, 100, "W.1.0.1", 1.5).unwrap();
    assert!(!off);
}
```

NOTE: the toggle upsert lives in a function around line 1365. Find its real name
(it is the `fn` whose body holds the `is_chapter = CASE WHEN is_chapter = 1 …`
SQL at 1371-1378). If it is NOT named `set_chapter_timestamp`, change the test's
two call sites to the real name — do not rename the function. Run
`rg -n "fn .*\bis_chapter\b|CASE WHEN is_chapter" src/db/queries.rs` and read the
enclosing `fn` signature to get the exact name and arg order; adjust the test's
arguments to match (the args are `(conn, line_mapping_id, media_id, citation, start_time)` per the SQL `params!` at line 1375).

- [ ] **Step 2: Run the test to verify it FAILS.**

Run: `cargo test --bins track_mark_column_roundtrips`
Expected: FAIL — the production SQL still says `is_chapter`, but the test table has
only `is_track_mark`, so the query errors with `no such column: is_chapter`.
(This proves the test exercises the real column reference.)

- [ ] **Step 3: Rename the column in the 7 SQL strings.** In `src/db/queries.rs`, change `is_chapter` → `is_track_mark` ONLY in these SQL string literals (leave every Rust identifier alone):

  - **Line ~146** (load_work timestamp SELECT): `lt.sentence_start_time, lt.source, lt.is_chapter \` → `lt.sentence_start_time, lt.source, lt.is_track_mark \`
  - **Line ~1371** (toggle upsert INSERT column list): `... source, is_chapter) \` → `... source, is_track_mark) \`
  - **Line ~1374** (toggle upsert DO UPDATE): `DO UPDATE SET is_chapter = CASE WHEN is_chapter = 1 THEN 0 ELSE 1 END, ...` → `DO UPDATE SET is_track_mark = CASE WHEN is_track_mark = 1 THEN 0 ELSE 1 END, ...`
  - **Line ~1378** (toggle read-back SELECT): `"SELECT is_chapter FROM line_timestamps WHERE ..."` → `"SELECT is_track_mark FROM line_timestamps WHERE ..."`
  - **Line ~1432** (get_timestamp_snapshot SELECT): `"SELECT citation, start_time, end_time, is_chapter \` → `"SELECT citation, start_time, end_time, is_track_mark \`
  - **Line ~1461** (restore_timestamp INSERT column list): `... source, is_chapter) \` → `... source, is_track_mark) \`
  - **Line ~1464** (restore_timestamp DO UPDATE): `DO UPDATE SET start_time = ?4, end_time = ?5, is_chapter = ?6, ...` → `DO UPDATE SET start_time = ?4, end_time = ?5, is_track_mark = ?6, ...`

  Do NOT touch:
  - Line ~137 `is_chapter: false,` (initializing a `Line` — Rust field).
  - Line ~161 `is_chapter: row.get::<_, Option<i64>>(6)?...` (sets `Timestamp.is_chapter` Rust field from column index 6 — field name stays; it reads position 6, which the renamed SELECT still supplies).
  - Line ~197 `if ts.media_id == mid && ts.is_chapter` (Rust field read).
  - Line ~223 `line.is_chapter = chapter_map.contains_key(&line.id);` (Rust field).
  - Line ~1441 `is_chapter: row.get::<_, bool>(3)...` (sets `TimestampSnapshot.is_chapter` from column index 3 — field stays).

  Verify the production SQL is fully renamed and no SQL `is_chapter` remains:
  ```bash
  rg -n "is_chapter" src/db/queries.rs | rg -i "SELECT|INSERT|UPDATE|SET |lt\.|VALUES|CONFLICT|FROM line_timestamps"
  ```
  Expected: ZERO matches (all SQL occurrences now say is_track_mark). The remaining
  `is_chapter` hits in the file are Rust field names — that is correct.

- [ ] **Step 4: Run the test to verify it PASSES.**

Run: `cargo test --bins track_mark_column_roundtrips`
Expected: PASS — the SQL now matches the `is_track_mark` test schema and the toggle round-trips.

- [ ] **Step 5: Build + full test suite + clippy gate.**

Run: `cargo build`
Expected: clean build.

Run: `cargo test --bins`
Expected: all pass (no regression; existing query tests that build a `line_timestamps` schema must use `is_track_mark` if they reference the column — if any existing test creates the table with `is_chapter` AND calls one of the four changed queries, update that test's CREATE TABLE to `is_track_mark`. Find them: `rg -n "is_chapter" src/db/queries.rs` inside `#[cfg(test)]` — update only column-name SQL in test schemas, not Rust field names).

Run: `cargo clippy 2>&1 | rg -c '^warning'`
Expected: ≤ 119 (baseline must not increase).

- [ ] **Step 6: Commit.**

```bash
git add src/db/queries.rs
git commit -m "refactor(db): read/write line_timestamps.is_track_mark (renamed column)

Match the litdb column rename is_chapter -> is_track_mark in the 7 queries.rs
SQL strings (load SELECT + the chapter-toggle/restore upserts + snapshot SELECT).
Rust field names (Line.is_chapter, Timestamp.is_chapter) and all reader chapter
machinery are unchanged — only the on-the-wire column name differs.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M4eSF8LVwLbcs49hzJD35M"
```

---

### Task 2: Coordinated rollout — apply litdb migration + this build together

**Files:** none (a live data + deploy operation).

**Interfaces:** none.

> **GATE / ORDER MATTERS.** Until now the live lit.db column is `is_chapter` and the
> SHIPPED reader still reads `is_chapter`. After Task 1, the NEW reader build reads
> `is_track_mark`. These must flip together: run the litdb migration, then run the
> new linux-lit build. Do NOT leave an old reader binary running against a migrated
> DB, or a new binary against an un-migrated DB — either errors on the timestamp
> queries.

- [ ] **Step 1: Back up the live DB.**

```bash
cd ~/utono/litdb
TS=$(TZ='America/Chicago' date +"%Y%m%dT%H%M%S")
\cp -f data/lit.db "data/lit.db.bak-trackmarkrollout-$TS"
echo "backup: data/lit.db.bak-trackmarkrollout-$TS"
```

- [ ] **Step 2: Apply the litdb migration to the live DB.**

```bash
~/utono/litdb/.venv/bin/python ~/utono/litdb/scripts/migrations/rename_is_chapter_to_track_mark.py
```
Expected: `Renamed line_timestamps.is_chapter -> is_track_mark; triggers regenerated.`

Verify:
```bash
sqlite3 ~/utono/litdb/data/lit.db "PRAGMA table_info(line_timestamps);" | rg "is_track_mark|is_chapter"
```
Expected: `is_track_mark` present, `is_chapter` absent.

- [ ] **Step 3: Build the new reader and run the headless check** (linux-lit CLAUDE.md Headless Verification). Open a work that HAS track marks (e.g. a prose work or a play with audio-chapter marks), confirm the gutter chapter signs still render and the `(`/`)` chapter-jump nav still moves between them — proving `load_work`'s renamed SELECT populates `Line.is_chapter` correctly from the renamed column. Press `c` on a line to toggle a mark (exercises the renamed toggle upsert + read-back) and confirm the sign appears/clears.

- [ ] **Step 4: Confirm the new structural keybind is unaffected.** Press `Ctrl+c` (the `ToggleChapterStart` action) on a prose paragraph — it flips `line_mapping.chapter_start` (NOT line_timestamps), so it is independent of this rename and must still work.

No commit (data + deploy operation). Report completion.

---

## Self-review notes

- This plan does NOT bump `SNAPSHOT_VERSION` because no serialized struct field is
  renamed — the snapshot's `is_chapter` field (snapshot.rs / TimestampSnapshot)
  keeps its name, so the serialized layout is byte-identical. Stale snapshots stay
  valid. (Confirmed: the column rename does not touch any `#[derive(Serialize…)]`
  field.)
- The reader's chapter-jump nav, gutter sign, `is_chapter_work`, and the
  scene-synopsis chapter count continue to read `Line.is_chapter` — still sourced
  from the (now renamed) audio column via `load_work`. Repointing that source to
  `(div1,div2)`/`chapter_start` so chapter-jump works without media is a SEPARATE
  later stage (transition guide Stage 3), NOT this plan.
- `Action::SetChapter`, its `c` keybind, and the keybinds-overlay "set chapter"
  label are unchanged (column-mapping-only scope). The accepted Minors from litdb
  Plan A's review (the `media_manager` stdout label and `timestamps-signs` gutter
  sign name still saying "chapter") are display labels for this same audio concept
  and likewise stay until a dedicated vocabulary pass — not this plan.
