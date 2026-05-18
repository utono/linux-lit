//! Verbs invoked from keymap.rs match arms. Each submodule groups verbs by
//! feature area. Phase 2 (F2) adds the Action enum here and re-exports the
//! verbs as the dispatch target.

pub mod authorship;
pub mod bookmarks;
pub mod concordance;
pub mod escape;
pub mod gloss;
pub mod pickers;
pub mod settings;
pub mod word_copy;

// Action enum identifying every reader-mode behavior. F2 maps KeyCombo →
// Action via Keymap; dispatch_action in keymap.rs translates Action into
// the corresponding verb call.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Category {
    Navigation,
    Media,
    Vocab,
    Display,
    Selection,
    Timestamps,
    App,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Action {
    // Page navigation
    PageForward,
    PageBackward,
    PageBackwardBottom,
    JumpToStart,
    JumpToEnd,

    // Cursor / dialogue navigation
    CursorNextDialogue,
    CursorPrevLine,
    CursorToPageBottom,
    JumpToNextDialogue,
    JumpToPrevDialogue,
    JumpToNextChapter,
    JumpToPrevChapter,
    JumpToNextScene,
    JumpToPrevScene,

    // Bookmarks
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    JumpToRecentBookmark,
    OpenBookmarkPicker,

    // Pickers / overlays
    OpenLibraryPicker,
    OpenRecentPicker,
    OpenMediaPicker,
    OpenConcordancePicker,
    OpenConcordanceWordPicker,
    OpenConcordanceListPicker,
    OpenConcordanceWorksPicker,
    OpenSettingsOverlay,
    OpenKeybindsOverlay,
    OpenSearch,

    // MPV / media
    TogglePlaybackSync,
    TogglePlayback,
    SeekShortBackward,
    SeekShortForward,
    SeekLongBackward,
    SeekLongForward,
    SeekBackward30,
    VolumeUp,
    VolumeDown,
    TogglePlaybackSpeed,

    // Vocab / glossing
    ToggleVocabPopup,
    VocabPopupNext,
    VocabPopupPrev,
    JumpToNextVocab,
    JumpToPrevVocab,
    ConcordanceNext,
    ConcordancePrev,
    ToggleVocabHighlight,
    ToggleGlossOverlay,
    OpenGlossPicker,

    // Visual / selection
    EnterVisualMode,
    WordCycleCopy,
    WordCollectCopy,

    // Translations
    ToggleTranslations,

    // Synopsis
    ToggleSynopsis,

    // Settings (in reader)
    AdjustFontSizeUp,
    AdjustFontSizeDown,
    ResetFontSize,
    CycleFontForward,
    CycleFontBackward,
    ToggleSignColumn,
    TogglePreviousWork,
    ToggleDim,
    ToggleTitleBar,
    ToggleAuthorship,
    PickAttributionSet,
    ShowFontInfo,
    ShowCurrentChapter,

    // Timestamps
    SetStartTime,
    SetEndTime,
    SetChapter,
    DeleteTimestamp,
    NudgeStartBackward,
    NudgeStartForward,
    UndoTimestamp,
    PlayCurrentLine,

    // App
    SaveAndQuit,
    ToggleDebugLogging,
    CopyLineMappingId,
    EscapeReaderMode,

    // Multi-key chords (entry — completion handled by KeyState)
    PendingG,
    PendingZ,

    // Search (in reader, when matches present)
    SearchNextMatch,
    SearchPrevMatch,
}

