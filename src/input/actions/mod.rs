//! Verbs invoked from keymap.rs match arms. Each submodule groups verbs by
//! feature area. Phase 2 (F2) adds the Action enum here and re-exports the
//! verbs as the dispatch target.

pub mod authorship;
pub mod bookmarks;
pub mod chapters;
pub mod chat;
pub(crate) mod claude_bridge;
pub mod concordance;
pub mod corpus_search;
pub mod echoes;
pub mod escape;
pub mod gloss;
pub mod journal;
pub mod overlay_cycle;
pub mod pickers;
pub mod rewrite_history;
pub mod segment_vim;
pub mod settings;
pub mod synopsis;
pub(crate) mod vocab_journal;
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
    /// Cursor-only: next dialogue line, NO media seek (h key).
    CursorNextDialogueNoSeek,
    /// Cursor-only: prev dialogue line, NO media seek (t key).
    CursorPrevDialogueNoSeek,
    JumpToNextSpeaker,
    JumpToPrevSpeaker,
    JumpToNextChapter,
    JumpToPrevChapter,
    JumpToNextScene,
    JumpToPrevScene,

    // Bookmarks
    ToggleBookmark,
    /// Overloaded `.`: single tap toggles the bookmark; a second quick tap
    /// reverts the toggle and opens the picker (ChordState::PendingPeriod).
    BookmarkTap,
    /// Toggle whether the cursor's paragraph begins a chapter (prose only).
    ToggleChapterStart,
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
    /// Like OpenSearch but seeks the PREVIOUS occurrence relative to the cursor
    /// (vim `?` vs `/`). Bound to Shift+slash (`question`).
    OpenSearchBackward,

    // MPV / media
    TogglePlaybackSync,
    /// Seek to the cursor line's start timestamp, THEN toggle pause. Used by the
    /// gamepad button and the translation overlay; the reader card uses
    /// `PlayCurrentLine` (always-resume) and `TogglePause` (no-seek toggle).
    TogglePlaybackFromTimestamp,
    /// Pure play/pause toggle — does NOT seek to the cursor line's start time on
    /// resume (unlike `TogglePlaybackFromTimestamp`, which seeks). Bound to `a`
    /// in the reader.
    TogglePause,
    SeekShortBackward,
    SeekShortForward,
    SeekLongBackward,
    SeekLongForward,
    SeekBackward30,
    VolumeUp,
    VolumeDown,
    TogglePlaybackSpeed,
    /// Cycle the karaoke narration highlight for the current work's class
    /// (prose vs plays/poetry): Off -> Phrase -> Line; persisted per class in
    /// config. Alt+p.
    TogglePhraseHighlight,

    // Vocab / glossing
    ToggleVocabPopup,
    /// Overloaded `r`: while the popup is visible a single tap cycles the
    /// segment's words; a second tap within the chord window toggles
    /// visibility (rr — see ChordState::PendingR in keymap.rs).
    VocabPopupTap,
    VocabPopupNext,
    VocabPopupPrev,
    HideVocabPopup,
    /// Ask Claude about the vocab popup's current word in the cursor
    /// segment and across the author's corpus; stores a kind='vocab'
    /// journal Q&A and renders it in the popup (R — gated on popup visible
    /// + a vocab word on the cursor line; silent no-op otherwise).
    VocabJournalAsk,
    /// Page the popup's Journal answer forward / backward (Ctrl+n /
    /// Ctrl+p; no-op outside the Journal view or when it fits one page).
    VocabJournalPageNext,
    VocabJournalPagePrev,
    JumpToNextVocab,
    JumpToPrevVocab,
    ConcordanceNext,
    ConcordancePrev,
    ToggleVocabHighlight,
    ToggleGlossOverlay,
    ToggleJournalOverlay,
    /// Reader-only: reopen whichever of the gloss/journal overlays was last
    /// open (fresh from the cursor). No-op with a toast if none remembered.
    /// Overlays themselves close via Escape only.
    ToggleLastOverlay,
    /// Plain `\`: cycle the per-segment overlays — journal Q&A → gloss →
    /// synopsis → journal (wraps). From the reader it opens the journal stop;
    /// inside each overlay `\` advances (handled in the overlay modal
    /// handlers, not this reader action). See input/actions/overlay_cycle.rs.
    CycleSegmentOverlays,
    /// Open the journal Q&A picker directly from the reading card (Alt+j),
    /// without first opening the journal overlay. Confirming a pick reveals the
    /// overlay on that Q&A; Escape returns to the reader.
    OpenJournalPicker,
    OpenGlossPicker,
    OpenLastGloss,
    /// Ctrl+f: open the cross-corpus journal/gloss regex search popup.
    OpenCorpusSearch,
    ShowEchoesBcp,
    ReopenEchoesBcp,
    ShowEchoTurnsBcp,
    ShowEchoesShx,
    ReopenEchoesShx,
    ShowEchoTurnsShx,

    // Visual / selection
    EnterVisualMode,
    /// Ctrl+a: auto-select the paragraph/speech around the cursor and enter
    /// visual mode pending a Journal Q&A ask; a second Ctrl+a (or Return)
    /// opens the "Ask a question about this passage" card directly.
    AskPassage,
    WordCycleCopy,
    WordCollectCopy,
    /// Open the cursor's segment (prose paragraph / verse line) in a read-only
    /// vim editor seeded in visual mode, for selecting text and copying it to
    /// the system clipboard. Nothing is ever saved back.
    OpenSegmentVim,

    // Translations
    ToggleTranslations,

    // Synopsis
    ToggleSynopsis,
    ShowSynopsisOverlay,
    ShowTranslationOverlay,

    // Page-scan image view (toggle the card to the leaf PNG; BCP1549 etc.)
    ToggleImageView,
    EnterPageCalibration,

    // Settings (in reader)
    AdjustFontSizeUp,
    AdjustFontSizeDown,
    ResetFontSize,
    CycleFontForward,
    CycleFontBackward,
    ToggleSignColumn,
    ToggleColumnLayout,
    TogglePreviousWork,
    ToggleDim,
    /// `a`: open the chat panel layout (card pins right); with the panel
    /// already open it cycles focus INTO the panel rather than closing —
    /// closing is `Ctrl+a` (CloseChatLayout).
    ToggleChatLayout,
    /// `Ctrl+a`: close the chat panel layout — the hide half of `a`'s show.
    /// Works from the reader AND from inside the panel (prompt/transcript),
    /// so one chord always dismisses it. No-op when the panel isn't open.
    CloseChatLayout,
    /// Reader-mode `-`: open the chat panel pinned to the reader-gloss covering
    /// the cursor line (no `V` needed). No-op toast if the line isn't glossed.
    ReaderGlossChatAtCursor,
    /// Ctrl+l: flip a floating chat panel to the other reading column.
    ChatPanelFlipSide,
    ThemeNext,
    ThemePrev,
    RootVariantNext,
    RootVariantPrev,
    CycleScansion,
    ToggleTitleBar,
    ToggleAuthorship,
    PickAttributionSet,
    ShowFontInfo,
    /// Ctrl+Alt+t: desktop notification of the current theme name and root
    /// color (read-only; does not cycle). Sibling of the theme/root cycle binds.
    ShowThemeInfo,
    ShowCurrentChapter,

    // Timestamps
    SetStartTime,
    SetEndTime,
    SetChapter,
    DeleteTimestamp,
    /// Overloaded BackSpace: single tap only toasts the line's timestamp; a
    /// second quick tap deletes it (ChordState::PendingBackspace).
    DeleteTimestampTap,
    NudgeStartBackward,
    NudgeStartForward,
    UndoTimestamp,
    PlayCurrentLine,

    // App
    SaveAndQuit,
    ToggleDebugLogging,
    ToggleNavTest,
    CopyLineMappingId,
    CopyWorkInfo,
    EscapeReaderMode,

    // Multi-key chords (entry — completion handled by KeyState)
    PendingG,

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
            | Action::CursorNextDialogueNoSeek
            | Action::CursorPrevDialogueNoSeek
            | Action::JumpToNextSpeaker
            | Action::JumpToPrevSpeaker
            | Action::JumpToNextChapter
            | Action::JumpToPrevChapter
            | Action::JumpToNextScene
            | Action::JumpToPrevScene
            | Action::ToggleBookmark
            | Action::BookmarkTap
            | Action::ToggleChapterStart
            | Action::NextBookmark
            | Action::PrevBookmark
            | Action::JumpToRecentBookmark
            | Action::OpenBookmarkPicker => Category::Navigation,

            // Media
            Action::TogglePlaybackSync
            | Action::TogglePlaybackFromTimestamp
            | Action::TogglePause
            | Action::SeekShortBackward
            | Action::SeekShortForward
            | Action::SeekLongBackward
            | Action::SeekLongForward
            | Action::SeekBackward30
            | Action::VolumeUp
            | Action::VolumeDown
            | Action::TogglePlaybackSpeed
            | Action::TogglePhraseHighlight => Category::Media,

            // Vocab
            Action::ToggleVocabPopup
            | Action::VocabPopupTap
            | Action::VocabPopupNext
            | Action::VocabPopupPrev
            | Action::HideVocabPopup
            | Action::VocabJournalAsk
            | Action::VocabJournalPageNext
            | Action::VocabJournalPagePrev
            | Action::JumpToNextVocab
            | Action::JumpToPrevVocab
            | Action::ToggleVocabHighlight
            | Action::ToggleGlossOverlay
            | Action::ToggleJournalOverlay
            | Action::ToggleLastOverlay
            | Action::CycleSegmentOverlays
            | Action::OpenJournalPicker
            | Action::OpenGlossPicker
            | Action::OpenLastGloss
            | Action::OpenCorpusSearch
            | Action::ShowEchoesBcp
            | Action::ReopenEchoesBcp
            | Action::ShowEchoTurnsBcp
            | Action::ShowEchoesShx
            | Action::ReopenEchoesShx
            | Action::ShowEchoTurnsShx
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
            | Action::ToggleColumnLayout
            | Action::ToggleDim
            | Action::ToggleChatLayout
            | Action::CloseChatLayout
            | Action::ReaderGlossChatAtCursor
            | Action::ChatPanelFlipSide
            | Action::ThemeNext
            | Action::ThemePrev
            | Action::RootVariantNext
            | Action::RootVariantPrev
            | Action::CycleScansion
            | Action::ToggleTitleBar
            | Action::ToggleAuthorship
            | Action::PickAttributionSet
            | Action::ShowFontInfo
            | Action::ShowThemeInfo
            | Action::ShowCurrentChapter
            | Action::ToggleTranslations
            | Action::ToggleSynopsis
            | Action::ShowSynopsisOverlay
            | Action::ShowTranslationOverlay
            | Action::ToggleImageView
            | Action::EnterPageCalibration
            | Action::OpenSettingsOverlay => Category::Display,

            // Selection
            Action::EnterVisualMode
            | Action::AskPassage
            | Action::WordCycleCopy
            | Action::WordCollectCopy
            | Action::OpenSegmentVim => Category::Selection,

            // Timestamps
            Action::SetStartTime
            | Action::SetEndTime
            | Action::SetChapter
            | Action::DeleteTimestamp
            | Action::DeleteTimestampTap
            | Action::NudgeStartBackward
            | Action::NudgeStartForward
            | Action::UndoTimestamp
            | Action::PlayCurrentLine => Category::Timestamps,

            // App
            Action::SaveAndQuit
            | Action::ToggleDebugLogging
            | Action::ToggleNavTest
            | Action::CopyLineMappingId
            | Action::CopyWorkInfo
            | Action::EscapeReaderMode
            | Action::PendingG
            | Action::SearchNextMatch
            | Action::SearchPrevMatch
            | Action::OpenLibraryPicker
            | Action::OpenRecentPicker
            | Action::OpenMediaPicker
            | Action::OpenKeybindsOverlay
            | Action::OpenSearch
            | Action::OpenSearchBackward
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
            Action::CursorNextDialogueNoSeek => "CursorNextDialogueNoSeek",
            Action::CursorPrevDialogueNoSeek => "CursorPrevDialogueNoSeek",
            Action::JumpToNextSpeaker => "JumpToNextSpeaker",
            Action::JumpToPrevSpeaker => "JumpToPrevSpeaker",
            Action::JumpToNextChapter => "JumpToNextChapter",
            Action::JumpToPrevChapter => "JumpToPrevChapter",
            Action::JumpToNextScene => "JumpToNextScene",
            Action::JumpToPrevScene => "JumpToPrevScene",
            Action::ToggleBookmark => "ToggleBookmark",
            Action::BookmarkTap => "BookmarkTap",
            Action::ToggleChapterStart => "ToggleChapterStart",
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
            Action::OpenSearchBackward => "OpenSearchBackward",
            Action::TogglePlaybackSync => "TogglePlaybackSync",
            Action::TogglePlaybackFromTimestamp => "TogglePlaybackFromTimestamp",
            Action::TogglePause => "TogglePause",
            Action::SeekShortBackward => "SeekShortBackward",
            Action::SeekShortForward => "SeekShortForward",
            Action::SeekLongBackward => "SeekLongBackward",
            Action::SeekLongForward => "SeekLongForward",
            Action::SeekBackward30 => "SeekBackward30",
            Action::VolumeUp => "VolumeUp",
            Action::VolumeDown => "VolumeDown",
            Action::TogglePlaybackSpeed => "TogglePlaybackSpeed",
            Action::TogglePhraseHighlight => "TogglePhraseHighlight",
            Action::ToggleVocabPopup => "ToggleVocabPopup",
            Action::VocabPopupTap => "VocabPopupTap",
            Action::VocabPopupNext => "VocabPopupNext",
            Action::VocabPopupPrev => "VocabPopupPrev",
            Action::HideVocabPopup => "HideVocabPopup",
            Action::VocabJournalAsk => "VocabJournalAsk",
            Action::VocabJournalPageNext => "VocabJournalPageNext",
            Action::VocabJournalPagePrev => "VocabJournalPagePrev",
            Action::JumpToNextVocab => "JumpToNextVocab",
            Action::JumpToPrevVocab => "JumpToPrevVocab",
            Action::ToggleVocabHighlight => "ToggleVocabHighlight",
            Action::ToggleGlossOverlay => "ToggleGlossOverlay",
            Action::ToggleJournalOverlay => "ToggleJournalOverlay",
            Action::ToggleLastOverlay => "ToggleLastOverlay",
            Action::CycleSegmentOverlays => "CycleSegmentOverlays",
            Action::OpenJournalPicker => "OpenJournalPicker",
            Action::OpenGlossPicker => "OpenGlossPicker",
            Action::OpenLastGloss => "OpenLastGloss",
            Action::OpenCorpusSearch => "OpenCorpusSearch",
            Action::ShowEchoesBcp => "ShowEchoesBcp",
            Action::ReopenEchoesBcp => "ReopenEchoesBcp",
            Action::ShowEchoTurnsBcp => "ShowEchoTurnsBcp",
            Action::ShowEchoesShx => "ShowEchoesShx",
            Action::ReopenEchoesShx => "ReopenEchoesShx",
            Action::ShowEchoTurnsShx => "ShowEchoTurnsShx",
            Action::ConcordanceNext => "ConcordanceNext",
            Action::ConcordancePrev => "ConcordancePrev",
            Action::EnterVisualMode => "EnterVisualMode",
            Action::AskPassage => "AskPassage",
            Action::WordCycleCopy => "WordCycleCopy",
            Action::WordCollectCopy => "WordCollectCopy",
            Action::OpenSegmentVim => "OpenSegmentVim",
            Action::ToggleTranslations => "ToggleTranslations",
            Action::ToggleSynopsis => "ToggleSynopsis",
            Action::ShowSynopsisOverlay => "ShowSynopsisOverlay",
            Action::ShowTranslationOverlay => "ShowTranslationOverlay",
            Action::ToggleImageView => "ToggleImageView",
            Action::EnterPageCalibration => "EnterPageCalibration",
            Action::AdjustFontSizeUp => "AdjustFontSizeUp",
            Action::AdjustFontSizeDown => "AdjustFontSizeDown",
            Action::ResetFontSize => "ResetFontSize",
            Action::CycleFontForward => "CycleFontForward",
            Action::CycleFontBackward => "CycleFontBackward",
            Action::ToggleSignColumn => "ToggleSignColumn",
            Action::ToggleColumnLayout => "ToggleColumnLayout",
            Action::TogglePreviousWork => "TogglePreviousWork",
            Action::ToggleDim => "ToggleDim",
            Action::ToggleChatLayout => "ToggleChatLayout",
            Action::CloseChatLayout => "CloseChatLayout",
            Action::ReaderGlossChatAtCursor => "ReaderGlossChatAtCursor",
            Action::ChatPanelFlipSide => "ChatPanelFlipSide",
            Action::ThemeNext => "ThemeNext",
            Action::ThemePrev => "ThemePrev",
            Action::RootVariantNext => "RootVariantNext",
            Action::RootVariantPrev => "RootVariantPrev",
            Action::CycleScansion => "CycleScansion",
            Action::ToggleTitleBar => "ToggleTitleBar",
            Action::ToggleAuthorship => "ToggleAuthorship",
            Action::PickAttributionSet => "PickAttributionSet",
            Action::ShowFontInfo => "ShowFontInfo",
            Action::ShowThemeInfo => "ShowThemeInfo",
            Action::ShowCurrentChapter => "ShowCurrentChapter",
            Action::SetStartTime => "SetStartTime",
            Action::SetEndTime => "SetEndTime",
            Action::SetChapter => "SetChapter",
            Action::DeleteTimestamp => "DeleteTimestamp",
            Action::DeleteTimestampTap => "DeleteTimestampTap",
            Action::NudgeStartBackward => "NudgeStartBackward",
            Action::NudgeStartForward => "NudgeStartForward",
            Action::UndoTimestamp => "UndoTimestamp",
            Action::PlayCurrentLine => "PlayCurrentLine",
            Action::SaveAndQuit => "SaveAndQuit",
            Action::ToggleDebugLogging => "ToggleDebugLogging",
            Action::ToggleNavTest => "ToggleNavTest",
            Action::CopyLineMappingId => "CopyLineMappingId",
            Action::CopyWorkInfo => "CopyWorkInfo",
            Action::EscapeReaderMode => "EscapeReaderMode",
            Action::PendingG => "PendingG",
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
            Action::OpenLibraryPicker, Action::TogglePlaybackFromTimestamp, Action::ToggleVocabPopup,
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
        assert_eq!(Action::TogglePlaybackFromTimestamp.category(), Category::Media);
        assert_eq!(Action::TogglePause.category(), Category::Media);
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
        assert_eq!(Action::OpenSearch.category(), Category::App);
    }

    #[test]
    fn speaker_turn_actions_are_navigation() {
        assert_eq!(Action::JumpToNextSpeaker.category(), Category::Navigation);
        assert_eq!(Action::JumpToPrevSpeaker.category(), Category::Navigation);
        assert_eq!(Action::JumpToNextSpeaker.name(), "JumpToNextSpeaker");
        assert_eq!(Action::JumpToPrevSpeaker.name(), "JumpToPrevSpeaker");
        let a: Action = serde_json::from_str("\"JumpToNextSpeaker\"").expect("parse");
        assert_eq!(a, Action::JumpToNextSpeaker);
    }

    #[test]
    fn action_name_returns_variant_string() {
        assert_eq!(Action::PageForward.name(), "PageForward");
        assert_eq!(Action::JumpToNextChapter.name(), "JumpToNextChapter");
        assert_eq!(Action::TogglePlaybackFromTimestamp.name(), "TogglePlaybackFromTimestamp");
        assert_eq!(Action::TogglePause.name(), "TogglePause");
        assert_eq!(Action::SaveAndQuit.name(), "SaveAndQuit");
    }

    #[test]
    fn toggle_column_layout_serde_roundtrip() {
        let json = "\"ToggleColumnLayout\"";
        let action: Action = serde_json::from_str(json).expect("parse");
        assert_eq!(action, Action::ToggleColumnLayout);
        assert_eq!(action.name(), "ToggleColumnLayout");
        assert_eq!(action.category(), Category::Display);
    }
}
