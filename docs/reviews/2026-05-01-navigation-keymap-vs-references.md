# Navigation / Keymap Review vs Reference Codebases

**Date:** 2026-05-01
**Linux-lit files reviewed:** `src/input/keymap.rs` (1586 lines), `src/input/navigation.rs` (3934 lines), `src/input/keymap_config.rs` (404 lines), `src/input/actions/mod.rs` (338 lines), `src/input/picker_keys.rs` (65 lines)
**References consulted:** `bk/src/main.rs` (426 lines), `bk/src/view.rs` (444 lines), `lue/lue/input_handler.py` (223 lines)

## Summary

Since the prior review (2026-04-29), linux-lit has implemented InputMode dispatch, category-grouped bindings, PickerAction factoring, and Action::name logging — closing four of eight findings. The remaining shape gap is that `keymap.rs` still routes overlay keys through per-mode handler functions with hardcoded match arms rather than through the `Keymap`/`Action` system, and `navigation.rs` (3934 lines) still conflates viewport math, cursor verbs, scroll mechanics, highlight rendering, and page-turn animation in one file. After this round's refactors, bk's View-per-mode pattern and lue's category-dispatch translate line-for-line to linux-lit's structure.

## Findings

### F1. Overlay key routing still bypasses the Keymap/Action system [pattern-alignment]

**Reference shape:** `bk/src/view.rs:13-17` — every mode implements `View::on_key`, dispatched from one site (`main.rs:184`). Adding a keybind to Toc means adding one line to `Toc::on_key`. `lue/lue/input_handler.py:154-208` — all keys resolve to command strings through the same shortcut dict lookup, regardless of mode.

**Linux-lit shape:** `keymap.rs:101-121` — `handle_key` correctly dispatches to per-mode handler functions via `InputMode` match. But each handler (`handle_gloss_key` at line 536, `handle_visual_key` at 958, `handle_settings_key` at 455, etc.) contains its own hardcoded match arms. These ~800 lines of per-mode key matching duplicate the pattern that `Keymap::lookup` + `dispatch_action` already solves for Reader mode. Adding a new keybind to any overlay requires editing handler code, not a config table.

**Refactor toward reference:** Extend `Keymap` to hold per-mode binding maps (`gloss`, `visual`, `settings`, `search`, etc.), each mapping `KeyCombo → Action`. Add overlay-specific Action variants (`GlossScrollDown`, `GlossDelete`, `SettingsAdjustLeft`, etc.). Per-mode handlers reduce to `keymap.lookup(mode, key, mods) → dispatch_overlay_action(state, action)`. Mirrors bk's per-View `on_key` while keeping linux-lit's data-driven approach.

**Leverage unlocked:** User-customizable overlay bindings via `keymap.json` come for free. bk's `Toc.on_key(Char('j')) => self.next(bk, 1)` translates to a table entry `KeyCombo::plain("j") → PickerMoveDown`. Adding keybinds to the gloss overlay or visual mode is a config change, not a code change.

**Risk if ignored:** Each new overlay feature (e.g., gloss search, visual block selection) requires editing handler functions rather than adding table entries. The ~800 lines of handler code grow monotonically.

**Effort:** L

---

### F2. `navigation.rs` conflates five concerns in one 3934-line file [pattern-alignment]

**Reference shape:** `bk/src/view.rs:179-317` — Page viewport management is 6 methods averaging 5 lines. Position (jump/mark) in `main.rs:219-243`. Rendering in `view.rs:318-393`. Three clean concerns in separate locations.

**Linux-lit shape:** `navigation.rs:1-3934` — contains: (a) page boundary math (`visible_range`, `trim_*`, `clamp_at_section_break`, `build_page_tops` — ~500 lines), (b) cursor movement verbs (`jump_to_start`, `cursor_next_dialogue`, `jump_to_next_chapter` — ~500 lines), (c) scroll mechanics (`set_page`, `set_page_instant`, `snap_scroll_to_line`, page-turn animation — ~400 lines), (d) highlight/dim rendering (`update_highlight` — ~150 lines), (e) word-copy/concordance helpers (~200 lines), (f) tests (~600 lines). These have no cross-dependencies that prevent splitting.

**Refactor toward reference:** Extract into: `viewport.rs` (VisibleRange, trim_*, page_tops — pure functions, unit-testable without GTK), `cursor.rs` (jump/nav verbs — take `&mut AppState`, call viewport and scroll), `scroll.rs` (set_page, animation, snap_scroll, bottom_clip — GTK scroll plumbing), `highlight.rs` (dim/cursor-line tag management). Maps to bk's split: viewport.rs ↔ bk's render logic, cursor.rs ↔ bk's on_key handlers, scroll.rs ↔ bk's terminal redraw.

