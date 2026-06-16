use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::HashMap;

use super::line_types;
use super::models::{Line, MediaItem, TimeRange, Timestamp, Work, WorkSummary};
use crate::scansion::{LineScansion, ScanSyllable};

fn db_path() -> String {
    // LIT_DB_PATH lets an isolated run (e.g. the headless nav-fuzz) read its own
    // private copy of the database instead of the shared lit.db, so it doesn't
    // contend with a live `cargo run` session's SQLite file locks.
    if let Ok(p) = std::env::var("LIT_DB_PATH") {
        if !p.is_empty() {
            return p;
        }
    }
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    format!("{}/utono/litdb/data/lit.db", home)
}

pub fn open_db() -> Result<Connection, rusqlite::Error> {
    Connection::open_with_flags(db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn list_works(conn: &Connection) -> Result<Vec<WorkSummary>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT abbrev, title, author, work_type FROM works ORDER BY title")?;
    let rows = stmt.query_map([], |row| {
        Ok(WorkSummary {
            abbrev: row.get(0)?,
            title: row.get(1)?,
            author: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            work_type: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Load Wright scansion for every scanned line of `abbrev`, keyed by
/// `line_mapping.id`. Lines with no `line_meter` row are absent from the map
/// (rendered plain by the caller). Mirrors `load_work`'s query idiom.
pub fn load_scansion_for_work(
    conn: &Connection,
    abbrev: &str,
) -> Result<HashMap<i64, LineScansion>, rusqlite::Error> {
    // 1. line_meter rows for this work's lines.
    let mut meter_stmt = conn.prepare(
        "SELECT lm.line_id, lm.line_type, lm.caesura_after \
         FROM line_meter lm JOIN line_mapping m ON lm.line_id = m.id \
         WHERE m.work_abbrev = ?1",
    )?;
    let mut map: HashMap<i64, LineScansion> = HashMap::new();
    let meter_rows = meter_stmt.query_map([abbrev], |row| {
        let line_id: i64 = row.get(0)?;
        let line_type: String = row.get(1)?;
        let caesura_after: Option<i32> = row.get(2)?;
        Ok((line_id, line_type, caesura_after))
    })?;
    for r in meter_rows {
        let (line_id, line_type, caesura_after) = r?;
        map.insert(line_id, LineScansion { line_type, caesura_after, syllables: Vec::new() });
    }

    // 2. syllable_scan rows, appended in position order to their line.
    let mut syl_stmt = conn.prepare(
        "SELECT s.line_id, s.surface, s.ictus, s.is_extrametrical \
         FROM syllable_scan s JOIN line_mapping m ON s.line_id = m.id \
         WHERE m.work_abbrev = ?1 ORDER BY s.line_id, s.position",
    )?;
    let syl_rows = syl_stmt.query_map([abbrev], |row| {
        let line_id: i64 = row.get(0)?;
        let surface: Option<String> = row.get(1)?;
        let ictus: i64 = row.get(2)?;
        let is_extra: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
        Ok((line_id, surface.unwrap_or_default(), ictus as i8, is_extra != 0))
    })?;
    for r in syl_rows {
        let (line_id, surface, ictus, is_extrametrical) = r?;
        if let Some(ls) = map.get_mut(&line_id) {
            ls.syllables.push(ScanSyllable { surface, ictus, is_extrametrical });
        }
    }
    Ok(map)
}

pub fn load_work(conn: &Connection, abbrev: &str) -> Result<Work, rusqlite::Error> {
    // 1. Get work metadata
    let (title, author, work_type): (String, String, String) = conn.query_row(
        "SELECT title, COALESCE(author, ''), work_type FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    // text_file column may not exist yet (manual migration) — graceful fallback
    let text_file: Option<String> = conn.query_row(
        "SELECT text_file FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| row.get(0),
    ).unwrap_or(None);

    let is_prose = line_types::is_prose_work(&work_type);

    // 2. Load all lines
    let mut line_stmt = conn.prepare(
        "SELECT id, canonical_text, normalized_text, speaker, div1, div2, line_in_div \
         FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div",
    )?;
    let lines: Vec<Line> = line_stmt
        .query_map([abbrev], |row| {
            let text: String = row.get(1)?;
            let normalized: String = row.get(2)?;
            let speaker: Option<String> = row.get(3)?;
            let div1: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let div2: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
            let line_in_div: i64 = row.get(6)?;
            let citation = format!("{}.{}.{}.{}", abbrev, div1, div2, line_in_div);
            Ok(Line {
                id: row.get(0)?,
                citation,
                is_dialogue: line_types::is_dialogue(&text, is_prose),
                text,
                normalized,
                speaker,
                timestamp: None,
                div1,
                div2,
                line_in_div,
                is_chapter: false,
                is_spoken: None,
            })
        })?
        .collect::<Result<_, _>>()?;

    // 3. Load timestamps
    let mut ts_stmt = conn.prepare(
        "SELECT lt.line_mapping_id, lt.start_time, lt.end_time, lt.media_id, \
         lt.sentence_start_time, lt.source, lt.is_chapter \
         FROM line_timestamps lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let timestamps: Vec<Timestamp> = ts_stmt
        .query_map([abbrev], |row| {
            let source: String = row.get::<_, Option<String>>(5)?.unwrap_or_default();
            Ok(Timestamp {
                line_id: row.get(0)?,
                start: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                end: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                media_id: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                sentence_start: row.get::<_, Option<f64>>(4)?,
                is_manual: source == "manual",
                is_chapter: row.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
            })
        })?
        .collect::<Result<_, _>>()?;

    // 4. Load media paths (needed to determine active media_id for timestamp filtering)
    let mut media_stmt = conn.prepare(
        "SELECT mf.id, mf.path FROM media_files mf \
         JOIN work_media_associations wma ON wma.media_id = mf.id \
         WHERE wma.work_abbrev = ?1 \
         ORDER BY wma.priority DESC",
    )?;
    let media_rows: Vec<(i64, String)> = media_stmt
        .query_map([abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let media_id = media_rows.first().map(|(id, _)| *id);
    let media_ids: Vec<i64> = media_rows.iter().map(|(id, _)| *id).collect();
    let media_paths: Vec<String> = media_rows.into_iter().map(|(_, path)| path).collect();

    // 5. Build timestamp lookup: line_id -> TimeRange (filtered by active media_id)
    let mut ts_map: HashMap<i64, TimeRange> = HashMap::new();
    for ts in &timestamps {
        if media_id.map_or(true, |mid| ts.media_id == mid) {
            ts_map.entry(ts.line_id).or_insert(TimeRange {
                start: ts.start,
                end: ts.end,
                sentence_start: ts.sentence_start,
                is_manual: ts.is_manual,
            });
        }
    }

    // 5b. Build chapter lookup from already-loaded timestamps (no extra DB query)
    let mut chapter_map: HashMap<i64, bool> = HashMap::new();
    if let Some(mid) = media_id {
        for ts in &timestamps {
            if ts.media_id == mid && ts.is_chapter {
                chapter_map.insert(ts.line_id, true);
            }
        }
    }

    // 5c. Load spoken status for the active media
    let mut spoken_map: HashMap<i64, bool> = HashMap::new();
    if let Some(mid) = media_id {
        let mut spoken_stmt = conn.prepare(
            "SELECT line_mapping_id, is_spoken FROM line_spoken_status WHERE media_id = ?1",
        )?;
        let rows = spoken_stmt.query_map([mid], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? != 0))
        })?;
        for row in rows {
            let (lm_id, spoken) = row?;
            spoken_map.insert(lm_id, spoken);
        }
    }

    // 6. Attach timestamps and spoken status to lines
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|mut line| {
            line.timestamp = ts_map.get(&line.id).copied();
            line.is_chapter = chapter_map.contains_key(&line.id);
            line.is_spoken = spoken_map.get(&line.id).copied();
            line
        })
        .collect();

    Ok(Work {
        abbrev: abbrev.to_string(),
        title,
        author,
        work_type,
        text_file,
        lines,
        timestamps,
        media_paths,
        media_ids,
        media_id,
    })
}

/// Load translations for a work, keyed by line_mapping.id.
///
/// For `-Amb` (Ambrose edition) works, translations are stored against the
/// base edition's line_mapping rows. If the direct query returns nothing,
/// fall back to matching Ambrose lines to base-edition lines by
/// (div1, div2, normalized_text) and key the translations to the Ambrose
/// line_mapping.id so the app's existing lookup by line.id works unchanged.
pub fn load_translations(
    conn: &Connection,
    abbrev: &str,
) -> Result<HashMap<i64, String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT lm.id, lt.translation \
         FROM line_translations lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([abbrev], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, translation) = row?;
        map.insert(id, translation);
    }

    if map.is_empty() {
        if let Some(base) = abbrev.strip_suffix("-Amb") {
            let mut stmt = conn.prepare(
                "SELECT a.id, MIN(lt.translation) \
                 FROM line_mapping a \
                 JOIN line_mapping b \
                   ON b.work_abbrev = ?2 \
                  AND b.div1 = a.div1 \
                  AND b.div2 = a.div2 \
                  AND b.normalized_text = a.normalized_text \
                 JOIN line_translations lt ON lt.line_mapping_id = b.id \
                 WHERE a.work_abbrev = ?1 \
                 GROUP BY a.id",
            )?;
            let rows = stmt.query_map([abbrev, base], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, translation) = row?;
                map.insert(id, translation);
            }
        }
    }

    Ok(map)
}

pub fn load_synopses(conn: &Connection, work_abbrev: &str) -> HashMap<(i64, i64), String> {
    let mut map = HashMap::new();
    let mut stmt = match conn.prepare(
        "SELECT div1, div2, synopsis FROM scene_synopses WHERE work_abbrev = ?1"
    ) {
        Ok(s) => s,
        Err(_) => return map,
    };
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    });
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            map.insert((row.0, row.1), row.2);
        }
    }
    map
}

/// Update (or insert) the synopsis text for one scene. Used by the `A` amend
/// flow in the synopsis overlay; the UNIQUE(work_abbrev, div1, div2) constraint
/// makes this an upsert. Requires a read-write connection (open_db_rw).
pub fn save_synopsis(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    synopsis: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO scene_synopses (work_abbrev, div1, div2, synopsis, claude_model) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(work_abbrev, div1, div2) DO UPDATE SET \
             synopsis = excluded.synopsis, claude_model = excluded.claude_model",
        rusqlite::params![work_abbrev, div1, div2, synopsis, claude_model],
    )?;
    Ok(())
}