impl Action {
    pub fn category(&self) -> Category {
        match self {
            // Navigation
            Action::PageForward
            | Action::PageBackward
            | Action::PageBackwardBottom
            | Action::JumpToStart
            | Action::JumpToEnd
            | Action::CursorNextDialogue
            | Action::CursorPrevLine
            | Action::CursorToPageBottom
            | Action::JumpToNextDialogue
            | Action::JumpToPrevDialogue
            | Action::JumpToNextChapter
            | Action::JumpToPrevChapter
            | Action::JumpToNextScene
            | Action::JumpToPrevScene
            | Action::ToggleBookmark
            | Action::NextBookmark
            | Action::PrevBookmark
            | Action::JumpToRecentBookmark
            | Action::OpenBookmarkPicker => Category::Navigation,

            // Media
            Action::TogglePlaybackSync
            | Action::TogglePlayback
            | Action::SeekShortBackward
            | Action::SeekShortForward
            | Action::SeekLongBackward
            | Action::SeekLongForward
            | Action::SeekBackward30
            | Action::VolumeUp
            | Action::VolumeDown
            | Action::TogglePlaybackSpeed => Category::Media,

            // Vocab
            Action::ToggleVocabPopup
            | Action::VocabPopupNext
            | Action::VocabPopupPrev
            | Action::JumpToNextVocab
            | Action::JumpToPrevVocab
            | Action::ToggleVocabHighlight
            | Action::ToggleGlossOverlay
            | Action::OpenGlossPicker
            | Action::OpenConcordancePicker
            | Action::OpenConcordanceWordPicker
            | Action::OpenConcordanceListPicker
            | Action::OpenConcordanceWorksPicker
            | Action::ConcordanceNext
            | Action::ConcordancePrev => Category::Vocab,

            // Display
            Action::AdjustFontSizeUp
            | Action::AdjustFontSizeDown
            | Action::ResetFontSize
            | Action::CycleFontForward
            | Action::CycleFontBackward
            | Action::ToggleSignColumn
            | Action::ToggleDim
            | Action::ToggleTitleBar
            | Action::ToggleAuthorship
            | Action::PickAttributionSet
            | Action::ShowFontInfo
            | Action::ShowCurrentChapter
            | Action::ToggleTranslations
            | Action::ToggleSynopsis
            | Action::OpenSettingsOverlay => Category::Display,

            // Selection
            Action::EnterVisualMode
            | Action::WordCycleCopy
            | Action::WordCollectCopy => Category::Selection,

            // Timestamps
            Action::SetStartTime
            | Action::SetEndTime
            | Action::SetChapter
            | Action::DeleteTimestamp
            | Action::NudgeStartBackward
            | Action::NudgeStartForward
            | Action::UndoTimestamp
            | Action::PlayCurrentLine => Category::Timestamps,

            // App
            Action::SaveAndQuit
            | Action::ToggleDebugLogging
            | Action::CopyLineMappingId
            | Action::EscapeReaderMode
            | Action::PendingG
            | Action::PendingZ
            | Action::SearchNextMatch
            | Action::SearchPrevMatch
            | Action::OpenLibraryPicker
            | Action::OpenRecentPicker
            | Action::OpenMediaPicker
            | Action::OpenKeybindsOverlay
            | Action::OpenSearch
            | Action::TogglePreviousWork => Category::App,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Action::PageForward => "PageForward",
            Action::PageBackward => "PageBackward",
            Action::PageBackwardBottom => "PageBackwardBottom",
            Action::JumpToStart => "JumpToStart",
            Action::JumpToEnd => "JumpToEnd",
            Action::CursorNextDialogue => "CursorNextDialogue",
            Action::CursorPrevLine => "CursorPrevLine",
            Action::CursorToPageBottom => "CursorToPageBottom",
            Action::JumpToNextDialogue => "JumpToNextDialogue",
            Action::JumpToPrevDialogue => "JumpToPrevDialogue",
            Action::JumpToNextChapter => "JumpToNextChapter",
            Action::JumpToPrevChapter => "JumpToPrevChapter",
            Action::JumpToNextScene => "JumpToNextScene",
            Action::JumpToPrevScene => "JumpToPrevScene",
            Action::ToggleBookmark => "ToggleBookmark",
            Action::NextBookmark => "NextBookmark",
            Action::PrevBookmark => "PrevBookmark",
            Action::JumpToRecentBookmark => "JumpToRecentBookmark",
            Action::OpenBookmarkPicker => "OpenBookmarkPicker",
            Action::OpenLibraryPicker => "OpenLibraryPicker",
            Action::OpenRecentPicker => "OpenRecentPicker",
            Action::OpenMediaPicker => "OpenMediaPicker",
            Action::OpenConcordancePicker => "OpenConcordancePicker",
            Action::OpenConcordanceWordPicker => "OpenConcordanceWordPicker",
            Action::OpenConcordanceListPicker => "OpenConcordanceListPicker",
            Action::OpenConcordanceWorksPicker => "OpenConcordanceWorksPicker",
            Action::OpenSettingsOverlay => "OpenSettingsOverlay",
            Action::OpenKeybindsOverlay => "OpenKeybindsOverlay",
            Action::OpenSearch => "OpenSearch",
            Action::TogglePlaybackSync => "TogglePlaybackSync",
            Action::TogglePlayback => "TogglePlayback",
            Action::SeekShortBackward => "SeekShortBackward",
            Action::SeekShortForward => "SeekShortForward",
            Action::SeekLongBackward => "SeekLongBackward",
            Action::SeekLongForward => "SeekLongForward",
            Action::SeekBackward30 => "SeekBackward30",
            Action::VolumeUp => "VolumeUp",
            Action::VolumeDown => "VolumeDown",
            Action::TogglePlaybackSpeed => "TogglePlaybackSpeed",
            Action::ToggleVocabPopup => "ToggleVocabPopup",
            Action::VocabPopupNext => "VocabPopupNext",
            Action::VocabPopupPrev => "VocabPopupPrev",
            Action::JumpToNextVocab => "JumpToNextVocab",
            Action::JumpToPrevVocab => "JumpToPrevVocab",
            Action::ToggleVocabHighlight => "ToggleVocabHighlight",
            Action::ToggleGlossOverlay => "ToggleGlossOverlay",
            Action::OpenGlossPicker => "OpenGlossPicker",
            Action::ConcordanceNext => "ConcordanceNext",
            Action::ConcordancePrev => "ConcordancePrev",
            Action::EnterVisualMode => "EnterVisualMode",
            Action::WordCycleCopy => "WordCycleCopy",
            Action::WordCollectCopy => "WordCollectCopy",
            Action::ToggleTranslations => "ToggleTranslations",
            Action::ToggleSynopsis => "ToggleSynopsis",
            Action::AdjustFontSizeUp => "AdjustFontSizeUp",
            Action::AdjustFontSizeDown => "AdjustFontSizeDown",
            Action::ResetFontSize => "ResetFontSize",
            Action::CycleFontForward => "CycleFontForward",
            Action::CycleFontBackward => "CycleFontBackward",
            Action::ToggleSignColumn => "ToggleSignColumn",
            Action::TogglePreviousWork => "TogglePreviousWork",
            Action::ToggleDim => "ToggleDim",
            Action::ToggleTitleBar => "ToggleTitleBar",
            Action::ToggleAuthorship => "ToggleAuthorship",
            Action::PickAttributionSet => "PickAttributionSet",
            Action::ShowFontInfo => "ShowFontInfo",
            Action::ShowCurrentChapter => "ShowCurrentChapter",
            Action::SetStartTime => "SetStartTime",
            Action::SetEndTime => "SetEndTime",
            Action::SetChapter => "SetChapter",
            Action::DeleteTimestamp => "DeleteTimestamp",
            Action::NudgeStartBackward => "NudgeStartBackward",
            Action::NudgeStartForward => "NudgeStartForward",
            Action::UndoTimestamp => "UndoTimestamp",
            Action::PlayCurrentLine => "PlayCurrentLine",
            Action::SaveAndQuit => "SaveAndQuit",
            Action::ToggleDebugLogging => "ToggleDebugLogging",
            Action::CopyLineMappingId => "CopyLineMappingId",
            Action::EscapeReaderMode => "EscapeReaderMode",
            Action::PendingG => "PendingG",
            Action::PendingZ => "PendingZ",
            Action::SearchNextMatch => "SearchNextMatch",
            Action::SearchPrevMatch => "SearchPrevMatch",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_category() {
        let actions = [
            Action::PageForward, Action::CursorNextDialogue, Action::ToggleBookmark,
            Action::OpenLibraryPicker, Action::TogglePlayback, Action::ToggleVocabPopup,
            Action::EnterVisualMode, Action::ToggleTranslations, Action::AdjustFontSizeUp,
            Action::SetStartTime, Action::SaveAndQuit, Action::SearchNextMatch,
        ];
        for a in &actions {
            let _ = a.category();
        }
    }

    #[test]
    fn category_assignments_are_correct() {
        assert_eq!(Action::PageForward.category(), Category::Navigation);
        assert_eq!(Action::JumpToNextChapter.category(), Category::Navigation);
        assert_eq!(Action::NextBookmark.category(), Category::Navigation);
        assert_eq!(Action::TogglePlayback.category(), Category::Media);
        assert_eq!(Action::SeekShortForward.category(), Category::Media);
        assert_eq!(Action::ToggleVocabPopup.category(), Category::Vocab);
        assert_eq!(Action::OpenConcordancePicker.category(), Category::Vocab);
        assert_eq!(Action::AdjustFontSizeUp.category(), Category::Display);
        assert_eq!(Action::ToggleTranslations.category(), Category::Display);
        assert_eq!(Action::OpenSettingsOverlay.category(), Category::Display);
        assert_eq!(Action::EnterVisualMode.category(), Category::Selection);
        assert_eq!(Action::SetStartTime.category(), Category::Timestamps);
        assert_eq!(Action::SaveAndQuit.category(), Category::App);
        assert_eq!(Action::PendingG.category(), Category::App);
        assert_eq!(Action::PendingZ.category(), Category::App);
        assert_eq!(Action::OpenSearch.category(), Category::App);
    }

    #[test]
    fn action_name_returns_variant_string() {
        assert_eq!(Action::PageForward.name(), "PageForward");
        assert_eq!(Action::JumpToNextChapter.name(), "JumpToNextChapter");
        assert_eq!(Action::TogglePlayback.name(), "TogglePlayback");
        assert_eq!(Action::SaveAndQuit.name(), "SaveAndQuit");
    }
}
