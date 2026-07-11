//! Chat layout (Tab): left chat panel + right-pinned card. This task ships
//! the layout toggle only; the panel widget and conversation land in later
//! tasks of the chat-layout plan.

use crate::app::AppState;
use gtk4::prelude::WidgetExt;
use std::cell::RefCell;
use std::rc::Rc;

/// Minimum freed left space (px) required to open the chat layout.
const CHAT_MIN_PANEL_W: i32 = 500;

/// Re-apply the card margins for the current chat_layout_open value.
pub(crate) fn reapply_card_margins(s: &AppState) {
    let ww = s.window.width().max(0);
    crate::app::layout::apply_card_sizing(
        &s.content_hbox,
        ww,
        crate::app::layout::effective_column_width(s),
        s.column_count(),
        s.translations_visible,
        s.chat_layout_open,
    );
}

pub(crate) fn close_chat_layout(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat_layout_open = false;
    reapply_card_margins(s);
    s.input_mode = crate::app::InputMode::Reader;
    s.chat_panel.hide();
    crate::logging::log("CHAT: layout closed");
}

pub(crate) fn toggle_chat_layout(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.chat_layout_open {
        // Panel already open: Tab (from reader focus) cycles INTO the prompt;
        // closing is Ctrl+Tab's job (ToggleLastOverlay shadow).
        focus_prompt(&mut s);
        return;
    }
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(&s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        crate::ui::toast::show_transient(
            &s.chapter_toast,
            "No room for chat panel at this layout",
            3,
        );
        return;
    }
    s.chat_layout_open = true;
    reapply_card_margins(&s);
    size_panel(&s);
    s.chat_panel.show();
    crate::logging::log(&format!("CHAT: layout opened (free={}px)", free));
    focus_prompt(&mut s);
}

/// Chat layout: the panel's vim prompt gains input focus. Opens the ask-card
/// input (title/hint/theme colors) and sets the panel header for the current
/// cursor position.
pub(crate) fn focus_prompt(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatPrompt;
    s.chat_panel.open_input(
        "Ask about this passage",
        "Ctrl+Enter send \u{b7} Tab cycle",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    set_panel_header(s);
}

/// Chat layout: the transcript pane gains input focus (j/k move the exchange
/// cursor, s saves, Tab cycles to the reader, Ctrl+Tab closes).
pub(crate) fn focus_transcript(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatTranscript;
}

/// Chat layout: the reader pane gains input focus (full reader keys live;
/// the panel stays open and visible).
pub(crate) fn focus_reader(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::Reader;
}

/// Submit the chat prompt's current text as a new turn. Stubbed here (toast);
/// implemented in Task 6.
pub(crate) fn submit_chat_prompt(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    crate::ui::toast::show_transient(&s.chapter_toast, "Chat send lands in the next task", 2);
}

/// Move the transcript exchange cursor by `delta`. Stubbed here; implemented
/// in a later task.
pub(crate) fn transcript_cursor_move(_s: &mut AppState, _delta: i32) {}

/// Save the transcript's currently selected exchange. Stubbed here; implemented
/// in Task 7.
pub(crate) fn save_selected_exchange(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    crate::ui::toast::show_transient(&s.chapter_toast, "Save lands in a later task", 2);
}

/// Size the panel to the freed left space at the card's height.
pub(crate) fn size_panel(s: &AppState) {
    let ww = s.window.width().max(0);
    let (card_w, card_h) = crate::app::layout::main_card_rect(s);
    let end = crate::app::layout::CARD_OUTER_MARGIN;
    // left outer margin (24) + gap to the card (16)
    let w = ww - card_w - end - 24 - 16;
    s.chat_panel.size_to(w, card_h);
}

pub(crate) fn set_panel_header(s: &AppState) {
    let Some(w) = s.current_work.as_ref() else {
        return;
    };
    let (d1, d2) = s
        .work_line_for_buffer(s.current_line)
        .and_then(|wi| w.lines.get(wi))
        .map(|l| (l.div1, l.div2))
        .unwrap_or((0, 0));
    let scene = crate::app::scene_synopsis::scene_label_for(s, d1, d2);
    s.chat_panel.set_header(&w.title, &w.author, &scene);
}
