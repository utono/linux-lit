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
- `bcp_modernize.py` / `bcp_modern_table.py` / `verify_modern.py` /
  `bootstrap_modern_table.py` / `ingest_bcp_modern.py` — the deterministic
  modern-spelling pipeline + drift guard (read-aloud Step A, route 1).
- `modern_spoken_txt.py` — lit.db rows → modern-spelling speakable `.txt` for
  TTS (read-aloud Step B, route 1): modernize + drop rubrics/glosses/markers,
  skip non-liturgical rites. Separate from `tei_to_text.py` (which renders the
  *display* `.txt` with markers for the reader).

Read-aloud / audio (lit.db + `~/utono/litdb/scripts/`):
- `media_manager.py` — register an mp3 (`media_files`) + `work_media_associations`.
- `align_monotonic.py` / `build_phrase_timestamps.py` — post-hoc timestamp
  alignment (alternative to ElevenLabs with-timestamps).
- Reader audio sync: `media_files`, `line_timestamps`, MPV integration in
  linux-lit `src/mpv/`, `src/main.rs`.
- Voice setup: `docs/guides/elevenlabs-v3-custom-voices.md`; lit.db
  `author_default_voice` / `voice_catalog`.

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

## Read-aloud: LLM modern-spelling + ElevenLabs mp3, synced to original text

A work like `BCP1549` is displayed in **original Tudor spelling** ("OURE father,
whiche arte in heaven"). To read it aloud, you do **not** want a TTS engine
sounding out Tudor orthography — you want it to *pronounce the modern equivalent*
("Our father, which art in heaven") while the reader highlights the original
line. The mechanism: store a **parallel modern-spelling line** for each canonical
line, synthesize one mp3 from the modern text, and attach per-line timestamps so
the reader's existing audio-sync highlights the original line as its modern audio
plays.

This reuses linux-lit's standard audio-sync path (`media_files` +
`line_timestamps`, the same one used for human-recorded audiobooks) — nothing
reader-side is new; only the *text fed to TTS* and the *timestamp source* differ.

### The data model

- **Modern text** lives in a parallel work `<ABBREV>M` (e.g. `BCP1549M`),
  **line-for-line aligned** with the original: identical `(div1, line_in_div)`,
  same row count, modern `canonical_text`. (This mirrors the existing
  `ingest_bcp_modern.py` / `BCP1559M` convention — but see the alignment note
  below now that the original is paragraph-level.)
- **The mp3** is one `media_files` row (`work_abbrev = <ABBREV>`,
  `source_text_path` = the modern `.txt` it was synthesized from), associated to
  the work via `work_media_associations`.
- **Timing** is one `line_timestamps` row per original `line_mapping_id`
  (`media_id`, `start_time`, `end_time`). The reader keys sync off the ORIGINAL
  line's id, so the highlight tracks the original-spelling text while the
  modern-pronunciation audio plays.

