# Per-Work Narrator Voice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the prose/gloss narrator voice data-driven — resolvable per-work, falling back per-author, falling back to a global male default — replacing the hardcoded `starts_with("BCP")` rule.

**Architecture:** Add a nullable `works.default_voice_id` column and an `author_default_voice` table to lit.db (seeded Shakespeare→Eleanor). A new helper `resolve_prose_voice` resolves per-work → per-author → global Benedick. The prose branch of `resolve_default_voice` delegates to it; verse resolution is unchanged.

**Tech Stack:** Rust, rusqlite (SQLite), cargo test. Schema migration lives in `ensure_voice_catalog_table` (the narration-voice migration, called once at `app.rs:2544`).

---

## File Structure

- **Modify** `src/db/queries.rs`:
  - `ensure_voice_catalog_table` (line ~647) — also add the `works.default_voice_id` column, create `author_default_voice`, seed Shakespeare→Eleanor.
  - `resolve_default_voice` (line ~859) — prose branch delegates to the new helper.
  - **Add** `resolve_prose_voice` — new private helper.
  - Test module — extend `seed_catalog_and_chars` (line ~2775); replace `resolve_prose_narrator_eleanor_except_bcp` (line ~2878) with new tests.
- **Modify** `docs/guides/elevenlabs-v3-custom-voices.md` — update the prose-narrator note.

The production migration entry point (`app.rs:2544`) needs **no change** — it already calls `ensure_voice_catalog_table`, which we extend.

## Pre-flight (run once before Task 1)

- [ ] **Snapshot lit.db** (only undo — the DB is gitignored, no git history):

```bash
\cp -f ~/utono/litdb/data/lit.db /tmp/lit.db.bak-narrator-voice
ls -l /tmp/lit.db.bak-narrator-voice
```

Expected: the backup file exists with a non-zero size.

---

### Task 1: Schema — add column, table, and seed in `ensure_voice_catalog_table`

**Files:**
- Modify: `src/db/queries.rs` — end of `ensure_voice_catalog_table`, before `Ok(())` at line ~684.

- [ ] **Step 1: Write the failing test**

Add this test inside the `#[cfg(test)] mod tests` block (after `seed_catalog_and_chars`, near line 2783). It asserts the schema migration creates the column and seeds the author row.

```rust
    #[test]
    fn ensure_voice_catalog_adds_author_voice_schema() {
        let conn = Connection::open_in_memory().unwrap();
        // works table must exist for the ADD COLUMN to target.
        conn.execute_batch(
            "CREATE TABLE works (abbrev TEXT UNIQUE NOT NULL, author TEXT);"
        ).unwrap();
        ensure_voice_catalog_table(&conn).unwrap();
        // works.default_voice_id column now exists.
        let has_col: bool = conn
            .prepare("SELECT default_voice_id FROM works")
            .is_ok();
        assert!(has_col, "works.default_voice_id column should exist");
        // author_default_voice seeded Shakespeare -> Eleanor.
        let vid: String = conn
            .query_row(
                "SELECT voice_id FROM author_default_voice WHERE author = 'Shakespeare'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(vid, crate::elevenlabs::DEFAULT_FEMALE_VOICE_ID);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin linux-lit ensure_voice_catalog_adds_author_voice_schema 2>&1 | rg 'test result|FAILED|no such'`
Expected: FAIL — `no such table: author_default_voice` (or no such column).

- [ ] **Step 3: Add the migration code**

In `src/db/queries.rs`, inside `ensure_voice_catalog_table`, replace the final `Ok(())` (line ~684) with the schema additions then `Ok(())`:

```rust
    // --- Per-work / per-author narrator voice (prose/gloss) ---
    // works.default_voice_id: nullable per-work override. SQLite ADD COLUMN has
    // no IF NOT EXISTS, so guard on PRAGMA table_info to stay idempotent.
    let has_default_voice_col: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('works') WHERE name = 'default_voice_id'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)).optional())
        .unwrap_or(None)
        .unwrap_or(false);
    if !has_default_voice_col {
        // Ignore the error if the works table doesn't exist yet (fresh/test DB);
        // the column is created with the table elsewhere or not needed.
        let _ = conn.execute("ALTER TABLE works ADD COLUMN default_voice_id TEXT", []);
    }

    // author_default_voice: per-author narrator. Seed Shakespeare -> Eleanor;
    // every other author falls through to the global male default at resolve time.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS author_default_voice (
            author   TEXT PRIMARY KEY,
            voice_id TEXT NOT NULL
        );"
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO author_default_voice (author, voice_id) VALUES ('Shakespeare', ?1)",
        rusqlite::params![DEFAULT_FEMALE_VOICE_ID],
    )?;

    Ok(())
```

Note: `use crate::elevenlabs::*;` at the top of the function already brings `DEFAULT_FEMALE_VOICE_ID` into scope. `.optional()` requires `use rusqlite::OptionalExtension;` — verify it's imported at the top of the file; if not, add `use rusqlite::OptionalExtension;` (it is already used by `resolve_default_voice`, so it is in scope).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bin linux-lit ensure_voice_catalog_adds_author_voice_schema 2>&1 | rg 'test result'`
Expected: PASS — `1 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(tts): schema for per-work/author narrator voice

Add works.default_voice_id column and author_default_voice table to
ensure_voice_catalog_table; seed Shakespeare -> Eleanor. Idempotent.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `resolve_prose_voice` helper + delegate the prose branch

**Files:**
- Modify: `src/db/queries.rs` — add helper above `resolve_default_voice` (line ~859); replace the prose branch inside it (lines ~865–880).
- Modify: test seed `seed_catalog_and_chars` (line ~2775) to create a `works` table with authors and the author-voice schema.

- [ ] **Step 1: Extend the test seed helper**

Replace `seed_catalog_and_chars` (line ~2775) with this version, which also creates a `works` table (with a Shakespeare work, a BCP work, a Dickens work, and a per-work-override work) and runs the author-voice migration:

```rust
    fn seed_catalog_and_chars(conn: &Connection) {
        ensure_voice_catalog_table(conn).unwrap();
        ensure_characters_table(conn).unwrap();
        // works table with authors, for prose narrator resolution.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS works (
                abbrev TEXT UNIQUE NOT NULL,
                author TEXT,
                default_voice_id TEXT
            );
            INSERT INTO works (abbrev, author) VALUES ('Rom', 'Shakespeare');
            INSERT INTO works (abbrev, author) VALUES ('Lr', 'Shakespeare');
            INSERT INTO works (abbrev, author) VALUES ('Ham', 'Shakespeare');
            INSERT INTO works (abbrev, author) VALUES ('BCP1662', 'Book of Common Prayer');
            INSERT INTO works (abbrev, author) VALUES ('BCP1549M', 'Book of Common Prayer');
            INSERT INTO works (abbrev, author) VALUES ('OT', 'Charles Dickens');
            INSERT INTO works (abbrev, author, default_voice_id)
                VALUES ('OVERRIDE', 'Shakespeare', 'OVERRIDE_VOICE_XXXXX');"
        ).unwrap();
        // Re-run migration now that works exists (idempotent): the first call
        // above already created+seeded author_default_voice and created the
        // voice_catalog; this second call is a no-op safety net proving
        // idempotency with the works table present.
        ensure_voice_catalog_table(conn).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Rom','JULIET','female',14)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Lr','LEAR','male',80)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Rom','NURSE','female')", []).unwrap();
    }
