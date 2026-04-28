# F4 + F2 Design: Verbs Out of Dispatch + Keymap as Data

**Status:** Approved (brainstorm 2026-04-28).
**Source review:** `docs/reviews/2026-04-28-keymap-vs-references.md` F4 and F2.
**Out of scope:** F1 (`OverlayMode` trait — deferred to a separate keymap-mode brainstorm); F3 (chord state via one-shot modes — depends on F1); per-overlay keymaps in JSON; modifier-conditional overloads (`Ctrl+p` open vs nav).

---

## Problem

`src/input/keymap.rs` is a single ~1750-line `handle_key` function that walks ~13 overlay-visibility checks via if/else before reaching base reader keys, with every overlay's keys, every state mutation, and every async dispatch inlined at its match arm. Two pain points the review surfaced:

**F4 — verbs inlined at dispatch sites.** ~10 match arms contain 30-80 line bodies that spawn async work, mutate state, send MPV commands, and manipulate Vec/HashMap state inline. Borrow-checker traps documented in commit comments (`set_text triggers connect_changed which borrows state, so the mutable borrow must be dropped first`). Action testing requires synthesizing key events.

**F2 — keymap embedded in dispatch.** Every key string is hard-coded in a match arm. Re-binding "x" from `page_forward` to `page_backward` requires editing source and recompiling. The keybinds-overlay (Ctrl+/) is rendered from static markdown — no automated check that displayed keys match dispatch.

---

## Reference shape

**F4:** `bk/src/main.rs:111-275` — `Bk` struct has 8 small named methods (`jump`, `jump_byte`, `mark`, `search`, `pad`, etc.). Each ≤30 lines. View modes call them via `bk.method(...)` from match arms; the modes are dispatch only.

**F2:** `lue/lue/input_handler.py:9-37` — `DEFAULT_KEYBOARD_SHORTCUTS` is a nested dict. `process_input` walks it via `_matches_shortcut(data, nav_shortcuts.get("next_paragraph", "k"))`. `load_keyboard_shortcuts(file_path)` swaps the table at runtime.

linux-lit's substrate (Rust + GTK4 + async tokio) makes neither pattern translate verbatim. F4 borrows the verbs-as-functions shape; F2 borrows the data-driven dispatch shape via `serde_json` + a static default function.

---

## F4: Verbs out of dispatch

### Module layout

New `src/input/actions/` directory partitioned by feature area. Each verb is a free function (not a method on AppState — see Q3 in brainstorm; AppState already 2700+ lines and growing).

```
src/input/actions/
├── mod.rs              # re-exports public verbs; later (F2) defines Action enum
├── bookmarks.rs        # toggle_bookmark, jump_to_recent_bookmark
├── pickers.rs          # load_selected_work, open_bookmark_picker,
                        # open_media_picker, confirm_media_selection,
                        # delete_bookmark
├── concordance.rs      # open_concordance_picker, handle_word_selection
└── settings.rs         # apply_settings_change, revert_to_snapshot,
                        # reset_to_defaults
```

### Verb signature shape

Two variants:

```rust
// Async verb (most common) — takes Rc<RefCell<AppState>> for spawn closures:
pub fn toggle_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    // Internally clones state, spawns glib::spawn_future_local, etc.
}

// Sync verb (no spawn, no async) — takes &mut AppState directly:
pub fn revert_to_snapshot(state: &mut AppState) {
    // ...
}
```

All verbs:
- Encapsulate the entire spawn/borrow/await dance internally.
- Return `()` or `bool` (true if action was meaningful, mirrors current keymap arm conventions for use as a match-arm tail expression).
- Take `&tokio::runtime::Handle` only if they spawn.

### Verbs to extract (12 total)

12 verbs to relocate, of which 3 are already free fns in `keymap.rs` and only need to move (no body changes). Locations refer to current `keymap.rs` line ranges:

1. `pickers::load_selected_work` — `keymap.rs:17-73` (already a free fn `load_selected_work` at top of file; pure relocation).
2. `pickers::open_bookmark_picker` — `keymap.rs:651-679` (Ctrl+m).
3. `pickers::open_media_picker` — `keymap.rs:620-648` (Ctrl+Shift+M).
4. `pickers::delete_bookmark` — `keymap.rs:343-393` (Delete/d in bookmark picker).
5. `pickers::confirm_media_selection` — `keymap.rs:438-503` (Return in media picker).
6. `bookmarks::toggle_bookmark` — `keymap.rs:1591-1631` (m in reader).
7. `bookmarks::jump_to_recent_bookmark` — `keymap.rs:1110-1147` (g; chord).
8. `concordance::open_concordance_picker` — `keymap.rs:1225-1253` (Ctrl+\).
9. `concordance::handle_word_selection` — `keymap.rs:77-157` (already a free fn `handle_concordance_word_selection`; pure relocation).
10. `settings::apply_settings_change` — `keymap.rs:1782-1831` (already a helper; pure relocation).
11. `settings::revert_to_snapshot` — `keymap.rs:702-741` (Escape in settings, 40 lines).
12. `settings::reset_to_defaults` — `keymap.rs:777-812` (r in settings, 36 lines).

### Call-site shape change

Before:
```rust
"m" => {
    let (abbrev, line_mapping_id, buffer_line) = { ... };
    if let (Some(abbrev), Some(lm_id)) = (abbrev, line_mapping_id) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            // 30+ lines
        });
    }
    true
}
```

After:
```rust
"m" => {
    crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle);
    true
}
```

### Tests

No new unit tests for F4. Verbs are GTK-bound (touch AppState fields, gtk widgets, db calls). Verification model: `cargo build` + manual smoke test that each action still works after extraction.

### Effort

M. 12 verbs, mostly mechanical relocation (3 are pure relocations of existing free fns). Each verb extraction is a small commit. No public API change; verbs are crate-private (`pub(crate)`).

---

## F2: Keymap as data

### Action enum (pure verbs, no payloads)

`src/input/actions/mod.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub enum Action {
    // Page navigation
    PageForward, PageBackward, PageBackwardBottom,
    JumpToStart, JumpToEnd,

    // Cursor / dialogue navigation
    CursorNextDialogue, CursorPrevLine, CursorToPageBottom,
    JumpToNextDialogue, JumpToPrevDialogue,
    JumpToNextChapter, JumpToPrevChapter,
    JumpToNextScene, JumpToPrevScene,

    // Bookmarks
    ToggleBookmark, NextBookmark, PrevBookmark,
    JumpToRecentBookmark,    // g; chord
    OpenBookmarkPicker,      // Ctrl+m

    // Pickers / overlays
    OpenLibraryPicker,       // Ctrl+p
    OpenMediaPicker,         // Ctrl+Shift+M
    OpenConcordancePicker,   // Ctrl+\
    OpenConcordanceWordPicker, // Ctrl+Shift+P
    OpenConcordanceListPicker, // Ctrl+Alt+p
    OpenSettingsOverlay,     // Ctrl+,
    OpenKeybindsOverlay,     // Ctrl+/
    OpenSearch,              // /

    // MPV / media
    TogglePlaybackSync,      // s
    TogglePlayback,          // Tab
    SeekShortBackward, SeekShortForward,    // o, e (compiled-in 3.5s)
    SeekLongBackward, SeekLongForward,      // O, E (compiled-in 60s)
    SeekBackward30,          // Left (compiled-in 30s)
    VolumeUp, VolumeDown,    // Ctrl+Up, Ctrl+Down (compiled-in 5%)
    TogglePlaybackSpeed,     // + (compiled-in 1.0/1.3 toggle)

    // Vocab / glossing
    ToggleVocabPopup,        // h
    VocabPopupNext,          // \ when popup visible
    VocabPopupPrev,          // # when popup visible
    JumpToNextVocab,         // r (when no concordance)
    JumpToPrevVocab,         // R (when no concordance)
    ToggleVocabHighlight,    // Alt+\

    // Visual / selection
    EnterVisualMode,         // V
    WordCycleCopy,           // w
    WordCollectCopy,         // W

    // Translations
    ToggleTranslations,      // i

    // Settings (in reader)
    AdjustFontSizeUp, AdjustFontSizeDown,    // |, !
    ResetFontSize,           // 0
    CycleFontForward,        // f
    CycleFontBackward,       // F
    ToggleSignColumn,        // l
    ToggleCursorLine,        // -
    ToggleDim,               // Alt+d
    ShowFontInfo,            // Alt+f

    // Timestamps
    SetStartTime,            // u, Right
    SetEndTime,              // Alt+i
    SetChapter,              // .
    DeleteTimestamp,         // BackSpace
    NudgeStartBackward,      // p
    NudgeStartForward,       // P
    UndoTimestamp,           // U
    PlayCurrentLine,         // a

    // App
    SaveAndQuit,             // Ctrl+Alt+l
    ToggleDebugLogging,      // Ctrl+d
    CopyLineMappingId,       // Ctrl+y

    // Multi-key chords (entry — completion handled by KeyState)
    PendingG,                // g (waits for second key)

    // Search (in reader, when matches present)
    SearchNextMatch,         // n
    SearchPrevMatch,         // N
}
```

