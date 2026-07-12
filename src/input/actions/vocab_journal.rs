//! Vocab journal Q&A: ask Claude about the vocab popup's current word in the
//! cursor segment and across the author's corpus; store as a kind='vocab'
//! journal entry and render in the popup. Pure prompt-assembly helpers here
//! are unit-tested; the stateful handlers mirror journal::ask_claude.

/// Max other-work occurrence lines fed to the prompt.
pub(crate) const CORPUS_HITS_CAP: usize = 10;

/// True when `line` contains `word` as a whole token, case-insensitively.
/// Tokenizes like db::concordance::load_concordance_words (apostrophes bind
/// to the token), so "franklin's" matches "franklin" but "heart" never
/// matches "art" — find_word_occurrences uses LIKE '%word%' and needs this
/// post-filter.
pub(crate) fn line_contains_word(line: &str, word: &str) -> bool {
    let word = word.to_lowercase();
    line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
        .map(|t| t.to_lowercase())
        .any(|t| {
            t == word
                || t.strip_suffix("'s") == Some(word.as_str())
                || t.strip_suffix("\u{2019}s") == Some(word.as_str())
        })
}

/// The CORPUS OCCURRENCES block: other-work lines containing the word,
/// grouped under work titles, deduped, capped at `cap` with a "+N more"
/// tail. `current_canonical` excludes the reading work and its media
/// variants (Cym, Cym-Amb, Cym-BBC share the base "Cym").
pub(crate) fn vocab_corpus_block(
    hits: &[crate::db::concordance::ConcordanceRow],
    current_canonical: &str,
    word: &str,
    cap: usize,
) -> String {
    let variant_prefix = format!("{current_canonical}-");
    let mut seen = std::collections::HashSet::new();
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut skipped = 0usize;
    for h in hits {
        if h.work_abbrev == current_canonical || h.work_abbrev.starts_with(&variant_prefix) {
            continue;
        }
        if !line_contains_word(&h.canonical_text, word) {
            continue;
        }
        if !seen.insert((h.work_abbrev.clone(), h.canonical_text.clone())) {
            continue;
        }
        if lines.len() >= cap {
            skipped += 1;
            continue;
        }
        lines.push((
            h.title.clone(),
            format!("  {}.{}.{}: {}", h.div1, h.div2, h.line_in_div, h.canonical_text),
        ));
    }
    if lines.is_empty() {
        return "(none found)".to_string();
    }
    let mut out = String::new();
    let mut last: Option<&str> = None;
    for (title, line) in &lines {
        if last != Some(title.as_str()) {
            if last.is_some() {
                out.push('\n');
            }
            out.push_str(title);
            out.push_str(":\n");
            last = Some(title);
        }
        out.push_str(line);
        out.push('\n');
    }
    if skipped > 0 {
        out.push_str(&format!("(+{skipped} more occurrences not shown)\n"));
    }
    out.trim_end().to_string()
}

/// The one-line question stored as the entry's `question` and shown in the
/// popup's Q line.
pub(crate) fn vocab_question(word: &str, author: &str) -> String {
    format!("\u{201c}{word}\u{201d} in this segment, and across {author}")
}

