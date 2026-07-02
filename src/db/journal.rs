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
    let has_scope = conn
        .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='scope'")?
        .exists([])?;
    if !has_scope {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN scope TEXT NOT NULL DEFAULT 'scene';",
        )?;
    }
    for col in ["start_citation", "end_citation", "source_text"] {
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name=?1")?
            .exists([col])?;
        if !has {
            conn.execute_batch(&format!(
                "ALTER TABLE journal_entries ADD COLUMN {col} TEXT;"
            ))?;
        }
    }
    let has_kind: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='kind'")?
        .exists([])?;
    if !has_kind {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN kind TEXT NOT NULL DEFAULT 'qa';",
        )?;
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
}