**Leverage unlocked:** Pagination edge-case fixes (the most frequent bug class — 5 of the last 20 commits touch this file) land in `viewport.rs` without touching cursor or animation. foliate-js `paginator.js` reads map to `viewport.rs`; bk's `on_key` maps to `cursor.rs`.

**Risk if ignored:** Merge conflicts between pagination fixes and cursor features. Grep results for a viewport bug also surface animation code, concordance helpers, and test harnesses.

**Effort:** L

---

### F3. `dispatch_action` embeds side-effects beyond verb calls [pattern-alignment]

**Reference shape:** `bk/src/view.rs:267-312` — each key arm is one function call or one assignment: `self.scroll_down(bk, bk.rows)`, `bk.quit = true`. No pre/post branching.

**Linux-lit shape:** `keymap.rs:1042-1060` — `OpenLibraryPicker` arm contains 10 lines of precondition checking (is picker/bookmark/media/settings visible?), concordance teardown, and gloss hide before setting the mode. `keymap.rs:1086-1100` — `OpenSettingsOverlay` reads 6 config fields, calls show with them, then sets mode. `keymap.rs:1159-1215` — `JumpToNextVocab`/`JumpToPrevVocab` contain 15-line inline concordance advance logic with nested borrows. These should be verb-level concerns, not dispatch-level.

**Refactor toward reference:** Move precondition/setup logic into the verb functions: `actions::pickers::open_library_picker` handles visibility checks and concordance teardown; `actions::concordance::jump_to_next_vocab` handles the advance-within-work vs plain-jump branching. `dispatch_action` becomes a pure routing table like bk's `on_key` — one call per arm, no branching.

**Leverage unlocked:** Dispatch table is scannable at a glance. Verbs are self-contained — callable from tests, gamepad, or a future command palette without duplicating the setup logic. bk's `on_key` translates line-for-line.

**Risk if ignored:** Calling verbs from other contexts (gamepad, tests, scripting) requires duplicating the branching that wraps them.

**Effort:** S

---

### F4. Gloss key handler contains 250+ lines of inline UI/DB logic [pattern-alignment]

**Reference shape:** `bk/src/view.rs:22-47` — Mark and Jump views are 25 lines each. Key handlers call methods on `bk`, never construct widgets or run I/O inline.

**Linux-lit shape:** `keymap.rs:536-615` — `handle_gloss_key` dispatches j/k/g/G/d/a/n/c/Escape (reasonable). But the functions it calls are defined inline in the same file: `navigate_gloss_passage` (lines 617-688, 70 lines of DB queries + widget updates), `navigate_gloss` (690-710), `show_delete_confirmation` (723-790, builds GTK dialog inline), `delete_current_gloss` (792-820, DB delete + UI refresh), `show_amend_dialog` (1456-1513, builds another GTK dialog), `add_gloss` (1516-1586, Claude API call + DB save). Total: ~400 lines of gloss business logic living in `keymap.rs`.

**Refactor toward reference:** Move gloss navigation, delete, amend, and add functions to a `gloss.rs` action module (alongside `actions/bookmarks.rs`, `actions/pickers.rs`). `handle_gloss_key` calls into that module. Mirrors bk's pattern where view.rs calls methods on `bk`, and the methods live where their domain logic belongs.

**Leverage unlocked:** `keymap.rs` becomes pure key routing. Gloss logic is testable in isolation. Future gloss features (export, search, batch operations) land in the gloss module, not in the keymap file.

**Risk if ignored:** `keymap.rs` grows by ~50 lines for each new gloss feature. The file is already 1586 lines; the gloss block alone is 400 of those.

**Effort:** M

---

### F5. `handle_key` has pre-dispatch interceptions that shadow the Keymap [bug-suspect]

**Reference shape:** `bk/src/view.rs:267-312` — Page.on_key is a flat match with no pre-match interception. Each key has exactly one handler. `lue/lue/input_handler.py:160-208` — linear if-elif chain, each key matches exactly once.

**Linux-lit shape:** `keymap.rs:31-44` — `Ctrl+n/p` picker navigation fires before mode dispatch (line 102) when `picker_visible`. Lines 46-60: `Ctrl+Shift+P` opens concordance word picker. Lines 63-72: `Ctrl+Alt+p` opens concordance list picker. Lines 74-91: `Ctrl+p` opens library picker. Lines 151-165: vocab popup intercepts `g` and `Tab` before the keymap lookup. These 130 lines of pre-dispatch interceptions run before `state.borrow().keymap.lookup(…)` at line 225, creating implicit priority that's hard to reason about. If a pre-dispatch block matches, the keymap never sees the key.

