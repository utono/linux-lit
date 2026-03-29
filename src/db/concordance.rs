use rusqlite::Connection;

/// A hit from the cross-work concordance search.
#[derive(Debug, Clone)]
pub struct ConcordanceRow {
    pub line_mapping_id: i64,
    pub work_abbrev: String,
    pub title: String,
    pub author: String,
    pub div1: i64,
    pub div2: i64,
    pub line_in_div: i64,
    pub canonical_text: String,
    pub has_audio: bool,
}

/// Find all lines containing `word` across all works with line_mapping entries.
/// Results ordered by author, work, position.
pub fn find_word_occurrences(
    conn: &Connection,
    word: &str,
) -> Result<Vec<ConcordanceRow>, rusqlite::Error> {
    let pattern = format!("%{}%", word.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT lm.id, lm.work_abbrev, w.title, w.author,
                lm.div1, COALESCE(lm.div2, 0), lm.line_in_div, lm.canonical_text,
                EXISTS(
                    SELECT 1 FROM line_timestamps lt WHERE lt.line_mapping_id = lm.id
                ) AS has_audio
         FROM line_mapping lm
         JOIN works w ON w.abbrev = lm.work_abbrev
         WHERE lm.normalized_text LIKE ?1
         ORDER BY w.author, lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div",
    )?;
    let rows = stmt.query_map([&pattern], |row| {
        Ok(ConcordanceRow {
            line_mapping_id: row.get(0)?,
            work_abbrev: row.get(1)?,
            title: row.get(2)?,
            author: row.get(3)?,
            div1: row.get(4)?,
            div2: row.get(5)?,
            line_in_div: row.get(6)?,
            canonical_text: row.get(7)?,
            has_audio: row.get::<_, i64>(8)? != 0,
        })
    })?;
    rows.collect()
}

/// Load all vocab words globally (for the cross-work concordance word picker).
/// Returns (word, 0) across all works. Count is deferred to avoid expensive LIKE join.
pub fn load_global_vocab_words(
    conn: &Connection,
) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT word FROM vocab_words ORDER BY word",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, 0usize))
    })?;
    rows.collect()
}