/// Restore a synopsis's text WITHOUT changing its recorded `claude_model`. Used
/// by the `U` undo path, which reverts to the pre-amend text — that earlier text
/// was authored by whatever model the row already records, so undo must not
/// overwrite the model the way a fresh amend (save_synopsis) does. If the row
/// doesn't exist yet (no prior amend persisted), this is a no-op UPDATE.
pub fn restore_synopsis_text(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    synopsis: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE scene_synopses SET synopsis = ?4 \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3",
        rusqlite::params![work_abbrev, div1, div2, synopsis],
    )?;
    Ok(())
}

/// Load all vocab words + variants for matching against buffer text.
/// Returns a HashSet of lowercase words (base words + variants).
pub fn load_vocab_words(
    conn: &Connection,
    _work_abbrev: &str,
) -> Result<std::collections::HashSet<String>, rusqlite::Error> {
    let mut words = std::collections::HashSet::new();

    let mut stmt = conn.prepare("SELECT LOWER(word) FROM vocab_words")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        words.insert(row?);
    }

    let mut stmt = conn.prepare("SELECT LOWER(v.variant) FROM vocab_word_variants v")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        words.insert(row?);
    }

    Ok(words)
}

/// Load definition and sources for a vocab word.
pub fn load_vocab_definition(
    conn: &Connection,
    word: &str,
) -> Option<(String, Vec<String>)> {
    let result: Result<(String, Option<String>), _> = conn.query_row(
        "SELECT w.definition, GROUP_CONCAT(s.source) \
         FROM vocab_words w \
         LEFT JOIN vocab_word_sources s ON s.word_id = w.id \
         WHERE LOWER(w.word) = ?1 \
         GROUP BY w.id",
        [word.to_lowercase()],
        |row| Ok((
            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            row.get::<_, Option<String>>(1)?,
        )),
    );
    match result {
        Ok((def, sources_str)) => {
            let sources: Vec<String> = sources_str
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default();
            if def.is_empty() { None } else { Some((def, sources)) }
        }
        Err(_) => None,
    }
}

#[allow(dead_code)]
pub struct VocabEtymology {
    pub prefix: Option<String>,
    pub prefix_gloss: Option<String>,
    pub root: Option<String>,
    pub root_gloss: Option<String>,
    pub suffix: Option<String>,
    pub suffix_gloss: Option<String>,
}

/// Load etymology breakdown from vocab_rhetoric.
pub fn load_vocab_etymology(
    conn: &Connection,
    word: &str,
) -> Option<VocabEtymology> {
    conn.query_row(
        "SELECT vr.prefix, vr.prefix_gloss, vr.root, \
         vr.root_gloss, vr.suffix, vr.suffix_gloss \
         FROM vocab_rhetoric vr \
         JOIN vocab_words vw ON vr.word_id = vw.id \
         WHERE LOWER(vw.word) = ?1",
        [word.to_lowercase()],
        |row| Ok(VocabEtymology {
            prefix: row.get::<_, Option<String>>(0)?,
            prefix_gloss: row.get::<_, Option<String>>(1)?,
            root: row.get::<_, Option<String>>(2)?,
            root_gloss: row.get::<_, Option<String>>(3)?,
            suffix: row.get::<_, Option<String>>(4)?,
            suffix_gloss: row.get::<_, Option<String>>(5)?,
        }),
    ).ok()
}

/// Load a vocab-word gloss for a word near a given line.
pub fn load_vocab_gloss(
    conn: &Connection,
    word: &str,
    work_abbrev: &str,
    line_citation: &str,
) -> Option<String> {
    let word_id: i64 = conn.query_row(
        "SELECT id FROM vocab_words WHERE LOWER(word) = ?1",
        [word.to_lowercase()],
        |row| row.get(0),
    ).ok()?;

    conn.query_row(
        "SELECT g.gloss_text FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE g.gloss_type = 'vocab-word' \
         AND g.word_id = ?1 \
         AND p.work_abbrev = ?2 \
         AND p.start_citation <= ?3 \
         AND p.end_citation >= ?3",
        rusqlite::params![word_id, work_abbrev, line_citation],
        |row| row.get::<_, String>(0),
    ).ok()
}

/// List all vocab words found in a work's text, with occurrence counts.
pub fn load_vocab_word_list(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT canonical_text FROM line_mapping \
         ORDER BY div1, div2, line_in_div"
    )?;
    let lines: Vec<String> = stmt.query_map([], |row| {
        row.get::<_, String>(0)
    })?.collect::<Result<_, _>>()?;

    let vocab = load_vocab_words(conn, work_abbrev)?;

    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in &lines {
        for token in line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}') {
            let lower = token.to_lowercase();
            if vocab.contains(&lower) {
                *counts.entry(lower).or_insert(0) += 1;
            }
        }
    }

    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

pub fn list_media_for_work(
    conn: &Connection,
    abbrev: &str,
) -> Result<Vec<MediaItem>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT mf.id, mf.path, wma.display_name, wma.priority \
         FROM media_files mf \
         JOIN work_media_associations wma ON wma.media_id = mf.id \
         WHERE wma.work_abbrev = ?1 \
         ORDER BY wma.priority DESC",
    )?;
    let rows = stmt.query_map([abbrev], |row| {
        Ok(MediaItem {
            media_id: row.get(0)?,
            path: row.get(1)?,
            display_name: row.get(2)?,
            priority: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn set_media_priority(
    conn: &Connection,
    abbrev: &str,
    media_id: i64,
) -> Result<(), rusqlite::Error> {
    // Find the current max priority for this work
    let max_priority: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(priority), 10) FROM work_media_associations WHERE work_abbrev = ?1",
            [abbrev],
            |row| row.get(0),
        )?;
    // Set all other media for this work to priority 10
    conn.execute(
        "UPDATE work_media_associations SET priority = 10 WHERE work_abbrev = ?1",
        [abbrev],
    )?;
    // Set the selected one to max + 10 (or at least 20)
    let new_priority = (max_priority + 10).max(20);
    conn.execute(
        "UPDATE work_media_associations SET priority = ?1 WHERE work_abbrev = ?2 AND media_id = ?3",
        rusqlite::params![new_priority, abbrev, media_id],
    )?;
    Ok(())
}

pub fn open_db_rw() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

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
        let has_col: bool = conn
            .prepare(&format!(
                "SELECT 1 FROM pragma_table_info('{table}') WHERE name = 'claude_model'"
            ))?
            .exists([])?;
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
    let has_age: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('characters') WHERE name = 'age'")?
        .exists([])?;
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
    // and lets a user-edited row survive a re-run. Benedick (male) / Beatrice
    // (female) are the gender defaults; a male speaker older than Romeo's 15–25
    // band resolves to Benedick via resolve_default_voice's nearest-band step.
    let seed: [(&str, &str, i64, i64, &str, &str); 8] = [
        (ROMEO_VOICE_ID,    "male",   15, 25, "verse", "Romeo — young male verse+prose"),
        (ROMEO_VOICE_ID,    "male",   15, 25, "prose", "Romeo — young male verse+prose"),
        (BENEDICK_VOICE_ID, "male",   26, 34, "verse", "Benedick — witty male verse+prose"),
        (BENEDICK_VOICE_ID, "male",   26, 34, "prose", "Benedick — witty male verse+prose"),
        (JULIET_VOICE_ID,   "female", 12, 19, "verse", "Juliet — young female verse+prose"),
        (JULIET_VOICE_ID,   "female", 12, 19, "prose", "Juliet — young female verse+prose"),
        (BEATRICE_VOICE_ID, "female", 20, 30, "verse", "Beatrice — female verse+prose"),
        (BEATRICE_VOICE_ID, "female", 20, 30, "prose", "Beatrice — female verse+prose"),
    ];
    for (vid, gender, lo, hi, role, label) in seed {
        conn.execute(
            "INSERT OR IGNORE INTO voice_catalog
             (voice_id, model_id, gender, age_min, age_max, role, label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![vid, OP_MODEL_ID, gender, lo, hi, role, label],
        )?;
    }
    Ok(())
}

/// The voices associated with a gloss, ordered by `position` (cycle order).
pub fn get_gloss_voices(conn: &Connection, gloss_id: i64) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT voice_id, model_id FROM gloss_voices WHERE gloss_id = ?1 ORDER BY position",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![gloss_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}

/// Toggle a voice's membership in a gloss's set. Returns `true` if it was ADDED
/// (appended at the next position), `false` if it was REMOVED.
pub fn toggle_gloss_voice(
    conn: &Connection,
    gloss_id: i64,
    voice_id: &str,
    model_id: &str,
) -> bool {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM gloss_voices WHERE gloss_id = ?1 AND voice_id = ?2",
            rusqlite::params![gloss_id, voice_id],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        let _ = conn.execute(
            "DELETE FROM gloss_voices WHERE gloss_id = ?1 AND voice_id = ?2",
            rusqlite::params![gloss_id, voice_id],
        );
        false
    } else {
        let next_pos: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM gloss_voices WHERE gloss_id = ?1",
                rusqlite::params![gloss_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "INSERT INTO gloss_voices (gloss_id, voice_id, model_id, position) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![gloss_id, voice_id, model_id, next_pos],
        );
        true
    }
}

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

/// Ensure the gloss_audio table exists (per-block TTS cache, keyed by kind).
pub fn ensure_gloss_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Fresh installs get the new shape directly.
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS gloss_audio ({GLOSS_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);"
    ))?;

    // Upgrade a legacy table (no `kind` column) by rebuilding to the new shape.
    let has_kind: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('gloss_audio') WHERE name = 'kind'")?
        .exists([])?;
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

