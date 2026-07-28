# Refile journal entries whose band disagrees with their citation

_2026-07-28 (US Central). Status: approved, ready for a plan._

Follow-on to `2026-07-27-journal-cycle-scope-filter-design.md` (merged
`526d5eb0`). That change fixed WHICH entries the `\` cycle can find; this one
fixes WHERE three entries are filed.

## Problem

Reading Bleak House chapter 1 and pressing Ctrl+j reports
**"No journal entry for this segment"** — while three chapter-1 Q&As exist,
filed under chapter 0.

The three rows, the only such rows in all of lit.db:

```
id 7 | BH | scene | filed 0.0 | cites BH.1.0.0  | source_text NULL
id 8 | BH | scene | filed 0.0 | cites BH.1.0.12 | source_text present
id 9 | BH | scene | filed 0.0 | cites BH.1.0.16 | source_text present
```

Band `(0,0)` is the **Preface** (`line_in_div` 1–8: "PREFACE", "A Chancery
judge once had the kindness to inform me…", "1853"). Ids 8 and 9 ask about
the Chancery courts and about when Bleak House was written, and cite chapter
1 lines 12 and 16 — both inside chapter 1's real range (9–38). They have no
legitimate claim to the Preface band.

Same root cause as the scope corruption already fixed: a litdb re-import
renumbered the edition's chapters (a front-matter offset), leaving these
entries banded under the old numbering while their citations — written from
the reading cursor — address the new one.

## Why the reader breaks

`toggle_overlay` (Ctrl+j) resolves the cursor's band via
`current_scene_divs`, then asks `find_scene_band_pages(work, d1, d2)`
(`src/input/actions/journal.rs:1387-1397`). Reading chapter 1 asks for
`(1, 0)`, which returns empty, so the miss toast fires
(`journal.rs:1404-1409`).

Nothing is wrong with that code. It asks the right question; the data
answers wrongly.

### Correction to the prior review's finding

The final whole-branch review of the previous branch reported this as a
`land_on_page` → `position()` → `unwrap_or(0)` mis-landing. **That failure
does not occur.** Traced against live data: the probe returns band `(0,0)`,
`load_band_pages` returns a list that DOES contain ids 7/8/9, `position()`
succeeds, and the entry lands correctly. The real defect is the band-lookup
miss above — a different and more visible failure. Recorded here so the
wrong mechanism is not carried forward.

## Change

A one-time claim-keyed migration setting `div1` from the entry's own
`start_citation`, for rows where the two disagree:

```sql
UPDATE journal_entries
   SET div1 = <div1 parsed from start_citation>
 WHERE start_citation IS NOT NULL AND end_citation IS NOT NULL
   AND scope IN ('scene','passage')
   AND <parsed div1> != div1
```

Scoped to `scope IN ('scene','passage')` so author- and work-scope rows
(which use sentinel divs `(-2,-2)` and `(-1,-1)`) are never touched.

Dry-run against a copy of lit.db: **3 rows changed**, all BH, all refiled
`0.0` → `1.0`. Afterwards chapter 1's band returns all three and the Preface
band is correctly empty. No other work is affected.

`div2` is not rewritten: it is 0 for every affected row, and no row in
lit.db disagrees on `div2` alone.

### Parsing the citation

`parse_citation` (`src/db/models.rs:10`) already splits `ABBREV.div1.div2.line`
via `rsplitn(4, '.')`. The migration must reuse it rather than re-deriving
the parse in SQL — an abbrev containing a dot would break a SQL `substr`
approach, and `parse_citation` is the codebase's single definition of that
format. This means the migration reads candidate rows, parses in Rust, and
writes back only the mismatches, rather than doing it in one UPDATE.

## Consequences

- **Ctrl+j on BH chapter 1 finds its three Q&As.** The reported bug.
- **The Preface band becomes empty.** Correct: it never had a real Q&A.
- **`\` reach is unchanged.** It selects by citation span and never consulted
  the band, so refiling cannot alter which entries it finds.
- **Id 7 stays unreachable by `\`.** Its citation line is `0`, outside
  chapter 1's real range (9–38) — a placeholder, consistent with its NULL
  `source_text`. Ctrl+j and the picker reach it; `\` correctly does not.
  This is the designed behavior for a chapter-level question, not a gap.

## Ordering against the scope retag

The already-merged retag (`retag-passage-scope-2026-07-27`) will move ids 8
and 9 from `scene` to `passage` on the next launch, since both have
citations and `source_text`. Id 7 stays `scene` (NULL `source_text`).

Both migrations are claim-keyed and independent — `scope IN ('scene',
'passage')` covers ids 8/9 either way, and this migration reads `div1`,
which the retag does not touch. Order does not matter. The new migration is
registered AFTER the retag in `BOOKMARKS_INIT` purely for readability.

## Testing

TDD, in-memory `rusqlite` only — never the shared lit.db.

1. **Red first.** A row filed `(0,0)` citing `ABBREV.1.0.12` is refiled to
   `div1 = 1`; assert `find_scene_band_pages(work, 1, 0)` then returns it and
   `(0, 0)` does not.
2. **Guard.** A row whose band already matches its citation is untouched, and
   is not counted in the returned total.
3. **Guard.** Author-scope (`div1 = -2`) and work-scope (`div1 = -1`) rows are
   never refiled, even though their `work_abbrev`/citation shapes differ.
4. **Guard.** A row with NULL citations is untouched.
5. **Idempotence.** A second run returns `Ok(0)` without touching a row
   (claim key held), matching `retag_passage_scoped_journal_entries`.
6. **On screen (non-waivable).** Headless: land on BH-Barrett chapter 1,
   press Ctrl+j, confirm the journal overlay opens on a chapter-1 entry
   rather than toasting "No journal entry for this segment". Must be run
   against a DB copy seeded to the PRE-migration state so the test proves
   the fix rather than the seed.

## Acceptance

- Ctrl+j on BH-Barrett chapter 1 opens the journal overlay (screenshot).
- `cargo build`, `cargo clippy`, `cargo test --bins` green
  (baseline 1222 passed / 0 failed / 3 ignored).
- The shared lit.db is never written during development or testing.

## Not in scope

- **The upstream litdb defect.** Something rewrites `scope` and renumbers
  bands on re-import. Per CLAUDE.md's upstream-routing rule that fix belongs
  in litdb, with a ledger entry here linking to it. This migration repairs
  today's data; it does not prevent recurrence. Both repair migrations use
  date-suffixed, bumpable keys for that reason.
- **Q&A picker scope cycling** (author/work/scene via Alt+t) — its own spec,
  its own branch, after this one.
