# Vocab-word gloss bind (Ctrl+Shift+g)

_2026-08-10. Status: approved, implementing._

Open the gloss overlay filtered to `vocab-word` glosses for the passage
covering the reader cursor line.

## Problem

Some words carry `gloss_type='vocab-word'` glosses in lit.db — 51 rows
overall, 33 in `Err` and 12 in `LoJ`. They are reachable today only as a
one-line `gloss` field inside the vocab popup (`r`), rendered by
`load_vocab_gloss` (`src/db/queries.rs:645`) into the popup's
`.definition-gloss` row. There is no way to read one full-width in the
gloss overlay the way every other gloss type is read.

## Two data facts that shape the design

Both were measured against lit.db before designing, and both make the
change smaller than it first looks:

1. **Vocab-word glosses are per-OCCURRENCE, not per-word.** All 12 LoJ
   rows gloss the same word, `solicitude`, each attached to a different
   passage (`LoJ.1.2207`, `LoJ.4.14043`, …). So the bind must resolve
   *which* occurrence — it cannot key on the word alone.
2. **Their passages carry NO sibling gloss types.** Every one of the 12
   LoJ passages has `vocab-word` as its only type. So the overlay opens
   on exactly one candidate: `start_gloss_idx` has nothing to choose
   between, and there is no type-cycling behavior to design.

## Approach

Scope is the **cursor line**, mirroring Ctrl+g. Cursor inside a glossed
passage opens that passage's gloss; anywhere else toasts. This is
passage-scoped, not token-scoped: the bind works anywhere in the glossed
span, not only on the word itself — correct, given the glosses are stored
against passages.

`try_open_syntax_gloss_at_cursor` (`gloss.rs:3736`) is an exact
precedent — the same function scoped to a single gloss type — so the new
code clones its proven shape rather than inventing one.

### Components

**`try_open_vocab_gloss_at_cursor(state) -> bool`**, in
`src/input/actions/gloss.rs` beside the syntax variant, with
`const GLOSS_TYPES: &[&str] = &["vocab-word"]`:

1. Resolve cursor -> `(work.canonical_abbrev, (div1, div2, line_in_div))`.
   Glosses are STORED under the canonical base abbrev, so looking them up
   any other way makes a variant edition (`-Amb`, `-BBC`) miss its own
   glosses — the recurring lookup-mismatch bug class.
2. `find_glossed_passages(&conn, &abbrev, GLOSS_TYPES)`.
3. Find the covering passage via `passage_covers` on parsed citations.
4. `find_glosses_by_start(...)`; return false if empty.
5. Save `gloss_return_pos`, then `open_gloss_overlay(..., from_picker:
   false, desired_type: None, entry_open: true)`.

All DB reads complete before any `borrow_mut`, matching the borrow
discipline of both neighbors.

**`open_vocab_gloss_at_cursor(state)`** — thin wrapper that toasts
`"No vocab gloss on this line"` when the `try_` form returns false,
mirroring `open_gloss_at_cursor`'s toast contract. The bind calls this.
The bare `try_` form stays available should the `\` overlay cycle ever
want a vocab stop; it is NOT wired into the cycle now (YAGNI).

The gloss overlay itself needs no changes — it renders whatever
`all_glosses` it is handed.

### Keybind

`Ctrl+Shift+g` -> `Action::ShowVocabGloss`, keeping the whole concept on
the `g` gloss hub:

- `Ctrl+g` — `ToggleGlossOverlay`
- `Alt+g` — `OpenGlossPicker`
- `Ctrl+Shift+g` — `ShowVocabGloss` (new)

RPD-verified: `g` sits on `<AD07>` as `[ g, G ]`, so `g` is level 1
(unshifted) and Ctrl vs Ctrl+Shift are distinct chords on that cap.

### Surfaces updated in the same change

Required, not optional — a missed mirror is a silent shadow or a stale
legend:

- `src/input/actions/mod.rs` — `Action::ShowVocabGloss` variant
- `src/input/keymap.rs` — dispatch arm
- `src/input/keymap_config.rs` — `KeyCombo::ctrl_shift("g")` + `g`-hub comment
- `src/ui/keybinds_overlay.rs` — keycap strip AND `describe()` arm
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — else the JSON
  silently shadows the compiled default

## Error handling

- No work loaded / no line under cursor / DB open failure -> `false`,
  wrapper toasts. No panic, no partial state mutation.
- No covering passage, or covering passage has no `vocab-word` gloss ->
  `false`, toast. Nothing opens, no overlay state touched.
- Escape returns to the saved reader page via `gloss_return_pos`.

## Testing

- **Unit:** `ctrl_shift("g")` resolves to `ShowVocabGloss`; `ctrl("g")`
  still resolves to `ToggleGlossOverlay` (guards the level-2 distinctness).
- **Headless (cage):** land on a LoJ `solicitude` passage and confirm the
  overlay actually PAINTS. A green build does not prove the overlay opens
  — acceptance must exercise the visible surface, per the #13 lesson.

## Out of scope

- A `vocab-word` stop in `GlossPickerFilter` (cursor-line scope chosen).
- Per-token resolution (glosses are stored per passage).
- Wiring vocab into the `\` overlay cycle.
