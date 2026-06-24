pub mod authorship_picker;
pub mod action_popup;
pub mod ask_card;
pub mod footer;
pub mod concordance_bar;
pub mod concordance_list_picker;
pub mod concordance_works_picker;
pub mod concordance_picker;
pub mod concordance_word_picker;
pub mod gloss_block;
pub mod gloss_ipa;
pub mod gloss_overlay;
pub(crate) mod gloss_render;
pub mod gloss_util;
pub mod journal_overlay;
pub mod journal_picker;
pub mod gloss_picker;
pub mod echo_picker;
pub mod echo_line_picker;
pub mod echo_turns_picker;
pub mod echo_keybinds_overlay;
pub mod vocab_popup;
pub mod gamepad_overlay;
pub mod keybinds_overlay;
pub mod library_picker;
pub mod bookmark_picker;
pub mod media_picker;
pub mod page_image_overlay;
pub mod picker_attach;
pub mod picker_filter;
pub mod picker_nav;
pub mod search_bar;
pub mod settings_overlay;
pub mod toast;
pub mod translation_overlay;
pub mod voice_picker;

/// The side margin (left and right) for the full-screen gloss / synopsis / ask
/// cards: a quarter of the *live* card width, which keeps the prose near the
/// ~65-char readability optimum on a wide (~1660px) card.
///
/// CRITICAL: this is anchored to the on-screen `card_width`, NOT the fixed
/// `column_width`. The echo view deliberately uses `column_width / 8` instead
/// (a different value and concept); do NOT route those sites through here —
/// conflating the two reintroduces the "tiny margin / edge-to-edge text on a
/// wide card" bug. See `gloss_overlay::show_gloss_with_color` and audit #27.
pub(crate) fn card_side_margin(card_width: i32) -> i32 {
    card_width / 4
}

/// Re-assert the italic verse tags (`gloss-stage`, `gloss-bracket`) to the top
/// of `table`'s priority order. An overlay's buffer-wide font tag is built with
/// `.font("Family Size")`, whose Pango description carries a regular (upright)
/// STYLE attribute; added last, it would override the italic tags by add-order
/// priority and flatten stage/bracket directions to upright. Call this AFTER
/// applying the font tag in each overlay's `apply_font`. The gloss and journal
/// overlays both render verse via `gloss_render::populate_verse_buffer`, so both
/// own these tag names and must stay in sync — hence one shared helper.
pub(crate) fn reassert_italic_tags(table: &gtk4::TextTagTable) {
    use gtk4::prelude::*;
    let top = table.size();
    for italic in ["gloss-stage", "gloss-bracket"] {
        if let Some(t) = table.lookup(italic) {
            if top > 0 {
                t.set_priority(top - 1);
            }
        }
    }
}
