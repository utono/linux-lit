# Creating a Kindle-like layout for BCP (and similar) works

This guide documents the full, repeatable pipeline that turns a public-domain
liturgical/structured text — plus reference page images like those in
`~/utono/literature/BCP/cummings-brian/1549/` — into a work that renders in the
**linux-lit reader with the Oxford BCP Kindle layout**: centered decorative
headings, centered-italic rubrics, speaker cues on their own centered-italic
line, small-caps divine names, and wrapped/justified prose paragraphs.

It was developed for the 1549 Book of Common Prayer (work `BCP1549`) but is
written to generalize to any similarly-structured work (other BCP editions,
liturgies, breviaries, or any source with heads / stage-directions /
call-and-response / verse).

## TL;DR — the pipeline

```
justus HTML  --bcp_html.extract_blocks-->  classified blocks (## / [ ] / @ / body)
classified blocks  --html_to_tei-->  first-pass per-rite TEI  (one block = one element)
                       (refine by hand against the page IMAGES)
per-rite TEI  --tei_to_rows-->  lit.db line_mapping rows   (canonical_text)   ── SOURCE OF TRUTH
per-rite TEI  --tei_to_text (convert_many)-->  one whole-book .txt (display)
works.text_file = the .txt   →   linux-lit maps .txt → rows (ParagraphAccumulate)
                                  and styles via apply_bcp_formatting
```

**The TEI is the single source of truth.** Both the lit.db rows and the display
`.txt` are derived from the same TEI, so they agree by construction and the
reader's text-matcher maps the `.txt` onto the rows 1:1. Never hand-edit the
`.txt` or the rows directly — edit the TEI and regenerate.

## What the page images are for

The Cummings (Oxford World's Classics) page images at
`~/utono/literature/BCP/cummings-brian/<year>/IMG_*.PNG` are a **structural and
typographic reference only** — you read them to decide WHERE each element falls
(which lines are heads, rubrics, speaker exchanges, psalm verses; where
small-caps and drop-caps sit) and what the target layout looks like.

**Do NOT transcribe the images.** The wording always comes from the
public-domain `justus.anglican.org` source (already in lit.db / the HTML), never
from OCR of the copyrighted edition. Images prime the structure; the text is
justus. This keeps the work inside the repo's copyright guardrail.

## The data model the reader expects

linux-lit's reader (`src/app.rs::apply_bcp_formatting`,
`src/db/line_types.rs`) keys styling off **text markers in the canonical_text**,
not off HTML or visual cues. One `line_mapping` row per liturgical unit:

| canonical_text form | meaning | reader styling |
|---|---|---|
| `## Text` | heading / rite title / canticle title | centered, bold, larger |
| `[Text]` or `[¶ Text]` | rubric (stage direction) | centered or hanging italic |
| `@ Speaker.` | speaker cue on its own line | centered italic (cue), marker stripped for display |
| plain text | prayer / response / verse line | wrapped body; small-caps openers & divine names |

The markers (`## `, `[ ]`, `@ `, `¶`) all normalize away during text-matching,
so the display `.txt` (which renders them as centering / italics / indentation
instead) still matches the rows. The reader strips `## ` and `@ ` for display;
rubric brackets are handled by the rubric tag.

Key reader facts (so you do not re-derive them):
- `apply_bcp_formatting` re-derives every line type from the **mapped work-line's
  original canonical_text** (which keeps the markers), not from the displayed
  buffer — so it works identically on the DB path and the text_file path.
- The gate runs `apply_bcp_formatting` for ANY `is_bcp_work` (abbrev starts with
  `BCP`), on both paths.
- Marker constants are shared: reader `line_types::BCP_SPEAKER_MARKER` ==
  pipeline `tei_to_rows.SPEAKER_MARKER` == `"@ "`.
- Full (Fill) justification is a GtkTextView dead-end; the Kindle "justified"
  look is approximated by wrapped paragraphs + centering, not true justify.

## Step-by-step

All scripts live in `~/utono/ws-book-of-common-prayer-references/scripts/`. Run
with `PYTHONPATH=.` from that repo. **Back up the shared lit.db before any
write** (`~/utono/litdb/data/lit.db`).

### 0. Prerequisites
- The original-spelling work must already be ingested in lit.db
  (`ingest_bcp.py <ABBREV>` from the justus HTML manifest in `bcp_editions.py`).
  Confirm: `sqlite3 lit.db "SELECT COUNT(*) FROM line_mapping WHERE work_abbrev='<ABBREV>'"`.
- The justus HTML sources exist under
  `~/utono/literature/BCP/justus.anglican.org/<year>/` and are listed per rite
  in `scripts/bcp_editions.py` (`EDITIONS[<ABBREV>]["rites"]`, `dir`).

