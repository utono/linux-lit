# Finishing the reader side for per-line verse (linux-lit) — design

**Status:** finished design — ready for `superpowers:writing-plans` (Phase A
first). Phases B and C are scoped here but each becomes its own plan.

**Date:** 2026-07-24

**Supersedes:** the **Facet-2 (verse) portion** of
`docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md`. That design
targeted the **block-granularity** LoJ data model (one `line_mapping` row per
verse *block*, embedded `\n`, one block-level timestamp) and the reader code that
shipped from it (commits `e1d5c762`…`ec2dc056`) still assumes that shape. The
litdb per-line reimport (below) changed the data model, so the shipped reader now
runs against data its design did not anticipate. This spec finishes the reader
side for the **per-line** model. The 2026-07-23 spec's Facet-1 (inline italics)
material is carried into **Phase B** here.

**Paired producer (litdb) — DONE:** the per-line verse reimport is complete.
- spec: `~/utono/litdb/docs/superpowers/specs/2026-07-24-loj-per-line-verse-timing-design.md`
- plan: `~/utono/litdb/docs/superpowers/plans/2026-07-24-loj-per-line-verse-timing.md`
  (its "Reader handoff" appendix holds the verified findings this spec builds on)

## What the data model changed to

The litdb reimport replaced block-granularity verse with **one `line_mapping` row
per verse LINE**. As now in lit.db for all 6 LoJ volumes (21,543 rows,
div1=1..6):

- A verse row's `canonical_text` is **one verse line**, leading spaces preserved
  (the reader derives the indent tier from them). No embedded `\n`.
- A mid-poem stanza break is **one empty `verse` row** (empty/whitespace
  `canonical_text`). It holds no timestamp and renders as the stanza gap.
  (Vol1 has zero of these — its verse has no internal blank-line breaks — but
  later volumes may.)
- Each verse row has **its own timestamp** (14,449 rows timed whole-work; vols
  1–4 + vol5 tail narrated, vols 5–6 index/appendix ~untimed by design).
- `block_type` ∈ `{'prose','verse','heading'}` unchanged; no 4th value.

This is a **net simplification** of what the reader was built for: a verse row is
now 1:1 with a display line (the case the reader already handles for prose), and
per-line timestamps make per-line seek and per-line karaoke fall out naturally.

## Why a new spec (not an edit of the old one)

The old Facet-2 design is internally coherent for a model that no longer exists;
editing it in place would leave contradictory block-vs-line statements. This spec
supersedes that portion cleanly and is grounded in the current reader code
(citations are the state at design time — re-confirm during implementation).

## Scope & phasing

Three phases, sequenced by readiness and risk. **Each phase gets its own
implementation plan.** Phase A is ready now and low-risk; it does not wait on B
or C.

- **Phase A — per-line verse finish (linux-lit, ready now, low-risk):** per-line
  rendering + indent tiers, empty-verse-row stanza gap, per-line cursor seek, and
  retiring the now-wrong block-granularity machinery.
- **Phase B — inline italics `_word_` (linux-lit, higher-risk, own plan):** the
  Facet-1 offset-remap work, carried from the 2026-07-23 spec. Corpus-wide.
  Decoupled from A.
- **Phase C — verse karaoke made correct + verified (cross-repo, data-gated):**
  reader line-by-line verse karaoke, blocked on a litdb `phrase_timestamps`
  backfill for LoJ.

---

## Phase A — per-line verse finish

### A1. Data contract (what the reader consumes)

Each verse row = one display line, leading spaces = indent signal. Empty verse
row = stanza gap (no timestamp). Each verse row independently timestamped.
Headings (`block_type='heading'`) are one row each, centered (unchanged from the
shipped design — uniform centering, no content heuristic). Non-LoJ works are all
`block_type='prose'` and render byte-identical to today.

### A2. `prepare_block_buffer` — retire the verse `\n`-split

`src/app/text_prep.rs` (`prepare_block_buffer`, ~line 231). Today:

```rust
if crate::db::line_types::is_verse_line(&l.block_type) {
    for vline in l.text.split('\n') {
        let (tier, n) = leading_space_tier(vline);
        buf_lines.push(vline[n..].to_string());
        source_index.push(wi);
        indent_tiers.push(tier);
    }
} else { /* one buffer line, tier 0 */ }
```

The inner `split('\n')` loop is now vestigial — a per-line verse row splits into
exactly one line. Retire the loop for verse: one buffer line per row directly,
tier from that row's own leading spaces (strip the leading spaces from the
displayed line, record the tier). The `source_index`/`indent_tiers` invariants
(non-decreasing, every work-line index emitted exactly once) are preserved.

**Empty verse row** (`l.text` empty/whitespace) yields **one empty buffer line,
tier 0** — the same result `split('\n')` already produces on `""` (→ `[""]`,
`leading_space_tier("")` → `(0,0)`). The retirement is behavior-preserving;
locked by a test (A6). The empty-line-preserving behavior is **load-bearing** —
do not "optimize away" empty buffer lines, or the stanza gap and the 1:1
source-index contract break.

### A3. `build_line_map_blocks` — unchanged; per-line seek falls out

`src/text_file_map.rs` (`build_line_map_blocks`, ~line 243). It maps buffer↔work
purely by `source_index` structure (collapses runs of equal source index), with
**no text matching** — so an empty verse row flows through with a real
`buffer_to_work` entry (not filtered), and with one buffer line per row the map is
1:1. Therefore **per-line cursor seek "just works"**: a seek/nav to verse line N
resolves to work row N and lands on THAT line's own timestamp, not the stanza's
first row. No code change; a test locks the 1:1 mapping (A6).

### A4. Empty verse row = stanza gap (the one new rendering rule)

`src/app/formatting.rs` (`apply_block_typography`, verse branch ~lines 708–715).
Today every verse row gets `verse-indent-{tier}` and (on source change)
`verse-stanza-gap`. New rule: an **empty** verse row gets **only** the
`verse-stanza-gap` tag (blank vertical space) — no `verse-indent-*`, no
cursor/karaoke target:

```rust
if crate::db::line_types::is_verse_line(bt) {
    if line.text.trim().is_empty() {
        // stanza-gap separator: gap tag only, no indent / no tint target
        state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
    } else {
        let tier = state.block_indent_tiers.get(bl).copied().unwrap_or(0);
        state.buffer.apply_tag_by_name(&format!("verse-indent-{tier}"), &start, &end);
        if prev_src != Some(wi) {
            state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
        }
    }
} else if crate::db::line_types::is_heading_line(bt) { /* unchanged */ }
```

`prev_src` still updates to `Some(wi)` for the empty row, so the next stanza's
first line also gets its gap tag — harmless (adjacent gap tags collapse to one
visual separator) and consistent with the existing contiguous-run logic. The
`verse-stanza-gap` tag already exists (`ensure_block_typography_tags`,
`formatting.rs:~652`).

**Cursor must skip the gap.** The empty buffer line still exists (to keep the 1:1
source-index↔row contract), so cursor/seek navigation must NOT rest on it: a
move that would land on an empty verse row advances to the nearest non-empty
verse (or prose/heading) line in the travel direction. The plan specifies the
skip against the actual navigation code (the `u`/`.`-style line moves and the
audio-cursor placement) — an empty verse row is a visual separator, never a
navigation stop. Since vol1 has zero empty verse rows, the plan's headless proof
of this rule uses a later volume (or a synthetic fixture) that has one.

### A5. Retire the block-granularity tint machinery

The whole-block **cursor** tint was already reverted (`fc936aa1`,
`1357dc27`) — cursor tint is line-by-line on verse, correct and unchanged.
Phase A additionally retires the whole-block **karaoke** path as the verse model:
`block_buffer_range` (`src/input/phrase_highlight.rs`, ~line 185) and the
"tint the whole block" branch exist only to serve "one block = one timestamp = N
buffer lines," which per-line data eliminates. Phase A stops the code asserting
block granularity for verse (verse karaoke, when it renders, is line-by-line like
prose — Phase C). Removing dead paths is in scope; the guard is that the
non-LoJ/prose karaoke behavior is byte-identical after the change.

