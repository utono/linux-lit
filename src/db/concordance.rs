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

/// Load all content words (minus stopwords) from the author's works.
/// Returns a deduplicated, alphabetically sorted list.
pub fn load_concordance_words(
    conn: &Connection,
    author: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    use std::collections::HashSet;
    use crate::db::stopwords::STOPWORDS;

    let stopwords: HashSet<&str> = STOPWORDS.iter().copied().collect();

    let mut stmt = conn.prepare(
        "SELECT lm.normalized_text
         FROM line_mapping lm
         JOIN works w ON w.abbrev = lm.work_abbrev
         WHERE w.author = ?1",
    )?;
    let rows = stmt.query_map([author], |row| row.get::<_, String>(0))?;

    let mut words: HashSet<String> = HashSet::new();
    for row in rows {
        let line = row?;
        for token in line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}') {
            let lower = token.to_lowercase();
            if lower.len() >= 2 && !stopwords.contains(lower.as_str()) {
                words.insert(lower);
            }
        }
    }

    let mut result: Vec<String> = words.into_iter().collect();
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let home = std::env::var("HOME").unwrap_or_default();
        let db_path = format!("{}/utono/litdb/data/lit.db", home);
        Connection::open(&db_path).expect("Failed to open lit.db for tests")
    }

    #[test]
    fn concordance_words_excludes_stopwords() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare").unwrap();
        // Stopwords should not appear
        assert!(!words.contains(&"the".to_string()));
        assert!(!words.contains(&"and".to_string()));
        assert!(!words.contains(&"is".to_string()));
        // Content words should appear
        assert!(words.contains(&"time".to_string()));
        assert!(words.contains(&"love".to_string()));
    }

    #[test]
    fn concordance_words_sorted_alphabetically() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare").unwrap();
        let mut sorted = words.clone();
        sorted.sort();
        assert_eq!(words, sorted);
    }
}
