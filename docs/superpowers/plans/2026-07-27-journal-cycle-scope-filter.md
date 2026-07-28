# `\` Cycle Journal Stop: Select by Citation Span Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `\` overlay cycle and the main-card line tint find journal
entries by citation span rather than by filing `scope`, and retag the
mis-scoped entries already in lit.db.

**Architecture:** Two one-line SQL predicate removals in `src/db/journal.rs`
(location queries stop testing a filing column), plus a one-time claim-keyed
data migration in `src/db/migrations.rs` that repairs rows whose `scope` was
overwritten by a past litdb re-import. Each is independently testable against
an in-memory SQLite connection.

**Tech Stack:** Rust, rusqlite (SQLite), GTK4. Tests are `#[cfg(test)]` unit
tests using the existing `mem()` in-memory-connection helper in
`src/db/journal.rs`.

**Spec:** `docs/superpowers/specs/2026-07-27-journal-cycle-scope-filter-design.md`

## Global Constraints

- Work on a branch off `master`, per the project's git-branching rule. Master
  is clean at `306e9c06`.
- `cargo build`, `cargo clippy`, and `cargo test` must all be green before the
  branch is finished.
- Do NOT run `cargo run` — the user launches the app. Headless verification
  uses the cage harness only.
- lit.db (`~/utono/litdb/data/lit.db`) is shared, live, mutable state. The
  user's running linux-lit instance may hold it open. Never write to it from a
  test; tests use `Connection::open_in_memory()`.
- Segment scoping is preserved throughout: an entry qualifies only when its
  citation span covers the anchor line. Do NOT reintroduce a scene-band
  fallback.
- `scope` remains the FILING concept (which band `land_on_page` uses); the
  citation remains the LOCATION concept. Every change here enforces that split.

---

### Task 1: `find_journal_page_for_line` selects by span, not scope

Fixes both user-reported `\` failures. `find_journal_page_for_line` is the
probe behind the cycle's journal stop; its `scope = 'passage'` predicate hides
scene-filed entries that carry a citation span.

**Files:**
- Modify: `src/db/journal.rs:637-641` (the SQL in `find_journal_page_for_line`)
- Modify: `src/db/journal.rs` (`mod tests`, append the new tests near the
  existing `passage_citation_ranges_distinct_and_scoped` test at line 807)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: no signature change. `find_journal_page_for_line(conn:
  &Connection, work_abbrev: &str, div1: i64, div2: i64, line_in_div: i64) ->
  Result<Option<(i64, i64, i64)>, rusqlite::Error>` keeps its exact shape,
  returning `(band_div1, band_div2, id)`. Task 3's comment edits describe this
  function's new behavior; Task 4's headless check exercises it.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src/db/journal.rs`. `save_journal_page` cannot
write citations, so the test inserts the scene-filed-but-cited row directly —
which is exactly the row shape a litdb re-import leaves behind.

