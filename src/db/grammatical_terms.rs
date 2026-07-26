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
