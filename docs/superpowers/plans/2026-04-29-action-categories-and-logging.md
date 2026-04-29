# Action Categories and Name Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a runtime-queryable `Category` enum and a `name()` method to `Action`, group default bindings by category, and log the resolved action name in `dispatch_action`.

**Architecture:** `Category` enum and `Action` methods live in `src/input/actions/mod.rs`. Default bindings in `keymap_config.rs` refactor from one flat function into 7 category sub-functions merged by the top-level function. One log line added to `dispatch_action` in `keymap.rs`.

**Tech Stack:** Rust, serde, serde_json

---

### Task 1: Add Category enum and Action::category() method

**Files:**
- Modify: `src/input/actions/mod.rs`

- [ ] **Step 1: Write tests for category() and name()**

Add at the bottom of `src/input/actions/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_category() {
        // Iterate all actions via a known list and verify category returns without panic.
        let actions = [
            Action::PageForward, Action::CursorNextDialogue, Action::ToggleBookmark,
            Action::OpenLibraryPicker, Action::TogglePlayback, Action::ToggleVocabPopup,
            Action::EnterVisualMode, Action::ToggleTranslations, Action::AdjustFontSizeUp,
            Action::SetStartTime, Action::SaveAndQuit, Action::SearchNextMatch,
        ];
        for a in &actions {
            let _ = a.category(); // should not panic
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib actions::tests`
Expected: FAIL — `Category` type and methods don't exist yet.

- [ ] **Step 3: Add Category enum**

In `src/input/actions/mod.rs`, after the `use serde::{Deserialize, Serialize};` line (line 14), add:

```rust
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
```

- [ ] **Step 4: Add Action::category() method**

After the closing `}` of the `Action` enum (after line 113), add:

```rust
impl Action {
    pub fn category(&self) -> Category {
        use Action::*;
        match self {
            // Navigation
            PageForward | PageBackward | PageBackwardBottom
            | JumpToStart | JumpToEnd
            | CursorNextDialogue | CursorPrevLine | CursorToPageBottom
            | JumpToNextDialogue | JumpToPrevDialogue
            | JumpToNextChapter | JumpToPrevChapter
            | JumpToNextScene | JumpToPrevScene
            | ToggleBookmark | NextBookmark | PrevBookmark
            | JumpToRecentBookmark | OpenBookmarkPicker => Category::Navigation,

            // Media
            TogglePlaybackSync | TogglePlayback
            | SeekShortBackward | SeekShortForward
            | SeekLongBackward | SeekLongForward | SeekBackward30
            | VolumeUp | VolumeDown | TogglePlaybackSpeed => Category::Media,

            // Vocab
            ToggleVocabPopup | VocabPopupNext | VocabPopupPrev
            | JumpToNextVocab | JumpToPrevVocab | ToggleVocabHighlight
            | OpenConcordancePicker | OpenConcordanceWordPicker
            | OpenConcordanceListPicker => Category::Vocab,

            // Display
            AdjustFontSizeUp | AdjustFontSizeDown | ResetFontSize
            | CycleFontForward | CycleFontBackward
            | ToggleSignColumn | ToggleCursorLine | ToggleDim | ShowFontInfo
            | ToggleTranslations | OpenSettingsOverlay => Category::Display,

            // Selection
            EnterVisualMode | WordCycleCopy | WordCollectCopy => Category::Selection,

            // Timestamps
            SetStartTime | SetEndTime | SetChapter | DeleteTimestamp
            | NudgeStartBackward | NudgeStartForward
            | UndoTimestamp | PlayCurrentLine => Category::Timestamps,

            // App
            SaveAndQuit | ToggleDebugLogging | CopyLineMappingId
            | PendingG | SearchNextMatch | SearchPrevMatch
            | OpenLibraryPicker | OpenMediaPicker
            | OpenKeybindsOverlay | OpenSearch => Category::App,
        }
    }

    pub fn name(&self) -> &'static str {
        // Leak a static string from serde's serialization. This runs once
        // per keypress (not hot-loop), so the convenience is worth it.
        // Each variant serializes to e.g. "\"PageForward\""; strip quotes.
        // Use a match instead for zero-alloc:
        use Action::*;
        match self {
            PageForward => "PageForward",
            PageBackward => "PageBackward",
            PageBackwardBottom => "PageBackwardBottom",
            JumpToStart => "JumpToStart",
            JumpToEnd => "JumpToEnd",
            CursorNextDialogue => "CursorNextDialogue",
            CursorPrevLine => "CursorPrevLine",
            CursorToPageBottom => "CursorToPageBottom",
            JumpToNextDialogue => "JumpToNextDialogue",
            JumpToPrevDialogue => "JumpToPrevDialogue",
            JumpToNextChapter => "JumpToNextChapter",
            JumpToPrevChapter => "JumpToPrevChapter",
            JumpToNextScene => "JumpToNextScene",
            JumpToPrevScene => "JumpToPrevScene",
            ToggleBookmark => "ToggleBookmark",
            NextBookmark => "NextBookmark",
            PrevBookmark => "PrevBookmark",
            JumpToRecentBookmark => "JumpToRecentBookmark",
            OpenBookmarkPicker => "OpenBookmarkPicker",
            OpenLibraryPicker => "OpenLibraryPicker",
            OpenMediaPicker => "OpenMediaPicker",
            OpenConcordancePicker => "OpenConcordancePicker",
            OpenConcordanceWordPicker => "OpenConcordanceWordPicker",
            OpenConcordanceListPicker => "OpenConcordanceListPicker",
            OpenSettingsOverlay => "OpenSettingsOverlay",
            OpenKeybindsOverlay => "OpenKeybindsOverlay",
            OpenSearch => "OpenSearch",
            TogglePlaybackSync => "TogglePlaybackSync",
            TogglePlayback => "TogglePlayback",
            SeekShortBackward => "SeekShortBackward",
            SeekShortForward => "SeekShortForward",
            SeekLongBackward => "SeekLongBackward",
            SeekLongForward => "SeekLongForward",
            SeekBackward30 => "SeekBackward30",
            VolumeUp => "VolumeUp",
            VolumeDown => "VolumeDown",
            TogglePlaybackSpeed => "TogglePlaybackSpeed",
            ToggleVocabPopup => "ToggleVocabPopup",
            VocabPopupNext => "VocabPopupNext",
            VocabPopupPrev => "VocabPopupPrev",
            JumpToNextVocab => "JumpToNextVocab",
            JumpToPrevVocab => "JumpToPrevVocab",
            ToggleVocabHighlight => "ToggleVocabHighlight",
            EnterVisualMode => "EnterVisualMode",
            WordCycleCopy => "WordCycleCopy",
            WordCollectCopy => "WordCollectCopy",
            ToggleTranslations => "ToggleTranslations",
            AdjustFontSizeUp => "AdjustFontSizeUp",
            AdjustFontSizeDown => "AdjustFontSizeDown",
            ResetFontSize => "ResetFontSize",
            CycleFontForward => "CycleFontForward",
            CycleFontBackward => "CycleFontBackward",
            ToggleSignColumn => "ToggleSignColumn",
            ToggleCursorLine => "ToggleCursorLine",
            ToggleDim => "ToggleDim",
            ShowFontInfo => "ShowFontInfo",
            SetStartTime => "SetStartTime",
            SetEndTime => "SetEndTime",
            SetChapter => "SetChapter",
            DeleteTimestamp => "DeleteTimestamp",
            NudgeStartBackward => "NudgeStartBackward",
            NudgeStartForward => "NudgeStartForward",
            UndoTimestamp => "UndoTimestamp",
            PlayCurrentLine => "PlayCurrentLine",
            SaveAndQuit => "SaveAndQuit",
            ToggleDebugLogging => "ToggleDebugLogging",
            CopyLineMappingId => "CopyLineMappingId",
            PendingG => "PendingG",
            SearchNextMatch => "SearchNextMatch",
            SearchPrevMatch => "SearchPrevMatch",
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib actions::tests`
Expected: 3 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/mod.rs
git commit -m "Add Category enum and Action::category()/name() methods

Seven categories: Navigation, Media, Vocab, Display, Selection,
Timestamps, App. Each Action variant maps to exactly one category.
name() returns the variant name as a static string for debug logging."
```

---

### Task 2: Group default_reader_bindings into category sub-functions

**Files:**
- Modify: `src/input/keymap_config.rs:170-284`

- [ ] **Step 1: Write test for grouped bindings**

In `src/input/keymap_config.rs`, add to the existing `tests` module (after the last test):

```rust
    #[test]
    fn grouped_bindings_match_flat_count() {
        let m = default_reader_bindings();
        // The grouped sub-functions should produce the same total as the flat map.
        // If this fails after adding a binding to a sub-function, the flat merge
        // in default_reader_bindings is missing it.
        assert!(m.len() > 50, "expected ~70 default bindings, got {}", m.len());
    }
