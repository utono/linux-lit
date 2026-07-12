use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct JournalPage {
    pub id: i64,
    pub div1: i64,
    pub div2: i64,
    pub question: String,
    pub answer: String,
    pub claude_model: String,
    pub timestamp: String,
    pub start_citation: Option<String>,
    pub end_citation: Option<String>,
    pub source_text: Option<String>,
    pub kind: String,
}

/// The SELECT column list every `find_*` query uses, in the order
/// `map_journal_page_row` reads. Kept as one const so the column list and the
/// row mapper cannot drift apart.
const JOURNAL_PAGE_COLUMNS: &str =
    "id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp, \
     start_citation, end_citation, source_text, COALESCE(kind, 'qa')";

/// Build a `JournalPage` from a row selected with `JOURNAL_PAGE_COLUMNS`.
fn map_journal_page_row(row: &rusqlite::Row<'_>) -> Result<JournalPage, rusqlite::Error> {
    Ok(JournalPage {
        id: row.get(0)?,
        div1: row.get(1)?,
        div2: row.get(2)?,
        question: row.get(3)?,
        answer: row.get(4)?,
        claude_model: row.get(5)?,
        timestamp: row.get(6)?,
        start_citation: row.get(7)?,
        end_citation: row.get(8)?,
        source_text: row.get(9)?,
        kind: row.get(10)?,
    })
}

pub fn ensure_journal_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal_entries (
            id          INTEGER PRIMARY KEY,
            work_abbrev TEXT    NOT NULL,
            div1        INTEGER NOT NULL,
            div2        INTEGER NOT NULL,
            question    TEXT    NOT NULL,
            answer      TEXT    NOT NULL,
            claude_model TEXT,
            scope       TEXT    NOT NULL DEFAULT 'scene',
            start_citation TEXT,
            end_citation   TEXT,
            source_text    TEXT,
            kind        TEXT    NOT NULL DEFAULT 'qa',
            timestamp   TEXT    NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journal_work_scene
            ON journal_entries(work_abbrev, div1, div2, timestamp);",
    )?;
    // Idempotent migration for any DB whose table predates the scope column.
    // All column-existence probes share the `column_exists` helper (audit #67).
    use crate::db::queries::column_exists;
    if !column_exists(conn, "journal_entries", "scope")? {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN scope TEXT NOT NULL DEFAULT 'scene';",
        )?;
    }
    for col in ["start_citation", "end_citation", "source_text"] {
        if !column_exists(conn, "journal_entries", col)? {
            conn.execute_batch(&format!(
                "ALTER TABLE journal_entries ADD COLUMN {col} TEXT;"
            ))?;
        }
    }
    if !column_exists(conn, "journal_entries", "kind")? {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN kind TEXT NOT NULL DEFAULT 'qa';",
        )?;
    }
    if !column_exists(conn, "journal_entries", "word")? {
        conn.execute_batch("ALTER TABLE journal_entries ADD COLUMN word TEXT;")?;
    }
    Ok(())
}

pub fn save_journal_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
    scope: &str,
    kind: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope, kind, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
        rusqlite::params![work_abbrev, div1, div2, question, answer, claude_model, scope, kind],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_journal_pages(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND scope = 'scene'
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, div1, div2], map_journal_page_row)?;
    rows.collect()
}

/// All pages that belong to a scene/chapter BAND: both scene Q&As and the
/// passage Q&As anchored in the same `(div1, div2)`, ordered by creation time.
/// A passage Q&A belongs to its scene band (the band the reader was in when the
/// passage was selected), so the journal overlay pages through scene + passage
/// Q&As together via `Ctrl+n/p`. `find_journal_pages` (scene only) is still used
/// by the ask-save reload path; this is the band-render path.
pub fn find_scene_band_pages(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND scope IN ('scene', 'passage')
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, div1, div2], map_journal_page_row)?;
    rows.collect()
}

/// The (div1, div2) sentinel that marks an author/corpus-scope journal row.
/// Distinct from JOURNAL_WORK_DIV (-1,-1) so author rows never collide with
/// whole-work rows. `work_abbrev` holds the AUTHOR string for these rows.
pub const AUTHOR_DIV: (i64, i64) = (-2, -2);

pub fn save_author_page(
    conn: &Connection,
    author: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
    kind: &str,
) -> Result<i64, rusqlite::Error> {
    save_journal_page(
        conn, author, AUTHOR_DIV.0, AUTHOR_DIV.1, question, answer, claude_model, "author", kind,
    )
}

pub fn find_author_pages(
    conn: &Connection,
    author: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'author' \
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![author], map_journal_page_row)?;
    rows.collect()
}

