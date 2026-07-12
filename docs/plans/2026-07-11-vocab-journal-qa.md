# Vocab Journal Q&A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `R` in the main card (vocab popup visible, vocab word on the cursor
line) immediately asks Claude to discuss the popup's current word in this
segment and across the author's corpus, stores it as a `kind='vocab'` journal
Q&A, and renders the answer inside the vocab popup — paginated via
`Ctrl+n`/`Ctrl+p` with the word + definition pinned at the bottom of every
page.

**Spec:** `docs/plans/2026-07-11-vocab-journal-qa-design.md` (read it first).

**Architecture:** Mirrors the journal overlay's `ask_claude` shape (prompt →
API → immediate DB insert → repaint) via `claude_bridge::run_claude_request`.
New `VocabView::Journal` popup view; new `journal.vocab` prompt key with a
compiled fallback; corpus evidence from `db::concordance::find_word_occurrences`.
Reuse lookup is exact: `work + div1/div2 + kind='vocab' + word` (new nullable
`word` column).

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite, tokio; existing lit.db at
`~/utono/litdb/data/lit.db`.

## Global Constraints

- Verify with `cargo build`; **never run `cargo run`** — the user launches
  the app. Headless verification uses cage/grim per `CLAUDE.md`.
- Pre-existing failing test `db::queries::tests::test_load_work_hamlet`
  (asserts live lit.db state) is expected in every full run — not caused by
  this work.
- Every keybind change updates all three: `src/input/keymap_config.rs`,
  the stowed `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
  (`~/.config/linux-lit/keymap.json` is its symlink), and the Ctrl+/ overlay
  (`src/ui/keybinds_overlay.rs`).
- All journal paths key by `Work.canonical_abbrev`.
- The answer prompt targets 10–15 sentences (no hard cap; pagination absorbs
  overflow).
- Commit after every task; message style `feat:`/`test:`/`docs:` as in
  recent history.

## File Structure

- `src/db/journal.rs` — `word` column migration, `save_vocab_page`,
  `find_vocab_page` (+ tests).
- `src/gloss.rs` — `vocab_journal_prompt` beside `journal_qa_prompt`.
- `src/input/actions/vocab_journal.rs` — **new**: pure prompt-assembly
  helpers (+ tests) and the stateful `vocab_journal_ask` /
  `vocab_journal_page` handlers.
- `src/ui/vocab_popup.rs` — `VocabView::Journal`, `JournalBody`,
  `update_journal`, `journal_page`, footer refresh.
- `src/app/vocab_popup.rs` — `JournalDisplay` state, show dispatch, view
  resets, body height cap.
- `src/app/mod.rs` — `VocabPopupState` construction gains `journal: None`.
- `src/theme.rs` — `.vocab-popup .journal-pin` CSS.
- `src/input/actions/mod.rs` — 3 new `Action` variants (+ category/name).
- `src/input/keymap.rs` — 3 dispatch arms.
- `src/input/keymap_config.rs` — 3 binds + test updates.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — 3 entries.
- `src/ui/keybinds_overlay.rs` — keycap strip + describe/expanded entries.

---

### Task 1: DB layer — `word` column, save + reuse lookup

**Files:**
- Modify: `src/db/journal.rs` (migration at `ensure_journal_table`, new fns
  after `find_passage_pages`, tests in the existing `mod tests`)

**Interfaces:**
- Consumes: existing `JOURNAL_PAGE_COLUMNS`, `map_journal_page_row`,
  `crate::db::queries::column_exists`.
- Produces (used by Task 5):

```rust
pub fn save_vocab_page(
    conn: &Connection, work_abbrev: &str, div1: i64, div2: i64,
    start_citation: &str, end_citation: &str, source_text: &str,
    word: &str, question: &str, answer: &str, claude_model: &str,
) -> Result<i64, rusqlite::Error>;

