//! Definitions of grammatical structures used by syntax glosses.
//!
//! Reference data, not passage data: "main clause" means the same thing in
//! every sentence, so it is stored once here rather than re-derived by the
//! model on every gloss.

use rusqlite::Connection;

/// Every known term and its definition, ordered by term.
///
/// Returns EMPTY on any error rather than propagating: a definitions table
/// being unreadable must not cost the reader their analysis. The caller
/// degrades to a gloss with no Terms section.
pub fn load_all(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = match conn
        .prepare("SELECT term, definition FROM grammatical_terms ORDER BY term")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Insert terms not already present. Returns how many rows were added.
///
/// `INSERT OR IGNORE`: the stored definition wins over a newly supplied one.
/// Consistency beats recency — a term that already has a definition should
/// keep it, or two glosses of the same passage could disagree.
///
/// Needs a WRITE connection (`open_db_rw`); the shared `open_db` is opened
/// `SQLITE_OPEN_READ_ONLY`, under which every insert here silently fails.
pub fn insert_missing(conn: &Connection, terms: &[(String, String)]) -> usize {
    let mut added = 0usize;
    for (term, def) in terms {
        if term.trim().is_empty() || def.trim().is_empty() {
            continue;
        }
        let n = conn.execute(
            "INSERT OR IGNORE INTO grammatical_terms (term, definition, source)
             VALUES (?1, ?2, 'claude')",
            rusqlite::params![term.trim(), def.trim()],
        );
        added += n.unwrap_or(0);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table with the real shape, in memory — no lit.db, no display.
    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute(
            "CREATE TABLE grammatical_terms (
                id INTEGER PRIMARY KEY,
                term TEXT UNIQUE NOT NULL,
                definition TEXT NOT NULL,
                source TEXT,
                created_at TEXT DEFAULT (datetime('now')))",
            [],
        )
        .expect("create");
        conn
    }

    #[test]
    fn insert_missing_adds_new_terms_and_reports_the_count() {
        // The path that had never executed in a real run: every gloss so far
        // used only seeded terms, so a broken insert would have stayed
        // invisible. `open_db` is READ-ONLY in this codebase, and
        // `unwrap_or(0)` swallows the resulting error — a caller using the
        // wrong opener logs "0 inserted" and looks healthy forever.
        let conn = db();
        let added = insert_missing(
            &conn,
            &[("periodic sentence".into(), "a sentence whose main clause is withheld".into())],
        );
        assert_eq!(added, 1);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM grammatical_terms", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 1);
    }

    #[test]
    fn insert_missing_keeps_the_stored_definition_on_a_duplicate() {
        // Consistency beats recency: two glosses of the same passage must not
        // disagree because the second one re-defined a term.
        let conn = db();
        insert_missing(&conn, &[("main clause".into(), "the original".into())]);
        let added = insert_missing(&conn, &[("main clause".into(), "a rewrite".into())]);
        assert_eq!(added, 0, "duplicate must not insert");
        let def: String = conn
            .query_row("SELECT definition FROM grammatical_terms WHERE term='main clause'", [], |r| r.get(0))
            .expect("definition");
        assert_eq!(def, "the original", "stored definition must win");
    }

    #[test]
    fn insert_missing_skips_blank_terms_and_definitions() {
        let conn = db();
        let added = insert_missing(
            &conn,
            &[("".into(), "no term".into()), ("no definition".into(), "   ".into())],
        );
        assert_eq!(added, 0);
    }

    #[test]
    fn load_all_returns_terms_alphabetically() {
        let conn = db();
        insert_missing(
            &conn,
            &[
                ("subject".into(), "s".into()),
                ("appositive".into(), "a".into()),
                ("main clause".into(), "m".into()),
            ],
        );
        let got = load_all(&conn);
        let terms: Vec<&str> = got.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(terms, vec!["appositive", "main clause", "subject"]);
    }

    #[test]
    fn load_all_is_empty_when_the_table_is_missing() {
        // A definitions table being unreadable must not cost the reader their
        // analysis — the caller degrades to a gloss with no Terms section.
        let conn = Connection::open_in_memory().expect("in-memory db");
        assert!(load_all(&conn).is_empty());
    }
}
