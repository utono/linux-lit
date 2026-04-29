# Navigation / Keymap Review vs Reference Codebases

**Date:** 2026-04-29
**Linux-lit files reviewed:** `src/input/keymap.rs` (1270 lines), `src/input/navigation.rs` (4033 lines), `src/input/keymap_config.rs` (371 lines), `src/input/actions/mod.rs` (113 lines)
**References consulted:** `bk/src/main.rs` (426 lines), `bk/src/view.rs` (444 lines), `lue/lue/input_handler.py` (223 lines)

## Summary

Linux-lit's keymap pipeline (KeyCombo → Action → dispatch_action → verb) already mirrors bk's trait-based View dispatch in spirit, but the two systems diverge in how overlay/mode routing is expressed: bk delegates to discrete View objects with their own `on_key`, while linux-lit inlines all overlay routing as a 600-line if/else cascade in `handle_key`. The headline alignment win is extracting overlay-local keymaps out of `handle_key` into the `Keymap`/`Action` system so every input path runs through one dispatch mechanism — after which bk's View pattern and lue's category-grouped shortcuts translate line-for-line to linux-lit.

## Findings

### F1. Overlay key routing lives outside the Action dispatch [pattern-alignment]

**Reference shape:** `bk/src/view.rs:13-17` — each mode (Page, Toc, Search, Mark, Jump) implements the `View` trait with its own `on_key(bk, kc)`. The main loop (`main.rs:184`) calls `self.view.on_key(self, e.code)` — one dispatch site, mode-polymorphic.

**Linux-lit shape:** `keymap.rs:28-752` — `handle_key` is a single 750-line function with ~12 sequential `if picker_visible` / `if settings_visible` / `if search_visible` blocks, each containing its own hardcoded match arms. Only the final `state.borrow().keymap.lookup(…)` at line 747 goes through the Action enum. Overlay keys bypass `Keymap` entirely.

**Refactor toward reference:** Extend `Keymap` to hold per-mode maps (reader, picker, bookmark_picker, settings, search, visual, etc.) mirroring bk's per-View `on_key`. `handle_key` becomes: (1) determine active mode, (2) `keymap.lookup(mode, key, modifiers)` → `Action`, (3) `dispatch_action(state, action)`. The ~600 lines of inline overlay routing collapse into `default_*_bindings()` tables in `keymap_config.rs` and per-mode match arms in `dispatch_action`.

**Leverage unlocked:** bk's mode-dispatch pattern translates mechanically to linux-lit: bk's `Toc.on_key` corresponds to linux-lit's `Keymap::picker` table. Future overlay keybinds (e.g., adding j/k to any new picker) land in config tables, not in the middle of a 750-line function. User-customizable overlay bindings via `keymap.json` come for free.

**Risk if ignored:** Every new overlay or picker requires splicing another 20-40 line block into the middle of `handle_key`, increasing merge conflicts and making it harder to reason about key shadowing between modes.

**Effort:** L

---

### F2. No mode enum — active mode inferred from widget visibility [pattern-alignment]

**Reference shape:** `bk/src/main.rs:104` — `view: &'a dyn View` is the single source of truth for current mode. Mode transitions are explicit assignments (`bk.view = &Toc`, `bk.view = &Page`).

**Linux-lit shape:** `keymap.rs:28-44,88,156,229,325,393,415,430,443,474,511,541,588,629,673` — active mode is derived from a cascade of `state.borrow().X.is_visible()` checks. The order of these checks is load-bearing: if two overlays are visible simultaneously (a bug, but possible), the first match wins silently.

**Refactor toward reference:** Add an `InputMode` enum to `AppState` (`Reader`, `Picker`, `BookmarkPicker`, `MediaPicker`, `Settings`, `Search`, `Visual`, `GlossOverlay`, `GamepadOverlay`, `KeybindsOverlay`, `ConcordancePicker`, `ConcordanceWordPicker`, `ConcordanceListPicker`, `ActionPopup`). Set it on show/hide. `handle_key` switches on `state.input_mode` instead of probing widgets. Mirrors bk's `view` field.

**Leverage unlocked:** Mode identity is O(1) instead of O(overlays). Future cross-reference with bk's View dispatch is direct — bk's `bk.view = &Toc` maps to `state.input_mode = InputMode::Picker`. Eliminates the class of bugs where two overlays compete for key routing.

**Risk if ignored:** Subtle priority bugs when overlays overlap; each new overlay requires careful insertion at the right position in the cascade.

**Effort:** M

---

### F3. Keymap categories not grouped by domain [pattern-alignment]

**Reference shape:** `lue/lue/input_handler.py:9-37` — shortcuts are grouped into named categories: `navigation`, `tts_controls`, `display_controls`, `application`. The dispatch at lines 154-208 matches by category first, then by action within category.

**Linux-lit shape:** `keymap_config.rs:173-284` — `default_reader_bindings()` is a flat `HashMap<KeyCombo, Action>` with comment headers but no structural grouping. The JSON schema (`keymap.json`) also has a flat `reader` array.