pub fn find_vocab_page(
    conn: &Connection, work_abbrev: &str, div1: i64, div2: i64, word: &str,
) -> Result<Option<JournalPage>, rusqlite::Error>;
```

- [ ] **Step 1: Write the failing tests** — append inside `mod tests` in
  `src/db/journal.rs`:

```rust
    #[test]
    fn vocab_word_column_migrates_idempotently() {
        let conn = mem();
        ensure_journal_table(&conn).unwrap(); // second call must not error
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='word'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has, "word column should exist after ensure_journal_table");
    }

    #[test]
    fn vocab_page_roundtrip_and_reuse_lookup() {
        let conn = mem();
        let id = save_vocab_page(
            &conn, "Cym", 3, 2, "Cym.3.2.77", "Cym.3.2.80",
            "A riding suit no costlier than would fit\nA franklin's huswife.",
            "franklin", "\u{201c}franklin\u{201d} in this segment, and across Shakespeare",
            "Imogen prices her disguise\u{2026}", "claude-opus-4-8",
        ).unwrap();
        assert!(id > 0);

        // Exact reuse hit.
        let page = find_vocab_page(&conn, "Cym", 3, 2, "franklin").unwrap().unwrap();
        assert_eq!(page.id, id);
        assert_eq!(page.kind, "vocab");
        assert_eq!(page.source_text.as_deref().unwrap(), "A riding suit no costlier than would fit\nA franklin's huswife.");

        // Different word, different segment, different work: all miss.
        assert!(find_vocab_page(&conn, "Cym", 3, 2, "huswife").unwrap().is_none());
        assert!(find_vocab_page(&conn, "Cym", 3, 3, "franklin").unwrap().is_none());
        assert!(find_vocab_page(&conn, "Ham", 3, 2, "franklin").unwrap().is_none());

        // Most recent wins (same-second timestamps tie-break on id DESC).
        let id2 = save_vocab_page(
            &conn, "Cym", 3, 2, "Cym.3.2.77", "Cym.3.2.80", "src",
            "franklin", "Q2?", "Second answer.", "m",
        ).unwrap();
        assert_eq!(find_vocab_page(&conn, "Cym", 3, 2, "franklin").unwrap().unwrap().id, id2);

        // Vocab rows ride the passage scope into the scene band render.
        let band = find_scene_band_pages(&conn, "Cym", 3, 2).unwrap();
        assert_eq!(band.len(), 2);
        assert!(band.iter().all(|p| p.kind == "vocab"));

        // A plain passage Q&A (kind='qa') must never satisfy the vocab lookup.
        save_passage_page(&conn, "Cym", 3, 4, "Cym.3.4.1", "Cym.3.4.2", "s", "Q?", "A.", "m").unwrap();
        assert!(find_vocab_page(&conn, "Cym", 3, 4, "franklin").unwrap().is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd ~/utono/linux-lit && cargo test --bins db::journal::tests::vocab -- --nocapture
```

Expected: compile error — `save_vocab_page`/`find_vocab_page` not found.

- [ ] **Step 3: Implement.** In `ensure_journal_table`, after the `kind`
  migration block (line ~81), add:

```rust
    if !column_exists(conn, "journal_entries", "word")? {
        conn.execute_batch("ALTER TABLE journal_entries ADD COLUMN word TEXT;")?;
    }
```

After `find_passage_pages`, add:

```rust
/// Insert a vocab-word journal Q&A: passage scope anchored to the cursor
/// segment, `kind='vocab'`, with the word stored for exact reuse lookup.
pub fn save_vocab_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_citation: &str,
    end_citation: &str,
    source_text: &str,
    word: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope,
             start_citation, end_citation, source_text, kind, word, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'passage', ?7, ?8, ?9, 'vocab', ?10,
                 datetime('now'))",
        rusqlite::params![
            work_abbrev, div1, div2, question, answer, claude_model,
            start_citation, end_citation, source_text, word
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The most recent vocab Q&A for `word` in the segment's `(div1, div2)`, or
/// None. Pressing R with a hit renders the stored answer — no duplicate ask.
pub fn find_vocab_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    word: &str,
) -> Result<Option<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND kind = 'vocab' AND word = ?4 \
         ORDER BY timestamp DESC, id DESC LIMIT 1",
    ))?;
    let mut rows = stmt.query_map(
        rusqlite::params![work_abbrev, div1, div2, word],
        map_journal_page_row,
    )?;
    rows.next().transpose()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --bins db::journal::tests -- --nocapture
```

Expected: all `db::journal` tests PASS (including the pre-existing ones).

- [ ] **Step 5: Commit**

```bash
git add src/db/journal.rs
git commit -m "feat(db): vocab journal pages — word column, save_vocab_page, find_vocab_page"
```

---

### Task 2: Prompt builder + pure assembly helpers

**Files:**
- Modify: `src/gloss.rs` (add `vocab_journal_prompt` directly after
  `journal_qa_prompt`, line ~197)
- Create: `src/input/actions/vocab_journal.rs` (pure helpers + tests; the
  stateful handlers arrive in Task 5)
- Modify: `src/input/actions/mod.rs` (module declaration)

**Interfaces:**
- Consumes: `crate::gloss::{template_or, genre_unit}` (private `template_or`
  stays private — `vocab_journal_prompt` lives inside gloss.rs);
  `crate::db::concordance::ConcordanceRow` (fields: `work_abbrev`, `title`,
  `div1`, `div2`, `line_in_div`, `canonical_text`).
- Produces (used by Task 5):

```rust
// src/gloss.rs
pub fn vocab_journal_prompt(work_type: &str) -> String;

// src/input/actions/vocab_journal.rs
pub(crate) const CORPUS_HITS_CAP: usize = 10;
pub(crate) fn line_contains_word(line: &str, word: &str) -> bool;
pub(crate) fn vocab_corpus_block(
    hits: &[crate::db::concordance::ConcordanceRow],
    current_canonical: &str, word: &str, cap: usize,
) -> String;
pub(crate) fn vocab_question(word: &str, author: &str) -> String;
pub(crate) fn vocab_user_message(
    genre: &str, title: &str, author: &str, unit_label: &str,
    scene_label: &str, word: &str, segment: &str, corpus_block: &str,
) -> String;
```

- [ ] **Step 1: Declare the module.** In `src/input/actions/mod.rs`, after
  `pub mod synopsis;` (line 18):

```rust
pub(crate) mod vocab_journal;
```

- [ ] **Step 2: Write the failing tests.** Create
  `src/input/actions/vocab_journal.rs`:

```rust
//! Vocab journal Q&A: ask Claude about the vocab popup's current word in the
//! cursor segment and across the author's corpus; store as a kind='vocab'
//! journal entry and render in the popup. Pure prompt-assembly helpers here
//! are unit-tested; the stateful handlers mirror journal::ask_claude.

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
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test --bins input::actions::vocab_journal -- --nocapture
```

Expected: compile error — helper functions not defined.

- [ ] **Step 4: Implement the helpers** — above the `#[cfg(test)]` block in
  `src/input/actions/vocab_journal.rs`:

```rust
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
```

- [ ] **Step 5: Add the system prompt.** In `src/gloss.rs`, directly after
  `journal_qa_prompt`'s closing brace (line ~197):

