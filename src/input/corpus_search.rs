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
    /// Primary row text: the question (journal) or the gloss's own prose (gloss),
    /// first line, markup stripped. The scannable content of the row.
    pub label: String,
    /// Secondary, right-aligned column: the work + location (and speaker for a
    /// gloss), rendered dimmed so the primary text reads first.
    pub detail: String,
    pub sort_key: (String, i32, i32),
}

/// Primary label: the question's first line.
pub fn journal_label(row: &JournalRow) -> String {
    row.question.lines().next().unwrap_or("").trim().to_string()
}

/// Detail column: "Cym 5.5" — work abbrev + act.scene.
pub fn journal_detail(row: &JournalRow) -> String {
    format!("{} {}.{}", row.work_abbrev, row.div1, row.div2)
}

/// Primary label: the gloss's own prose, first line, with the leading
/// `<speaker>…</speaker>` / `<segment>` block markup stripped so the row reads as
/// plain text (gloss bodies are stored as markup).
pub fn gloss_label(row: &GlossRow) -> String {
    let clean = strip_gloss_markup(&row.gloss_text);
    clean.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// Detail column: "Cym.5.5.1 · Belarius" — citation + FIRST speaker (the stored
/// speaker field can be a long comma list of everyone in the passage; only the
/// first is kept so the index column stays narrow and the primary prose keeps
/// its width). The speaker is lowercased so the popup's `font-variant: small-caps`
/// renders it as small capitals (already-uppercase letters would stay full-size).
/// Speaker omitted when empty.
pub fn gloss_detail(row: &GlossRow) -> String {
    let first_speaker = row.speaker.split(',').next().unwrap_or("").trim();
    if first_speaker.is_empty() {
        row.start_citation.clone()
    } else {
        format!("{} · {}", row.start_citation, first_speaker.to_lowercase())
    }
}

/// Strip gloss body markup so a row label reads as prose. Drops any
/// `<speaker>…</speaker>` element WHOLE (tag and its inner name — the speaker is
/// shown in the detail column, so it would be redundant noise in the primary
/// text), then removes all remaining `<…>` tags (`<segment>`, etc.), keeping their
/// inner text.
fn strip_gloss_markup(s: &str) -> String {
    let without_speaker = remove_element(s, "speaker");
    let mut out = String::with_capacity(without_speaker.len());
    let mut in_tag = false;
    for c in without_speaker.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Remove every `<tag>…</tag>` element (opening tag, inner content, closing tag)
/// for the given `tag`, case-sensitively. Unclosed opening tags are left for the
/// general tag-stripper to handle.
fn remove_element(s: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        match after_open.find(&close) {
            Some(end) => rest = &after_open[end + close.len()..],
            None => {
                rest = after_open; // unclosed: drop the opening tag, keep the tail
                break;
            }
        }
    }
    out.push_str(rest);
    out
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
            detail: journal_detail(r),
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
            detail: gloss_detail(r),
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
        // Primary label is the question alone (no work prefix — that lives in detail).
        assert_eq!(hits[0].label, "the question text");
    }

    #[test]
    fn journal_detail_is_work_and_location() {
        let hits = filter_journal(&[jrow(7, "q", "ans")], &build_matcher("q"));
        // jrow builds work "Cym", div1 5, div2 5.
        assert_eq!(hits[0].detail, "Cym 5.5");
    }

    #[test]
    fn gloss_label_strips_speaker_markup() {
        // Gloss bodies are markup; the primary label must read as prose, not
        // "<speaker>BELARIUS</speaker>...".
        let rows = vec![grow(1, "Cym.5.5.1", "BELARIUS",
            "<speaker>BELARIUS</speaker>\n<segment>a note on nobility</segment>")];
        let hits = filter_gloss(&rows, &build_matcher("nobility"));
        assert_eq!(hits[0].label, "a note on nobility");
        assert!(!hits[0].label.contains('<'));
    }

    #[test]
    fn gloss_detail_is_citation_and_speaker() {
        let rows = vec![grow(1, "Cym.5.5.1", "BELARIUS", "text with nobility")];
        let hits = filter_gloss(&rows, &build_matcher("nobility"));
        // Speaker lowercased for small-caps rendering; citation stays as-is.
        assert_eq!(hits[0].detail, "Cym.5.5.1 · belarius");
    }

    #[test]
    fn gloss_detail_omits_empty_speaker() {
        let rows = vec![grow(1, "Cym.5.5.1", "", "text with nobility")];
        let hits = filter_gloss(&rows, &build_matcher("nobility"));
        assert_eq!(hits[0].detail, "Cym.5.5.1");
    }

    #[test]
    fn gloss_detail_keeps_only_first_of_many_speakers() {
        // The stored speaker field can list every speaker in the passage; the
        // index column keeps just the first so the primary prose keeps its width.
        let rows = vec![grow(1, "2H6.2.1.43",
            "GLOUCESTER, CARDINAL, KING HENRY, CARDINAL", "text with nobility")];
        let hits = filter_gloss(&rows, &build_matcher("nobility"));
        assert_eq!(hits[0].detail, "2H6.2.1.43 · gloucester");
    }
}
