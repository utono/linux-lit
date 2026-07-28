# Journal Band Refile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refile the three journal entries whose `div1` disagrees with their own citation, so Ctrl+j on BH chapter 1 finds its Q&As instead of reporting none.

**Architecture:** One claim-keyed one-time migration in `src/db/migrations.rs`, following `retag_passage_scoped_journal_entries` (merged in `526d5eb0`) as its structural template. Candidate rows are read, their citations parsed in Rust via the existing `parse_citation`, and only genuine mismatches written back — all inside one transaction.

**Tech Stack:** Rust, rusqlite (SQLite), GTK4. Tests are `#[cfg(test)]` unit tests on in-memory connections.

**Spec:** `docs/superpowers/specs/2026-07-28-journal-band-refile-design.md`

## Global Constraints

- Branch off `master` (currently `84e79729`). Per CLAUDE.md this work gets a
  worktree under `~/utono/linux-lit-wt/<branch>`.
- **NEVER open, query for writing, or run any migration against the shared
  lit.db at `~/utono/litdb/data/lit.db`.** All tests use
  `Connection::open_in_memory()`. The migration is claim-keyed — one stray
  run consumes the key and silently skips the user's real repair.
- Do NOT run `cargo run`. The user launches the app themselves.
- **linux-lit is a BIN-ONLY crate.** `cargo test --lib` fails with "no
  library targets found". Use `cargo test --bins`. Baseline before this work
  is **1222 passed / 0 failed / 3 ignored**.
- A PRE-EXISTING deny-level clippy error at `src/db/queries.rs:2456` (March
  2026, unrelated) makes `cargo clippy --all-targets` fail. Plain
  `cargo clippy` is the project gate and is clean. Do not touch queries.rs.
- Parse citations with `crate::db::models::parse_citation` — never re-derive
  the format in SQL. An abbrev containing a dot breaks a `substr` approach.
- Only `div1` is rewritten. `div2` is 0 on every affected row and no row in
  lit.db disagrees on `div2` alone.

---

### Task 1: The refile migration

**Files:**
- Modify: `src/db/migrations.rs` (add after `retag_passage_scoped_journal_entries`)
- Modify: `src/db/migrations.rs` (`mod tests`)
- Modify: `src/app/mod.rs` (the `BOOKMARKS_INIT.call_once` block, after the
  `retag_passage_scoped_journal_entries` line)

**Interfaces:**
- Consumes: `crate::db::models::parse_citation(cite: &str) -> Option<(i64, i64, i64)>`
  returning `(div1, div2, line)`; `ensure_one_time_migrations_table`, already
  called earlier in the same init block.
