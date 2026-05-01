# Navigation / Keymap Post-Refactor Review vs Reference Codebases

**Date:** 2026-05-01
**Linux-lit files reviewed:** `src/input/keymap.rs` (1070 lines), `src/input/navigation.rs` (1811 lines), `src/input/viewport.rs` (1286 lines), `src/input/scroll.rs` (680 lines), `src/input/highlight.rs` (280 lines), `src/input/keymap_config.rs` (404 lines), `src/input/actions/mod.rs` (339 lines), `src/input/picker_keys.rs` (65 lines)
**References consulted:** `bk/src/main.rs` (426 lines), `bk/src/view.rs` (444 lines), `lue/lue/input_handler.py` (223 lines)

## Summary

Since the prior review (same day), all seven findings have been implemented: navigation.rs split into viewport/scroll/highlight, chord state consolidated into a ChordState enum, dispatch_action cleaned to one-call-per-arm, gloss logic extracted to actions/gloss.rs, and pre-dispatch interceptions absorbed into the keymap. The module structure now closely mirrors bk's separation of concerns — viewport.rs maps to bk's `Page::render` layout logic, navigation.rs cursor verbs map to bk's `Page::on_key` scroll methods, and scroll.rs maps to bk's terminal redraw mechanics. The remaining gaps are: overlay modes still use hardcoded handlers instead of the Keymap/Action system, navigation.rs still conflates pure cursor verbs with GTK-coupled concordance/word-copy helpers, and the Escape multi-state handler in keymap.rs bypasses the Action dispatch.

## Findings

### F1. Overlay key routing still bypasses the Keymap/Action system [pattern-alignment]

**Reference shape:** `bk/src/view.rs:13-17` — every mode implements `View::on_key`, dispatched from one site. The Toc view (line 143-165) uses the same `KeyCode` matching pattern as Page (line 267-312) — each view owns its bindings but shares the dispatch mechanism. `lue/lue/input_handler.py:154-208` — all keys resolve through the same `_matches_shortcut` lookup.

**Linux-lit shape:** `keymap.rs:59-76` — `handle_key` dispatches to per-mode functions correctly. But `handle_gloss_key` (lines 483-561), `handle_visual_key` (688-727), `handle_settings_key` (402-456), `handle_keybinds_key` (607-641) each contain hardcoded match arms. Reader mode's keys go through `Keymap::lookup` → `dispatch_action`; overlay keys go through per-function match arms. Two dispatch mechanisms coexist.

**Refactor toward reference:** Add per-mode binding maps to `Keymap` (e.g., `gloss: HashMap<KeyCombo, OverlayAction>`, `visual: HashMap<KeyCombo, OverlayAction>`). Overlay-specific Action variants (GlossScrollDown, SettingsAdjustLeft, etc.) route through a parallel `dispatch_overlay_action`. Handler functions become lookup + dispatch, not match blocks.

**Leverage unlocked:** User-customizable overlay bindings via `keymap.json` without code changes. bk's `Toc::on_key` translates to a config entry rather than requiring mental mapping to a hardcoded match arm.

**Risk if ignored:** Every new overlay keybind requires editing a handler function. The ~500 lines of overlay handler code grow linearly with features.

**Effort:** L

---

### F2. navigation.rs still mixes pure cursor verbs with GTK-coupled helpers [pattern-alignment]

**Reference shape:** `bk/src/view.rs:180-207` — Page's `scroll_down`, `scroll_up`, `next_chapter`, `prev_chapter` are pure position mutations (set `bk.line`, `bk.chapter`). They don't call clipboard commands, spawn processes, or query databases.

**Linux-lit shape:** `navigation.rs:1-1811` — the file now cleanly owns cursor verbs (jump_to_start, page_forward, jump_to_next_chapter, etc.) after the viewport/scroll/highlight extraction. But it also contains: (a) `concordance_jump_to_current` (lines 881-940) which spawns child processes and sends MPV commands, (b) `concordance_position_cursor` / `concordance_resolve_indices` / `concordance_seek` / `concordance_update_bar` / `find_sentence_start_by_timestamp` (lines 942-1035) — 94 lines of concordance DB + MPV logic, (c) `word_cycle_copy` / `word_collect_copy` / `extract_buffer_line_words` / `apply_word_underline` (lines 1044-1205) — 162 lines of clipboard + GTK tag logic, (d) 556 lines of tests (1211-1811).

**Refactor toward reference:** Move concordance navigation (lines 881-1035) to `actions/concordance.rs` — it already has `handle_word_selection` and `open_picker`, and concordance jumping is an action, not a cursor verb. Move word_cycle/collect (lines 1044-1205) to `actions/selection.rs` or a new `word_copy.rs` — they involve clipboard I/O and GTK tags, not cursor positioning. This leaves navigation.rs as ~750 lines of pure cursor verbs + `after_page_change` + `seek_to_current_line`, matching bk's scope.

