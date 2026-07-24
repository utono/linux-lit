# Reader journal-bind reshuffle: consolidate onto the `j` cap

**Date:** 2026-07-23
**Status:** Approved, ready to implement

## Requested change

In reader mode, three binds move:

1. **Drop** `Ctrl+j` (currently `ToggleJournalOverlay`).
2. **Move** `Alt+j` (`OpenJournalPicker`) → `Ctrl+j`.
3. **Move** `Ctrl+a` (`OpenRecentQaPicker`) → `Alt+j`.

## Before → after

| Combo    | Before                | After               |
|----------|-----------------------|---------------------|
| `Ctrl+j` | ToggleJournalOverlay  | OpenJournalPicker   |
| `Alt+j`  | OpenJournalPicker     | OpenRecentQaPicker  |
| `Ctrl+a` | OpenRecentQaPicker    | (unbound)           |

`ToggleJournalOverlay` becomes unbound in the reader table (the Action enum
variant stays — still reachable via a `keymap.json` override, like
`ToggleLastOverlay`). `Ctrl+a` becomes free.

## Consistency rationale

The app's key→concept map (keybind-consistency-guide.md) has `j` = journal and
`a` = ask. This change consolidates BOTH journal pickers onto the `j` cap
(`Ctrl+j` = journal picker, `Alt+j` = recent-Q&A jump-back), which strengthens
"j = journal" — recent-Q&A is a journal jump-back, so it reads more naturally on
`j` than on `a`. The overlay TOGGLE is dropped; the `\` overlay cycle remains the
way into the journal overlay. The guide's `j`/`a` entries and change log are
updated to match.

## Surfaces to update (lockstep)

All four mirrors change in one commit:

1. **`src/input/keymap_config.rs`** — the compiled default table (lines ~353-357)
   AND the `#[cfg(test)]` assertions that check these combos (lines ~541 etc.).
2. **`src/ui/keybinds_overlay.rs`** — the Ctrl+/ overlay: the `j` keycap strip
   entry (line 86: modifier chips `C-j`/`M-j`), the `a` keycap strip entry
   (line 69: drop the `C-a` chip), and the `describe()` detail arms
   (`journal tog` label removed/repointed, `jrnl Q&A picker`, `recent Q&A`, and
   the `play/pause` `a`-cap prose that mentions Ctrl+a).
3. **`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`** — the stowed
   override (lines 7, 53, 54). It shadows compiled defaults, so it MUST change or
   the JSON silently reverts the reshuffle. Redeploy with
   `cd ~/tty-dotfiles && stow linux-lit`.
4. **`docs/guides/keybind-consistency-guide.md`** — update the `j`/`a` concept
   entries and append a change-log line.

## Non-goals

- No change to plain `j` (next bookmark), plain `a` (TogglePause), `Shift+a`
  (authorship), or `Ctrl+Shift+a` (attribution set).
- `ToggleJournalOverlay` is not deleted from the Action enum — only unbound.

## Testing

- `cargo build` + `cargo clippy` clean; `cargo test --bins` green (the
  keymap_config assertions are updated to the new mapping).
- Headless cage: open the Ctrl+/ overlay, confirm the `j` cap shows
  `C-j jrnl Q&A picker` / `M-j recent Q&A` and the `a` cap no longer shows a
  `C-a` chip. Drive `Ctrl+j` and `Alt+j` in reader mode and confirm the correct
  picker opens (KEY log + screenshot).
- Confirm on the real renderer via the deployed keymap.json.