/// Return the cached audio path for a gloss block in a SPECIFIC voice, if any.
pub fn find_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM gloss_audio
         WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3 AND voice_id = ?4",
        rusqlite::params![gloss_id, kind, index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Default age used when a character has no curated age (NULL).
const DEFAULT_AGE: i64 = 40;

/// Read (Gender, age) for a speaker. Multi-speaker (comma) / no row → (Unknown,
/// None); a real DB error is logged and also yields (Unknown, None). Generalizes
/// get_character_gender to also pull age.
fn get_character_gender_age(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
) -> (crate::elevenlabs::Gender, Option<i64>) {
    if speaker.contains(',') {
        return (crate::elevenlabs::Gender::Unknown, None);
    }
    let row: Result<(String, Option<i64>), _> = conn.query_row(
        "SELECT gender, age FROM characters WHERE work_abbrev = ?1 AND speaker = ?2",
        rusqlite::params![work_abbrev, speaker],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    match row {
        Ok((g, a)) => (crate::elevenlabs::Gender::from_db(&g), a),
        Err(rusqlite::Error::QueryReturnedNoRows) => (crate::elevenlabs::Gender::Unknown, None),
        Err(e) => {
            crate::log_fmt!(
                "get_character_gender_age: unexpected DB error for {}/{}: {}",
                work_abbrev, speaker, e
            );
            (crate::elevenlabs::Gender::Unknown, None)
        }
    }
}

/// Pick the default (voice_id, model_id) for a speaker by (gender, age) from the
/// voice_catalog: the narrowest band CONTAINING the age, else the NEAREST
/// same-gender band, else the legacy `voice_for` constants. `is_verse` selects
/// the verse/prose role. Unknown/neutral gender → male; missing age → DEFAULT_AGE.
pub fn resolve_default_voice(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
    is_verse: bool,
) -> (String, String) {
    // All prose (explication) is read by Beatrice — one consistent narrator for
    // the modern-English commentary, regardless of the speaker's gender/age.
    // (Verse still picks by (gender, age) below; a per-gloss associated voice
    // still overrides this default at the call site in play_block_tts.)
    if !is_verse {
        return (
            crate::elevenlabs::BEATRICE_VOICE_ID.to_string(),
            crate::elevenlabs::OP_MODEL_ID.to_string(),
        );
    }

    let (gender, age_opt) = get_character_gender_age(conn, work_abbrev, speaker);
    // Catalog gender is 'male' | 'female'; everything not Female → male.
    let cat_gender = if gender == crate::elevenlabs::Gender::Female { "female" } else { "male" };
    let age = age_opt.unwrap_or(DEFAULT_AGE);
    let role = if is_verse { "verse" } else { "prose" };

    // 1. Containment: narrowest band that contains `age`.
    let contained: Option<(String, String)> = conn
        .query_row(
            "SELECT voice_id, model_id FROM voice_catalog
             WHERE gender = ?1 AND role = ?2 AND ?3 BETWEEN age_min AND age_max
             ORDER BY (age_max - age_min) ASC LIMIT 1",
            rusqlite::params![cat_gender, role, age],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!(
                "resolve_default_voice: containment query error for {}/{}: {}",
                work_abbrev, speaker, e
            );
            None
        });
    if let Some(hit) = contained {
        return hit;
    }

    // 2. Nearest same-gender/role band: clamped distance from `age` to the band's
    //    [age_min, age_max] interval — below-band uses (age_min - age), above-band
    //    uses (age - age_max), inside-band is 0 (those are already caught by step 1).
    let nearest: Option<(String, String)> = conn
        .query_row(
            "SELECT voice_id, model_id FROM voice_catalog
             WHERE gender = ?1 AND role = ?2
             ORDER BY MAX(0, age_min - ?3) + MAX(0, ?3 - age_max) ASC LIMIT 1",
            rusqlite::params![cat_gender, role, age],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!(
                "resolve_default_voice: nearest query error for {}/{}: {}",
                work_abbrev, speaker, e
            );
            None
        });
    if let Some(hit) = nearest {
        return hit;
    }

    // 3. Last resort (catalog empty / no same-gender voice — unreachable given
    //    the seed rows): the legacy gender-only constants.
    let (v, m) = crate::elevenlabs::voice_for(gender, is_verse);
    (v.to_string(), m.to_string())
}

/// Insert or replace the audio path for a gloss block in a specific voice.
pub fn save_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(gloss_id, kind, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![gloss_id, kind, index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}

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

/// Ensure the synopsis_audio table exists (lazy CREATE, like gloss_audio — no
/// user_version migration, no SNAPSHOT bump; this is not a LineMap change).
pub fn ensure_synopsis_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS synopsis_audio ({SYNOPSIS_AUDIO_COLUMNS});
         CREATE INDEX IF NOT EXISTS idx_synopsis_audio_scene
             ON synopsis_audio(work_abbrev, div1, div2);"
    ))
}

/// Cached MP3 path for a synopsis paragraph in a specific voice, if any.
pub fn find_synopsis_audio(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    paragraph_index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM synopsis_audio
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3
           AND paragraph_index = ?4 AND voice_id = ?5",
        rusqlite::params![work_abbrev, div1, div2, paragraph_index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Upsert a cached synopsis-paragraph MP3 path.
pub fn save_synopsis_audio(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    paragraph_index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO synopsis_audio
            (work_abbrev, div1, div2, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(work_abbrev, div1, div2, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![work_abbrev, div1, div2, paragraph_index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}

/// Delete all cached audio rows for a gloss (call when the gloss is removed,
/// since SQLite FK cascade is not enabled app-wide). Returns the number of rows
/// removed, so a caller can report exactly how many cached takes were purged.
pub fn delete_gloss_audio(conn: &Connection, gloss_id: i64) -> Result<usize, rusqlite::Error> {
    let n = conn.execute(
        "DELETE FROM gloss_audio WHERE gloss_id = ?1",
        rusqlite::params![gloss_id],
    )?;
    Ok(n)
}

/// Delete the cached audio rows for ONE block of a gloss (all voices) and return
/// their `audio_path`s so the caller can remove the files. Scoped, unlike
/// `delete_gloss_audio` which clears a whole gloss. Used by the fix-IPA flow to
/// invalidate just the corrected source block before re-synthesis.
pub fn delete_gloss_audio_block(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
) -> Result<Vec<String>, rusqlite::Error> {
    let paths: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT audio_path FROM gloss_audio
             WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![gloss_id, kind, index], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    conn.execute(
        "DELETE FROM gloss_audio WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        rusqlite::params![gloss_id, kind, index],
    )?;
    Ok(paths)
}

/// Load all bookmarked line_mapping_ids for a work.
pub fn load_bookmarks(conn: &Connection, work_abbrev: &str) -> Result<Vec<i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT line_mapping_id FROM bookmarks WHERE work_abbrev = ?1"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| row.get::<_, i64>(0))?;
    rows.collect()
}

/// Toggle a bookmark on a line. Returns true if added, false if removed.
pub fn toggle_bookmark(
    conn: &Connection,
    work_abbrev: &str,
    line_mapping_id: i64,
) -> Result<bool, rusqlite::Error> {
    let existing: Option<i64> = conn.query_row(
        "SELECT id FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_mapping_id],
        |row| row.get(0),
    ).optional()?;

    if let Some(id) = existing {
        conn.execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO bookmarks (work_abbrev, line_mapping_id) VALUES (?1, ?2)",
            rusqlite::params![work_abbrev, line_mapping_id],
        )?;
        Ok(true)
    }
}

/// Get the line_mapping_id of the most recently created bookmark for a work.
pub fn most_recent_bookmark(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT line_mapping_id FROM bookmarks WHERE work_abbrev = ?1 ORDER BY created_at DESC LIMIT 1",
        [work_abbrev],
        |row| row.get(0),
    ).optional()
}

/// Load bookmarks with line text for the picker, sorted by most recent first.
pub fn load_bookmarks_with_details(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<super::models::BookmarkItem>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT b.line_mapping_id, lm.canonical_text, lm.speaker, \
                lm.div1, lm.div2, lm.line_in_div \
         FROM bookmarks b \
         JOIN line_mapping lm ON b.line_mapping_id = lm.id \
         WHERE b.work_abbrev = ?1 \
         ORDER BY lm.div1, lm.div2, lm.line_in_div"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        let div1: i64 = row.get(3)?;
        let div2: i64 = row.get(4)?;
        let line_in_div: i64 = row.get(5)?;
        let citation = format!("{}.{}.{}.{}", work_abbrev, div1, div2, line_in_div);
        Ok(super::models::BookmarkItem {
            line_mapping_id: row.get(0)?,
            line_text: row.get(1)?,
            speaker: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            citation,
        })
    })?;
    rows.collect()
}

/// Delete a bookmark by work and line_mapping_id.
pub fn delete_bookmark(
    conn: &Connection,
    work_abbrev: &str,
    line_mapping_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_mapping_id],
    )?;
    Ok(())
}

pub fn upsert_start_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source) \
         VALUES (?1, ?2, ?3, ?4, 'manual') \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET start_time = ?4, source = 'manual', updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    Ok(())
}

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

pub fn upsert_chapter(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: f64,
) -> Result<bool, rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source, is_chapter) \
         VALUES (?1, ?2, ?3, ?4, 'manual', 1) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET is_chapter = CASE WHEN is_chapter = 1 THEN 0 ELSE 1 END, source = 'manual', updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    let new_val: bool = conn.query_row(
        "SELECT is_chapter FROM line_timestamps WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id],
        |row| row.get(0),
    )?;
    Ok(new_val)
}

pub fn update_end_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    end_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE line_timestamps SET end_time = ?3, updated_at = CURRENT_TIMESTAMP \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id, end_time],
    )?;
    Ok(())
}

pub fn delete_timestamp(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM line_timestamps WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id],
    )?;
    Ok(())
}

pub fn get_timestamp_snapshot(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Result<Option<crate::input::timestamps::TimestampSnapshot>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT citation, start_time, end_time, is_chapter \
         FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
    )?;
    let result = stmt.query_row(rusqlite::params![line_mapping_id, media_id], |row| {
        Ok(crate::input::timestamps::TimestampSnapshot {
            citation: row.get(0)?,
            start_time: row.get(1)?,
            end_time: row.get(2)?,
            is_chapter: row.get::<_, bool>(3).unwrap_or(false),
        })
    });
    match result {
        Ok(snap) => Ok(Some(snap)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn restore_timestamp(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: Option<f64>,
    end_time: Option<f64>,
    is_chapter: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, end_time, source, is_chapter) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET start_time = ?4, end_time = ?5, is_chapter = ?6, updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time, end_time, is_chapter],
    )?;
    Ok(())
}

/// Merge multiple lines into one. Updates the first line's text and deletes the rest.

