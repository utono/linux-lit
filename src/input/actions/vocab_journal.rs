//! Vocab journal Q&A: ask Claude about the vocab popup's current word in the
//! cursor segment and across the author's corpus; store as a kind='vocab'
//! journal entry and render in the popup. Pure prompt-assembly helpers here
//! are unit-tested; the stateful handlers mirror journal::ask_claude.

use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;

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
///
/// Dedupe is on `canonical_text` ALONE: lit.db line_mapping duplicates every
/// line per media edition (Tit, Tit-Amb, Tit-Argo carry identical-text rows
/// under titles like "Titus Andronicus" / "Titus Andronicus (Ambrose)"), so a
/// per-(abbrev,text) key would let each variant consume the cap as a fake
/// separate work. Hits arrive ordered by work_abbrev, so the base edition
/// sorts first and wins the group title.
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
        if !seen.insert(h.canonical_text.clone()) {
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

/// R in the main card: vocab journal Q&A for the popup's current word.
/// Silent no-op unless the popup is visible AND the popup's current word
/// sits on the cursor line. Stored answers render without a new API call.
pub(crate) fn vocab_journal_ask(state_rc: &Rc<RefCell<AppState>>) {
    let gathered = {
        let s = state_rc.borrow();
        if !s.vocab_popup.popup.is_visible() || s.vocab_popup.data.is_empty() {
            None
        } else {
            let word = s.vocab_popup.data[s.vocab_popup.index].word.clone();
            let on_line = s
                .vocab_matches
                .iter()
                .any(|m| m.line_index == s.current_line && m.word == word);
            let seg = crate::input::segments::segment_context(&s, 0);
            match (s.current_work.as_ref(), seg) {
                (Some(w), Some(seg)) if on_line && !seg.cursor_lines.is_empty() => Some((
                    word,
                    w.title.clone(),
                    w.author.clone(),
                    w.canonical_abbrev.clone(),
                    w.work_type.clone(),
                    seg.div1,
                    seg.div2,
                    seg.cursor_lines.first().map(|l| l.citation.clone()).unwrap_or_default(),
                    seg.cursor_lines.last().map(|l| l.citation.clone()).unwrap_or_default(),
                    seg.segments.get(seg.cursor_index).cloned().unwrap_or_default(),
                    s.config.claude_model.clone(),
                )),
                _ => None,
            }
        }
    };
    let Some((word, title, author, canonical, work_type, div1, div2, start_cit, end_cit, segment, model)) =
        gathered
    else {
        return;
    };
    let question = vocab_question(&word, &author);

    // In-flight guard: a second R on the SAME word while its request is still
    // pending would send a duplicate paid API call and insert a second row. A
    // second R on a DIFFERENT word replaces the pending display, by design.
    {
        use crate::app::vocab_popup::JournalDisplay;
        let s = state_rc.borrow();
        if matches!(
            s.vocab_popup.journal.as_ref(),
            Some(JournalDisplay::Pending { word: w, .. }) if *w == word
        ) {
            return;
        }
    }

    // One DB connection serves both the reuse lookup and the corpus query.
    let conn = crate::db::queries::open_db().ok();

    // Reuse: a stored vocab Q&A for this word + segment renders immediately.
    if let Some(conn) = conn.as_ref() {
        if let Ok(Some(page)) =
            crate::db::journal::find_vocab_page(conn, &canonical, div1, div2, &word)
        {
            let mut s = state_rc.borrow_mut();
            s.vocab_popup.journal = Some(crate::app::vocab_popup::JournalDisplay::Answer {
                word: word.clone(),
                question: page.question.clone(),
                answer: page.answer.clone(),
                model: page.claude_model.clone(),
            });
            s.vocab_popup.view = crate::ui::vocab_popup::VocabView::Journal;
            crate::app::vocab_popup::show_vocab_popup(&s);
            crate::logging::log(&format!("VOCAB QA: stored answer for '{word}'"));
            return;
        }
    }

    // Fresh ask: corpus evidence, pending render, request.
    let corpus_block = conn
        .as_ref()
        .and_then(|conn| crate::db::concordance::find_word_occurrences(conn, &word, &author).ok())
        .map(|hits| vocab_corpus_block(&hits, &canonical, &word, CORPUS_HITS_CAP))
        .unwrap_or_else(|| "(none found)".to_string());

    let (genre, unit, _units) = crate::gloss::genre_unit(&work_type);
    let unit_label = {
        let mut c = unit.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    };
    let user_msg = vocab_user_message(
        genre,
        &title,
        &author,
        &unit_label,
        &crate::app::scene_synopsis::scene_label(div1, div2),
        &word,
        &segment,
        &corpus_block,
    );

    {
        let mut s = state_rc.borrow_mut();
        s.vocab_popup.journal = Some(crate::app::vocab_popup::JournalDisplay::Pending {
            word: word.clone(),
            question: question.clone(),
        });
        s.vocab_popup.view = crate::ui::vocab_popup::VocabView::Journal;
        crate::app::vocab_popup::show_vocab_popup(&s);
    }
    crate::logging::log(&format!("VOCAB QA: asking about '{word}' in {canonical} {div1}.{div2}"));

    let model_for_db = model.clone();
    let word_ok = word.clone();
    let question_ok = question.clone();
    let word_err = word;
    let question_err = question;
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        crate::gloss::vocab_journal_prompt(&work_type),
        user_msg,
        model,
        move |st, answer| {
            // Insert FIRST — a paid answer must survive any UI race.
            match crate::db::queries::open_db_rw() {
                Ok(conn) => {
                    if let Err(e) = crate::db::journal::save_vocab_page(
                        &conn, &canonical, div1, div2, &start_cit, &end_cit,
                        &segment, &word_ok, &question_ok, &answer, &model_for_db,
                    ) {
                        crate::logging::log(&format!("VOCAB QA: db write failed: {e}"));
                    }
                }
                Err(e) => crate::logging::log(&format!("VOCAB QA: db open failed: {e}")),
            }
            let mut s = st.borrow_mut();
            if journal_pending_for(&s, &word_ok) {
                s.vocab_popup.journal = Some(crate::app::vocab_popup::JournalDisplay::Answer {
                    word: word_ok.clone(),
                    question: question_ok.clone(),
                    answer,
                    model: model_for_db.clone(),
                });
                crate::app::vocab_popup::show_vocab_popup(&s);
            }
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            if journal_pending_for(&s, &word_err) {
                s.vocab_popup.journal = Some(crate::app::vocab_popup::JournalDisplay::Error {
                    word: word_err.clone(),
                    question: question_err.clone(),
                    message: msg.to_string(),
                });
                crate::app::vocab_popup::show_vocab_popup(&s);
            }
        },
    );
}

