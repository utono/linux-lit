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
         ORDER BY lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div, lm.sub_line",
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
    use std::collections::HashMap;

    fn test_conn() -> Connection {
        // Honor LIT_DB_PATH so the suite can run against a copy of the DB
        // (e.g. a migrated copy), matching queries::db_path()'s convention.
        let db_path = std::env::var("LIT_DB_PATH").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{}/utono/litdb/data/lit.db", home)
        });
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

    /// Build a line map for a work, replicating the app's prepare_text_for_display logic.
    fn build_line_map_for_work(work: &crate::db::models::Work) -> Option<crate::text_file_map::LineMap> {
        let path = work.text_file.as_ref()?;
        let contents = std::fs::read_to_string(path).ok()?;
        let file_lines: Vec<String> = contents.lines().map(String::from).collect();
        let cleaned_lines: Vec<String> = {
            let mut result: Vec<String> = Vec::with_capacity(file_lines.len());
            for (i, line) in file_lines.iter().enumerate() {
                if crate::db::line_types::is_blank(line) {
                    let next_non_blank = file_lines[i + 1..]
                        .iter()
                        .find(|l| !crate::db::line_types::is_blank(l));
                    if let Some(next) = next_non_blank {
                        if crate::db::line_types::is_speaker(next) {
                            continue;
                        }
                    }
                }
                if let Some(stripped) = line.strip_prefix("## ") {
                    result.push(stripped.to_string());
                } else {
                    result.push(line.clone());
                }
            }
            result
        };
        let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
        Some(crate::text_file_map::build_line_map(&cleaned_lines, &work.lines, is_prose))
    }

    /// Replicate concordance_resolve_indices logic: given a work + line_map,
    /// resolve a line_mapping_id to a buffer index. Returns None if unmatched.
    fn resolve_buf_idx(
        work: &crate::db::models::Work,
        line_map: &crate::text_file_map::LineMap,
        line_mapping_id: i64,
    ) -> Option<usize> {
        let work_idx = work.lines.iter().position(|l| l.id == line_mapping_id)?;
        let bi = line_map.work_to_buffer[work_idx];
        if line_map.buffer_to_work.get(bi) == Some(&Some(work_idx)) {
            Some(bi)
        } else {
            None
        }
    }

    /// Pick random concordance words, walk every hit, verify that resolved
    /// buffer lines contain the search word (or are correctly skipped when
    /// the line map can't match them).
    #[test]
    fn concordance_hits_resolve_to_correct_lines() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare").unwrap();
        assert!(!words.is_empty());

        // Pick 10 words spread across the list
        let step = words.len() / 10;
        let test_words: Vec<&str> = (0..10)
            .map(|i| words[i * step].0.as_str())
            .collect();

        // Cache loaded works to avoid re-loading
        let mut work_cache: HashMap<String, (crate::db::models::Work, crate::text_file_map::LineMap)> =
            HashMap::new();

        let mut total_hits = 0usize;
        let mut resolved = 0usize;
        let mut skipped_unmatched = 0usize;
        let mut skipped_no_text_file = 0usize;
        let mut wrong_line = Vec::new();

        for word in &test_words {
            let hits = find_word_occurrences(&conn, word, "Shakespeare").unwrap();
            total_hits += hits.len();

            for hit in &hits {
                if !work_cache.contains_key(&hit.work_abbrev) {
                    let work = crate::db::queries::load_work(&conn, &hit.work_abbrev).unwrap();
                    if let Some(lm) = build_line_map_for_work(&work) {
                        work_cache.insert(hit.work_abbrev.clone(), (work, lm));
                    } else {
                        skipped_no_text_file += hits.len();
                        continue;
                    }
                }

                let (work, lm) = work_cache.get(&hit.work_abbrev).unwrap();
                match resolve_buf_idx(work, lm, hit.line_mapping_id) {
                    Some(buf_idx) => {
                        resolved += 1;
                        let work_idx = work.lines.iter().position(|l| l.id == hit.line_mapping_id).unwrap();
                        let normalized = work.lines[work_idx].normalized.to_lowercase();
                        if !normalized.contains(&word.to_lowercase()) {
                            wrong_line.push(format!(
                                "word='{}' line_id={} abbrev='{}' buf_idx={} normalized='{}'",
                                word, hit.line_mapping_id, hit.work_abbrev, buf_idx,
                                &work.lines[work_idx].normalized,
                            ));
                        }
                    }
                    None => {
                        skipped_unmatched += 1;
                    }
                }
            }
        }

        eprintln!(
            "concordance resolve: {} words, {} total hits, {} resolved, {} skipped (unmatched), {} skipped (no text file)",
            test_words.len(), total_hits, resolved, skipped_unmatched, skipped_no_text_file,
        );
        eprintln!("  words: {:?}", test_words);

        assert!(
            wrong_line.is_empty(),
            "Resolved lines that don't contain the concordance word:\n{}",
            wrong_line.join("\n"),
        );
        assert!(resolved > 0, "Expected at least some hits to resolve");
        // buf_idx=0 should never appear for unmatched lines (the old bug)
        // This is implicitly tested: resolve_buf_idx returns None for unmatched.
    }
}