/// Replace a set of lines with new text lines. Updates the first line,
/// deletes excess old lines, or inserts new lines if output has more.
/// `old_ids`: IDs of original lines (ordered).
/// `new_texts`: replacement texts (ordered).
pub fn replace_lines(
    conn: &Connection,
    work_abbrev: &str,
    old_ids: &[i64],
    new_texts: &[String],
) -> Result<(), rusqlite::Error> {
    if old_ids.is_empty() || new_texts.is_empty() {
        return Ok(());
    }

    // Update existing lines where we have both old and new
    let update_count = old_ids.len().min(new_texts.len());
    for i in 0..update_count {
        conn.execute(
            "UPDATE line_mapping SET canonical_text = ?2, normalized_text = ?2 WHERE id = ?1",
            rusqlite::params![old_ids[i], new_texts[i]],
        )?;
    }

    // Delete excess old lines
    for &id in &old_ids[update_count..] {
        conn.execute("DELETE FROM line_mapping WHERE id = ?1", [id])?;
    }

    // Insert new lines if output has more than old
    if new_texts.len() > old_ids.len() {
        // Get div info from the first old line to use for new inserts
        let (div1, div2, base_line_in_div): (i64, i64, i64) = conn.query_row(
            "SELECT div1, div2, line_in_div FROM line_mapping WHERE id = ?1",
            [old_ids[0]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        for (i, text) in new_texts[old_ids.len()..].iter().enumerate() {
            let new_line_in_div = base_line_in_div + (old_ids.len() + i) as i64;
            conn.execute(
                "INSERT INTO line_mapping (work_abbrev, canonical_text, normalized_text, div1, div2, line_in_div) \
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
                rusqlite::params![work_abbrev, text, div1, div2, new_line_in_div],
            )?;
        }
    }

    Ok(())
}


#[derive(Debug, Clone)]
pub struct SavedGloss {
    pub gloss_id: i64,
    pub passage_id: i64,
    pub gloss_text: String,
    pub timestamp: String,
    pub gloss_type: String,
}

pub fn find_existing_gloss(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
    gloss_type: &str,
) -> Result<Option<SavedGloss>, rusqlite::Error> {
    let gt = gloss_type.to_string();
    conn.query_row(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND p.end_citation = ?3 \
           AND g.gloss_type = ?4 \
         ORDER BY g.timestamp DESC \
         LIMIT 1",
        rusqlite::params![work_abbrev, start_citation, end_citation, gloss_type],
        |row| {
            Ok(SavedGloss {
                gloss_id: row.get(0)?,
                gloss_text: row.get(1)?,
                timestamp: row.get(2)?,
                passage_id: row.get(3)?,
                gloss_type: gt.clone(),
            })
        },
    )
    .optional()
}

pub fn find_all_glosses(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
    gloss_types: &[&str],
) -> Result<Vec<SavedGloss>, rusqlite::Error> {
    if gloss_types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..gloss_types.len())
        .map(|i| format!("?{}", i + 4))
        .collect();
    let sql = format!(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id, g.gloss_type \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND p.end_citation = ?3 \
           AND g.gloss_type IN ({}) \
         ORDER BY g.timestamp DESC",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(work_abbrev.to_string()));
    params.push(Box::new(start_citation.to_string()));
    params.push(Box::new(end_citation.to_string()));
    for gt in gloss_types {
        params.push(Box::new(gt.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(
        param_refs.as_slice(),
        |row| {
            Ok(SavedGloss {
                gloss_id: row.get(0)?,
                gloss_text: row.get(1)?,
                timestamp: row.get(2)?,
                passage_id: row.get(3)?,
                gloss_type: row.get(4)?,
            })
        },
    )?;
    rows.collect()
}

#[derive(Debug, Clone)]
pub struct GlossedPassage {
    pub passage_id: i64,
    pub work_abbrev: String,
    pub start_citation: String,
    pub end_citation: String,
    pub act: i64,
    pub scene: i64,
    pub speaker: String,
    pub source_text: String,
}

pub fn find_glossed_passages(
    conn: &Connection,
    work_abbrev: &str,
    gloss_types: &[&str],
) -> Result<Vec<GlossedPassage>, rusqlite::Error> {
    if gloss_types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..gloss_types.len())
        .map(|i| format!("?{}", i + 2))
        .collect();
    let sql = format!(
        "SELECT DISTINCT p.id, p.work_abbrev, p.start_citation, p.end_citation, \
                p.act, p.scene, p.character, p.source_text \
         FROM passages p \
         JOIN glosses g ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND g.gloss_type IN ({}) \
         ORDER BY p.act, p.scene, p.start_citation",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(work_abbrev.to_string()));
    for gt in gloss_types {
        params.push(Box::new(gt.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(
        param_refs.as_slice(),
        |row| {
            Ok(GlossedPassage {
                passage_id: row.get(0)?,
                work_abbrev: row.get(1)?,
                start_citation: row.get(2)?,
                end_citation: row.get(3)?,
                act: row.get(4)?,
                scene: row.get(5)?,
                speaker: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                source_text: row.get(7)?,
            })
        },
    )?;
    rows.collect()
}

pub fn save_gloss(
    conn: &Connection,
    hash: &str,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
    act: i64,
    scene: i64,
    character: &str,
    source_text: &str,
    gloss_text: &str,
    gloss_type: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO passages \
         (hash, work_abbrev, start_citation, end_citation, act, scene, character, source_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![hash, work_abbrev, start_citation, end_citation, act, scene, character, source_text],
    )?;

    let passage_id: i64 = conn.query_row(
        "SELECT id FROM passages WHERE work_abbrev = ?1 AND start_citation = ?2 AND end_citation = ?3",
        rusqlite::params![work_abbrev, start_citation, end_citation],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO glosses (passage_id, gloss_type, gloss_text, claude_model) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![passage_id, gloss_type, gloss_text, claude_model],
    )?;

    Ok(())
}

pub fn update_gloss(
    conn: &Connection,
    gloss_id: i64,
    gloss_text: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE glosses SET gloss_text = ?1, claude_model = ?2, timestamp = CURRENT_TIMESTAMP WHERE id = ?3",
        rusqlite::params![gloss_text, claude_model, gloss_id],
    )?;
    Ok(())
}

pub fn delete_gloss(conn: &Connection, gloss_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM glosses WHERE id = ?1", [gloss_id])?;
    Ok(())
}

/// Load a map of work abbreviation → title for all works.
pub fn load_work_titles(conn: &Connection) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT abbrev, title FROM works")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (abbrev, title) = row?;
        map.insert(abbrev, title);
    }
    Ok(map)
}

/// A candidate cross-work echo found by semantic search.
#[derive(Debug, Clone)]
pub struct EchoCandidate {
    pub work_abbrev: String,
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64,
    pub speaker: String,
    pub passage_type: String,
    pub passage_text: String,
    pub similarity: f32,
}

/// Decode a stored embedding blob (little-endian f32 values) into a vector.
fn decode_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return -1.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return -1.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Find the top-N passages most similar to the query, excluding the source
/// work. Ranks by a blend of semantic cosine and an optional affect (NRC-VAD)
/// axis: `score = (1 - w) * semantic + w * affect`, where `w` is
/// `affect_weight` in [0, 1].
///
/// `query_text` is the raw highlighted passage text (NOT the enriched
/// "SPEAKER to ADDRESSEE: ..." string) — its VAD is computed locally so the
/// speaker labels don't skew the affect score, matching the document side.
///
/// At `affect_weight == 0.0` (the default), the affect axis is skipped
/// entirely and the ranking is byte-for-byte the pure semantic ranking. The
/// affect axis is also skipped if the lexicon is unavailable or a candidate
/// has no stored `sentiment` blob.
pub fn find_similar_passages(
    conn: &Connection,
    query_embedding: &[f32],
    query_text: &str,
    exclude_work: &str,
    top_n: usize,
    affect_weight: f32,
) -> Result<Vec<EchoCandidate>, rusqlite::Error> {
    let base_exclude = exclude_work.strip_suffix("-Amb").unwrap_or(exclude_work);

    // Only engage the affect axis when it's both requested and possible.
    let affect_on = affect_weight > 0.0 && crate::db::affect::lexicon_available();
    let query_vad = if affect_on {
        crate::db::affect::compute_vad(query_text)
    } else {
        None
    };

    let mut stmt = conn.prepare(
        "SELECT work_abbrev, div1, div2, start_line, speaker, passage_type, passage_text, embedding, sentiment \
         FROM passage_embeddings \
         WHERE work_abbrev != ?1",
    )?;

    let rows = stmt.query_map([base_exclude], |row| {
        let blob: Vec<u8> = row.get(7)?;
        let sentiment: Option<Vec<u8>> = row.get(8)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            blob,
            sentiment,
        ))
    })?;

    let mut candidates: Vec<EchoCandidate> = Vec::new();
    for row in rows {
        let (work_abbrev, div1, div2, start_line, speaker, passage_type, passage_text, blob, sentiment) =
            row?;
        let emb = decode_embedding(&blob);
        let sim = cosine_similarity(query_embedding, &emb);

        // Blend in the affect cosine when active and both sides have a vector.
        // If anything is missing for this candidate, fall back to pure semantic
        // similarity for it rather than penalizing it.
        let score = match (query_vad, sentiment.as_deref().and_then(crate::db::affect::decode_sentiment)) {
            (Some(qv), Some(cv)) => {
                let affect = crate::db::affect::affect_cosine(&qv, &cv);
                (1.0 - affect_weight) * sim + affect_weight * affect
            }
            _ => sim,
        };

        candidates.push(EchoCandidate {
            work_abbrev,
            div1,
            div2,
            start_line,
            speaker,
            passage_type,
            passage_text,
            similarity: score,
        });
    }

    candidates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(top_n);
    Ok(candidates)
}

// ─── Echo links persistence ─────────────────────────────────────────────────

/// Identifies a turn (the cache key for its echoes).
#[derive(Debug, Clone)]
pub struct EchoTurnKey {
    pub work_abbrev: String,
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64,
    pub end_line: i64,
    pub speaker: String,
    pub turn_text: String,
}

/// A stored echo link (cached search result, possibly curated).
#[derive(Debug, Clone)]
pub struct StoredEchoLink {
    pub link_id: i64,
    pub echo_work_abbrev: String,
    pub echo_div1: i64,
    pub echo_div2: i64,
    pub echo_start_line: i64,
    pub echo_text: String,
    pub similarity: f32,
    pub curated: bool,
    pub rank: i64,
}

/// A turn in a work that has at least one echo link. Used by the
/// echo-turns picker (Ctrl+Shift+G) to list all annotated turns.
#[derive(Debug, Clone)]
pub struct EchoTurnSummary {
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64, // line_in_div of the turn's first line
    pub speaker: String,
    pub turn_text: String,
}

/// List every turn in `work_abbrev` that has >= 1 echo link, in reading
/// order (div1, div2, start_line). The JOIN + GROUP BY guarantees only
/// turns with links appear.
pub fn list_echo_turns_for_work(
    conn: &Connection,
    work_abbrev: &str,
    channel: crate::db::echo_channel::EchoChannel,
) -> Result<Vec<EchoTurnSummary>, rusqlite::Error> {
    let sql = format!(
        "SELECT t.div1, t.div2, t.start_line, t.speaker, t.turn_text \
         FROM echo_turns t \
         JOIN echo_links l ON l.turn_id = t.id \
         WHERE t.work_abbrev = ?1 AND {} \
         GROUP BY t.id \
         ORDER BY t.div1, t.div2, t.start_line",
        channel.sql_predicate(),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(EchoTurnSummary {
            div1: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            div2: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            start_line: row.get(2)?,
            speaker: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            turn_text: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// Create the echo_turns and echo_links tables if absent.
pub fn ensure_echo_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS echo_turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            work_abbrev TEXT NOT NULL,
            div1 INTEGER,
            div2 INTEGER,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            speaker TEXT,
            turn_text TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            UNIQUE(work_abbrev, div1, div2, start_line, end_line)
        );
        CREATE TABLE IF NOT EXISTS echo_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            turn_id INTEGER NOT NULL REFERENCES echo_turns(id) ON DELETE CASCADE,
            echo_work_abbrev TEXT NOT NULL,
            echo_div1 INTEGER,
            echo_div2 INTEGER,
            echo_start_line INTEGER,
            echo_text TEXT NOT NULL,
            similarity REAL,
            curated INTEGER NOT NULL DEFAULT 0,
            rank INTEGER NOT NULL,
            UNIQUE(turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_text)
        );
        CREATE INDEX IF NOT EXISTS idx_echo_links_turn ON echo_links(turn_id);"
    )?;
    // Migration: add echo_start_line to pre-existing echo_links tables.
    // Ignore the "duplicate column" error if it already exists.
    let _ = conn.execute("ALTER TABLE echo_links ADD COLUMN echo_start_line INTEGER", []);
    Ok(())
}

/// Find a cached turn whose line range CONTAINS the given line, for a work.
///
/// BCP echo_turns are keyed by chunk boundaries (start_line..end_line spanning
/// several physical lines), so a reader's cursor on a single line inside a chunk
/// won't match `find_echo_turn`'s exact start/end. This range lookup resolves the
/// containing chunk. Returns (turn_id, start_line, end_line, speaker, turn_text)
/// so the caller can build a full EchoSession. Prefers the smallest matching
/// span if chunks ever overlap.
pub fn find_echo_turn_containing(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    line: i64,
) -> Result<Option<(i64, i64, i64, Option<String>, String)>, rusqlite::Error> {
    conn.query_row(
        "SELECT id, start_line, end_line, speaker, turn_text FROM echo_turns \
         WHERE work_abbrev = ?1 AND div1 = ?2 \
           AND start_line <= ?3 AND end_line >= ?3 \
         ORDER BY (end_line - start_line) ASC LIMIT 1",
        rusqlite::params![work_abbrev, div1, line],
        |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        )),
    )
    .optional()
}