```

- [ ] **Step 2: Write the failing tests**

Replace the existing `resolve_prose_narrator_eleanor_except_bcp` test (line ~2878) with:

```rust
    #[test]
    fn resolve_prose_voice_precedence() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        let model = crate::elevenlabs::OP_MODEL_ID.to_string();
        let eleanor = crate::elevenlabs::DEFAULT_FEMALE_VOICE_ID.to_string();
        let benedick = crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string();

        // Shakespeare prose -> Eleanor (author_default_voice row).
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", false),
            (eleanor.clone(), model.clone())
        );
        // BCP prose -> Benedick (no author row -> global default).
        assert_eq!(
            resolve_default_voice(&conn, "BCP1662", "UNKNOWN", false),
            (benedick.clone(), model.clone())
        );
        // Other author (Dickens) prose -> Benedick (global default).
        assert_eq!(
            resolve_default_voice(&conn, "OT", "NOBODY", false),
            (benedick.clone(), model.clone())
        );
        // Per-work override beats the author default (even for Shakespeare).
        assert_eq!(
            resolve_default_voice(&conn, "OVERRIDE", "ANY", false),
            ("OVERRIDE_VOICE_XXXXX".to_string(), model.clone())
        );
        // Verse path is UNCHANGED: UNKNOWN verse -> male; Juliet verse -> female.
        assert_eq!(
            resolve_default_voice(&conn, "BCP1662", "UNKNOWN", true),
            (benedick.clone(), model.clone())
        );
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", true),
            (crate::elevenlabs::JULIET_VOICE_ID.to_string(), model)
        );
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --bin linux-lit resolve_prose_voice_precedence 2>&1 | rg 'test result|FAILED|assert'`
Expected: FAIL — the override/global-default assertions fail because the current code returns Eleanor for all non-BCP prose and Benedick only for BCP (no per-work or author lookup yet).

- [ ] **Step 4: Add the helper**

Insert this function immediately above `pub fn resolve_default_voice` (line ~859):

```rust
/// The narrator voice_id for PROSE/gloss of `work_abbrev`:
/// per-work `works.default_voice_id` → per-author `author_default_voice` →
/// global male default (Benedick). Always resolves; a query error logs and
/// falls through (e.g. a fresh DB without a `works` table → global default).
fn resolve_prose_voice(conn: &Connection, work_abbrev: &str) -> String {
    // 1. Per-work override.
    let per_work: Option<String> = conn
        .query_row(
            "SELECT default_voice_id FROM works
             WHERE abbrev = ?1 AND default_voice_id IS NOT NULL",
            rusqlite::params![work_abbrev],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!("resolve_prose_voice: per-work query error for {}: {}", work_abbrev, e);
            None
        });
    if let Some(v) = per_work {
        return v;
    }
    // 2. Per-author default (join works.author -> author_default_voice).
    let per_author: Option<String> = conn
        .query_row(
            "SELECT adv.voice_id FROM works w
             JOIN author_default_voice adv ON adv.author = w.author
             WHERE w.abbrev = ?1",
            rusqlite::params![work_abbrev],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!("resolve_prose_voice: per-author query error for {}: {}", work_abbrev, e);
            None
        });
    if let Some(v) = per_author {
        return v;
    }
    // 3. Global default: the male narrator.
    crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string()
}
```

- [ ] **Step 5: Delegate the prose branch**

In `resolve_default_voice`, replace the current prose short-circuit (lines ~865–874, the comment block + the `if !is_verse { ... }` that branches on `starts_with("BCP")`) with:

```rust
    // Prose (explication) reads in ONE narrator per work, resolved from data:
    // per-work override → per-author default → global male default. Shakespeare
    // is seeded to Eleanor; all other authors fall to the male default. (Verse
    // still picks by (gender, age) below; a per-gloss associated voice still
    // overrides this default at the call site in play_block_tts.)
    if !is_verse {
        return (
            resolve_prose_voice(conn, work_abbrev),
            crate::elevenlabs::OP_MODEL_ID.to_string(),
        );
    }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --bin linux-lit resolve 2>&1 | rg 'test result|FAILED'`
Expected: PASS — all resolve tests pass, including `resolve_prose_voice_precedence`.

- [ ] **Step 7: Full build + test sweep**

Run: `cargo build 2>&1 | rg -i 'error|Finished' && cargo test --bin linux-lit 2>&1 | rg 'test result' | tail -3`
Expected: build `Finished`; test results all `ok`, `0 failed`.

- [ ] **Step 8: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(tts): data-driven prose narrator; drop BCP hardcode

resolve_default_voice's prose branch now delegates to resolve_prose_voice:
per-work works.default_voice_id -> per-author author_default_voice ->
global Benedick. Shakespeare seeds to Eleanor; all other authors default
to male. Removes the starts_with(\"BCP\") special case. Verse unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Apply the migration to the live lit.db + verify

**Files:** none (data migration). The app runs `ensure_voice_catalog_table` at startup, but apply it now so existing glosses resolve correctly without launching the app.

- [ ] **Step 1: Apply schema to live lit.db**

```bash
sqlite3 ~/utono/litdb/data/lit.db "ALTER TABLE works ADD COLUMN default_voice_id TEXT;" 2>&1 || echo "(column may already exist — ok)"
sqlite3 ~/utono/litdb/data/lit.db "CREATE TABLE IF NOT EXISTS author_default_voice (author TEXT PRIMARY KEY, voice_id TEXT NOT NULL);"
sqlite3 ~/utono/litdb/data/lit.db "INSERT OR IGNORE INTO author_default_voice (author, voice_id) VALUES ('Shakespeare', 'D4LX5VBnEN6zrrsnTMO8');"
```

- [ ] **Step 2: Verify the live rows (cannot git-diff the DB)**

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT 1 FROM pragma_table_info('works') WHERE name='default_voice_id';"
sqlite3 ~/utono/litdb/data/lit.db "SELECT author, voice_id FROM author_default_voice;"
```