Per Q5 decision A: pure verbs, no payloads. Numeric parameters that today live inline (seek 3.5s, volume 5%, etc.) become compiled-in constants — separate from the keymap. If a user wants different seek values, that's a config concern (`seek_short`, `seek_long`), not a keymap concern.

### KeyCombo type

`src/input/keymap_config.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct KeyCombo {
    pub key: String,         // GDK key name: "x", "Return", "BackSpace", "comma", etc.
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl KeyCombo {
    pub fn plain(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: false, alt: false }
    }
    pub fn ctrl(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: false, alt: false }
    }
    // ... ctrl_shift, alt, ctrl_alt builders for common combos
}
```

GDK key names match what `keymap.rs:169` already logs (`KEY: name=j ctrl=...`). No translation needed.

### Keymap struct

```rust
pub struct Keymap {
    pub reader: HashMap<KeyCombo, Action>,
}

impl Keymap {
    pub fn load() -> Self {
        let path = config_path();  // ~/.config/linux-lit/keymap.json
        if let Ok(text) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<KeymapJson>(&text) {
                Ok(json) => {
                    let mut km = Self::default();
                    json.merge_into(&mut km);
                    km
                }
                Err(e) => {
                    crate::logging::log(&format!(
                        "keymap.json parse error: {}; using defaults", e));
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }

    pub fn lookup(&self, key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
        let combo = KeyCombo {
            key: key.to_string(), ctrl, shift, alt,
        };
        self.reader.get(&combo).copied()
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self { reader: default_reader_bindings() }
    }
}

fn default_reader_bindings() -> HashMap<KeyCombo, Action> {
    let mut m = HashMap::new();
    m.insert(KeyCombo::plain("x"), Action::PageForward);
    m.insert(KeyCombo::plain("y"), Action::PageBackward);
    m.insert(KeyCombo::plain("j"), Action::CursorNextDialogue);
    m.insert(KeyCombo::plain("k"), Action::CursorPrevLine);
    m.insert(KeyCombo::ctrl("f"), Action::PageForward);
    m.insert(KeyCombo::ctrl("b"), Action::PageBackward);
    // ... ~70 entries total
    m
}
```

### JSON file format

A list of objects:

```json
{
  "reader": [
    {"key": "x", "action": "PageForward"},
    {"key": "y", "action": "PageBackward"},
    {"key": "j", "action": "CursorNextDialogue"},
    {"key": "f", "ctrl": true, "action": "PageForward"},
    {"key": "M", "ctrl": true, "shift": true, "action": "OpenMediaPicker"}
  ]
}
```

