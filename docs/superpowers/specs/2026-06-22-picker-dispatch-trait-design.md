# Picker dispatch trait + `picker_for_mode` accessor — design

## Goal

Collapse the hand-written `InputMode → picker field` dispatch that is duplicated
across the picker nav (`j`/`k`) and Escape-hide paths, by introducing a small
`Picker` trait and one accessor `picker_for_mode(&AppState, InputMode) ->
Option<&dyn Picker>`. The mapping `(mode → field)` then lives in exactly ONE
place instead of being copied into every dispatch match.

This is the previously-PARKED larger-project item from the audit ledger (it was
deliberately NOT given a safe-scope `#N` because it touches control flow and the
borrow model). It ships as its own spec → plan → refactor → merge with a
**mandatory headless verification gate** — it is bigger and riskier than the
behavior-preserving tail extractions #9–#14.

## Why this is safe to do now (the parked concern, resolved)

The reason it was parked: a full picker-trait collapse risked unifying
`move_selection`'s per-picker empty-start/clamp variants, which audit #6
**deliberately preserved**. On inspection that concern does not apply to THIS
scope: the variant logic lives *inside* each `move_selection` body, not in the
dispatch arms. Every nav dispatch arm is the uniform
`state.borrow().<picker>.move_selection(±1)`. So routing through a trait method
leaves each picker's own index math untouched — `#6`'s preserved variants are not
disturbed.

## Scope (decided)

The trait covers **`move_selection` + `hide`** only. It collapses:

- the two `MoveDown` / `MoveUp` match blocks in `keymap.rs` (10 arms each), and
- the **7 plain** Escape `hide(); input_mode = Reader` arms.

It does NOT cover Confirm dispatch, picker open/`show()` pairs, or the special
Escape arms (see Exclusions). This is the middle of the three scope options
considered; the "full trait + open" option was declined as too high-blast-radius.

## Component

A new module `src/input/picker_dispatch.rs`:

```rust
use crate::app::{AppState, InputMode};

/// The uniform slice of picker behavior that the keymap dispatches by InputMode.
/// Per-picker index math stays inside each `move_selection` impl (the variants
/// audit #6 preserved); this trait only routes to it.
pub trait Picker {
    fn move_selection(&self, delta: i32);
    fn hide(&self);
}

// One trivial forwarding impl per dispatched picker, e.g.:
impl Picker for crate::ui::bookmark_picker::BookmarkPicker {
    fn move_selection(&self, delta: i32) { self.move_selection(delta); }
    fn hide(&self) { self.hide(); }
}
// … repeated for each picker in the accessor below.

/// The single source of truth for "which picker is active in this mode".
pub(crate) fn picker_for_mode(s: &AppState, mode: InputMode) -> Option<&dyn Picker> {
    match mode {
        InputMode::BookmarkPicker => Some(&s.bookmark_picker),
        InputMode::MediaPicker => Some(&s.media_picker),
        InputMode::ConcordancePicker => Some(&s.concordance_picker),
        InputMode::ConcordanceWordPicker => Some(&s.concordance_word_picker),
        InputMode::ConcordanceListPicker => Some(&s.concordance_list_picker),
        InputMode::ConcordanceWorksPicker => Some(&s.concordance_works_picker),
        InputMode::GlossPicker => Some(&s.gloss_picker),
        InputMode::AuthorshipPicker => Some(&s.authorship_picker),
        InputMode::JournalPicker => Some(&s.journal_picker),
        InputMode::EchoLinePicker => Some(&s.echo_line_picker),
        _ => None,
    }
}
```

The trait is defined in our crate, so `impl Picker for ui::<X>` satisfies the
orphan rule. Forwarding impls call the existing inherent methods — no behavior
moves.

Trait membership: every picker with `move_selection(&self, i32)` + `hide(&self)`
(all 14) MAY get a one-line `impl Picker`; the accessor maps only the modes that
are actually dispatched today. `voice_picker` / `echo_picker` / `echo_turns_picker`
are dispatched by their own dedicated keybind handlers (not the `MoveDown`/
`MoveUp` match), so they need no accessor arm now — adding their impls is optional
and harmless (do it only if it costs nothing).

