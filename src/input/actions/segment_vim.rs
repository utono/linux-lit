use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Copy-only vim view of the cursor's segment (the `v` bind on the main card).
///
/// Opens the cursor's buffer line — the prose paragraph or verse line the
/// cursor highlight covers — in the gloss overlay's in-place vim editor,
/// seeded straight into VISUAL mode. The purpose is selecting text and copying
/// it to the system clipboard (visual `y`), NOT editing: `:w`/`:wq`/`R` are
/// refused with a toast, and nothing is ever written back to the reading
/// buffer or lit.db. `:q`/`:q!`/double-Esc close back to the reader.
pub(crate) fn open(state: &Rc<RefCell<AppState>>) {
    let opened = {
        let mut s = state.borrow_mut();
        if s.current_work.is_none() {
            return;
        }
        let text = crate::input::viewport::buffer_line_text(&s.buffer, s.current_line);
        let text = text.trim_end().to_string();
        if text.trim().is_empty() {
            false
        } else {
            let (cw, h) = crate::app::layout::overlay_card_size(&s);
            // Open the overlay chrome sized for this text, then swap the view
            // to the raw vim edit buffer — the same two-step the gloss `e`
            // editor uses, minus any stored artifact behind it.
            s.gloss_overlay.show_gloss_with_color(
                "", &text, cw, h, Some(&s.theme.root_color), &[],
            );
            let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
            s.gloss_overlay.set_edit_copy_only(true);
            s.gloss_overlay.enter_edit_buffer(&text, &fill, &fg);
            // Seed VISUAL mode: selecting is the whole point of this surface.
            let _ = s
                .gloss_overlay
                .feed_edit_key(crate::input::vim::VimKey::Char('v'));
            s.input_mode = crate::app::InputMode::SegmentVim;
            crate::logging::log(&format!(
                "SEGMENT_VIM: opened line {} ({} chars)",
                s.current_line,
                text.chars().count()
            ));
            true
        }
    };
    if !opened {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "Nothing to copy on this line", 2);
    }
}

/// Close the segment vim editor and return to the reader. Never saves — the
/// editor buffer is discarded wholesale.
pub(crate) fn close(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.gloss_overlay.exit_edit_buffer();
    s.gloss_overlay.hide();
    crate::app::return_to_reader_mode(&mut s);
    crate::logging::log("SEGMENT_VIM: closed");
}

/// Toast shown when a save/rewrite verb is used in the copy-only editor.
pub(crate) fn refuse_save(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    crate::input::navigation::show_chapter_toast_secs(&s, "Copy-only view \u{2014} nothing is saved (y copies, :q exits)", 3);
}
