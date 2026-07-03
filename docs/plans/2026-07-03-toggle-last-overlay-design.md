# Toggle-last-overlay bind (reader ⇄ last gloss/journal)

**Date:** 2026-07-03
**Status:** design, awaiting user review

## Problem

`Ctrl+g` (`ToggleGlossOverlay`) and `Ctrl+j` (`ToggleJournalOverlay`) are each
self-contained toggles for one specific overlay. There is no single bind that
flips between the **main reading card** and **whichever** of the gloss / journal
overlays you last had open. To peek your last gloss and flip right back, you
have to remember which overlay it was and press the matching key.

## Goal

One bind that:

- **From an open gloss or journal overlay** → closes it back to the reader.
- **From the reader, with a remembered overlay** → reopens that same overlay
  (gloss for the cursor line, or journal on the cursor's scene), fresh from the
  current cursor — identical to pressing `Ctrl+g` / `Ctrl+j`.
- **From the reader with nothing remembered yet** → no-op with a toast
  ("No overlay to reopen").

## Decisions

- **Bind: `Ctrl+Tab`.** Free in the reader map; reads as "switch back";
  consistent with `Ctrl+g`/`Ctrl+j` being overlay toggles. (User away at design
  time — recommended default; trivially re-bindable in `keymap.json`.)
- **Reopen is fresh from the cursor**, not a restored overlay scroll position.
  This reuses the existing `toggle_overlay` open path verbatim; matches today's
  `Ctrl+g` / `Ctrl+j` behavior. No overlay-internal position stashing.
- **`last_overlay` is recorded at the single close chokepoint**
  (`return_to_reader_mode`), so EVERY close path feeds it — the `Ctrl+g`/`Ctrl+j`
  toggle-close, `Escape`, and the undo-confirm return. Opening the journal with
  `Ctrl+j` and closing it with `Escape` still lets `Ctrl+Tab` reopen the journal.

## Design

### New state (`src/app/mod.rs`)

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LastOverlay { Gloss, Journal }
```

Add to `AppState`: `pub(crate) last_overlay: Option<LastOverlay>` (init `None`).

### Record on close (`return_to_reader_mode`, `src/app/mod.rs:3874`)

Before overwriting `input_mode` with `Reader`, capture the mode being left:

```rust
pub(crate) fn return_to_reader_mode(state: &mut AppState) {
    match state.input_mode {
        InputMode::GlossOverlay => state.last_overlay = Some(LastOverlay::Gloss),
        InputMode::JournalOverlay => state.last_overlay = Some(LastOverlay::Journal),
        _ => {}
    }
    state.input_mode = InputMode::Reader;
    apply_reader_gloss_highlighting(state);
}
```

`SynopsisOverlay` is its own mode (renders *through* the gloss overlay widget but
carries `InputMode::SynopsisOverlay`), so the `GlossOverlay` arm does not
mis-record a synopsis close as a gloss. Confirm this during implementation by
checking `is_showing_synopsis()` is never true while `input_mode ==
GlossOverlay` at a close; if it can be, gate the Gloss arm on
`!gloss_overlay.is_showing_synopsis()`.

### New action + dispatcher

- `src/input/actions/mod.rs` — add `ToggleLastOverlay` to `Action`.
- New fn `toggle_last_overlay(state)` (in `gloss.rs`, alongside the other
  toggles, or `journal.rs` — either; it references both):

```rust
pub(crate) fn toggle_last_overlay(state: &Rc<RefCell<AppState>>) {
    let mode = state.borrow().input_mode;
    match mode {
        InputMode::GlossOverlay => gloss::toggle_overlay(state),   // closes it
        InputMode::JournalOverlay => journal::toggle_overlay(state), // closes it
        InputMode::Reader => match state.borrow().last_overlay {
            Some(LastOverlay::Gloss) => gloss::toggle_overlay(state),   // opens it
            Some(LastOverlay::Journal) => journal::toggle_overlay(state), // opens it
            None => show_tts_toast(state, "No overlay to reopen"),
        },
        _ => {} // some other overlay is up; ignore
    }
}
```

Both `toggle_overlay` fns already do open-if-closed / close-if-open, so calling
them from `Reader` opens and from the matching overlay closes — no new open/close
logic.

- `src/input/keymap.rs` dispatch arm:
  `ToggleLastOverlay => crate::input::actions::gloss::toggle_last_overlay(state),`

### Bind (both files — JSON overrides compiled defaults)

- `src/input/keymap_config.rs`: `(KeyCombo::ctrl("Tab"), Action::ToggleLastOverlay),`
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`:
  `{"key": "Tab", "ctrl": true, "action": "ToggleLastOverlay"}`

(Deploy the stow package after; the running app reads `keymap.json` at launch.)

### Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs`)

Tab lives on `MOD_SEQ_ROW` / a modifier row — add the `Ctrl+Tab` →
`ToggleLastOverlay` cap + a `describe()` arm ("Reopen / close last overlay
(gloss or journal) — src/input/actions/gloss.rs"). Use the
`update-cairo-keybinds-overlay` skill so no blank slot / wrong label slips in.
The three per-overlay legends (gloss/synopsis/journal) do **not** need this — the
bind is a reader-mode bind, handled via `keymap_config` dispatch, not in those
overlays' modal handlers.

## Files touched

- `src/app/mod.rs` — `LastOverlay` enum, `AppState.last_overlay` field, record in
  `return_to_reader_mode`
- `src/input/actions/mod.rs` — `ToggleLastOverlay` variant
- `src/input/actions/gloss.rs` — `toggle_last_overlay` dispatcher
- `src/input/keymap.rs` — dispatch arm
- `src/input/keymap_config.rs` + stowed `keymap.json` — the `Ctrl+Tab` bind
- `src/ui/keybinds_overlay.rs` — Ctrl+/ overlay cap + describe arm

## Out of scope / YAGNI

- Restoring overlay scroll position on reopen.
- Cycling through more than the single last overlay (no history stack).
- A visible indicator of what `Ctrl+Tab` would reopen.
- Any change to synopsis / translation / echoes overlays.

## Testing

- `cargo build` + `cargo clippy`.
- No pure-logic unit test fits (the behavior is GTK-mode routing). Verify by
  headless launch (`e2e-env.sh` or manual cage): open gloss with `Ctrl+g`,
  `Ctrl+Tab` → back to reader, `Ctrl+Tab` → gloss reopens; repeat with `Ctrl+j`;
  open with `Ctrl+j`, close with `Escape`, `Ctrl+Tab` → journal reopens;
  `Ctrl+Tab` from a fresh reader with nothing opened → toast, no crash.