```

This test already exists as `default_reader_bindings_returns_nonempty_map` — but we add an explicit count check. Actually, the existing test already checks `> 50`. We just need to make sure the refactored function returns the same count. Record the current count first:

Run: `cargo test --lib keymap_config::tests::default_reader_bindings_returns_nonempty_map -- --nocapture`

Note the count, then proceed.

- [ ] **Step 2: Replace default_reader_bindings with grouped sub-functions**

Replace the entire `default_reader_bindings` function body (lines 170-284 of `src/input/keymap_config.rs`) with:

```rust
/// Compiled-in default reader bindings, assembled from per-category
/// sub-functions. Each sub-function groups bindings by the Action's
/// Category for organizational clarity; the runtime Keymap is a flat
/// HashMap.
pub fn default_reader_bindings() -> HashMap<KeyCombo, Action> {
    let mut m = HashMap::new();
    for (combo, action) in nav_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in media_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in vocab_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in display_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in selection_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in timestamp_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in app_bindings() {
        m.insert(combo, action);
    }
    m
}

fn nav_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        // Page navigation
        (KeyCombo::plain("x"), Action::PageForward),
        (KeyCombo::plain("y"), Action::PageBackward),
        (KeyCombo::plain("less"), Action::PageBackward),
        (KeyCombo::plain("space"), Action::PageForward),
        (KeyCombo::shift("space"), Action::PageBackward),
        (KeyCombo::ctrl("f"), Action::PageForward),
        (KeyCombo::ctrl("u"), Action::PageForward),
        (KeyCombo::ctrl("b"), Action::PageBackward),
        // Cursor / dialogue
        (KeyCombo::plain("j"), Action::CursorNextDialogue),
        (KeyCombo::plain("k"), Action::CursorPrevLine),
        (KeyCombo::plain("Q"), Action::CursorToPageBottom),
        (KeyCombo::plain("Up"), Action::JumpToPrevDialogue),
        (KeyCombo::shift("Up"), Action::PageBackwardBottom),
        (KeyCombo::plain("Down"), Action::JumpToNextDialogue),
        (KeyCombo::plain("comma"), Action::JumpToPrevDialogue),
        (KeyCombo::shift("comma"), Action::PageBackwardBottom),
        (KeyCombo::plain("q"), Action::JumpToNextDialogue),
        // Multi-key chord entry (gg → JumpToStart)
        (KeyCombo::plain("g"), Action::PendingG),
        (KeyCombo::plain("G"), Action::JumpToEnd),
        // Chapter / scene
        (KeyCombo::plain("bracketleft"), Action::JumpToPrevChapter),
        (KeyCombo::plain("braceleft"), Action::JumpToNextChapter),
        (KeyCombo::plain("2"), Action::JumpToPrevScene),
        (KeyCombo::plain("3"), Action::JumpToNextScene),
        // Bookmarks
        (KeyCombo::plain("m"), Action::ToggleBookmark),
        (KeyCombo::plain("semicolon"), Action::NextBookmark),
        (KeyCombo::shift("semicolon"), Action::PrevBookmark),
        (KeyCombo::plain("colon"), Action::PrevBookmark),
        (KeyCombo::ctrl("m"), Action::OpenBookmarkPicker),
    ]
}

fn media_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("s"), Action::TogglePlaybackSync),
        (KeyCombo::plain("Tab"), Action::TogglePlayback),
        (KeyCombo::plain("o"), Action::SeekShortBackward),
        (KeyCombo::plain("e"), Action::SeekShortForward),
        (KeyCombo::plain("O"), Action::SeekLongBackward),
        (KeyCombo::plain("E"), Action::SeekLongForward),
        (KeyCombo::plain("Left"), Action::SeekBackward30),
        (KeyCombo::ctrl("Up"), Action::VolumeUp),
        (KeyCombo::ctrl("Down"), Action::VolumeDown),
        (KeyCombo::plain("plus"), Action::TogglePlaybackSpeed),
    ]
}

