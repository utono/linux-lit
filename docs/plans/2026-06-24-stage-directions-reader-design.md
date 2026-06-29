# Stage directions in the reader — design

**Date:** 2026-06-24
**Status:** Approved, ready for implementation plan
**Upstream:** litdb branch `feat/folger-stage-directions` (merged) added
stage-direction rows to `lit.db` via a new `line_mapping.sub_line` column. This
spec is the reader-side follow-up so linux-lit displays, navigates, and
gloss-selects those rows correctly.
**Supersedes:** the workaround-based approach in
`feat/gloss-overlay-stage-directions` (that branch's `<stage>` parse/render is
kept; its `inject_stage_directions` workaround is dropped — see §5).

## Background (verified DB state)

`lit.db` now stores stage directions as sub-numbered `line_mapping` rows:

- New column `line_mapping.sub_line`. Spoken lines: `sub_line = 0`,
  `line_in_div` = the scholarly Folger number (UNCHANGED). Stage directions:
  share the host spoken line's `line_in_div` with `sub_line = 1..N` in document
  order, `[bracketed]` `canonical_text`, `speaker = NULL`.
- Canonical order is `div1, div2, line_in_div, sub_line`.
- `2H6` example (1.4.43): spoken row `43.0` "Lay hands…", then stage rows
  `43.1`/`43.2` (the two physical lines of `[The Guard arrest… / …their
  papers.]`) and `43.3` `[To Jourdain.]`. `2H6` and `2H6-Amb` are now
  byte-identical (both 3537 rows; verified equal counts and that gloss citations
  like `2H6.1.1.35` resolve to spoken row `35.0`).
- Stage rows are DB-distinguishable: `2H6` has 3208 spoken rows (`sub_line=0`,
  speaker non-null) and 329 stage rows (`sub_line>0`, speaker NULL).

## The problem this fixes

Two layers currently drop stage directions:

1. **Load order.** `load_work` (and 4 other queries) order by
   `div1, div2, line_in_div` with no `sub_line`, so the stage rows sharing a
   spoken line's number sort ambiguously against it.
2. **The buffer↔DB matcher (the linchpin).** The reader renders Shakespeare from
   `folger-cleaned/<work>.txt` (which already contains the stage directions) and
   text-matches each `.txt` line to a DB row in `build_line_map`. But
   `text_file_map::normalize()` STRIPS bracket contents, so a stage line like
   `[To Jourdain.]` normalizes to empty on BOTH sides and the matcher SKIPS it
   (the `if nf.is_empty()/db_norm.is_empty() { continue; }` guards). The stage
   `.txt` line still displays, but has NO `buffer_to_work` entry — so a visual
   selection over it yields no work line (the original "no stage directions in
   the gloss" bug) and navigation cannot anchor to it.

Because the DB now carries the per-line truth (`sub_line`), the reader should
read stage-vs-dialogue from the mapped `Line`, not re-infer it from buffer text
— consistent with the project rule "if lit.db encodes a per-line fact, surface
it through LineMap/Line and read it; never reconstruct it by classifying buffer
text" (CLAUDE.md "Pagination & Scene Boundaries"; memory
`feedback_authoritative_metadata_not_text_inference`).

## Design — 8 components

### 1. `Line` gains `sub_line`

Add `pub sub_line: i64` to `Line` (`src/db/models.rs`). Read it in every
`line_mapping` row mapping (`src/db/queries.rs` `load_work` etc.). When
`sub_line > 0`, force `is_dialogue = false` (a stage direction is never spoken
dialogue). Spoken rows keep `sub_line = 0` and their existing `is_dialogue`
classification.

### 2. ORDER BY sweep — append `, sub_line`

Append `, sub_line` to the five queries that load `line_mapping` rows in line
order so rows arrive in document order:

- `src/db/queries.rs:112`, `:573`, `:1292`, `:2220`
- `src/db/concordance.rs:35`

(Chunk/journal queries ordering by `a_line`/div only are unaffected. Grep
`rg 'ORDER BY[^;]*line_in_div' src/` to confirm none are missed.)

This alone makes DB-rendered works (no `text_file`) show stage directions in
position, since their reading card builds straight from ordered `work.lines`.

### 3. `build_line_map` matches stage lines (the linchpin)

In `build_line_map_mode` (`src/text_file_map.rs`), add stage-line matching so a
stage `.txt` line maps to its DB stage row:

- When a `.txt` (buffer) line satisfies `line_types::is_stage_direction`, match
  it against the next DB stage row (`Line.sub_line > 0`) by **raw trimmed text**
  equality, advancing within the existing cursor window — instead of the
  bracket-stripped `normalize()` path (which makes both sides empty).