```rust
/// System prompt for the vocab journal Q&A (R in the main card with the
/// vocab popup open). DB template `journal.vocab` or the compiled fallback;
/// genre vocabulary substituted like `journal_qa_prompt`. The 10–15 sentence
/// target sizes answers for the popup panel (overflow pages via Ctrl+n/p).
pub fn vocab_journal_prompt(work_type: &str) -> String {
    const FALLBACK: &str = "\
You are a literary interlocutor helping a reader who is studying vocabulary while working through a {genre}. The reader's cursor is on a segment of the {genre} that contains a vocabulary word, and they want to understand how the word works — here and across the author's other works.

Discuss, in this order: first, what the word means in this segment and what work it does there — register, tone, image, characterization, irony; second, how the author uses the word elsewhere, using the lines supplied under CORPUS OCCURRENCES as your primary evidence, grouping observations by work and noting shifts in sense or register between uses; third, anything about the word itself that helps a reader building vocabulary, such as etymology or an older sense, but only when it genuinely illuminates the usage.

Ground every claim in the supplied segment and occurrence lines. You may quote briefly from the supplied lines when discussing them. If CORPUS OCCURRENCES says none were found, say plainly that the word appears only here in the available corpus and focus on this segment's usage.

Write 10 to 15 sentences of flowing prose. Keep paragraphs short — two to four sentences — and separate paragraphs with a blank line. No markdown, no bullet lists, no numbered lists, no headers. Do not use the = sign; write paraphrases as prose.";
    let (genre, unit, units) = genre_unit(work_type);
    template_or("journal.vocab", FALLBACK)
        .replace("{genre}", genre)
        .replace("{units}", units)
        .replace("{unit}", unit)
}
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test --bins input::actions::vocab_journal -- --nocapture
```

Expected: 6 tests PASS.

- [ ] **Step 7: Commit**

```bash
git add src/gloss.rs src/input/actions/mod.rs src/input/actions/vocab_journal.rs
git commit -m "feat: vocab journal prompt (journal.vocab) + pure corpus/user-message builders"
```

---

### Task 3: Popup UI — Journal view with pagination and pinned word block

**Files:**
- Modify: `src/ui/vocab_popup.rs`
- Modify: `src/theme.rs` (one CSS rule)

**Interfaces:**
- Consumes: existing `VocabWordData`, popup CSS classes
  (`definition-header`, `definition-word`, `definition-text`,
  `definition-etymology`, `definition-hint`), `{vocab_popup_border}` format
  arg already present in `generate_css` (theme.rs:981).
- Produces (used by Task 4/5):

```rust
pub enum VocabView { Definition, Gloss, Journal }   // Journal added

#[derive(Clone, Copy)]
pub enum JournalBody<'a> {
    Pending { model: &'a str },
    Answer { text: &'a str },
    Error { message: &'a str },
}

impl VocabPopup {
    pub fn update_journal(&self, data: &VocabWordData, index: usize,
        total: usize, question: &str, body: JournalBody,
        saved_model: Option<&str>, max_body_height: i32);
    pub fn journal_page(&self, dir: i32) -> bool;  // true = page turned
}
```

- [ ] **Step 1: Extend the view enum and struct.** In
  `src/ui/vocab_popup.rs`:

`VocabView` (line ~14) gains a variant:

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum VocabView {
    Definition,
    Gloss,
    /// The vocab journal Q&A (R): paged answer above a pinned
    /// word+definition block. Rendered by `update_journal`, not `update`.
    Journal,
}
```

`VocabPopup` (line ~19) gains two fields, initialized in `new()` just before
the final struct literal:

```rust
pub struct VocabPopup {
    container: GtkBox,
    content_box: GtkBox,
    header_label: Label,
    counter_label: Label,
    footer_label: Label,
    /// The Journal answer's scroll region while that view is showing —
    /// Ctrl+n/p page it. None in Definition/Gloss views.
    journal_scroll: std::cell::RefCell<Option<gtk4::ScrolledWindow>>,
    /// Footer text without the page suffix ("saved · <model>").
    journal_footer_base: std::cell::RefCell<String>,
}
```

In `new()`:

```rust
        VocabPopup {
            container,
            content_box,
            header_label,
            counter_label,
            footer_label,
            journal_scroll: std::cell::RefCell::new(None),
            journal_footer_base: std::cell::RefCell::new(String::new()),
        }
```

- [ ] **Step 2: Keep `update` exhaustive and journal-free.** In `update()`,
  first line of the body (before the content clear):

```rust
        *self.journal_scroll.borrow_mut() = None;
```

and add an arm to its `match view` (after `VocabView::Gloss => { ... }`):

```rust
            // Journal renders via update_journal; reaching here means the
            // caller forgot to route — show nothing rather than stale data.
            VocabView::Journal => {}
