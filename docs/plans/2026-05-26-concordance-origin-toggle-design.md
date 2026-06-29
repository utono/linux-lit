# Concordance Origin Toggle

Date: 2026-05-26

## Problem

When the user enters concordance mode (Ctrl+\ → select word) and then navigates across works with r/R, there is no way to quickly return to the work and line they were viewing before entering concordance mode. The existing `-` key (`TogglePreviousWork`) only tracks the most recent work switch, which gets overwritten by each r/R hop. After several hops, the origin is lost.

## Solution

Make the `-` key concordance-aware. When concordance mode is active, `-` toggles between the origin work (the work and exact line the user was on when they entered concordance mode) and the current concordance hit. When concordance mode is inactive, `-` behaves as it does today.

## New state

Add to `AppState`:

```rust
pub concordance_origin: Option<ConcordanceOrigin>,
```

Where `ConcordanceOrigin` is:

```rust
pub struct ConcordanceOrigin {
    pub work_abbrev: String,
    pub line_mapping_id: usize,
}
```

This struct lives in `src/concordance.rs` alongside `ConcordanceState`.

### When it is set

Set once when the concordance picker confirms a word selection (the moment the user picks a word from the Ctrl+\ picker). Captures `current_work.abbrev` and the `line_mapping_id` for `current_line` at that moment.

It is NOT updated by subsequent r/R hops — it always points to the pre-concordance origin.

### When it is cleared

Cleared by `EscapeReaderMode` alongside `concordance_state`.

## Modified `-` behavior

`toggle_previous_work()` gains an early branch at the top of the function:

1. Check: is `concordance_state.is_some()` AND `concordance_origin.is_some()`?
2. If yes, enter concordance toggle mode:
   - If the current work matches the origin work → load the concordance hit's work and navigate to the hit's `line_mapping_id` (from `concordance_state.occurrences[current_index]`)
   - If the current work does NOT match the origin work → load the origin work and navigate to `concordance_origin.line_mapping_id`
   - MPV is paused on toggle (same as current `-` behavior)
   - Concordance state is fully preserved across the toggle — r/R continue to work after returning
3. If no (concordance not active) → existing `config.previous_work` behavior, unchanged.

## Position handling

- Toggle to origin: load the origin work and pass `concordance_origin.line_mapping_id` as the target line to `display_work_at_with_prepared`. This restores the exact cursor position the user was on when they entered concordance mode.
- Toggle back to concordance hit: read `concordance_state.occurrences[current_index]` to get the hit's `work_abbrev` and `line_mapping_id`, then load that work at that line.

## Edge cases

- **Origin work = concordance hit work**: When a concordance hit is in the same work the user started from, `-` is a no-op (already viewing the origin). r/R handle intra-work navigation.
- **No concordance state**: `-` falls through to existing `TogglePreviousWork` behavior.
- **Escape during origin view**: If the user toggles to the origin and then presses Escape, concordance state is cleared and `-` reverts to standard previous-work behavior.
- **config.previous_work interaction**: Cross-work concordance hops update `config.previous_work` through the normal `display_work` path. The concordance toggle does NOT modify `config.previous_work` — it works through its own `concordance_origin` field. When concordance ends (Escape), `-` returns to using `config.previous_work` which will point to whatever the last non-origin work switch was.

## Files to modify

- `src/concordance.rs` — add `ConcordanceOrigin` struct
- `src/app.rs` — add `concordance_origin: Option<ConcordanceOrigin>` to `AppState`, clear in relevant resets
- `src/input/actions/concordance.rs` — set `concordance_origin` when concordance picker confirms a word
- `src/input/actions/pickers.rs` — add concordance-aware branch to `toggle_previous_work()`
- `src/input/actions/escape.rs` — clear `concordance_origin` alongside `concordance_state`

No new keybinds, no new Action variants. The existing `TogglePreviousWork` action and `-` key are reused with context-dependent behavior.
