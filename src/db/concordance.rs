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

/// Find all lines containing `word` within a single author's works.
/// Results ordered by work, position.
pub fn find_word_occurrences(
    conn: &Connection,
    word: &str,
    author: &str,
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
         WHERE w.author = ?1
           AND lm.normalized_text LIKE ?2
         ORDER BY lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div",
    )?;
    let rows = stmt.query_map(rusqlite::params![author, &pattern], |row| {
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
/// Returns deduplicated words with occurrence counts, sorted alphabetically.
pub fn load_concordance_words(
    conn: &Connection,
    author: &str,
) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    use std::collections::HashMap;
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

    let mut counts: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let line = row?;
        for token in line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}') {
            let lower = token.to_lowercase();
            if lower.len() >= 2 && lower.starts_with(|c: char| c.is_alphabetic()) && !stopwords.contains(lower.as_str()) {
                *counts.entry(lower).or_insert(0) += 1;
            }
        }
    }

    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
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
        let word_strs: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
        assert!(!word_strs.contains(&"the"));
        assert!(!word_strs.contains(&"and"));
        assert!(!word_strs.contains(&"is"));
        assert!(word_strs.contains(&"time"));
        assert!(word_strs.contains(&"love"));
    }

    #[test]
    fn concordance_words_sorted_alphabetically() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare").unwrap();
        let names: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn concordance_words_have_counts() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare").unwrap();
        let time_entry = words.iter().find(|(w, _)| w == "time");
        assert!(time_entry.is_some());
        assert!(time_entry.unwrap().1 > 0);
    }

    #[test]
    fn concordance_words_excludes_numeric_only() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare").unwrap();
        let word_strs: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
        assert!(!word_strs.contains(&"2d"));
        assert!(!word_strs.contains(&"6d"));
    }

    #[test]
    fn find_occurrences_filters_by_author() {
        let conn = test_conn();
        let hits = find_word_occurrences(&conn, "love", "Shakespeare").unwrap();
        assert!(!hits.is_empty());
        for hit in &hits {
            assert_eq!(hit.author, "Shakespeare");
        }
    }
}