- The match is reliable 1:1: the litdb parser derived stage TEXT from
  `folger-cleaned` (one DB row per physical `.txt` line), so the `.txt` stage
  line and the DB `canonical_text` are byte-identical, including multi-line stage
  directions (each physical line is its own `sub_line` row — verified for
  `2H6` 1.4.43's `43.1`/`43.2`).
- Populate `buffer_to_work[buf_idx]` (and the inverse `work_to_buffer`) for the
  stage line. Leave `normalize()` and all spoken-line matching untouched (it is
  perf-critical; do not change the hot path).

This is the prerequisite for BOTH the gloss selection (§5) and DB-driven nav
(§4) — without a `buffer_to_work` entry, a stage buffer line has no `Line` to
read.

### 4. DB-driven navigation classification

The nav binds (`,` `q` `y` `x` `g` `GG`) and pagination currently classify each
buffer line by regex on its text via closures in `src/input/viewport.rs`
(`is_stage`, `is_dialogue`, `is_speaker`) and `is_dialogue_line`
(`viewport.rs:664`). Route these through the mapped `Line` when one exists:

- For a buffer line `i`, look up `work_line_for_buffer(i)` (`app/mod.rs:573`,
  which reads `LineMap.buffer_to_work`). If it resolves to a `Line`, derive
  stage-ness from `line.sub_line > 0` and dialogue-ness from `line.is_dialogue`
  (which §1 already sets correctly for stage rows).
- Fall back to the existing regex classifiers ONLY when the buffer line has no
  mapped work line (works without DB coverage, mid-load, or no `text_file`).
  Keep the regex helpers as that fallback — do not delete them.
- Scope to the stage/dialogue distinction this feature needs. Speaker /
  separator / act-scene classification may stay regex for now (a later project
  can DB-drive the rest); only convert what this feature requires
  (stage-vs-dialogue) to avoid scope creep.

Result: `j/k` dialogue nav, `,`/`q` sync stepping, and `x`/`y`/`g`/`GG` page/jump
binds skip stage rows because the mapped `Line` says so — not because a regex
guessed. The displayed reading card still SHOWS stage lines; only spoken-line
stepping skips them.

### 5. Merge the rendering branch, drop the workaround

Merge `feat/gloss-overlay-stage-directions` into this work. It provides the
`GlossElement::Stage` variant, italic `<stage>` rendering in the gloss overlay,
and `build_source_header` emitting `<stage>` for `is_stage_direction` lines —
all still correct and needed.

DROP `inject_stage_directions` (and its trailing-flush + the `_original`/
`original` plumbing added for it). It synthesized stage lines into the result
card because the DB lacked them; with §1–§3 the visual selection now carries the
real stage `Line`s, so `build_source_header(&selected_lines, …)` emits `<stage>`
directly and both the loading and result cards render the real stage directions.
Remove the helper, its tests, and the call site in `show_gloss_with_color`
(restore the parameter to its pre-injection form).

### 6. Snapshot cache version bump

Bump `SNAPSHOT_VERSION` 8 → 9 (`src/snapshot.rs:35`). Works gained rows and the
serialized `LineMap` now references stage lines, so cached snapshots are stale
and must regenerate on next load. (The version gate already forces regeneration
when it changes.)

### 7. Simplify `-Amb` gloss matching (handoff item 6)

`app/mod.rs:~3585` (`apply_reader_gloss_highlighting`) and a regression test in
`text_file_map.rs` currently match glossed source lines by TEXT (not citation
tuple) because `-Amb` editions historically renumbered lines. That divergence is
gone: base and `-Amb` are byte-identical and gloss citations align with base
tuples (verified). Text-matching is still CORRECT (edition-identical text), so:

- Keep the text-match behavior (lowest risk — no behavioral switch).
- Remove the now-stale `-Amb`-renumbering rationale from the comments at both
  sites, replacing it with the current reality (editions are byte-identical;
  text-match retained as edition-robust and harmless).
- Add/keep a regression test asserting base/`-Amb` parity for a known passage so
  a future divergence is caught.

Do NOT switch to tuple-matching in this project — that is a separate, testable
change with no benefit to the feature.

### 8. Verification

Pure unit tests (`cargo test --bins`):

- §1: a stage `line_mapping` row maps to a `Line` with `sub_line > 0` and
  `is_dialogue == false`.
- §3: `build_line_map` over a fixture with interleaved stage `.txt` lines and DB
  stage rows populates `buffer_to_work` for the stage lines (incl. a multi-line
  stage direction) and keeps spoken-line mapping unchanged.
- §4: the DB-driven classifier returns stage=true / dialogue=false for a mapped
  stage line and falls back to regex for an unmapped line.
- §7: base/`-Amb` parity regression.

Headless / visual (user-run per CLAUDE.md Headless Verification — runtime/visual
criteria can't be unit-tested):

- `2H6`, `2H6-Amb`, `Ham`: stage directions render interleaved and italic in the
  reading card and in the gloss overlay (loading AND result card); the source
  turn carries them without `inject_stage_directions`.
- `,` `q` `y` `x` `g` `GG` and `j/k` skip stage rows and land on dialogue; the
  nav-fuzz lands on-page with balanced spreads.
- Glosses still highlight the correct passage lines on both base and `-Amb`.

## Out of scope

- Converting speaker/separator/act-scene classification to DB-driven (later
  project). Only stage-vs-dialogue is converted here.
- Switching `-Amb` gloss matching from text to tuple (kept as text-match).
- Any litdb / data change (done upstream).
- The "deleting a gloss should clear the main-card coloring" bug (separate,
  queued task).

## Risks

- **§3 and §4 depend on the buffer→DB bridge being correct for stage lines.** A
  wrong stage match would mis-anchor nav or the selection. Mitigation: raw-text
  1:1 match (verified byte-identical), plus the §3/§4 unit tests and the headless
  nav-fuzz.
- **§5 deletion.** Removing `inject_stage_directions` must not regress the
  result card; the headless gloss check on `2H6` is the gate.
- **Snapshot regeneration (§6).** First load of every work rebuilds its
  snapshot; confirm no startup regression on a large work (e.g. Bleak House
  prose path, which is unaffected by stage rows but still re-snapshots).

## Acceptance criteria

- Glossing a stage-bearing passage shows the stage directions interleaved and
  italic in both gloss cards, sourced from real DB rows (no
  `inject_stage_directions`).
- Stage directions render in position in the reading card for both `text_file`
  and DB-rendered works.
- `,` `q` `y` `x` `g` `GG` `j` `k` treat stage lines as non-dialogue via the
  mapped `Line` (DB-driven), regex only as the no-mapping fallback.
- `cargo build`, `cargo test --bins`, `cargo clippy` pass; `SNAPSHOT_VERSION` is
  9; glosses still highlight on base and `-Amb`.