/// Async guard: true while the popup is visible with a PENDING Journal
/// display for `word`. Cursor moves, word cycles, and view toggles all
/// clear `journal`, so a stale reply repaints nothing (the DB insert has
/// already happened).
fn journal_pending_for(s: &AppState, word: &str) -> bool {
    use crate::app::vocab_popup::JournalDisplay;
    s.vocab_popup.popup.is_visible()
        && matches!(
            s.vocab_popup.journal.as_ref(),
            Some(JournalDisplay::Pending { word: w, .. }) if w == word
        )
}

/// Ctrl+n / Ctrl+p: page the popup's Journal answer. No-op outside the
/// Journal view (the keys stay inert in normal reading).
pub(crate) fn vocab_journal_page(state_rc: &Rc<RefCell<AppState>>, dir: i32) {
    let s = state_rc.borrow();
    if s.vocab_popup.view != crate::ui::vocab_popup::VocabView::Journal
        || !s.vocab_popup.popup.is_visible()
    {
        return;
    }
    s.vocab_popup.popup.journal_page(dir);
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
    fn corpus_block_dedupes_other_work_media_variants() {
        // lit.db line_mapping duplicates every line per media edition. The
        // base edition sorts first (hits are ordered by work_abbrev), so the
        // line is emitted once under the base title — the variant rows must
        // NOT consume the cap as fake separate works.
        let hits = vec![
            hit("Tit", "Titus Andronicus", 2, 3, 40, "A franklin passed this way"),
            hit("Tit-Amb", "Titus Andronicus (Ambrose)", 2, 3, 40, "A franklin passed this way"),
        ];
        let block = vocab_corpus_block(&hits, "Cym", "franklin", 10);
        assert_eq!(block.matches("A franklin passed this way").count(), 1);
        assert!(block.contains("Titus Andronicus:"), "base title wins, block was:\n{block}");
        assert!(!block.contains("(Ambrose)"), "variant title not shown, block was:\n{block}");
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