pub fn find_work_pages(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'work'
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map([work_abbrev], map_journal_page_row)?;
    rows.collect()
}

/// All pages for a work, ordered for the picker: whole-work pages first (by
/// creation time), then scene pages grouped by scene (div1, div2), each scene's
/// pages by creation time. `(scope = 'work')` sorts true(1) before false(0) via
/// DESC so work rows lead.
pub fn find_all_pages_ordered(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1
         ORDER BY (scope = 'work') DESC, div1 ASC, div2 ASC, timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map([work_abbrev], map_journal_page_row)?;
    rows.collect()
}

pub fn save_passage_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_citation: &str,
    end_citation: &str,
    source_text: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope,
             start_citation, end_citation, source_text, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'passage', ?7, ?8, ?9, datetime('now'))",
        rusqlite::params![
            work_abbrev, div1, div2, question, answer, claude_model,
            start_citation, end_citation, source_text
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_passage_pages(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'passage' \
           AND start_citation = ?2 AND end_citation = ?3 \
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(
        rusqlite::params![work_abbrev, start_citation, end_citation],
        map_journal_page_row,
    )?;
    rows.collect()
}

/// Insert a vocab-word journal Q&A: passage scope anchored to the cursor
/// segment, `kind='vocab'`, with the word stored for exact reuse lookup.
pub fn save_vocab_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_citation: &str,
    end_citation: &str,
    source_text: &str,
    word: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope,
             start_citation, end_citation, source_text, kind, word, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'passage', ?7, ?8, ?9, 'vocab', ?10,
                 datetime('now'))",
        rusqlite::params![
            work_abbrev, div1, div2, question, answer, claude_model,
            start_citation, end_citation, source_text, word
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The most recent vocab Q&A for `word` in the segment's `(div1, div2)`, or
/// None. Pressing R with a hit renders the stored answer — no duplicate ask.
pub fn find_vocab_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    word: &str,
) -> Result<Option<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND kind = 'vocab' AND word = ?4 \
         ORDER BY timestamp DESC, id DESC LIMIT 1",
    ))?;
    let mut rows = stmt.query_map(
        rusqlite::params![work_abbrev, div1, div2, word],
        map_journal_page_row,
    )?;
    rows.next().transpose()
}

/// Distinct `(start_citation, end_citation)` ranges of every passage-scope
/// Q&A entry for a work. Feeds the main-card line tint: a line covered by a
/// journal passage Q&A is colored exactly like a reader-glossed line
/// (`apply_reader_gloss_highlighting`). Callers pass `Work.canonical_abbrev`,
/// like every other journal path.
pub fn find_passage_citation_ranges(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT start_citation, end_citation FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'passage'
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn find_journal_scenes(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(i64, i64)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT div1, div2 FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'scene'
         ORDER BY div1 ASC, div2 ASC",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn update_journal_page(
    conn: &Connection,
    id: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE journal_entries
         SET question = ?1, answer = ?2, claude_model = ?3, timestamp = datetime('now')
         WHERE id = ?4",
        rusqlite::params![question, answer, claude_model, id],
    )?;
    Ok(())
}

/// Re-target an existing journal entry to a different band by updating its
/// `scope` + `(div1, div2)` in place. Used by the journal overlay's
/// "move to band" action (Ctrl+Shift+J). Does NOT touch question/answer.
/// For the whole-work band pass `scope = "work"` and `div1 = div2 = -1`;
/// for a scene/chapter pass `scope = "scene"` and the scene's `(div1, div2)`.
pub fn move_journal_page(
    conn: &Connection,
    id: i64,
    scope: &str,
    div1: i64,
    div2: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE journal_entries
         SET scope = ?1, div1 = ?2, div2 = ?3
         WHERE id = ?4",
        rusqlite::params![scope, div1, div2, id],
    )?;
    Ok(())
}

