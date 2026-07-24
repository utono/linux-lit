# Richer inline rendering for prose works — design (linux-lit)

**Status:** **Facet 2 (verse rendering) is a FINISHED reader-side design** as of
2026-07-24 — ready for `superpowers:writing-plans` (see "Facet 2 — reader design
(RESOLVED)"). **Facet 1 (inline italics) remains a deferred brainstorming
starter** — its own spec→plan cycle later; the offset hazard it carries is why it
was split out. The two facets are NO LONGER coupled (see "Scope decision" below).

**Date:** 2026-07-23 (Facet-2 reader design resolved 2026-07-24)

## Scope decision (2026-07-24) — facets split, verse first

The original starter assumed one shared tagging pass for both facets. Grounding
the design in the actual reader code (`build_line_map`, `formatting.rs`,
`phrase_highlight.rs`, `viewport.rs`) changed the risk picture and the two facets
are now **decoupled into separate spec→plan→implement cycles**:

- **Facet 2 (verse) is LOWER-risk and goes FIRST.** It rides the reader's
  EXISTING `LineMap` row-split/fold mechanism (BCP splits one DB row into N
  buffer lines; folded stage directions collapse N rows into one). Pagination
  (`viewport.rs`) counts buffer lines by real pixel geometry, never DB rows — so
  a verse row expanding to N lines paginates correctly *by construction*. The
  400-row LoJ trial data is render-testable today.
- **Facet 1 (italics) is HIGHER-risk and follows.** Hiding `_` delimiters shifts
  buffer offsets that `phrase_highlight`/vocab/search index into — a genuine
  offset-remap problem. It is corpus-wide (PP, MobyDick, Ibsen, …), not LoJ-only.
  Deferred so the easy, testable verse work does not wait on the hard one.

**This document's FINISHED design covers Facet 2 only.** Facet 1 sections below
are preserved verbatim as the deferred follow-up's starting point.

**Paired spec (litdb):** Facet 2 below depends on a **verse-preserving
reimport** of LoJ that is specified separately in litdb:
`docs/superpowers/specs/2026-07-23-loj-verse-preserving-reimport-design.md`
(Phase 0). This spec covers only the **reader/rendering** side.

## The umbrella problem

The reader renders `prose`/`prose_book` works as **uniform wrapped prose** with
no inline distinctions. Two kinds of inline structure real prose works carry are
lost on screen:

1. **Italics** (`_word_` markup) — render as literal underscores.
2. **Verse blocks embedded in prose** (epitaphs, quoted poems, Latin verse) —
   render as ordinary wrapped prose, losing line breaks and verse typography.

Both surfaced from *Life of Johnson* (LoJ). **They are NOT symmetrical:**

- **Facet 1 (italics) is purely a reader feature.** The `_..._` markup is
  already in the DB (verified 1:1 against the HTML `<i>` tags). The reader just
  needs to render it.
- **Facet 2 (verse) depends on litdb Phase 0.** The current LoJ import collapsed
  verse blocks into flat space-joined rows — line breaks and indentation are gone
  from the data. The reader cannot render structure that is not present. The
  verse-preserving reimport (litdb spec) must land first; this spec covers what
  the reader does with the restored structure.

Note (verified against the code): rendering a prose work from a `.txt` file
instead of DB rows changes only *alignment* (the `LineMap`), **not** how lines
render — both paths build the buffer via `lines.join("\n")` + uniform prose
wrapping. `.txt` fixes neither facet. See "Why not `.txt`".

The two facets share one root: the prose reading buffer has no inline
sub-structure. Solve the sub-paragraph tagging once for both.

---

## Facet 1 — Inline italics (`_word_`)

Gutenberg / Ibsen editions delimit italics with paired underscores (`_word_`).
The markup lives in `line_mapping.canonical_text` and the reader shows it
**verbatim** — the buffer is filled by `state.buffer.set_text(...)` with no markup
parsing — so underscores render literally (`SECTION IV. _A TALE OF A TUB_.`).

For *A Tale of a Tub* (TT) the markup was mostly heading decoration and was
**stripped** in lit.db (litdb `strip-italic-underscores` skill). LoJ is
different: its italics are overwhelmingly **meaningful** and should be
**rendered**.

### Why LoJ must render, not strip

Of 22,140 paired `_..._` spans in LoJ's 9,707 marked rows:

- **10,403 work/book titles** — *Life*, *Memoirs of Mr. William Whitehead*,
  *London, a Poem, in Imitation of the Third Satire of Juvenal*.
- **5,204 multi-word phrases** — *His leaf also shall not wither*.
- **5,212 single-word emphasis** — *farrago*, *verbatim*.
- **544 Latin / foreign** — *Ex alieno ingenio Poeta, ex suo tantum versificator*.
- ~800 citation abbrevs / roman numerals / currency (`_ib_`, `_s_`).

Stripping flattens "he wrote *London*" (poem) into "he went to London" (city).

### Goal

Render prose `_..._` runs as **inline italic spans**: delimiters not shown, the
enclosed run styled `pango::Style::Italic`. Everything that reads the buffer
(copy, search, vocab, karaoke, pagination) must agree on the displayed text.

### Scope (corpus-wide, not LoJ-only)

Every prose/Ibsen work still carrying the markup:

- `prose_book`: LoJ 9707
- `prose`: PP 244, MobyDick 180, TTC 88, TWWLN 9, VF 3, BH 1, BH-Barrett 1,
  BH-Margolyes 1, BH-Vance 1, DC 1
- `play` (Ibsen): WildDuck 638, DollsHouse 296, HeddaGabler 25, MasterBuilder
  23, Ghosts 14, Eyolf 5, DeadAwaken 4
- `epic_translation`: Il 1

TT was stripped and is not listed. **Open question:** do Ibsen plays opt in?
Their underscores are stage-direction emphasis inside `[...]` lines that
`apply_dialogue_formatting` ALREADY italicizes whole-line — inline italics there
may be redundant or conflicting. Decide per work_type.

### The central design tension: buffer offsets

Hiding delimiters **shifts every character offset after them**. Many consumers
index the buffer by offset / `iter_at_line` + column and must share one
coordinate system. The delimiters exist in `canonical_text` but must NOT exist in
the displayed buffer — two coordinate spaces (source vs displayed) that today are
identical and would diverge.

Integration points that read the buffer by position (grep-confirmed; verify):

- `src/app/text_prep.rs` — builds `filtered_contents`, calls `buffer.set_text`;
  `is_prose` gate. Where delimiter stripping + a source↔display offset map would
  be produced.
- `src/app/formatting.rs` — `apply_dialogue_formatting` applies TextTags
  **per line** (stage-direction italic, BCP rubrics). Prose falls through to a
  plain `dialogue-indent` branch. Inline italic needs a NEW per-span tag pass
  keyed off the offset map.
- `src/input/actions/word_copy.rs` — `extract_buffer_line_words` uses
  `iter_at_line` + `trim_matches(!alphanumeric)`. Offsets must line up.
- `src/input/phrase_highlight.rs` — karaoke highlight over char ranges, from
  **buffer offsets** (`buffer_line_text`, `iter_at_line`, `buffer.text`). Hiding
  delimiters shifts its coordinates. **Highest-risk consumer.** **Precedent:**
  this file ALREADY reconciles an identical hazard — see its "translations hidden
  (inflated buffer misaligns offsets)" logic. Follow / generalize that approach
  rather than inventing a new offset map.
- `src/app/vocab_popup.rs` / vocab highlight — vocab spans matched/tagged by
  position.
- `src/input/search.rs`, `overlay_search.rs`, `corpus_search.rs` — a query for
  `verbatim` must match the displayed (delimiter-free) text.
- `src/ui/pagination.rs` — page breaks from buffer geometry; confirm.

**Core question:** strip delimiters at `set_text` and maintain a source→display
offset map every consumer consults, OR keep one coordinate space another way? The
offset map is the likely answer; name who owns it and who consults it.

### Data edge cases (from lit.db survey)

- **Unbalanced underscores:** 61 of LoJ's 9,707 rows have an ODD `_` count. The
  parser must NOT italicize to end-of-line on an unpaired `_`. Rule? (Recommend:
  render literal + count, so the data defect stays visible.)
- **Word-internal welds:** `letter_letter` (`John_son_`, `See_post_`) — litdb
  strip guard skips these (8 in LoJ). For rendering, `John_son_` → italic `son`?
  Or treat as upstream data defect.
- **Nested / adjacent runs:** `_A_, _B_` vs `_A _B_ C_`. Pick a non-greedy pair
  rule (`_([^_]+)_`) and document it.
- **Currency/measure:** `120_l_.`, `£322 10_s._` — italic `l`/`s`; renders as a
  normal span, no special case.

### What NOT to do (Facet 1)

- **Do not convert `_x_` → `*x*` in lit.db.** The buffer parses no markdown;
  asterisks render literally.
- **Do not strip LoJ in lit.db** (unlike TT) — keep the markup as the source.
- **Do not reuse whole-line `stage_italic_tag`** — that tags entire lines; inline
  italic needs sub-line character ranges.

### Facet-1 non-goals

- Bold, small-caps, or other markup — only italic `_..._`.
- Re-italicizing works already stripped (TT).
- Changing how plays/BCP already italicize whole lines.

---

## Facet 2 — Verse blocks embedded in prose (rendering)

**Depends on litdb Phase 0** (verse-preserving reimport) having restored verse
line breaks + indentation + markers to `line_mapping`. This section is the
reader's job: render those marked blocks as verse.

The reference image (Penguin LoJ) shows the target: line-broken verse, 2-tier
indentation, centered small-caps speaker cues (`MELIBŒUS.`), centered italic
verse titles (*Translation of* VIRGIL.), stanza breaks — interleaved with
justified prose.

### Producer contract — RESOLVED by litdb (2026-07-24)

The litdb Phase 0 plan is written and executed (`~/utono/litdb/docs/superpowers/
plans/2026-07-23-loj-verse-preserving-reimport.md` + `2026-07-24-loj-llm-
classification.md`). It resolves this spec's Q4 marker-contract question
concretely — the reader must consume THIS shape, not the earlier guesses:

- **Marker = a row flag, not a block-range table.** New column
  `line_mapping.block_type TEXT NOT NULL DEFAULT 'prose'` ∈
  `{'prose','verse','heading'}`. Non-LoJ rows stay `'prose'` (whole-corpus safe).
- **One `line_mapping` row per BLOCK, not per verse-line.** A multi-line verse
  block is a SINGLE row whose `canonical_text` contains **embedded `\n`** between
  verse lines, with **leading spaces** preserved as the indent (2 tiers, from
  `<br>`+`&nbsp;` → `  `). Chosen because it aligns 1:1 with the whisperX
  transcript's stanza-level segments. **This is the key shape the reader must
  absorb:** today one `line_mapping` row ≈ one display line; a verse row now
  carries its own multi-line structure inside one id.
- **Indent = literal leading spaces in the text, NOT a tier integer.** The reader
  derives tiers by counting leading spaces per `\n`-split line. Something must
  ensure those spaces survive the reader's line-trimming (linux-lit trims lines —
  see the two-column-split-trim behavior).