**Refactor toward reference:** Absorb all pre-dispatch interceptions into the per-mode dispatch. `Ctrl+n/p` picker navigation belongs in `handle_picker_key` (it's already handled there via `PickerAction`). Vocab popup `g`/`Tab` should be a sub-mode or handled after the keymap lookup with the popup-visible check moved into the verb. `Ctrl+p`/`Ctrl+Shift+P`/`Ctrl+Alt+p` are already in the keymap defaults — the pre-dispatch blocks are redundant with them.

**Leverage unlocked:** Key priority is explicit and flat (one dispatch per mode, like bk's `on_key`). No hidden shadowing between pre-dispatch blocks and the keymap. Easier to debug "why didn't my keybind fire" — one code path per mode.

**Risk if ignored:** Adding a new keybind that conflicts with a pre-dispatch interception silently fails. The `Ctrl+p` pre-dispatch block at line 74 duplicates logic that the `OpenLibraryPicker` Action already handles — they can diverge.

**Effort:** M

---

### F6. Reader mode's `pending_g`/`pending_z` chords are hand-rolled state machines [pattern-alignment]

**Reference shape:** `bk/src/view.rs:22-47` — bk handles `m` and `'` by switching to a transient view (Mark, Jump). The view's `on_key` receives the *next* key and completes the chord. Each transient view is 10 lines. No timeouts, no global state flags.

**Linux-lit shape:** `keymap.rs:126-149` — `pending_g` and `pending_z` are boolean flags on `KeyState`, checked at the top of Reader mode's handler. `dispatch_action` arms at lines 1328-1343 set these flags with 500ms timeouts that auto-clear via `glib::timeout_add_local_once`. The same pattern repeats in `handle_gloss_key` (line 543) and `handle_visual_key` (line 977) and `handle_keybinds_key` (line 888, `pending_ctrl_slash`). Four separate places manage chord state with the same boolean + timeout pattern.

**Refactor toward reference:** Generalize to a chord-entry mechanism: an enum `ChordState { None, PendingG, PendingZ, PendingCtrlSlash }` on `KeyState` with a single timeout. When a chord-initiating key fires, set the state and start the timer. On the next key, resolve and clear. This mirrors bk's transient-view pattern without changing linux-lit's timer-based approach. One implementation, used by all four sites.

**Leverage unlocked:** Adding new chords (e.g., `gc` for gloss copy, `zb` for scroll-cursor-bottom) is adding an enum variant and a match arm, not duplicating the boolean + timeout + 3-site check pattern. bk's Mark/Jump views translate directly to chord enum variants.

**Risk if ignored:** Each new chord duplicates the boolean + timeout + handler-check pattern. The `pending_ctrl_slash` in `handle_keybinds_key` is already a copy-paste of the `pending_g` pattern.

**Effort:** S

---

### F7. `after_page_change` reason has dead variant flag `should_update_label` [bug-suspect]

**Reference shape:** `bk/src/view.rs` — no dead methods on view state; each field drives a visible behavior.

**Linux-lit shape:** `navigation.rs:1209-1212` — `PageChangeReason::should_update_label()` is defined and tested (lines 3605-3609) but no longer called from `after_page_change`. It was used by the page_line_label overlay that was removed in commit `8e29a1e` (the commit just before this review). The method and its test survive as dead code.

**Refactor toward reference:** Remove `should_update_label` from `PageChangeReason` and its tests. bk's principle: state fields drive visible behavior; dead methods are removed.

**Leverage unlocked:** `PageChangeReason` accurately reflects what it controls. Future readers don't wonder what label it refers to.

**Risk if ignored:** Minor — dead code misleads future maintainers into thinking a label update path exists.

**Effort:** S

## Out of scope

- **Pagination algorithm shape** (visible_range, trim chain, section-break clamping) — covered in the pagination-vs-references reviews. This review focuses on keymap/navigation module structure.
- **Audio seek suppression state machine** (`suppress_sync_until` in navigation.rs) — overlaps with audio-sync subsystem; belongs in a separate review against lue/openreader.
- **Visual mode selection** (`handle_visual_key`, `visual.rs`) — linux-lit-specific feature with no direct reference analog. The chord-state finding (F6) touches it peripherally.
- **Page-turn animation** (crossfade, slide in `set_page`) — no reference analog (bk is terminal, lue is terminal). The extraction to `scroll.rs` (F2) would move it, but the animation itself isn't a reference-alignment question.
- **Test extraction** from navigation.rs — 600 lines of tests should move with their code when F2 splits the file, but test organization isn't a reference-alignment finding.

## Suggested next step

Implement F7 (dead `should_update_label` — trivial cleanup, 5 minutes), then F6 (chord-state generalization — S-effort, eliminates 4 copy-paste sites), then F3 (clean dispatch_action — S-effort, makes dispatch scannable). These three are independent and can be done in any order. F4 (gloss extraction) is M-effort and reduces `keymap.rs` by 25%. F1 and F2 are the large structural refactors that depend on F3/F4 being clean first.
