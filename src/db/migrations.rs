//! One-shot schema migrations — the `ensure_*` DDL probes (audit #94): each
//! fn is idempotent, takes only a `&Connection`, and creates-or-upgrades one
//! table or column family. Moved verbatim out of queries.rs (pure motion).
//! The varying SQL bodies are load-bearing and deliberately NOT deduplicated
//! (see the audit ledger's standing exclusions). `column_exists`, the shared
//! pragma probe (#37), stays in queries.rs — non-migration code uses it too.

use rusqlite::{Connection, OptionalExtension};

use super::queries::column_exists;

/// Column + constraint body of the gloss_audio table, shared by the fresh-install
/// CREATE and the legacy-rebuild migration so the two cannot drift.
const GLOSS_AUDIO_COLUMNS: &str = "
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    gloss_id        INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL DEFAULT 'explication',
    paragraph_index INTEGER NOT NULL,
    audio_path      TEXT NOT NULL,
    voice_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(gloss_id, kind, paragraph_index, voice_id)
";

/// Column body of the synopsis_audio table (per-paragraph synopsis TTS cache).
/// Keyed by scene + paragraph + voice (synopses have no glosses FK).
const SYNOPSIS_AUDIO_COLUMNS: &str = "
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    work_abbrev     TEXT NOT NULL,
    div1            INTEGER NOT NULL,
    div2            INTEGER NOT NULL,
    paragraph_index INTEGER NOT NULL,
    audio_path      TEXT NOT NULL,
    voice_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(work_abbrev, div1, div2, paragraph_index, voice_id)
";

const JOURNAL_AUDIO_COLUMNS: &str = "
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_id        INTEGER NOT NULL,
    paragraph_index INTEGER NOT NULL,
    audio_path      TEXT NOT NULL,
    voice_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(entry_id, paragraph_index, voice_id)
";

/// Ensure the `glosses` and `scene_synopses` tables carry a `claude_model`
/// column recording which Claude model authored each row. These two tables are
/// part of the external lit.db core schema (not created by the app), so this is
/// an idempotent legacy migration: probe for the column and ADD it if missing,
/// mirroring `ensure_characters_table`'s `pragma_table_info` pattern. Existing
/// rows predate model tracking and are backfilled once to the long-standing
/// default model so they are not left as NULL "unknown". New writes stamp the
/// actual model via `save_gloss` / `save_synopsis` / `update_gloss`.
pub fn ensure_claude_model_columns(conn: &Connection) -> Result<(), rusqlite::Error> {
    // The default model glosses/synopses were created with before this column
    // existed (see default_claude_model in src/config.rs at the time of the
    // backfill). Used only to stamp pre-tracking rows.
    const BACKFILL_MODEL: &str = "claude-opus-4-7";

    for table in ["glosses", "scene_synopses"] {
        let has_col = column_exists(conn, table, "claude_model")?;
        if !has_col {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN claude_model TEXT;"
            ))?;
            // One-time backfill of the rows that existed before the column did.
            conn.execute(
                &format!(
                    "UPDATE {table} SET claude_model = ?1 WHERE claude_model IS NULL"
                ),
                rusqlite::params![BACKFILL_MODEL],
            )?;
        }
    }
    Ok(())
}

/// Ensure `works.vocab_highlight` exists. Per-work flag: `1` colors inline vocab
/// words in the reading card, `0` does not. The column is part of the external
/// lit.db core schema on the user's DB (already present with curated per-work
/// values); this migration only matters on a fresh/other DB that lacks it.
///
/// CRITICAL: this NEVER backfills or resets existing values — the user's
/// 199-work DB carries an intentional split and a blanket UPDATE would destroy
/// it. When the column is absent we ADD it with `DEFAULT 0` so genuinely-new
/// works are off by default; when it is present we do nothing.
pub fn ensure_vocab_highlight_column(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "works", "vocab_highlight")? {
        conn.execute_batch(
            "ALTER TABLE works ADD COLUMN vocab_highlight INTEGER DEFAULT 0;",
        )?;
    }
    Ok(())
}