**Leverage unlocked:** Navigation.rs becomes purely about cursor position mutations, matching bk's `view.rs` methods. Concordance logic is colocated in `actions/concordance.rs` for cross-reference with openreader. Clipboard/selection logic is self-contained for future changes (e.g., Wayland primary selection).

**Risk if ignored:** Navigation.rs remains 1811 lines. Concordance bugs require reading cursor verb context; word-copy changes touch the same file as page-turn fixes.

**Effort:** M

---

### F3. Escape handler is a 33-line multi-state inline block that bypasses dispatch [bug-suspect]

**Reference shape:** `bk/src/view.rs:269` — `Esc | Char('q') => bk.quit = true`. One line, one action, no state cascade. `lue/lue/input_handler.py:160-161` — quit is a single shortcut match.

**Linux-lit shape:** `keymap.rs:128-161` — the Escape handler checks concordance state, then AB loop, then search matches, executing different cleanup logic for each. This block runs before `keymap.lookup()` at line 165, meaning Escape can never be remapped in keymap.json. The three branches (concordance clear, AB loop clear, search clear) are independent state-machine exits with unrelated teardown logic jammed into one 33-line if/else cascade.

**Refactor toward reference:** Extract three Action variants: `ClearConcordance`, `ClearAbLoop`, `ClearSearch`. Map Escape to a composite `EscapeReaderMode` Action whose verb checks precedence (concordance → AB → search → noop). Or: keep the precedence chain but move it into an `actions/escape.rs` verb, so `dispatch_action(EscapeReaderMode)` calls the verb. Either way, Escape goes through the keymap like every other key.

**Leverage unlocked:** Escape becomes remappable via keymap.json. The teardown logic for each state machine is testable independently. bk's flat dispatch translates without a "but Escape is special" exception.

**Risk if ignored:** Escape is the only reader-mode key that bypasses the Keymap system. Its inline AB-loop teardown (8 lines of state mutation + gutter redraw) is the kind of logic that accumulates bugs when modified alongside the AB-loop state machine elsewhere.

**Effort:** S

---

### F4. `handle_key` still has inline Shift+Tab / Ctrl+g gloss toggle before keymap lookup [pattern-alignment]

**Reference shape:** `bk/src/view.rs:267-312` — Page.on_key is a flat match, no pre-match interceptions.

**Linux-lit shape:** `keymap.rs:108-124` — 17 lines checking for `ISO_Left_Tab` or `Ctrl+g` to toggle the gloss overlay. This runs in Reader mode before the keymap lookup at line 165. The logic reads gloss state, constructs the overlay, and sets input mode inline.

**Refactor toward reference:** Add `ToggleGlossOverlay` Action variant. Map `ISO_Left_Tab` and `Ctrl+g` to it in `keymap_config.rs`. Move the 17-line toggle logic to `actions/gloss.rs::toggle_overlay(state)`. The pre-dispatch block disappears; these keys route through keymap like everything else.

**Leverage unlocked:** Gloss overlay toggle is remappable. `keymap.rs:38-170` (the Reader-mode pre-dispatch section) reduces to just chord resolution and keymap lookup — matching bk's flat on_key.

**Risk if ignored:** Adding more gloss keybinds (e.g., Ctrl+Shift+G for a different gloss view) tempts copy-pasting the pre-dispatch pattern instead of using the keymap.

**Effort:** S

---

### F5. `ChordState::PendingG` completion lives inline in `handle_key`, not in dispatch [pattern-alignment]

**Reference shape:** `bk/src/view.rs:22-47` — Jump's `on_key` receives the second key and completes the chord by calling `bk.jump(pos)`. The completion logic is in the transient view, not the main key handler.

**Linux-lit shape:** `keymap.rs:82-105` — when `chord == PendingG`, the second key is resolved inline: `g` → `jump_to_start` or `extend_to_start`, `semicolon` → `jump_to_recent_bookmark`. Similarly for `PendingZ` at lines 99-105 (`t` → `scroll_cursor_top`). These completions bypass the keymap entirely — the second key of a chord is never looked up in the binding table.

**Refactor toward reference:** When `PendingG` is active, construct a synthetic combo (e.g., `KeyCombo { key: "g", chord_prefix: "g" }`) or use a separate chord completion map in Keymap. Chord completions become data: `gg → JumpToStart`, `g; → JumpToRecentBookmark`, `zt → ScrollCursorTop`. New chord completions are config entries, not code edits.

**Leverage unlocked:** Adding `gc` (gloss copy), `zb` (scroll cursor bottom), or `gp` (jump to prev chapter) is a keymap.json entry. bk's Mark/Jump transient views translate to chord-completion map entries.

