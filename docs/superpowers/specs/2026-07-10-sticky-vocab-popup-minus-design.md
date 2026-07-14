# Sticky vocab popup on `-`, hide on `Ctrl+-` — design

Date: 2026-07-10
Status: approved (brainstormed with user; see decisions inline)

## Problem

`z` (VocabPopupNext) cycles vocab words in the segment, but the popup
auto-hides after 3 seconds (fade armed in `handle_vocab_popup_key`,
`src/input/keymap.rs:3497-3526`). The user wants the cycle on `-` with NO
auto-hide, and an explicit dismiss key.

## Decision summary (user-approved)

- Plain `-` → `VocabPopupNext`, replacing `TogglePause` (pause remains on
  `a` and Space).
- `Ctrl+-` → new `Action::HideVocabPopup`, replacing `OpenRecentPicker`
  (which becomes UNBOUND; relocation deferred until missed).
- `z` → freed entirely (binding removed, not remapped).
- `#` keeps `VocabPopupPrev`; `H` keeps `ToggleVocabPopup`.
- Auto-hide is removed for the WHOLE popup (both next and prev), not just
  the `-` path — a fade on prev but not next would be incoherent. The popup
  stays up until `Ctrl+-` (or `H` toggle-off).

## Details

### Handler changes

- Delete the 3s `glib::timeout_add_local_once` arming in
  `handle_vocab_popup_key` (keymap.rs:3497-3526).
- KEEP the `fade_gen` counter + `adw::TimedAnimation` (500ms, EaseOutQuad)
  machinery; `HideVocabPopup` reuses it so the explicit dismiss fades
  instead of hard-blinking.
- `HideVocabPopup` is idempotent: no-op when the popup is not visible
  (distinct from `ToggleVocabPopup`, which would re-open it).

### Current bindings displaced

- `keymap_config.rs:282` `(plain("minus"), TogglePause)` → replaced.
- `keymap_config.rs:390` `(ctrl("minus"), OpenRecentPicker)` → replaced;
  OpenRecentPicker left unbound.
- `keymap_config.rs:228` `(plain("z"), VocabPopupNext)` → removed.

### The four keybind surfaces (all in one change)

1. `src/input/keymap_config.rs` compiled defaults (above).
2. Stow source `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` —
   its `minus`/`ctrl+minus`/`z` lines (live file lines 67-68, 126) silently
   OVERRIDE compiled changes if not updated in the same change.
3. Ctrl+/ keybinds overlay: KeyDefs + `describe()` arms for `-`, `Ctrl+-`,
   and the removed `z` — run the update-cairo-keybinds-overlay skill
   (three-pass cross-reference).
4. RPD layout check for the `minus` key name (already a known-good GTK name
   in the current map; `Ctrl+-` chord is unaffected by RPD symbol shifts).

### New action

`Action::HideVocabPopup` in `src/input/actions/mod.rs` (+ name string +
vocab-popup action-class arm alongside Toggle/Next/Prev at mod.rs:251-253).

## Testing

- `cargo test --bins`: keymap tests referencing `minus` (e.g. the
  modifier-distinguishing lookup test) updated for the new actions.
- Headless verify: `-` pressed repeatedly → popup visible, cycling, still
  visible after >3s (no fade); `Ctrl+-` → 500ms fade-out; `z` → no ACTION
  logged; `-` during audio playback → no TogglePause in the log.

## Out of scope

- Relocating OpenRecentPicker to a new key.
- Any change to `#` (prev) or `H` (toggle) bindings.
- Popup content/layout changes.
