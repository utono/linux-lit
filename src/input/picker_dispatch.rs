use crate::app::{AppState, InputMode};

/// The uniform slice of picker behavior the keymap dispatches by InputMode.
/// Per-picker index math stays inside each `move_selection` impl (the variants
/// audit #6 preserved); this trait only routes to it.
pub trait Picker {
    fn move_selection(&self, delta: i32);
    fn hide(&self);
}

macro_rules! impl_picker {
    ($ty:ty) => {
        impl Picker for $ty {
            fn move_selection(&self, delta: i32) {
                <$ty>::move_selection(self, delta);
            }
            fn hide(&self) {
                <$ty>::hide(self);
            }
        }
    };
}

impl_picker!(crate::ui::bookmark_picker::BookmarkPicker);
impl_picker!(crate::ui::media_picker::MediaPicker);
impl_picker!(crate::ui::concordance_picker::ConcordancePicker);
impl_picker!(crate::ui::concordance_word_picker::ConcordanceWordPicker);
impl_picker!(crate::ui::concordance_list_picker::ConcordanceListPicker);
impl_picker!(crate::ui::concordance_works_picker::ConcordanceWorksPicker);
impl_picker!(crate::ui::gloss_picker::GlossPicker);
impl_picker!(crate::ui::authorship_picker::AuthorshipPicker);
impl_picker!(crate::ui::journal_picker::JournalQaPicker);
impl_picker!(crate::ui::journal_move_picker::JournalMovePicker);
impl_picker!(crate::ui::journal_term_input::JournalTermInput);
impl_picker!(crate::ui::echo_line_picker::EchoLinePicker);

/// The single source of truth for "which picker is active in this mode".
/// Returns None for non-picker modes (caller no-ops, matching the old `_ => {}`).
pub(crate) fn picker_for_mode(s: &AppState, mode: InputMode) -> Option<&dyn Picker> {
    match mode {
        InputMode::BookmarkPicker => Some(&s.bookmark_picker),
        InputMode::MediaPicker => Some(&s.media_picker),
        InputMode::ConcordancePicker => Some(&s.concordance_picker),
        InputMode::ConcordanceWordPicker => Some(&s.concordance_word_picker),
        InputMode::ConcordanceListPicker => Some(&s.concordance_list_picker),
        InputMode::ConcordanceWorksPicker => Some(&s.concordance_works_picker),
        InputMode::GlossPicker => Some(&s.gloss_picker),
        InputMode::AuthorshipPicker => Some(&s.authorship_picker),
        InputMode::JournalPicker => Some(&s.journal_picker),
        InputMode::JournalMovePicker => Some(&s.journal_move_picker),
        InputMode::JournalTermInput => Some(&s.journal_term_input),
        InputMode::EchoLinePicker => Some(&s.echo_line_picker),
        _ => None,
    }
}