- **`heading` conflates speaker cues AND titles AND chapter/section heads.** The
  producer collapses `MELIBŒUS.` (speaker), `Translation of VIRGIL.` (verse
  title), and chapter heads all into `block_type='heading'`. The reader must
  distinguish them ITSELF (by content/context) if it wants centered-small-caps
  speaker vs centered-italic title styling — or accept both as centered. Headings
  come from `<h1>`–`<h5>` (not only `<h5>` — vol4/vol5 use `<h2>`).
- **Italics stay `_…_` in `canonical_text`** (Facet 1 confirmed — LoJ is NOT
  stripped; `<i>`→`_…_` at import). Facet 1's contract is unchanged.
- **Flat structure:** `div1=1, div2=0`, sequential `line_in_div`; VOLUME `div1`s
  re-derived afterward. `normalized_text` = `canonical_text` with `\n`→space,
  punctuation stripped, lowercased (so alignment still matches the transcript).
- **`normalized_text` note:** the aligner (`map_gutenberg_timestamps.py`) reads
  `canonical_text` and re-normalizes on the fly, so a verse row's embedded `\n`
  does not break alignment — verse is timed at BLOCK (stanza) granularity.

**Reader-side shape this introduces:** a verse `line_mapping` id spans MULTIPLE
visual lines. The "New reader-side risk" the starter feared here turned out to be
an ALREADY-SOLVED pattern (see the reader design below) — the reader's `LineMap`
handles exactly this cardinality (BCP, folded stage directions), and pagination
counts buffer lines by pixel geometry. It is a known shape, not novel machinery.