### A6. Testing & acceptance (Phase A)

- **TDD, mandatory (LineMap/buffer contract):**
  - `prepare_block_buffer_empty_verse_row_is_one_blank_line_tier0` — an empty
    verse row → one empty buffer line, tier 0, its own source index
    (`buf_lines == ["…A", "", "…B"]`, `source_index == [0,1,2]`,
    `indent_tiers == [0,0,0]`).
  - `build_line_map_blocks_per_line_verse_maps_each_line_to_own_row` — N all-verse
    rows, strictly-increasing `source_index` → `buffer_to_work == [Some(0)..]`,
    `work_to_buffer == [0..]` (1:1; per-line seek invariant).
  - (Exact test bodies are in the litdb plan's Reader-handoff appendix — reuse
    them verbatim.)
- **Pure-helper tests** (`cargo test --bins`): `leading_space_tier` mapping;
  empty-line preservation through the retired-split path.
- **Headless on-screen proof (non-optional gate):** drive the cage/grim harness
  (`test-headless-navigation` skill / the protocol in `~/utono/linux-lit/CLAUDE.md`)
  to a real LoJ verse passage. Confirm by eye + pixel-measure: each verse line on
  its own line at the correct indent tier; a stanza gap renders as blank vertical
  space (use a volume/fixture that has an empty verse row); seeking to a specific
  verse line lands the cursor on THAT line (its own timestamp); the audio cursor
  tracks per verse line; cursor nav skips the gap.
- **Regression guard:** a non-LoJ prose work (BH or PP) screenshots IDENTICAL to
  pre-change (the `block_type='prose'` default path is untouched), and prose
  karaoke is unchanged after A5.

### Phase-A non-goals

- Inline italics (Phase B).
- Verse karaoke rendering (Phase C — needs phrase data).
- Any change to non-verse/non-LoJ rendering.

---

## Phase B — inline italics (`_word_` → italic span)

Higher-risk; its own implementation plan. Carried from the 2026-07-23 spec's
Facet-1 sections (those remain the detailed reference; summarized here).

### B1. Goal

Render prose `_..._` runs as inline italic spans — delimiters hidden, the
enclosed run styled `pango::Style::Italic`. LoJ's italics are overwhelmingly
meaningful (~10.4k work/book titles, ~5.2k phrases, ~5.2k single-word emphasis,
~544 Latin/foreign), so LoJ renders (not strips, unlike TT). Everything that
reads the buffer (copy, search, vocab, karaoke, pagination) must agree on the
displayed (delimiter-free) text.

### B2. The central hazard (why B is separate from A)

Hiding `_` delimiters **shifts every character offset after them**. Consumers
that index the buffer by offset must share one coordinate system:
`src/input/phrase_highlight.rs` (**highest-risk**), vocab
(`src/app/vocab_popup.rs`), search (`search.rs`/`overlay_search.rs`/
`corpus_search.rs`), word-copy (`src/input/actions/word_copy.rs`), pagination
(`src/ui/pagination.rs`). Italics touch character-span offsets; Phase-A verse
touches line structure — they share only that both add sub-paragraph tags in
`formatting.rs`, so B is cleanly decoupled and A ships without it.

### B3. Approach decision (for the Phase-B plan)

Two candidates: (a) strip delimiters at `set_text` and maintain a
source→display **offset map** every consumer consults; (b) the **disable-not-remap**
precedent `phrase_highlight` already uses for hidden translations
(`phrase_highlight.rs` gates ~290/323/523). The code survey found NO existing
offset-remap to reuse — the proven move is disable-not-remap. The Phase-B plan
must survey whether a real offset map is unavoidable before committing, name who
owns the map and who consults it, and follow the sub-line char-range tagging idiom
that already exists (`apply_bcp_formatting` small-caps/divine-name spans).

### B4. Data edge cases & scope (from the 2026-07-23 survey)