### 1. Paragraph-level extraction (`bcp_html.extract_blocks`)
The justus HTML is fragment-level (a paragraph hard-wrapped with `<br>`; a title
split across many lines). `extract_blocks` collapses it to paragraph-level
blocks and classifies each:
- `<br>` inside a block → a space (paragraph stays ONE block); block elements
  (`<p>`, `<div>`, headings, `<li>`, `<tr>`) end a block.
- Heads: `<h1>-<h3>`, `<center>`, or `<font size="+1|+2|+3">` (non-italic) → `## `.
- Speaker cues: a centered italic small-font div
  (`<div align="center"><i><font size="-1">Aunswere.</font></i></div>`) — a
  short label ending in `.` → `@ `.
- Rubrics: a mostly-italic block of ≥ `RUBRIC_MIN_WORDS` words → `[ ]`
  (`¶`-prefixed when it leads with a pilcrow).
- Drop-cap initials are re-glued; chrome/editorial noise dropped.

These signals are the justus 1549 conventions; **a new source may use different
markup** — adjust `_heading_texts` / `_speaker_texts` / the rubric heuristic to
its conventions (the classification is the only source-specific part).

### 2. Bootstrap a first-pass TEI (`html_to_tei.py`)
```bash
PYTHONPATH=. python scripts/html_to_tei.py --abbrev <ABBREV> --div1 <N> \
    --out ~/utono/literature/BCP/cummings-brian/<year>/TEI/<NN>-<slug>.xml
```
Wraps each classified block in a TEI element (`## `→`<head>`, `[ ]`→`<rubric>`,
`@ ` + next block → `<sp><speaker>/<p>`, else `<p>`). The text is verbatim from
the justus source, so the round-trip is text-preserving (the generated TEI's
`tei_to_rows` output equals `extract_blocks`). This is a **bootstrap** — it does
not infer `<lg>`/`<l>` psalm/canticle verse structure or fix ambiguous markup.

### 3. Refine the TEI by hand against the images (the craft step)
Open the rite's page images and bring the bootstrap up to full fidelity:
- Mark **psalms/canticles** as `<lg type="psalm|canticle">` with `<head>` and
  one `<l>` per verse (keep the Tudor mediation `: ` as text; glorias get
  `type="gloria"`).
- Wrap **small-caps** divine names and canticle/collect opening words in
  `<hi rend="sc">` where the image shows them.
