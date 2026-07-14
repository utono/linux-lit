---
name: rebuild-echo-embeddings
description: Use when the passage_embeddings table in lit.db is missing, stale, or needs rebuilding for semantic echo search — re-runs the Voyage AI pre-computation that embeds Shakespeare speaker turns and 2-turn exchanges for cross-work inner-monologue echo lookup
---

# Rebuild Echo Embeddings

Re-run the offline pre-computation that powers semantic echo search. The
script reads Shakespeare lines from `line_mapping` in lit.db, groups them
into speaker turns and 2-turn exchanges, enriches each with dramatic
context (`SPEAKER to ADDRESSEE: text`), embeds via Voyage AI, and stores
the vectors in the `passage_embeddings` table.

It also computes a 3-D affect vector (Valence/Arousal/Dominance) per passage
from the NRC-VAD lexicon, stored in the `sentiment` column for the optional
echo affect re-rank axis (see `set-echo-affect-weight`). Affect scoring is
cheap local arithmetic — no API cost.

## Two run modes

The script is the single entry point for both:

- **Full build** — table missing or dropped: embeds all ~58k passages via
  Voyage (30-75 min, ~3.7M tokens) AND writes sentiment vectors inline.
- **Sentiment-only backfill** — table already fully embedded but the
  `sentiment` column is missing or NULL: the script migrates the column in,
  **skips every already-embedded row (zero Voyage tokens)**, and backfills
  affect vectors locally in seconds. This is the path to run after adding the
  affect axis to an existing corpus.

## Prerequisites

- `VOYAGE_API_KEY` in the environment (stored in `~/.config/shell/secrets`)
  — required even for sentiment-only backfill (the client is constructed at
  startup), but no tokens are spent if all rows are already embedded
- A payment method on the Voyage dashboard to unlock standard rate limits
  (the free 200M token grant still applies — the full run uses ~3.7M tokens)
- `voyageai` Python package: `pip install --break-system-packages voyageai`
- **NRC-VAD lexicon** at `scripts/data/NRC-VAD-Lexicon.txt` (gitignored —
  redistribution-restricted). If absent, embeddings still build but sentiment
  is left NULL (the script warns). Download once:
  ```bash
  cd ~/utono/linux-lit/scripts/data
  curl -L -o nrc-vad.zip https://saifmohammad.com/WebDocs/Lexicons/NRC-VAD-Lexicon.zip
  unzip -j nrc-vad.zip 'NRC-VAD-Lexicon/NRC-VAD-Lexicon.txt' -d .
  rm nrc-vad.zip
  ```

## How to Run

```bash
source ~/.config/shell/secrets && python3 ~/utono/linux-lit/scripts/build_embeddings.py
```

For a long run, launch it in the background — it embeds ~58k passages
(~29.5k turns + ~28.8k exchanges) in roughly 30-75 minutes at standard
rate limits.

## Resumable

The script is resumable. On restart it loads already-embedded passage keys
from `passage_embeddings` and skips them — no wasted API calls. To force a
full rebuild, drop the table first:

```bash
sqlite3 ~/utono/litdb/data/lit.db "DROP TABLE IF EXISTS passage_embeddings;"
```

(Leaving the table in place resumes; the script only creates it when absent.)

## Verify

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT passage_type, COUNT(*) FROM passage_embeddings GROUP BY passage_type;"
```

Expect ~29.5k `turn` rows and ~28.8k `exchange` rows. Each embedding blob
is 4096 bytes (1024 little-endian f32 values for `voyage-4-large`).

Confirm sentiment vectors are present (each is a 12-byte blob = 3 f32):

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT COUNT(*) FROM passage_embeddings WHERE sentiment IS NOT NULL;"
```

Expect the same total as the row count. A count of 0 means the NRC-VAD
lexicon was missing during the run.

## When to Rebuild

- The `passage_embeddings` table is missing or was dropped
- lit.db gained or lost Shakespeare works (new texts to embed)
- The embedding model changed in `scripts/build_embeddings.py` (`MODEL`
  constant) — must match `MODEL` in `src/voyage.rs`
- The enrichment format changed (must match `build_echo_query` in
  `src/input/visual.rs`)
- The `sentiment` column is missing or NULL (run for the cheap
  sentiment-only backfill — no Voyage tokens spent)

## Key Files

- `scripts/build_embeddings.py` — the pre-computation script (model, batch
  size, grouping, enrichment, resume logic)
- `src/voyage.rs` — runtime query embedding; `MODEL` must match the script
- `src/db/queries.rs` — `find_similar_passages`, `decode_embedding` (reads
  the blobs this script writes)
- `docs/superpowers/specs/2026-05-30-semantic-echo-search-design.md` — full design

## Important: Keep Model in Sync

The runtime query embedding (`src/voyage.rs`) and the document embeddings
(this script) MUST use the same Voyage model — cosine similarity across
different models is meaningless. If you change `MODEL` in one, change it in
both and rebuild the whole table.
