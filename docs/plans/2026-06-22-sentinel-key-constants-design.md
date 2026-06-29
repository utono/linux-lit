# Named sentinel-key constants — design

## Goal

Replace the magic-number scene-key sentinels with two named `pub(crate) const`s
so they are greppable and self-documenting, and a future feature cannot silently
reuse a sentinel value. This is audit opportunity #8, deliberately scoped to only
the two unambiguous sentinels.

## Scope

After auditing every sentinel site:

- **In scope:**
  - `SYNOPSIS_WHOLE_WORK: (i64, i64) = (-2, 0)` — the whole-work synopsis scene
    key. 8 functional + test sites, all in `src/app.rs`, all unambiguously this
    sentinel.
  - `JOURNAL_WORK_DIV: (i64, i64) = (-1, -1)` — the `(div1, div2)` stored for a
    journal page scoped to the whole work. 1 functional literal site
    (`src/input/actions/journal.rs:271`); everywhere else journal already
    abstracts the band via the `JournalBand` enum.
- **Explicitly EXCLUDED:**
  - `(0, 0)` "Prologue" — **overloaded**. It is the Prologue scene key in some
    paths, but ALSO a generic not-found / default / uninitialized sentinel
    (`scene_label` fallbacks at app.rs:5624/5649/5660/5677, initial
    `synopsis_overlay_scene`/`synopsis_amend_scene` at app.rs:1878–1879,
    `JournalBand::Scene(0,0)` default at app.rs:1817, and the unrelated
    `search_bar.update_counter(0, 0)` x/y counter). Aliasing every `(0,0)` to a
    `PROLOGUE` constant would be a correctness hazard. Left as-is.
  - Induction `(-1, *)` — has **no live key site**; only text-classifier test
    strings (`is_act_scene_marker("Induction")`). Nothing to replace.

## Component

Two module-level constants in `src/app.rs` (where the synopsis sentinel and 8 of
the 9 sites live):

```rust
/// Scene-key sentinel for the whole-work synopsis (not a real (div1,div2)
/// scene). Sorts before all real scenes in the synopsis picker; `whole_work_label`
/// maps it to "Whole work". Distinct from the journal whole-work key, which lives
/// in a separate table and disambiguates by its `scope` column.
pub(crate) const SYNOPSIS_WHOLE_WORK: (i64, i64) = (-2, 0);

/// (div1, div2) stored for a journal page scoped to the whole work (vs a scene).
/// The journal_entries table ALSO carries a `scope` TEXT column ('work'/'scene'),
/// so this pair is not unique on its own — it is always paired with scope='work'.
pub(crate) const JOURNAL_WORK_DIV: (i64, i64) = (-1, -1);
```

Place them near the top of the synopsis-related free functions in `app.rs`
(e.g. just above `whole_work_label`), or with the other module-level
consts/items — match the file's existing placement style.

## Call-site changes (exact, behavior-identical)

### `src/app.rs` — `SYNOPSIS_WHOLE_WORK`

Replace each `(-2, 0)` scene-key literal:

- **5599** (`whole_work_label`): `if (div1, div2) == SYNOPSIS_WHOLE_WORK {`
- **6251** (`prepend_whole_work`): `out.push(SYNOPSIS_WHOLE_WORK);`
- **6263** (`has_whole_work`): `let has_whole_work = s.synopsis_cache.contains_key(&SYNOPSIS_WHOLE_WORK);`
- **6292** (scene-loop guard): `if k != SYNOPSIS_WHOLE_WORK && seen.insert(k) && s.synopsis_cache.contains_key(&k) {`

Test sites (use the const, per the agreed decision — so a value change can't
silently desync the tests):

- **6714**: `whole_work_label(-2, 0)` takes the div1/div2 as **separate args**, so
  it becomes `super::whole_work_label(SYNOPSIS_WHOLE_WORK.0, SYNOPSIS_WHOLE_WORK.1)`.
- **6725**: the `vec![(-2, 0), (1, 1), (1, 2), (2, 1)]` entry is a **tuple**, so it
  becomes `vec![SYNOPSIS_WHOLE_WORK, (1, 1), (1, 2), (2, 1)]`.
- **6737**: `vec![(-2, 0)]` → `vec![SYNOPSIS_WHOLE_WORK]`.

Doc-comment touch-ups (prose currently spells `(-2,0)`):
- **5596**: keep the literal in prose OR reword to name the const; reword to
  `... the whole-work synopsis position (SYNOPSIS_WHOLE_WORK), or None for ...`.
- **6246**: `Put the whole-work synopsis key (SYNOPSIS_WHOLE_WORK) first ...`
- **6289–6291**: update the two `(-2,0)` mentions in the comment to name the const.

### `src/input/actions/journal.rs` — `JOURNAL_WORK_DIV`

- **271**: the band-match arm `JournalBand::Work => ("work", -1_i64, -1_i64),`
  becomes
  `JournalBand::Work => ("work", crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1),`.
  The `JournalBand::Work` enum arms elsewhere in the file are untouched — only this
  DB-write coordinate literal is replaced.

## Behavior preservation

A `const (i64, i64)` compiled into the same positions is bit-identical to the
literal. No new control flow, no new module, no new public surface beyond the two
`pub(crate)` consts. The only risk is mis-selecting a site; mitigated by the
narrow scope (only `(-2, 0)` and the single journal `(-1, -1)`, both
unambiguous). The implementer must NOT touch any `(0, 0)` or `(-1, *)` literal.

## Global Constraints

- **No behavior change.** Literal→const swap only. Reviewer confirms each
  replaced site is a scene-key sentinel use (not a coincidental zero / unrelated
  pair).
- **Do NOT touch `(0,0)` or Induction `(-1,*)` sites** — out of scope and
  hazardous (overloaded / no key site).
- **No keybind change** → do NOT touch `keybinds_overlay.rs`, `keymap_config.rs`,
  `keymap.json`.
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): `rg`/`fd` not `grep`/`find`; `\mv -f`/`\cp -f`/
  `command rm -f` for non-interactive overwrite/delete.

## Testing

The existing unit tests `whole_work_label` / `prepend_whole_work`
(`app.rs` ~6714+) already exercise the `SYNOPSIS_WHOLE_WORK` paths and become the
regression guard (they now reference the const). No new tests are needed — the
change is a pure literal→const substitution with no new logic. The journal site
has no unit test (GTK/DB path); its correctness is the const value matching the
prior literal, verified by the reviewer.

Verification = `cargo build` + `cargo clippy` clean + `cargo test --bins` green +
reviewer site-by-site confirmation. No cage pass needed (no runtime/visual
surface changes — the on-disk keys and in-memory keys are numerically identical).

## Out of scope

- The `(0,0)` Prologue overload and Induction `(-1,*)` (see Scope).
- Other audit refactors (#5 footer/hint builder, #6 Picker trait) — each its own
  spec.
