//! Cross-corpus regex search popup widget (Ctrl+f, wired in a later task).
//! Cloned from `gloss_picker.rs`'s card scaffolding; the data/filter layer
//! caches both corpora and delegates matching to the pure
//! `input::corpus_search` module (Task 1) via `search::build_matcher`.

use std::cell::{Cell, RefCell};

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Overlay};

use crate::input::corpus_search::{self, Corpus, CorpusHit, GlossRow, JournalRow};

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

        let header = Label::builder()
            .label("[JOURNAL | gloss]   (regex)")
            .halign(gtk4::Align::Start)
            .build();

        let search_entry = Entry::builder()
            .placeholder_text("Search journal/gloss (regex)...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        picker_box.append(&header);
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

    pub fn corpus(&self) -> Corpus {
        self.corpus.get()
    }

    pub fn set_corpus(&self, c: Corpus) {
        self.corpus.set(c);
        self.header.set_text(match c {
            Corpus::Journal => "[JOURNAL | gloss]   (regex)",
            Corpus::Gloss => "[journal | GLOSS]   (regex)",
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
            let lbl = Label::new(Some(&h.label));
            lbl.set_xalign(0.0);
            lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            let row = ListBoxRow::builder().child(&lbl).build();
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