/// Find a cached turn row id by its key.
pub fn find_echo_turn(conn: &Connection, key: &EchoTurnKey) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT id FROM echo_turns \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND start_line = ?4 AND end_line = ?5",
        rusqlite::params![key.work_abbrev, key.div1, key.div2, key.start_line, key.end_line],
        |row| row.get::<_, i64>(0),
    )
    .optional()
}

/// Insert (or fetch existing) the turn row, returning its id.
pub fn save_echo_turn(conn: &Connection, key: &EchoTurnKey) -> Result<i64, rusqlite::Error> {
    if let Some(id) = find_echo_turn(conn, key)? {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO echo_turns (work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            key.work_abbrev, key.div1, key.div2, key.start_line, key.end_line,
            key.speaker, key.turn_text
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Load all echo links for a turn, curated first then by rank.
pub fn load_echo_links(conn: &Connection, turn_id: i64, channel: crate::db::echo_channel::EchoChannel) -> Result<Vec<StoredEchoLink>, rusqlite::Error> {
    // JOIN echo_turns so the channel predicate can see the turn's work_abbrev
    // (the BCP channel is "either side is BCP", not just the link side).
    let sql = format!(
        "SELECT l.id, l.echo_work_abbrev, COALESCE(l.echo_div1, 0), COALESCE(l.echo_div2, 0), \
                COALESCE(l.echo_start_line, 0), l.echo_text, \
                COALESCE(l.similarity, 0.0), l.curated, l.rank \
         FROM echo_links l JOIN echo_turns t ON t.id = l.turn_id \
         WHERE l.turn_id = ?1 AND {} \
         ORDER BY l.curated DESC, l.rank ASC",
        channel.sql_predicate(),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([turn_id], |row| {
        Ok(StoredEchoLink {
            link_id: row.get(0)?,
            echo_work_abbrev: row.get(1)?,
            echo_div1: row.get(2)?,
            echo_div2: row.get(3)?,
            echo_start_line: row.get(4)?,
            echo_text: row.get(5)?,
            similarity: row.get::<_, f64>(6)? as f32,
            curated: row.get::<_, i64>(7)? != 0,
            rank: row.get(8)?,
        })
    })?;
    rows.collect()
}

/// Insert echo links for a turn. Ignores duplicates (UNIQUE constraint).
/// Tuple: (work, div1, div2, start_line, text, similarity, rank).
pub fn insert_echo_links(
    conn: &Connection,
    turn_id: i64,
    links: &[(String, i64, i64, i64, String, f32, i64)],
) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(
        "INSERT OR IGNORE INTO echo_links \
         (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
    )?;
    for (work, d1, d2, sl, text, sim, rank) in links {
        stmt.execute(rusqlite::params![turn_id, work, d1, d2, sl, text, *sim as f64, rank])?;
    }
    Ok(())
}

/// Toggle the curated flag on a link, returning the new state.
pub fn toggle_echo_curated(conn: &Connection, link_id: i64) -> Result<bool, rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET curated = 1 - curated WHERE id = ?1",
        [link_id],
    )?;
    conn.query_row(
        "SELECT curated FROM echo_links WHERE id = ?1",
        [link_id],
        |row| row.get::<_, i64>(0).map(|v| v != 0),
    )
}

/// Insert a manual curated echo link at the top of the curated group (rank 0),
/// shifting existing curated ranks down. Returns the new link's id.
pub fn add_curated_echo_link(
    conn: &Connection,
    turn_id: i64,
    work: &str,
    div1: i64,
    div2: i64,
    line_in_div: i64,
    text: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = rank + 1 WHERE turn_id = ?1 AND curated = 1",
        [turn_id],
    )?;
    conn.execute(
        "INSERT INTO echo_links \
         (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0.0, 1, 0)",
        rusqlite::params![turn_id, work, div1, div2, line_in_div, text],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Set a link's rank and curated flag.
pub fn set_echo_link_rank(conn: &Connection, link_id: i64, rank: i64, curated: bool) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = ?2, curated = ?3 WHERE id = ?1",
        rusqlite::params![link_id, rank, curated as i64],
    )?;
    Ok(())
}

/// Delete all non-curated links for a turn (used by refresh).
pub fn delete_noncurated_echo_links(conn: &Connection, turn_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM echo_links WHERE turn_id = ?1 AND curated = 0",
        [turn_id],
    )?;
    Ok(())
}

/// Delete every link (curated and non-curated) for a turn.
pub fn delete_all_echo_links(conn: &Connection, turn_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM echo_links WHERE turn_id = ?1", [turn_id])?;
    Ok(())
}

/// Delete a single echo link by id.
pub fn delete_echo_link(conn: &Connection, link_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM echo_links WHERE id = ?1", [link_id])?;
    Ok(())
}

/// Resolve a line's line_mapping.id from its location within a work.
pub fn line_id_for_location(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    line_in_div: i64,
) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM line_mapping \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND line_in_div = ?4 \
         LIMIT 1",
        rusqlite::params![work_abbrev, div1, div2, line_in_div],
        |row| row.get::<_, i64>(0),
    )
    .ok()
}

/// Search every line whose canonical text contains `query` (case-insensitive),
/// across all works. Returns (work_abbrev, div1, div2, line_in_div, text), capped.
pub fn search_lines(conn: &Connection, query: &str, limit: i64)
    -> Result<Vec<(String, i64, i64, i64, String)>, rusqlite::Error>
{
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT work_abbrev, div1, div2, line_in_div, canonical_text \
         FROM line_mapping \
         WHERE canonical_text LIKE ?1 COLLATE NOCASE \
         ORDER BY work_abbrev, div1, div2, line_in_div \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    rows.collect()
}