**Refactor toward reference:** Add a `category` field to `keymap.json` bindings (optional, for documentation/validation) and group `default_reader_bindings()` into sub-functions (`nav_bindings()`, `media_bindings()`, `vocab_bindings()`, etc.) that each return a `Vec<(KeyCombo, Action)>`. The flat HashMap merge stays the same, but the source is structured. Mirrors lue's category dict.

**Leverage unlocked:** When reading lue's `nav_shortcuts.get("prev_paragraph")`, the linux-lit equivalent is mechanically `nav_bindings()` → `JumpToPrevChapter`. Keybind documentation, the Ctrl+/ overlay, and conflict detection can all iterate by category instead of the full flat map.

**Risk if ignored:** Flat map grows monotonically with each new action; cross-referencing lue's categorized shortcuts requires mental mapping.

**Effort:** S

---

### F4. `navigation.rs` conflates viewport, cursor, and page-index concerns [pattern-alignment]

**Reference shape:** `bk/src/view.rs:179-317` (Page) — viewport management is 6 methods (`scroll_down`, `scroll_up`, `next_chapter`, `prev_chapter`, `click`, `start_search`) averaging 5 lines each. `bk/src/main.rs:219-243` — position management (jump, mark) is separate from rendering.

**Linux-lit shape:** `navigation.rs` (4033 lines) — one file holds: page boundary computation (`visible_range`, `trim_*`, `clamp_at_section_break`), cursor movement (`cursor_next_dialogue`, `jump_to_prev_chapter`), highlight management (`update_highlight`), page-turn animation (`set_page`, `capture_page_snapshot`), scroll helpers (`snap_scroll_to_line`, `center_cursor`), audio seek (`seek_to_current_line`), vocab navigation, concordance navigation, word-copy clipboard, and 500+ lines of tests.

**Refactor toward reference:** Split `navigation.rs` into modules mirroring bk's separation: `viewport.rs` (visible_range, trim_*, bottom_clip, page_tops index — pure page-boundary math), `cursor.rs` (cursor_next_dialogue, jump_to_*, bookmark nav — the verbs), `scroll.rs` (set_page, set_page_instant, snap_scroll_to_line, page-turn animation), `highlight.rs` (update_highlight, dim tag management). Each module corresponds to one bk concern: viewport.rs ↔ bk's `view.rs` render logic, cursor.rs ↔ bk's `on_key` scroll_up/scroll_down, scroll.rs ↔ bk's terminal clear-and-redraw.

**Leverage unlocked:** Pagination-edge-case fixes (the most frequent bug class in recent commits) land in `viewport.rs` without touching cursor or animation code. Cross-referencing foliate-js `paginator.js` maps to `viewport.rs`; cross-referencing bk's `on_key` maps to `cursor.rs`.

**Risk if ignored:** The 4000-line file grows with each new navigation feature; merge conflicts between pagination fixes and cursor features are common.

**Effort:** L

---

### F5. Inline DB queries in overlay key handlers [bug-suspect]

**Reference shape:** `bk/src/view.rs:267-312` (Page.on_key) — key handlers mutate `bk` state only (chapter, line, view, mark). No I/O in the key handler; persistence happens at exit (`main.rs:415-424`).

**Linux-lit shape:** `keymap.rs:280-316` — the media picker's `"p"` handler opens a read-write database connection (`open_db_rw`), runs a SQL update (`set_media_priority`), queries the result, and updates UI — all synchronously inside a key handler on the GTK main thread.

**Refactor toward reference:** Move the DB write to an async task dispatched via `tokio_handle.spawn_blocking`, matching the pattern used by `toggle_bookmark` (which correctly delegates to `actions::bookmarks`). The key handler sends a message or calls an action verb; the verb handles I/O off-thread. Aligns with bk's principle that key handlers only mutate in-memory state.

