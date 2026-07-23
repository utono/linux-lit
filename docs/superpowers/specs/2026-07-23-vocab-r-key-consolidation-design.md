# Vocab `r`-key consolidation + finish the ask/rewrite key convention

**Date:** 2026-07-23
**Status:** approved, ready for planning
**Scope:** keybind reshuffle across main reader, gloss overlay, journal overlay
(plus the three lockstep mirrors: `keymap_config.rs`, the overlay legends, and
the stowed `keymap.json`).

## Motivation

The codebase already assigns **one key per concept** — `g` = gloss, `j` =
journal, `w` = rewrite, `a` = ask — but vocab is scattered across three keys
(`r`, `\`, `-`) and two `Ctrl+r` outliers break the convention:

1. Main reader `Ctrl+r` → `VocabJournalAsk` — an "ask" action wearing the vocab
   key, and the odd one out (every other ask surface already sits on `Ctrl+a`).
2. Gloss overlay `Ctrl+r` → `begin_rewrite` — a *rewrite* action that should be
   on `w` like its siblings (journal-overlay rewrite and chat rewrite are
   already on `Ctrl+w`).

Plain `r` is *already* vocab on every reading surface (reader, chat, journal,
gloss), and `R` / `Ctrl+Shift+r` are already deliberately left unbound on
chat/journal/gloss with comments reserving them "for the vocab surface." So the
codebase is primed for this consolidation; this change finishes it.

**Priority signal from the user:** `AddVocabWord` is used more often than
`VocabJournalAsk`, so `AddVocabWord` gets the prime short chord (`Ctrl+r`) and
`VocabJournalAsk` moves to the longer `Ctrl+Shift+r`.

## Guiding principle

One key per concept: **`r` = vocab · `g` = gloss · `j` = journal · `w` =
rewrite · `a` = ask.** After this change, `Ctrl+r` never means "rewrite"
anywhere, and every vocab function lives on the `r` key (except the jumps, which
stay on the `-` stepper key — see Non-goals).

## Changes

### Change 1 — Main reader: `r` becomes the vocab hub

| Chord           | Before          | After                         |
|-----------------|-----------------|-------------------------------|
| `r`             | VocabPopupTap   | VocabPopupTap *(unchanged)*   |
| `Ctrl+r`        | VocabJournalAsk | **AddVocabWord**              |
| `Ctrl+Shift+r`  | *(free)*        | **VocabJournalAsk**           |
| `Alt+r`         | *(free)*        | **ToggleVocabHighlight**      |

`ToggleVocabHighlight` moves off `Alt+\`; `AddVocabWord` moves off
`Ctrl+Alt+\`; `VocabJournalAsk` moves off `Ctrl+r`.

### Change 2 — `AddVocabWord` consolidated onto `Ctrl+r` everywhere

`AddVocabWord` is reachable from inside the gloss and journal overlays today via
an inline `Ctrl+Alt+\` check. Consolidate all of it onto `Ctrl+r`:

- Reader: `Ctrl+r` → AddVocabWord (Change 1).
- Gloss overlay: add `Ctrl+r` → AddVocabWord (its old `Ctrl+r` = begin_rewrite
  moves — Change 3).
- Journal overlay: change `Ctrl+r` from consumed no-op → AddVocabWord.
- **Remove the `Ctrl+Alt+\` chord entirely** — the reader default bind AND both
  in-overlay inline checks. `AddVocabWord` then lives only on `Ctrl+r`.

### Change 3 — Gloss rewrite joins the `w` family

| Surface        | Chord    | Before        | After                   |
|----------------|----------|---------------|-------------------------|
| Gloss overlay  | `Ctrl+r` | begin_rewrite | *(now AddVocabWord)*    |
| Gloss overlay  | `Ctrl+w` | *(free)*      | **begin_rewrite**       |

Result: `Ctrl+w` = rewrite on every overlay (gloss + journal + chat).

### Freed chords (left free, not reassigned)

- `Ctrl+Alt+\` (was AddVocabWord)
- `Alt+\` (was ToggleVocabHighlight)

## Non-goals (YAGNI)

- **Vocab jumps stay put.** `JumpToNextVocab` (`Ctrl+-`, also the vocab-loop
  entry) and `JumpToPrevVocab` (`Ctrl+Shift+-`) remain on the `-` stepper key.
  Remapping them ripples into the vocab-loop mode's own entry/exit
  (`vocab_loop.rs`) and was ruled too invasive for this change.
- **The `Ctrl+a` ask family is untouched.** Visual-mode / journal / gloss
  `Ctrl+a` all keep opening their ask cards. `VocabJournalAsk` deliberately
  stays on the `r` key (as `Ctrl+Shift+r`), NOT folded into `Ctrl+a` — per the
  user's explicit choice to keep every vocab function on `r`.
- Journal-overlay `Ctrl+w` rewrite-target chooser and chat `Ctrl+w` are already
  correct — no change.

## Correctness details

- **RPD case-handling.** `Ctrl+Shift+<letter>` arrives as **lowercase key name +
  shift=true**, NOT the uppercase glyph (confirmed by the `OpenLastGloss`
  precedent and its test at `src/input/keymap_config.rs:607-616`). So
  `VocabJournalAsk` must register as `KeyCombo::ctrl_shift("r")` AND, mirroring
  `OpenLastGloss`, also `KeyCombo::ctrl_shift("R")` for robustness across
  layouts that capitalize.
- **No hidden collision at `Ctrl+Shift+r`.** The gloss overlay already uses
  `Ctrl+Shift+r/R` for *restore-browsed-revision* (`keymap.rs:2411-2414`). That
  is the gloss `InputMode`; the new `Ctrl+Shift+r` = VocabJournalAsk is a
  main-reader bind. `InputMode` is a flat enum with one handler active at a
  time, so there is no runtime collision. Do not "fix" the gloss revision bind
  when mirroring — it is intentionally a different surface.
- **`Ctrl+r` no-op removal.** The journal overlay's `Ctrl+r` consumed no-op
  (`keymap.rs:2007`) is replaced by the AddVocabWord arm; ensure the chord still
  can't fall through to the term-filter intercept or the plain-`r` vocab arm.

## Lockstep mirrors (required by CLAUDE.md)

Every keybind change updates all three mirrors in the same change:

1. **`src/input/keymap_config.rs`** — reader compiled defaults: set `Ctrl+r` =
   AddVocabWord, add `Ctrl+Shift+r` (+`R`) = VocabJournalAsk, add `Alt+r` =
   ToggleVocabHighlight; remove `Ctrl+Alt+\` = AddVocabWord and `Alt+\` =
   ToggleVocabHighlight. Update the corresponding assertions in the module's
   test block.
2. **`src/input/keymap.rs`** modal handlers —
   - `handle_gloss_key`: move `begin_rewrite` from the `"r"` (Ctrl) arm to a
     `"w" if is_ctrl` arm; add `Ctrl+r` → `vocab_add::open`; remove the inline
     `Ctrl+Alt+\` add-vocab check (KM:2323).
   - `handle_journal_key`: change the `Ctrl+r` no-op arm to `vocab_add::open`;
     remove the inline `Ctrl+Alt+\` add-vocab check (KM:1897).
3. **`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`** — update the
   `r` / `Ctrl+r` entries, add `Ctrl+Shift+r` and `Alt+r`, remove the `Alt+\`
   ToggleVocabHighlight entry (and the `Ctrl+Alt+\` AddVocabWord entry if
   present). Deploy with `cd ~/tty-dotfiles && stow linux-lit`.
4. **Legends** —
   - `src/ui/keybinds_overlay.rs` (main-card Cairo keycap strip **and** the
     `describe()` detail arm): `Ctrl+r` → add vocab word, `Ctrl+Shift+r` →
     vocab Q&A, `Alt+r` → toggle vocab highlight; drop the `Ctrl+Alt+\` /
     `Alt+\` vocab entries.
   - `src/ui/gloss_keybinds_overlay.rs`: `Ctrl+r` now = add vocab word (was
     rewrite), `Ctrl+w` = rewrite this gloss. (Bonus: the gloss/journal/chat
     legends currently omit their `Ctrl+r` entirely — this change closes that
     stale gap for gloss.)
   - Run the `update-cairo-keybinds-overlay` three-pass cross-reference across
     `keymap_config.rs`, the `ui/*_keybinds_overlay.rs` legends, and the stowed
     `keymap.json`.

`docs/guides/keybind-surface-guide.md` is NOT in the lockstep set (on-request
only).

## Testing

Keybind wiring is verified by the compiled-default assertions in the
`keymap_config.rs` test block (update them to match) plus `cargo build` /
`cargo test --bins`. On-screen acceptance (does `Ctrl+r` open the add-vocab
card; does the gloss overlay's `Ctrl+w` start a rewrite) is a headless
cage/grim drive or a manual hand-off — decided at the end of implementation per
the project's "testing before completion" rule.

## Pre-merge review gate

This change meets the spec threshold (reshuffles multiple reader-surface
keybinds, spans main card + gloss overlay + journal overlay). It gets
`superpowers:requesting-code-review` before merge, and the
`update-cairo-keybinds-overlay` three-pass cross-reference runs inside that
review.