- Fix call-and-response that the bootstrap bracketed as rubrics but are really
  responses (e.g. the Litany's italic "Good lorde deliver us." lines).
- Catch heads the source styled differently (e.g. a `<p align="center">`
  subtitle that was not big-font).
- `04-matins.xml` is the worked reference example — copy its structure.
- Validate: `xmllint --noout <rite>.xml`.

### 4. Rebuild the lit.db rows from the TEI (`reimport_bcp1549_tei.py`)
```bash
\cp -f ~/utono/litdb/data/lit.db ~/utono/litdb/data/lit.db.bak-$(date +%Y%m%dT%H%M%S)
PYTHONPATH=. python scripts/reimport_bcp1549_tei.py --div1 <N> --tei <rite>.xml [--dry-run]
```
Per-rite: DELETEs that rite's `div1` rows and re-inserts one row per TEI block
(`tei_to_rows.rite_rows`). Other rites are untouched, so author/refine one rite
at a time. Original spelling — no modernization.

### 5. Render the whole-book display `.txt` (`tei_to_text.convert_many`)
```bash
PYTHONPATH=. python -c "import sys;sys.path.insert(0,'scripts'); \
from tei_to_text import convert_many; from pathlib import Path; \
d=Path('~/utono/literature/BCP/cummings-brian/<year>/TEI'.replace('~',str(Path.home()))); \
Path(d/'bcp-<year>.txt').write_text(convert_many(sorted(d.glob('[0-9][0-9]-*.xml'))))"
```
Renders heads centered, rubrics indented/centered-italic, `@ ` cue lines,
small-caps as ALL-CAPS, paragraphs one-physical-line (the reader soft-wraps).
Per-rite TEI files are named `NN-slug.xml` so glob order == rite order.

### 6. Wire it into the reader
```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "UPDATE works SET text_file='<…>/TEI/bcp-<year>.txt' WHERE abbrev='<ABBREV>'"
rm -f ~/.cache/linux-lit/snapshots/<ABBREV>.text.bin   # force a rebuild
```
With `text_file` set, linux-lit loads the `.txt`, matches each line onto the rows
via `MatchMode::ParagraphAccumulate`, and styles via `apply_bcp_formatting`.

### 7. Verify
- **Mapping gate (must be 100%)**: every lit.db row must be reachable by
  accumulating consecutive `.txt` lines. Simulate `ParagraphAccumulate`:
  normalize both, walk the `.txt` accumulating until the running text equals the
  row. `matched == total rows`, `unmatched == 0`.
- **Tests**: `python -m pytest -q` (pipeline) and, in linux-lit,
  `cargo test --bins`.
- **Visual**: `cd ~/utono/linux-lit && cargo run`, open the work, page through,
  compare to the page images. The agent cannot reliably screenshot the live
  session — ask the user to eyeball.

## Generalizing to other works

The pipeline is edition- and work-agnostic; only two pieces are
source-specific:

1. **The HTML→blocks classifier** (`bcp_html.py`) keys on the justus markup
   conventions (`<font size>` heads, centered-italic-div speakers, italic-fraction
   rubrics). A different source needs its own detection rules — keep the OUTPUT
   contract (`## ` / `[ ]` / `@ ` / plain) and the rest of the pipeline is reused
   unchanged.
2. **The hand-refinement** (step 3) is always per-work craft against the images.

Everything downstream (TEI → rows, TEI → `.txt`, the matcher, the reader
styling) is shared. To onboard a new work: ingest its original-spelling source,
point `bcp_editions.py` at its rite files, adapt the classifier if its markup
differs, then run steps 2–7 per rite.

For a brand-new rite with no prior lit.db work, see the `modernize-bcp-rite`
skill (`.claude/skills/modernize-bcp-rite/`) for the upstream ingest + (optional
modern-spelling) path; this guide picks up once the original-spelling rows exist.

## Reference: file inventory

Pipeline (`~/utono/ws-book-of-common-prayer-references/scripts/`):
- `bcp_editions.py` — per-edition rite manifest (div1 order, source `dir`, files).
- `ingest_bcp.py` — first ingest of the justus HTML → original-spelling rows.
- `bcp_html.py` — `extract_blocks`: HTML → classified paragraph-level blocks.
- `html_to_tei.py` — classified blocks → first-pass per-rite TEI.
- `tei_to_rows.py` — TEI → lit.db `canonical_text` rows (source of truth).
- `tei_to_text.py` — TEI(s) → display `.txt` (`convert` / `convert_many`).
- `reimport_bcp1549_tei.py` — per-rite DB rebuild from the TEI.

Reader (`~/utono/linux-lit/src/`):
- `db/line_types.rs` — `is_bcp_work`, `is_bcp_heading`, `is_rubric`,
  `rubric_is_centered`, `is_bcp_speaker`, `strip_bcp_speaker_marker`,
  `BCP_SPEAKER_MARKER`, `divine_name_spans`.
- `app.rs` — `apply_bcp_formatting` (the styling pass), the BCP gate in
  `apply_dialogue_formatting`, `clean_file_lines` (marker stripping for display),
  the DB-path buffer build in `display_work`.
- `text_file_map.rs` — `MatchMode::ParagraphAccumulate` (maps `.txt` → rows).

Data (`~/utono/literature/BCP/`):
- `justus.anglican.org/<year>/*.htm` — source text (original spelling).
- `cummings-brian/<year>/IMG_*.PNG` — structure/typography reference images.
- `cummings-brian/<year>/TEI/NN-slug.xml` — per-rite TEI (source of truth).
- `cummings-brian/<year>/TEI/bcp-<year>.txt` — rendered whole-book display file.

Related specs (`docs/superpowers/specs/`): `2026-06-17-tei-to-text-render.md`,
`2026-06-16-bcp1549-modern-tei-design.md`; in linux-lit:
`2026-06-18-whole-1549-tei-kindle-design.md`,
`2026-06-16-bcp-decorative-typography-design.md`.

## Known limitations / refinement backlog

- **Bootstrap misses verse structure.** `<lg>`/`<l>` psalm/canticle markup and
  `<hi rend="sc">` small-caps are added by hand (step 3); a fresh bootstrap has
  heads/rubrics/speakers/paragraphs only.
- **Litany call-and-response** renders as bracketed rubrics, not speaker cues
  (the responses are italic but not in centered-italic divs). Needs a per-rite
  pass or a Litany-specific classifier rule.
- **Some subtitles** styled as `<p align="center">` (not big-font) are not
  detected as heads by the bootstrap.
- **Source hyphenation** ("admini- nistracion", "through- out") survives from the
  justus line-break artifacts; de-hyphenate in the TEI where it matters.
- **Decorative ornaments** (leaf glyphs ❧, drop-caps) are not rendered — GTK
  TextTags cannot inject glyphs; deferred.
- **Echo embeddings** are keyed by line range; after a re-import they are stale
  and must be rebuilt (`embed_bcp.py <ABBREV> --reembed`, needs `VOYAGE_API_KEY`).