/// Look up a single line's start time for a given media file. Returns None when
/// no timestamp row exists for that (line, media) pair.
pub fn line_start_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64> {
    conn.query_row(
        "SELECT start_time FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_id, media_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_lines_matches_substring_case_insensitive_with_limit() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_mapping (
                id INTEGER PRIMARY KEY, work_abbrev TEXT, canonical_text TEXT,
                div1 INTEGER, div2 INTEGER, line_in_div INTEGER
             );
             INSERT INTO line_mapping (id, work_abbrev, canonical_text, div1, div2, line_in_div) VALUES
                (1, 'Ham', 'To be, or not to be', 3, 1, 56),
                (2, 'Mac', 'Tomorrow and tomorrow', 5, 5, 19),
                (3, 'Lr',  'Nothing will come of nothing', 1, 1, 92);",
        ).unwrap();
        let hits = search_lines(&conn, "TOMORROW", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], ("Mac".to_string(), 5, 5, 19, "Tomorrow and tomorrow".to_string()));
        let all = search_lines(&conn, "o", 2).unwrap();
        assert_eq!(all.len(), 2);
        assert!(search_lines(&conn, "zzzz", 10).unwrap().is_empty());
    }

    #[test]
    fn line_start_time_reads_stored_value() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_timestamps (
                line_mapping_id INTEGER, media_id INTEGER, start_time REAL
             );
             INSERT INTO line_timestamps (line_mapping_id, media_id, start_time)
                VALUES (42, 7, 123.5);",
        )
        .unwrap();
        assert_eq!(line_start_time(&conn, 42, 7), Some(123.5));
        // Wrong media or missing line -> None.
        assert_eq!(line_start_time(&conn, 42, 99), None);
        assert_eq!(line_start_time(&conn, 1, 7), None);
    }

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

    #[test]
    fn set_echo_link_rank_updates_rank_and_curated() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY, turn_id INTEGER, echo_work_abbrev TEXT,
                echo_div1 INTEGER, echo_div2 INTEGER, echo_start_line INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER
             );
             INSERT INTO echo_links (id, turn_id, echo_work_abbrev, echo_div1, echo_div2,
                echo_start_line, echo_text, similarity, curated, rank)
                VALUES (1, 7, 'Ham', 1, 1, 1, 'x', 0.0, 0, 5);",
        ).unwrap();
        set_echo_link_rank(&conn, 1, 2, true).unwrap();
        let (rank, curated): (i64, i64) = conn.query_row(
            "SELECT rank, curated FROM echo_links WHERE id = 1", [],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(rank, 2);
        assert_eq!(curated, 1);
    }

    #[test]
    fn add_curated_echo_link_inserts_at_top_shifting_curated() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT, turn_id INTEGER, echo_work_abbrev TEXT,
                echo_div1 INTEGER, echo_div2 INTEGER, echo_start_line INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER
             );
             INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2,
                echo_start_line, echo_text, similarity, curated, rank) VALUES
                (7, 'Mac', 5, 5, 19, 'old curated', 0.0, 1, 0),
                (7, 'Lr', 1, 1, 92, 'noncurated', 0.0, 0, 0);",
        ).unwrap();
        let new_id = add_curated_echo_link(&conn, 7, "Ham", 3, 1, 56, "To be").unwrap();
        let (curated, rank): (i64, i64) = conn.query_row(
            "SELECT curated, rank FROM echo_links WHERE id = ?1", [new_id],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((curated, rank), (1, 0));
        let old_rank: i64 = conn.query_row(
            "SELECT rank FROM echo_links WHERE echo_text = 'old curated'", [],
            |r| r.get(0)).unwrap();
        assert_eq!(old_rank, 1);
        let nc: (i64, i64) = conn.query_row(
            "SELECT curated, rank FROM echo_links WHERE echo_text = 'noncurated'", [],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(nc, (0, 0));
    }

    #[test]
    fn list_echo_turns_for_work_returns_only_linked_turns_in_reading_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT, work_abbrev TEXT NOT NULL,
                div1 INTEGER, div2 INTEGER, start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL, speaker TEXT, turn_text TEXT NOT NULL
             );
             CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT, turn_id INTEGER NOT NULL,
                echo_work_abbrev TEXT, echo_div1 INTEGER, echo_div2 INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER,
                echo_start_line INTEGER
             );
             -- Two Hamlet turns with links, one without; one turn in another work.
             INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text)
                VALUES
                (1, 'Ham', 3, 1, 56, 60, 'HAMLET', 'To be or not to be'),
                (2, 'Ham', 1, 2, 10, 12, 'HAMLET', 'O that this too too'),
                (3, 'Ham', 5, 1, 1, 2, 'GHOST', 'no links here'),
                (4, 'Mac', 1, 1, 1, 2, 'MACBETH', 'is this a dagger');
             INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_text, curated, rank)
                VALUES
                (1, 'Mac', 'echo a', 0, 0),
                (1, 'Lr', 'echo b', 1, 1),
                (2, 'Mac', 'echo c', 0, 0),
                (4, 'Ham', 'echo d', 0, 0);",
        ).unwrap();

        let rows = list_echo_turns_for_work(&conn, "Ham", crate::db::echo_channel::EchoChannel::Shakespeare).unwrap();
        // Turn 3 (no links) and turn 4 (other work) excluded -> only 2 rows.
        assert_eq!(rows.len(), 2);
        // Reading order: (1,2,10) before (3,1,56) -> turn 2 first, then turn 1.
        assert_eq!(rows[0].speaker, "HAMLET");
        assert_eq!(rows[0].div1, 1);
        assert_eq!(rows[0].div2, 2);
        assert_eq!(rows[0].start_line, 10);
        assert_eq!(rows[1].div1, 3);
        assert_eq!(rows[1].start_line, 56);
        assert_eq!(rows[1].turn_text, "To be or not to be");
    }

    #[test]
    fn load_echo_links_filters_by_channel() {
        use crate::db::echo_channel::EchoChannel;
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'Ham',5,1,1,4,'Clown','a')", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'BCP1559',11,NULL,1,'I am the resurrection',0.9,1,0)", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'Mac',1,2,5,'Tomorrow',0.8,0,0)", []).unwrap();
        let bcp = load_echo_links(&conn, 1, EchoChannel::Bcp).unwrap();
        assert_eq!(bcp.len(), 1);
        assert_eq!(bcp[0].echo_work_abbrev, "BCP1559");
        let shx = load_echo_links(&conn, 1, EchoChannel::Shakespeare).unwrap();
        assert_eq!(shx.len(), 1);
        assert_eq!(shx[0].echo_work_abbrev, "Mac");
    }

    #[test]
    fn list_echo_turns_for_work_filters_by_channel() {
        use crate::db::echo_channel::EchoChannel;
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'Ham',5,1,1,4,'Clown','a')", []).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (2,'Ham',1,2,10,12,'Hamlet','b')", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'BCP1559',11,NULL,1,'x',0.9,1,0)", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (2,'Mac',1,2,5,'y',0.8,0,0)", []).unwrap();
        let bcp = list_echo_turns_for_work(&conn, "Ham", EchoChannel::Bcp).unwrap();
        assert_eq!(bcp.len(), 1);
        assert_eq!(bcp[0].start_line, 1);
        let shx = list_echo_turns_for_work(&conn, "Ham", EchoChannel::Shakespeare).unwrap();
        assert_eq!(shx.len(), 1);
        assert_eq!(shx[0].start_line, 10);
    }

    #[test]
    fn find_echo_turn_containing_matches_by_range() {
        // BCP echo_turns span a chunk; a cursor on any line inside resolves it.
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'BCP1559',11,NULL,13,20,NULL,'I AM the resurrection')", []).unwrap();
        // A line inside the chunk resolves.
        let hit = find_echo_turn_containing(&conn, "BCP1559", 11, 15).unwrap();
        assert!(hit.is_some());
        let (id, start, end, speaker, _text) = hit.unwrap();
        assert_eq!((id, start, end), (1, 13, 20));
        assert!(speaker.is_none());
        // A line outside the chunk does not.
        assert!(find_echo_turn_containing(&conn, "BCP1559", 11, 99).unwrap().is_none());
        // Boundaries are inclusive.
        assert!(find_echo_turn_containing(&conn, "BCP1559", 11, 13).unwrap().is_some());
        assert!(find_echo_turn_containing(&conn, "BCP1559", 11, 20).unwrap().is_some());
        // Wrong rite (div1) does not match.
        assert!(find_echo_turn_containing(&conn, "BCP1559", 5, 15).unwrap().is_none());
    }

    #[test]
    fn bcp_channel_includes_bcp_turn_with_shakespeare_echo() {
        // The inverse direction (BCP -> Shakespeare): turn is a BCP work, echo
        // is a Shakespeare work. The two-sided filter must put this in the BCP
        // channel even though echo_work_abbrev is NOT 'BCP%'.
        use crate::db::echo_channel::EchoChannel;
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'BCP1559',11,NULL,1,3,NULL,'I am the resurrection')", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'Ham',5,1,1,'the grave',0.9,1,0)", []).unwrap();

        // load_echo_links: the Shakespeare echo of a BCP turn is BCP-channel.
        let bcp = load_echo_links(&conn, 1, EchoChannel::Bcp).unwrap();
        assert_eq!(bcp.len(), 1);
        assert_eq!(bcp[0].echo_work_abbrev, "Ham");
        // ...and NOT in the Shakespeare channel.
        assert_eq!(load_echo_links(&conn, 1, EchoChannel::Shakespeare).unwrap().len(), 0);

        // list_echo_turns_for_work: the BCP work's turn shows in the BCP channel.
        let turns = list_echo_turns_for_work(&conn, "BCP1559", EchoChannel::Bcp).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].start_line, 1);
        assert_eq!(list_echo_turns_for_work(&conn, "BCP1559", EchoChannel::Shakespeare).unwrap().len(), 0);
    }

    #[test]
    fn test_open_db() {
        let conn = open_db().expect("Failed to open lit.db");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM works", [], |row| row.get(0))
            .unwrap();
        assert!(count > 0, "works table should have rows");
    }

    #[test]
    fn test_list_works() {
        let conn = open_db().unwrap();
        let works = list_works(&conn).unwrap();
        assert!(works.len() > 100, "Should have 100+ works");
        assert!(works.iter().any(|w| w.abbrev == "Ham"));
    }

    #[test]
    fn test_load_translations() {
        let conn = open_db().unwrap();
        let translations = load_translations(&conn, "Ham").unwrap();
        // Hamlet may or may not have translations — just verify no crash
        // and that the return type is correct
        assert!(translations.len() >= 0);
    }

    #[test]
    fn test_load_translations_ambrose_fallback() {
        let conn = open_db().unwrap();
        let translations = load_translations(&conn, "Err-Amb").unwrap();
        assert!(
            !translations.is_empty(),
            "Err-Amb should get translations via -Amb fallback to Err"
        );
        let amb_ids: std::collections::HashSet<i64> = conn
            .prepare("SELECT id FROM line_mapping WHERE work_abbrev='Err-Amb'")
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            translations.keys().all(|k| amb_ids.contains(k)),
            "Keys must be Err-Amb line_mapping.id, not Err's"
        );
    }

    #[test]
    fn test_load_vocab_words() {
        let conn = open_db().unwrap();
        let words = load_vocab_words(&conn, "Ham").unwrap();
        assert!(!words.is_empty(), "Should have vocab words for Hamlet");
    }

    #[test]
    fn test_load_vocab_definition() {
        let conn = open_db().unwrap();
        let words = load_vocab_words(&conn, "Ham").unwrap();
        if let Some(word) = words.iter().next() {
            let def = load_vocab_definition(&conn, word);
            let _ = def;
        }
    }

    #[test]
    fn test_load_vocab_word_list() {
        let conn = open_db().unwrap();
        let list = load_vocab_word_list(&conn, "Ham").unwrap();
        if list.len() > 1 {
            assert!(list[0].0 <= list[1].0, "Should be alphabetically sorted");
        }
    }

    #[test]
    fn test_bookmark_toggle() {
        let conn = open_db_rw().expect("Failed to open lit.db rw");
        ensure_bookmarks_table(&conn).expect("Failed to create bookmarks table");

        // Use a known work and line
        let work_abbrev = "Ham";
        let line_id: i64 = conn.query_row(
            "SELECT id FROM line_mapping WHERE work_abbrev = ?1 LIMIT 1",
            [work_abbrev],
            |row| row.get(0),
        ).expect("Hamlet should have lines");

        // Clean up any leftover test bookmark
        let _ = conn.execute(
            "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
            rusqlite::params![work_abbrev, line_id],
        );

        // Toggle on
        let added = toggle_bookmark(&conn, work_abbrev, line_id).unwrap();
        assert!(added, "First toggle should add bookmark");

        // Should appear in load_bookmarks
        let bookmarks = load_bookmarks(&conn, work_abbrev).unwrap();
        assert!(bookmarks.contains(&line_id));

        // Should be the most recent
        let recent = most_recent_bookmark(&conn, work_abbrev).unwrap();
        assert_eq!(recent, Some(line_id));

        // Toggle off
        let removed = toggle_bookmark(&conn, work_abbrev, line_id).unwrap();
        assert!(!removed, "Second toggle should remove bookmark");

        // Should no longer appear
        let bookmarks = load_bookmarks(&conn, work_abbrev).unwrap();
        assert!(!bookmarks.contains(&line_id));
    }

    #[test]
    fn test_load_bookmarks_with_details() {
        let conn = open_db_rw().expect("Failed to open lit.db rw");
        ensure_bookmarks_table(&conn).expect("Failed to create bookmarks table");

        let work_abbrev = "Ham";
        let line_id: i64 = conn.query_row(
            "SELECT id FROM line_mapping WHERE work_abbrev = ?1 LIMIT 1",
            [work_abbrev],
            |row| row.get(0),
        ).expect("Hamlet should have lines");

        // Clean up
        let _ = conn.execute(
            "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
            rusqlite::params![work_abbrev, line_id],
        );

        // Add a bookmark
        toggle_bookmark(&conn, work_abbrev, line_id).unwrap();

        // Load with details
        let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
        let found = items.iter().find(|i| i.line_mapping_id == line_id);
        assert!(found.is_some(), "Should find the bookmarked line");
        let item = found.unwrap();
        assert!(!item.line_text.is_empty(), "Line text should not be empty");
        assert!(!item.citation.is_empty(), "citation should not be empty");

        // Delete it
        delete_bookmark(&conn, work_abbrev, line_id).unwrap();
        let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
        assert!(
            !items.iter().any(|i| i.line_mapping_id == line_id),
            "Bookmark should be deleted"
        );
    }

    #[test]
    fn gloss_audio_roundtrip_and_upsert() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // gloss_audio references glosses(id); create a minimal glosses table for the FK.
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (4823);",
        )
        .unwrap();
        ensure_gloss_audio_table(&conn).unwrap();

        // Miss before insert.
        assert_eq!(find_gloss_audio(&conn, 4823, "explication", 0, "voiceA").unwrap(), None);

        // Insert, then hit.
        save_gloss_audio(&conn, 4823, "explication", 0, "/tmp/a/0.mp3", "voiceA", "modelA").unwrap();
        assert_eq!(
            find_gloss_audio(&conn, 4823, "explication", 0, "voiceA").unwrap(),
            Some("/tmp/a/0.mp3".to_string())
        );

        // Upsert: same (gloss_id, kind, paragraph_index, voice_id) replaces the path.
        save_gloss_audio(&conn, 4823, "explication", 0, "/tmp/a/0b.mp3", "voiceA", "modelB").unwrap();
        assert_eq!(
            find_gloss_audio(&conn, 4823, "explication", 0, "voiceA").unwrap(),
            Some("/tmp/a/0b.mp3".to_string())
        );

        // Distinct paragraph_index is a separate row.
        save_gloss_audio(&conn, 4823, "explication", 1, "/tmp/a/1.mp3", "voiceA", "modelA").unwrap();
        assert_eq!(
            find_gloss_audio(&conn, 4823, "explication", 1, "voiceA").unwrap(),
            Some("/tmp/a/1.mp3".to_string())
        );
    }

    #[test]
    fn delete_gloss_audio_removes_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (7);",
        )
        .unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        save_gloss_audio(&conn, 7, "explication", 0, "/tmp/7/0.mp3", "v", "m").unwrap();
        save_gloss_audio(&conn, 7, "explication", 1, "/tmp/7/1.mp3", "v", "m").unwrap();
        assert!(find_gloss_audio(&conn, 7, "explication", 0, "v").unwrap().is_some());
        let removed = delete_gloss_audio(&conn, 7).unwrap();
        assert_eq!(removed, 2, "should report both deleted audio rows");
        assert!(find_gloss_audio(&conn, 7, "explication", 0, "v").unwrap().is_none());
        assert!(find_gloss_audio(&conn, 7, "explication", 1, "v").unwrap().is_none());
    }

    #[test]
    fn delete_gloss_audio_block_scopes_to_one_block() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (7);",
        ).unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        let ins = |kind: &str, idx: i64, voice: &str, path: &str| {
            conn.execute(
                "INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
                 VALUES (7, ?1, ?2, ?3, ?4, 'm')",
                rusqlite::params![kind, idx, path, voice],
            ).unwrap();
        };
        ins("source", 0, "vA", "/a0.mp3");
        ins("source", 0, "vB", "/a0b.mp3"); // same block, second voice
        ins("source", 1, "vA", "/a1.mp3");  // different block — must survive
        let paths = delete_gloss_audio_block(&conn, 7, "source", 0).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/a0.mp3".to_string()));
        assert!(paths.contains(&"/a0b.mp3".to_string()));
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM gloss_audio WHERE gloss_id=7", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn gloss_audio_kind_distinct_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (9);",
        )
        .unwrap();
        ensure_gloss_audio_table(&conn).unwrap();

        save_gloss_audio(&conn, 9, "explication", 0, "/e0.mp3", "v", "m").unwrap();
        save_gloss_audio(&conn, 9, "source", 0, "/s0.mp3", "v", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 9, "explication", 0, "v").unwrap(), Some("/e0.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 9, "source", 0, "v").unwrap(), Some("/s0.mp3".to_string()));

        save_gloss_audio(&conn, 9, "source", 0, "/s0b.mp3", "v", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 9, "source", 0, "v").unwrap(), Some("/s0b.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 9, "explication", 0, "v").unwrap(), Some("/e0.mp3".to_string()));
    }

    #[test]
    fn gloss_audio_caches_per_voice() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (1);",
        ).unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        // two voices for the SAME (gloss, kind, index) coexist as separate rows
        save_gloss_audio(&conn, 1, "source", 0, "/a.mp3", "vA", "m1").unwrap();
        save_gloss_audio(&conn, 1, "source", 0, "/b.mp3", "vB", "m2").unwrap();
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vA").unwrap(), Some("/a.mp3".into()));
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vB").unwrap(), Some("/b.mp3".into()));
        // re-saving the same (gloss,kind,index,voice) overwrites just that one
        save_gloss_audio(&conn, 1, "source", 0, "/a2.mp3", "vA", "m1").unwrap();
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vA").unwrap(), Some("/a2.mp3".into()));
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vB").unwrap(), Some("/b.mp3".into()));
        // a voice with no cached row -> None
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vZ").unwrap(), None);
    }

    #[test]
    fn gloss_audio_migrates_unique_key_to_per_voice() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (5);",
        ).unwrap();
        // Pre-per-voice shape: has `kind`, but 3-column UNIQUE (no voice_id).
        conn.execute_batch(
            "CREATE TABLE gloss_audio (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gloss_id INTEGER NOT NULL,
                kind TEXT NOT NULL DEFAULT 'explication',
                paragraph_index INTEGER NOT NULL,
                audio_path TEXT NOT NULL,
                voice_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(gloss_id, kind, paragraph_index)
            );
            INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
                VALUES (5, 'source', 0, '/old.mp3', 'vA', 'm');",
        ).unwrap();

        ensure_gloss_audio_table(&conn).unwrap();
        // Existing row preserved under its voice.
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vA").unwrap(), Some("/old.mp3".to_string()));
        // A second voice now coexists (was impossible under the old UNIQUE).
        save_gloss_audio(&conn, 5, "source", 0, "/new.mp3", "vB", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vA").unwrap(), Some("/old.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vB").unwrap(), Some("/new.mp3".to_string()));

        // Idempotent: a second ensure does not re-migrate or lose data.
        ensure_gloss_audio_table(&conn).unwrap();
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vA").unwrap(), Some("/old.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vB").unwrap(), Some("/new.mp3".to_string()));
    }

    #[test]
    fn gloss_audio_migrates_legacy_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (3);",
        )
        .unwrap();
        // Legacy table shape (no `kind` column), with one row.
        conn.execute_batch(
            "CREATE TABLE gloss_audio (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gloss_id INTEGER NOT NULL,
                paragraph_index INTEGER NOT NULL,
                audio_path TEXT NOT NULL,
                voice_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(gloss_id, paragraph_index)
            );
            INSERT INTO gloss_audio (gloss_id, paragraph_index, audio_path, voice_id, model_id)
                VALUES (3, 0, '/legacy0.mp3', 'v', 'm');",
        )
        .unwrap();

        ensure_gloss_audio_table(&conn).unwrap();
        assert_eq!(find_gloss_audio(&conn, 3, "explication", 0, "v").unwrap(), Some("/legacy0.mp3".to_string()));
        save_gloss_audio(&conn, 3, "source", 0, "/s.mp3", "v", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 3, "source", 0, "v").unwrap(), Some("/s.mp3".to_string()));

        // Idempotent: a second ensure call is a no-op and preserves data.
        ensure_gloss_audio_table(&conn).unwrap();
        assert_eq!(find_gloss_audio(&conn, 3, "explication", 0, "v").unwrap(), Some("/legacy0.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 3, "source", 0, "v").unwrap(), Some("/s.mp3".to_string()));
    }

    #[test]
    fn test_load_work_hamlet() {
        let conn = open_db().unwrap();
        let work = load_work(&conn, "Ham").unwrap();
        assert_eq!(work.title, "Hamlet");
        assert_eq!(work.work_type, "play");
        assert!(work.lines.len() > 4000, "Hamlet should have 4000+ lines");
        assert_eq!(work.lines[0].text, "Who\u{2019}s there?");
        assert!(work.lines[0].is_dialogue);
        assert!(!work.timestamps.is_empty(), "Work should have timestamps loaded");
    }

    #[test]
    fn ensure_characters_table_creates_usable_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_characters_table(&conn).unwrap();
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Ham','HAMLET','male')",
            [],
        ).unwrap();
        let g: String = conn
            .query_row(
                "SELECT gender FROM characters WHERE work_abbrev='Ham' AND speaker='HAMLET'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(g, "male");
    }

    #[test]
    fn characters_table_has_age_column() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_characters_table(&conn).unwrap();
        // age column exists and is nullable
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Ham','GHOST','male')",
            [],
        ).unwrap();
        let age: Option<i64> = conn
            .query_row("SELECT age FROM characters WHERE speaker='HAMLET'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(age, Some(30));
        let none: Option<i64> = conn
            .query_row("SELECT age FROM characters WHERE speaker='GHOST'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn characters_table_migrates_legacy_no_age() {
        let conn = Connection::open_in_memory().unwrap();
        // legacy 3-column table (pre-age) with a row
        conn.execute_batch(
            "CREATE TABLE characters (
                work_abbrev TEXT NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL,
                PRIMARY KEY (work_abbrev, speaker));
             -- 3-col positional INSERT is valid here: legacy table, pre-migration
             INSERT INTO characters VALUES ('Ham','HAMLET','male');",
        ).unwrap();
        ensure_characters_table(&conn).unwrap(); // should ALTER ADD age
        // existing row preserved, age NULL
        let (g, a): (String, Option<i64>) = conn
            .query_row("SELECT gender, age FROM characters WHERE speaker='HAMLET'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(g, "male");
        assert_eq!(a, None);
    }

    #[test]
    fn from_db_parses_lowercase_and_defaults_unknown() {
        use crate::elevenlabs::Gender;
        assert_eq!(Gender::from_db("male"), Gender::Male);
        assert_eq!(Gender::from_db("female"), Gender::Female);
        assert_eq!(Gender::from_db("neutral"), Gender::Neutral);
        assert_eq!(Gender::from_db("MALE"), Gender::Unknown);   // case-sensitive by design
        assert_eq!(Gender::from_db("garbage"), Gender::Unknown);
    }

    #[test]
    fn gloss_voices_toggle_add_remove_and_order() {
        let conn = Connection::open_in_memory().unwrap();
        // Parent table for the gloss_id FK (rusqlite enforces foreign keys).
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (1), (2);",
        )
        .unwrap();
        ensure_gloss_voices_table(&conn).unwrap();
        // add two voices -> both present, in insertion order
        assert!(toggle_gloss_voice(&conn, 1, "vA", "m1"));   // true = added
        assert!(toggle_gloss_voice(&conn, 1, "vB", "m2"));
        assert_eq!(
            get_gloss_voices(&conn, 1),
            vec![("vA".to_string(), "m1".to_string()), ("vB".to_string(), "m2".to_string())]
        );
        // toggling vA again removes it
        assert!(!toggle_gloss_voice(&conn, 1, "vA", "m1"));  // false = removed
        assert_eq!(get_gloss_voices(&conn, 1), vec![("vB".to_string(), "m2".to_string())]);
        // a different gloss has its own (empty) set
        assert!(get_gloss_voices(&conn, 2).is_empty());
    }

    #[test]
    fn gloss_voices_readd_goes_to_end() {
        let conn = Connection::open_in_memory().unwrap();
        // Parent table for the gloss_id FK (rusqlite enforces foreign keys).
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (1), (2);",
        )
        .unwrap();
        ensure_gloss_voices_table(&conn).unwrap();
        toggle_gloss_voice(&conn, 1, "vA", "m");  // pos 0
        toggle_gloss_voice(&conn, 1, "vB", "m");  // pos 1
        toggle_gloss_voice(&conn, 1, "vA", "m");  // remove vA
        toggle_gloss_voice(&conn, 1, "vA", "m");  // re-add vA -> pos 2 (after vB)
        assert_eq!(
            get_gloss_voices(&conn, 1),
            vec![("vB".to_string(), "m".to_string()), ("vA".to_string(), "m".to_string())],
            "re-added voice should sort after existing ones (end of cycle order)"
        );
    }

    #[test]
    fn voice_catalog_seeds_four_pairs() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_voice_catalog_table(&conn).unwrap();
        // 8 rows: 4 pairs x verse/prose
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 8);
        // Benedick prose (witty male, the older/default male) is present with its band
        let (vid, lo, hi): (String, i64, i64) = conn
            .query_row(
                "SELECT voice_id, age_min, age_max FROM voice_catalog \
                 WHERE gender='male' AND role='prose' AND age_min=26",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(vid, crate::elevenlabs::BENEDICK_VOICE_ID);
        assert_eq!((lo, hi), (26, 34));
        // idempotent: a second ensure does not duplicate rows
        ensure_voice_catalog_table(&conn).unwrap();
        let n2: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n2, 8);
    }

    fn seed_catalog_and_chars(conn: &Connection) {
        ensure_voice_catalog_table(conn).unwrap();
        ensure_characters_table(conn).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Rom','JULIET','female',14)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Lr','LEAR','male',80)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Rom','NURSE','female')", []).unwrap();
    }

    #[test]
    fn resolve_containment_picks_the_band_containing_age() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Juliet 14 female -> Juliet voice (12-19) verse
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", true),
            (crate::elevenlabs::JULIET_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_nearest_band_when_no_containment() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Lear 80 male VERSE: no band contains 80; nearest male band is Benedick
        // (26-34, distance 46) vs Romeo (15-25, distance 55) -> Benedick verse.
        // (Prose is always Beatrice — see resolve_prose_always_beatrice.)
        assert_eq!(
            resolve_default_voice(&conn, "Lr", "LEAR", true),
            (crate::elevenlabs::BENEDICK_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_null_age_uses_default_age_40() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Nurse female, NULL age -> DEFAULT_AGE 40. No female band contains 40
        // (Juliet 12-19, Beatrice 20-30); nearest is Beatrice (dist 10) verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "NURSE", true),
            (crate::elevenlabs::BEATRICE_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_unknown_gender_defaults_male() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // No characters row -> Unknown gender -> male; NULL age -> 40; no male band
        // contains 40, nearest is Benedick (26-34, dist 6) -> Benedick verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "NOBODY", true),
            (crate::elevenlabs::BENEDICK_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_neutral_gender_uses_male_voice() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Rom','CHORUS','neutral',40)",
            [],
        ).unwrap();
        // neutral -> male; age 40, no male band contains it, nearest is Benedick
        // (26-34, dist 6) -> Benedick verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "CHORUS", true),
            (crate::elevenlabs::BENEDICK_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn synopsis_audio_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_synopsis_audio_table(&conn).unwrap();

        // Miss before save.
        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit, None);

        save_synopsis_audio(
            &conn, "KingJohn", 4, 2, 0, "/tmp/a.mp3", "voice123", "eleven_v3",
        )
        .unwrap();

        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/a.mp3"));

        // Different voice is a separate cache entry → miss.
        let other = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voiceXYZ").unwrap();
        assert_eq!(other, None);

        // Upsert updates the path in place.
        save_synopsis_audio(
            &conn, "KingJohn", 4, 2, 0, "/tmp/b.mp3", "voice123", "eleven_v3",
        )
        .unwrap();
        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/b.mp3"));
    }

    #[test]
    fn resolve_prose_always_beatrice() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Prose (is_verse=false) is ALWAYS Beatrice, regardless of the speaker's
        // gender/age — even a young male like Romeo, or an unknown speaker.
        let beatrice = (
            crate::elevenlabs::BEATRICE_VOICE_ID.to_string(),
            crate::elevenlabs::OP_MODEL_ID.to_string(),
        );
        assert_eq!(resolve_default_voice(&conn, "Lr", "LEAR", false), beatrice);
        assert_eq!(resolve_default_voice(&conn, "Rom", "NOBODY", false), beatrice);
        // ...while the same speaker in VERSE still picks by gender/age (Benedick).
        assert_eq!(
            resolve_default_voice(&conn, "Lr", "LEAR", true),
            (crate::elevenlabs::BENEDICK_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }
}

#[cfg(test)]
mod scansion_tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, work_abbrev TEXT,
               div1 INTEGER, div2 INTEGER, line_in_div INTEGER, canonical_text TEXT);
             CREATE TABLE line_meter (line_id INTEGER, syllable_count INTEGER,
               nominal_feet INTEGER, line_type TEXT, caesura_after INTEGER,
               is_rhymed INTEGER, confidence REAL, source_note TEXT);
             CREATE TABLE syllable_scan (line_id INTEGER, position INTEGER,
               foot_index INTEGER, ictus INTEGER, foot_type TEXT, surface TEXT,
               start_char INTEGER, end_char INTEGER, phenomenon TEXT,
               is_extrametrical INTEGER);
             INSERT INTO line_mapping VALUES (10,'TN',1,1,1,'If music');
             INSERT INTO line_meter (line_id,syllable_count,nominal_feet,line_type,caesura_after)
               VALUES (10,2,5,'regular',NULL);
             INSERT INTO syllable_scan (line_id,position,foot_index,ictus,surface,is_extrametrical)
               VALUES (10,1,1,0,'If',0),(10,2,1,1,'mu',0);
             INSERT INTO line_mapping VALUES (11,'TN',1,1,2,'O brave');
             INSERT INTO line_meter (line_id,syllable_count,nominal_feet,line_type,caesura_after)
               VALUES (11,2,5,'feminine_ending',1);
             INSERT INTO syllable_scan (line_id,position,foot_index,ictus,surface,is_extrametrical)
               VALUES (11,1,1,1,'O',0),(11,2,1,0,'brave',1);",
        ).unwrap();
        c
    }

    #[test]
    fn loads_scansion_keyed_by_line_id() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        let ls = map.get(&10).expect("line 10 present");
        assert_eq!(ls.line_type, "regular");
        assert_eq!(ls.caesura_after, None);
        assert_eq!(ls.syllables.len(), 2);
        assert_eq!(ls.syllables[1].ictus, 1);
        assert_eq!(ls.syllables[1].surface, "mu");
    }

    #[test]
    fn loads_caesura_and_extrametrical() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        let ls = map.get(&11).expect("line 11 present");
        assert_eq!(ls.line_type, "feminine_ending");
        assert_eq!(ls.caesura_after, Some(1));          // Option<i32> Some-branch
        assert_eq!(ls.syllables.len(), 2);
        assert!(!ls.syllables[0].is_extrametrical);     // 0 -> false
        assert!(ls.syllables[1].is_extrametrical);      // 1 -> true
        assert_eq!(ls.syllables[0].surface, "O");
    }

    #[test]
    fn unscanned_line_absent_from_map() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        assert!(map.get(&999).is_none());
    }
}
