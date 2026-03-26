use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;

use super::line_types;
use super::models::{Line, TimeRange, Timestamp, Work, WorkSummary};

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

    let is_prose = line_types::is_prose_work(&work_type);

    // 2. Load all lines
    let mut line_stmt = conn.prepare(
        "SELECT id, canonical_text, normalized_text, speaker \
         FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div",
    )?;
    let lines: Vec<Line> = line_stmt
        .query_map([abbrev], |row| {
            let text: String = row.get(1)?;
            let normalized: String = row.get(2)?;
            let speaker: Option<String> = row.get(3)?;
            Ok(Line {
                id: row.get(0)?,
                is_dialogue: line_types::is_dialogue(&text, is_prose),
                text,
                normalized,
                speaker,
                timestamp: None,
            })
        })?
        .collect::<Result<_, _>>()?;

    // 3. Load timestamps
    let mut ts_stmt = conn.prepare(
        "SELECT lt.line_mapping_id, lt.start_time, lt.end_time, lt.media_id \
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
            })
        })?
        .collect::<Result<_, _>>()?;

    // 4. Build timestamp lookup: line_id -> TimeRange
    let mut ts_map: HashMap<i64, TimeRange> = HashMap::new();
    for ts in &timestamps {
        ts_map.entry(ts.line_id).or_insert(TimeRange {
            start: ts.start,
            end: ts.end,
        });
    }

    // 5. Attach timestamps to lines
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|mut line| {
            line.timestamp = ts_map.get(&line.id).copied();
            line
        })
        .collect();

    // 6. Load media paths
    let mut media_stmt = conn.prepare(
        "SELECT mf.path FROM media_files mf \
         JOIN work_media_associations wma ON wma.media_id = mf.id \
         WHERE wma.work_abbrev = ?1 \
         ORDER BY wma.priority DESC",
    )?;
    let media_paths: Vec<String> = media_stmt
        .query_map([abbrev], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    Ok(Work {
        abbrev: abbrev.to_string(),
        title,
        author,
        work_type,
        lines,
        timestamps,
        media_paths,
    })
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
    fn test_load_work_hamlet() {
        let conn = open_db().unwrap();
        let work = load_work(&conn, "Ham").unwrap();
        assert_eq!(work.title, "Hamlet");
        assert_eq!(work.work_type, "play");
        assert!(work.lines.len() > 4000, "Hamlet should have 4000+ lines");
        assert_eq!(work.lines[0].text, "Who\u{2019}s there?");
        assert!(work.lines[0].is_dialogue);
        let with_ts = work.lines.iter().filter(|l| l.timestamp.is_some()).count();
        assert!(with_ts > 0, "Some lines should have timestamps");
    }
}
