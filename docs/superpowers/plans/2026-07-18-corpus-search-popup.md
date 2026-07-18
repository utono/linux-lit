# Cross-corpus Regex Search Popup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Ctrl+f` fzf-style popup that regex-searches the whole journal Q&A corpus or the whole reader-gloss corpus (Tab toggles), cross-work, and on select opens the matching entry's overlay with the match highlighted.

**Architecture:** A pure, GTK-free search core (`corpus_search.rs`) filters already-loaded rows with the existing `search::build_matcher` regex engine. Two new cross-work DB loaders feed it. A cloned picker widget (`corpus_search_popup.rs`, modeled on `gloss_picker.rs`) drives live filtering; a select handler reuses existing work-load + overlay-open + `/`-highlight paths.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite, the `regex` crate (already a dependency), the project's cage/grim/wtype headless e2e harness.

**Design doc:** `docs/superpowers/specs/2026-07-18-corpus-search-popup-design.md`

## Global Constraints

- Regex matching MUST use `search::build_matcher(query) -> regex::Regex` (smart-case, literal-fallback baked in). It is `pub(crate)` — callable from any module in this crate. Never reimplement matching; never route through `journal_fts`.
- Any `GtkEntry::connect_changed` handler that can fire during `show()`/reset (which calls `set_text("")`, emitting `changed` synchronously) MUST use `state.try_borrow()`, not `state.borrow()`, or it double-borrows → non-unwinding abort. Signal wiring lives in `src/app/mod.rs`, matching existing pickers.
- New picker panels attach via `picker_attach::attach_overlay_panel` (an `add_overlay` layer) — NEVER inserted into the size-bearing widget chain.
- Gloss reader glosses have `gloss_type = 'reader-gloss'` (per `find_glossed_passages`). The gloss body column is `glosses.gloss_text`; work/citation/speaker come from joining `passages` (`work_abbrev`, `start_citation`, `character`).
- Work abbrevs from gloss/journal rows may be edition variants (e.g. `-Amb`); compare/open through `Work::canonical_abbrev` normalization, as the gloss picker load path already does.
- `cargo build` to verify; do NOT run the app (`cargo run`) — the user launches it. Headless e2e uses the cage harness in CLAUDE.md.
- Every keybind change also updates: the compiled default (`keymap_config.rs`) AND the stowed `~/.config/linux-lit/keymap.json` (or JSON shadows the compiled bind); the Ctrl+/ reader overlay (`keybinds_overlay.rs`); and the journal + gloss overlay legends where `Ctrl+f` is now live.
- Tests marked `#[ignore]` (the cage e2e) stay ignored so bare `cargo test` is green; unit/DB tests run under `cargo test --bins`.

---

### Task 1: Pure search core — `corpus_search.rs`

**Files:**
- Create: `src/input/corpus_search.rs`
- Modify: `src/input/mod.rs` (add `pub mod corpus_search;`)

**Interfaces:**
- Consumes: `crate::input::search::build_matcher` (returns `regex::Regex`).
- Produces:
  - `pub enum Corpus { Journal, Gloss }` (derives `Clone, Copy, PartialEq, Debug`)
  - `pub struct JournalRow { pub id: i64, pub work_abbrev: String, pub div1: i32, pub div2: i32, pub question: String, pub answer: String }`
  - `pub struct GlossRow { pub gloss_id: i64, pub work_abbrev: String, pub start_citation: String, pub speaker: String, pub gloss_text: String }`
  - `pub struct CorpusHit { pub corpus: Corpus, pub entry_id: i64, pub work_abbrev: String, pub label: String, pub sort_key: (String, i32, i32) }` (derives `Clone, Debug`)
  - `pub fn filter_journal(rows: &[JournalRow], re: &regex::Regex) -> Vec<CorpusHit>`
  - `pub fn filter_gloss(rows: &[GlossRow], re: &regex::Regex) -> Vec<CorpusHit>`
  - `pub fn journal_label(row: &JournalRow) -> String` and `pub fn gloss_label(row: &GlossRow) -> String` (used by the widget too)