```rust
    /// Insert a journal entry with an explicit scope AND a citation span.
    /// `save_journal_page` takes no citations and `save_passage_page` forces
    /// `scope='passage'`, so neither can build the row this bug is about: a
    /// `scope='scene'` entry that still carries a span (what a litdb
    /// re-import leaves behind — 19 such rows in lit.db).
    fn insert_cited(
        conn: &Connection,
        work: &str,
        div1: i64,
        div2: i64,
        scope: &str,
        start: &str,
        end: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO journal_entries
                (work_abbrev, div1, div2, question, answer, claude_model,
                 scope, start_citation, end_citation, source_text)
             VALUES (?1, ?2, ?3, 'Q?', 'A.', 'm', ?4, ?5, ?6, 'src')",
            rusqlite::params![work, div1, div2, scope, start, end],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// The reported bug (2026-07-27, both reports). A `scope='scene'` entry
    /// whose citation span covers the cursor line must be reachable: the `\`
    /// cycle probes through this function, and BH's entries are ALL
    /// scene-filed, so the journal stop was dead on that work.
    #[test]
    fn scene_scoped_entry_with_a_span_is_found() {
        let conn = mem();
        // Mirrors lit.db id 24: filed under band (2,0), citing BH.2.0.48.
        let id = insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.48", "BH.2.0.48");

        let hit = find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap();
        assert_eq!(
            hit,
            Some((2, 0, id)),
            "a scene-filed entry whose span covers the line must be found"
        );
    }

    /// Guard: the fix must not widen into "any entry in the band". An entry
    /// with no citation carries no location, so it stays unreachable by `\`
    /// (Ctrl+j and the picker still reach it).
    #[test]
    fn entry_without_citations_is_still_not_found() {
        let conn = mem();
        save_journal_page(&conn, "BH", 2, 0, "Q?", "A.", "m", "scene", "qa").unwrap();

        assert_eq!(find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap(), None);
    }

    /// Guard: segment scoping is intact. A span that does not cover the
    /// anchor must not match, whatever its scope — this is the 2026-07-27
    /// rule that stopped `\` opening a Q&A about a different passage.
    #[test]
    fn span_not_covering_the_anchor_is_not_found() {
        let conn = mem();
        insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.10", "BH.2.0.20");

        assert_eq!(find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap(), None);
    }

    /// Both scopes are candidates now, so the existing priority rule must
    /// still pick the NARROWEST enclosing span (largest start <= line).
    #[test]
    fn narrowest_span_wins_across_mixed_scopes() {
        let conn = mem();
        insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.40", "BH.2.0.60");
        let narrow = insert_cited(&conn, "BH", 2, 0, "passage", "BH.2.0.47", "BH.2.0.49");

        let hit = find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap();
        assert_eq!(hit, Some((2, 0, narrow)), "nearest start must still win");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib scene_scoped_entry_with_a_span_is_found -- --nocapture
```

Expected: FAIL. The assertion reports `left: None, right: Some((2, 0, 1))` —
the row exists but the `scope = 'passage'` predicate filters it out.
`entry_without_citations_is_still_not_found` and
`span_not_covering_the_anchor_is_not_found` PASS already (they assert the
behavior that is correct today); `narrowest_span_wins_across_mixed_scopes`
FAILS, returning the passage row's id only by luck of it being the sole
candidate — confirm it fails or passes for the right reason before moving on.

- [ ] **Step 3: Drop the scope predicate**

In `src/db/journal.rs`, in `find_journal_page_for_line`, change the prepared
statement from:

```rust
    let mut stmt = conn.prepare(
        "SELECT div1, div2, id, start_citation, end_citation FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'passage' \
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
```

to:

```rust
    let mut stmt = conn.prepare(
        "SELECT div1, div2, id, start_citation, end_citation FROM journal_entries \
         WHERE work_abbrev = ?1 \
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib journal:: -- --nocapture
```

Expected: PASS — all four new tests, plus every pre-existing `db::journal`
test still green (notably `passage_citation_ranges_distinct_and_scoped`,
which is unaffected because its scene row has NULL citations).

- [ ] **Step 5: Commit**

```bash
git add src/db/journal.rs
git commit -m "fix(journal): \\ cycle finds cited entries whatever their scope

find_journal_page_for_line filtered scope='passage', so a scene-filed entry
carrying a citation span was invisible to the \\ overlay cycle. All 11 of
BH's journal entries are scene-filed, so the journal stop was dead on that
work: \\ from the gloss stop toasted 'Nothing else to cycle to' on a passage
whose Q&A the picker was listing.

The band says where an entry is FILED; the citation says where the passage
LIVES. Selection is now purely span-based, as the function's own doc comment
already described.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 2: `find_passage_citation_ranges` selects by span, not scope

The same defect on the reader's line-tint path: a line covered by a journal
Q&A is tinted like a reader-glossed line, and scene-filed entries produced no
tint. Found during planning; recorded in the spec's "Second site" section.

**Files:**
- Modify: `src/db/journal.rs:596-600` (the SQL in `find_passage_citation_ranges`)
- Modify: `src/db/journal.rs` (`mod tests`)

**Interfaces:**
- Consumes: `insert_cited(conn, work, div1, div2, scope, start, end) -> i64`
  from Task 1's test module. Task 1 must land first.
- Produces: no signature change. `find_passage_citation_ranges(conn:
  &Connection, work_abbrev: &str) -> Result<Vec<(String, String)>,
  rusqlite::Error>` keeps its shape.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src/db/journal.rs`:

```rust
    /// The line tint (`apply_reader_gloss_highlighting`) marks lines covered
    /// by a journal Q&A. It read the same `scope='passage'` filter as the `\`
    /// probe, so scene-filed entries left their lines untinted — the reader
    /// had no on-page sign the Q&A existed.
    #[test]
    fn citation_ranges_include_scene_scoped_entries() {
        let conn = mem();
        insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.48", "BH.2.0.48");
        insert_cited(&conn, "BH", 3, 0, "passage", "BH.3.0.80", "BH.3.0.82");

        let mut ranges = find_passage_citation_ranges(&conn, "BH").unwrap();
        ranges.sort();
        assert_eq!(
            ranges,
            vec![
                ("BH.2.0.48".to_string(), "BH.2.0.48".to_string()),
                ("BH.3.0.80".to_string(), "BH.3.0.82".to_string()),
            ],
            "scene-filed entries with a span must tint their lines too"
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test --lib citation_ranges_include_scene_scoped_entries -- --nocapture
```

Expected: FAIL — the returned vec holds only the `BH.3.0.80` pair; the
scene-filed range is missing.

- [ ] **Step 3: Drop the scope predicate**

In `src/db/journal.rs`, in `find_passage_citation_ranges`, change:

```rust
    let mut stmt = conn.prepare(
        "SELECT DISTINCT start_citation, end_citation FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'passage'
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
```

to:

```rust
    let mut stmt = conn.prepare(
        "SELECT DISTINCT start_citation, end_citation FROM journal_entries
         WHERE work_abbrev = ?1
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
```

Also update the doc comment above the function (`src/db/journal.rs:587-591`),
replacing "of every passage-scope Q&A entry for a work" with "of every Q&A
entry that carries one, whatever its filing scope".

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib journal:: -- --nocapture
```

Expected: PASS, including the pre-existing
`passage_citation_ranges_distinct_and_scoped` — its scene-scope row has NULL
citations, so the `IS NOT NULL` guard still excludes it and its assertion is
unchanged. If that test now fails, STOP: the predicate was over-widened.

- [ ] **Step 5: Commit**

```bash
git add src/db/journal.rs
git commit -m "fix(journal): line tint covers cited entries whatever their scope

find_passage_citation_ranges carried the same scope='passage' filter as
find_journal_page_for_line, so a scene-filed entry with a citation span left
its lines untinted on the main card — no on-page sign the Q&A existed.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 3: Correct the stale comments that assert the false rule

Three comments state "scene entries carry no span". That is false in the data
and is what produced the defect. Left in place, the next session re-derives it.

**Files:**
- Modify: `src/input/actions/journal.rs:1292-1298`
- Modify: `src/input/actions/overlay_cycle.rs:33-40`
- Modify: `src/db/journal.rs:605-610` (the `find_journal_page_for_line` doc
  comment, which says "The passage-scope Q&A entry whose …")

**Interfaces:**
- Consumes: the behavior established in Tasks 1 and 2.
- Produces: nothing code-facing. Comments only — no test.

- [ ] **Step 1: Fix the probe comment**

In `src/input/actions/journal.rs`, replace the comment block at lines
1292-1298:

```rust
    // SPAN-SCOPED ONLY (2026-07-27). The scene-band fallback that used to sit
    // here answered "does this CHAPTER have any Q&A" — a question with no
    // reference to the cursor — so `\` opened whichever entry sorted oldest in
    // the band. The `\` lap shows material about the segment under the cursor,
    // so the only hit that counts is an entry whose citation span contains the
    // anchor. FILING SCOPE IS NOT THE TEST (corrected 2026-07-27): this used
    // to require `scope='passage'` on the belief that scene-filed entries
    // carry no span. They do — a litdb re-import rewrites `scope` while
    // leaving citations intact, and 19 rows in lit.db are scene-filed WITH a
    // span. Entries with NO citation carry no location and stay unreachable
    // by `\`; Ctrl+j and the picker still reach them.
```

- [ ] **Step 2: Fix the module doc**

In `src/input/actions/overlay_cycle.rs`, replace the final sentence of the
EVERY STOP IS SEGMENT-SCOPED paragraph (lines 38-40):

```rust
//! `JournalOpenScope::SegmentOnly` here; Ctrl+j keeps the band fallback.
//! A journal entry is reachable by `\` when its citation span covers the
//! anchor, whatever its filing `scope` (corrected 2026-07-27 — the probe
//! used to require `scope='passage'`, which hid every scene-filed entry that
//! carried a span, and made the journal stop dead on Bleak House). Entries
//! with no citation at all are unreachable by `\` — reach them with Ctrl+j
//! or the picker.
```

- [ ] **Step 3: Fix the query doc comment**

In `src/db/journal.rs`, change the first line of the
`find_journal_page_for_line` doc comment from:

```rust
/// The passage-scope Q&A entry whose `[start_citation, end_citation]` line
```

to:

```rust
/// The Q&A entry — any filing scope — whose `[start_citation, end_citation]` line
```

- [ ] **Step 4: Verify nothing broke**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Finished` with no errors. Comments only, so no behavior change.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/journal.rs src/input/actions/overlay_cycle.rs src/db/journal.rs
git commit -m "docs(journal): correct the 'scene entries carry no span' claim

Three comments asserted a rule that is false in the data and that produced
the \\ cycle defect. A litdb re-import rewrites scope while leaving citations
intact; 19 rows in lit.db are scene-filed WITH a span.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 4: One-time migration retagging mis-scoped entries

The data itself is wrong, not just the queries. `save_vocab_page` hardcodes
`scope='passage'`, yet lit.db's two BH vocab entries read `scope='scene'` —
something rewrote them after insert, the same event that left two rows reading
`unassigned-after-reimport`. Tasks 1-2 make the reader correct regardless;
this repairs the stored rows so `scope` again means what it says.

**Retag rule:** `scope='scene'` AND both citations non-NULL AND `source_text`
non-NULL. `source_text` is written only by `save_passage_page` /
`save_vocab_page`, so that trio is the signature of a passage-created entry.
Verified against lit.db: retags 17 rows (BH 10, TT 7) and correctly excludes
the 2 chapter-level questions (BH id 7, TT id 57), which have placeholder `.0`
line citations and no `source_text`.

**Files:**
- Modify: `src/db/migrations.rs` (add the migration beside
  `purge_stale_passage_journal_audio`, which is the pattern to copy)
- Modify: `src/db/migrations.rs` (`mod tests` at line 426)
- Modify: `src/app/mod.rs:3534` (call it in the `BOOKMARKS_INIT` block)

**Interfaces:**
- Consumes: `ensure_one_time_migrations_table(conn: &Connection) ->
  Result<(), rusqlite::Error>`, already called at `src/app/mod.rs:3532`, and
  `crate::db::journal::ensure_journal_table` at `src/app/mod.rs:3528` — both
  run BEFORE the new call site, so the tables exist.
- Produces: `pub fn retag_passage_scoped_journal_entries(conn: &Connection) ->
  Result<usize, rusqlite::Error>` — returns the number of rows retagged, or
  `Ok(0)` when the claim key was already taken.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src/db/migrations.rs`:

```rust
    /// The migration retags entries a re-import mis-filed, and ONLY those.
    /// The signature of a passage-created entry is citations + source_text
    /// (only save_passage_page / save_vocab_page write source_text).
    #[test]
    fn retag_only_touches_cited_entries_with_source_text() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();

        let insert = |scope: &str, cite: Option<&str>, src: Option<&str>| {
            conn.execute(
                "INSERT INTO journal_entries
                    (work_abbrev, div1, div2, question, answer, claude_model,
                     scope, start_citation, end_citation, source_text)
                 VALUES ('BH', 2, 0, 'Q?', 'A.', 'm', ?1, ?2, ?2, ?3)",
                rusqlite::params![scope, cite, src],
            )
            .unwrap();
            conn.last_insert_rowid()
        };

        // Mis-filed by a re-import: cited AND has source_text -> retag.
        let mis_filed = insert("scene", Some("BH.2.0.48"), Some("How Alexander wept…"));
        // Genuinely chapter-level: placeholder citation, no source_text -> leave.
        let chapter_q = insert("scene", Some("BH.1.0.0"), None);
        // No citation at all -> leave.
        let bare = insert("scene", None, None);
        // Already correct -> untouched, and not double-counted.
        let already = insert("passage", Some("BH.3.0.80"), Some("src"));

        let n = retag_passage_scoped_journal_entries(&conn).unwrap();
        assert_eq!(n, 1, "exactly the mis-filed row is retagged");

        let scope_of = |id: i64| -> String {
            conn.query_row(
                "SELECT scope FROM journal_entries WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(scope_of(mis_filed), "passage");
        assert_eq!(scope_of(chapter_q), "scene", "chapter-level Q stays scene");
        assert_eq!(scope_of(bare), "scene", "uncited entry stays scene");
        assert_eq!(scope_of(already), "passage");
    }

    /// Claim-keyed: a second run is a no-op, matching
    /// purge_stale_passage_journal_audio's contract.
    #[test]
    fn retag_runs_only_once() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::journal::ensure_journal_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();
        conn.execute(
            "INSERT INTO journal_entries
                (work_abbrev, div1, div2, question, answer, claude_model,
                 scope, start_citation, end_citation, source_text)
             VALUES ('BH', 2, 0, 'Q?', 'A.', 'm', 'scene',
                     'BH.2.0.48', 'BH.2.0.48', 'src')",
            [],
        )
        .unwrap();

        assert_eq!(retag_passage_scoped_journal_entries(&conn).unwrap(), 1);
        assert_eq!(
            retag_passage_scoped_journal_entries(&conn).unwrap(),
            0,
            "the claim key must make a second run a no-op"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib retag_ -- --nocapture
```

Expected: FAIL to COMPILE — `cannot find function
retag_passage_scoped_journal_entries in this scope`. That is the expected
red state.

- [ ] **Step 3: Write the migration**

Add to `src/db/migrations.rs`, immediately after
`purge_stale_passage_journal_audio`:

```rust
/// Marker key claimed by `retag_passage_scoped_journal_entries` so the retag
/// runs exactly once across the DB's lifetime. Bump the date suffix if a
/// future re-import mis-files entries again.
const RETAG_PASSAGE_SCOPE_KEY: &str = "retag-passage-scope-2026-07-27";

/// One-time repair for journal entries whose `scope` a litdb re-import
/// overwrote. `save_passage_page` and `save_vocab_page` both hardcode
/// `scope='passage'`, yet lit.db holds vocab entries reading `scope='scene'` —
/// the same event that left two rows reading `unassigned-after-reimport`
/// rewrote them.
///
/// The signature of a passage-created entry is a citation pair PLUS a
/// non-NULL `source_text`: only those two writers store source_text. A
/// chapter-level question saved from the reader has a placeholder `.0`
/// citation and no source_text, so it is correctly left alone.
///
/// This is a data repair, not a correctness dependency: `find_journal_page_for_line`
/// and `find_passage_citation_ranges` select on the citation span and no
/// longer consult `scope` at all. It exists so `scope` again means what it
/// says for the paths that legitimately filter on it (`find_passage_pages`,
/// `find_journal_pages`).
///
/// Claims `RETAG_PASSAGE_SCOPE_KEY` in `one_time_migrations` (caller must
/// `ensure_one_time_migrations_table` first) before writing anything; if the
/// marker was already claimed, returns `Ok(0)` without touching a row.
pub fn retag_passage_scoped_journal_entries(
    conn: &Connection,
) -> Result<usize, rusqlite::Error> {
    let claimed = conn.execute(
        "INSERT OR IGNORE INTO one_time_migrations (key) VALUES (?1)",
        [RETAG_PASSAGE_SCOPE_KEY],
    )?;
    if claimed == 0 {
        return Ok(0);
    }
    conn.execute(
        "UPDATE journal_entries
            SET scope = 'passage'
          WHERE scope = 'scene'
            AND start_citation IS NOT NULL
            AND end_citation IS NOT NULL
            AND source_text IS NOT NULL",
        [],
    )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib retag_ -- --nocapture
```