---

## Facet 2 — reader design (RESOLVED 2026-07-24)

Finished, plan-ready design. Grounded in the actual reader code (citations are
the state at design time; re-confirm during implementation). **This cycle renders
`block_type='verse'` blocks + uniformly-centered `block_type='heading'` rows.**

### 1. Scope & activation

- **Data-driven, no gate.** Rendering keys off `block_type` read from the DB.
  Non-LoJ works are all `block_type='prose'` (migration default) and render
  byte-identical to today. A work gains verse rendering exactly when its rows
  carry verse/heading types. The `block_type` flag IS the gate — no env/config
  flag, no per-work allowlist.
- **Read the new column.** `block_type` has ZERO references in `src/` today. Add a
  `block_type: String` field to `Line` (`src/db/models.rs:43`) and select it in
  `load_work` (`src/db/queries.rs`). Default `"prose"` if absent (older DBs).
- **Facet 1 (inline italics) is OUT of this cycle** — deferred (see Scope
  decision). Italics stay as literal `_…_` in verse/prose text for now.

### 2. The spine — a verse-aware `LineMap` builder

The correctness core. Today LoJ (`prose_book`, `text_file=NULL`, not BCP) hits the
DEFAULT buffer-fill path (`src/app/mod.rs:4425`): `line_map = None`, a strict 1:1
`join("\n")`. A verse row's embedded `\n` WOULD create extra buffer lines while
`line_map = None` keeps a positional 1:1 buffer↔work identity — so every consumer
keying off `work_line_for_buffer` (timestamps, sync, `u`/`.`, concordance)
desyncs by one per extra verse line. This is the exact hazard the BCP path's
comment (`mod.rs:4383–4386`) documents and already solves.

