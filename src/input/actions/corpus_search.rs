//! Ctrl+f cross-corpus regex search popup: open (load both corpora), and the
//! select handler that jumps to the chosen entry with the match highlighted
//! (select lands in a later task; this file only wires `open`).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::EditableExt;

use crate::app::{AppState, InputMode};
use crate::input::corpus_search::Corpus;

/// Open the popup. Loads both corpora once, defaults corpus to the opening
/// context's kind (gloss/journal overlay) or the last-used corpus otherwise.
pub fn open(state: &Rc<RefCell<AppState>>) {
    let (journal, gloss) = {
        let conn = crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
    s.corpus_search_return_mode = s.input_mode;
    s.corpus_search_popup.set_rows(journal, gloss);
    s.corpus_search_popup.set_corpus(corpus);
    s.corpus_search_popup.search_entry().set_text(""); // emits changed (guarded)
    s.corpus_search_popup.populate_list("");
    s.corpus_search_popup.show();
    s.input_mode = InputMode::CorpusSearch;
}