fn vocab_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("h"), Action::ToggleVocabPopup),
        (KeyCombo::plain("backslash"), Action::VocabPopupNext),
        (KeyCombo::plain("numbersign"), Action::VocabPopupPrev),
        (KeyCombo::plain("r"), Action::JumpToNextVocab),
        (KeyCombo::plain("R"), Action::JumpToPrevVocab),
        (KeyCombo::alt("backslash"), Action::ToggleVocabHighlight),
        (KeyCombo::ctrl("backslash"), Action::OpenConcordancePicker),
        (KeyCombo::ctrl_shift("P"), Action::OpenConcordanceWordPicker),
        (KeyCombo::ctrl_alt("p"), Action::OpenConcordanceListPicker),
    ]
}

fn display_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("exclam"), Action::AdjustFontSizeDown),
        (KeyCombo::plain("bar"), Action::AdjustFontSizeUp),
        (KeyCombo::plain("0"), Action::ResetFontSize),
        (KeyCombo::plain("f"), Action::CycleFontForward),
        (KeyCombo::plain("F"), Action::CycleFontBackward),
        (KeyCombo::plain("l"), Action::ToggleSignColumn),
        (KeyCombo::plain("minus"), Action::ToggleCursorLine),
        (KeyCombo::alt("d"), Action::ToggleDim),
        (KeyCombo::alt("f"), Action::ShowFontInfo),
        (KeyCombo::plain("i"), Action::ToggleTranslations),
        (KeyCombo::ctrl("comma"), Action::OpenSettingsOverlay),
    ]
}

fn selection_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("V"), Action::EnterVisualMode),
        (KeyCombo::plain("w"), Action::WordCycleCopy),
        (KeyCombo::plain("W"), Action::WordCollectCopy),
    ]
}

fn timestamp_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("u"), Action::SetStartTime),
        (KeyCombo::plain("Right"), Action::SetStartTime),
        (KeyCombo::alt("i"), Action::SetEndTime),
        (KeyCombo::plain("period"), Action::SetChapter),
        (KeyCombo::plain("BackSpace"), Action::DeleteTimestamp),
        (KeyCombo::plain("p"), Action::NudgeStartBackward),
        (KeyCombo::plain("P"), Action::NudgeStartForward),
        (KeyCombo::plain("U"), Action::UndoTimestamp),
        (KeyCombo::plain("a"), Action::PlayCurrentLine),
    ]
}

fn app_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::ctrl("d"), Action::ToggleDebugLogging),
        (KeyCombo::ctrl("p"), Action::OpenLibraryPicker),
        (KeyCombo::ctrl_shift("M"), Action::OpenMediaPicker),
        (KeyCombo::ctrl("slash"), Action::OpenKeybindsOverlay),
        (KeyCombo::plain("slash"), Action::OpenSearch),
        (KeyCombo::ctrl_alt("l"), Action::SaveAndQuit),
        (KeyCombo::ctrl("y"), Action::CopyLineMappingId),
        (KeyCombo::plain("n"), Action::SearchNextMatch),
        (KeyCombo::plain("N"), Action::SearchPrevMatch),
    ]
}
```

Note: `PendingG` and `G` are in `nav_bindings` (they're navigation chord entry). `ToggleDebugLogging` moved to `app_bindings`. `OpenSettingsOverlay` is in `display_bindings`. Concordance pickers are in `vocab_bindings`. `OpenLibraryPicker`, `OpenMediaPicker`, `OpenKeybindsOverlay`, `OpenSearch` are in `app_bindings`.

- [ ] **Step 3: Run all keymap_config tests**

Run: `cargo test --lib keymap_config::tests`
Expected: all 7 existing tests PASS. The binding count should be unchanged.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap_config.rs
git commit -m "Group default_reader_bindings into 7 category sub-functions

nav_bindings, media_bindings, vocab_bindings, display_bindings,
selection_bindings, timestamp_bindings, app_bindings. Each returns
Vec<(KeyCombo, Action)>; default_reader_bindings merges them into
the flat HashMap. Organizational refactor — no runtime change."
```

---

### Task 3: Add action name logging to dispatch_action

**Files:**
- Modify: `src/input/keymap.rs:758-765`

- [ ] **Step 1: Add the log line**

In `src/input/keymap.rs`, inside `dispatch_action`, after the `use crate::input::actions::Action::*;` line (line 764) and before `match action {` (line 765), add:

```rust
    crate::logging::log(&format!("ACTION: {}", action.name()));
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Log resolved action name in dispatch_action

Every keypress that reaches dispatch_action now logs 'ACTION: PageForward'
(or whichever variant). Enables grep-able debugging of keybind dispatch."
```
