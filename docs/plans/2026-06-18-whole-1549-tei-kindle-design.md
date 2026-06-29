# Whole 1549 BCP: paragraph-level re-import + whole-book TEI, Kindle layout

**Status:** Design / scope (recommend + open questions; pending implementation plan)
**Supersedes (for 1549):**
`2026-06-16-bcp1549-modern-tei-design.md` (references repo) — the Matins-only +
`BCP1549M` approach is **abandoned**: no `BCP1549M` parallel work; the whole 1549
book is the target, and the modern text (derived from the justus 1549 source via
the existing modernization pipeline) lives in `BCP1549` itself.
**Related:**
`2026-06-18-bcp-kindle-text-file-rendering-design.md`,
`2026-06-16-bcp-decorative-typography-design.md`,
`2026-06-16-bcp-sentence-per-line-design.md` (this repo);
`2026-06-17-tei-to-text-render.md`, `2026-06-17-bcp1662-spoken-tei-design.md`
(references repo — the renderer + the 1662 precedent this mirrors).

## Goal

Render the **entire 1549 Book of Common Prayer** (all 18 rites) in the reader
with the Oxford BCP Kindle layout, by:

1. **Re-importing `BCP1549`** into lit.db so its `line_mapping` rows are
   **paragraph-level "canonical lines"** (a whole prayer/paragraph is one row),
   not the current hard-wrapped HTML fragments.
2. **Authoring whole-book TEI** for 1549 — one TEI per rite, modern spelling
   derived from the **justus** 1549 text via the existing modernization pipeline
   (justus is *original* spelling; the modern words are produced downstream, not
   read modern from justus — see Findings) — rendered to one whole-book `.txt`.
3. **Wiring `works.text_file`** for `BCP1549` to that `.txt`, so the reader's
   existing text_file path maps and styles it.

`BCP1549M` is abandoned. This is the **1662 pipeline, applied to 1549** — 1662
already does exactly this and works in production (see Precedent).

## Background — why this supersedes the prior 1549/Matins specs

The earlier 1549 spec produced a Matins-only modern work (`BCP1549M`, 142 rows)
+ `matins.xml`, and `2026-06-18-bcp-kindle-text-file-rendering-design.md` tried
to render Matins by pointing `BCP1549.text_file` at `matins-1549.txt`. That
failed (`mapped_buffer_lines=0`) for two reasons found in this investigation:

- **Wrong work scope.** `BCP1549.text_file` pointed at a *Matins-only* `.txt`,
  but `BCP1549` is the *whole book* (2853 rows). The matcher starts at row 0 and
  its 8-row skip window (`text_file_map.rs` `ACC_SKIP_WINDOW`) cannot reach
  Matins (~row 1160), so nothing maps.
- **Wrong spelling + wrong file.** `matins-1549.txt` is *original* spelling
  (`OURE`, `Mattyns`); the canonical `matins.txt` is *modern* (`OUR`, `Matins`)
  and aligns with `BCP1549M`, not `BCP1549`.

The user's decision: don't chase Matins-only; make the whole 1549 book a
paragraph-level work with whole-book TEI, modern spelling from justus, and drop
`BCP1549M`.

## The working precedent — BCP1662 (copy this)

BCP1662 already is what 1549 should become. Verified:

- **Per-rite TEI set:** `~/utono/literature/BCP/1662/TEI/01-preface.xml`,
  `02-of-ceremonies.xml`, … `31-articles.xml` (one TEI per voiced rite).
- **One whole-book `.txt`:** `bcp-1662-spoken.txt` (rendered from those TEIs;
  `tei_to_text.py` supports multi-file via `convert_many`).
- **Paragraph-level rows:** `BCP1662` has **677** `line_mapping` rows, e.g. id
  897435 is the **3294-char** Preface paragraph as ONE canonical line.
- **Wired + mapping:** `BCP1662.text_file = .../bcp-1662-spoken.txt`; renders via
  the reader's text_file path + `MatchMode::ParagraphAccumulate`.
- **Re-import script exists:** `scripts/reimport_bcp1662_spoken.py` (references
  repo) — the pattern the 1549 re-import mirrors.

So every piece of this plan has a 1662 analog already in the tree.

## Findings (load-bearing, with anchors)