**Risk if ignored:** Each new chord completion adds an if-branch to the top of `handle_key`, expanding the pre-dispatch section that the prior review's refactors shrunk.

**Effort:** M

---

### F6. Picker handlers repeat the same Hide/Show/Mode pattern across 6 modes [pattern-alignment]

**Reference shape:** `bk/src/view.rs:131-177` — Toc is a single View impl. Enter = switch to Page, Escape = switch to Page. Mode transitions are one-liners because each View is self-contained.

**Linux-lit shape:** `keymap.rs:265-399` — `handle_picker_key` handles 5 input modes (BookmarkPicker, MediaPicker, ConcordancePicker, ConcordanceWordPicker, ConcordanceListPicker) in one function with per-mode match blocks for Hide (lines 277-285), Confirm (lines 288-352), MoveDown (lines 355-366), MoveUp (lines 367-376), and Unhandled (lines 377-399). Each block is a 5-way match on the mode enum, delegating to the appropriate picker widget. The `Confirm` block alone is 64 lines with 5 branches.

**Refactor toward reference:** Define a `Picker` trait with `hide()`, `confirm()`, `move_selection(delta)`, `selected_value()`. Each picker type (BookmarkPicker, MediaPicker, etc.) implements it. `handle_picker_key` becomes: `resolve_picker_key(key) → match { Hide => picker.hide(), Confirm => picker.confirm(), Move => picker.move_selection(delta) }` — no per-mode branching. Mirrors bk's trait-dispatch where `bk.view.on_key(bk, kc)` dispatches without knowing which View is active.

**Leverage unlocked:** Adding a 6th picker (e.g., ThemePicker) means implementing the Picker trait, not editing 5 match blocks. bk's View trait pattern translates line-for-line. The per-mode Confirm logic moves into each picker's `confirm()` method.

**Risk if ignored:** Each new picker adds one arm to each of the 5 match blocks (Hide, Confirm, MoveDown, MoveUp, Unhandled) — 5 sites to edit per picker.

**Effort:** M

---

### F7. `dispatch_action` returns `true` unconditionally for most arms [pattern-alignment]

**Reference shape:** `bk/src/view.rs:267-312` — `on_key` returns nothing (the View trait is `fn on_key(&self, bk: &mut Bk, kc: KeyCode)`). There is no consumed/not-consumed signal because bk controls the event loop — unmatched keys fall through to `_ => ()`.

**Linux-lit shape:** `keymap.rs:733-977` — `dispatch_action` returns `bool` (true = consumed). Every arm returns `true`. The two exceptions are `SearchNextMatch` and `SearchPrevMatch` (lines 961-976) which return `false` when no search matches exist — but this is dead logic since these actions are only dispatched when the keymap matches, and the user's intent was clearly to invoke the action. The return value is consumed by `handle_key` which passes it to GTK's event propagation.

**Refactor toward reference:** Change `dispatch_action` to return `()` and have `handle_key` return `true` unconditionally when the keymap matched. SearchNext/PrevMatch should always return true (consume the key) and show a "no matches" feedback rather than passing the key to GTK. This matches bk's pattern where recognized keys never propagate.

**Leverage unlocked:** Verb authors don't need to remember to return `true`. The `bool` return type on 60+ match arms is pure noise. bk's `on_key` → `()` maps directly.

**Risk if ignored:** Minor — a future verb author returning `false` accidentally would cause the key to propagate to GTK, potentially triggering unexpected behavior (e.g., inserting text in the buffer).

**Effort:** S

## Out of scope

- **Overlay keybind data-driven configuration** — F1 describes the direction but implementation design (how to represent overlay Actions, backward compatibility with existing keymap.json) needs a separate brainstorm.
- **Page-turn animation** in scroll.rs — no reference analog (both bk and lue are terminal apps). Structural extraction is done; animation tuning is a UI concern.
- **Gamepad overlay** (`handle_gamepad_key`, `gamepad.rs`) — linux-lit-specific, no reference analog.
- **Timestamps module** (`timestamps.rs`) — a domain distinct from navigation/keymap; belongs in an audio-sync review against lue/openreader.
- **Search module** (`search.rs`) — the Search input mode handler (`handle_search_key`) is 20 lines and already clean. Its architecture mirrors bk's `Search` view (view.rs:396-444) closely enough that no refactor is needed.

## Suggested next step

Implement F3 (Escape handler extraction — S-effort, eliminates the last pre-dispatch bypass) and F4 (gloss toggle to Action — S-effort), which together make the Reader-mode path in `handle_key` a clean chord-resolution + keymap-lookup with no exceptions. Then F7 (dispatch returns void — S-effort, trivial cleanup). F2 (concordance/word-copy extraction from navigation.rs — M-effort) and F5 (chord completion map — M-effort) are the next structural improvements. F1 and F6 are the larger refactors that benefit from F3-F5 being clean first.
