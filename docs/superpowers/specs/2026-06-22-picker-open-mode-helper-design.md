# Picker open-mode helper + borrow normalization — design

## Goal

Give the "enter picker mode" step of every picker-open path a single named
helper, and normalize the 4 sites that redundantly take two RefCell borrows
(`state.borrow().picker.show(); state.borrow_mut().input_mode = ...`) down to one
`borrow_mut`. **Zero observable behavior change.**

This is the remaining follow-on from the parked picker-dispatch work (the
`Picker` trait + `picker_for_mode` accessor already shipped for the nav and
plain-Escape-hide paths). The **Confirm dispatch is deliberately NOT touched** —
its arms are genuinely bespoke (different `selected_X()` return types, different
post-selection handlers) and abstracting them would add complexity, not remove
it.

## Honest scope note

This is a small, low-payoff refactor. Its value is NOT line-count reduction (the
helper wraps a single assignment). Its value is:
1. a consistent, named idiom for "enter picker mode" across all open paths, and
2. removing a latent double-RefCell-borrow at 4 sites (two acquisitions where one
   suffices).

It is the tail of the picker-dispatch series; after it, the open/Confirm
follow-on is fully resolved (Confirm intentionally left bespoke).

## Why the helper captures ONLY the mode set

The picker-open sites are NOT uniform in their `show()`:

- 5 pickers call a no-arg `show()` (bookmark, media, concordance,
  concordance_word, gloss).
- concordance_list calls `show(&occurrences, current_index)` — takes args.
- concordance_works calls `show(&works)` — takes args.
- library_picker uses a two-call `show_prepare*()` + `show_finish()`.

The ONLY byte-uniform fragment across all in-scope sites is the trailing
`<state>.input_mode = InputMode::XPicker`. So the helper captures exactly that;
each caller keeps its own `show(...)` with whatever arguments it needs.

## Component

A top-level free fn in `src/input/actions/pickers.rs` (most callers live there;
it already imports `AppState` / `InputMode`):

```rust
/// Enter the given picker `mode`. The uniform tail of every picker-open path;
/// the caller does its own `show(...)` first (the show signature varies per
/// picker, so only the mode-set is shared).
pub(crate) fn open_picker_mode(s: &mut AppState, mode: InputMode) {
    s.input_mode = mode;
}
```

## Call-site changes (8 sites, 7 pickers)

### Borrow-normalized (4 sites) — collapse two borrows into one

`pickers.rs:342` bookmark, `:447` media, `:829` concordance_word, `:903` gloss.
Pattern before:
```rust
state_clone.borrow().bookmark_picker.show();
state_clone.borrow_mut().input_mode = crate::app::InputMode::BookmarkPicker;
```
After:
```rust
let mut s = state_clone.borrow_mut();
s.bookmark_picker.show();
crate::input::actions::pickers::open_picker_mode(&mut s, crate::app::InputMode::BookmarkPicker);
```
(`show()` is `&self`, callable through `&mut s`. For sites already inside a
`pickers.rs` fn, the `crate::input::actions::pickers::` prefix may be dropped to a
bare `open_picker_mode`.)

Note `concordance_word` (829) currently does
`state.borrow_mut().set_words(words); state.borrow().show(); state.borrow_mut().input_mode = ...`
— three borrows. Normalize the `show()` + mode-set tail into one `borrow_mut`; the
preceding `set_words` borrow may stay separate or fold in (either is fine, no
behavior change).

### Mode-set swap only (4 sites) — borrow shape unchanged

- `concordance.rs:315` and `:336`: already inside one `let mut s = state.borrow_mut()`.
  Replace `s.input_mode = InputMode::ConcordancePicker;` with
  `crate::input::actions::pickers::open_picker_mode(&mut s, crate::app::InputMode::ConcordancePicker);`.
- `pickers.rs:840` concordance_list, `:862` concordance_works: keep their
  `show(&args)` and existing `drop(s)` / borrow shape; replace only the trailing
  `state.borrow_mut().input_mode = InputMode::X;` with a `borrow_mut` + helper
  call (or, where a guard is already held, the in-scope form).

## Behavior-preservation requirement

At the 4 normalized sites, merging `show()` and the mode-set under ONE
`borrow_mut` holds the borrow across both statements. This is safe ONLY if
nothing between them re-enters `state`. The implementer MUST confirm, at each of
the 4 sites, that there is no intervening `state.borrow*()` / `state_clone.borrow*()`
call between the `show()` and the mode-set before merging. (They are adjacent
statements today, so this should hold — but verify per site.)

## Explicitly EXCLUDED (leave fully as-is)

- **The Confirm dispatch block** in `handle_picker_key` — bespoke per arm.
- **library_picker open paths** (`pickers.rs:725`, `:747`) — `show_prepare*` +
  `show_finish` two-call pattern tangled across two borrows; untouched.
- **`set_items` / `set_words` calls** — vary per picker; stay at call site.

## Verification

- `cargo build` + `cargo test --bins` (413) — covers the logic.
- Per-site check: no intervening `state.borrow*()` between `show()` and the
  mode-set at the 4 normalized sites (else do NOT merge that site's borrow — keep
  it two-borrow and only swap the mode-set line).
- Light runtime smoke: the cage boot/render harness (app launches, renders) —
  this touches open paths but does not change WHAT `show()` does. Picker-open was
  user-verified in the prior dispatch round; this does not alter it. A full
  open-a-picker e2e is not required for this low-risk change, but if the cage runs
  cleanly, confirm a picker still opens.