Corpus-wide, not LoJ-only: PP 244, MobyDick 180, TTC 88, TWWLN 9, VF 3, BH/
BH-Barrett/BH-Margolyes/BH-Vance 1 each, DC 1, Il 1, and Ibsen plays (WildDuck
638, DollsHouse 296, HeddaGabler 25, MasterBuilder 23, Ghosts 14, Eyolf 5,
DeadAwaken 4). **Open questions for the Phase-B plan:** Ibsen opt-in (their `_` is
stage-direction emphasis already whole-line italicized by
`apply_dialogue_formatting` — may be redundant/conflicting); unpaired-underscore
policy (61 LoJ rows have an odd `_` count — recommend render-literal + log so the
data defect stays visible); word-internal welds (`John_son_`); nested/adjacent
runs (pick non-greedy `_([^_]+)_`).

### Phase-B non-goals

Bold/small-caps/other markup (only italic `_..._`); re-italicizing already-stripped
works (TT); changing how plays/BCP already italicize whole lines; converting
`_x_`→`*x*` in lit.db (buffer parses no markdown).

---

## Phase C — verse karaoke made correct + verified (data-gated)

### C1. The blocker (why karaoke "isn't working in LoJ" today)

LoJ has **0 `phrase_timestamps`** rows. Karaoke highlighting sweeps over
phrase-timed spans; with no phrase data the sweep has nothing to light
(`spans=0`) — exactly what the 2026-07-23 spec observed. This is a **data gap,
not a reader bug**. The prerequisite is a litdb `backfill-phrase-timestamps` run
for LoJ (media_id 233), which is a separate litdb step (still pending, always
after the full reimport — now satisfied).

### C2. Reader side (make it line-by-line correct)

With per-line timestamps + phrase data, verse karaoke uses the **same line-by-line
sweep as prose** — no block machinery (retired in Phase A). The old
whole-block-vs-line-by-line debate is dissolved by the per-line data: each verse
line is its own timed unit, so line-by-line is the only coherent model. The reader
work is "verse karaoke behaves like prose karaoke," not new machinery.

### C3. Sequencing (the plan must enforce)

1. litdb: `backfill-phrase-timestamps` for LoJ (media 233) → phrase rows exist.
2. linux-lit: verse karaoke renders line-by-line (verify no block-granularity
   assumption survives from before Phase A).
3. Headless on-screen proof: karaoke sweep tracks the audio line-by-line over a
   narrated LoJ verse passage; prose karaoke unchanged.

**Phase C cannot complete its on-screen gate until step 1 lands** — the spec
names this so C is not started before its data exists.

### Phase-C non-goals

Per-verse-line karaoke tuning beyond parity with prose; any karaoke change for
non-verse works.

---

## Cross-repo dependencies (stated once)

- **Phase A, Phase B** — pure linux-lit; no litdb dependency. A is ready now.
- **Phase C** — depends on litdb `backfill-phrase-timestamps` for LoJ (media 233).
  Do not start C's on-screen work before that data exists.

## Carry-forward this retires

- litdb plan's "per-line seek deferred upstream" note → **done by Phase A** (the
  1:1 map gives per-line seek). Retire it when Phase A ships.

## References

- Superseded Facet-2 design (and the detailed Facet-1 reference for Phase B):
  `docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md`.
- Producer (litdb) per-line reimport spec + plan (DONE):
  `~/utono/litdb/docs/superpowers/specs/2026-07-24-loj-per-line-verse-timing-design.md`,
  `~/utono/litdb/docs/superpowers/plans/2026-07-24-loj-per-line-verse-timing.md`
  (Reader-handoff appendix = the verified findings + verbatim test bodies for A6).
- Shipped verse-block reader commits (block-granularity, being finished here):
  `e1d5c762`…`ec2dc056`; cursor-tint revert `fc936aa1` / `1357dc27`.
- Reader code touch points: `src/app/text_prep.rs` (`prepare_block_buffer`),
  `src/text_file_map.rs` (`build_line_map_blocks`), `src/app/formatting.rs`
  (`apply_block_typography`, `ensure_block_typography_tags`),
  `src/input/phrase_highlight.rs` (`block_buffer_range`), `src/db/line_types.rs`
  (`is_verse_line`/`is_heading_line`).
- Headless verification: `test-headless-navigation` skill; cage/grim protocol in
  `~/utono/linux-lit/CLAUDE.md`.
