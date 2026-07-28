# Retire "scene" for "division" across lit.db and linux-lit

_2026-07-28 (US Central). Status: approved, ready for a plan._

## Problem

"scene" is play vocabulary that leaked into the shared structural layer. A
`scene_synopses` table holds Bleak House's chapter synopses; `JournalBand::Scene`
names a band that is a chapter in a novel and a book in an epic; and
`journal_entries.scope = 'scene'` is stored against prose entries. The word is
wrong for every work type except plays, which is most of the library.

The reader's USER-FACING surfaces were fixed on 2026-07-28 (`7874b838`,
`e8457e68`): the picker header and row division column now both read
`gloss::genre_unit`, so a novel says CHAPTER and an epic BOOK. This spec is
about the layer beneath — the identifiers and stored values that still say
"scene".

## Measured scope

Counted, not estimated:

- **linux-lit**: 1320 occurrences of "scene" in `src/**/*.rs`. **700 are in
  comment lines**; ~620 are code. Those resolve to **93 distinct identifiers**
  (`scene_synopsis`, `scene_label`, `scene_lines`, `JournalBand::Scene`,
  `scene_override`, …).
- **litdb**: 684 occurrences across `scripts/**/*.py`; 12 files reference
  `scene_synopses` directly.
- **lit.db schema**: one live table `scene_synopses` (1157 rows) plus four
  historical backups (`scene_synopses_backup`, `_para_backup`,
  `_bh_preshakes_backup`, `_prev3_backup`). **No COLUMN is named `scene`.**
- **lit.db data**: `journal_entries.scope = 'scene'` on 8 rows.

## Three different things named "scene"

The rename covers two of them and deliberately leaves the third:

1. **Structural identifiers** — `scene_synopses`, `JournalBand::Scene`,
   `scene_label`, and the other 90. These name a work's division generically
   and are wrong on nine of eleven work types. **RENAME to `division`.**
2. **The stored value** `journal_entries.scope = 'scene'`. Written and
   filtered by both repos. **MIGRATE to `'division'`.**
3. **The play-genre noun** — `gloss.rs:125` maps `work_type = "play"` to the
   nouns `("play", "scene", "scenes")`, feeding `genre_unit`. A play's
   division genuinely IS a scene. **KEEP.** Renaming this would make the UI
   less accurate, not more — it is the one place the word is correct.

The distinction matters because a naive global search-and-replace would
destroy (3) along with (1).

## Change

### Rust identifiers (linux-lit)

Rename the 93 identifiers `scene* → division*`, `Scene → Division`. Notable:

- `JournalBand::Scene(d1, d2)` → `JournalBand::Division(d1, d2)`
- `src/app/scene_synopsis.rs` → `src/app/division_synopsis.rs`, and its
  `scene_label` / `scene_label_for` → `division_label` / `division_label_for`
  — note `pickers.rs` ALREADY has a `division_label`; these must not collide,
  so one gets a disambiguating name decided during implementation.
- `scene_synopses` query strings in `src/db/queries.rs` (12 references).

Comments are updated where they name a renamed identifier, and where they use
"scene" generically for a division. Comments that discuss PLAYS specifically
keep the word.

### Python identifiers (litdb)

The same rename across `scripts/`, including the 12 files that read
`scene_synopses`.

### Schema + data migration

One claim-keyed one-time migration, following the pattern of the two repairs
already shipped (`retag_passage_scoped_journal_entries`,
`refile_journal_bands_from_citations`):

```sql
ALTER TABLE scene_synopses RENAME TO division_synopses;
UPDATE journal_entries SET scope = 'division' WHERE scope = 'scene';
```

The **four backup tables keep their names**. They are historical artifacts,
not live schema; renaming them adds risk for no benefit and loses the link to
the dated snapshots they came from.

## The cross-repo ordering constraint

This is the sharpest risk in the change. **Both repos read
`scene_synopses`**, so the instant the table is renamed, whichever side has
not been updated breaks.

The migration must therefore be **compatibility-first**, not a flag day:

1. Ship BOTH repos able to read EITHER name — try `division_synopses`, fall
   back to `scene_synopses`. No behavior change; nothing renamed yet.
2. Run the rename migration.
3. Remove the fallback once both repos are confirmed on the new name.

Skipping step 1 means any running reader instance, any half-finished litdb
script, or any rollback leaves a broken pair. The user runs linux-lit
continuously and litdb scripts ad hoc, so a flag day would break a live
session.

## Non-goals

- **The play-genre noun** (item 3 above) — explicitly kept.
- **The four backup tables** — left named as they are.
- **`div1`/`div2` column names** — already neutral; nothing to do.
- **Prose comments that discuss plays** — "the scene ends" about a Shakespeare
  scene stays.
- **Any behavior change.** This is a vocabulary change. If the reader looks or
  behaves differently afterward, something is wrong.

## Testing

- `cargo test --bins` green at every step (baseline 1239 passed / 0 failed /
  3 ignored). `cargo clippy` clean.
- `pytest scripts/tests/` green in litdb.
- **The compatibility step is testable in isolation**: with the fallback in
  place and the table still named `scene_synopses`, everything must pass; then
  rename the table on a COPY and everything must pass again unchanged.
- **On-screen (non-waivable):** synopses still render (the `scene_synopses`
  table feeds them), the Q&A picker still opens on all three scopes, and the
  `\` overlay cycle still reaches journal entries. A rename that silently
  drops synopsis rendering would pass every unit test.
- Migration tested against a COPY of lit.db, never the live file, asserting
  1157 synopsis rows survive and the 8 scope rows flip.

## Acceptance

- No `scene`-named identifier remains in either repo except the play-genre
  noun and play-specific prose.
- `division_synopses` holds 1157 rows; `scope='division'` on 8 rows;
  `scope='scene'` on 0.
- Reader behavior unchanged, verified on screen.
- Both repos green; the shared lit.db never written during development.

## Sequencing note

This is a two-repo change with a shared mutable database between them. It
should be its own plan with the compatibility step as a discrete, separately
merged task — not folded into a feature branch. Given the on-screen surfaces
were already fixed, the remaining benefit is internal consistency, so there is
no pressure to rush it.