### Current `BCP1549` granularity (the re-import problem)
- 2853 rows across 18 rites (`source_file` = the 18 justus `*_1549.htm` files:
  `front_matter`, `Of_Ceremonies`, `Kalendar` (1081!), `Matins_1549.htm` (148),
  … `Deacons`).
- Rows are **HTML fragments**, not paragraphs. The title page alone is 12 rows:
  `THE` / `booke of the common` / `prayer and admini-` / `nistracion of` / …
  This is the granularity the re-import must collapse.
- **0** rows carry `## ` headings; **436** carry `[...]` rubrics — so `BCP1549`
  was imported with the OLD line-level path, not `extract_blocks`.

### The HTML→rows extractor
- `scripts/bcp_html.py` (references repo): `extract_lines` (line-level, legacy)
  vs `extract_blocks` (`:135`) which already emits `## ` headings (`:187`) and
  `[...]` rubrics (`:188-189`, ≥`RUBRIC_MIN_WORDS` and ≥60% italic), and rejoins
  drop-cap initials (`:183-185`). **But** `extract_blocks` still splits at every
  `<br>`/block boundary (`:166-169`), so a hard-wrapped paragraph in the justus
  HTML becomes multiple rows. **Paragraph grouping does not exist yet** — it is
  the new work.
- `scripts/ingest_bcp.py`: one `works` row per edition; `div1` = 1-based rite
  index (`bcp_editions.EDITIONS`), `div2` NULL, `line_in_div` per row; calls
  `extract_blocks(path)` and inserts each returned string as a row (`:69-70`).
  `--reimport` deletes existing rows first (`:62-64`).

### justus sources (the text origin) — ORIGINAL spelling, modernized downstream
- `~/utono/literature/BCP/justus.anglican.org/1549/*.htm` — 18 per-rite files
  (= the `source_file` values). Public-domain (Charles Wohlers). **These are
  original/Tudor spelling** ("MATTYNS", "OURE father, whiche arte"). There is no
  modern justus file.
- **Modern spelling is produced downstream**, not read from justus: the
  references-repo pipeline `bcp_modernize.modernize_line` (spelling/punctuation
  only; vocabulary/grammar/word-order preserved) + `verify_modern.check_line`
  drift guard, bootstrapped per edition via `bootstrap_modern_table.py`, ingested
  by `ingest_bcp_modern.py`. This is what produced `BCP1559M`/`BCP1549M`.
- **Decision (user):** abandon the `BCP1549M` *parallel* work; the modern text
  should live in `BCP1549` itself. So the modernization pipeline still runs, but
  its output **re-texts `BCP1549`** rather than creating a `*M` sibling. Confirm
  in the plan (this changes `BCP1549` from original → modern spelling — see Open
  questions on echo/concordance impact).
- The README at `cummings-brian/1549/TEI/README.md` documents the one-way flow:
  justus → (modernize) → lit.db → hand-authored TEI → rendered `.txt`.

### The renderer is plain-text only (the Kindle-italics tension)
- `tei_to_text.py` / `2026-06-17-tei-to-text-render.md`: **no bold/italic/ANSI**
  ("ALL-CAPS is the only emphasis"); centering is leading spaces; rubrics are
  4-space indent; speakers inline (`Priest.  body`). The Oxford **Kindle** look
  (centered *italic* rubrics, centered *italic* speakers) **cannot** be carried
  by this `.txt`. Closing that gap needs reader-side TextTags keyed on markers —
  which is the linux-lit `apply_bcp_formatting` path, already proven on the DB
  path (`2026-06-16-bcp-decorative-typography-design.md`).

### Matcher / sync are granularity-agnostic
- `MatchMode::ParagraphAccumulate` (`text_file_map.rs:363`) re-accumulates
  consecutive physical `.txt` lines into one DB row; a paragraph-as-one-row maps
  trivially (degenerate 1:1). Timestamps/`u`/`.`/concordance key on
  `line_mapping.id`, never the physical line — so paragraph-level rows are safe
  for sync. Proven by BCP1662.

## Recommended design

### Phase A — paragraph-level re-import of `BCP1549`
Add **paragraph grouping** to the BCP extractor and re-import:
- Extend `extract_blocks` (or a new `extract_paragraphs`) so consecutive body
  lines that form one paragraph in the justus HTML collapse into **one** string,
  while headings (`## `), rubrics (`[...]`), speaker exchanges, and verse `<l>`
  lines stay as their own rows. Mirror 1662's paragraph rows.
