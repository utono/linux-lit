# Scene → Division Rename Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire "scene" as the structural word for a work's division, in both repos and in lit.db, without breaking a running reader and without touching the one place the word is correct.

**Architecture:** Five sequenced tasks. Code is renamed in both repos FIRST, while the table still has its old name; the table rename lands last, behind a compatibility view so a reader started before the migration keeps working. The play-genre noun in `gloss.rs` is untouched throughout.

**Tech Stack:** Rust (linux-lit), Python 3 (litdb), SQLite.

**Spec:** `docs/superpowers/specs/2026-07-28-scene-to-division-rename-design.md`

## Global Constraints

- **REVIEW GATES ARE WAIVED** for this plan (user, 2026-07-28). That waives
  the review SUBAGENTS only. `cargo build`, `cargo clippy`, `cargo test
  --bins`, `pytest`, and the on-screen check are correctness, not review, and
  remain MANDATORY.
- **Never write to `~/utono/litdb/data/lit.db`.** Every migration test runs
  against a COPY. Read-only `sqlite3` inspection is fine. A reader instance
  may be running against the live file at any moment.
- Do NOT run `cargo run`.
- **linux-lit is BIN-ONLY**: `cargo test --lib` FAILS. Use `cargo test
  --bins`; `cargo test --bins A B` is invalid (ONE filter). Baseline is
  **1239 passed / 0 failed / 3 ignored**.
- A PRE-EXISTING deny-level clippy error at `src/db/queries.rs:2456`
  (unrelated) makes `cargo clippy --all-targets` fail. Plain `cargo clippy`
  is the gate.
- **NO BEHAVIOR CHANGE.** This is a vocabulary change. If the reader looks or
  behaves differently afterward, something is wrong.