- Produces: `pub fn refile_journal_bands_from_citations(conn: &Connection) -> Result<usize, rusqlite::Error>` —
  returns how many rows were refiled, or `Ok(0)` when the claim key was
  already taken.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/db/migrations.rs`:

```rust
    /// Insert a journal row with an explicit band and citation. Mirrors the
    /// shape a litdb re-import leaves behind: the citation is written from
    /// the reading cursor, the band columns are whatever the import set.
    fn insert_banded(
        conn: &Connection,
        work: &str,
        div1: i64,
        div2: i64,
        scope: &str,
        citation: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO journal_entries
                (work_abbrev, div1, div2, question, answer, claude_model,
                 scope, start_citation, end_citation, source_text)
             VALUES (?1, ?2, ?3, 'Q?', 'A.', 'm', ?4, ?5, ?5, 'src')",
            rusqlite::params![work, div1, div2, scope, citation],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn band_of(conn: &Connection, id: i64) -> (i64, i64) {
        conn.query_row(
            "SELECT div1, div2 FROM journal_entries WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// The reported bug: BH ids 7/8/9 are filed under band (0,0) — the
    /// PREFACE — while citing chapter 1. Reading chapter 1 and pressing
    /// Ctrl+j asks find_scene_band_pages(work, 1, 0), gets nothing, and
    /// toasts "No journal entry for this segment".
    #[test]
    fn mismatched_band_is_refiled_from_its_citation() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        let mis_filed = insert_banded(&conn, "BH", 0, 0, "scene", Some("BH.1.0.12"));

        let n = refile_journal_bands_from_citations(&conn).unwrap();
        assert_eq!(n, 1);
        assert_eq!(band_of(&conn, mis_filed), (1, 0), "refiled to the cited chapter");

        // The whole point: the band render now finds it.
        let ch1 = crate::db::journal::find_scene_band_pages(&conn, "BH", 1, 0).unwrap();
        assert_eq!(ch1.len(), 1, "chapter 1's band now returns the entry");
        let ch0 = crate::db::journal::find_scene_band_pages(&conn, "BH", 0, 0).unwrap();
        assert!(ch0.is_empty(), "the Preface band is correctly empty");
    }

    /// Rows whose band already agrees with their citation must not be
    /// touched, and must not inflate the returned count.
    #[test]
    fn matching_band_is_left_alone() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        let ok = insert_banded(&conn, "BH", 2, 0, "passage", Some("BH.2.0.48"));

        assert_eq!(refile_journal_bands_from_citations(&conn).unwrap(), 0);
        assert_eq!(band_of(&conn, ok), (2, 0));
    }

    /// Author- and work-scope rows use SENTINEL divs ((-2,-2) and (-1,-1))
    /// and must never be refiled, whatever their citation looks like.
    #[test]
    fn sentinel_scopes_are_never_refiled() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        let author = insert_banded(&conn, "Charles Dickens", -2, -2, "author", Some("BH.1.0.5"));
        let work = insert_banded(&conn, "BH", -1, -1, "work", Some("BH.1.0.5"));

        assert_eq!(refile_journal_bands_from_citations(&conn).unwrap(), 0);
        assert_eq!(band_of(&conn, author), (-2, -2));
        assert_eq!(band_of(&conn, work), (-1, -1));
    }

    /// A row with no citation carries no location to refile from.
    #[test]
    fn uncited_row_is_never_refiled() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        let bare = insert_banded(&conn, "BH", 0, 0, "scene", None);

        assert_eq!(refile_journal_bands_from_citations(&conn).unwrap(), 0);
        assert_eq!(band_of(&conn, bare), (0, 0));
    }

    /// Claim-keyed, like every one-time migration in this file.
    #[test]
    fn refile_runs_only_once() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();
        insert_banded(&conn, "BH", 0, 0, "scene", Some("BH.1.0.12"));

        assert_eq!(refile_journal_bands_from_citations(&conn).unwrap(), 1);
        assert_eq!(
            refile_journal_bands_from_citations(&conn).unwrap(),
            0,
            "the claim key must make a second run a no-op"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --bins refile_ 2>&1 | tail -20
cargo test --bins mismatched_band matching_band sentinel_scopes uncited_row 2>&1 | tail -20
```

Expected: FAIL TO COMPILE — `cannot find function
refile_journal_bands_from_citations in this scope`. That is the correct red
state for a not-yet-written function.

- [ ] **Step 3: Write the migration**

Add to `src/db/migrations.rs`, immediately after
`retag_passage_scoped_journal_entries`:

```rust
/// Marker key claimed by `refile_journal_bands_from_citations` so the refile
/// runs exactly once across the DB's lifetime. Bump the date suffix if a
/// future re-import renumbers bands again.
const REFILE_JOURNAL_BANDS_KEY: &str = "refile-journal-bands-2026-07-28";

/// One-time repair for journal entries whose band columns disagree with their
/// own citation. A litdb re-import renumbered an edition's chapters (a
/// front-matter offset), leaving entries banded under the OLD numbering while
/// their citations — written from the reading cursor — address the NEW one.
///
/// Live data: three rows, all BH, filed under band (0,0) — the PREFACE —
/// while citing chapter 1. Reading chapter 1 and pressing Ctrl+j resolves the
/// cursor band to (1,0), calls `find_scene_band_pages`, gets nothing, and
/// toasts "No journal entry for this segment" — while those chapter-1 Q&As
/// sit filed under the Preface.
///
/// Only `div1` is rewritten: `div2` is 0 on every affected row and no row
/// disagrees on `div2` alone. Restricted to `scope IN ('scene','passage')`
/// so author-scope (div -2) and work-scope (div -1) sentinels are never
/// touched.
///
/// Citations are parsed with `parse_citation`, never with SQL string
/// surgery — an abbrev containing a dot would break a `substr` approach, and
/// `parse_citation` is this codebase's single definition of the format.
///
/// Claims `REFILE_JOURNAL_BANDS_KEY` in `one_time_migrations` (caller must
/// `ensure_one_time_migrations_table` first) before writing anything; if the
/// marker was already claimed, returns `Ok(0)` without touching a row. The
/// claim and the writes share one transaction, so a crash cannot consume the
/// key without doing the work.
pub fn refile_journal_bands_from_citations(
    conn: &Connection,
) -> Result<usize, rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;

    let claimed = tx.execute(
        "INSERT OR IGNORE INTO one_time_migrations (key) VALUES (?1)",
        [REFILE_JOURNAL_BANDS_KEY],
    )?;
    if claimed == 0 {
        return Ok(0);
    }

    // Read candidates, decide in Rust, write back only genuine mismatches.
    let rows: Vec<(i64, i64, String)> = {
        let mut stmt = tx.prepare(
            "SELECT id, div1, start_citation FROM journal_entries \
             WHERE start_citation IS NOT NULL AND end_citation IS NOT NULL \
               AND scope IN ('scene', 'passage')",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })?;
        mapped.collect::<Result<_, _>>()?
    };

    let mut refiled = 0usize;
    for (id, filed_div1, citation) in rows {
        let Some((cited_div1, _, _)) = crate::db::models::parse_citation(&citation) else {
            continue;
        };
        if cited_div1 == filed_div1 {
            continue;
        }
        tx.execute(
            "UPDATE journal_entries SET div1 = ?1 WHERE id = ?2",
            rusqlite::params![cited_div1, id],
        )?;
        refiled += 1;
    }

    tx.commit()?;
    Ok(refiled)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --bins refile_ 2>&1 | tail -10