- Re-import via `ingest_bcp.py BCP1549 --reimport` (delete + re-insert).
- **Row identity caveat (sharper than 1662).** `BCP1549` has **575
  `passage_embeddings`** (and 0 timestamps, 0 bookmarks). 1662's re-import
  (`reimport_bcp1662_spoken.py`) was an **in-place** `UPDATE ... SET
  canonical_text/normalized_text` keyed on `(div1, line_in_div)` + a DELETE of
  unspoken rows — it changed *text*, not *granularity* (row count ~unchanged), so
  embeddings stayed attached. **Collapsing fragments → paragraphs is harder:** it
  *reduces* row count (N rows → 1), so it cannot be a pure in-place update — the
  surviving paragraph row keeps one id, the merged-away fragment rows are deleted,
  and their 575 embeddings must be **re-embedded** (the embedding pipeline keys on
  the new paragraph rows). The plan must: pick the canonical surviving row per
  paragraph (first fragment's id, mirroring the matcher's "first sub-line
  canonical" rule), delete the rest, and **rebuild `passage_embeddings` for
  BCP1549** afterward (there is a rebuild path — see `rebuild-echo-embeddings`
  skill family). Bookmarks/timestamps are empty, so only embeddings are at risk.

### Phase B — whole-book 1549 TEI + `.txt`
- Author one TEI per rite under
  `~/utono/literature/BCP/cummings-brian/1549/TEI/` (e.g. `01-front-matter.xml`
  … `18-deacons.xml`), modern spelling from justus, structure from the Cummings
  images (priming only, not transcribed) — the `modernize-bcp-rite` workflow.
- Render all rites to one whole-book `.txt` via `tei_to_text.py` `convert_many`
  (the 1662 pattern), text must text-match the Phase-A paragraph rows.
- This is the **large** task (18 rites); recommend authoring rite-by-rite as
  vertical slices (Matins first — its TEI largely exists — then Evensong, …),
  each verified against its re-imported rows before the next.

### Phase C — wire + render
- Set `BCP1549.text_file` to the whole-book `.txt`; clear its snapshot
  (`~/.cache/linux-lit/snapshots/BCP1549.text.bin`).
- The reader's text_file path + `ParagraphAccumulate` map it (1662 proves this).
- **Kindle styling decision (open):** the plain `.txt` gives centered (space)
  text only. To get centered *italic* rubrics/speakers + small-caps like the
  Kindle, either (a) emit `## ` / `[...]` / a speaker marker in the `.txt` and
  run `apply_bcp_formatting` on the text_file path (extends
  `2026-06-18-bcp-kindle-text-file-rendering-design.md` trait #1/#2), or (b)
  accept plain centered text. Recommend (a) for fidelity — but it is a linux-lit
  reader change, separate from Phases A/B.

## Phasing
- **Phase A (litdb/references):** paragraph extractor + re-import. Self-contained.
- **Phase B (references/literature):** whole-book TEI authoring, rite by rite.
  Large; the bulk of the effort.
- **Phase C (litdb + linux-lit):** wire text_file; optional reader styling for
  the Kindle italics.

## Open questions (for the implementation plan)
- **Re-import strategy:** in-place re-text (preserve ids, 1662-style) vs
  delete+reinsert. Depends on whether `BCP1549` has timestamps/embeddings today.
- **Paragraph boundary rule:** what defines a paragraph in the justus HTML
  (blank line? `<p>`? heading/rubric/`<l>` as hard separators?). Needs a rule
  the extractor applies and the TEI `<p>` boundaries match.
- **Kalendar (1081 rows):** tabular almanac data — is it in scope for the
  Kindle layout, or excluded/special-cased? It dominates the row count and is
  not prose liturgy.
- **Kindle italics:** plain `.txt` (b) vs marker + reader styling (a).
- **Original vs modern in the same work:** the TEI text is modern; `BCP1549`'s
  current rows are original spelling. Re-importing to modern changes the work's
  spelling — confirm `BCP1549` should *become* modern (and how that affects
  echoes/concordance that may assume original spelling).

## Out of scope (this spec)
- The reader's pager wrapped-paragraph change (trait #3 of the sibling spec) —
  paragraph rows already soft-wrap; pagination of an over-tall paragraph is its
  own concern.
- Full justification (GTK dead-end). DB schema changes.
- Other editions (1559/1662 already done; 1559 modern is separate).