> **Alignment caveat (current state).** The legacy `ingest_bcp_modern.py` assumes
> the modern work is line-for-line with the *source rows*. Since `BCP1549` was
> re-imported to **paragraph-level** (this guide's Phase A), the modern work must
> be built against those same paragraph rows — one modern row per original
> canonical line — not the old fragment numbering. Generate the modern rows from
> the current `line_mapping`, preserving `(div1, line_in_div)` exactly.

### Step A — produce the modern spelling with an LLM (Claude API)

Two routes; pick by fidelity needs:

1. **Deterministic table (existing, cheap, drift-guarded).**
   `bcp_modernize.modernize_line` + `bcp_modern_table` (spelling/punctuation
   only, word-count-parity preserved, `verify_modern.check_line` drift guard).
   The table is bootstrapped *once* via Claude over residual Tudor tokens
   (`bootstrap_modern_table.py`, needs `ANTHROPIC_API_KEY`). Best when you want
   guaranteed structural parity and reproducibility.
2. **Direct LLM per canonical line (what this section adds).** Send each original
   `canonical_text` to the Claude API and ask for the **modern-spelling
   equivalent only** — same words, same order, only orthography/punctuation
   modernized. Better for irregular text the table misses; the cost is one API
   call per line (batch them).

LLM contract (keep it strict so the audio matches the page):
- **Modernize spelling and punctuation ONLY.** Never change vocabulary, grammar,
  or word order ("there be not three incomprehensibles" stays).
- **Preserve markers verbatim**: a leading `## `, a `[...]` rubric wrapper (and a
  leading `¶`), and the `@ ` speaker marker pass through unchanged — modernize
  only the inner words. (`modernize_line` is already marker-aware; an LLM prompt
  must be told the same.)
- **One line in, one line out.** Do not split or merge; the modern row must map
  1:1 to its original by `(div1, line_in_div)`.
- **Latin stays Latin.** Rubric tags like "Venite exultemus" / "Te Deum
  Laudamus" are not English; leave them.

Sketch (Claude Messages API; batch ~20 lines/call, low temperature):

```python
import os, anthropic, sqlite3
client = anthropic.Anthropic(api_key=os.environ["ANTHROPIC_API_KEY"])
db = sqlite3.connect(LIT_DB)
rows = db.execute(
    "SELECT div1, line_in_div, canonical_text FROM line_mapping "
    "WHERE work_abbrev='BCP1549' ORDER BY div1, line_in_div").fetchall()

SYSTEM = (
  "Modernize the SPELLING and PUNCTUATION ONLY of each numbered Tudor English "
  "line. Keep every word, its meaning, and word order. Preserve a leading "
  "'## ', '[' ... ']' brackets, a leading '¶', and a leading '@ ' exactly. "
  "Leave Latin unchanged. Return the same count of lines, numbered identically, "
  "one modern line each — no commentary.")

# For each batch: send numbered originals, parse numbered modern lines back,
# assert count parity, then write modern rows preserving (div1, line_in_div).
```

Validate every batch: **count parity** (modern lines == originals) and a
**word-count-per-line** check (a modern line should have the same word count as
its original — the drift guard's core invariant). Reject and retry any batch that
drifts; never let a split/merge through (it breaks the 1:1 timing map).

Write the modern rows as work `<ABBREV>M` with the SAME `(div1, line_in_div)` as
the originals (back up lit.db first).

### Step B — render the modern read-aloud text

Produce a plain modern `.txt` in `(div1, line_in_div)` order — one physical line
per modern canonical line, markers stripped (TTS should not speak `## ` / `[` /
`@ `; for a *reading* you typically also skip rubric lines, or speak them in a
quieter aside — decide per use). This is the text handed to ElevenLabs and stored
as `media_files.source_text_path`.

`scripts/modern_spoken_txt.py` (route 1, deterministic) implements exactly this:
it reads a work's `line_mapping` rows, runs `bcp_modernize.modernize_line` on
each, drops whole-row rubrics, strips short inline editorial glosses (keeping the
long bracketed liturgical additions), removes the `## ` / `@ ` / `^` / `*`
markers, and (default) skips the non-liturgical rites (front matter, calendar,
articles, colophons). One line per spoken row, rites blank-line separated:

```bash
PYTHONPATH=. python scripts/modern_spoken_txt.py --abbrev BCP1549 \
    --out ~/utono/literature/BCP/cummings-brian/1549/TEI/bcp-1549-modern-spoken.txt
```

`--all-rites` keeps the apparatus rites; `--show-kept-brackets` prints the
substantive bracketed passages retained, for review. (For route 2, send the same
rows to the Claude API instead of the table; the rest of Steps C–E is identical.)

### Step C — synthesize the mp3 with ElevenLabs (with timestamps)

Use the ElevenLabs **`/v1/text-to-speech/{voice_id}/with-timestamps`** HTTP
endpoint (model `eleven_v3` for quality, or a v2.5 model for speed). Unlike the
plain MCP `text_to_speech` tool, the with-timestamps endpoint returns the mp3
**plus per-character alignment** (`characters`, `character_start_times_seconds`,
`character_end_times_seconds`) — exact timings derived from the synthesis itself,
so no transcription/whisperX pass is needed.

- Voice: resolve via the work's author voice (`author_default_voice` /
  `voice_catalog`); see `docs/guides/elevenlabs-v3-custom-voices.md` for the v3
  voice setup. One narration voice for the whole work is the simplest start.
- Synthesize per chunk that stays within the model's character limit (e.g. one
  rite, or N lines), keeping a running offset so timings are continuous across
  chunks; concatenate the mp3 parts. Record which output character index begins
  each input line so you can map alignment → lines.
- ⚠️ Paid API. Synthesize once; the mp3 + timestamps are then cached in lit.db.

### Step D — attach mp3 + per-line timestamps to lit.db

1. Register the mp3 with `litdb/scripts/media_manager.py` (`associate`): inserts
   the `media_files` row (`work_abbrev`, `source_text_path`) and a
   `work_media_associations` row.
2. Convert the with-timestamps alignment into one `line_timestamps` row per
   ORIGINAL `line_mapping_id`: for each modern line, take the char-range that
   produced it, read its first char `start_time` and last char `end_time`, and
   write `(line_mapping_id = the ORIGINAL row's id, media_id, start_time,
   end_time)`. Because the modern work is `(div1, line_in_div)`-aligned to the
   original, mapping modern-line → original-id is a lookup, not a transcription.
   (If you instead synthesized per chunk and want post-hoc alignment, the litdb
   `align_monotonic.py` / `build_phrase_timestamps.py` path also works, but the
   with-timestamps response is exact and preferred.)

### Step E — read aloud

With `media_files` + `line_timestamps` populated, the reader's existing MPV-driven
sync plays the modern-audio mp3 and advances the highlight line-by-line over the
**original-spelling** display. No reader change is required — this is the same
mechanism as a human audiobook, with the audio sourced from modern-spelling TTS.

### Why this works

The original spelling is what the eye reads (faithful to the page images); the
modern spelling is only ever the *input to pronunciation*, never displayed. The
1:1 `(div1, line_in_div)` alignment is the hinge: it lets a modern-text mp3's
timings address original-text rows, so display and audio stay in lockstep.

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
- **Modern read-aloud work not yet built for BCP1549.** The read-aloud pipeline
  above is documented but not yet run for the paragraph-level `BCP1549` (the old
  `BCP1549M` was line-for-line with the *fragment* rows and was abandoned). It
  must be regenerated against the current paragraph rows.
- **Rubric/Latin in read-aloud.** Decide per work whether to speak `[...]`
  rubrics (instructions) and Latin canticle tags at all, or skip/aside them; the
  modern `.txt` for TTS (Step B) is where that choice is applied.