```

- [ ] **Step 3: Add `JournalBody`, `update_journal`, `journal_page`, and the
  footer refresher** — append after `update_synopsis`:

```rust
/// Body content for the Journal view.
#[derive(Clone, Copy)]
pub enum JournalBody<'a> {
    Pending { model: &'a str },
    Answer { text: &'a str },
    Error { message: &'a str },
}

impl VocabPopup {
    /// Render the Journal Q&A view: JOURNAL Q&A header, dim question line,
    /// the paged answer body (capped at `max_body_height`), then the pinned
    /// word + definition block that stays visible on every page. Footer
    /// ("saved · model — page N / M") appears only for a saved Answer.
    pub fn update_journal(
        &self,
        data: &VocabWordData,
        index: usize,
        total: usize,
        question: &str,
        body: JournalBody,
        saved_model: Option<&str>,
        max_body_height: i32,
    ) {
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }
        *self.journal_scroll.borrow_mut() = None;

        self.header_label.set_visible(false);
        if total > 1 {
            self.counter_label.set_text(&format!("{} / {}", index + 1, total));
            self.counter_label.set_visible(true);
        } else {
            self.counter_label.set_visible(false);
        }

        let qa_header = Label::builder()
            .label("JOURNAL Q&A")
            .halign(gtk4::Align::Start)
            .margin_bottom(4)
            .build();
        qa_header.add_css_class("definition-header");
        self.content_box.append(&qa_header);

        let q_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .margin_bottom(8)
            .build();
        q_label.add_css_class("definition-etymology");
        q_label.set_text(&format!("Q \u{b7} {question}"));
        self.content_box.append(&q_label);

        match body {
            JournalBody::Pending { model } => {
                let pending = Label::builder().halign(gtk4::Align::Start).build();
                pending.add_css_class("definition-etymology");
                pending.set_text(&format!("asking {model}\u{2026}"));
                self.content_box.append(&pending);
            }
            JournalBody::Error { message } => {
                let err = Label::builder()
                    .halign(gtk4::Align::Start)
                    .wrap(true)
                    .wrap_mode(gtk4::pango::WrapMode::WordChar)
                    .build();
                err.add_css_class("definition-etymology");
                err.set_text(message);
                self.content_box.append(&err);
            }
            JournalBody::Answer { text } => {
                let answer = Label::builder()
                    .halign(gtk4::Align::Start)
                    .valign(gtk4::Align::Start)
                    .wrap(true)
                    .wrap_mode(gtk4::pango::WrapMode::WordChar)
                    .build();
                answer.add_css_class("definition-text");
                answer.set_text(text);
                // External vscroll policy: no visible scrollbar; Ctrl+n/p
                // drive the adjustment in whole viewport-height pages.
                let scroll = gtk4::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk4::PolicyType::Never)
                    .vscrollbar_policy(gtk4::PolicyType::External)
                    .propagate_natural_height(true)
                    .max_content_height(max_body_height)
                    .child(&answer)
                    .build();
                self.content_box.append(&scroll);
                *self.journal_scroll.borrow_mut() = Some(scroll);
            }
        }

        // Pinned block: the word + its definition, fixed below the paged
        // body — visible on every page (spec: never scrolls away).
        let pin = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .build();
        pin.add_css_class("journal-pin");
        let word_label = Label::builder().halign(gtk4::Align::Start).build();
        word_label.add_css_class("definition-word");
        word_label.set_text(&data.word);
        pin.append(&word_label);
        if let Some(ref def) = data.definition {
            let def_label = Label::builder()
                .halign(gtk4::Align::Start)
                .wrap(true)
                .wrap_mode(gtk4::pango::WrapMode::Word)
                .build();
            def_label.add_css_class("definition-etymology");
            def_label.set_text(def);
            pin.append(&def_label);
        }
        self.content_box.append(&pin);

        // Footer only once an answer is saved (spec: hidden while pending).
        if let (JournalBody::Answer { .. }, Some(model)) = (body, saved_model) {
            let base = format!("saved \u{b7} {model}");
            *self.journal_footer_base.borrow_mut() = base.clone();
            self.footer_label.set_visible(true);
            self.footer_label.set_text(&base);
            // Page count needs a viewport allocation; refresh after layout.
            if let Some(scroll) = self.journal_scroll.borrow().clone() {
                let footer = self.footer_label.clone();
                gtk4::glib::idle_add_local_once(move || {
                    refresh_journal_footer(&scroll, &footer, &base);
                });
            }
        } else {
            self.journal_footer_base.borrow_mut().clear();
            self.footer_label.set_visible(false);
        }
    }

    /// Page the Journal answer by `dir` viewport-heights. Returns true when
    /// a page turn happened (Journal view with overflowing content only).
    pub fn journal_page(&self, dir: i32) -> bool {
        let scroll = match self.journal_scroll.borrow().clone() {
            Some(s) => s,
            None => return false,
        };
        let adj = scroll.vadjustment();
        let page = adj.page_size();
        if page <= 0.0 || adj.upper() <= page + 1.0 {
            return false;
        }
        let max = (adj.upper() - page).max(0.0);
        let new = (adj.value() + f64::from(dir) * page).clamp(0.0, max);
        if (new - adj.value()).abs() < 1.0 {
            return false;
        }
        adj.set_value(new);
        refresh_journal_footer(&scroll, &self.footer_label, &self.journal_footer_base.borrow());
        true
    }
}

/// Rewrite the Journal footer with the page position ("saved · m — page
/// 2 / 3 · C-n ▸"). Free function so the post-layout idle can call it with
/// cloned widgets (VocabPopup itself is not reference-counted).
fn refresh_journal_footer(scroll: &gtk4::ScrolledWindow, footer: &Label, base: &str) {
    let adj = scroll.vadjustment();
    let page = adj.page_size();
    if page <= 0.0 || adj.upper() <= page + 1.0 {
        footer.set_text(base);
        return;
    }
    let pages = (adj.upper() / page).ceil() as usize;
    let cur = ((adj.value() / page).round() as usize + 1).min(pages.max(1));
    footer.set_text(&format!("{base} \u{2014} page {cur} / {pages} \u{b7} C-n \u{25b8}"));
}
```

- [ ] **Step 4: CSS for the pinned block.** In `src/theme.rs`
  `generate_css`, directly after the `.vocab-popup .definition-hint` rule
  (line ~916), add:

```rust
         .vocab-popup .journal-pin {{ border-top: 1px solid {vocab_popup_border}; \
           padding-top: 10px; margin-top: 12px; }} \
