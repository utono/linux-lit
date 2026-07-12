use gtk4::prelude::{Cast, WidgetExt};
use super::AppState;

/// Grouped state for the vocab popup (the Popover widget itself plus its
/// per-open data list, navigation index, view mode, auto-show flag, anchor
/// line, and fade generation counter). Was seven flat `vocab_popup*` fields on
/// AppState; grouped per the AppState god-struct decomposition (render-tier).
/// NOTE: the separate vocab-HIGHLIGHT fields (vocab_words, vocab_matches,
/// vocab_tag, vocab_highlight_visible) are a different subsystem and stay
/// flat on AppState.
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

/// What the popup's Journal view is showing. Carries the word so the async
/// reply can verify the popup still shows the word it asked about — any
/// cursor move, word cycle, or view toggle clears this, and a stale reply
/// must not repaint it (the DB insert still happens regardless).
pub enum JournalDisplay {
    Pending { word: String, question: String },
    Answer { word: String, question: String, answer: String, model: String },
    Error { word: String, question: String, message: String },
}

/// Load vocab data for all words on the current line into state, show popup with first word.
pub fn open_vocab_popup(state: &mut AppState) {
    use crate::ui::vocab_popup::{VocabWordData, VocabView};

    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    let work_abbrev = state.current_work.as_ref().map(|w| w.abbrev.clone());
    let citation = state.current_work.as_ref().and_then(|work| {
        let work_line = state.work_line_for_buffer(state.current_line)?;
        let line = work.lines.get(work_line)?;
        Some(line.citation.clone())
    });

    // Collect unique vocab words on the current line
    let current_line = state.current_line;
    crate::logging::log(&format!(
        "VOCAB POPUP: current_line={}", current_line
    ));
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = state
        .vocab_matches
        .iter()
        .filter(|m| m.line_index == current_line)
        .filter(|m| seen.insert(m.word.clone()))
        .map(|m| m.word.clone())
        .collect();

    if words.is_empty() {
        crate::logging::log("VOCAB POPUP: no vocab words on current line");
        return;
    }
    crate::logging::log(&format!("VOCAB POPUP: {} words: {:?}", words.len(), words));

    state.vocab_popup.data = words
        .into_iter()
        .map(|w| {
            let definition = crate::db::queries::load_vocab_definition(&conn, &w)
                .map(|(d, _)| d);
            let etymology_markup = crate::db::queries::load_vocab_etymology(&conn, &w)
                .map(|e| format_etymology(&e, &crate::theme::vocab_popup_accent(&state.theme)));
            let gloss = match (&work_abbrev, &citation) {
                (Some(abbrev), Some(cit)) => {
                    crate::db::queries::load_vocab_gloss(&conn, &w, abbrev, cit)
                }
                _ => None,
            };
            VocabWordData { word: w, definition, etymology_markup, gloss }
        })
        .collect();

    state.vocab_popup.index = 0;
    state.vocab_popup.view = VocabView::Definition;
    state.vocab_popup.journal = None;
    state.vocab_popup.line = Some(current_line);

    update_vocab_popup_margin(state);
    show_vocab_popup(state);
}

/// Set the vocab popup's left margin so it starts just right of the text card.
pub(crate) fn update_vocab_popup_margin(state: &AppState) {
    let window = state.text_view.root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let window = match window {
        Some(w) => w,
        None => return,
    };
    let sw_right = gtk4::graphene::Point::new(
        state.scrolled_window.width() as f32,
        0.0,
    );
    if let Some(pt) = state.scrolled_window.compute_point(&window, &sw_right) {
        let margin = (pt.x() as i32 + 12).max(0);
        state.vocab_popup.popup.set_margin_start(margin);
    }
}

/// Hide the vocab popup.
pub fn close_vocab_popup(state: &mut AppState) {
    state.vocab_popup.popup.hide();
}

/// Render the current vocab popup entry.
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