## Call-site changes

### Nav (`keymap.rs` `MoveDown` / `MoveUp`)

The two ~10-arm match blocks become:

```rust
PickerAction::MoveDown => {
    let s = state.borrow();
    if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
        p.move_selection(1);
    }
    true
}
PickerAction::MoveUp => {
    let s = state.borrow();
    if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
        p.move_selection(-1);
    }
    true
}
```

Unmapped modes hit `_ => None` and no-op — exactly what the old `_ => {}` arm did.

### Escape-hide (`keymap.rs`)

The 7 plain `→ Reader` arms collapse into a fallback, leaving the 3 special arms
explicit:

```rust
let mut s = state.borrow_mut();
match mode {
    InputMode::GlossPicker => {
        s.gloss_picker.hide();
        if s.gloss_picker_from_overlay {
            s.gloss_picker_from_overlay = false;
            s.input_mode = InputMode::GlossOverlay;
        } else {
            s.input_mode = InputMode::Reader;
        }
    }
    InputMode::JournalPicker => { s.journal_picker.hide(); s.input_mode = InputMode::JournalOverlay; }
    InputMode::EchoLinePicker => { drop(s); crate::input::actions::echoes::cancel_add_echo(state); }
    _ => {
        if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
            p.hide();
            s.input_mode = InputMode::Reader;
        }
    }
}
```

**Borrow nuance:** `picker_for_mode(&s, …)` takes `&s` while the arm then assigns
`s.input_mode` (needs `&mut s`). `p.hide()` ends its use of the immutable borrow
before the assignment, so NLL should accept this. If the borrow checker objects,
the fallback form is: capture the hide into a bool / call `p.hide()` in an inner
scope that ends before the assignment, e.g.

```rust
_ => {
    let hidden = picker_for_mode(&s, mode).map(|p| p.hide()).is_some();
    if hidden { s.input_mode = InputMode::Reader; }
}
```

The plan verifies which compiles; either is behavior-identical.

## Explicitly EXCLUDED (stay as-is)

- **GlossPicker Escape** — `gloss_picker_from_overlay` toggle returning to
  `GlossOverlay` vs `Reader`; conditional return-mode, not the plain pattern.
- **JournalPicker Escape** — returns to `JournalOverlay`, not `Reader`.
- **EchoLinePicker Escape** — `drop(s); cancel_add_echo(state)`; different teardown.
- **Confirm dispatch** — each picker's Return does bespoke selection→handler work.
- **`show()` / open pairs** — the borrow-juggle open paths in `pickers.rs` /
  `concordance.rs`. Out of scope (the declined "full trait" option).
- **`settings_overlay` / `action_popup`** — `rem_euclid` over a `Vec`, no
  `move_selection(&self, i32)`; the #6 exclusion still holds.

## Verification (mandatory headless gate)

Unlike #9–#14, build + unit tests are NOT sufficient here: the change is dispatch
*wiring*, and whether `j`/`k` still move the right picker and `Escape` still
closes it is runtime behavior with no pure unit test.

- `cargo build` + `cargo test --bins` (413) — necessary, not sufficient.
- **Headless cage e2e — REQUIRED:** launch the reader, open a picker (e.g.
  `Ctrl+p` library or a concordance picker), send `j`/`k` and confirm the
  selection moves, send `Escape` and confirm it closes back to the reader. If the
  agent's cage is seat-blocked (live dwl owns the seat), **do NOT claim verified —
  ask the user to run the e2e** and paste the result/screenshot.
- **Grep gate:** no `InputMode::X => state.borrow().<x>_picker.move_selection`
  arm remains outside the accessor; the 3 special Escape arms still present;
  `picker_for_mode` is the only `(mode → picker)` match.

## Out of scope / future

If this lands cleanly, a follow-on could extend the same accessor to the Confirm
and open paths (the declined "full trait" scope) — but only with its own spec,
since those carry the borrow-juggle and bespoke-confirm risk this design avoids.
