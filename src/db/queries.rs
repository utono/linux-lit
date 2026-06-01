use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::HashMap;

use super::line_types;
use super::models::{Line, MediaItem, TimeRange, Timestamp, Work, WorkSummary};

fn db_path() -> String {
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
        "SELECT b.line_mapping_id, lm.canonical_text, b.created_at \
         FROM bookmarks b \
         JOIN line_mapping lm ON b.line_mapping_id = lm.id \
         WHERE b.work_abbrev = ?1 \
         ORDER BY b.created_at DESC"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(super::models::BookmarkItem {
            line_mapping_id: row.get(0)?,
            line_text: row.get(1)?,
            created_at: row.get(2)?,
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
        "INSERT INTO glosses (passage_id, gloss_type, gloss_text) VALUES (?1, ?2, ?3)",
        rusqlite::params![passage_id, gloss_type, gloss_text],
    )?;

    Ok(())
}

pub fn update_gloss(conn: &Connection, gloss_id: i64, gloss_text: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE glosses SET gloss_text = ?1, timestamp = CURRENT_TIMESTAMP WHERE id = ?2",
        rusqlite::params![gloss_text, gloss_id],
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
) -> Result<Vec<EchoTurnSummary>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.div1, t.div2, t.start_line, t.speaker, t.turn_text \
         FROM echo_turns t \
         JOIN echo_links l ON l.turn_id = t.id \
         WHERE t.work_abbrev = ?1 \
         GROUP BY t.id \
         ORDER BY t.div1, t.div2, t.start_line",
    )?;
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
pub fn load_echo_links(conn: &Connection, turn_id: i64) -> Result<Vec<StoredEchoLink>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, echo_work_abbrev, echo_div1, echo_div2, \
                COALESCE(echo_start_line, 0), echo_text, \
                COALESCE(similarity, 0.0), curated, rank \
         FROM echo_links WHERE turn_id = ?1 \
         ORDER BY curated DESC, rank ASC",
    )?;
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

        let rows = list_echo_turns_for_work(&conn, "Ham").unwrap();
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
        assert!(!item.created_at.is_empty(), "created_at should not be empty");

        // Delete it
        delete_bookmark(&conn, work_abbrev, line_id).unwrap();
        let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
        assert!(
            !items.iter().any(|i| i.line_mapping_id == line_id),
            "Bookmark should be deleted"
        );
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
}