- **DO NOT TOUCH `gloss.rs`'s genre table.** `"play" => ("play", "scene",
  "scenes")` is correct — a play's division IS a scene — and feeds
  `genre_unit`, which both the picker header and the row division column now
  read. Renaming it would make the UI wrong.
- Each repo works on its own branch in its own worktree
  (`~/utono/linux-lit-wt/…`, `~/utono/litdb-wt/…`).

## Ordering — read before starting

Both repos read `scene_synopses`, and **a reader instance may be running**
(verified: one was live while this plan was written). So the sequence is:

1. Rename linux-lit's Rust identifiers, table name UNCHANGED. (Task 1)
2. Rename litdb's Python identifiers, table name UNCHANGED. (Task 2)
3. Teach BOTH repos to read either table name. (Task 3)
4. Migrate the table + the stored scope value, leaving a compatibility view.
   (Task 4)
5. On-screen verification. (Task 5)

Tasks 1 and 2 are pure identifier renames that cannot break the DB. Only
Task 4 touches lit.db, and by then both repos tolerate either name.

---

### Task 1: Rename linux-lit's Rust identifiers

**Files:** ~54 files under `src/`. The rename is mechanical; the judgment is
in what NOT to rename.

**Interfaces:**
- Produces: `JournalBand::Division`, `src/app/division_synopsis.rs`, and the
  other renamed identifiers. Tasks 3-5 build on these.

- [ ] **Step 1: Inventory before touching anything**

```bash
cd ~/utono/linux-lit-wt/<branch>
rg -o "\bscene_\w+|\bScene\w*" --iglob '*.rs' src/ -N | sort -u > /tmp/scene-idents.txt
wc -l /tmp/scene-idents.txt
```

Expect ~93 distinct identifiers. Keep this file — Step 5 diffs against it.

- [ ] **Step 2: Rename, EXCLUDING the three keeps**

Rename `scene_* → division_*` and `Scene → Division` across `src/`, with
these EXCEPTIONS which must remain untouched:

1. **`src/gloss.rs`'s genre table and everything it feeds.** The literal
   `"scene"`/`"scenes"` strings in `genre_unit` are play vocabulary and are
   correct. Grep `rg -n '"scene"' src/gloss.rs` and leave every hit.
2. **Comments that discuss PLAYS specifically.** "the scene ends", "Act 1
   Scene 2" in a doc comment about Shakespeare stays. Comments using "scene"
   generically for a division get updated.
3. **The `"scene"` STRING in `journal_entries.scope` values** — e.g.
   `save_journal_page(..., "scene", ...)` and any `scope = 'scene'` SQL. That
   is DATA, migrated in Task 4. Renaming it here without the migration would
   break every existing row.

**THE SCOPE-VALUE COUPLING — the sharpest hazard in this plan.** The stored
value and the code that filters it MUST change in the same step as the data,
or entries silently vanish. Verified live, there are TWO such predicates:

- `src/db/journal.rs:261` — `find_scene_band_pages`, the band render
- `src/db/journal.rs:728` — `find_journal_page_for_line`, the `\` cycle probe

Both read `scope IN ('scene', 'passage')`. If Task 4 migrates the data to
`'division'` while these still say `'scene'`, the `\` overlay cycle and the
journal band both stop finding entries with NO error — precisely the bug
class fixed twice on 2026-07-27 (`670ec5d4`, `2293a3d1`).

**Resolution:** in Task 4, change these predicates to
`scope IN ('division', 'passage')` IN THE SAME COMMIT as the data migration,
and make the writers (`save_journal_page` call sites passing `"scene"`) write
`"division"` too. Do NOT split them across tasks. Task 5 verifies the `\`
cycle explicitly for this reason.

**NAME COLLISION — resolve deliberately.** `src/app/scene_synopsis.rs` has
`scene_label(div1, div2)` and `scene_label_for(state, div1, div2)`.
`src/input/actions/pickers.rs` ALREADY has `division_label(work_type, div1,
div2)` (added 2026-07-28) with different parameters and a different job.
A blind rename produces two `division_label`s.

Resolution: rename the synopsis ones to `synopsis_division_label` /
`synopsis_division_label_for`, since they build a SYNOPSIS OVERLAY heading,
not a picker column. Do NOT rename `pickers.rs::division_label`.

Also rename the file `src/app/scene_synopsis.rs` →
`src/app/division_synopsis.rs` and update its `mod` declaration.

- [ ] **Step 3: Leave the table name alone**

The SQL strings in `src/db/queries.rs` still say `scene_synopses` after this
task (6 real call sites at ~467, 495, 517, 546, 1058, 1063, plus test
fixtures). Task 3 makes them name-agnostic; Task 4 migrates. Renaming them
here would break against the un-migrated live DB.

- [ ] **Step 4: Verify**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

Expected: clean build, **1239 passed**, 0 clippy errors. The test count must
not change — this task adds and removes no tests.

- [ ] **Step 5: Confirm only the intended survivors remain**

```bash
rg -o "\bscene_\w+|\bScene\w*" --iglob '*.rs' src/ -N | sort -u
rg -n '"scene"' --iglob '*.rs' src/
```

Every remaining hit must be one of the three documented exceptions. If
anything else survives, it was missed — fix it before committing.

- [ ] **Step 6: Commit**

```bash
git add -A src/
git commit -F - <<'MSG'
refactor(naming): scene -> division for structural identifiers

"scene" is play vocabulary that named a generic division: JournalBand::Scene
is a chapter in a novel and a book in an epic. Renames the ~93 structural
identifiers and src/app/scene_synopsis.rs -> division_synopsis.rs.

Deliberately UNCHANGED: gloss.rs's genre table (a play's division really is a
scene, and it feeds genre_unit, which the picker header and row division
column both read), comments about plays specifically, and the "scene" value
stored in journal_entries.scope (data, migrated separately).

scene_label/-_for become synopsis_division_label/-_for rather than
division_label, which already exists in pickers.rs with a different job.