**Design: apply the BCP pattern to a new trigger.** Add a branch in
`rebuild_buffer_text` BEFORE the default fallback, active when `work.lines`
contains any non-`prose` `block_type`. Walk the rows:

- **verse** row → split `canonical_text` on embedded `\n`. For each split line,
  record its leading-space count (for the indent tier), then strip the leading
  spaces from the DISPLAYED line. Push each resulting line to the buffer; every
  one maps back (`source_index[b] = wi`) to the single source row.
- **heading / prose** row → one buffer line, mapped 1:1. Headings keep their text
  (centering is a formatting concern, not a text concern).

Build the map via a NEW `build_line_map_blocks` (sibling to `build_line_map_bcp`
in `src/text_file_map.rs`), so the many-buffer-lines-to-one-row mapping is
authoritative. Pagination (`src/input/viewport.rs`, `visible_range`) already
counts buffer lines by pixel geometry, so verse expansion paginates correctly by
construction — as BCP sentence-splits do today. The per-line leading-space counts
are handed to the formatting pass (§3); the literal spaces are gone from display.

### 3. Verse typography — the formatting pass

New `apply_block_typography` in `src/app/formatting.rs`, run after buffer fill for
any work that took the block-aware path. It iterates buffer lines, using the
`LineMap` to resolve each buffer line's source row + `block_type`, and applies
whole-line TextTags via the established `iter_at_line` + line-start/line-end idiom
(`apply_dialogue_formatting`, `formatting.rs:228–270`):