/// Refresh the vocab popup for the current line during playback sync.
/// If the new line has vocab words, update the popup content and position.
/// If it has none, close the popup.
pub fn refresh_vocab_popup(state: &mut AppState) {
    if !state.vocab_popup.popup.is_visible() {
        return;
    }

    use crate::ui::vocab_popup::{VocabWordData, VocabView};

    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    let work_abbrev = state.current_work.as_ref().map(|w| w.abbrev.clone());
    let citation = state.current_work.as_ref().and_then(|work| {
        let work_line = state.work_line_for_buffer(state.current_line)?;
        let line = work.lines.get(work_line)?;
        Some(line.citation.clone())
    });

    let current_line = state.current_line;
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = state
        .vocab_matches
        .iter()
        .filter(|m| m.line_index == current_line)
        .filter(|m| seen.insert(m.word.clone()))
        .map(|m| m.word.clone())
        .collect();

    if words.is_empty() {
        state.vocab_popup.data.clear();
        state.vocab_popup.popup.hide();
        state.vocab_popup.line = Some(current_line);
        return;
    }

    state.vocab_popup.data = words
        .into_iter()
        .map(|w| {
            let definition = crate::db::queries::load_vocab_definition(&conn, &w)
                .map(|(d, _)| d);
            let etymology_markup = crate::db::queries::load_vocab_etymology(&conn, &w)
                .map(|e| format_etymology(&e, &crate::theme::vocab_popup_accent(&state.theme)));
            let gloss = match (&work_abbrev, &citation) {
                (Some(abbrev), Some(cit)) => {
                    crate::db::queries::load_vocab_gloss(&conn, &w, abbrev, cit)
                }
                _ => None,
            };
            VocabWordData { word: w, definition, etymology_markup, gloss }
        })
        .collect();

    state.vocab_popup.index = 0;
    state.vocab_popup.view = VocabView::Definition;
    state.vocab_popup.journal = None;
    state.vocab_popup.line = Some(current_line);
    show_vocab_popup(state);
}

/// Cycling words or toggling views leaves the Journal display — it belongs
/// to one word only.
fn exit_journal_view(state: &mut AppState) {
    use crate::ui::vocab_popup::VocabView;
    if state.vocab_popup.view == VocabView::Journal {
        state.vocab_popup.view = VocabView::Definition;
    }
    state.vocab_popup.journal = None;
}

/// Cycle to the next vocab word in the popup.
pub fn vocab_popup_next(state: &mut AppState) {
    if state.vocab_popup.data.is_empty() {
        return;
    }
    exit_journal_view(state);
    state.vocab_popup.index = (state.vocab_popup.index + 1) % state.vocab_popup.data.len();
    show_vocab_popup(state);
}

pub fn vocab_popup_prev(state: &mut AppState) {
    if state.vocab_popup.data.is_empty() {
        return;
    }
    exit_journal_view(state);
    if state.vocab_popup.index == 0 {
        state.vocab_popup.index = state.vocab_popup.data.len() - 1;
    } else {
        state.vocab_popup.index -= 1;
    }
    show_vocab_popup(state);
}

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

/// Format a VocabEtymology into Pango markup.
fn format_etymology(e: &crate::db::queries::VocabEtymology, vocab_fg: &str) -> String {
    let mut parts = Vec::new();
    if let Some(ref prefix) = e.prefix {
        let gloss = e.prefix_gloss.as_deref().unwrap_or("");
        parts.push(format!(
            "<span foreground=\"{}\">{}</span> \"{}\"",
            vocab_fg,
            glib::markup_escape_text(prefix),
            glib::markup_escape_text(gloss)
        ));
    }
    if let Some(ref root) = e.root {
        let gloss = e.root_gloss.as_deref().unwrap_or("");
        if !parts.is_empty() {
            parts.push(" + ".to_string());
        }
        parts.push(format!(
            "<span foreground=\"{}\">{}</span> \"{}\"",
            vocab_fg,
            glib::markup_escape_text(root),
            glib::markup_escape_text(gloss)
        ));
    }
    if let Some(ref suffix) = e.suffix {
        let gloss = e.suffix_gloss.as_deref().unwrap_or("");
        if !parts.is_empty() {
            parts.push(" + ".to_string());
        }
        parts.push(format!(
            "<span foreground=\"{}\">{}</span> \"{}\"",
            vocab_fg,
            glib::markup_escape_text(suffix),
            glib::markup_escape_text(gloss)
        ));
    }
    parts.join("")
}
