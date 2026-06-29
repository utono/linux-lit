# Stage-directions reader follow-up (linux-lit)

**Date:** 2026-06-24
**Status:** Handoff — execute in a linux-lit session
**Upstream:** litdb branch `feat/folger-stage-directions` landed stage-direction
rows in `lit.db`. This note lists what the READER needs so it displays and
navigates them correctly.

## What changed in lit.db (already done)

- New column `line_mapping.sub_line`. Spoken lines have `sub_line=0` and
  `line_in_div` = the scholarly Folger line number (UNCHANGED from before).
  Stage directions share their host spoken line's `line_in_div` with
  `sub_line=1..N` (document order), `[bracketed]` `canonical_text`,
  `speaker=NULL`.
- Canonical ordering everywhere is now
  `ORDER BY div1, div2, line_in_div, sub_line`.
- All 78 stage-bearing Folger works (38 base + 40 production `-Amb`/`-BBC`/`-DC`)
  now carry their stage directions (~16,653 rows). Production works'
  `line_mapping` is byte-identical to their base `<work>` (same div/line/
  sub_line/text), so a gloss citation on the base resolves identically on `-Amb`.

## What the reader must do

1. **ORDER BY sweep — add `, sub_line`** to every query that loads
   `line_mapping` rows in line order, or stage directions will sort ambiguously
   against their host spoken line. Known sites (verify line numbers against
   current code):
   - `src/db/queries.rs` — the line-loading queries (≈ lines 112, 573, 1292,
     2220) `ORDER BY div1, div2, line_in_div` → append `, sub_line`.
   - `src/db/concordance.rs` (≈ line 35) likewise.
   - Chunks/journal queries that order by `a_line` / `div` only are unaffected.
   - Grep to be exhaustive: `rg 'ORDER BY[^;]*line_in_div' src/`.

2. **Add `sub_line` to the `Line` model / row mapping** (`src/db/models.rs`,
   wherever a `line_mapping` row is read into `Line`). Carry it through so the
   reader can tell spoken (`sub_line == 0`) from stage (`sub_line > 0`) rows.

3. **Render stage rows.** The parked branch
   `feat/gloss-overlay-stage-directions` already adds a `GlossElement::Stage`
   variant, italic rendering, and `build_source_header` emitting `<stage>` for
   stage lines. MERGE that branch. Now that real stage rows exist in the DB, its
   `inject_stage_directions` workaround (which synthesized stage lines because
   the DB lacked them) is unnecessary — DROP it and render the real
   `sub_line > 0` rows instead.

4. **Snapshot cache invalidation.** The reader caches a serialized `LineMap` per
   work keyed on a db_fingerprint, gated by `SNAPSHOT_VERSION`
   (`src/snapshot.rs`, currently `pub const n: u32 = 8`). The works now have new
   rows, so cached snapshots are stale. Bump `SNAPSHOT_VERSION` (→ 9) so every
   work's snapshot regenerates, and confirm the fingerprint covers the new rows
   (it should, since row count/content changed).

5. **Navigation / sync / TTS must SKIP stage rows.** `sub_line > 0` rows are
   non-dialogue — they have no timestamps and should not be a cursor stop for
   `j/k` dialogue navigation, audio-sync targets, or TTS. Filter `sub_line = 0`
   where the reader steps through *spoken* lines. (Visual display still shows
   them; only dialogue stepping skips them.)

6. **Simplify the `-Amb`-divergence handling.** `text_file_map.rs` (≈ line 1024)
   and `app/mod.rs` (≈ line 3585) special-case the fact that `-Amb` editions
   historically renumbered lines (so gloss citations were matched by TEXT, not
   tuple). Production works are now byte-identical to base, so that divergence is
   gone — the text-not-tuple gloss matching can be reduced/removed for production
   works once you confirm the new parity. (Verify before deleting; do it as a
   separate, tested step.)

## Verify

- Headless nav on a stage-bearing work (e.g. `2H6`, `Ham`, `2H6-Amb`) per
  `~/utono/linux-lit/CLAUDE.md` Headless Verification — confirm pagination and
  cursor land correctly and stage directions render. (The known `JumpEnd`
  last-page pagination bug is unrelated — see `project_jn_amb_jumpend_reader_bug`.)
- Glosses still highlight correctly (litdb kept all 107 passages/120 glosses;
  base citations are unchanged).

## References

- litdb design: `~/utono/litdb/docs/superpowers/specs/2026-06-23-folger-stage-directions-import-design.md`
- litdb plan: `~/utono/litdb/docs/superpowers/plans/2026-06-23-folger-stage-directions-import.md`
- litdb CLAUDE.md "Database" section (the durable model description).