Expected: PASS, both tests.

- [ ] **Step 5: Wire it into startup**

In `src/app/mod.rs`, add to the `BOOKMARKS_INIT.call_once` block, immediately
after the `purge_stale_passage_journal_audio` line at 3534:

```rust
            let _ = crate::db::migrations::retag_passage_scoped_journal_entries(&conn);
```

Order matters: `ensure_journal_table` (3528) and
`ensure_one_time_migrations_table` (3532) both run earlier in the same block,
so both tables exist by this point.

- [ ] **Step 6: Verify the build and full suite**

```bash
cargo build 2>&1 | tail -5 && cargo clippy 2>&1 | tail -20 && cargo test 2>&1 | tail -20
```

Expected: `Finished`, no clippy warnings on the changed files, all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/db/migrations.rs src/app/mod.rs
git commit -m "fix(journal): retag entries a re-import mis-filed as scene scope

save_vocab_page hardcodes scope='passage', yet lit.db's BH vocab entries read
'scene' — a past litdb re-import rewrote the column, the same event that left
two rows reading 'unassigned-after-reimport'. Retags rows carrying the
passage-created signature (citations + source_text), which is 17 rows in
lit.db and correctly skips the 2 chapter-level questions.

Claim-keyed via one_time_migrations, so it runs exactly once.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 5: Headless round-trip verification

Report B as an acceptance test. A green build is not "done" for a change with
visible behavior — this exercises probe and open together through the real
cycle. Per CLAUDE.md, agents verify GUI changes themselves via cage/grim.

**Files:**
- No source changes. This task produces evidence, not code.

**Interfaces:**
- Consumes: everything from Tasks 1-4, merged and built.
- Produces: a pass/fail observation to report to the user.

**Note on which DB this exercises.** `scripts/land-on.sh` runs against a
PRIVATE COPY of lit.db, never the shared file (see its header comment). So
this run reads entries with their ORIGINAL `scope='scene'` — Task 4's retag
has not touched them. That is deliberate and makes the test stronger: it
proves Tasks 1-2 fixed the reader independently of the data repair. Do NOT
"fix" the harness to use the live DB.

- [ ] **Step 1: Build and launch headless on the target chapter**

```bash
cargo build
./scripts/land-on.sh BH-Barrett 2.0
```

`land-on.sh` takes `WORK div1.div2 [journal|synopsis|gloss]` and is at
`scripts/`, NOT under the skill directory. It sets its own hermetic env
(private DB copy, private log, `LIT_DEV`/`LIT_HEADLESS_TEST`), so it does NOT
need wrapping in `e2e-env.sh`. Launch with no overlay arg so the run LANDS IN
READER MODE — the vim ask card eats Escapes one modal layer at a time, so
escaping into reader mode from an overlay is unreliable.

Launch must stay foreground-alive — use the harness `run_in_background`, never
`nohup`/`setsid`/`timeout`, which kill the instance the moment the wrapper
returns. On success it prints `WAYLAND_DISPLAY=…` and `log=…` to stderr;
export that `WAYLAND_DISPLAY` for every `wtype`/`grim` below, and read that
log path rather than guessing a `-{n}` slot.