```

(`{vocab_popup_border}` is already a named argument of this `format!` —
no new argument needed.)

- [ ] **Step 5: Build**

```bash
cargo build
```

Expected: compiles. (If `VocabView::Journal` breaks an exhaustive match in
`src/app/vocab_popup.rs::vocab_popup_toggle_view`, add the arm
`VocabView::Journal => VocabView::Definition,` there now — Task 4 revisits
that function anyway.)

- [ ] **Step 6: Commit**

```bash
git add src/ui/vocab_popup.rs src/theme.rs src/app/vocab_popup.rs
git commit -m "feat(ui): vocab popup Journal view — paged answer, pinned word+definition, page footer"
```

---

### Task 4: App-side journal display state and view resets

**Files:**
- Modify: `src/app/vocab_popup.rs`
- Modify: `src/app/mod.rs` (VocabPopupState construction — find it with
  `rg -n "VocabPopupState \{" src/app/mod.rs`)

**Interfaces:**
- Consumes: Task 3's `VocabView::Journal`, `JournalBody`, `update_journal`.
- Produces (used by Task 5):

```rust
pub enum JournalDisplay {
    Pending { word: String, question: String },
    Answer { word: String, question: String, answer: String, model: String },
    Error  { word: String, question: String, message: String },
}
// VocabPopupState gains: pub journal: Option<JournalDisplay>
// show_vocab_popup renders Journal view when view==Journal && journal.is_some()
```

- [ ] **Step 1: Add the display state.** In `src/app/vocab_popup.rs`, after
  the `VocabPopupState` struct:

```rust
/// What the popup's Journal view is showing. Carries the word so the async
/// reply can verify the popup still shows the word it asked about — any
/// cursor move, word cycle, or view toggle clears this, and a stale reply
/// must not repaint it (the DB insert still happens regardless).
pub enum JournalDisplay {
    Pending { word: String, question: String },
    Answer { word: String, question: String, answer: String, model: String },
    Error { word: String, question: String, message: String },
}
```

and a field on `VocabPopupState`:

```rust
pub struct VocabPopupState {
    pub popup: crate::ui::vocab_popup::VocabPopup,
    pub data: Vec<crate::ui::vocab_popup::VocabWordData>,
    pub index: usize,
    pub view: crate::ui::vocab_popup::VocabView,
    pub auto: bool,
    pub line: Option<usize>,
    pub fade_gen: std::rc::Rc<std::cell::Cell<u64>>,
    pub journal: Option<JournalDisplay>,
}
```

In `src/app/mod.rs`, add `journal: None,` to the `VocabPopupState { ... }`
construction site.

- [ ] **Step 2: Route the Journal view in `show_vocab_popup`.** Replace the
  body of `show_vocab_popup` (keeping the empty-data guard) with:

```rust
pub fn show_vocab_popup(state: &AppState) {
    if state.vocab_popup.data.is_empty() {
        state.vocab_popup.popup.hide();
        return;
    }
    let idx = state.vocab_popup.index;
    let total = state.vocab_popup.data.len();
    if state.vocab_popup.view == crate::ui::vocab_popup::VocabView::Journal {
        if let Some(ref j) = state.vocab_popup.journal {
            use crate::ui::vocab_popup::JournalBody;
            let (question, body, model) = match j {
                JournalDisplay::Pending { question, .. } => (
                    question.as_str(),
                    JournalBody::Pending { model: &state.config.claude_model },
                    None,
                ),
                JournalDisplay::Answer { question, answer, model, .. } => (
                    question.as_str(),
                    JournalBody::Answer { text: answer },
                    Some(model.as_str()),
                ),
                JournalDisplay::Error { question, message, .. } => (
                    question.as_str(),
                    JournalBody::Error { message },
                    None,
                ),
            };
            state.vocab_popup.popup.update_journal(
                &state.vocab_popup.data[idx],
                idx,
                total,
                question,
                body,
                model,
                journal_body_max_height(state),
            );
            state.vocab_popup.popup.show();
            return;
        }
    }
    let work_abbrev = state.current_work.as_ref()
        .map(|w| w.abbrev.as_str())
        .unwrap_or("");
    state.vocab_popup.popup.update(
        &state.vocab_popup.data[idx],
        idx,
        total,
        state.vocab_popup.view,
        work_abbrev,
    );
    state.vocab_popup.popup.show();
}

/// Height cap for the Journal answer body: half the window, floor 200px —
/// leaves room for the popup's fixed chrome (headers, pinned word +
/// definition, footer) at any geometry. Overflow pages via Ctrl+n/p.
fn journal_body_max_height(state: &AppState) -> i32 {
    let h = state
        .text_view
        .root()
        .map(|r| r.height())
        .unwrap_or(720);
    (h / 2).max(200)
}
```

- [ ] **Step 3: Reset the journal display everywhere the view resets.**

In `open_vocab_popup` (after `state.vocab_popup.view = VocabView::Definition;`)
and in `refresh_vocab_popup` (same spot), add:

```rust
    state.vocab_popup.journal = None;
```

Add a helper and use it in `vocab_popup_next` / `vocab_popup_prev` (first
line after the empty-data guard in each):

```rust
/// Cycling words or toggling views leaves the Journal display — it belongs
/// to one word only.
fn exit_journal_view(state: &mut AppState) {
    use crate::ui::vocab_popup::VocabView;
    if state.vocab_popup.view == VocabView::Journal {
        state.vocab_popup.view = VocabView::Definition;
    }
    state.vocab_popup.journal = None;
}
```

```rust
    exit_journal_view(state);
