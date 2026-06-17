//! Read the active Claude-API prompt for a given key from lit.db `api_prompts`.

use rusqlite::{Connection, OptionalExtension};

/// Read the active prompt text for `key` from an explicit connection.
/// Returns `None` if no active row exists or on any query error.
pub fn active_prompt_in(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT text FROM api_prompts WHERE prompt_key = ?1 AND is_active = 1 \
         ORDER BY version DESC LIMIT 1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

/// Open lit.db read-only and return the active prompt for `key`, or `None`
/// (missing row, missing table, or DB unavailable — caller falls back).
pub fn active_prompt(key: &str) -> Option<String> {
    let conn = crate::db::queries::open_db().ok()?;
    active_prompt_in(&conn, key)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE api_prompts (id INTEGER PRIMARY KEY, prompt_key TEXT, \
             version INTEGER, text TEXT, is_active INTEGER, note TEXT, \
             created_at TEXT);",
        )
        .unwrap();
    }

    #[test]
    fn returns_active_text() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute(
            "INSERT INTO api_prompts(prompt_key,version,text,is_active) \
             VALUES('k',1,'old',0),('k',2,'new',1)",
            [],
        )
        .unwrap();
        assert_eq!(super::active_prompt_in(&conn, "k"), Some("new".to_string()));
    }

    #[test]
    fn returns_none_when_absent() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert_eq!(super::active_prompt_in(&conn, "missing"), None);
    }
}
