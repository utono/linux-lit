# Action Categories, Action Name Logging, and Async Media Picker DB Write

Two independent improvements to the keymap/dispatch system, designed together because they share the same review origin (F3, F5, F8 from the 2026-04-29 navigation-keymap review).

## Part A: Action Categories + Action Name Logging (F3 + F8)

### Goal

Make each `Action` carry a runtime-queryable `Category` and a string name. Categories enable future grouping in the Ctrl+/ overlay and align linux-lit's binding vocabulary with lue's categorized shortcuts. Action names enable grep-able debug logging at the dispatch boundary.

### Category Enum

Seven categories, consolidated from the ~14 comment headers in `dispatch_action`:

- **Navigation** — Page nav, cursor/dialogue, chapter/scene, bookmarks (PageForward, PageBackward, PageBackwardBottom, JumpToStart, JumpToEnd, CursorNextDialogue, CursorPrevLine, CursorToPageBottom, JumpToNextDialogue, JumpToPrevDialogue, JumpToNextChapter, JumpToPrevChapter, JumpToNextScene, JumpToPrevScene, ToggleBookmark, NextBookmark, PrevBookmark, JumpToRecentBookmark, OpenBookmarkPicker)
- **Media** — MPV playback, seek, volume, speed, sync (TogglePlaybackSync, TogglePlayback, SeekShortBackward, SeekShortForward, SeekLongBackward, SeekLongForward, SeekBackward30, VolumeUp, VolumeDown, TogglePlaybackSpeed)
- **Vocab** — Vocab popup, highlighting, concordance (ToggleVocabPopup, VocabPopupNext, VocabPopupPrev, JumpToNextVocab, JumpToPrevVocab, ToggleVocabHighlight, OpenConcordancePicker, OpenConcordanceWordPicker, OpenConcordanceListPicker)
- **Display** — Font, dim, cursor line, sign column, translations, settings overlay (AdjustFontSizeUp, AdjustFontSizeDown, ResetFontSize, CycleFontForward, CycleFontBackward, ToggleSignColumn, ToggleCursorLine, ToggleDim, ShowFontInfo, ToggleTranslations, OpenSettingsOverlay)
- **Selection** — Visual mode, word copy (EnterVisualMode, WordCycleCopy, WordCollectCopy)
- **Timestamps** — All timestamp actions (SetStartTime, SetEndTime, SetChapter, DeleteTimestamp, NudgeStartBackward, NudgeStartForward, UndoTimestamp, PlayCurrentLine)
- **App** — Quit, debug, clipboard, search, library/media/keybinds pickers, chords (SaveAndQuit, ToggleDebugLogging, CopyLineMappingId, PendingG, SearchNextMatch, SearchPrevMatch, OpenLibraryPicker, OpenMediaPicker, OpenKeybindsOverlay, OpenSearch)

### Action Methods

Two methods on `Action`:

- `category(&self) -> Category` — match returning the fixed category for each variant.
- `name(&self) -> &'static str` — returns the variant name as a string. Implemented via serde round-trip (`serde_json::to_string` on the enum, strip quotes) or a manual match. The serde approach is fewer lines but allocates; the manual match is zero-alloc. Since this is called once per keypress (not hot-loop), either is fine. Prefer the serde approach for maintainability — adding a new Action variant automatically gets a name without updating a match arm.

### Grouped Default Bindings

`default_reader_bindings()` in `keymap_config.rs` refactors into 7 sub-functions, one per category:

- `nav_bindings() -> Vec<(KeyCombo, Action)>`
- `media_bindings() -> Vec<(KeyCombo, Action)>`
- `vocab_bindings() -> Vec<(KeyCombo, Action)>`
- `display_bindings() -> Vec<(KeyCombo, Action)>`
- `selection_bindings() -> Vec<(KeyCombo, Action)>`
- `timestamp_bindings() -> Vec<(KeyCombo, Action)>`
- `app_bindings() -> Vec<(KeyCombo, Action)>`

`default_reader_bindings()` calls all 7 and merges into one `HashMap<KeyCombo, Action>`. The runtime `Keymap` struct stays unchanged — flat HashMap lookup. The grouping is organizational only at this layer.

### Dispatch Logging

One log line at the top of `dispatch_action`, before the match:

```rust
crate::logging::log(&format!("ACTION: {}", action.name()));
```

### What Doesn't Change

- `Keymap` struct stays a flat `HashMap<KeyCombo, Action>`.
- `keymap.json` schema stays the same (flat `reader` array). A `"category"` field in JSON is deferred.
- The Ctrl+/ overlay rendering is not touched — it can query `Action::category()` in a future pass.
- No changes to overlay key routing.

---

## Part B: Async Media Picker DB Write (F5)

### Goal

Move the synchronous database write in the media picker's `"p"` key handler off the GTK main thread, eliminating a potential UI freeze on slow disk or locked database.

### Current State

`keymap.rs:270-318` — the media picker's `"p"` handler (when search entry is not focused) calls `open_db_rw()`, runs `set_media_priority()`, queries the result, and updates the picker widget, all synchronously inside the key handler.

### Design

Extract into a new verb function: `actions::pickers::set_media_default(state: &Rc<RefCell<AppState>>, tokio_handle: &tokio::runtime::Handle)`.

The verb:
1. Reads `media_id` and `abbrev` from `state` (borrow, extract, drop).
2. Spawns the DB work via `tokio_handle.spawn_blocking(move || { ... })` — opens the connection, calls `set_media_priority`, reads the new priority.
3. On completion, updates the picker widget via `glib::spawn_future_local` — calls `state.borrow_mut().media_picker.set_default(media_id, max_pri)`.

The key handler in `keymap.rs` becomes:

```rust
"p" => {
    let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
    if !is_search_focused {
        crate::input::actions::pickers::set_media_default(state, tokio_handle);
        return true;
    }
}
```

This matches the established async DB pattern in `actions::bookmarks::toggle_bookmark`.

### Error Handling

DB errors are logged (matching the current behavior) and silently dropped — the picker stays in its current state. No user-visible error UI needed; this matches all other DB error handling in linux-lit.
