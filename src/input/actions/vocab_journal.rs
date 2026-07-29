//! Vocab journal Q&A: ask Claude about the vocab popup's current word in the
//! cursor segment and across the author's corpus; store as a kind='vocab'
//! journal entry and render in the popup. Pure prompt-assembly helpers here
//! are unit-tested; the stateful handlers mirror journal::ask_claude.

use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;

/// Max other-work occurrence lines fed to the prompt.
pub(crate) const CORPUS_HITS_CAP: usize = 10;

/// Corpus-evidence placeholder when no occurrences exist OR the query fails —
/// the two paths must emit the same string so the prompt reads consistently.
const CORPUS_NONE_FOUND: &str = "(none found)";

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
        return CORPUS_NONE_FOUND.to_string();
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

/// Seed question for a vocab ask (the user's canonical generic phrasing) that
/// `journal::improve_question` then sharpens into the expert phrasing actually
/// stored as the entry's `question`.
pub(crate) fn vocab_question(word: &str) -> String {
    format!(
        "How does '{word}' function in this passage, this work and throughout \
         this author's corpus?"
    )
}

/// Assemble the vocab Q&A user message (pure; testable without state).
/// `request` is the (improved) question the answer should address.
pub(crate) fn vocab_user_message(
    genre: &str,
    title: &str,
    author: &str,
    unit_label: &str,
    synopsis_division_label: &str,
    word: &str,
    segment: &str,
    corpus_block: &str,
    request: &str,
) -> String {
    format!(
        "Work type: {genre}\nWork: \"{title}\" by {author}\n{unit_label}: {synopsis_division_label}\nVocabulary word: {word}\n\n\
         Segment (the reader's cursor segment, verbatim):\n{segment}\n\n\
         CORPUS OCCURRENCES \u{2014} lines containing the word elsewhere in {author}'s works:\n{corpus_block}\n\n\
         Reader's request:\n{request}",
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
    let question = vocab_question(&word);

    // In-flight guard: a second Ctrl+r on the SAME word while its request is
    // still pending would send a duplicate paid API call and insert a second
    // row. Tracked on AppState (not popup state) so cursor moves and popup
    // closes during the wait can't drop the guard.
    if state_rc.borrow().vocab_qa_inflight.as_deref() == Some(word.as_str()) {
        return;
    }

    // One DB connection serves both the reuse lookup and the corpus query.
    let conn = crate::db::queries::open_db().ok();

    // Reuse: a stored vocab Q&A for this word + segment opens the journal
    // overlay landed on it — no new API call, same end state as a fresh ask.
    if let Some(conn) = conn.as_ref() {
        if let Ok(Some(page)) =
            crate::db::journal::find_vocab_page(conn, &canonical, div1, div2, &word)
        {
            let mut s = state_rc.borrow_mut();
            crate::app::vocab_popup::close_vocab_popup(&mut s);
            crate::input::actions::journal::open_overlay_at_entry(&mut s, div1, div2, page.id);
            crate::logging::log(&format!("VOCAB QA: stored answer for '{word}' (entry {})", page.id));
            return;
        }
    }

    // Fresh ask. Open the journal overlay on a LOADING card showing the
    // question, so the ~25s round trip is not a blank surface (the wait covers
    // two Claude calls: improve-question, then the answer). Mirrors the
    // passage-ask flow's `begin_passage_ask` → `submit_passage_question`
    // ordering, which is the reason that path never shows an empty card:
    // `render_current` FIRST (it is what primes `last_card_size`, which
    // `show_loading` needs to size the card — it silently skips sizing while
    // that width is 0), then `set_running_head`, then `show_loading`.
    // The held bottom-strip toast stays: it is the cross-surface progress
    // signal, and still covers the case where the user leaves the overlay
    // before the answer lands.
    let hold_gen = {
        let mut s = state_rc.borrow_mut();
        s.vocab_qa_inflight = Some(word.clone());
        // The definition card would otherwise sit over the overlay scrim.
        crate::app::vocab_popup::close_vocab_popup(&mut s);
        s.journal.return_pos =
            Some((s.current_line, s.page_top.line(), s.page_top.offset()));
        s.journal.filter = None;
        s.journal.entry_page_id = None;
        s.journal_band = crate::app::JournalBand::Division(div1, div2);
        s.journal.page_index = 0;
        s.input_mode = crate::app::InputMode::JournalOverlay;
        crate::input::actions::journal::render_current(&mut s);
        let head = crate::app::division_synopsis::cursor_head(&s);
        s.journal_overlay.set_running_head(&head.0, &head.1);
        s.journal_overlay
            .show_loading(&question, "Refining question\u{2026}");
        crate::input::navigation::show_chapter_toast_hold(
            &s,
            &format!("Journal Q&A - {word}"),
        )
    };
    crate::logging::log(&format!("VOCAB QA: asking about '{word}' in {canonical} {div1}.{div2}"));

    // Sharpen the seed question into expert phrasing first, grounded on the
    // word so the improver keeps it — the same improve_question chain the
    // journal ask flow runs. Its error path falls back to the seed, so the
    // main ask proceeds either way. The improved phrasing is both what the
    // model answers and what the entry stores as its question.
    let synopsis_division_label = crate::app::division_synopsis::synopsis_division_label(div1, div2);
    let keep_terms = vec![word.clone()];
    crate::input::actions::journal::improve_question(
        state_rc,
        question,
        &keep_terms,
        move |st, improved| {
            let corpus_block = crate::db::queries::open_db()
                .ok()
                .and_then(|conn| {
                    crate::db::concordance::find_word_occurrences(&conn, &word, &author).ok()
                })
                .map(|hits| vocab_corpus_block(&hits, &canonical, &word, CORPUS_HITS_CAP))
                .unwrap_or_else(|| CORPUS_NONE_FOUND.to_string());
            let (genre, unit, _units) = crate::gloss::genre_unit(&work_type);
            let unit_label = crate::input::actions::journal::titlecase_first(unit);
            let user_msg = vocab_user_message(
                genre,
                &title,
                &author,
                &unit_label,
                &synopsis_division_label,
                &word,
                &segment,
                &corpus_block,
                &improved,
            );

            // improve_question's on_done is a shared Fn — clone what the
            // request closures consume.
            let canonical = canonical.clone();
            let start_cit = start_cit.clone();
            let end_cit = end_cit.clone();
            let segment = segment.clone();
            let model_for_db = model.clone();
            let word_ok = word.clone();
            let word_err = word.clone();
            let question_ok = improved;
            // Second stage of the loading card: swap the seed question for the
            // sharpened phrasing that is actually being answered, and relabel
            // the indicator. Skipped if the user already left the overlay —
            // repainting it would drag them back to a surface they closed.
            if st.borrow().input_mode == crate::app::InputMode::JournalOverlay {
                let s = st.borrow();
                s.journal_overlay
                    .show_loading(&question_ok, "Answering\u{2026}");
            }
            crate::input::actions::claude_bridge::run_claude_request(
                st,
                crate::gloss::vocab_journal_prompt(&work_type),
                user_msg,
                model.clone(),
                move |st2, answer| {
                    // Insert FIRST — a paid answer must survive any UI race.
                    let saved_id = match crate::db::queries::open_db_rw() {
                        Ok(conn) => match crate::db::journal::save_vocab_page(
                            &conn, &canonical, div1, div2, &start_cit, &end_cit,
                            &segment, &word_ok, &question_ok, &answer, &model_for_db,
                        ) {
                            Ok(id) => Some(id),
                            Err(e) => {
                                crate::logging::log(&format!("VOCAB QA: db write failed: {e}"));
                                None
                            }
                        },
                        Err(e) => {
                            crate::logging::log(&format!("VOCAB QA: db open failed: {e}"));
                            None
                        }
                    };
                    let mut s = st2.borrow_mut();
                    if s.vocab_qa_inflight.as_deref() == Some(word_ok.as_str()) {
                        s.vocab_qa_inflight = None;
                    }
                    crate::input::navigation::release_chapter_toast_hold(&s, hold_gen);
                    match saved_id {
                        // Reveal the saved entry — from plain reading, or from
                        // the loading card this ask itself put up (the submit
                        // path claims JournalOverlay so the wait has a surface
                        // to paint on, so THAT mode is now also a reveal case;
                        // it replaces the spinner with the answer).
                        //
                        // Any OTHER mode means the user navigated away
                        // mid-wait — don't hijack it; the entry is stored, so a
                        // later Ctrl+r opens it instantly.
                        Some(id)
                            if matches!(
                                s.input_mode,
                                crate::app::InputMode::Reader
                                    | crate::app::InputMode::JournalOverlay
                            ) =>
                        {
                            crate::app::vocab_popup::close_vocab_popup(&mut s);
                            crate::input::actions::journal::open_overlay_at_entry(
                                &mut s, div1, div2, id,
                            );
                        }
                        Some(_) => {
                            crate::input::navigation::show_chapter_toast_secs(
                                &s,
                                &format!("Journal Q&A ready - {word_ok}"),
                                4,
                            );
                        }
                        None => {
                            // The answer arrived but could not be stored. If the
                            // loading card is still up it must not keep spinning
                            // for an entry that will never render — `show_message`
                            // stops the animator and puts the failure on the card.
                            let msg = format!("Journal Q&A save failed - {word_ok}");
                            if s.input_mode == crate::app::InputMode::JournalOverlay {
                                s.journal_overlay.show_message(&msg);
                            }
                            crate::input::navigation::show_chapter_toast_secs(&s, &msg, 4);
                        }
                    }
                },
                move |st2, msg| {
                    crate::logging::log(&format!("VOCAB QA: request failed: {msg}"));
                    let mut s = st2.borrow_mut();
                    if s.vocab_qa_inflight.as_deref() == Some(word_err.as_str()) {
                        s.vocab_qa_inflight = None;
                    }
                    crate::input::navigation::release_chapter_toast_hold(&s, hold_gen);
                    // Same as the save-failure arm: never leave the loading card
                    // spinning on a request that will never complete.
                    let failed = format!("Journal Q&A failed - {word_err}");
                    if s.input_mode == crate::app::InputMode::JournalOverlay {
                        s.journal_overlay.show_message(&failed);
                    }
                    crate::input::navigation::show_chapter_toast_secs(&s, &failed, 4);
                },
            );
        },
    );
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
        let q = vocab_question("franklin");
        assert_eq!(
            q,
            "How does 'franklin' function in this passage, this work and throughout this author's corpus?"
        );

        let msg = vocab_user_message(
            "play", "Cymbeline", "William Shakespeare", "Scene", "3.2",
            "franklin", "A riding suit no costlier\u{2026}", "(none found)",
            "How does 'franklin' operate here?",
        );
        assert!(msg.contains("Work type: play"));
        assert!(msg.contains("Work: \"Cymbeline\" by William Shakespeare"));
        assert!(msg.contains("Scene: 3.2"));
        assert!(msg.contains("Vocabulary word: franklin"));
        assert!(msg.contains("Segment (the reader's cursor segment, verbatim):\nA riding suit"));
        assert!(msg.contains("CORPUS OCCURRENCES"));
        assert!(msg.trim_end().ends_with(
            "Reader's request:\nHow does 'franklin' operate here?"
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
