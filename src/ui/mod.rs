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