```

Rewrite `vocab_popup_toggle_view` as:

```rust
/// Toggle between definition and gloss view (Journal drops back to
/// Definition).
pub fn vocab_popup_toggle_view(state: &mut AppState) {
    use crate::ui::vocab_popup::VocabView;
    state.vocab_popup.view = match state.vocab_popup.view {
        VocabView::Definition => VocabView::Gloss,
        VocabView::Gloss => VocabView::Definition,
        VocabView::Journal => VocabView::Definition,
    };
    state.vocab_popup.journal = None;
    show_vocab_popup(state);
}
```

- [ ] **Step 4: Build**

```bash
cargo build
```

Expected: compiles with no warnings about unused `JournalDisplay` variants
(`Pending`/`Answer`/`Error` are all constructed in Task 5 — a temporary
dead-code warning here is fine).

- [ ] **Step 5: Commit**

```bash
git add src/app/vocab_popup.rs src/app/mod.rs
git commit -m "feat(app): vocab popup journal display state, view routing, resets"
```

---

### Task 5: Actions, ask handler, page handlers, dispatch

**Files:**
- Modify: `src/input/actions/mod.rs` (3 enum variants + category + name)
- Modify: `src/input/actions/vocab_journal.rs` (stateful handlers)
- Modify: `src/input/keymap.rs` (3 dispatch arms)

**Interfaces:**
- Consumes: Task 1 `save_vocab_page`/`find_vocab_page`; Task 2 helpers +
  `vocab_journal_prompt`; Task 4 `JournalDisplay`/`show_vocab_popup`;
  existing `segments::segment_context`,
  `db::concordance::find_word_occurrences`,
  `app::scene_synopsis::scene_label(div1, div2) -> String`,
  `claude_bridge::run_claude_request`.
- Produces:

```rust
pub(crate) fn vocab_journal_ask(state_rc: &Rc<RefCell<AppState>>);
pub(crate) fn vocab_journal_page(state_rc: &Rc<RefCell<AppState>>, dir: i32);
// Action::VocabJournalAsk, Action::VocabJournalPageNext, Action::VocabJournalPagePrev
```

- [ ] **Step 1: Enum variants.** In `src/input/actions/mod.rs`, after
  `HideVocabPopup` (line ~122):

```rust
    /// Ask Claude about the vocab popup's current word in the cursor
    /// segment and across the author's corpus; stores a kind='vocab'
    /// journal Q&A and renders it in the popup (R — gated on popup visible
    /// + a vocab word on the cursor line; silent no-op otherwise).
    VocabJournalAsk,
    /// Page the popup's Journal answer forward / backward (Ctrl+n /
    /// Ctrl+p; no-op outside the Journal view or when it fits one page).
    VocabJournalPageNext,
    VocabJournalPagePrev,
```

In `category()`, extend the Vocab arm (after `| Action::HideVocabPopup`):

```rust
            | Action::VocabJournalAsk
            | Action::VocabJournalPageNext
            | Action::VocabJournalPagePrev
```

In `name()`, after `Action::HideVocabPopup => "HideVocabPopup",`:

```rust
            Action::VocabJournalAsk => "VocabJournalAsk",
            Action::VocabJournalPageNext => "VocabJournalPageNext",
            Action::VocabJournalPagePrev => "VocabJournalPagePrev",
```

(`parse_action` deserializes via the derived serde impl, so the JSON names
work with no further change.)

- [ ] **Step 2: Handlers.** In `src/input/actions/vocab_journal.rs`, add at
  the top:

```rust
use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;
```

and above the test module:

```rust
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

    // Reuse: a stored vocab Q&A for this word + segment renders immediately.
    if let Ok(conn) = crate::db::queries::open_db() {
        if let Ok(Some(page)) =
            crate::db::journal::find_vocab_page(&conn, &canonical, div1, div2, &word)
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
    let corpus_block = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::concordance::find_word_occurrences(&conn, &word, &author).ok())
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
    if s.vocab_popup.view != crate::ui::vocab_popup::VocabView::Journal {
        return;
    }
    s.vocab_popup.popup.journal_page(dir);
}
```

- [ ] **Step 3: Dispatch arms.** In `src/input/keymap.rs`, after the
  `HideVocabPopup` arm (line ~3278):

```rust
        VocabJournalAsk => crate::input::actions::vocab_journal::vocab_journal_ask(state),
        VocabJournalPageNext => crate::input::actions::vocab_journal::vocab_journal_page(state, 1),
        VocabJournalPagePrev => crate::input::actions::vocab_journal::vocab_journal_page(state, -1),
```

- [ ] **Step 4: Build + existing tests**

```bash
cargo build && cargo test --bins input::actions -- --nocapture
```

Expected: compiles; vocab_journal tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/mod.rs src/input/actions/vocab_journal.rs src/input/keymap.rs
git commit -m "feat: VocabJournalAsk/PageNext/PagePrev actions — ask handler with reuse lookup + paging"
```

---

### Task 6: Keybinds — compiled defaults, keymap.json, Ctrl+/ overlay

**Files:**
- Modify: `src/input/keymap_config.rs` (binds + the
  `r_cycles_vocab_and_ctrl_r_hides_popup` test)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
- Modify: `src/ui/keybinds_overlay.rs`

**Interfaces:**
- Consumes: Task 5's Action variants.
- Produces: `R`→`VocabJournalAsk`, `Ctrl+n`→`VocabJournalPageNext`,
  `Ctrl+p`→`VocabJournalPagePrev` in both keymap layers + overlay entries.

- [ ] **Step 1: Update the keymap test FIRST (it will fail).** In
  `r_cycles_vocab_and_ctrl_r_hides_popup` (keymap_config.rs line ~471),
  replace

```rust
        assert_eq!(m.get(&KeyCombo::plain("R")), None);
```

with

```rust
        assert_eq!(m.get(&KeyCombo::plain("R")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::ctrl("n")), Some(&Action::VocabJournalPageNext));
        assert_eq!(m.get(&KeyCombo::ctrl("p")), Some(&Action::VocabJournalPagePrev));
```

and update the comment above it: `// minus and # freed; R = vocab journal
Q&A; Ctrl+n/p page its answer.`

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test --bins r_cycles_vocab -- --nocapture
```

Expected: FAIL — `plain("R")` maps to None.

- [ ] **Step 3: Add the binds.** In `vocab_bindings()`
  (keymap_config.rs:298), after the `(KeyCombo::ctrl("r"), ...)` line:

```rust
        // R: vocab journal Q&A — ask about the popup's current word (gated
        // on popup visible + a vocab word on the cursor line). Ctrl+n/p
        // page the popup's Journal answer; the pickers/overlays keep their
        // own modal Ctrl+n/p (handled before reader dispatch).
        (KeyCombo::plain("R"), Action::VocabJournalAsk),
        (KeyCombo::ctrl("n"), Action::VocabJournalPageNext),
        (KeyCombo::ctrl("p"), Action::VocabJournalPagePrev),