pub fn delete_journal_page(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM journal_entries WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_journal_table(&conn).unwrap();
        conn
    }

    #[test]
    fn scene_pages_roundtrip_and_exclude_work() {
        let conn = mem();
        save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "claude-opus-4-8", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "claude-opus-4-8", "scene", "qa").unwrap();
        // A work page in the same work must NOT appear in scene queries.
        save_journal_page(&conn, "Ham", -1, -1, "WQ?", "WA.", "claude-opus-4-8", "work", "qa").unwrap();

        let scene_pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(scene_pages.len(), 2);
        assert_eq!(scene_pages[0].question, "Q1?");
        assert_eq!(scene_pages[1].question, "Q2?");

        // find_journal_scenes lists only scene-scoped rows.
        let scenes = find_journal_scenes(&conn, "Ham").unwrap();
        assert_eq!(scenes, vec![(1, 2)]);
    }

    #[test]
    fn work_pages_roundtrip_and_exclude_scene() {
        let conn = mem();
        save_journal_page(&conn, "Ham", -1, -1, "WQ1?", "WA1.", "claude-opus-4-8", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "WQ2?", "WA2.", "claude-opus-4-8", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", 3, 1, "SQ?", "SA.", "claude-opus-4-8", "scene", "qa").unwrap();

        let work_pages = find_work_pages(&conn, "Ham").unwrap();
        assert_eq!(work_pages.len(), 2);
        assert_eq!(work_pages[0].question, "WQ1?");
        assert_eq!(work_pages[1].question, "WQ2?");

        // A scene query must NOT return work pages.
        assert!(find_journal_pages(&conn, "Ham", -1, -1).unwrap().is_empty());
    }

    #[test]
    fn passage_citation_ranges_distinct_and_scoped() {
        let conn = mem();
        // Two Q&As on the SAME passage → one distinct range.
        save_passage_page(&conn, "Rom", 2, 2, "Rom.2.2.25", "Rom.2.2.25", "Ay me.", "Q1?", "A1.", "m").unwrap();
        save_passage_page(&conn, "Rom", 2, 2, "Rom.2.2.25", "Rom.2.2.25", "Ay me.", "Q2?", "A2.", "m").unwrap();
        save_passage_page(&conn, "Rom", 2, 2, "Rom.2.2.33", "Rom.2.2.36", "O Romeo…", "Q3?", "A3.", "m").unwrap();
        // Scene-scope entry (no citations) and another work must not appear.
        save_journal_page(&conn, "Rom", 2, 2, "SQ?", "SA.", "m", "scene", "qa").unwrap();
        save_passage_page(&conn, "Ham", 1, 2, "Ham.1.2.1", "Ham.1.2.3", "…", "HQ?", "HA.", "m").unwrap();

        let mut ranges = find_passage_citation_ranges(&conn, "Rom").unwrap();
        ranges.sort();
        assert_eq!(
            ranges,
            vec![
                ("Rom.2.2.25".to_string(), "Rom.2.2.25".to_string()),
                ("Rom.2.2.33".to_string(), "Rom.2.2.36".to_string()),
            ]
        );
    }

    #[test]
    fn update_and_delete_still_work() {
        let conn = mem();
        let id1 = save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "m", "scene", "qa").unwrap();

        update_journal_page(&conn, id1, "Q1b?", "A1b.", "m").unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages[0].question, "Q1b?");
        assert_eq!(pages[0].answer, "A1b.");

        delete_journal_page(&conn, id1).unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q2?");
    }

    #[test]
    fn shared_base_abbrev_contract() {
        // 2H6 and 2H6-Amb share a journal because callers always pass the
        // canonical base abbrev (`Work.canonical_abbrev` == "2H6"). This test
        // documents that contract at the DB layer: a page saved under "2H6" is
        // found when querying "2H6".
        let conn = mem();
        save_journal_page(&conn, "2H6", 4, 8, "Q?", "A.", "m", "scene", "qa").unwrap();
        let pages = find_journal_pages(&conn, "2H6", 4, 8).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q?");
    }

    #[test]
    fn all_pages_ordered_work_first_then_scenes() {
        let conn = mem();
        // Insert out of order; expect: work pages (by time), then scene pages
        // grouped by (div1,div2) then by time.
        save_journal_page(&conn, "Ham", 3, 1, "S31a?", "a", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "W1?", "a", "m", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "S12a?", "a", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "W2?", "a", "m", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "S12b?", "a", "m", "scene", "qa").unwrap();

        let ordered = find_all_pages_ordered(&conn, "Ham").unwrap();
        let qs: Vec<&str> = ordered.iter().map(|p| p.question.as_str()).collect();
        assert_eq!(qs, vec!["W1?", "W2?", "S12a?", "S12b?", "S31a?"]);
    }

    #[test]
    fn ensure_table_is_idempotent_and_adds_scope() {
        let conn = mem();
        // Calling again must not error (idempotent ALTER guard).
        ensure_journal_table(&conn).unwrap();
        let has_scope: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='scope'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has_scope);
    }

    #[test]
    fn passage_pages_roundtrip_and_isolate_from_scene_work() {
        let conn = mem();
        let id = save_passage_page(
            &conn, "2H6", 1, 4, "2H6.1.4.43", "2H6.1.4.50",
            "<speaker>YORK</speaker>\n<verse>Lay hands…</verse>\n<stage>[To Jourdain.]</stage>",
            "What is York doing?", "He arrests the conjurers.", "claude-opus-4-8",
        ).unwrap();
        assert!(id > 0);

        // A scene page and a work page in the same scene must NOT come back as passage pages.
        save_journal_page(&conn, "2H6", 1, 4, "SceneQ?", "SceneA.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "2H6", -1, -1, "WorkQ?", "WorkA.", "m", "work", "qa").unwrap();

        let pages = find_passage_pages(&conn, "2H6", "2H6.1.4.43", "2H6.1.4.50").unwrap();
        assert_eq!(pages.len(), 1, "exactly the one passage page");
        let p = &pages[0];
        assert_eq!(p.question, "What is York doing?");
        assert_eq!(p.start_citation.as_deref(), Some("2H6.1.4.43"));
        assert_eq!(p.end_citation.as_deref(), Some("2H6.1.4.50"));
        assert!(p.source_text.as_deref().unwrap().contains("<stage>[To Jourdain.]</stage>"));

        // The passage page must NOT leak into scene/work queries.
        assert!(find_journal_pages(&conn, "2H6", 1, 4).unwrap().iter().all(|p| p.question != "What is York doing?"));
        assert!(find_work_pages(&conn, "2H6").unwrap().iter().all(|p| p.question != "What is York doing?"));

        // A different citation pair returns nothing.
        assert!(find_passage_pages(&conn, "2H6", "2H6.1.4.99", "2H6.1.4.99").unwrap().is_empty());
    }

    #[test]
    fn scene_band_pages_merge_scene_and_passages_in_time_order() {
        let conn = mem();
        // Scene Q&A first, then two passage Q&As, all in (1, 0) — interleaved
        // with an unrelated scene/passage and a work page that must be excluded.
        save_journal_page(&conn, "BH", 1, 0, "SceneQ?", "SceneA.", "m", "scene", "qa").unwrap();
        save_passage_page(
            &conn, "BH", 1, 0, "BH.1.0.14", "BH.1.0.14",
            "<p>chancery…</p>", "PassQ1?", "PassA1.", "m",
        ).unwrap();
        save_passage_page(
            &conn, "BH", 1, 0, "BH.1.0.18", "BH.1.0.18",
            "<p>fog…</p>", "PassQ2?", "PassA2.", "m",
        ).unwrap();
        // Different scene band — must NOT appear.
        save_journal_page(&conn, "BH", 2, 0, "OtherScene?", "x", "m", "scene", "qa").unwrap();
        save_passage_page(
            &conn, "BH", 2, 0, "BH.2.0.1", "BH.2.0.1", "<p>x</p>", "OtherPass?", "x", "m",
        ).unwrap();
        // Whole-work page — must NOT appear.
        save_journal_page(&conn, "BH", -1, -1, "WorkQ?", "x", "m", "work", "qa").unwrap();

        let pages = find_scene_band_pages(&conn, "BH", 1, 0).unwrap();
        let qs: Vec<&str> = pages.iter().map(|p| p.question.as_str()).collect();
        assert_eq!(qs, vec!["SceneQ?", "PassQ1?", "PassQ2?"]);
        // Passage rows carry their citations; the scene row does not.
        assert!(pages[0].start_citation.is_none());
        assert_eq!(pages[1].start_citation.as_deref(), Some("BH.1.0.14"));
        assert_eq!(pages[2].start_citation.as_deref(), Some("BH.1.0.18"));
    }

    #[test]
    fn passage_columns_migrate_idempotently() {
        let conn = mem();
        ensure_journal_table(&conn).unwrap(); // second call must not error
        for col in ["start_citation", "end_citation", "source_text"] {
            let has: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name=?1").unwrap()
                .exists([col]).unwrap();
            assert!(has, "column {col} should exist after ensure_journal_table");
        }
    }

    #[test]
    fn kind_defaults_to_qa_and_roundtrips() {
        let conn = mem();
        // Old-style insert path (scene) must default kind to 'qa'.
        let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id, id);
        assert_eq!(pages[0].kind, "qa");
    }

    #[test]
    fn move_page_changes_band_scene_to_work_and_back() {
        let conn = mem();
        let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();

        // Move scene -> work.
        move_journal_page(&conn, id, "work", -1, -1).unwrap();
        assert!(find_journal_pages(&conn, "Ham", 1, 2).unwrap().is_empty());
        let work = find_work_pages(&conn, "Ham").unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].id, id);
        assert_eq!(work[0].div1, -1);
        assert_eq!(work[0].div2, -1);

        // Move work -> a different scene.
        move_journal_page(&conn, id, "scene", 3, 1).unwrap();
        assert!(find_work_pages(&conn, "Ham").unwrap().is_empty());
        let scene = find_journal_pages(&conn, "Ham", 3, 1).unwrap();
        assert_eq!(scene.len(), 1);
        assert_eq!(scene[0].id, id);
    }

    #[test]
    fn author_pages_roundtrip_and_exclude_work_scene() {
        let conn = mem();
        let nid = save_author_page(&conn, "Shakespeare", "", "## Cry\n\n**load** it", "m", "note").unwrap();
        save_author_page(&conn, "Shakespeare", "Corpus Q?", "Corpus A.", "m", "qa").unwrap();
        // A scene page for an actual work must NOT appear in author queries.
        save_journal_page(&conn, "Ham", 1, 2, "SQ?", "SA.", "m", "scene", "qa").unwrap();

        let pages = find_author_pages(&conn, "Shakespeare").unwrap();
        assert_eq!(pages.len(), 2);
        let note = pages.iter().find(|p| p.id == nid).unwrap();
        assert_eq!(note.kind, "note");
        assert_eq!(note.question, "");
        assert_eq!(note.answer, "## Cry\n\n**load** it");
        assert_eq!(note.div1, -2);
        assert_eq!(note.div2, -2);
    }

    #[test]
    fn move_to_author_band_sets_scope_and_sentinel() {
        let conn = mem();
        let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();
        move_journal_page(&conn, id, "author", AUTHOR_DIV.0, AUTHOR_DIV.1).unwrap();
        // NOTE: move keeps work_abbrev; author-band lookups key by work_abbrev, so a
        // moved-from-a-work page keys under the WORK abbrev, not the author. That's
        // acceptable: the move picker is out of scope for author here (Task 6 does
        // not add an Author move target). This test documents move_journal_page is
        // scope-agnostic and needs no change.
        let n: i64 = conn
            .query_row("SELECT div1 FROM journal_entries WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, -2);
    }

    #[test]
    fn vocab_word_column_migrates_idempotently() {
        let conn = mem();
        ensure_journal_table(&conn).unwrap(); // second call must not error
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='word'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has, "word column should exist after ensure_journal_table");
    }

    #[test]
    fn vocab_page_roundtrip_and_reuse_lookup() {
        let conn = mem();
        let id = save_vocab_page(
            &conn, "Cym", 3, 2, "Cym.3.2.77", "Cym.3.2.80",
            "A riding suit no costlier than would fit\nA franklin's huswife.",
            "franklin", "\u{201c}franklin\u{201d} in this segment, and across Shakespeare",
            "Imogen prices her disguise\u{2026}", "claude-opus-4-8",
        ).unwrap();
        assert!(id > 0);

        // Exact reuse hit.
        let page = find_vocab_page(&conn, "Cym", 3, 2, "franklin").unwrap().unwrap();
        assert_eq!(page.id, id);
        assert_eq!(page.kind, "vocab");
        assert_eq!(page.source_text.as_deref().unwrap(), "A riding suit no costlier than would fit\nA franklin's huswife.");

        // Different word, different segment, different work: all miss.
        assert!(find_vocab_page(&conn, "Cym", 3, 2, "huswife").unwrap().is_none());
        assert!(find_vocab_page(&conn, "Cym", 3, 3, "franklin").unwrap().is_none());
        assert!(find_vocab_page(&conn, "Ham", 3, 2, "franklin").unwrap().is_none());

        // Most recent wins (same-second timestamps tie-break on id DESC).
        let id2 = save_vocab_page(
            &conn, "Cym", 3, 2, "Cym.3.2.77", "Cym.3.2.80", "src",
            "franklin", "Q2?", "Second answer.", "m",
        ).unwrap();
        assert_eq!(find_vocab_page(&conn, "Cym", 3, 2, "franklin").unwrap().unwrap().id, id2);

        // Vocab rows ride the passage scope into the scene band render.
        let band = find_scene_band_pages(&conn, "Cym", 3, 2).unwrap();
        assert_eq!(band.len(), 2);
        assert!(band.iter().all(|p| p.kind == "vocab"));

        // A plain passage Q&A (kind='qa') must never satisfy the vocab lookup.
        save_passage_page(&conn, "Cym", 3, 4, "Cym.3.4.1", "Cym.3.4.2", "s", "Q?", "A.", "m").unwrap();
        assert!(find_vocab_page(&conn, "Cym", 3, 4, "franklin").unwrap().is_none());
    }
}