/// Ensure the bookmarks table exists in the database.
pub fn ensure_bookmarks_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS bookmarks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            work_abbrev TEXT NOT NULL,
            line_mapping_id INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            FOREIGN KEY (work_abbrev) REFERENCES works(abbrev),
            FOREIGN KEY (line_mapping_id) REFERENCES line_mapping(id),
            UNIQUE(work_abbrev, line_mapping_id)
        );"
    )?;
    Ok(())
}

/// Ensure the characters table exists. Keyed by (work_abbrev, speaker) with
/// speaker stored verbatim as it appears in line_mapping.speaker, so the
/// TTS-time lookup joins exactly with no runtime normalization. Carries gender
/// and a nullable `age` used by the age-aware default-voice selection. Rows are
/// loaded by scripts/curate_characters.py, not the app. See the character-gender
/// spec.
pub fn ensure_characters_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Fresh installs get the age column directly.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS characters (
            work_abbrev TEXT NOT NULL,
            speaker     TEXT NOT NULL,
            gender      TEXT NOT NULL,
            age         INTEGER,
            PRIMARY KEY (work_abbrev, speaker)
        );"
    )?;
    // Legacy migration: a pre-age table lacks the `age` column. ADD it (the
    // pragma probe mirrors ensure_gloss_audio_table's column-existence check).
    let has_age = column_exists(conn, "characters", "age")?;
    if !has_age {
        conn.execute_batch("ALTER TABLE characters ADD COLUMN age INTEGER;")?;
    }
    Ok(())
}

/// Ensure the per-gloss voice-set table exists. A gloss can be associated with
/// zero, one, or more voices; `position` gives a stable cycle order. Rows are
/// added/removed via `toggle_gloss_voice`. The FK declares ON DELETE CASCADE,
/// but note SQLite enforces FKs per-connection and `open_db_rw` does not set
/// `PRAGMA foreign_keys = ON`, so in practice deleting a gloss leaves orphaned
/// gloss_voices rows (harmless — they reference a gloss that can no longer be
/// queried). See the per-gloss-voice-set spec.
pub fn ensure_gloss_voices_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gloss_voices (
            gloss_id  INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
            voice_id  TEXT NOT NULL,
            model_id  TEXT NOT NULL,
            position  INTEGER NOT NULL,
            PRIMARY KEY (gloss_id, voice_id)
        );"
    )?;
    Ok(())
}