```

- [ ] **Step 4: Run to verify it passes**

```bash
cargo test --bins keymap_config -- --nocapture
```

Expected: all keymap_config tests PASS.

- [ ] **Step 5: keymap.json.** In
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, next to the
  existing `"r"` entries add (matching the file's one-object-per-line
  style, e.g. `{"key": "A", "action": "ToggleAuthorship"}`):

```json
    {"key": "R", "action": "VocabJournalAsk"},
    {"key": "n", "ctrl": true, "action": "VocabJournalPageNext"},
    {"key": "p", "ctrl": true, "action": "VocabJournalPagePrev"},
```

Verify the stow symlink is live (both commands must show the same content):

```bash
ls -l ~/.config/linux-lit/keymap.json
rg -n "VocabJournalAsk" ~/.config/linux-lit/keymap.json
```

- [ ] **Step 6: Ctrl+/ overlay.** In `src/ui/keybinds_overlay.rs`:

UPPER_ROW `r` entry (line ~60) becomes:

```rust
    key("r", "R", "vocab tap", "R: vocab Q&A", &[("C-r", "vocab \u{25b6}")]),
```

UPPER_ROW `p` entry (line ~55) gains a chord:

```rust
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("M-p", "phrase hl"), ("C-p", "Q&A page \u{25b2}")]),
```

HOME_ROW `n` entry (line ~77) becomes:

```rust
    key("n", "N", "next match", "N: prev match", &[("C-n", "Q&A page \u{25bc}")]),
```

describe() detail arms (near the existing `"vocab tap"` arm, line ~287):

```rust
        "vocab Q&A" => "Action::VocabJournalAsk (popup visible + vocab word \
on cursor line: ask/show stored) — src/input/actions/vocab_journal.rs",
        "Q&A page \u{25bc}" => "Action::VocabJournalPageNext — src/input/actions/vocab_journal.rs",
        "Q&A page \u{25b2}" => "Action::VocabJournalPagePrev — src/input/actions/vocab_journal.rs",
```

Expanded-label map (near `"vocab tap"`, line ~444):

```rust
        "vocab Q&A" => "vocab word journal Q&A",
        "Q&A page \u{25bc}" => "vocab Q&A next page",
        "Q&A page \u{25b2}" => "vocab Q&A previous page",
```

Then invoke the `update-cairo-keybinds-overlay` skill's three-pass
cross-reference to verify the overlay matches keymap_config exactly (it
catches strip/describe drift).

- [ ] **Step 7: Build + full bin tests**

```bash
cargo build && cargo test --bins
```

Expected: PASS except the pre-existing `test_load_work_hamlet` failure.

- [ ] **Step 8: Commit**

```bash
git add src/input/keymap_config.rs src/ui/keybinds_overlay.rs
git commit -m "feat: bind R vocab journal Q&A, Ctrl+n/p answer paging (config + overlay)"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "linux-lit: R vocab journal Q&A, Ctrl+n/p paging" && cd ~/utono/linux-lit
```

---

### Task 7: lit.db prompt row + headless e2e verification

**Files:**
- Modify: `~/utono/litdb/data/lit.db` (`api_prompts` row — additive, safe)
- No source changes; verification only.

- [ ] **Step 1: Insert the `journal.vocab` prompt row** (text identical to
  the compiled fallback; the DB row is authoritative thereafter):

```bash
sqlite3 ~/utono/litdb/data/lit.db <<'SQL'
INSERT INTO api_prompts (prompt_key, version, text, is_active, note)
VALUES ('journal.vocab', 1,
'You are a literary interlocutor helping a reader who is studying vocabulary while working through a {genre}. The reader''s cursor is on a segment of the {genre} that contains a vocabulary word, and they want to understand how the word works — here and across the author''s other works.

Discuss, in this order: first, what the word means in this segment and what work it does there — register, tone, image, characterization, irony; second, how the author uses the word elsewhere, using the lines supplied under CORPUS OCCURRENCES as your primary evidence, grouping observations by work and noting shifts in sense or register between uses; third, anything about the word itself that helps a reader building vocabulary, such as etymology or an older sense, but only when it genuinely illuminates the usage.

Ground every claim in the supplied segment and occurrence lines. You may quote briefly from the supplied lines when discussing them. If CORPUS OCCURRENCES says none were found, say plainly that the word appears only here in the available corpus and focus on this segment''s usage.

Write 10 to 15 sentences of flowing prose. Keep paragraphs short — two to four sentences — and separate paragraphs with a blank line. No markdown, no bullet lists, no numbered lists, no headers. Do not use the = sign; write paraphrases as prose.',
1, 'vocab journal Q&A (R in vocab popup) — seeded with linux-lit compiled fallback v1');
SQL
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT prompt_key, version, is_active FROM api_prompts WHERE prompt_key='journal.vocab';"
```

Expected: one row, `journal.vocab|1|1`.

- [ ] **Step 2: Build + prepare a DB copy with a seeded long answer.**
  Everything headless runs against a COPY so the live lit.db and the user's
  session are untouched. `$SCRATCH` below is the session scratchpad
  directory.

```bash
cd ~/utono/linux-lit && cargo build
SCRATCH=/tmp/claude-1000/-home-mlj-utono-linux-lit/56960eed-921b-416a-b8b4-607e398820f6/scratchpad
\cp -f ~/utono/litdb/data/lit.db "$SCRATCH/lit-e2e.db"
```

Pick the target work + word from whatever the dev config will load
(headless instances read `last_work` from `~/.config/linux-lit/config-dev.json`):

```bash
WORK=$(jq -r .last_work ~/.config/linux-lit/config-dev.json)
echo "$WORK"
```

Find a vocab word that appears on a line of that work (vocab table names
are in `src/db/queries.rs:540-640` — check the actual schema first, then
intersect):

```bash
sqlite3 "$SCRATCH/lit-e2e.db" ".schema vocab_words" | head -5
# Adjust the column list to the schema printed above if it differs:
sqlite3 "$SCRATCH/lit-e2e.db" \
  "SELECT vw.word, lm.div1, COALESCE(lm.div2,0), lm.canonical_text
   FROM vocab_words vw
   JOIN line_mapping lm ON lm.normalized_text LIKE '%' || vw.word || '%'
   WHERE lm.work_abbrev = '$WORK' LIMIT 5;"