**Leverage unlocked:** Key handlers become pure state transitions (bk's pattern). Future DB operations in key handlers follow the same async verb pattern instead of ad-hoc inline queries.

**Risk if ignored:** Synchronous `open_db_rw` + SQL on the GTK main thread blocks the UI. On a slow disk or locked database, the app freezes on a keypress. This is the only remaining inline DB call in a key handler — all other DB operations correctly go through action verbs.

**Effort:** S

---

### F6. Picker key handling duplicated across 6 picker types [pattern-alignment]

**Reference shape:** `bk/src/view.rs:109-177` (Toc) — one `View` impl with `prev`, `next`, `cursor`, `click`. The Toc is the only list-mode; all list navigation is in one place.

**Linux-lit shape:** `keymap.rs:88-586` — six picker/list overlays (library picker, bookmark picker, media picker, concordance picker, concordance word picker, concordance list picker) each have near-identical blocks: Escape→hide, Return→confirm, Down/j→move(1), Up/k→move(-1), Ctrl+n→move(1), Ctrl+p→move(-1). The pattern repeats ~6 times with minor variations (bookmark picker adds `Delete/d`, media picker adds `p` for priority).

**Refactor toward reference:** Define a `PickerActions` enum (`Hide`, `Confirm`, `MoveDown`, `MoveUp`, `Delete`, `SetPriority`) and a shared lookup function that maps key+modifier to `PickerActions`. Each picker's block reduces to: (1) lookup → `PickerAction`, (2) match on the action with picker-specific confirm/delete logic. Mirrors bk's single Toc impl — one navigation vocabulary for all list modes.

**Leverage unlocked:** Adding a new picker (e.g., theme picker, timestamp picker) requires only the confirm/delete specialization, not another 30-line key block. bk's Toc.on_key reads as the template for all linux-lit pickers.

**Risk if ignored:** Each new picker copies another 30-40 lines of boilerplate. Inconsistencies creep in (e.g., concordance list picker uses `j/n` and `k/p` for movement while others use `j` and `k`).

**Effort:** M

---

### F7. `dispatch_action` side-effects beyond verb calls [pattern-alignment]

**Reference shape:** `bk/src/view.rs:267-312` — each key arm is one function call or one assignment. `scroll_down(bk, bk.rows)`, `bk.quit = true`, `bk.view = &Toc`. No pre/post logic around the dispatch.

**Linux-lit shape:** `keymap.rs:779-814` — `JumpToNextChapter` / `JumpToPrevChapter` / `JumpToNextScene` / `JumpToPrevScene` arms contain inline logic (check work_type, toggle translations) before calling the navigation verb. Lines 1046-1051: `SetStartTime` calls the verb, then conditionally calls `cursor_next_dialogue`. These are action-level behaviors leaked into the dispatch table.

**Refactor toward reference:** Move the pre/post logic into the verb functions themselves. `navigation::jump_to_next_chapter` should accept `&mut AppState` and handle the translation toggle internally. `timestamps::set_start_time` should advance the cursor on success. `dispatch_action` becomes a pure routing table matching bk's `on_key` — one call per arm, no branching.

**Leverage unlocked:** `dispatch_action` becomes scannable at a glance (bk's on_key is). Verb functions are self-contained — calling them from tests, from gamepad input, or from a future scripting API doesn't require duplicating the pre/post logic.

**Risk if ignored:** Side-effects in dispatch make it impossible to call verbs from other contexts (gamepad, tests, macros) without duplicating the branching logic that wraps them.

**Effort:** S

---

### F8. No lue-style semantic command names in the dispatch path [pattern-alignment]

**Reference shape:** `lue/lue/input_handler.py:165-208` — key handler resolves to a string command name (`'prev_paragraph'`, `'next_sentence'`, `'pause'`), then posts it to the event loop via `reader._post_command_sync(cmd)`. The command name is the API.

**Linux-lit shape:** `keymap_config.rs:120-143` — `Keymap::lookup` returns `Option<Action>`, which `dispatch_action` matches directly. The `Action` enum serves as the command vocabulary, but it's a Rust enum, not a string — no runtime introspection, no logging of "which command ran", no command-line / scripting interface.

**Refactor toward reference:** Add `Action::name(&self) -> &'static str` (derived from serde's Serialize, or a manual match returning the variant name as a string). Log the resolved action name in `dispatch_action` alongside the key name already logged in `handle_key`. This mirrors lue's `cmd` variable — a human-readable command name at the dispatch boundary.

**Leverage unlocked:** Debug logs show "dispatched JumpToNextChapter" instead of requiring the reader to trace from key name through keymap lookup to the match arm. Future command palette, scripting, or macro-record features have a string command vocabulary ready. lue's command-name dispatch translates directly.

**Risk if ignored:** Debugging keybind issues requires mental tracing through the lookup → dispatch chain; no grep-able command names in logs.

**Effort:** S

## Out of scope

- **Pagination algorithm differences** (foliate-js paginator.js) — belongs in a separate pagination-vs-references review; this review covers keymap/navigation module structure, not page-boundary math.
- **Audio seek / sync state machine** (lue timing_calculator) — the seek logic in `navigation.rs` overlaps with audio-sync, which has its own reference pairing (lue, openreader). Noted but deferred.
- **Test coverage for overlay key handlers** — the 500+ lines of page-turn tests in navigation.rs are excellent; overlay key routing has no tests. This is a testing gap, not a reference-alignment finding.
- **Gamepad input** (`gamepad.rs`) — separate input channel, not covered by bk or lue references.
- **Word-copy clipboard helpers** (`word_cycle_copy`, `word_collect_copy`) — no reference analog; these are linux-lit-specific features.

## Suggested next step

Implement F3 (category-grouped bindings) and F8 (action name logging) first — both are S-effort, immediately useful for debugging, and lay groundwork for F1 (per-mode keymaps). Then tackle F7 (clean dispatch_action) before attempting F1/F2/F6 which are structural changes.