Expected: first prints `1`; second prints `Shakespeare|D4LX5VBnEN6zrrsnTMO8`.

- [ ] **Step 3: No commit** — lit.db is gitignored. Record the change is applied; the `/tmp` snapshot is the undo.

---

### Task 4: Docs

**Files:**
- Modify: `docs/guides/elevenlabs-v3-custom-voices.md` — the prose-narrator note (the paragraph beginning "**Prose narrator, with one exception.**").

- [ ] **Step 1: Replace the BCP-exception note**

Replace the `> **Prose narrator, with one exception.** ...` block with:

```markdown
> **Prose narrator is data-driven.** All prose (explication) reads in one
> narrator per work, resolved by `resolve_prose_voice`:
> `works.default_voice_id` (per-work override) → `author_default_voice` matched
> on `works.author` (per-author default) → the global male default (Benedick).
> Shakespeare is seeded to Eleanor; every other author (BCP, Dickens, Ibsen,
> KJV, …) falls to the male default unless given an `author_default_voice` row
> or a per-work `default_voice_id`. To change a work's narrator, set
> `works.default_voice_id`; to change an author's, insert/update
> `author_default_voice`. Verse is unaffected — it still resolves by
> (gender, age), so a work's UNKNOWN speakers land on the male default there too.
```

- [ ] **Step 2: Commit**

```bash
git add docs/guides/elevenlabs-v3-custom-voices.md
git commit -m "docs(tts): describe data-driven prose narrator resolution

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final verification

- [ ] `cargo build 2>&1 | rg -i 'error|Finished'` → `Finished`.
- [ ] `cargo test --bin linux-lit 2>&1 | rg 'test result' | tail -3` → all `ok`, `0 failed`.
- [ ] Live lit.db: `SELECT author, voice_id FROM author_default_voice;` → `Shakespeare|D4LX5VBnEN6zrrsnTMO8`.
- [ ] (Optional, in-app) Open a Shakespeare gloss → narrated by Eleanor; a BCP gloss → narrated by Benedick.
