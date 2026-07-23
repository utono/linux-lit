# `Ctrl+$` root-variant cycling in all overlays and pickers

## Problem

`Ctrl+$` cycles the reader's root/wallpaper color variant (`RootVariantNext`;
`Ctrl+Shift+$` / `Ctrl+Alt+$` → `RootVariantPrev`). It is a READER-ONLY bind
(`keymap_config.rs:385-389`), dispatched to
`settings::cycle_root_variant(state, forward)`. Inside any overlay or picker
(gloss, journal, synopsis, echoes, translation, all the pickers, the keybinds
overlays, chat, action popup, …) the chord is dead — the reader must close the
overlay to recolor the background, then reopen it.

## Goal

Make `Ctrl+$` (next) / `Ctrl+Shift+$` and `Ctrl+Alt+$` (prev) cycle the root
variant in EVERY non-reader mode — all overlays and all pickers — mirroring the
reader bind, so the background can be recolored without leaving the current
view.

## Design — one global guard, not ~30 handler edits

`handle_key` (`src/input/keymap.rs`) dispatches non-reader input through a
SINGLE chokepoint (~line 261):

```rust
let mode = state.borrow().input_mode;
if mode != crate::app::InputMode::Reader {
    return match mode { /* ~30 per-mode handlers */ };
}
```

Add a global guard IMMEDIATELY BEFORE that `if mode != Reader` match (i.e.
after `let mode = ...`), so it runs for every non-reader mode in one place:

```rust
// Global: Ctrl+$ cycles the root/wallpaper variant in EVERY overlay and
// picker, mirroring the reader bind (keymap_config.rs: RootVariantNext/Prev on
// the RPD <TLDE> `$` cap). `$` is level-1 on RPD, so all three chords deliver
// key_name "dollar" with the ctrl flag; shift/alt selects the direction.
// Ctrl+$ → next; Ctrl+Shift+$ / Ctrl+Alt+$ → prev. The modal vim editors
// (JournalEdit/GlossEdit/SegmentVim/AddVocab) never reach here — they return at
// the top of handle_key — so this cannot fire while typing in a vim editor.
if mode != crate::app::InputMode::Reader
    && is_ctrl
    && key_name == "dollar"
{
    let forward = !(is_shift || is_alt);
    crate::input::actions::settings::cycle_root_variant(state, forward);
    return true;
}
```

(Placement: after `let mode = state.borrow().input_mode;` and BEFORE the
`if mode != Reader { return match … }`. The `mode != Reader` clause in the guard
is belt-and-suspenders — Reader already handles `Ctrl+$` via the compiled table
earlier in `handle_key`, so this guard only matters for non-reader modes.)

### Why a global guard (not editing each handler)

The reader bind lives in the compiled `keymap_config.rs` table, but overlays
and pickers use hand-written per-mode handler functions — there is no table to
add a row to, and there are ~30 handlers. A single guard at the dispatch
chokepoint is the correct, DRY mirror of "the reader bind, everywhere": one
source of truth, no per-handler drift, and it automatically covers any
future overlay mode that flows through the same match.

### Direction mapping (mirror the reader binds exactly)

`keymap_config.rs:385-389`:
- `Ctrl+$` (`ctrl("dollar")`) → `RootVariantNext`
- `Ctrl+Shift+$` (`ctrl_shift("dollar")`) → `RootVariantPrev`
- `Ctrl+Alt+$` (`ctrl_alt("dollar")`) → `RootVariantPrev`

So: `forward = !(is_shift || is_alt)` — plain `Ctrl+$` is next; adding shift OR
alt makes it prev. `cycle_root_variant(state, forward)` is the existing action.

## Scope

- ALL non-reader modes that reach the dispatch chokepoint — overlays AND
  pickers AND the text-entry modes (Search, OverlaySearchInput, CorpusSearch,
  ChatPrompt, JournalTermInput, PageCalibration). Per the user's decision:
  include everywhere, no carve-out. `Ctrl+$` is a control-chord (not a literal
  `$`), so it never eats typed text.
- The modal vim editors (JournalEdit/GlossEdit/SegmentVim/AddVocab) are
  EXCLUDED automatically — they return at the top of `handle_key` before this
  guard.

## Non-goals / untouched

- No `keymap_config.rs` change (overlays are not table-driven; the reader bind
  already exists).
- No overlay Ctrl+/ legend change — root-variant cycling is a global reader
  affordance not currently listed in any overlay legend; keep it that way
  (adding it to ~7 legends is out of scope; revisit on request).
- Confirmed no overlay/picker handler currently binds `dollar` (grep clean), so
  the guard consumes no key another handler wanted.

## Risks

- **Minimal.** One guard calling an existing action. The only real check: it
  fires in a representative overlay and a picker, and does not regress any
  overlay that expected `dollar` (none do — verified).
- `cycle_root_variant` takes `state.borrow_mut()` internally; the guard passes
  `state` (the `&Rc<RefCell<AppState>>`) without holding a borrow across the
  call — the `let mode = state.borrow().input_mode;` temporary drops at its `;`
  before the guard runs. No nested borrow.

## Testing

- **Headless (cage):** open the gloss overlay, press `Ctrl+$`, confirm the
  background root color changes (screenshot before/after — the app bg / wallpaper
  color shifts). Repeat `Ctrl+Shift+$` to confirm it cycles the other direction.
  Open a picker (e.g. the library picker or a concordance picker) and confirm
  `Ctrl+$` cycles there too. Confirm the overlay/picker stays open and otherwise
  unaffected (the chord is consumed, not passed through).
- `cargo test --bins` stays green (no unit-testable pure logic added; the
  `cycle_root_variant` behavior is unchanged).
- `cargo clippy --bin linux-lit` — no new errors.

## Acceptance

- `Ctrl+$` / `Ctrl+Shift+$` / `Ctrl+Alt+$` cycle the root variant from within
  every overlay and picker, in the correct direction, without closing the
  overlay.
- The modal vim editors are unaffected (still type/command normally).
- No other overlay behavior changes; no key another handler wanted is consumed.