/// Ensure the voice catalog exists and is seeded with the four narration voice
/// pairs and their age bands. Used by `resolve_default_voice` to pick the
/// default voice by (gender, age). Seeding is idempotent (INSERT OR IGNORE on
/// the (gender, age_min, age_max, role) PK). The user can later add/adjust rows.
/// See docs/superpowers/plans/2026-06-08-age-aware-default-voice-phase2.md.
pub fn ensure_voice_catalog_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    use crate::elevenlabs::*;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS voice_catalog (
            voice_id  TEXT NOT NULL,
            model_id  TEXT NOT NULL,
            gender    TEXT NOT NULL,
            age_min   INTEGER NOT NULL,
            age_max   INTEGER NOT NULL,
            role      TEXT NOT NULL,
            label     TEXT,
            PRIMARY KEY (gender, age_min, age_max, role)
        );"
    )?;
    // Seed the four character voices, each used for BOTH verse and prose (same
    // voice_id in its verse + prose row). INSERT OR IGNORE keeps it idempotent
    // and lets a user-edited row survive a re-run. Benedick (male) / Eleanor
    // (female) are the gender defaults; a male speaker older than Romeo's 15–25
    // band resolves to Benedick via resolve_default_voice's nearest-band step.
    let seed: [(&str, &str, i64, i64, &str, &str); 8] = [
        (ROMEO_VOICE_ID,    "male",   15, 25, "verse", "Romeo — young male verse+prose"),
        (ROMEO_VOICE_ID,    "male",   15, 25, "prose", "Romeo — young male verse+prose"),
        (DEFAULT_MALE_VOICE_ID, "male",   26, 34, "verse", "Benedick — witty male verse+prose"),
        (DEFAULT_MALE_VOICE_ID, "male",   26, 34, "prose", "Benedick — witty male verse+prose"),
        (JULIET_VOICE_ID,   "female", 12, 19, "verse", "Juliet — young female verse+prose"),
        (JULIET_VOICE_ID,   "female", 12, 19, "prose", "Juliet — young female verse+prose"),
        (DEFAULT_FEMALE_VOICE_ID,   "female", 20, 30, "verse", "Eleanor — British female verse+prose"),
        (DEFAULT_FEMALE_VOICE_ID,   "female", 20, 30, "prose", "Eleanor — British female verse+prose"),
    ];
    for (vid, gender, lo, hi, role, label) in seed {
        conn.execute(
            "INSERT OR IGNORE INTO voice_catalog
             (voice_id, model_id, gender, age_min, age_max, role, label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![vid, OP_MODEL_ID, gender, lo, hi, role, label],
        )?;
    }

    // --- Per-work / per-author narrator voice (prose/gloss) ---
    // works.default_voice_id: nullable per-work override. SQLite ADD COLUMN has
    // no IF NOT EXISTS, so guard on PRAGMA table_info to stay idempotent.
    let has_default_voice_col: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('works') WHERE name = 'default_voice_id'")
        .and_then(|mut s| s.query_row([], |_| Ok(true)).optional())
        .unwrap_or(None)
        .unwrap_or(false);
    if !has_default_voice_col {
        // Ignore the error if the works table doesn't exist yet (fresh/test DB).
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
}

/// Ensure the gloss_audio table exists (per-block TTS cache, keyed by kind).
pub fn ensure_gloss_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Fresh installs get the new shape directly.
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS gloss_audio ({GLOSS_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);"
    ))?;

    // Upgrade a legacy table (no `kind` column) by rebuilding to the new shape.
    let has_kind = column_exists(conn, "gloss_audio", "kind")?;
    if !has_kind {
        conn.execute_batch(&format!(
            "BEGIN;
             ALTER TABLE gloss_audio RENAME TO gloss_audio_old;
             CREATE TABLE gloss_audio ({GLOSS_AUDIO_COLUMNS});
             INSERT INTO gloss_audio (id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp)
                SELECT id, gloss_id, 'explication', paragraph_index, audio_path, voice_id, model_id, timestamp
                FROM gloss_audio_old;
             DROP TABLE gloss_audio_old;
             CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
             COMMIT;"
        ))?;
    }

    // Legacy migration 2: the UNIQUE key omits voice_id (pre per-voice cache).
    // Detect by the table's stored DDL still naming the 3-column UNIQUE.
    let old_unique: bool = conn
        .prepare(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='gloss_audio' \
             AND sql LIKE '%UNIQUE(gloss_id, kind, paragraph_index)%' \
             AND sql NOT LIKE '%voice_id)%'",
        )?
        .exists([])?;
    if old_unique {
        conn.execute_batch(&format!(
            "BEGIN;
             ALTER TABLE gloss_audio RENAME TO gloss_audio_old;
             CREATE TABLE gloss_audio ({GLOSS_AUDIO_COLUMNS});
             INSERT INTO gloss_audio (id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp)
                SELECT id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp
                FROM gloss_audio_old;
             DROP TABLE gloss_audio_old;
             CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
             COMMIT;"
        ))?;
    }
    Ok(())
}

/// Ensure the synopsis_audio table exists (lazy CREATE, like gloss_audio — no
/// user_version migration, no SNAPSHOT bump; this is not a LineMap change).
pub fn ensure_synopsis_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS synopsis_audio ({SYNOPSIS_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_synopsis_audio_scene
             ON synopsis_audio(work_abbrev, div1, div2);"
    ))
}

