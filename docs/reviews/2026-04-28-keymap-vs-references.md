# Navigation/Keymap Review vs Reference Codebases

**Date:** 2026-04-28
**Linux-lit files reviewed:** `src/input/keymap.rs` (1911 lines, full read), `src/input/navigation.rs` (3872 lines, targeted: cursor/page entry points)
**References consulted:** `~/Documents/repos/linux-lit/bk/src/main.rs` (426 lines, full read), `~/Documents/repos/linux-lit/bk/src/view.rs` (444 lines, full read), `~/Documents/repos/linux-lit/lue/lue/input_handler.py` (223 lines, full read)

## Summary

Linux-lit's keymap is one ~1750-line `handle_key` function that walks ~13 overlay-visibility checks via if/else before reaching base keys, with every overlay's keys, every state mutation, and every async dispatch inlined at its match arm. bk solves the same dispatch problem with a `View` trait (8 mode structs, ~5 methods each) and a single `bk.view.on_key(bk, kc)` call at the event loop. lue separates the keymap *table* (a JSON-loadable dict) from the *dispatcher* (a single match-string-to-command function), so re-binding doesn't touch dispatch code. The headline alignment win: extract per-overlay `Mode` structs implementing a small trait (bk's pattern) AND lift the keybind table out of the dispatch fn (lue's pattern) so future overlay additions touch one struct, future re-binds touch one config file, and `keymap.rs` shrinks to dispatch + trait impls.

## Findings

### F1. Layered if/else dispatch on overlay visibility; bk uses `View` trait [pattern-alignment]

**Reference shape:** `bk/src/view.rs:13-18` — `pub trait View { fn render; fn on_key; fn on_mouse; fn on_resize; }`. `bk/src/main.rs:184` dispatches `self.view.on_key(self, e.code)`. Mode swap is `bk.view = &Toc`. 8 mode structs (Page, Toc, Mark, Jump, Metadata, Help, Search, plus reader). Each struct's `impl View` is contiguous and small (~30 LOC).

**Linux-lit shape:** `src/input/keymap.rs:160-1742` — single 1583-line `handle_key`. ~13 sequential overlay-visibility checks (`picker_visible`, `bookmark_picker_visible`, `media_picker_visible`, `settings_visible`, `search_visible`, `gloss_visible`, `gamepad_visible`, `keybinds_visible`, `concordance_picker.is_visible`, `conc_word_picker_visible`, `conc_list_picker_visible`, `action_popup_visible`, `in_visual`). Each check inlines its keys, state mutations, and async dispatches. A new overlay grows the chain.

**Refactor toward reference:** Define `trait OverlayMode { fn name(&self) -> &str; fn handle_key(&self, state, key, key_state, tokio_handle) -> bool; fn on_resize(&self, state) {} }`. One struct per overlay (`LibraryPickerMode`, `BookmarkPickerMode`, `SettingsMode`, ..., plus `ReaderMode` for the no-overlay case). Replace the if/else chain with `pick_active_mode(&state).handle_key(...)`. Each mode's keymap lives next to its struct definition, not inside a 1583-line function.

**Leverage unlocked:** Future bk reads translate directly. New overlay = new struct, no edit to dispatch fn. Per-overlay unit tests become tractable (instantiate `LibraryPickerMode`, call `handle_key` with synthetic state). `on_resize` becomes the uniform hook the F6 (closed-without-action in pagination review) tick callback could call per-overlay rather than reader-only.

**Risk if ignored:** `keymap.rs` keeps growing per overlay; isolated overlay testing stays infeasible.

**Effort:** L (per pagination review F10; do as larger keymap refactor)

---

### F2. Keybind table embedded in dispatch; lue lifts it to JSON [pattern-alignment]

**Reference shape:** `lue/lue/input_handler.py:9-37` — `DEFAULT_KEYBOARD_SHORTCUTS` is a nested dict (`navigation`, `tts_controls`, `display_controls`, `application`). `process_input` walks the dict and dispatches via `_matches_shortcut(data, nav_shortcuts.get("next_paragraph", "k"))`. `load_keyboard_shortcuts(file_path)` swaps the table at runtime. Re-binding needs zero dispatch edits.

**Linux-lit shape:** `src/input/keymap.rs:1339-1741` (single-keys block) — every key string is hard-coded inside a match arm next to its handler call. To remap "x" from `page_forward` to `page_backward`, you grep the file, edit the match arm, recompile.

**Refactor toward reference:** Define `enum Action { PageForward, PageBackward, NextDialogue, ... }` listing every reader action. Add `pub struct Keymap { bindings: HashMap<KeyCombo, Action> }` loaded from `~/.config/linux-lit/keymap.json` with a default. Dispatch becomes: lookup `(key_name, ctrl, shift, alt)` in the map → get `Action` → dispatch via a small `match action { ... }` table. Per-overlay tables (the F1 trait carries one each).

**Leverage unlocked:** Users re-bind without source edits. Action-to-handler matrix is one place to read for "what does the reader do?". lue reads translate directly. The keybinds-overlay (`Ctrl+/`) currently rendered from a static markdown could be regenerated from the keymap dict — single source of truth.

**Risk if ignored:** Re-binding requires source edits + recompile. The keybinds-overlay drifts from reality (no automated check the displayed keys match the dispatch).

**Effort:** M

---

### F3. `key_state.pending_g` / `pending_ctrl_slash` handled inline; bk uses one-shot mode [pattern-alignment]

**Reference shape:** `bk/src/view.rs:21-32, 34-47` — `Mark` and `Jump` are full View structs that consume the next key, then `bk.view = &Page` to return. The "pending" state IS the active mode. `Page::on_key` line 276 sets `bk.view = &Mark` on `m`, line 277 on `'` sets `bk.view = &Jump`. No separate `pending_*` flags.

**Linux-lit shape:** `src/input/keymap.rs:11-15, 1100-1148, 1369-1374, 1218-1222` — `KeyState { pending_g: bool, pending_ctrl_slash: bool }` with manual `glib::timeout_add_local_once` 500ms timers to clear. Each chord adds two state fields and a timeout. `g` checks `pending_g` at line 1101 to dispatch `gg` or `g;`. New chords (e.g., `mx` set-mark) require new fields.

**Refactor toward reference:** Replace `KeyState` flags with one-shot modes in the F1 trait. `PendingGMode` consumes the next key (`g` → jump_to_start, `;` → most-recent-bookmark, anything else → cancel and forward to ReaderMode). `PendingCtrlSlashMode` consumes `g` to swap keybinds→gamepad. The 500ms auto-clear becomes the mode's `on_timeout` (or just a glib timeout that clears `state.active_mode`).

**Leverage unlocked:** Adding vim-style `mx` (set mark x) / `'x` (jump to mark x) becomes "add `MarkSetMode` and `MarkJumpMode` structs" not "add 4 new bool fields and 4 timer closures." bk's mark/jump pattern translates directly. Cancellation logic centralizes (any unhandled key in a one-shot mode falls through to ReaderMode).

**Risk if ignored:** Each new chord adds state fields and timer closures; cancellation logic re-implemented per chord.

**Effort:** M (depends on F1)

---

### F4. State mutation inlined in keymap arms; bk's View routes through Bk methods [pattern-alignment]

**Reference shape:** `bk/src/view.rs:267-313` — `Page::on_key` is a 47-line match. Each arm calls a method on `bk` (e.g., `self.scroll_down(bk, bk.rows / 2)`, `bk.search(args)`, `bk.mark('\'')`). The mode is dispatch only; the verbs live on `Bk` (the state).

**Linux-lit shape:** `src/input/keymap.rs:1567-1631` (m bookmark toggle, 64 lines), `:702-815` (settings overlay handlers, 113 lines), `:441-500` (media picker Return, 63 lines including async lambda) — long inlined async closures, db calls, cmd_tx sends, vec manipulation. A keymap arm IS the implementation, not just a dispatch entry.

**Refactor toward reference:** Move every >5-line keymap arm into a verb on `AppState` or a dedicated `crate::input::actions::*` module. Examples: `state.toggle_bookmark_at_cursor(tokio_handle)` (replaces lines 1567-1631), `state.media_picker.confirm_selection(tokio_handle)` (replaces lines 441-500). Keymap arms become one-line dispatch like bk: `Char('m') => state.toggle_bookmark_at_cursor(tokio_handle)`.

**Leverage unlocked:** Verbs become directly testable (no key event needed). `keymap.rs` becomes readable in one sitting. Future audio-sync / bookmark / media reviews don't have to grep through keymap to find what `m` does — they read the verb directly.

**Risk if ignored:** keymap.rs accretes business logic; same logic gets re-implemented when called from other entry points (gamepad, future API).

**Effort:** M (independent of F1)

---

### F5. Picker N/P navigation duplicated 5×; bk centralizes scroll on the trait [pattern-alignment]

**Reference shape:** `bk/src/view.rs:143-165` (Toc) and `:267-313` (Page) — both have `Down | Char('j')` / `Up | Char('k')` arms calling `self.next(bk, n)` / `self.prev(bk, n)`. Same key shape, mode-specific scroll quantum (1 for Toc cursor, 3 for Page line-step, `bk.rows/2` for half-page).

**Linux-lit shape:** `src/input/keymap.rs` — 5 separate `Ctrl+n / Ctrl+p` blocks (lines 173-185 picker, 299-311 bookmark_picker, 418-430 media_picker, 901-908 concordance_picker, 952-959 concordance_word_picker, 1000-1007 concordance_list_picker, 1015-1037 action_popup) plus `j/k` arms inside each visible-overlay block (lines 395-408, 504-516, 988-998). Same predicate logic duplicated ~6×.

**Refactor toward reference:** The F1 `OverlayMode` trait carries `fn move_selection(&self, state, delta: i32)`. The dispatcher handles `Ctrl+N`/`Ctrl+P`/`Down`/`Up`/`j`/`k` → calls `mode.move_selection(state, ±1)`. Per-mode override is just the implementation.

**Leverage unlocked:** Six blocks of duplicate logic collapse to one dispatcher arm. Adding a new picker = implement `move_selection`, get `Ctrl+N`/`j`/etc. for free. Same shape as bk's `Toc::next` + `Toc::prev` pair shared across views.

**Risk if ignored:** Bug in one picker's `move_selection` (e.g., wrap-around vs clamp) won't sync across the others; per-picker rework required when changing scroll behavior.

**Effort:** S (depends on F1; collapses 6 sites)

---

### F6. Two `Ctrl+p` semantics ad-hoc disambiguated; bk's mode lifts the question [pattern-alignment]

**Reference shape:** `bk/src/view.rs:267-313` — `Page::on_key` doesn't have to ask "which mode am I in?" It IS the Page mode. `Toc::on_key` handles its own keys. Mode IS the disambiguator.

**Linux-lit shape:** `src/input/keymap.rs:212-225` — `Ctrl+p` is overloaded: open library picker IF `!picker_visible && !bookmark_picker.is_visible() && !media_picker.is_visible()`, ELSE move selection up. The disambiguation is a 4-condition `&&` check at the top of dispatch. `Ctrl+n` is similar (lines 173-185 for picker, 299-311 for bookmark, etc).

**Refactor toward reference:** Under F1, `Ctrl+p` in `LibraryPickerMode` means "move selection up". In `ReaderMode` it means "open library picker". Same key, different action per mode — exactly what mode dispatch is for. The 4-condition guard goes away.

**Leverage unlocked:** Adding a third overlay-with-N/P doesn't add another `&& !x.is_visible()` to the open-picker guard. Behavior of every key under every mode is enumerable by reading per-mode keymaps.

**Risk if ignored:** Each new picker overlay adds another `!x.is_visible()` condition to `Ctrl+p`'s open guard. Forget one (as commit `efc8adf` documents was a real bug) and pickers conflict.

**Effort:** S (subset of F1 payoff)

---

### F7. No render abstraction; bk's `View::render` lets Help/Metadata be transient pages [pattern-alignment]

**Reference shape:** `bk/src/view.rs:74-107` — `Help::render` returns its content as `Vec<String>`; `Help::on_key` returns to Page on any key. Same for `Metadata::render` (lines 49-72). The mode owns its render output.

**Linux-lit shape:** Overlays are GTK widgets pre-built at startup; `keybinds_overlay`, `gamepad_overlay`, `correction_overlay`, `settings_overlay` are concrete struct fields on AppState shown/hidden by widget visibility (e.g., `s.keybinds_overlay.show()` at line 1215). No render dispatch; the keymap.rs flow is "show widget, mode keys route to it."

**Refactor toward reference:** Don't translate this directly — linux-lit's GTK substrate makes pre-built widgets the right call. **However**, the *trait shape* still maps: F1's `OverlayMode` carries `fn enter(&self, state)` (replaces `s.keybinds_overlay.show()`) and `fn exit(&self, state)` (`hide()`). `KeybindsMode::enter` becomes the canonical place to coordinate "hide other overlays before showing self" (currently inlined at lines 1209-1214 with 5 explicit `s.X.hide()` calls).

**Leverage unlocked:** "Hide siblings before showing self" centralizes — adding a new overlay to "siblings" is one place to edit, not a grep across every other overlay's open path.

**Risk if ignored:** Each new overlay has to remember to hide every other overlay in its open handler; discovered via runtime conflicts.

**Effort:** S (slot into F1; worth listing because it surfaces a real coupling bug class)

---

### F8. `is_search_focused` re-checked at every key arm; bk's Search mode owns it [pattern-alignment]

**Reference shape:** `bk/src/view.rs:395-427` — `Search` mode's `on_key` handles every key (including `Char(c)` for typed input). Other modes don't have to ask "is the search bar focused?" because if Search is the active mode, focus is implicit.

**Linux-lit shape:** `src/input/keymap.rs:344, 396, 403, 504, 511, 519` — bookmark and media picker arms repeatedly check `state.borrow().bookmark_picker.search_entry().has_focus()` to decide whether `j`/`k`/`d`/`p` mean "navigate list" or "type into search box." The check is re-done at every arm.

**Refactor toward reference:** Under F1, when the search entry has focus inside an overlay, that's a sub-mode (`BookmarkPickerSearchMode`). The picker's `j`/`k` only fire in `BookmarkPickerListMode`. The mode swap on focus change is one event, not a per-key check.

**Leverage unlocked:** Removes ~6 duplicate `has_focus()` calls. Adding a new picker key doesn't have to remember to gate on search focus.

**Risk if ignored:** Easy to forget the focus gate when adding a new key (e.g., the recent `d` for delete-bookmark at line 343 has the gate; future additions might not). Drift between pickers' focus rules.

**Effort:** S (subset of F1 payoff)

---

### F9. Async closures inlined at dispatch sites; bk keeps Bk methods sync [pattern-alignment + bug-suspect]

**Reference shape:** `bk/src/main.rs:182-204` — event loop is sync: `event::read()? -> view.on_key(...) -> render(self)`. Heavy work (search across chapters, file IO) is sync methods on `Bk`. No async, no closure capture, no spawn.

**Linux-lit shape:** Multiple keymap arms spawn `glib::spawn_future_local(async move { ... })` blocks 30-80 lines long: lines 32-71 (`load_selected_work`), 444-501 (media picker Return), 355-391 (bookmark delete), 660-678 (Ctrl+m bookmark picker open), 1119-1147 (g; jump-to-recent-bookmark), 1233-1252 (Ctrl+\ concordance picker open), 1607-1629 (m bookmark toggle). Each captures `state_clone`, `tokio_handle`, builds DB params, awaits, mutates state inside the async block.

**Refactor toward reference:** linux-lit needs async (the DB calls would block the GTK main loop), so don't kill it — extract. Move each spawn to a verb in `crate::input::actions::*` (e.g., `actions::load_work(state, abbrev, tokio_handle)`, `actions::open_bookmark_picker(state, tokio_handle)`). Keymap arm becomes one line: `actions::load_work(state, abbrev, tokio_handle)`. The capture pattern is encapsulated in one place per action.

**Leverage unlocked:** Same async work used from gamepad input or future REST API doesn't reimplement the spawn dance. Easier to verify correctness — one place per action handles "drop borrows before spawn", "borrow_mut after await", etc. The `efc8adf` "prevent Ctrl+p from opening library picker when bookmark/media picker is visible" class of bug becomes structurally hard once the action knows its preconditions.

**Risk if ignored:** Same async spawn pattern reimplemented per call site; borrow-after-spawn bugs (state.borrow_mut held across .await would deadlock on next borrow) easy to introduce. The `set_text triggers connect_changed which borrows state, so the mutable borrow must be dropped first` comment at line 1248-1249 documents one such trap.

**Effort:** M (independent of F1)

---

### F10. `apply_settings_change` is the only verb-style helper; expand the pattern [pattern-alignment]

**Reference shape:** `bk/src/main.rs:111-275` — `Bk` has 8 small methods (`new`, `run`, `jump`, `jump_byte`, `jump_reset`, `mark`, `pad`, `search`). Modes call them. The verbs are named, contiguous, and each ≤30 lines.

**Linux-lit shape:** `src/input/keymap.rs:1782-1831` — `apply_settings_change` is a 50-line helper that takes a `SettingsChange` enum and routes to mutations. The settings overlay's `Escape` handler (lines 702-741, 40 lines of revert logic) and `r` handler (lines 777-812, 36 lines of reset logic) re-implement similar dispatch inline. Three nearly-identical mutation matrices.

**Refactor toward reference:** Generalize: `apply_settings_change(state, SettingsChange::*)` becomes the only mutator. Escape calls it with `SettingsChange::Snapshot(snapshot_values)`. `r` calls it with `SettingsChange::Defaults`. Lines 702-741 and 777-812 each shrink to ~6 lines.

**Leverage unlocked:** One canonical "apply settings" function; future settings additions land in one place. The Escape/Reset/Confirm trio's "do all mutations in this order" invariant is trivially preserved.

**Risk if ignored:** Adding a new setting requires editing 3 places (Escape, r, apply_settings_change) and getting them all consistent. Past commits show this drifting.

**Effort:** S (independent; pure dedup)

---

## Out of scope

- **gg / gx multi-key chords** — covered by F3.
- **Gamepad dispatch** (not yet read in detail) — would also benefit from F1's mode pattern (`GamepadMode` translates buttons to actions); defer to a gamepad-focused review.
- **MPV command sending pattern** (`s.cmd_tx.try_send(...)` scattered) — pagination/audio-sync concern, not keymap; defer.
- **Visual mode** (`src/input/visual.rs` not read end-to-end) — already a sub-module; would slot naturally under F1's `VisualMode`.
- **Help text generation** — bk's `Help::render` returning hard-coded strings doesn't scale; under F2, the keybinds overlay would auto-render from the keymap dict.

## Suggested next step

F2 (keymap-as-data) and F4 (verbs out of dispatch) are the highest-leverage independent refactors and don't require F1's L-effort. Either makes future work mechanical:
- **F2 first** if the immediate pain is "I want to remap keys without recompiling" or "the keybinds overlay drifts from reality."
- **F4 first** if the immediate pain is "keymap.rs is hard to navigate" or "I can't unit-test the action without simulating a key event."

F1 is the structural payoff but L-effort; do it after F2 + F4 land, when the per-mode keymap tables and verb modules already exist and the trait extraction becomes mostly mechanical relocation.
