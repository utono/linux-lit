# Plan: Re-import Gutenberg prose works as one-sentence-per-line

## Problem

Five "(Gutenberg)" prose works render badly in linux-lit: each line is a
hard-wrapped ~59-char *fragment* (the cursor/sign-column treats every wrap
fragment as its own line, and text breaks mid-sentence). The 19 other dynamic
prose works render fine because they store **one sentence per `line_mapping`
row**.

### Root cause (confirmed)

- `scripts/prepare_gutenberg.py` only strips Gutenberg boilerplate/TOC/footer —
  it **keeps Project Gutenberg's ~59-char hard line-wrapping**
  (`'\n'.join(lines)`, no unwrap, no sentence split).
- `scripts/import_gutenberg.py` inserts **one `line_mapping` row per physical
  text line** (`for line in f:`), even though its own `--help` says the input
  should be "one sentence/line per line".
- So each wrap fragment became a DB row.

### The fix already exists

`scripts/split_sentences.py` does exactly the right thing (clean_gutenberg +
front-matter skip + `extract_paragraphs` + nltk `sent_tokenize` + abbreviation
re-join + junk filter), emitting **one sentence per line** with blank lines
between paragraphs, to `<name>-lines.txt`.

Dry-run verified on Bleak House: 32,159 fragments -> **19,251 clean sentences**
(avg 99 chars, paragraphs preserved). Output at `/tmp/bleak-house-lines.txt`.

## Decision

- **Adopt `split_sentences.py`** in the Gutenberg pipeline (do NOT duplicate its
  logic into `prepare_gutenberg.py`).
- Re-import the 5 works from `-lines.txt`, then re-map timestamps and re-derive
  per-media data.

## Affected works (current state)

| Abbrev | Title | lines (now) | timestamps | media_id(s) | bookmarks | phrase_ts | spoken |
|--------|-------|-------------|-----------|-------------|-----------|-----------|--------|
| BH  | Bleak House          | 32159 | 64137 | 243,244 | 4 | 120478 | 64318 |
| ACC | A Christmas Carol    | 3090  | 3073  | 247     | 0 | 0      | 3090  |
| TTC | A Tale of Two Cities | 12321 | 12279 | 246     | 0 | 0      | 12321 |
| PP  | The Pickwick Papers  | 28200 | 56155 | 241,245 | 0 | 0      | 28200 |
| TT  | A Tale of a Tub      | 3697  | 3687  | 56      | 0 | 0      | 3687  |