A list (not a map keyed by KeyCombo) because JSON map keys are strings; serializing KeyCombo to a string would require a custom format. List of objects is straightforward to read, write, and grep.

### Stow package

Per Q-stow: keymap.json is managed by stow. New stow package:

```
~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json
```

Deploy via `cd ~/tty-dotfiles && stow linux-lit`. Result: `~/.config/linux-lit/keymap.json` becomes a symlink to the version-controlled defaults. Users who want custom bindings either edit the file in tty-dotfiles (and commit) or remove the symlink and write a local copy.

The shipped `keymap.json` is the canonical default — exact same bindings as `default_reader_bindings()` returns, just in JSON. If a user removes their `keymap.json` entirely, linux-lit falls back to compiled-in defaults (which match).

### Dispatch shape change

The base-keys section (currently `keymap.rs:1338-1741`, ~400 lines of `match key_name`) becomes:

```rust
// At the bottom of handle_key, after all overlay-visible blocks:
if let Some(action) = state.borrow().keymap.lookup(key_name, is_ctrl, is_shift, is_alt) {
    return dispatch_action(state, action, key_state, tokio_handle);
}
return false;

fn dispatch_action(
    state: &Rc<RefCell<AppState>>,
    action: Action,
    key_state: &Rc<RefCell<KeyState>>,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    use Action::*;
    match action {
        PageForward => {
            navigation::page_forward(&mut state.borrow_mut());
            true
        }
        PageBackward => {
            navigation::page_backward(&mut state.borrow_mut());
            true
        }
        ToggleBookmark => {
            actions::bookmarks::toggle_bookmark(state, tokio_handle);
            true
        }
        PendingG => {
            // chord entry — set state, schedule clear
            key_state.borrow_mut().pending_g = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_g = false;
            });
            true
        }
        // ...
    }
}
```

`dispatch_action` is one canonical action-to-verb table. Adding a new action = add enum variant + dispatch arm + (usually) a new verb in `actions::*`. The keymap entry is data (JSON or default fn).

### Validation

- Unknown action name in JSON → log warning, skip that binding.
- Malformed JSON → log warning, fall back entirely to defaults.
- Conflicting bindings (same KeyCombo defined twice) → last one wins (HashMap insert behavior), log info.
- Empty `reader` list in JSON → log warning, use defaults.

### Out of scope for F2

- **Per-overlay keymaps** (F1 territory). Overlay-visible blocks (`picker_visible`, `bookmark_picker_visible`, `settings_visible`, etc.) keep their inline match arms unchanged.
- **Multi-key chord state** (`pending_g`, `pending_ctrl_slash`). Chord ENTRY (`g` press) routes through Action::PendingG, but chord COMPLETION (the second key dispatch) stays in the inline `if key_state.borrow().pending_g` block. F3 from the review will lift this under F1.
- **Modifier-conditional overloads** (e.g., `Ctrl+p` opens picker OR moves selection depending on visibility). Stays inline because the disambiguation is overlay-state-dependent, which the keymap data model doesn't capture.

### Tests

Pure unit tests in `keymap_config_tests` mod:

1. `default_reader_bindings_returns_nonempty_map` — smoke test that defaults populate.
2. `default_reader_bindings_contains_known_bindings` — assert "x" → PageForward, "j" → CursorNextDialogue, etc. (~3-5 sentinel checks).
3. `keymap_load_parses_minimal_json` — synthetic JSON with one binding override; assert merge replaces the default.
4. `keymap_load_falls_back_on_missing_file` — `Keymap::load()` with no file → returns defaults.
5. `keymap_load_falls_back_on_malformed_json` — synthetic invalid JSON → returns defaults; warning logged.
6. `keymap_load_skips_unknown_action` — JSON references a non-existent action variant → that binding skipped, others kept.
7. `keymap_lookup_returns_none_for_unbound_key` — `lookup("zzz", ...)` → None.
8. `keymap_lookup_distinguishes_modifiers` — `("x", ctrl=false)` vs `("x", ctrl=true)` resolve to different actions when both are bound.

