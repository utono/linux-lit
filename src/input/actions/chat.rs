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
    crate::logging::log("CHAT: layout closed");
}

pub(crate) fn toggle_chat_layout(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.chat_layout_open {
        // Panel already open: Tab (from reader focus) cycles INTO the panel;
        // full cycle behavior lands with the focus task. For now close.
        close_chat_layout(&mut s);
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
    crate::logging::log(&format!("CHAT: layout opened (free={}px)", free));
}