Not affected: `glosses`/`echo_links`/`passage_embeddings` have no rows for these
(prose works don't use the Shakespeare echo/gloss system). `line_translations` = 0.
Media associations (`work_media_associations`) key on `media_id` (unchanged) and
survive re-import.

## What re-import destroys

`import_gutenberg.py --reimport` deletes and recreates `line_mapping` rows for
the work (new `id`s), and deletes that work's `line_timestamps` and
`line_spoken_status`. Anything keyed on the **old `line_mapping.id`** is
invalidated:

- **line_timestamps** — deleted by --reimport; must be re-mapped from whisperX.
- **line_spoken_status** — deleted by --reimport; must be re-populated per media.
- **phrase_timestamps** — NOT auto-deleted (keyed on line_mapping_id + media_id);
  BH has 120k rows that will dangle. Must be deleted + rebuilt (or left, then
  rebuilt) — optional feature, but stale rows should be cleared.
- **bookmarks** — BH has 4, keyed on old `line_mapping_id`; will dangle. Either
  accept loss or migrate by matching text (see Step 6).
- **chunks** — verify whether any of the 5 have chunk rows keyed on line ids.

## Steps (per work; pilot on BH first)

> Pre-flight every DB-writing step: ensure **no linux-lit instance is running**
> (it rewrites config/state on exit and must not race the DB).
> `pgrep -af target/debug/linux-lit` must be empty.

### Step 0 — Back up the DB
```
cp ~/utono/litdb/data/lit.db ~/utono/litdb/data/lit.db.bak-$(TZ='America/Chicago' date +%Y%m%dT%H%M%S)
```

### Step 1 — Generate sentence-per-line text
For each work, from its raw Gutenberg `.txt` (e.g. `bleak-house.txt`, NOT the
`-prepared.txt`):
```
cd ~/utono/litdb
.venv/bin/python3 scripts/split_sentences.py \
    ~/utono/literature/<author-dir>/<name>.txt \
    -o ~/utono/literature/<author-dir>/<name>-lines.txt
```
Review head/tail; confirm no boilerplate, first line is opening prose/heading,
last line is the final sentence. (BH already dry-run'd -> /tmp/bleak-house-lines.txt.)

### Step 2 — Re-import from -lines.txt
```
.venv/bin/python3 scripts/import_gutenberg.py BH \
    ~/utono/literature/dickens-charles/bleak-house-lines.txt --reimport
```
(`--reimport` deletes old line_mapping/line_timestamps/line_spoken_status for BH.)

### Step 3 — Point the work at the new source text
The `works.text_file` column should reference the new `-lines.txt` (or be set to
NULL if we want the dynamic-from-DB path; since rows are now sentences, either
renders correctly). Decide one convention; recommended: keep `text_file` pointing
at `-lines.txt` for snapshot/line-map consistency.
```
sqlite3 lit.db "UPDATE works SET text_file='.../bleak-house-lines.txt' WHERE abbrev='BH';"
```
Also update `~/utono/lit/plugins/lua/lit_core/work.lua` FILENAME_TO_ABBREV if the
filename changed (`-prepared.txt` -> `-lines.txt`).

### Step 4 — Re-map timestamps (per media_id) [tty2, slow]
For each media_id of the work, run `map_gutenberg_timestamps.py` against the
whisperX JSON (SequenceMatcher alignment). BH has two media (243, 244); PP has
two (241, 245).
```
python -u scripts/map_gutenberg_timestamps.py BH <MEDIA_ID> <WHISPERX_JSON> --verify
```

### Step 5 — Re-populate spoken status + chapters + sentences (per media_id)
- `populate-spoken-status` (whisper-transcript) per media_id.
- `mark_chapters.py <ABBREV> --apply` — **the `--apply` flag is required**;
  without it `mark_chapters.py` only does a dry-run and writes nothing. (It is
  not per-media: it sets `is_chapter` on all media's rows in one call.) Dry-run
  first to confirm the chapter count, then re-run with `--apply`.
- `detect_sentences.py BH --media-id <ID>` (`--paragraphs-only` is now trivial
  since rows are already sentences — confirm whether still needed).

> NOTE (lesson from BH): the first BH run called `mark_chapters.py BH` without
> `--apply`, leaving 0 chapters; re-running with `--apply` marked all 67. Always
> include `--apply` in the rollout scripts.

### Step 6 — Handle stale references
- **phrase_timestamps**: delete BH rows, optionally rebuild via
  `build_phrase_timestamps.py` (optional word-level highlight).
  ```
  sqlite3 lit.db "DELETE FROM phrase_timestamps WHERE media_id IN (243,244)
                  AND line_mapping_id NOT IN (SELECT id FROM line_mapping);"
  ```
- **bookmarks (BH, 4)**: these point at old line ids. Either accept loss, or
  before re-import capture their `canonical_text` and re-insert matching the new
  row whose text contains that fragment. Low value (4 bookmarks) — likely accept
  loss, but capture them first:
  ```
  sqlite3 lit.db "SELECT b.line_mapping_id, lm.canonical_text FROM bookmarks b
                  JOIN line_mapping lm ON b.line_mapping_id=lm.id
                  WHERE b.work_abbrev='BH';"
  ```
- **chunks**: check `SELECT COUNT(*) FROM chunks WHERE work_abbrev='BH';` and
  clear/rebuild if present.

### Step 7 — Clear the snapshot cache
The cached snapshot self-invalidates on text_file path/mtime change, but to be
safe:
```
rm -f ~/.cache/linux-lit/snapshots/BH.text.bin
```

### Step 8 — Verify in linux-lit
Launch, Ctrl+p -> Bleak House, Ctrl+Shift+M -> media. Confirm:
- Prose reflows as full sentences (no mid-sentence breaks, one dot per sentence).
- Playback sync tracks; `,`/`q` paragraph nav works.
- Cursor/bookmark/seek behave.

### Step 9 — Roll out to ACC, TTC, PP, TT

The per-work flow is baked into two wrappers — Steps 0-7 above are automated:

1. **Prep (DB-side, fast, linux-lit CLOSED)** — backs up the DB, sentence-splits
   the raw Gutenberg `.txt`, re-imports, NULLs `text_file`, clears stale
   phrase_timestamps/bookmarks/snapshot, and prints the tty2 command:
   ```
   ~/utono/litdb/scripts/reimport-gutenberg-prep.sh <ABBREV> <RAW_GUTENBERG_TXT> <MEDIA_ID>...
   ```
2. **Timestamps (tty2, slow)** — run the command it printed:
   ```
   ~/utono/litdb/scripts/reimport-gutenberg-timestamps.sh <ABBREV> <MEDIA_ID>:<WHISPERX_JSON>...
   ```
   (maps timestamps + spoken status + sentences per media, then marks chapters
   with `--apply`, then prints verification counts.)

Concrete invocations (raw sources + media verified present):

- **ACC** (1 media): `reimport-gutenberg-prep.sh ACC ~/utono/literature/dickens-charles/a-christmas-carol.txt 247`
- **TTC** (1 media): `reimport-gutenberg-prep.sh TTC ~/utono/literature/dickens-charles/a-tale-of-two-cities.txt 246`
- **PP**  (2 media): `reimport-gutenberg-prep.sh PP ~/utono/literature/dickens-charles/the-pickwick-papers.txt 241 245`
- **TT**  (1 media): `reimport-gutenberg-prep.sh TT ~/utono/literature/swift-jonathan/a-tale-of-a-tub.txt 56`

Then flip `push_to_device` if needed and verify in linux-lit (Steps 8-9 above).
PP has two media (241,245) like BH; both get aligned in one tty2 run.

### Step 10 — Update the wizard skill (pipeline fix going forward)
Edit `~/utono/litdb/.claude/skills/wizard-gutenberg/SKILL.md` Step 1/2 to run
`split_sentences.py` -> `-lines.txt` and import that, replacing the
`prepare_gutenberg.py` -> `-prepared.txt` flow. Note in the skill that
`prepare_gutenberg.py` alone leaves hard-wrapping and must not be used as the
import source for prose.

## Open questions for the user

1. Keep `works.text_file` pointing at `-lines.txt`, or set NULL (dynamic)? Both
   render correctly now; `-lines.txt` keeps the snapshot/line-map fast path.
2. Rebuild phrase_timestamps (word-level highlight) for BH/PP, or skip (line-level
   highlight is fine)?
3. Migrate BH's 4 bookmarks by text, or accept loss?
4. Delete the now-obsolete `-prepared.txt` files, or leave them?

## Notes / risks

- Timestamp re-mapping is the expensive, GPU/JSON-heavy step; run on tty2.
- The new sentence count (~19k for BH) differs from old (~32k), so all per-line
  data must be rebuilt, not migrated 1:1.
- Do all DB writes with linux-lit closed.
- Pilot fully on BH and verify before touching the other four.