- [ ] **Step 1: Write the failing tests**

Create `src/input/corpus_search.rs` with only the test module (types/functions absent so it fails to compile → RED):

```rust
//! Pure, GTK-free cross-corpus regex filtering for the Ctrl+f search popup.
//! Mirrors the pure/gtk split of `overlay_search`. Matching reuses
//! `search::build_matcher` (smart-case, literal fallback).

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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bins corpus_search 2>&1 | tail -20`
Expected: compile error (`cannot find type JournalRow`, `filter_journal` undefined).

- [ ] **Step 3: Write the minimal implementation**

Prepend above the `#[cfg(test)]` module:

```rust
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
```

Add `pub mod corpus_search;` to `src/input/mod.rs` (alphabetically near `concordance`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bins corpus_search 2>&1 | tail -12`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/input/corpus_search.rs src/input/mod.rs
git commit -m "feat(corpus-search): pure GTK-free regex filter core"
```

---

### Task 2: Cross-work DB loaders

**Files:**
- Modify: `src/db/journal.rs` (add `list_all_journal_rows`)
- Modify: `src/db/queries.rs` (add `list_all_gloss_rows`)
- Test: inline `#[cfg(test)]` in each (follow the existing test style in those files — e.g. `find_glossed_passages`'s test at `queries.rs:3620` builds an in-memory or fixture DB).

**Interfaces:**
- Consumes: `crate::input::corpus_search::{JournalRow, GlossRow}`.
- Produces:
  - `pub fn list_all_journal_rows(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<crate::input::corpus_search::JournalRow>>`
  - `pub fn list_all_gloss_rows(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<crate::input::corpus_search::GlossRow>>`

- [ ] **Step 1: Write the failing test (journal loader)**

In `src/db/journal.rs` test module, add a test that opens the real DB read-only and asserts the loader returns rows with non-empty bodies. Follow the pattern of the existing journal tests in that file (they use `crate::db::queries::open_db()`):

```rust
#[test]
fn list_all_journal_rows_loads_body_text() {
    let conn = crate::db::queries::open_db().unwrap();
    let rows = list_all_journal_rows(&conn).unwrap();
    // Whatever the corpus, every row must carry its answer text (regression
    // guard: the loader must SELECT answer, not just metadata).
    assert!(rows.iter().all(|r| !r.work_abbrev.is_empty()));
    if let Some(r) = rows.first() {
        assert!(!r.answer.is_empty() || !r.question.is_empty());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins list_all_journal_rows 2>&1 | tail -15`
Expected: compile error (`list_all_journal_rows` undefined).

- [ ] **Step 3: Implement the journal loader**

In `src/db/journal.rs`:

```rust
/// Every journal entry across all works, with body text, for the Ctrl+f
/// cross-corpus search popup. Ordered by work then band for stable display.
pub fn list_all_journal_rows(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Vec<crate::input::corpus_search::JournalRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, work_abbrev, div1, div2, question, answer
         FROM journal_entries
         ORDER BY work_abbrev, div1, div2, id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(crate::input::corpus_search::JournalRow {
                id: r.get(0)?,
                work_abbrev: r.get(1)?,
                div1: r.get(2)?,
                div2: r.get(3)?,
                question: r.get(4)?,
                answer: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins list_all_journal_rows 2>&1 | tail -8`
Expected: PASS.

- [ ] **Step 5: Write the failing test (gloss loader)**

In `src/db/queries.rs` test module (mirror the `find_glossed_passages` test at line ~3620):

```rust
#[test]
fn list_all_gloss_rows_loads_gloss_text() {
    let conn = open_db().unwrap();
    let rows = list_all_gloss_rows(&conn).unwrap();
    // Regression guard for the citation-only gap: gloss_text MUST be loaded.
    if let Some(r) = rows.first() {
        assert!(!r.gloss_text.is_empty());
        assert!(!r.work_abbrev.is_empty());
    }
}
```

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test --bins list_all_gloss_rows 2>&1 | tail -15`
Expected: compile error (`list_all_gloss_rows` undefined).

- [ ] **Step 7: Implement the gloss loader**

In `src/db/queries.rs`:

```rust
/// Every reader-gloss gloss across all works, with body text + citation +
/// speaker, for the Ctrl+f cross-corpus search popup. Joins passages for the
/// work/citation/speaker that the glosses row lacks.
pub fn list_all_gloss_rows(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Vec<crate::input::corpus_search::GlossRow>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, p.work_abbrev, COALESCE(p.start_citation, ''),
                COALESCE(p.character, ''), g.gloss_text
         FROM glosses g
         JOIN passages p ON p.id = g.passage_id
         WHERE g.gloss_type = 'reader-gloss'
         ORDER BY p.work_abbrev, p.start_citation, g.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(crate::input::corpus_search::GlossRow {
                gloss_id: r.get(0)?,
                work_abbrev: r.get(1)?,
                start_citation: r.get(2)?,
                speaker: r.get(3)?,
                gloss_text: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
```

- [ ] **Step 8: Run to verify it passes**

Run: `cargo test --bins list_all_gloss_rows 2>&1 | tail -8`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/db/journal.rs src/db/queries.rs
git commit -m "feat(corpus-search): cross-work journal + gloss body loaders"
```

---

### Task 3: Popup widget — `corpus_search_popup.rs`

**Files:**
- Create: `src/ui/corpus_search_popup.rs`
- Modify: `src/ui/mod.rs` (add `pub mod corpus_search_popup;`)
- Read for template: `src/ui/gloss_picker.rs` (clone), `src/ui/picker_nav.rs`, `src/ui/picker_attach.rs`.

**Interfaces:**
- Consumes: `corpus_search::{Corpus, JournalRow, GlossRow, CorpusHit, filter_journal, filter_gloss}`, `search::build_matcher`, `picker_attach::attach_overlay_panel`, `picker_nav`.
- Produces `pub struct CorpusSearchPopup` with:
  - `pub fn new() -> Self`
  - `pub fn attach(&self, base: &impl IsA<gtk4::Widget>)`
  - `pub fn search_entry(&self) -> &gtk4::Entry`
  - `pub fn set_rows(&mut self, journal: Vec<JournalRow>, gloss: Vec<GlossRow>)` — cache both corpora
  - `pub fn set_corpus(&self, c: Corpus)` and `pub fn toggle_corpus(&self) -> Corpus` (updates header, returns new corpus)
  - `pub fn corpus(&self) -> Corpus`
  - `pub fn populate_list(&self, query: &str)` — filter active corpus, rebuild list, select row 0
  - `pub fn selected_hit(&self) -> Option<CorpusHit>`
  - `pub fn move_selection(&self, delta: i32)`
  - `pub fn show(&self)` / `pub fn hide(&self)`

- [ ] **Step 1: Read the template**

Read `src/ui/gloss_picker.rs` in full (112 lines). The new widget is that structure with: (a) both corpora cached in `RefCell<Vec<..>>` fields, (b) a `Cell<Corpus>` for the active corpus, (c) a header `Label`, (d) `populate_list` compiling `build_matcher` and calling `filter_journal`/`filter_gloss` instead of `subsequence_match`, storing the resulting `Vec<CorpusHit>` in a `RefCell` so `selected_hit()` can read it.

- [ ] **Step 2: Write the widget**

Create `src/ui/corpus_search_popup.rs`. Use `gloss_picker.rs` verbatim for the `attach`/`show`/`hide`/`search_entry`/`move_selection`/`selected_index` scaffolding (build the card via `picker_nav::build_picker_card` etc. exactly as gloss_picker does). Replace the data + filter parts:

```rust
use std::cell::{Cell, RefCell};
use gtk4::prelude::*;
use gtk4::{Entry, Label, ListBox};

use crate::input::corpus_search::{
    self, Corpus, CorpusHit, GlossRow, JournalRow,
};

pub struct CorpusSearchPopup {
    // ... same overlay/scrim/picker_box/search_entry/list_box fields as
    // GlossPicker (copy them) ...
    overlay: gtk4::Overlay,
    scrim: gtk4::Box,
    picker_box: gtk4::Box,
    search_entry: Entry,
    list_box: ListBox,
    header: Label,
    corpus: Cell<Corpus>,
    journal_rows: RefCell<Vec<JournalRow>>,
    gloss_rows: RefCell<Vec<GlossRow>>,
    hits: RefCell<Vec<CorpusHit>>,
}

impl CorpusSearchPopup {
    // new(): build widgets like GlossPicker::new(); add `header` label to the
    // picker_box top; init corpus = Journal.

    pub fn set_rows(&self, journal: Vec<JournalRow>, gloss: Vec<GlossRow>) {
        *self.journal_rows.borrow_mut() = journal;
        *self.gloss_rows.borrow_mut() = gloss;
    }

    pub fn corpus(&self) -> Corpus { self.corpus.get() }

    pub fn set_corpus(&self, c: Corpus) {
        self.corpus.set(c);
        self.header.set_text(match c {
            Corpus::Journal => "[JOURNAL | gloss]   (regex)",
            Corpus::Gloss   => "[journal | GLOSS]   (regex)",
        });
    }

    pub fn toggle_corpus(&self) -> Corpus {
        let next = match self.corpus.get() {
            Corpus::Journal => Corpus::Gloss,
            Corpus::Gloss => Corpus::Journal,
        };
        self.set_corpus(next);
        next
    }

    pub fn populate_list(&self, query: &str) {
        // clear list_box children (same as gloss_picker)
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
        let re = crate::input::search::build_matcher(query);
        let hits = match self.corpus.get() {
            Corpus::Journal => corpus_search::filter_journal(&self.journal_rows.borrow(), &re),
            Corpus::Gloss   => corpus_search::filter_gloss(&self.gloss_rows.borrow(), &re),
        };
        for h in &hits {
            let row = gtk4::ListBoxRow::new();
            let lbl = Label::new(Some(&h.label));
            lbl.set_xalign(0.0);
            lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            row.set_child(Some(&lbl));
            self.list_box.append(&row);
        }
        *self.hits.borrow_mut() = hits;
        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_hit(&self) -> Option<CorpusHit> {
        let idx = self.list_box.selected_row()?.index();
        if idx < 0 { return None; }
        self.hits.borrow().get(idx as usize).cloned()
    }

    // attach(), show(), hide(), search_entry(), move_selection():
    // COPY from GlossPicker verbatim (attach delegates to
    // picker_attach::attach_overlay_panel).
}
```

Add `pub mod corpus_search_popup;` to `src/ui/mod.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | rg -i "error\[" | head` (empty = clean)
Expected: no errors. (Widget is not yet wired to state; unused-warnings are fine.)

- [ ] **Step 4: Commit**

```bash
git add src/ui/corpus_search_popup.rs src/ui/mod.rs
git commit -m "feat(corpus-search): popup widget (clone of gloss_picker)"
```

---

### Task 4: Input mode + open/keymap wiring

**Files:**
- Modify: `src/app/mod.rs` — add `InputMode::CorpusSearch`; add the popup to `AppState` (field `pub corpus_search_popup: CorpusSearchPopup`, plus `pub last_corpus: crate::input::corpus_search::Corpus`); attach it where other pickers attach; wire `connect_changed` (GUARDED).
- Create: `src/input/actions/corpus_search.rs` — `open`.
- Modify: `src/input/actions/mod.rs` — add `Action::OpenCorpusSearch` variant + its `as_str`/category arms (mirror `OpenGlossPicker`).
- Modify: `src/input/keymap.rs` — dispatch `Ctrl+f` → open from reader/journal-overlay/gloss-overlay; route `CorpusSearch` mode keys.
- Modify: `src/input/keymap_config.rs` — `(KeyCombo::ctrl("f"), Action::OpenCorpusSearch)`.

**Interfaces:**
- Consumes: Task 3's `CorpusSearchPopup`, Task 1/2 loaders.
- Produces: `pub fn open(state: &Rc<RefCell<AppState>>)` in `actions/corpus_search.rs`; `Action::OpenCorpusSearch`; `InputMode::CorpusSearch`.

- [ ] **Step 1: Add the InputMode + AppState field**

In `src/app/mod.rs`: add `CorpusSearch,` to `enum InputMode` (near the other picker modes). Add to `AppState`: `pub corpus_search_popup: crate::ui::corpus_search_popup::CorpusSearchPopup,` and `pub last_corpus: crate::input::corpus_search::Corpus,` (init `Corpus::Journal` in the constructor). Attach the popup next to the other picker `.attach(...)` calls.

- [ ] **Step 2: Add the Action variant**

In `src/input/actions/mod.rs`: add `OpenCorpusSearch,` to `enum Action`; add its arm to the `as_str`/name match (`Action::OpenCorpusSearch => "OpenCorpusSearch"`) and any category grouping, mirroring `OpenGlossPicker`.

- [ ] **Step 3: Write `open`**

Create `src/input/actions/corpus_search.rs`:

```rust
//! Ctrl+f cross-corpus regex search popup: open (load both corpora), and the
//! select handler that jumps to the chosen entry with the match highlighted.

use std::cell::RefCell;
use std::rc::Rc;
use crate::app::{AppState, InputMode};
use crate::input::corpus_search::Corpus;

/// Open the popup. Loads both corpora once, defaults corpus to the opening
/// context's kind (gloss/journal overlay) or the last-used corpus otherwise.
pub fn open(state: &Rc<RefCell<AppState>>) {
    let (journal, gloss) = {
        let conn = crate::db::queries::open_db()
            .expect(crate::db::queries::OPEN_DB_PANIC_MSG);
        let j = crate::db::journal::list_all_journal_rows(&conn).unwrap_or_default();
        let g = crate::db::queries::list_all_gloss_rows(&conn).unwrap_or_default();
        (j, g)
    };
    let mut s = state.borrow_mut();
    let corpus = match s.input_mode {
        InputMode::GlossOverlay => Corpus::Gloss,
        InputMode::JournalOverlay => Corpus::Journal,
        _ => s.last_corpus,
    };
    // Remember where to return on Escape.
    s.corpus_search_return_mode = s.input_mode; // add this field to AppState
    s.corpus_search_popup.set_rows(journal, gloss);
    s.corpus_search_popup.set_corpus(corpus);
    s.corpus_search_popup.search_entry().set_text(""); // emits changed (guarded)
    s.corpus_search_popup.populate_list("");
    s.corpus_search_popup.show();
    s.input_mode = InputMode::CorpusSearch;
}
```

(Add `pub corpus_search_return_mode: InputMode` to `AppState`, init `Reader`. Add `pub mod corpus_search;` to `src/input/actions/mod.rs`.)

- [ ] **Step 4: Wire the GUARDED connect_changed**

In `src/app/mod.rs`, beside the journal_term_input `try_borrow`-guarded wiring (~line 2580), add:

```rust
{
    let state_cc = state.clone();
    state.borrow().corpus_search_popup.search_entry().connect_changed(move |entry| {
        // set_text("") in open() fires this synchronously under borrow_mut;
        // try_borrow prevents the double-borrow abort.
        if let Ok(s) = state_cc.try_borrow() {
            s.corpus_search_popup.populate_list(&entry.text());
        }
    });
}
```

- [ ] **Step 5: Route the keys in keymap.rs**

Add `Ctrl+f` open dispatch in the reader, journal-overlay, and gloss-overlay key handlers (gate on `is_ctrl && key_name == "f"` → `crate::input::actions::corpus_search::open(state)`). Add a `CorpusSearch` mode arm handling: `Return` → select (Task 5), `Tab`/`ISO_Left_Tab` → `toggle_corpus()` then `populate_list(current query)`, `Up`/`Down` → `move_selection`, `Escape` → hide + restore `corpus_search_return_mode`. Model the arm on the `JournalTermInput` mode arm. The `Action::OpenCorpusSearch` dispatch (`keymap.rs` action match + `keymap_config.rs` default bind) mirrors `OpenGlossPicker`.

- [ ] **Step 6: Verify build**

Run: `cargo build 2>&1 | rg -i "error\[" | head`
Expected: no errors.

- [ ] **Step 7: Commit**

```bash
git add src/app/mod.rs src/input/actions/mod.rs src/input/actions/corpus_search.rs \
        src/input/keymap.rs src/input/keymap_config.rs
git commit -m "feat(corpus-search): Ctrl+f open, mode routing, guarded live filter"
```

---

### Task 5: Select handler — jump + highlight

**Files:**
- Modify: `src/input/actions/corpus_search.rs` — add `select`.
- Read for template: `src/input/actions/concordance.rs` (cross-work `load_work` + `display_work_at_with_prepared` at ~line 457-471), `src/input/actions/journal.rs` (`render_filtered_match`/`show_page` for showing an arbitrary entry; `toggle_overlay`), `src/input/actions/gloss.rs` (gloss overlay open path), `src/ui/{journal,gloss}_overlay.rs::set_search_matches`.

**Interfaces:**
- Consumes: `CorpusSearchPopup::selected_hit`, `overlay_search::{OverlaySearch, collect}`, the overlays' `set_search_matches`/`reapply_search`.
- Produces: `pub fn select(state: &Rc<RefCell<AppState>>)`.

- [ ] **Step 1: Implement `select`**

```rust
/// Enter on a result: hide the popup, load the hit's work if needed, open the
/// matching overlay on that entry, and seed the overlay's `/` search with the
/// popup pattern so the match highlights (n/N step within the entry).
pub fn select(state: &Rc<RefCell<AppState>>) {
    let (hit, pattern) = {
        let s = state.borrow();
        let Some(hit) = s.corpus_search_popup.selected_hit() else { return };
        (hit, s.corpus_search_popup.search_entry().text().to_string())
    };
    {
        let mut s = state.borrow_mut();
        s.corpus_search_popup.hide();
        s.last_corpus = hit.corpus;
    }

    // 1. Load the work if it isn't the current one (normalize via canonical
    //    abbrev — see concordance.rs cross-work load, queries::load_work +
    //    app::display_work_at_with_prepared).
    // 2. Open the journal or gloss overlay positioned on hit.entry_id
    //    (journal: the render_filtered_match / show_page path; gloss: the
    //    gloss-overlay open path keyed by gloss id).
    // 3. Seed the overlay search:
    //       let os = crate::input::overlay_search::OverlaySearch {
    //           pattern: pattern.clone(),
    //           matches: crate::input::overlay_search::collect(&entry_text, &pattern),
    //           current: 0,
    //       };
    //       overlay.set_search_matches(&os);   // journal_overlay OR gloss_overlay
    //    where `entry_text` is the overlay's rendered buffer text for that entry.
    //
    // Follow the cited templates for the exact overlay-open calls; every
    // building block already exists (this task wires them, adds no new engine).
}
```

Wire the `CorpusSearch` mode `Return` arm (Task 4, Step 5) to call `select`.

- [ ] **Step 2: Verify build**

Run: `cargo build 2>&1 | rg -i "error\[" | head`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/corpus_search.rs src/input/keymap.rs
git commit -m "feat(corpus-search): select loads work, opens entry, seeds highlight"
```

---

### Task 6: Keybind mirrors (overlays + JSON) and unit sweep

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` — add `Ctrl+f` to the reader Ctrl+/ overlay (keycap strip + `describe()` arm) via the `update-cairo-keybinds-overlay` skill's three-pass cross-reference.
- Modify: `src/ui/journal_keybinds_overlay.rs` and `src/ui/gloss_keybinds_overlay.rs` — add a `Ctrl+f  search all Q&As / glosses` line to each `GROUPS` const.
- Modify: `~/.config/linux-lit/keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`) — add `{"key": "f", "ctrl": true, "action": "OpenCorpusSearch"}`. Deploy: `cd ~/tty-dotfiles && stow linux-lit`.

- [ ] **Step 1: Update the reader Ctrl+/ overlay**

Use the `update-cairo-keybinds-overlay` skill. Add `Ctrl+f` → "search all Q&As / glosses" to the keycap strip and the `describe()` detail arm. Run its three-pass cross-reference.

- [ ] **Step 2: Update the two overlay legends**

Add the `Ctrl+f` line to `journal_keybinds_overlay.rs` and `gloss_keybinds_overlay.rs` `GROUPS`.

- [ ] **Step 3: Update + deploy keymap.json**

Add the bind to `~/tty-dotfiles/linux-lit/...keymap.json`; `cd ~/tty-dotfiles && stow linux-lit`.

- [ ] **Step 4: Full unit sweep**

Run: `cargo test --bins 2>&1 | tail -5`
Expected: all pass (pre-existing `theme_cycle_defaults_to_reading_themes` failure, if still present from unrelated uncommitted `theme.rs`, is out of scope — note it, don't fix here). `cargo clippy 2>&1 | rg -i "warning: .*corpus" | head` — clean up any clippy warnings in the new files.

- [ ] **Step 5: Commit**

```bash
git add src/ui/keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs src/ui/gloss_keybinds_overlay.rs
git commit -m "feat(corpus-search): Ctrl+f in reader + overlay keybind legends"
```

---

### Task 7: Headless e2e verification

**Files:**
- Read: `CLAUDE.md` "Headless Verification" section; `scripts/e2e-env.sh`.

- [ ] **Step 1: Build + launch a throwaway cage instance**

```bash
cd ~/utono/linux-lit && cargo build
export XDG_RUNTIME_DIR=/run/user/1000
LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit 2>/tmp/cage-corpus.log &
sleep 4
```
Find the cage wayland socket (`ls /run/user/1000/wayland-*`), export `WAYLAND_DISPLAY`, `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`.

- [ ] **Step 2: Drive and screenshot each acceptance criterion**

With `wtype` (confirm current key names in `keymap_config.rs`; give 3s to map; check `stat -c%s` before Read):
1. `Ctrl+f` from the reader → popup opens; screenshot shows the entry box, `[JOURNAL | gloss] (regex)` header, a result list.
2. Type a regex (e.g. `bello|beaut`) → list filters to matching Q&As.
3. `Tab` → header flips to `[journal | GLOSS]`; list re-filters to gloss hits.
4. Select a row + `Return` → the entry overlay opens (loading its work if needed) with the matched term highlighted; `n`/`N` step matches.
5. `Ctrl+f` → `Escape` → returns to the opening context.

Open every PNG and report what you see inline (UI review protocol). A passing exit code is not enough.

- [ ] **Step 3: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```
(ONLY this exact pattern — a bare `pkill -f target/debug/linux-lit` kills the user's live instance.)

- [ ] **Step 4: Final commit (if any harness tweaks were needed)**

```bash
git add -A && git commit -m "test(corpus-search): headless e2e drive of the search popup"
```

---

## Finishing

Per the project convention: once tests pass and the tree is clean, merge `feat/corpus-search-popup` back to `master` from the main checkout (`git checkout master && git merge --no-ff`), re-verify `cargo build`, `git push origin master`, `git branch -d`. The user's uncommitted `chat.rs`/`theme.rs`/`headless-e2e-env.md` must be preserved throughout — never `git add -A` them into a corpus-search commit.

## Self-Review notes

- **Spec coverage:** corpus toggle (T3/T4), cross-work (T2 loaders + T5 load), regex+smart-case (T1 via build_matcher), one-row-per-entry (T1 dedup by filter+map), select-opens-overlay-with-highlight (T5), Ctrl+f from three contexts (T4), error/edge (empty corpus → empty list is inherent; invalid regex → T1 test; no matches → empty list; work-load failure → follow concordance template in T5), guarded connect_changed (T4 S4), add_overlay attach (T3), legend/JSON mirrors (T6), headless verify (T7). All covered.
- **Known imprecision:** T5's exact overlay-open calls are described by template reference, not literal code, because the journal/gloss "open on an arbitrary entry id" entry points must be read from the current source (`render_filtered_match`, gloss open path) — the plan names the exact functions to model on. Every *new* surface (core, loaders, widget, mode, open) has complete code.
