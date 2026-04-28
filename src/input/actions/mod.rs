//! Verbs invoked from keymap.rs match arms. Each submodule groups verbs by
//! feature area. Phase 2 (F2) adds the Action enum here and re-exports the
//! verbs as the dispatch target.

pub mod bookmarks;
pub mod concordance;
pub mod pickers;
pub mod settings;

// Action enum identifying every reader-mode behavior. F2 maps KeyCombo →
// Action via Keymap; dispatch_action in keymap.rs translates Action into
// the corresponding verb call.

use serde::{Deserialize, Serialize};

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
    OpenMediaPicker,
    OpenConcordancePicker,
    OpenConcordanceWordPicker,
    OpenConcordanceListPicker,
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
    ToggleVocabHighlight,

    // Visual / selection
    EnterVisualMode,
    WordCycleCopy,
    WordCollectCopy,

    // Translations
    ToggleTranslations,

    // Settings (in reader)
    AdjustFontSizeUp,
    AdjustFontSizeDown,
    ResetFontSize,
    CycleFontForward,
    CycleFontBackward,
    ToggleSignColumn,
    ToggleCursorLine,
    ToggleDim,
    ShowFontInfo,

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

    // Multi-key chords (entry — completion handled by KeyState)
    PendingG,

    // Search (in reader, when matches present)
    SearchNextMatch,
    SearchPrevMatch,
}