`Keymap::load()` reads from disk, so tests for it use a custom path injection (refactor `load(path: &Path) -> Self` and have the public no-arg `load()` call `load(&config_path())`).

### Effort

M. ~70 default bindings to enumerate, dispatch table to write, JSON parsing + validation, stow package setup. Mechanical but voluminous.

---

## Integration

### Phase order

Two sequential phases. F4 first (verbs out of dispatch), F2 second (keymap as data).

**Why:**
1. F4 is internal refactor with no public API change. Each verb extraction is a small, independent commit. Safe to ship even if F2 never lands.
2. After F4, F2's `Action` enum becomes mechanical naming — variants are exactly the public verb names. Defining the enum is bookkeeping, not design.

### Phase 1 sub-phases (F4)

Each sub-phase ends in commit + manual smoke test:

- **1a:** `actions/mod.rs` skeleton + relocate `apply_settings_change` + `handle_concordance_word_selection` (already free fns; pure relocation, smallest commit).
- **1b:** `actions/pickers.rs` — extract 5 picker verbs.
- **1c:** `actions/bookmarks.rs` — extract 2 bookmark verbs.
- **1d:** `actions/concordance.rs` — extract 1 verb (`open_concordance_picker`); move `handle_word_selection` here from 1a.
- **1e:** `actions/settings.rs` — extract `revert_to_snapshot`, `reset_to_defaults`. `apply_settings_change` already moved in 1a.

After Phase 1: `keymap.rs` shrinks by ~250 lines. Each affected match arm becomes a one-line verb dispatch. Build + manual smoke verifies each sub-phase.

### Phase 2 sub-phases (F2)

- **2a:** Add `Cargo.toml` dependency on `serde` + `serde_json` (probably already present transitively; confirm). Define `Action` enum + `KeyCombo` struct in `actions/mod.rs` + `keymap_config.rs`. Add `#[derive(Serialize, Deserialize)]`. No callers yet; warns dead_code.
- **2b:** Implement `Keymap` struct + `default_reader_bindings()` in new `src/input/keymap_config.rs`. Enumerate all current reader bindings (~70 entries) as defaults. Add unit tests.
- **2c:** Add `pub keymap: Keymap` field to AppState; populate in constructor with `Keymap::load()`.
- **2d:** Write `dispatch_action(state, action, key_state, tokio_handle)` table in `keymap.rs`. Wire after all overlay-visible blocks.
- **2e:** Migrate base-key match arms to keymap lookup. Each arm replaced with the lookup; final cleanup deletes the now-dead match block.
- **2f:** Create stow package `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` with the default bindings exported as JSON. Document in CLAUDE.md.

After Phase 2: keymap.rs's base-keys match block (lines 1338-1741, ~400 lines) collapses to ~5 lines (lookup + dispatch). Total `keymap.rs` reduction: ~650 lines net.

### File map

- **Create:**
  - `src/input/actions/mod.rs` (action enum, re-exports)
  - `src/input/actions/bookmarks.rs`
  - `src/input/actions/pickers.rs`
  - `src/input/actions/concordance.rs`
  - `src/input/actions/settings.rs`
  - `src/input/keymap_config.rs` (Keymap, KeyCombo, default_reader_bindings, load)
  - `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
- **Modify:**
  - `src/input/keymap.rs` — extract verbs (Phase 1), then collapse base-key match arms (Phase 2).
  - `src/input/mod.rs` — declare `actions` and `keymap_config` submodules.
  - `src/app.rs` — add `keymap: Keymap` field to AppState; init in constructor.
  - `Cargo.toml` — confirm/add `serde_json` dependency.
  - `~/utono/linux-lit/CLAUDE.md` — document the keymap.json + stow workflow.

### Manual verification protocol (used after each phase)

```
1. cargo build (must succeed; warnings only).
2. cargo run.
3. Reader navigation:
   - Press x, y, j, k, q, comma — confirm normal nav.
   - Press gg, G — confirm jump-to-start/end.
   - Press [, ], 2, 3 — chapter / scene jumps.
