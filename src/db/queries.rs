use rusqlite::{Connection, OpenFlags};
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
            let div1: i64 = row.get(4)?;
            let div2: i64 = row.get(5)?;
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
         lt.sentence_start_time, lt.sentence_end_time \
         FROM line_timestamps lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let timestamps: Vec<Timestamp> = ts_stmt
        .query_map([abbrev], |row| {
            Ok(Timestamp {
                line_id: row.get(0)?,
                start: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                end: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                media_id: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                sentence_start: row.get::<_, Option<f64>>(4)?,
                sentence_end: row.get::<_, Option<f64>>(5)?,
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
    let media_paths: Vec<String> = media_rows.into_iter().map(|(_, path)| path).collect();

    // 5. Build timestamp lookup: line_id -> TimeRange (filtered by active media_id)
    let mut ts_map: HashMap<i64, TimeRange> = HashMap::new();
    for ts in &timestamps {
        if media_id.map_or(true, |mid| ts.media_id == mid) {
            ts_map.entry(ts.line_id).or_insert(TimeRange {
                start: ts.start,
                end: ts.end,
                sentence_start: ts.sentence_start,
                sentence_end: ts.sentence_end,
            });
        }
    }

    // 5b. Build chapter lookup: line_id -> bool (filtered by active media_id)
    let mut chapter_map: HashMap<i64, bool> = HashMap::new();
    if let Some(mid) = media_id {
        let mut ch_stmt = conn.prepare(
            "SELECT line_mapping_id FROM line_timestamps \
             WHERE media_id = ?1 AND is_chapter = 1",
        )?;
        let rows = ch_stmt.query_map([mid], |row| row.get::<_, i64>(0))?;
        for row in rows {
            chapter_map.insert(row?, true);
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
        media_id,
    })
}

/// Load translations for a work, keyed by line_mapping.id.
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
    Ok(map)
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
    #[allow(dead_code)]
    pub root_base: Option<String>,
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
        "SELECT vr.prefix, vr.prefix_gloss, vr.root, vr.root_base, \
         vr.root_gloss, vr.suffix, vr.suffix_gloss \
         FROM vocab_rhetoric vr \
         JOIN vocab_words vw ON vr.word_id = vw.id \
         WHERE LOWER(vw.word) = ?1",
        [word.to_lowercase()],
        |row| Ok(VocabEtymology {
            prefix: row.get::<_, Option<String>>(0)?,
            prefix_gloss: row.get::<_, Option<String>>(1)?,
            root: row.get::<_, Option<String>>(2)?,
            root_base: row.get::<_, Option<String>>(3)?,
            root_gloss: row.get::<_, Option<String>>(4)?,
            suffix: row.get::<_, Option<String>>(5)?,
            suffix_gloss: row.get::<_, Option<String>>(6)?,
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
        "SELECT canonical_text FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div"
    )?;
    let lines: Vec<String> = stmt.query_map([work_abbrev], |row| {
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
         DO UPDATE SET start_time = ?4, updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    Ok(())
}

pub fn upsert_chapter(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source, is_chapter) \
         VALUES (?1, ?2, ?3, ?4, 'manual', 1) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET is_chapter = 1, updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    Ok(())
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


#[cfg(test)]
mod tests {
    use super::*;

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