- [ ] **Step 2: Move the cursor to the target line and confirm**

The landing is at the top of chapter 2; the "How Alexander wept" line is
BH.2.0.48. Drive `j` until the cursor reaches it, confirming position from the
log's `CURSOR_LINE:` breadcrumbs rather than by eye:

```bash
rg 'CURSOR_LINE:' "$LAND_ON_LOG" | tail -3
```

Do not proceed until the cursor is on 48 — the whole test is about what covers
THAT line.

- [ ] **Step 3: Drive the round trip and capture**

```bash
wtype -k backslash    # reader -> gloss stop
sleep 1
wtype -k backslash    # gloss -> journal stop  (this is the fix)
sleep 1
grim -o HEADLESS-1 target/ui/journal-roundtrip.png
```

Confirm each keypress landed by checking for its `KEY:` line in the log before
trusting the screenshot. An empty ~2-byte PNG from `grim` means not-mapped-yet,
not failure — check `stat -c%s` and retry after a sleep.

- [ ] **Step 4: Open the PNG and report what is on screen**

Per the UI review protocol, open the capture and report inline what it shows.

Expected: the journal overlay showing entry 24 — the citation line
`— Bleak House (Sean Barrett), 2.0.48` and the question "To Dickens's
contemporaries, how would they have understood the assertion that 'everbody
knows' how Alexander wept?".

FAIL if the toast "Nothing else to cycle to for this passage" appears, or the
gloss overlay is still showing.

- [ ] **Step 5: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Use exactly this pattern — a bare `pkill -f target/debug/linux-lit` kills the
user's live instance.

- [ ] **Step 6: Record lit.db's pre-migration state for the user to check**

The retag runs at app start against the SHARED lit.db, so it applies on the
user's next real launch — not during any test here. Capture the before-state
now so the after can be confirmed:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT scope, COUNT(*) FROM journal_entries GROUP BY 1;"
```

Expected BEFORE: `author|2`, `passage|27`, `scene|25`,
`unassigned-after-reimport|2`.
Expected AFTER the user's next launch: `passage|44`, `scene|8`, the other two
unchanged (17 rows move).

Do NOT run the migration by hand against lit.db, and do not launch the app to
force it — the user's running instance may hold the DB open, and `cargo run`
is theirs to invoke.

- [ ] **Step 7: Report to the user**

State plainly what was observed, with the screenshot. Include the lit.db
before/after counts from Step 6 so they can confirm the retag on next launch.
If the headless launch genuinely fails after a retry, say so and hand off the
manual steps: open BH-Barrett ch. 2, put the cursor on "How Alexander wept…",
press `\` twice, expect the journal entry rather than the toast.

---

## Finishing

Per CLAUDE.md's finishing-a-branch rule: merge back to master locally, then
push — no PR, no asking.

1. Confirm `cargo build`, `cargo clippy`, `cargo test` green and the tree clean.
2. `git checkout master && git merge --no-ff <branch>`
3. Re-verify the build on master.
4. `git push origin master`
5. `git branch -d <branch>`

The spec threshold applies: this change spans one subsystem and no keybinds
moved, so no pre-merge code-review gate is required. The on-screen check in
Task 5 is NOT waivable — it is correctness, not review.

## Follow-ups (NOT this branch)

- **Upstream: what rewrote `scope`?** Two rows read
  `unassigned-after-reimport` and the vocab entries lost their hardcoded
  `scope='passage'`, so a litdb re-import is overwriting the column. Per
  CLAUDE.md's upstream-routing rule that fix belongs in litdb, with a
  troubleshooting-ledger entry here linking to the upstream commit. Task 4
  repairs today's data but does not prevent recurrence.
- **Q&A picker scope cycling** — the user's original request, already scoped:
  author / work / scene cycled with Alt+t, opening on Work, author scope
  spanning every work by the current author. Gets its own spec.
