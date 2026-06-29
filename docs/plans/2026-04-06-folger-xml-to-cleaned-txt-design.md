# Folger XML to Cleaned TXT Extraction

**Date:** 2026-04-06

## Goal

Create a one-time Python script that extracts clean play text from the Folger Shakespeare TEI XML files (`~/utono/literature/shakespeare-william/folger-xml/*.xml`) and writes cleaned `.txt` files to `~/utono/literature/shakespeare-william/folger-cleaned/`.

The cleaned files strip all editorial preamble (title, editors, character lists, dedications, arguments, prefaces) and produce a format that linux-lit can load directly — no runtime filtering needed.

## Output Format

**Plays** (37 files with `<sp>` elements):

```
## Prologue

[Enter the Prologue in armor.]

PROLOGUE
In Troy there lies the scene. From isles of Greece
The princes orgulous, their high blood chafed,

[Prologue exits.]

## Act 1, Scene 1

[Enter Pandarus and Troilus.]

TROILUS
Call here my varlet; I'll unarm again.
Why should I war without the walls of Troy

PANDARUS
Will this gear ne'er be mended?
```

**Poems** (Son, Ven, Luc, PhT — 4 files with no `<sp>` elements):

```
1

From fairest creatures we desire increase,
That thereby beauty's rose might never die,

2

When forty winters shall besiege thy brow
And dig deep trenches in thy beauty's field,
```

Poems use the div2 `n` attribute as a section header (sonnet number, stanza number).

## Format Rules

- **Headers**: `## Act N, Scene N` for plays. `## Prologue`, `## Epilogue`, `## Induction` for special sections. Sonnet/stanza numbers as bare number on own line for poems.
- **Speakers**: bare name on own line, ALL CAPS, extracted from `<speaker>` element text
- **Stage directions**: `[text]` in brackets, extracted from `<stage>` elements
- **Verse lines**: one line per `<milestone unit="ftln">`, text reconstructed from `<w>`, `<c>`, `<pc>` children
- **Blank lines**: one blank line between speeches, one blank line before stage directions between speeches
- **Stripped content**: `<teiHeader>`, `<div1 type="preface">`, `<div1 type="dedication">`, `<div1 type="argument">`, character lists, all `===` separators
- **Kept content**: prologues, epilogues, inductions, choruses — these are part of the play

## XML Structure Summary

The Folger TEI XML uses milestone-based lineation (not `<l>` elements):

- `<div1 type="act|prologue|epilogue|induction">` — top-level structural divisions
- `<div2 type="scene">` with `n` attribute — scenes within acts
- `<sp who="...">` — speeches containing `<speaker>` and `<ab>` (anonymous block)
- `<stage type="entrance|exit|...">` — stage directions, can appear inside or between speeches
- `<milestone unit="ftln" n="1.1.1" ana="#verse|#prose|#short">` — marks a line of text
- `<w>`, `<c>`, `<pc>` — individual words, spaces, punctuation

Text reconstruction: collect all `<w>`, `<c>`, `<pc>` text between consecutive `<milestone>` elements (or between a milestone and the next structural boundary).

## Script Design

**Language:** Python 3, stdlib only (`xml.etree.ElementTree`)

**Location:** `~/utono/literature/shakespeare-william/folger-xml-to-cleaned.py`

**Filename mapping:** The script needs a mapping from XML abbreviation (e.g., `Tro`) to the kebab-case name used in folger-txt (e.g., `troilus-and-cressida`). The cleaned files use the same kebab-case names: `troilus-and-cressida.txt`.

**Algorithm for plays:**

1. Parse XML, find the TEI namespace
2. Skip `teiHeader` entirely
3. For each `<div1>` in the body:
   - Skip `type="preface"`, `type="dedication"`, `type="argument"`
   - For `type="act"`: emit `## Act N` (from `n` attribute)
   - For `type="prologue|epilogue|induction"`: emit `## Prologue` etc.
4. For each `<div2>` within an act:
   - Emit `## Act N, Scene M` (combining parent act and scene `n`)
5. Walk children of each div2 (or div1 for prologues):
   - `<stage>`: extract text from w/c/pc children, emit `[text]`
   - `<sp>`: extract speaker from `<speaker>`, emit speaker name, then process `<ab>` for lines
   - Within `<ab>`: each `<milestone unit="ftln">` starts a new line. Collect w/c/pc text until next milestone or end of ab.

**Algorithm for poems:**

1. Same header/namespace handling
2. For each `<div2>` (sonnet/stanza), emit the `n` attribute as a header
3. Walk milestones and reconstruct lines the same way as plays

**Edge cases:**

- `<q>` (quotation) elements contain `<w>`/`<c>`/`<pc>` — treat transparently, just extract text
- `<foreign>` elements — extract text normally
- `<seg>` (songs, letters) — extract text normally
- Inline `<stage>` within an `<ab>` — always emit as `[text]` on its own line
- `<lb>` (line break) elements — ignore, lineation comes from milestones
- `<head>` elements within div2 — ignore (e.g., "Scene 1" headers, we generate our own)
- Stage directions with `<w>` elements that have `n` attributes matching a milestone — these are part of the stage direction text, not verse lines

## Validation

After generation, compare each cleaned file against its original folger-txt counterpart:

- Line count should be similar (cleaned will be shorter due to stripped preamble)
- Spot-check a few files: first speech, last speech, a mid-play speech should match
- Run linux-lit's line map matching against the cleaned files — match percentage should be >= 96%

## DB Update

After generating cleaned files, update the `text_file` column in `lit.db` for each work to point to the new `folger-cleaned/` path. This is a simple SQL UPDATE per work.

The script should also emit these UPDATE statements (or run them directly) so the process is repeatable.

## linux-lit Changes

The cleaned format is designed to be compatible with the existing `line_types.rs` classification:

- `## Act N, Scene N` — matches `is_act_scene_marker` (starts with "Act" or "Scene" after case folding). Need to add `##` prefix detection.
- Speaker names — unchanged, `is_speaker` works as-is
- Stage directions — unchanged, `is_stage_direction` works as-is
- No `===` separators — `is_separator` won't match anything, which is fine

The only required change: `is_act_scene_marker` should also match lines starting with `## `. And `is_separator` can stay as-is (harmless). The blank-line-before-speaker stripping in `rebuild_buffer_text` can be removed since the cleaned files won't have extraneous blanks.

## Scope

- Python extraction script (one file, ~200-300 lines)
- 42 cleaned txt files in `folger-cleaned/`
- DB update (42 UPDATE statements)
- Minor `line_types.rs` tweak for `##` headers
- Remove blank-line filtering in `rebuild_buffer_text` (optional, can be done later)
