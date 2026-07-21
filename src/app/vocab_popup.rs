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
    /// True while the popup is anchored to the CHAT PANEL: `chat::size_panel`
    /// carves the popup's slot out of the panel height (popup renders BELOW a
    /// raised panel bottom, borders on the transcript text margins) instead of
    /// the corner float. Cleared on close.
    pub chat_inline: bool,
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

/// Which words the popup loads on open.
pub enum VocabScope {
    /// Every unique vocab match on the reader's current line (the historical
    /// `rr` behavior, main card only).
    CursorLine,
    /// An explicit word list — the overlay/chat surfaces collect these from
    /// their own visible text and hand them in.
    Words(Vec<String>),
}

/// Where an opened popup anchors.
#[derive(PartialEq)]
pub enum VocabAnchor {
    /// The reader placements (`position_vocab_popup`): side strip / column float.
    Reader,
    /// Gloss/journal overlays: the same frameless strip as the reader
    /// (transparent over the scrim), corner card only when the strip is too
    /// narrow to wrap text.
    Corner,
    /// Below the chat panel: `chat::size_panel` raises the panel bottom and
    /// places the card in the freed strip, aligned with the transcript text.
    ChatPanel,
}

/// Load vocab data for the words in `scope` into state and show the popup with
/// the first word, anchored per `anchor`.
pub fn open_vocab_popup_scoped(state: &mut AppState, scope: VocabScope, anchor: VocabAnchor) {
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

    // Collect the unique word set for this scope.
    let words: Vec<String> = match scope {
        VocabScope::CursorLine => {
            let current_line = state.current_line;
            crate::logging::log(&format!(
                "VOCAB POPUP: current_line={}", current_line
            ));
            let mut seen = std::collections::HashSet::new();
            state
                .vocab_matches
                .iter()
                .filter(|m| m.line_index == current_line)
                .filter(|m| seen.insert(m.word.clone()))
                .map(|m| m.word.clone())
                .collect()
        }
        VocabScope::Words(w) => {
            let mut seen = std::collections::HashSet::new();
            w.into_iter().filter(|w| seen.insert(w.clone())).collect()
        }
    };

    if words.is_empty() {
        crate::logging::log("VOCAB POPUP: no vocab words in scope");
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
    state.vocab_popup.line = Some(state.current_line);
    state.vocab_popup.chat_inline = anchor == VocabAnchor::ChatPanel;

    match anchor {
        VocabAnchor::Reader => position_vocab_popup(state),
        VocabAnchor::Corner => position_overlay_vocab_popup(state),
        // ChatPanel: geometry is computed by chat::size_panel (called from
        // show_vocab_popup once the content is rendered and measurable).
        VocabAnchor::ChatPanel => {}
    }
    show_vocab_popup(state);
}

/// Load vocab data for all words on the current line into state, show popup with first word.
pub fn open_vocab_popup(state: &mut AppState) {
    open_vocab_popup_scoped(state, VocabScope::CursorLine, VocabAnchor::Reader);
}

/// Place the vocab popup for the current layout. Single-column: the strip
/// just right of the text card (the historical placement). Two-column: a
/// full-column float over the reading column the cursor is NOT in, mirroring
/// the chat panel's float geometry (see `chat::size_panel`). Called at every
/// (re)show so the float side follows the cursor across the column split.
pub(crate) fn position_vocab_popup(state: &AppState) {
    if state.column_count() == 2 {
        let over_right = !crate::input::actions::chat::cursor_in_right_column(state);
        let (_, card_h) = crate::app::layout::main_card_rect(state);
        let (x, w) = crate::app::layout::column_float_rect(state, over_right);
        state.vocab_popup.popup.place_float(x, w, card_h);
        crate::logging::log(&format!(
            "VOCAB POPUP: float col_x={x} col_w={w} card_h={card_h}"
        ));
        return;
    }
    if let Some(margin) = strip_margin_start(state) {
        state.vocab_popup.popup.place_strip(margin);
    }
}

/// Left margin of the strip right of the text card, in window coords (the
/// `place_strip` geometry). None when the card is not yet rooted.
fn strip_margin_start(state: &AppState) -> Option<i32> {
    let window = state.text_view.root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok())?;
    let sw_right = gtk4::graphene::Point::new(
        state.scrolled_window.width() as f32,
        0.0,
    );
    let pt = state.scrolled_window.compute_point(&window, &sw_right)?;
    Some((pt.x() as i32 + 12).max(0))
}

/// Place the popup for the gloss/journal overlays: the reader's strip
/// geometry with the scrim showing through (frameless, like the main card).
/// Falls back to the boxed corner card when the strip right of the card is
/// too narrow for wrapped text (e.g. a wide two-column play card).
fn position_overlay_vocab_popup(state: &AppState) {
    let win_w = state.text_view.root().map(|r| r.width()).unwrap_or(0);
    match strip_margin_start(state) {
        Some(margin) if win_w - margin >= 240 => {
            state.vocab_popup.popup.place_overlay_strip(margin);
        }
        _ => state.vocab_popup.popup.place_corner(),
    }
}

/// Hide the vocab popup. A chat-anchored popup also hands its carved slot
/// back to the panel (size_panel restores the full height once the popup is
/// hidden and `chat_inline` cleared), and the transcript repaginates for the
/// regrown height.
pub fn close_vocab_popup(state: &mut AppState) {
    state.vocab_popup.popup.hide();
    if state.vocab_popup.chat_inline {
        state.vocab_popup.chat_inline = false;
        if state.chat_layout_open {
            crate::input::actions::chat::size_panel(state);
            crate::input::actions::chat::rerender_current_view(state);
        }
    }
}

/// Render the current vocab popup entry.
pub fn show_vocab_popup(state: &mut AppState) {
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
            resize_chat_slot(state);
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
    resize_chat_slot(state);
}

/// Re-carve the chat panel's popup slot after a content change (word step,
/// view toggle, journal answer arriving) — the popup's natural height moved,
/// so the panel's raised bottom must move with it, and the transcript
/// repaginates for the new height. No-op unless the popup is chat-anchored
/// and the chat layout is open.
fn resize_chat_slot(state: &mut AppState) {
    if state.vocab_popup.chat_inline && state.chat_layout_open {
        crate::input::actions::chat::size_panel(state);
        crate::input::actions::chat::rerender_current_view(state);
    }
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
    // Reader cursor sync must NOT repaint overlay/chat-scoped popups: those show
    // corner cards keyed to overlay-block words, and a sync tick here would
    // clobber them with the reader's current-LINE words (or hide them).
    if state.input_mode != crate::app::InputMode::Reader {
        return;
    }
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
        state.vocab_popup.view = VocabView::Definition;
        state.vocab_popup.journal = None;
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
    // The cursor may have crossed the column split since the last show —
    // re-place so the float stays on the non-cursor column.
    position_vocab_popup(state);
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
