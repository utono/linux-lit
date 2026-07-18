//! Cross-corpus regex search popup widget (Ctrl+f, wired in a later task).
//! Cloned from `gloss_picker.rs`'s card scaffolding; the data/filter layer
//! caches both corpora and delegates matching to the pure
//! `input::corpus_search` module (Task 1) via `search::build_matcher`.

use std::cell::{Cell, RefCell};

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Overlay};

use crate::input::corpus_search::{self, Corpus, CorpusHit, GlossRow, JournalRow};

/// Header title per active corpus. The active corpus is spelled out; the caret
/// points at what Tab switches to. Kept terse so the styled title band reads
/// cleanly (uppercase + letter-spacing come from `.library-picker-title` CSS).
const HEADER_JOURNAL: &str = "JOURNAL  ·  regex  ·  ⇥ gloss";
const HEADER_GLOSS: &str = "GLOSS  ·  regex  ·  ⇥ journal";

pub struct CorpusSearchPopup {
    pub overlay: Overlay,
    scrim: GtkBox,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    header: Label,
    corpus: Cell<Corpus>,
    journal_rows: RefCell<Vec<JournalRow>>,
    gloss_rows: RefCell<Vec<GlossRow>>,
    hits: RefCell<Vec<CorpusHit>>,
}

impl CorpusSearchPopup {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = crate::ui::picker_nav::build_picker_card();

        // Styled header BAND (matches every other card picker) rather than a bare
        // label jammed against the card's top edge. The title carries the active
        // corpus + "regex" mode indicator; `set_corpus` rewrites it.
        let (header_box, header) = crate::ui::picker_nav::build_picker_header(HEADER_JOURNAL);

        let search_entry = Entry::builder()
            .placeholder_text("Search journal / gloss  (regex)")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        picker_box.set_visible(false);

        CorpusSearchPopup {
            overlay,
            scrim: crate::ui::picker_nav::build_picker_scrim(),
            picker_box,
            search_entry,
            list_box,
            header,
            corpus: Cell::new(Corpus::Journal),
            journal_rows: RefCell::new(Vec::new()),
            gloss_rows: RefCell::new(Vec::new()),
            hits: RefCell::new(Vec::new()),
        }
    }

    pub fn set_rows(&mut self, journal: Vec<JournalRow>, gloss: Vec<GlossRow>) {
        *self.journal_rows.borrow_mut() = journal;
        *self.gloss_rows.borrow_mut() = gloss;
    }

    pub fn set_corpus(&self, c: Corpus) {
        self.corpus.set(c);
        self.header.set_text(match c {
            Corpus::Journal => HEADER_JOURNAL,
            Corpus::Gloss => HEADER_GLOSS,
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
        crate::ui::picker_nav::clear_list(&self.list_box);

        let re = crate::input::search::build_matcher(query);
        let hits = match self.corpus.get() {
            Corpus::Journal => corpus_search::filter_journal(&self.journal_rows.borrow(), &re),
            Corpus::Gloss => corpus_search::filter_gloss(&self.gloss_rows.borrow(), &re),
        };

        for h in &hits {
            // Two-part row: the entry text (primary, ellipsized) + a dimmed,
            // right-aligned work·location column (`picker-item-detail`), so the
            // eye scans content first and the citation reads as a quiet index.
            let hbox = crate::ui::picker_nav::two_label_row(&h.label, &h.detail);
            let row = ListBoxRow::builder().child(&hbox).build();
            self.list_box.append(&row);
        }

        *self.hits.borrow_mut() = hits;
        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn selected_hit(&self) -> Option<CorpusHit> {
        let idx = self.list_box.selected_row()?.index();
        if idx < 0 {
            return None;
        }
        self.hits.borrow().get(idx as usize).cloned()
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
        self.scrim.set_visible(false);
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay,
            base,
            &self.scrim,
            &self.picker_box,
        );
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }
}