/// Assemble the vocab Q&A user message (pure; testable without state).
pub(crate) fn vocab_user_message(
    genre: &str,
    title: &str,
    author: &str,
    unit_label: &str,
    scene_label: &str,
    word: &str,
    segment: &str,
    corpus_block: &str,
) -> String {
    format!(
        "Work type: {genre}\nWork: {title} by {author}\n{unit_label}: {scene_label}\nVocabulary word: {word}\n\n\
         Segment (the reader's cursor segment, verbatim):\n{segment}\n\n\
         CORPUS OCCURRENCES \u{2014} lines containing the word elsewhere in {author}'s works:\n{corpus_block}\n\n\
         Reader's request:\nDiscuss the use of \u{201c}{word}\u{201d} in this segment, and how {author} uses the word elsewhere in the corpus.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::concordance::ConcordanceRow;

    fn hit(abbrev: &str, title: &str, d1: i64, d2: i64, line: i64, text: &str) -> ConcordanceRow {
        ConcordanceRow {
            line_mapping_id: 0,
            work_abbrev: abbrev.to_string(),
            title: title.to_string(),
            author: "William Shakespeare".to_string(),
            div1: d1,
            div2: d2,
            line_in_div: line,
            canonical_text: text.to_string(),
            has_audio: false,
        }
    }

    #[test]
    fn line_contains_word_matches_tokens_not_substrings() {
        assert!(line_contains_word("A franklin's huswife.", "franklin"));
        assert!(line_contains_word("There's a franklin in the Wild of Kent", "franklin"));
        assert!(line_contains_word("The Franklin rode on.", "franklin")); // case-insensitive
        assert!(!line_contains_word("My heart is heavy", "art")); // no substrings
        assert!(!line_contains_word("frankincense and myrrh", "franklin"));
    }

    #[test]
    fn corpus_block_excludes_current_work_and_variants() {
        let hits = vec![
            hit("Cym", "Cymbeline", 3, 2, 77, "A franklin's huswife."),
            hit("Cym-Amb", "Cymbeline", 3, 2, 77, "A franklin's huswife."),
            hit("1H4", "Henry IV, Part 1", 2, 1, 55, "There is a franklin in the Wild of Kent"),
        ];
        let block = vocab_corpus_block(&hits, "Cym", "franklin", 10);
        assert!(!block.contains("huswife"), "current work + variants excluded");
        assert!(block.contains("Henry IV, Part 1:"));
        assert!(block.contains("2.1.55: There is a franklin in the Wild of Kent"));
    }

    #[test]
    fn corpus_block_dedupes_filters_and_caps() {
        let mut hits = vec![
            // Duplicate line text in the same work → one entry.
            hit("1H4", "Henry IV, Part 1", 2, 1, 55, "a franklin in the Wild of Kent"),
            hit("1H4", "Henry IV, Part 1", 2, 1, 55, "a franklin in the Wild of Kent"),
            // LIKE-substring false positive → filtered out.
            hit("WT", "The Winter's Tale", 4, 4, 10, "frankincense on the altar"),
        ];
        for i in 0..12 {
            hits.push(hit("MV", "The Merchant of Venice", 1, 1, i, &format!("franklin line {i}")));
        }
        let block = vocab_corpus_block(&hits, "Cym", "franklin", 10);
        assert_eq!(block.matches("a franklin in the Wild of Kent").count(), 1);
        assert!(!block.contains("frankincense"));
        // 1 (1H4) + 12 (MV) unique matching lines, cap 10 → 3 skipped.
        assert!(block.contains("(+3 more occurrences not shown)"), "block was:\n{block}");
    }

    #[test]
    fn corpus_block_empty_says_none_found() {
        let hits = vec![hit("Cym", "Cymbeline", 3, 2, 77, "A franklin's huswife.")];
        assert_eq!(vocab_corpus_block(&hits, "Cym", "franklin", 10), "(none found)");
    }

    #[test]
    fn question_and_user_message_format() {
        let q = vocab_question("franklin", "William Shakespeare");
        assert_eq!(q, "\u{201c}franklin\u{201d} in this segment, and across William Shakespeare");

        let msg = vocab_user_message(
            "play", "Cymbeline", "William Shakespeare", "Scene", "3.2",
            "franklin", "A riding suit no costlier\u{2026}", "(none found)",
        );
        assert!(msg.contains("Work type: play"));
        assert!(msg.contains("Work: Cymbeline by William Shakespeare"));
        assert!(msg.contains("Scene: 3.2"));
        assert!(msg.contains("Vocabulary word: franklin"));
        assert!(msg.contains("Segment (the reader's cursor segment, verbatim):\nA riding suit"));
        assert!(msg.contains("CORPUS OCCURRENCES"));
        assert!(msg.trim_end().ends_with(
            "Discuss the use of \u{201c}franklin\u{201d} in this segment, and how William Shakespeare uses the word elsewhere in the corpus."
        ));
    }

    #[test]
    fn vocab_journal_prompt_substitutes_genre_and_targets_length() {
        let p = crate::gloss::vocab_journal_prompt("play");
        assert!(!p.contains("{genre}"));
        assert!(!p.contains("{unit}"));
        // Length target present whether the DB row or the fallback served.
        assert!(p.contains("10 to 15 sentences") || p.contains("10\u{2013}15 sentences"));
    }
}