4. Bookmarks:
   - Press m to toggle bookmark; ; / : to walk bookmarks.
   - Ctrl+m to open bookmark picker; navigate, jump, delete.
   - Press g; for jump-to-recent.
5. Pickers:
   - Ctrl+p library picker; Ctrl+Shift+M media picker.
   - Ctrl+\ concordance picker.
6. Settings:
   - Ctrl+, opens settings; j/k navigates; h/l adjusts; r resets;
     Escape reverts; Return saves.
7. MPV / playback:
   - Tab toggles playback; o/e/O/E seek; +/- speed.
8. Vocab / glossing:
   - h toggles vocab popup; \ / # cycle words.
9. Confirm: 'verified' or describe regression.
```

After Phase 2 only, also:

```
10. Edit ~/.config/linux-lit/keymap.json (or stow your own).
    Override one binding (e.g., remap "x" to PageBackward).
    Restart linux-lit; confirm the override takes effect.
11. Introduce a syntax error in keymap.json.
    Restart; confirm linux-lit logs a warning and falls back to defaults.
```

### Test counts

- Phase 1 (F4): no new unit tests — verbs are GTK-bound. Build + smoke verifies each sub-phase. Tests stay at 118 / 1 pre-existing fail.
- Phase 2 (F2): ~8 new unit tests for keymap parsing, default population, merge behavior, malformed JSON fallback. Tests rise to ~126 / 1 pre-existing fail.

### Risks

- **Phase 1 borrow-checker traps.** Each verb extraction has to preserve the existing borrow/spawn dance correctly. Spawn closures that previously captured `&Rc<RefCell<AppState>>` and mutated state inside the async block need careful translation. Mitigation: extract one verb at a time; cargo build + smoke after each.
- **Phase 2 enumeration completeness.** ~70 default bindings to enumerate. Easy to miss one. Mitigation: grep `match key_name` and `is_ctrl && key_name ==` to enumerate all sites; spec self-review checks for any binding present today but missing from `default_reader_bindings()`.
- **Phase 2 stow package coordination.** The keymap.json file needs to be created in tty-dotfiles AND deployed via stow before the changed linux-lit will find user overrides. Mitigation: document this clearly in CLAUDE.md so users know to run `stow linux-lit`. Linux-lit falls back to compiled-in defaults if the JSON file is absent — works without the stow package, the package is just a convenience for editing.
- **Action enum drift.** New keys added to `keymap.rs` must also become Action variants. Mitigation: Phase 2's `dispatch_action` is the only consumer of Action; cargo's exhaustiveness check catches missing arms.

---

## Out of scope (deferred)

- **F1 (`OverlayMode` trait — keymap dispatch via polymorphism)** — L-effort; review explicitly says "do as part of a larger keymap refactor." Per-overlay keymaps in JSON depend on F1.
- **F3 (chord state via one-shot modes)** — depends on F1.
- **F5 (picker N/P duplication)** — depends on F1's trait carrying `move_selection`.
- **F6 (Ctrl+p disambiguation)** — depends on F1's mode dispatch.
- **F7 (overlay enter/exit hooks)** — depends on F1's trait carrying `enter`/`exit`.
- **F8 (search-focus gating)** — depends on F1's sub-mode pattern.
- **F10 (settings overlay dedup)** — partially absorbed by F4 (extracting `revert_to_snapshot` + `reset_to_defaults` factors out two of the three duplicate matrices).
- **Touch / mouse / gamepad dispatch** — gamepad-specific; would also benefit from F1; defer to a gamepad-focused review.
