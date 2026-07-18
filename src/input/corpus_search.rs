//! Pure, GTK-free cross-corpus regex filtering for the Ctrl+f search popup.
//! Mirrors the pure/gtk split of `overlay_search`. Matching reuses
//! `search::build_matcher` (smart-case, literal fallback).

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Corpus { Journal, Gloss }

#[derive(Clone, Debug)]
pub struct JournalRow {
    pub id: i64, pub work_abbrev: String,
    pub div1: i32, pub div2: i32,
    pub question: String, pub answer: String,
}

#[derive(Clone, Debug)]
pub struct GlossRow {
    pub gloss_id: i64, pub work_abbrev: String,
    pub start_citation: String, pub speaker: String, pub gloss_text: String,
}

#[derive(Clone, Debug)]
pub struct CorpusHit {
    pub corpus: Corpus,
    pub entry_id: i64,
    pub work_abbrev: String,
    pub label: String,
    pub sort_key: (String, i32, i32),
}

/// Row label: "Cym 5.5  <question first line>".
pub fn journal_label(row: &JournalRow) -> String {
    let q = row.question.lines().next().unwrap_or("").trim();
    format!("{} {}.{}  {}", row.work_abbrev, row.div1, row.div2, q)
}

/// Row label: "Cym.5.5.1  BELARIUS  <gloss first line>".
pub fn gloss_label(row: &GlossRow) -> String {
    let g = row.gloss_text.lines().next().unwrap_or("").trim();
    format!("{}  {}  {}", row.start_citation, row.speaker, g)
}

pub fn filter_journal(rows: &[JournalRow], re: &regex::Regex) -> Vec<CorpusHit> {
    let mut hits: Vec<CorpusHit> = rows
        .iter()
        .filter(|r| re.is_match(&r.question) || re.is_match(&r.answer))
        .map(|r| CorpusHit {
            corpus: Corpus::Journal,
            entry_id: r.id,
            work_abbrev: r.work_abbrev.clone(),
            label: journal_label(r),
            sort_key: (r.work_abbrev.clone(), r.div1, r.div2),
        })
        .collect();
    hits.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    hits
}

pub fn filter_gloss(rows: &[GlossRow], re: &regex::Regex) -> Vec<CorpusHit> {
    let mut hits: Vec<CorpusHit> = rows
        .iter()
        .filter(|r| re.is_match(&r.gloss_text))
        .map(|r| CorpusHit {
            corpus: Corpus::Gloss,
            entry_id: r.gloss_id,
            work_abbrev: r.work_abbrev.clone(),
            label: gloss_label(r),
            // Sort glosses by (work, then citation string via a stable proxy):
            // reuse start_citation lexical order by hashing act/scene out is
            // overkill — sort by (work, 0, 0) keeps DB order within a work.
            sort_key: (r.work_abbrev.clone(), 0, 0),
        })
        .collect();
    hits.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::search::build_matcher;

    fn jrow(id: i64, q: &str, a: &str) -> JournalRow {
        JournalRow { id, work_abbrev: "Cym".into(), div1: 5, div2: 5,
            question: q.into(), answer: a.into() }
    }
    fn grow(id: i64, cite: &str, spk: &str, text: &str) -> GlossRow {
        GlossRow { gloss_id: id, work_abbrev: "Cym".into(),
            start_citation: cite.into(), speaker: spk.into(), gloss_text: text.into() }
    }

    #[test]
    fn journal_matches_question_or_answer() {
        let rows = vec![
            jrow(1, "About paganism", "nothing here"),
            jrow(2, "unrelated", "the beatitude appears"),
            jrow(3, "no", "match"),
        ];
        let hits = filter_journal(&rows, &build_matcher("pagan|beatitude"));
        let ids: Vec<i64> = hits.iter().map(|h| h.entry_id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn one_hit_per_entry_even_with_multiple_matches() {
        // "fair" appears twice in the answer -> still ONE hit.
        let rows = vec![jrow(1, "q", "the fair root and the fair name")];
        assert_eq!(filter_journal(&rows, &build_matcher("fair")).len(), 1);
    }

    #[test]
    fn empty_query_returns_all_rows() {
        let rows = vec![jrow(1, "a", "b"), jrow(2, "c", "d")];
        assert_eq!(filter_journal(&rows, &build_matcher("")).len(), 2);
    }

    #[test]
    fn smart_case_lowercase_is_insensitive() {
        let rows = vec![jrow(1, "Belarius speaks", "x")];
        assert_eq!(filter_journal(&rows, &build_matcher("belarius")).len(), 1);
    }

    #[test]
    fn smart_case_uppercase_is_sensitive() {
        let rows = vec![jrow(1, "belarius speaks", "x")];
        assert_eq!(filter_journal(&rows, &build_matcher("Belarius")).len(), 0);
    }

    #[test]
    fn invalid_regex_falls_back_to_literal() {
        // Unclosed '(' -> build_matcher escapes it to a literal.
        let rows = vec![jrow(1, "has a (paren here", "x")];
        assert_eq!(filter_journal(&rows, &build_matcher("(paren")).len(), 1);
    }

    #[test]
    fn gloss_matches_body_text() {
        let rows = vec![
            grow(1, "Cym.5.5.1", "BELARIUS", "a note on nobility"),
            grow(2, "Cym.5.5.9", "CYMBELINE", "unrelated"),
        ];
        let hits = filter_gloss(&rows, &build_matcher("nobility"));
        assert_eq!(hits.iter().map(|h| h.entry_id).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn hits_carry_corpus_and_label() {
        let hits = filter_journal(&[jrow(7, "the question text", "ans")],
            &build_matcher("question"));
        assert_eq!(hits[0].corpus, Corpus::Journal);
        assert!(hits[0].label.contains("question text"));
    }
}