No behavior change. The scene_synopses table name is untouched here.
MSG
```

---

### Task 2: Rename litdb's Python identifiers

Same shape, other repo. Independent of Task 1 — they share no code.

**Files:** `scripts/**/*.py` (684 "scene" occurrences; 12 files reference
`scene_synopses`).

- [ ] **Step 1: Inventory**

```bash
cd ~/utono/litdb-wt/<branch>
rg -o "\bscene_\w+|\bScene\w*" --iglob '*.py' scripts/ -N | sort -u
```

- [ ] **Step 2: Rename with the same exceptions**

`scene_* → division_*`. KEEP: any play-specific prose, and the `'scene'`
string where it is a `journal_entries.scope` VALUE (Task 4 migrates that).

The table name `scene_synopses` stays in SQL strings for now — Task 3.

Note `scripts/chapter_synopses.py` and `whole_work_synopses.py` already use
chapter/work vocabulary; check whether their internal `scene_*` locals should
become `division_*` or something more specific to what they actually hold.

- [ ] **Step 3: Verify**

```bash
python -m pytest scripts/tests/ -q 2>&1 | tail -3
```

Expected: green. Record the count as the baseline for Task 3.

- [ ] **Step 4: Commit** (mirror Task 1's message, adjusted for Python)

---

### Task 3: Teach both repos to read either table name

The compatibility layer. After this, both repos work whether the table is
called `scene_synopses` or `division_synopses` — which is what makes Task 4
safe while a reader is running.

**Files:**
- `src/db/queries.rs` (linux-lit): the 6 real call sites
- `scripts/**` (litdb): the 12 files touching the table

- [ ] **Step 1: Add a resolver in linux-lit**

Add to `src/db/queries.rs`:

```rust
/// The synopsis table's name, tolerating either side of the 2026-07-28
/// rename. A reader started BEFORE the migration keeps working after it, and
/// a reader started after works against a not-yet-migrated DB — which is what
/// lets the rename land without a flag day while an instance is live.
///
/// Delete this and inline `"division_synopses"` once no un-migrated lit.db
/// remains in use.
pub fn synopsis_table(conn: &Connection) -> &'static str {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = 'division_synopses'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if exists { "division_synopses" } else { "scene_synopses" }
}
```

Note it accepts a VIEW as well as a table — Task 4 leaves a view behind, and
the resolver must not care which it is.

Rewrite the 6 call sites to build their SQL with `synopsis_table(conn)`
instead of the literal. The test fixtures at ~3956/3968/4006 create their own
table and can keep the literal, but each fixture must then be reachable by
the resolver — verify the tests still pass rather than assuming.

- [ ] **Step 2: The same in litdb**

Add an equivalent helper (e.g. in `scripts/common/db_utils.py`) and route the
12 files through it.

- [ ] **Step 3: Prove it works BOTH ways**

This is the step that makes Task 4 safe, so do it properly:

```bash
SCRATCH=/tmp/claude-1000/-home-mlj-utono-linux-lit/a9328b70-9801-4496-bb7e-d3bfe4cbf974/scratchpad
sqlite3 ~/utono/litdb/data/lit.db ".backup '$SCRATCH/compat-old.db'"
cp "$SCRATCH/compat-old.db" "$SCRATCH/compat-new.db"
sqlite3 "$SCRATCH/compat-new.db" "ALTER TABLE scene_synopses RENAME TO division_synopses;"
```

Run the linux-lit suite against each, and a litdb synopsis script against
each. Both must behave identically. Report the actual output.

- [ ] **Step 4: Verify + commit**

`cargo test --bins` still 1239; `pytest` still green.

---

### Task 4: Migrate the table and the stored scope value

The only task that writes lit.db.

**Files:** `src/db/migrations.rs` (+ its `mod tests`), `src/app/mod.rs`.

- [ ] **Step 1: Write the failing tests**

In `src/db/migrations.rs`'s `mod tests`, against in-memory connections:

```rust
    #[test]
    fn rename_scene_to_division_moves_table_and_scope() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scene_synopses (id INTEGER PRIMARY KEY, work_abbrev TEXT,
                 div1 INT, div2 INT, synopsis TEXT, claude_model TEXT);
             INSERT INTO scene_synopses (work_abbrev, div1, div2, synopsis)
                 VALUES ('BH', 1, 0, 's1'), ('Ham', 1, 2, 's2');",
        )
        .unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();
        crate::db::journal::save_journal_page(
            &conn, "BH", 1, 0, "Q?", "A.", "m", "scene", "qa",
        )
        .unwrap();

        let n = rename_scene_to_division(&conn).unwrap();
        assert!(n > 0);

        // Rows survive the table rename.
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM division_synopses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2, "every synopsis row survives");

        // The old name still READS, via the compatibility view.
        let via_view: i64 = conn
            .query_row("SELECT COUNT(*) FROM scene_synopses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(via_view, 2, "a pre-migration reader must keep working");

        // The stored scope value moved.
        let scoped: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journal_entries WHERE scope = 'division'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(scoped, 1);
        let stale: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journal_entries WHERE scope = 'scene'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0);
    }

    #[test]
    fn rename_scene_to_division_runs_only_once() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scene_synopses (id INTEGER PRIMARY KEY, work_abbrev TEXT,
                 div1 INT, div2 INT, synopsis TEXT, claude_model TEXT);",
        )
        .unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        assert!(rename_scene_to_division(&conn).is_ok());
        assert_eq!(rename_scene_to_division(&conn).unwrap(), 0,
                   "the claim key must make a second run a no-op");
    }

    /// A DB that is ALREADY on the new name must not be disturbed.
    #[test]
    fn rename_scene_to_division_skips_an_already_migrated_db() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE division_synopses (id INTEGER PRIMARY KEY, work_abbrev TEXT,
                 div1 INT, div2 INT, synopsis TEXT, claude_model TEXT);
             INSERT INTO division_synopses (work_abbrev, div1, div2, synopsis)
                 VALUES ('BH', 1, 0, 's1');",
        )
        .unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        assert_eq!(rename_scene_to_division(&conn).unwrap(), 0);
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM division_synopses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1, "an already-migrated DB is left alone");
    }
```

- [ ] **Step 2: Run them, confirm they fail to compile**

`cargo test --bins rename_scene_to_division` → "cannot find function".

- [ ] **Step 3: Write the migration**

In `src/db/migrations.rs`, following
`retag_passage_scoped_journal_entries`'s claim-keyed, transactional shape:

```rust
const RENAME_SCENE_TO_DIVISION_KEY: &str = "rename-scene-to-division-2026-07-28";

/// One-time rename of the structural "scene" vocabulary in the DB:
/// `scene_synopses` -> `division_synopses`, and `journal_entries.scope`
/// `'scene'` -> `'division'`.
///
/// Leaves a COMPATIBILITY VIEW named `scene_synopses` so a reader instance
/// started before the migration keeps rendering synopses. Reads work through
/// it unchanged; INSTEAD OF triggers forward writes. Verified against a copy
/// of lit.db: a bare view is read-only ("cannot modify scene_synopses because
/// it is a view"), so the triggers are required, not decorative.
///
/// The four historical `scene_synopses_*_backup` tables keep their names —
/// they are dated snapshots, not live schema.
///
/// Claims `RENAME_SCENE_TO_DIVISION_KEY` before writing; returns `Ok(0)` if
/// already claimed, or if the DB is already on the new name.
pub fn rename_scene_to_division(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let already: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='division_synopses'",
            [], |_| Ok(true),
        )
        .unwrap_or(false);
    if already {
        return Ok(0);
    }

    let tx = conn.unchecked_transaction()?;
    let claimed = tx.execute(
        "INSERT OR IGNORE INTO one_time_migrations (key) VALUES (?1)",
        [RENAME_SCENE_TO_DIVISION_KEY],
    )?;
    if claimed == 0 {
        return Ok(0);
    }

    tx.execute_batch(
        "ALTER TABLE scene_synopses RENAME TO division_synopses;
         CREATE VIEW scene_synopses AS SELECT * FROM division_synopses;
         CREATE TRIGGER scene_synopses_compat_ins INSTEAD OF INSERT ON scene_synopses BEGIN
             INSERT INTO division_synopses (work_abbrev, div1, div2, synopsis, claude_model)
             VALUES (NEW.work_abbrev, NEW.div1, NEW.div2, NEW.synopsis, NEW.claude_model);
         END;
         CREATE TRIGGER scene_synopses_compat_upd INSTEAD OF UPDATE ON scene_synopses BEGIN
             UPDATE division_synopses SET synopsis = NEW.synopsis,
                    claude_model = NEW.claude_model WHERE id = OLD.id;
         END;
         CREATE TRIGGER scene_synopses_compat_del INSTEAD OF DELETE ON scene_synopses BEGIN
             DELETE FROM division_synopses WHERE id = OLD.id;
         END;",
    )?;
    let n = tx.execute(
        "UPDATE journal_entries SET scope = 'division' WHERE scope = 'scene'",
        [],
    )?;
    tx.commit()?;
    Ok(n.max(1))
}
```

Verify the view's column list matches what the triggers insert — if
`scene_synopses` has columns this plan did not enumerate, the `SELECT *` view
is fine but the INSERT trigger must name them all. **Check the real schema
with `sqlite3 … ".schema scene_synopses"` first.**

- [ ] **Step 3b: Update the scope predicates IN THIS SAME COMMIT**

Non-negotiable — see the coupling note in Task 1. In `src/db/journal.rs`:

- line ~261 (`find_scene_band_pages`) and line ~728
  (`find_journal_page_for_line`): `scope IN ('scene', 'passage')` →
  `scope IN ('division', 'passage')`.
- every `save_journal_page(..., "scene", ...)` writer → `"division"`.
- the test fixtures that seed `"scene"` → `"division"`, EXCEPT any test
  specifically asserting the migration's before-state.

Grep to confirm none are missed:

```bash
rg -n "'scene'|\"scene\"" src/db/journal.rs src/input/actions/journal.rs
```

Every surviving hit must be a migration before-state or a comment. If a live
predicate still says `'scene'` after this step, the `\` cycle is broken and
Task 5 WILL catch it — but fix it here.

- [ ] **Step 4: Wire it in**

`src/app/mod.rs`, in the `BOOKMARKS_INIT.call_once` block, AFTER the existing
migrations:

```rust
            let _ = crate::db::migrations::rename_scene_to_division(&conn);
```

- [ ] **Step 5: Verify, including on a copy of the real DB**

```bash
cargo test --bins 2>&1 | tail -3     # expect 1242 (1239 + 3 new)
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

Then a real-data dry run on a COPY:

```bash
SCRATCH=/tmp/.../scratchpad
sqlite3 ~/utono/litdb/data/lit.db ".backup '$SCRATCH/rename-dry.db'"
# apply the migration to the copy via a tiny Rust test harness or by
# replaying the same SQL, then:
sqlite3 "$SCRATCH/rename-dry.db" "
  SELECT 'division rows', COUNT(*) FROM division_synopses;
  SELECT 'view rows',     COUNT(*) FROM scene_synopses;
  SELECT 'scope=division',COUNT(*) FROM journal_entries WHERE scope='division';
  SELECT 'scope=scene',   COUNT(*) FROM journal_entries WHERE scope='scene';"
```

Expected: 1157 / 1157 / 8 / 0. **Never run this against the live file.**

- [ ] **Step 6: Commit**

---

### Task 5: On-screen verification

Non-waivable even with review gates waived. A rename that silently drops
synopsis rendering passes every unit test.

- [ ] **Step 1: Launch headless against a MIGRATED copy**

```bash
SCRATCH=/tmp/.../scratchpad
export XDG_RUNTIME_DIR=$(mktemp -d)
cd ~/utono/linux-lit-wt/<branch>
LIT_DEV=1 LIT_NO_MPV=1 LIT_HEADLESS_TEST=1 \
  LIT_DB_PATH="$SCRATCH/rename-dry.db" LIT_LOG_PATH="$SCRATCH/rn.log" \
  LIT_START_WORK=BH-Barrett LIT_START_SCENE=2.0 \
  GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit 2>"$SCRATCH/rn-cage.err"
```

Use the harness `run_in_background`. Poll for `TEST_VIEWPORT_RECT` with an
until-loop. Resize to `1920x1236` and confirm `text_view.height … -> 1098`.
**Kill any prior cage FIRST, in its own command** — a `pkill` chained before
a launch races it and the launch dies.

- [ ] **Step 2: Check the three surfaces the rename could break**

1. **Synopses** — the `scene_synopses` table feeds them. Open the synopsis
   overlay and confirm it renders text, not an empty card.
2. **The Q&A picker** — Ctrl+j, then Alt+t twice. All three scopes must list
   rows, and the header must still read `CHAPTER — BH` (Task 1 renamed the
   code behind it).
3. **The `\` overlay cycle** — must still reach journal entries. Its probe
   filters `scope IN ('scene','passage')`, which Task 4's data migration
   changes to `'division'`. **If Task 1 did not update that predicate, `\`
   silently stops finding entries** — the exact bug class already fixed twice
   in this codebase. Verify explicitly.

- [ ] **Step 3: Open every capture and report what you see**

Per the UI review protocol. Quote the on-screen text.

- [ ] **Step 4: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Exactly that pattern — a bare `pkill -f target/debug/linux-lit` kills the
user's live instance.

---

## Finishing

Per CLAUDE.md, merge each repo to its own master locally, then push.

1. Both repos: build/clippy/tests green, worktree clean.
2. `git checkout master && git merge --no-ff <branch>` in each.
3. Re-verify the build on master.
4. `git push origin master` in each.
5. `git worktree remove …`, then `git branch -d <branch>`.

**Use `git commit -F <file>` for any message containing backticks.**

Merge **linux-lit first, then litdb** — linux-lit carries the migration, and
litdb's compatibility layer already tolerates both names either way.

## Follow-up (NOT this plan)

Once no un-migrated lit.db remains in use, delete the compatibility layer:
the `scene_synopses` view, its three triggers, and the `synopsis_table`
resolvers in both repos. That is a small, separate change and should not be
attempted while any older build might still run.