- **verse lines** → a per-line `left_margin` tag sized by tier (leading-space
  count → 0/2/4 spaces = tier 0/1/2 = base + tier×indent_px). Each verse line is
  its own buffer line, so no paragraph-wrap concern. Stanza spacing via
  `pixels_above_lines` at the block's first line (and trailing gap after its
  last). `left_margin` is used because GTK collapses leading whitespace and
  TextTag `indent`/justify are unreliable (project convention — see
  `GtkTextView justify/indent limits`).
- **heading rows** → centered small-caps, UNIFORMLY (no content heuristic —
  honors the one-flag producer contract; avoids text-inference per the reader's
  "authoritative metadata, not text inference" rule). Composed from existing
  primitives: the `speaker-name` SmallCaps variant + a Center-justification tag
  (as `stanza-number-center` already does).

**Reuses existing primitives** — sub-line char-range tagging (`apply_bcp_formatting`
small-caps/divine-name spans, `formatting.rs:479–541`) and whole-line margin/style
tags both already exist. The only NEW tags are per-tier `left_margin` variants.

**`layout.rs` is untouched.** The LoJ work stays `is_prose_work=true`, so its
card margins/gutter stay prose (correct). Verse indent here is SUB-paragraph,
layered via tags — not a work-level `is_verse` mode switch.

### 4. Karaoke over verse — whole-block tint

A verse block is one `line_mapping` id, one block-granularity timestamp, N buffer
lines. `phrase_highlight.rs` tints per-line within single-line rows; its
established response to "buffer diverges from timestamp granularity" is to DISABLE
(the translations-hidden gate at `phrase_highlight.rs:290/323/523`), not remap.

- **When playback/cursor is on a verse block, tint the ENTIRE block** (all N
  buffer lines) as one active unit — block granularity, matching its single
  timestamp. Reuses the line-tint machinery across the block's buffer-line range
  (resolved via the `LineMap`). Prose around it highlights normally.
- Verse in prose stays narrated at BLOCK (stanza) granularity — no per-verse-line
  timestamps (Facet-2 non-goal, unchanged).

### 5. Heading style — uniform (decided)