cargo test --bins mismatched_band matching_band sentinel_scopes uncited_row 2>&1 | tail -10
```

Expected: PASS — all five new tests.

- [ ] **Step 5: Wire it into startup**

In `src/app/mod.rs`, in the `BOOKMARKS_INIT.call_once` block, add immediately
after the `retag_passage_scoped_journal_entries` line:

```rust
            let _ = crate::db::migrations::refile_journal_bands_from_citations(&conn);
```

Order relative to the retag does not matter (they touch different columns and
`scope IN ('scene','passage')` covers the affected rows either way), but
placing it after keeps the two data repairs adjacent and readable. Both
`ensure_journal_table` and `ensure_one_time_migrations_table` run earlier in
the same block, so both tables exist. The `let _ =` matches every sibling
call in that block.

- [ ] **Step 6: Verify the full gate**

```bash
cargo build 2>&1 | tail -2
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
cargo test --bins 2>&1 | tail -3
```

Expected: `Finished`; 0 clippy errors; **1227 passed / 0 failed / 3 ignored**
(baseline 1222 + 5 new tests).

- [ ] **Step 7: Commit**

```bash
git add src/db/migrations.rs src/app/mod.rs
git commit -m "fix(journal): refile entries whose band disagrees with their citation

Reading BH chapter 1 and pressing Ctrl+j reported 'No journal entry for this
segment' while three chapter-1 Q&As sat filed under band (0,0) — the Preface.
The same litdb re-import that overwrote scope also renumbered these bands,
leaving them filed under the old chapter numbering while their citations
address the new one.

Refiles div1 from the entry's own start_citation for rows where the two
disagree: 3 rows in lit.db, all BH, all 0.0 -> 1.0. Claim-keyed and
transactional like the scope retag beside it.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 2: Headless on-screen verification

The non-waivable gate. A green build is not "done" for a change with visible
behavior.

**Files:** none. This task produces evidence.

**Interfaces:**
- Consumes: Task 1, built.
- Produces: a pass/fail observation plus a screenshot.

**Why a seeded DB copy is required.** `scripts/land-on.sh` copies the REAL
lit.db on every launch, and the migration runs at startup — so a plain run
would refile the copy and then test the post-migration state, proving
nothing. Seed a copy in the PRE-migration state and pre-claim the key, then
launch against it with `LIT_DB_PATH`.

- [ ] **Step 1: Build the pre-broken seed DB**

```bash
SCRATCH=/tmp/claude-1000/-home-mlj-utono-linux-lit/a9328b70-9801-4496-bb7e-d3bfe4cbf974/scratchpad
sqlite3 ~/utono/litdb/data/lit.db ".backup '$SCRATCH/band-seed.db'"
sqlite3 "$SCRATCH/band-seed.db" \
  "INSERT OR IGNORE INTO one_time_migrations (key) VALUES ('refile-journal-bands-2026-07-28');
   SELECT id, div1||'.'||div2, start_citation FROM journal_entries WHERE id IN (7,8,9);"
```

Reading the real lit.db to make a copy is fine; writing to it is not.
Expected output: ids 7/8/9 all filed `0.0`, citing `BH.1.0.*`.

Then reverse the already-shipped scope retag on the copy too, so the seed is
genuinely pre-migration for BOTH repairs — otherwise ids 8/9 arrive as
`passage` and the test is not reproducing the reported state:

```bash
sqlite3 "$SCRATCH/band-seed.db" \
  "UPDATE journal_entries SET scope='scene' WHERE id IN (8,9);
   INSERT OR IGNORE INTO one_time_migrations (key) VALUES ('retag-passage-scope-2026-07-27');"
```

- [ ] **Step 2: Confirm the bug reproduces on the seed (RED)**

Before testing the fix, prove the seed reproduces the reported failure. Query
the seed exactly as the reader's band lookup does:

```bash
sqlite3 "$SCRATCH/band-seed.db" \
  "SELECT COUNT(*) FROM journal_entries
    WHERE work_abbrev='BH' AND div1=1 AND div2=0 AND scope IN ('scene','passage');"
```

Expected: `0` — chapter 1's band is empty, which is exactly why Ctrl+j
toasts. If this prints non-zero, the seed is wrong; fix it before continuing.

- [ ] **Step 3: Launch headless on BH-Barrett chapter 1**

```bash
export XDG_RUNTIME_DIR=$(mktemp -d)
cd ~/utono/linux-lit-wt/<branch>
LIT_DEV=1 LIT_NO_MPV=1 LIT_HEADLESS_TEST=1 \
  LIT_DB_PATH="$SCRATCH/band-seed.db" LIT_LOG_PATH="$SCRATCH/band-test.log" \
  LIT_START_WORK=BH-Barrett LIT_START_SCENE=1.0 \
  GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit 2>"$SCRATCH/band-cage.err"
```

Launch with the harness `run_in_background` — it owns the lifecycle. A
`nohup`/`setsid`/`timeout` wrapper kills the instance the moment it returns.
Wait for `TEST_VIEWPORT_RECT` in the log before driving keys (poll with an
until-loop; do not chain sleeps).

The migration runs at startup and refiles the seed's rows — that is the
behavior under test.

- [ ] **Step 4: Press Ctrl+j and capture**

```bash
export WAYLAND_DISPLAY=wayland-0   # the socket in the fresh XDG_RUNTIME_DIR
wtype -M ctrl -k j -m ctrl
sleep 2
rg -n "KEY: name=j|JOURNAL-PAGINATE|No journal entry for this segment" "$SCRATCH/band-test.log" | tail -5
grim "$SCRATCH/band-ctrlj.png"
```

Confirm the `KEY:` line landed before trusting the screenshot. An empty
~2-byte PNG means not-mapped-yet — check `stat -c%s` and retry after a sleep.

PASS: `JOURNAL-PAGINATE` / `JOURNAL-TIMING` appear — the overlay opened.
FAIL: `No journal entry for this segment` toast.

**Driving gotchas that cost ~6 relaunches last time — do not rediscover
them:**
- `j` is `NextBookmark`, NOT line-down. Line-down is the `Down` key
  (`Action::CursorNextDialogue`).
- `line_in_div` is BOOK-GLOBAL, not chapter-relative. Verify the
  buffer↔`line_in_div` offset from the log before assuming a cursor landed
  where you intended.
- `LIT_START_SCENE` OVERRIDES `LIT_START_POS`; they cannot be combined.

- [ ] **Step 5: Open the PNG and report what is on screen**

Per the UI review protocol, open the capture and report inline what it shows
— quote the visible text. Expected: the journal overlay showing a chapter-1
Q&A (the Chancery-courts or "when was Bleak House written" entry), NOT the
reader with a toast.

- [ ] **Step 6: Clean up and record the real-DB expectation**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Use exactly that pattern — a bare `pkill -f target/debug/linux-lit` kills the
user's live instance.

Then capture the real DB's before-state for the user (READ ONLY):

```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT id, div1||'.'||div2 AS filed, start_citation FROM journal_entries WHERE id IN (7,8,9);"
```

Expected BEFORE: all three filed `0.0`. Expected AFTER the user's next real
launch: all three filed `1.0`. Do not run the migration by hand against the
real DB, and do not launch the app to force it.

- [ ] **Step 7: Report to the user**

State plainly what was observed, with the screenshot and the before/after
expectation. If the headless launch genuinely fails after a retry, say so and
hand off manual steps: open BH-Barrett chapter 1, press Ctrl+j, expect the
journal overlay rather than "No journal entry for this segment".

---

## Finishing

Per CLAUDE.md: merge back to master locally, then push — no PR, no asking.

1. Confirm `cargo build`, `cargo clippy`, `cargo test --bins` green and the
   tree clean.
2. `git checkout master && git merge --no-ff <branch>`
3. Re-verify the build on master.
4. `git push origin master`
5. `git worktree remove` the worktree, then `git branch -d <branch>`.

No pre-merge code-review gate is required: one subsystem, no keybinds moved.
The Task 2 on-screen check is NOT waivable — it is correctness, not review.

## Follow-ups (NOT this branch)

- **Upstream litdb:** whatever rewrites `scope` AND renumbers bands on
  re-import. Two separate corruptions now trace to it. Per CLAUDE.md's
  upstream-routing rule the fix belongs in litdb, with a ledger entry here
  linking to it.
- **Q&A picker scope cycling** — spec at
  `docs/superpowers/specs/2026-07-28-journal-picker-scope-cycling-design.md`,
  its own plan and branch.