/// Ensure the journal_audio table exists (lazy CREATE, like synopsis_audio — no
/// user_version migration, no SNAPSHOT bump; this is not a LineMap change). Each
/// row caches the synthesized MP3 for one paragraph block of a journal Q&A page,
/// keyed by the `journal_entries.id` so it follows the entry (and is purged with
/// it by `delete_journal_audio`).
pub fn ensure_journal_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS journal_audio ({JOURNAL_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_journal_audio_entry
             ON journal_audio(entry_id);"
    ))
}

/// Generic marker table for one-shot migrations/cleanups that must run
/// exactly once ever, across the DB's whole lifetime — as opposed to the
/// `ensure_*` DDL probes above, which are safe to re-run every launch because
/// `CREATE TABLE IF NOT EXISTS` / column-existence checks are naturally
/// idempotent. A cleanup like `purge_stale_passage_journal_audio` is NOT: its
/// WHERE clause matches a durable data shape (passage-scope entries with
/// source_text), so re-running it on every launch would delete legitimate
/// rows created after the first run, not just the originally-stale ones.
pub fn ensure_one_time_migrations_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS one_time_migrations (
             key         TEXT PRIMARY KEY,
             applied_at  DATETIME DEFAULT CURRENT_TIMESTAMP
         )",
    )
}

/// Marker key claimed by `purge_stale_passage_journal_audio` so the cleanup
/// below runs exactly once across the DB's lifetime. Bump the key (a new date
/// suffix) whenever a change re-shifts passage-entry paragraph indices — the
/// new key re-claims and the purge runs once more:
/// - `-2026-07-20`: verse-source paragraph regrouping (commit 9ac15c5e).
/// - `-2026-07-20b`: the trailing `———` source separator was dropped, shifting
///   every Q&A `paragraph_index` after the source block down by one.
const PURGE_PASSAGE_JOURNAL_AUDIO_KEY: &str = "purge-passage-journal-audio-2026-07-20b";