```

Record one `(WORD, DIV1, DIV2)` triple from the output, then seed a stored
answer long enough to force at least two popup pages (24 short sentences):

```bash
WORD=<word from output>; DIV1=<div1>; DIV2=<div2>
ANSWER=$(python3 - "$WORD" <<'PY'
import sys
w = sys.argv[1]
s = [f"Sentence {i+1} about the word {w} in this segment and across the corpus, padded so the answer overflows one popup panel." for i in range(24)]
print("\n\n".join(" ".join(s[i:i+3]) for i in range(0, 24, 3)))
PY
)
sqlite3 "$SCRATCH/lit-e2e.db" \
  "INSERT INTO journal_entries (work_abbrev, div1, div2, question, answer,
      claude_model, scope, start_citation, end_citation, source_text, kind, word)
   VALUES ('$WORK', $DIV1, $DIV2,
      '“$WORD” in this segment, and across the author', '$ANSWER',
      'seeded-e2e', 'passage', 'seed', 'seed', 'seed', 'vocab', '$WORD');"
```

Note: `find_vocab_page` matches on `(work_abbrev=canonical, div1, div2,
word)` — the seeded div pair must be the division of the line you will land
on. If `$WORK` is a media variant (e.g. `BH-Vance`), seed under its
canonical base abbrev (strip the `-suffix`).

- [ ] **Step 3: Launch headless and drive.** Per CLAUDE.md headless rules
  (cairo renderer mandatory, `LIT_NO_MPV=1`, scoped pkill only):

```bash
LIT_DB_PATH="$SCRATCH/lit-e2e.db" LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 3
export WAYLAND_DISPLAY=$(ls /run/user/1000 | rg '^wayland-' | tail -1)
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 2
# Land on the seeded line: search for the word (/ then text then Return).
wtype "/$WORD"; sleep 1; wtype -k Return; sleep 1
# Open the vocab popup on the line, then the journal Q&A.
wtype -k r; sleep 1
wtype -k R; sleep 2
grim "$SCRATCH/vocab-qa-page1.png"
# Page forward, capture again.
wtype -M ctrl -k n -m ctrl; sleep 1
grim "$SCRATCH/vocab-qa-page2.png"
pkill -f "cage -- ./target/debug/linux-lit"
```

(An empty ~2-byte PNG means not-mapped-yet — `sleep 3` and re-grim; check
`stat -c%s` before Read-ing.)

- [ ] **Step 4: Review the screenshots** (UI review protocol: open every
  PNG and report what you see inline). Verify by eye:

1. `vocab-qa-page1.png`: the popup shows JOURNAL Q&A header, the dim
   `Q ·` line, seeded answer text, the pinned word + definition at the
   bottom, and a footer `saved · seeded-e2e — page 1 / N · C-n ▸` with
   N ≥ 2. Nothing clipped at the panel edges.
2. `vocab-qa-page2.png`: answer body advanced (different sentences
   visible), footer reads `page 2 / N`, and the pinned word + definition
   block is IDENTICAL to page 1 (it must not scroll).

If the popup never opened, confirm the search actually landed on a line
with the word (front matter has no vocab; try a different `(WORD, DIV)`
triple).

- [ ] **Step 5: Full verification + wrap up**

```bash
cargo test --bins
cargo clippy
```

Expected: clippy clean for the new code; tests PASS except the pre-existing
`test_load_work_hamlet`.

```bash
git status
```

Expected: clean tree (screenshots + DB copy live in the scratchpad, not the
repo).

- [ ] **Step 6: Live-check note for the user.** Real-API `R` ask (fresh
  word) needs the user's key and live session — hand them: restart `crll`,
  cursor on a vocab line, `r` to open the popup, `R` to ask; expect pending
  → answer in the popup, entry visible in the journal overlay (Ctrl+j) for
  that scene band, and `Ctrl+n`/`Ctrl+p` paging on a long answer.

---

## Self-Review Notes

- Spec coverage: trigger/guard (T5), one-word-per-ask via popup index (T5),
  immediate send + insert-before-render (T5), 10–15 sentence prompt (T2/T7),
  corpus evidence with variant exclusion + none-found framing (T2),
  `kind='vocab'` passage rows + `word` column + exact reuse (T1, T5),
  Journal view with pending/answer/error states, pinned word+definition,
  footer with page indicator hidden while pending (T3/T4), Ctrl+n/p paging
  (T3/T5/T6), three-place keybind bookkeeping + overlay (T6), journal
  overlay renders vocab rows via the existing non-note branch (verified in
  T1's scene-band test; no overlay change needed), headless e2e + live
  check (T7).
- The `unit_label` titlecase in T5 duplicates `titlecase_first` in
  journal.rs (private there); three lines inline beats a visibility change.