All `block_type='heading'` rows render identically (centered small-caps). The
producer conflates speaker cues, verse titles, and chapter heads into one flag;
splitting them by content heuristic is explicitly NOT done this cycle (text
inference the reader's conventions warn against). Speaker cues render correctly;
verse titles lose their italic lead; chapter heads read as centered small-caps —
acceptable, and a cheap follow-up if the on-screen check says otherwise.

### 6. Testing & acceptance

- **TDD the spine (mandatory — LineMap/pagination TDD gate).** `build_line_map_blocks`
  unit tests BEFORE implementation, mirroring `folded_multiline_stage_direction_maps_to_its_rows`
  (`text_file_map.rs:1252`): a verse row splits into N buffer lines all mapping
  back to its one source row; leading-space counts captured; prose/heading stay
  1:1; `effective_line_count()` = expanded buffer-line count; `work_line_for_buffer`
  correct at every boundary (the sync/`u`-`.`/concordance invariant).
- **Pure-helper tests** (`cargo test --bins`): leading-space→tier mapping;
  block-boundary detection driving stanza spacing.
- **Headless on-screen verification (non-optional gate).** Drive the cage/grim
  harness to LoJ; screenshot a page with a known verse block (the Horace ode —
  one row, 4 lines, one timestamp, confirmed live in the 400-row trial). Open the
  PNG; confirm by eye + pixel-measure: verse line-broken (not wrapped), 2-tier
  indent renders, headings centered, nothing clips (clip-prevention ledger).
  Confirm sync: whole-block tint on the verse, prose unaffected. Hand the user the
  real-GL e2e command for a final eyeball (cage is software rendering).
- **Regression guard:** a non-LoJ prose work (BH or PP) screenshots IDENTICAL to
  pre-change — proving the `block_type='prose'` default path is untouched.

### Facet-2 open questions — ALL RESOLVED

- Q4 marker contract → litdb producer contract (above). **Resolved.**
- Multi-visual-line verse rows → `build_line_map_blocks`, BCP pattern (§2).
  **Resolved** (not novel — an existing shape).
- Heading split → uniform centering, no heuristic (§5). **Resolved.**
- Indent survival → count leading spaces → per-tier `left_margin` tag (§2–3).
  **Resolved.**
- Karaoke over verse → whole-block tint at block granularity (§4). **Resolved.**
- Rollout → data-driven, no gate (§1). **Resolved.**
- Shared tagging pass with Facet 1 → NO; facets decoupled (Scope decision).
  **Resolved.**

### Why the reader has no mechanism today

`is_verse` is derived from **`work_type`** alone
(`src/app/layout.rs`: `let is_verse = !is_prose_work(&work_type)`). It is a
**whole-work** property: a `prose_book` renders 100% prose typography. There is
**no per-line or per-block verse/prose mechanism** in the main reading buffer.
That mixed-typography capability is what this facet adds.

### How Gutenberg's formats encode LoJ (surveyed 2026-07-23)

Checked the richer formats for a semantic verse layer to emulate. There is none:

- **Italics = `<i>`** (4,333 in vol1), 1:1 with the DB `_..._`. Facet 1 needs no
  re-fetch.
- **Verse = NO semantic markup.** No `.verse`/`.poem`/`.linegroup` class in HTML
  or EPUB3. Verse is a `<p>` of `<br>`-delimited lines with `&nbsp;&nbsp;`
  indentation (2 tiers); speakers are `<h5>MELIBOEUS.</h5>`; titles are
  `<p><i>Translation of</i> VIRGIL. Pastoral I.</p>`.
- **Plain `.txt` flattens** `<br>` to line breaks and `&nbsp;` to leading spaces
  — the signals litdb Phase 0 detects.

Implication: "emulate the EPUB formatting" means reproducing the *visual* result
(line-broken, 2-tier indented block, centered speaker/title), NOT importing a
class that doesn't exist.

### The rendering feature

Given Phase 0's markers, `formatting.rs` applies, for prose works, sub-paragraph
verse typography to marked blocks:

- **Verse lines** (`block_type='verse'`) — line-broken on the row's embedded `\n`,
  no paragraph-wrap, preserve the indent from each line's leading spaces.
- **Speaker cues** (`MELIBŒUS.`) — centered small-caps (reuse the speaker-name
  tag primitive, but centered). NOTE: the producer marks these `block_type=
  'heading'`, same as titles/chapter-heads — the reader must split `heading` by
  content to pick this style (see Producer contract + open Q5).
- **Verse titles** (*Translation of* VIRGIL.) — centered, italic lead + caps/
  small-caps author. Also `block_type='heading'` (see above).
- **Stanza breaks** — blank-line spacing between stanzas.
- **Centering hint:** Gutenberg's `.agate`/`.xhtml_center` classes mark monument
  inscriptions/epitaphs as centered — some blocks want centering, not just indent.

This reuses the whole-work verse typography `layout.rs` already produces, scoped
to a block via the marker.

### Shared machinery with Facet 1

Both facets need `formatting.rs` to apply **sub-paragraph tags to prose** —
italic character spans (Facet 1) and verse rows expanded to line-broken,
leading-space-indented display lines (Facet 2). Design one tagging pass that
handles both, not two bolt-ons.

### Facet-2 non-goals

- Full verse *alignment* (per-verse-line timestamps) — verse in prose stays
  narrated at paragraph granularity; typography only.
- Detecting verse in the reader — the marker comes from litdb Phase 0; the reader
  trusts it.
- Building a semantic verse layer Gutenberg doesn't provide.

---

## Why not `.txt`

Redirected here from a "reimport LoJ to render from `.txt` like a Dickens novel"
idea. Findings that closed it:

- **No prose novel renders from `.txt`.** BH, TWWLN, PP, Cromwell all have
  `text_file=NULL` and render from DB rows — that IS the Dickens-novel convention.
- **`text_file` drives alignment, not prose rendering.** Both paths build the
  buffer identically; `.txt` changes neither italics nor verse layout.
- **Edition mismatch anyway.** The audio is the 51h **unabridged** reading
  (~475k transcript words). Gutenberg offers only Osgood (#1564, abridged ~220k,
  <half the audio) and Birkbeck Hill (6-vol unabridged + ~1.1M words of apparatus
  the narrator skips). Neither is a clean 1:1 prose-txt source.

The verse fix is a data reimport (litdb Phase 0, still DB rows) + this reader
feature — NOT a switch to `.txt` rendering.

---

## Open questions — status

**Facet 2 (verse) — ALL RESOLVED** (see "Facet 2 — reader design" above and its
"Facet-2 open questions — ALL RESOLVED" list). Ready for `writing-plans`.

**Facet 1 (inline italics) — DEFERRED to its own spec→plan cycle.** These remain
open and belong to that later cycle, NOT this one:

1. **One coordinate space or two?** If two, who owns the source↔display offset
   map, and where (alongside `LineMap`)? NOTE: the code map found NO existing
   offset-remap to reuse — `phrase_highlight`'s "translations hidden" precedent
   DISABLES the feature rather than remapping (gates at `phrase_highlight.rs:290/
   323/523`). So the cheap proven move is disable-not-remap; weigh that against a
   real offset map.
2. **Ibsen opt-in** for italics — given their whole-line stage-direction italic
   that `apply_dialogue_formatting` already applies? (litdb did NOT touch this —
   reader decides, per work_type.)
3. **Unpaired-underscore policy** (render literal / drop / log). 61 LoJ rows have
   an odd `_` count. (litdb did NOT touch this — reader decides.)
4. **Sub-line char-range tagging** exists already (`apply_bcp_formatting`
   small-caps/divine-name spans) — the italic pass should follow that idiom.

## References

- Reference image: Penguin Classics LoJ layout (user-provided 2026-07-23).
- litdb Phase 0 — the producer of the verse/heading structure this feature
  renders (spec + both plans; contract now RESOLVED, see above):
  - spec: `~/utono/litdb/docs/superpowers/specs/2026-07-23-loj-verse-preserving-reimport-design.md`
  - reimport plan: `~/utono/litdb/docs/superpowers/plans/2026-07-23-loj-verse-preserving-reimport.md`
  - LLM-classification plan (block_type set by an LLM, no human gates):
    `~/utono/litdb/docs/superpowers/plans/2026-07-24-loj-llm-classification.md`
- litdb strip tooling (TT path; italics fallback if Facet 1 declined):
  `~/utono/litdb/scripts/strip_italic_underscores.py`,
  `~/utono/litdb/.claude/skills/strip-italic-underscores/SKILL.md`.
- Whole-line italic precedent: `src/app/formatting.rs` (`stage_italic_tag`,
  `apply_bcp_formatting`).
- Whole-work verse typography precedent: `src/app/layout.rs` (`is_verse`
  branches: indent, line numbers, margins).
- Gutenberg format survey: HTML #8918
  `https://www.gutenberg.org/cache/epub/8918/pg8918-images.html`.
