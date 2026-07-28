# `\` cycle journal stop: select by citation span, not by `scope`

_2026-07-27 (US Central). Status: approved, ready for a plan._

## Problem

The plain `\` segment-overlay cycle (gloss → journal → syntax → gloss → …)
cannot reach journal entries that are `scope='scene'` but carry a citation
span. On Bleak House the journal stop is dead entirely: all 11 of BH's
journal entries are `scope='scene'`, so `\` never opens one.

Two user reports, one root cause.

**Report A (reader entry).** Cursor on "How Alexander wept when he had no
more worlds to conquer" (BH-Barrett, Chapter 2). `\` opens the reader gloss;
`\` again toasts "Nothing else to cycle to for this passage". A journal
entry for that exact segment exists — the Q&A picker lists it as
`How Alexander wept when he had no more worlds to conquer, everybody kn…
2.0 passage`.

**Report B (round trip — the stronger repro).** Ctrl+j → the Q&A picker →
select the Alexander entry → the journal overlay opens on it, showing the
citation `— Bleak House (Sean Barrett), 2.0.48`. `\` advances to the gloss
stop. `\` again toasts "Nothing else to cycle to for this passage" instead
of returning to the journal entry the user was reading two presses earlier.

The cycle cannot see an entry the user just came from.

## Root cause

`journal_has_content_at_cursor` (`src/input/actions/journal.rs:1282`) probes
the journal stop through `find_journal_page_for_line`
(`src/db/journal.rs:624`), whose query filters on filing scope:

```sql
WHERE work_abbrev = ?1 AND scope = 'passage'
  AND start_citation IS NOT NULL AND end_citation IS NOT NULL
```

The comment above the probe (`journal.rs:1292-1298`) states the rule this
encodes:

> `scope='scene'` entries carry no span and are deliberately unreachable by
> `\`

**That assumption is false in the data.** The reported entry is:

```
id 24 | work_abbrev BH | scope 'scene' | div1 2 | div2 0
start_citation BH.2.0.48 | end_citation BH.2.0.48
```

Scene-scoped *and* fully citation-bearing. This is not a one-off: lit.db
holds **19** `scope='scene'` entries with a non-NULL `start_citation`,
against 27 `scope='passage'` entries — roughly 40% of all span-bearing
journal entries are invisible to `\`.

The defect is a category error. `find_journal_page_for_line`'s own doc
comment (`journal.rs:631-636`) already draws the correct distinction:

> The band columns still say where the entry is FILED, so return them for
> `land_on_page`; the citation says where the passage LIVES.

The `scope = 'passage'` predicate contradicts that by re-introducing a
filing-based test into a location query. The Q&A picker already gets this
right — it labels a row "N.N passage" from
`start_citation.is_some() && end_citation.is_some()`
(`journal.rs:3038`), never from `scope`. That is exactly why the picker
advertises a passage the cycle then denies exists.

## Change

Drop `AND scope = 'passage'` from `find_journal_page_for_line`'s WHERE
clause. Candidate selection becomes purely span-based: any entry with a
parseable `start_citation`/`end_citation` pair whose span contains the
anchor line qualifies.

Nothing downstream changes:

- The `IS NOT NULL` guards already do the real filtering.
- The existing priority rule (nearest start wins, so a narrow passage
  nested in a wider one is preferred; newest id breaks ties) resolves
  overlaps unchanged.
- The returned band `(div1, div2)` still comes from the entry's stored
  columns, so `land_on_page` files the landing exactly as before.

`scope` remains the filing concept; the citation remains the location
concept.

## Stale comments corrected in the same change

Both assert the false "scene entries carry no span" rule. Left in place,
the next session re-derives the defect from them:

- `src/input/actions/journal.rs:1292-1298` — the SPAN-SCOPED ONLY note.
- `src/input/actions/overlay_cycle.rs:38-40` — the module doc's
  EVERY STOP IS SEGMENT-SCOPED paragraph repeats the claim.

Each is rewritten to state the rule that actually holds: a journal entry is
`\`-reachable when its citation span covers the anchor, whatever its
`scope`; entries with no citation at all are unreachable by `\` and are
reached with Ctrl+j or the picker.

## Explicitly preserved

Segment scoping is **not** relaxed. An entry still needs a citation span
covering the lap anchor to qualify. The 2026-07-27 change that removed the
scene-band fallback — which had `\` opening the band's oldest Q&A, about a
different passage than the one on screen — stands untouched.

Entries with NULL citations stay `\`-unreachable by design.

## Blast radius

`find_journal_page_for_line` has exactly two callers, both on the `\` path:

- `journal_has_content_at_cursor` (`journal.rs:1282`) — the stop probe.
- `open_journal_scene`'s cursor-hit branch (`journal.rs:1329-1352`) — the
  open.

They must agree, and a single shared query keeps them in lockstep. Ctrl+j's
band fallback, the Q&A picker, `find_scene_band_pages`, and `land_on_page`
do not call it and are untouched.

## Testing

TDD, per the project default for sync-adjacent fixes. Tests 1 and 2 are
pure SQL against an in-memory `rusqlite` connection; both must be written
first and test 1 must be seen failing.

1. **Red first.** Insert a `scope='scene'` entry whose citation span covers
   a line; assert `find_journal_page_for_line` returns it. Fails today,
   passes after the predicate is dropped.
2. **Guard.** An entry with NULL citations is still not returned, whatever
   its scope. Prevents the fix from widening into "any entry in the band".
3. **Guard.** A span that does not cover the anchor is still not returned —
   segment scoping intact.
4. **Round trip (headless).** Report B as an acceptance test: land on
   BH-Barrett 2.0.48, open the journal overlay on entry 24, press `\`
   twice, assert the journal overlay is showing entry 24 again. This is the
   strongest assertion available — it exercises probe and open together,
   through the real cycle.

## Acceptance

- Report A: cursor on BH-Barrett 2.0.48, `\` `\` reaches the journal entry
  rather than toasting.
- Report B: the `\` `\` round trip returns to the entry it started on.
- `cargo build`, `cargo clippy`, `cargo test` green.
- On-screen confirmation on the real renderer or via the headless harness —
  a green build is not "done" for a change with visible behavior.