/// One-time cleanup for changes that re-shape a passage Q&A's paragraph list
/// (see the key's bump log above — first the verse-source regrouping, commit
/// 9ac15c5e, which collapsed the quoted `<segment>`/`<stage>` source lines from
/// one-paragraph-per-line into a single multi-line paragraph; then the
/// dropped `———` separator), shifting every `paragraph_index` at and after
/// the source block for that entry. `journal_audio` has no stored text/hash to validate a
/// cache hit against (see JOURNAL_AUDIO_COLUMNS) — both playback paths
/// (`play_journal_block`, `find_cached_journal_block_audio` in
/// src/input/actions/gloss.rs) serve strictly by (entry_id, paragraph_index,
/// voice_id), so any passage entry with cached audio synthesized before the
/// regrouping would silently replay a stale take at the collided index
/// forever. Passage-scope entries with a non-null `source_text` are exactly
/// the ones whose paragraph layout changed; delete their cached rows so
/// `find_journal_audio` misses and `play_journal_block` re-synthesizes
/// on demand (mirrors `purge_journal_audio`'s delete-and-resynthesize
/// contract, just scoped to the affected entries instead of one).
///
/// The WHERE clause is NOT naturally idempotent: it matches on the durable
/// shape "passage entry with source_text", so a fresh passage Q&A answered
/// (and its audio re-synthesized) after the first run would match the SAME
/// clause on the next launch and get deleted again — silently re-billing
/// ElevenLabs synthesis forever. This fn claims a persistent marker in
/// `one_time_migrations` (caller must `ensure_one_time_migrations_table`
/// first) before deleting anything; if the marker was already claimed by a
/// prior run, it returns `Ok(0)` without touching `journal_audio` at all.
pub fn purge_stale_passage_journal_audio(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let claimed = conn.execute(
        "INSERT OR IGNORE INTO one_time_migrations (key) VALUES (?1)",
        [PURGE_PASSAGE_JOURNAL_AUDIO_KEY],
    )?;
    if claimed == 0 {
        // Marker already present from a prior run — already applied, skip.
        return Ok(0);
    }
    conn.execute(
        "DELETE FROM journal_audio
         WHERE entry_id IN (
             SELECT id FROM journal_entries
             WHERE scope = 'passage' AND source_text IS NOT NULL
         )",
        [],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal in-memory `journal_entries` shape (only the columns the purge
    /// query reads) so the test doesn't depend on the full production DDL in
    /// db/journal.rs.
    fn make_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE journal_entries (
                id          INTEGER PRIMARY KEY,
                scope       TEXT NOT NULL DEFAULT 'scene',
                source_text TEXT
            );",
        )
        .unwrap();
        ensure_journal_audio_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();
        conn
    }

    fn insert_entry(conn: &Connection, id: i64, scope: &str, source_text: Option<&str>) {
        conn.execute(
            "INSERT INTO journal_entries (id, scope, source_text) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, scope, source_text],
        )
        .unwrap();
    }

    fn insert_audio(conn: &Connection, entry_id: i64, paragraph_index: i64) {
        conn.execute(
            "INSERT INTO journal_audio (entry_id, paragraph_index, audio_path, voice_id, model_id)
             VALUES (?1, ?2, '/tmp/x.mp3', 'voice1', 'eleven_v3')",
            rusqlite::params![entry_id, paragraph_index],
        )
        .unwrap();
    }

    #[test]
    fn purge_deletes_only_passage_entries_with_source_text() {
        let conn = make_test_db();

        // Entry 1: passage scope with quoted verse source — affected.
        insert_entry(&conn, 1, "passage", Some("<segment>a</segment>\n<segment>b</segment>"));
        insert_audio(&conn, 1, 0);
        insert_audio(&conn, 1, 1);

        // Entry 2: scene-scope Q&A — never had a source block, unaffected.
        insert_entry(&conn, 2, "scene", None);
        insert_audio(&conn, 2, 0);

        // Entry 3: passage scope but no source_text (e.g. a non-verse passage
        // note) — paragraph layout never shifted, unaffected.
        insert_entry(&conn, 3, "passage", None);
        insert_audio(&conn, 3, 0);

        let deleted = purge_stale_passage_journal_audio(&conn).unwrap();
        assert_eq!(deleted, 2);

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM journal_audio", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2);

        let entry1_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journal_audio WHERE entry_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(entry1_rows, 0);
    }

    #[test]
    fn purge_is_marker_guarded_and_spares_fresh_rows_after_first_run() {
        let conn = make_test_db();
        insert_entry(&conn, 1, "passage", Some("<segment>a</segment>"));
        insert_audio(&conn, 1, 0);

        assert_eq!(purge_stale_passage_journal_audio(&conn).unwrap(), 1);

        // A fresh passage Q&A is answered (and its audio synthesized) AFTER
        // the first run — entry 2 matches the exact same WHERE clause as
        // entry 1 did (passage scope, non-null source_text).
        insert_entry(&conn, 2, "passage", Some("<segment>c</segment>"));
        insert_audio(&conn, 2, 0);

        // Second run must short-circuit on the marker (0 deleted), NOT
        // re-run the DELETE — otherwise it would match and wipe entry 2's
        // brand-new audio, re-billing synthesis on every launch.
        assert_eq!(purge_stale_passage_journal_audio(&conn).unwrap(), 0);

        let entry2_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journal_audio WHERE entry_id = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(entry2_rows, 1, "fresh audio inserted after the first run must survive");
    }

    #[test]
    fn purge_on_missing_journal_entries_table_is_a_query_error_not_a_panic() {
        // ensure_journal_audio_table alone (no journal_entries table) mirrors
        // a theoretical DB where journal_audio was created before journal
        // entries ever existed. The caller (`BOOKMARKS_INIT`) already treats
        // this fn's Result as `let _ =`, matching every other ensure_* call.
        let conn = Connection::open_in_memory().unwrap();
        ensure_journal_audio_table(&conn).unwrap();
        ensure_one_time_migrations_table(&conn).unwrap();
        assert!(purge_stale_passage_journal_audio(&conn).is_err());
    }
}
