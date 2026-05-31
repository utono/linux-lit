# Semantic Echo Search for Inner-Monologue Glosses

**Date:** 2026-05-30

## Problem

The inner-monologue gloss system needs cross-work echoes from Shakespeare's corpus — lines from other plays where a character performs the same dramatic action under the same governing convention. Word concordances fail here because the connection is semantic, not lexical. Paris saying "my wife" connects to Parolles asking "Are you meditating on virginity?" — same dramatic action (claiming access to a woman's body through social convention), zero shared vocabulary.

Currently Claude API guesses echoes from its training data. The results are often apt but citations are frequently hallucinated, and the model has no way to search the actual corpus systematically.

## Solution

Pre-compute sentence embeddings for all Shakespeare speaker turns and 2-turn exchanges in lit.db. At gloss time, embed the highlighted passage, find the most similar passages from other works, and show candidates in a picker. The user selects an echo, which is injected into the Claude prompt.

## Architecture

Two phases:

- **Offline pre-computation** — Python script reads Shakespeare lines from `line_mapping`, groups into speaker turns and 2-turn exchanges, enriches with dramatic context, embeds via Voyage AI API, stores vectors in lit.db
- **Runtime lookup** — Rust code in linux-lit embeds the highlighted passage (single API call), computes cosine similarity against stored vectors, shows top candidates in a picker, injects the user's selection into the Claude prompt

## Data Model

New table in lit.db:

```sql
CREATE TABLE passage_embeddings (
    id INTEGER PRIMARY KEY,
    work_abbrev TEXT NOT NULL,
    div1 INTEGER,
    div2 INTEGER,
    speaker TEXT,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    passage_type TEXT NOT NULL,  -- 'turn' or 'exchange'
    passage_text TEXT NOT NULL,
    enriched_text TEXT NOT NULL,
    embedding BLOB NOT NULL,
    UNIQUE(work_abbrev, start_line, end_line, passage_type)
);
CREATE INDEX idx_pe_work ON passage_embeddings(work_abbrev);
CREATE INDEX idx_pe_type ON passage_embeddings(passage_type);
```

- `passage_text` — raw text of the turn or exchange
- `enriched_text` — text prepended with speaker, addressee, and scene context before embedding, so the vector captures dramatic situation not just vocabulary
- `embedding` — float32 vector stored as blob (1024 dims for `voyage-4`)
- `passage_type` — `'turn'` for single speaker turn, `'exchange'` for 2-turn dialogue pair

Estimated scale: ~15-20k speaker turns + ~15-20k exchanges = ~30-40k rows. Storage: ~200-250MB with 1024-dim vectors. One-time embedding cost via Voyage AI: ~$0.01-0.02.

## Pre-computation Script

`scripts/build_embeddings.py`:

1. Read all Shakespeare lines from `line_mapping` (WHERE `work_abbrev IN (SELECT abbrev FROM works WHERE author = 'Shakespeare' AND abbrev NOT LIKE '%-Amb')`)
2. Group consecutive lines by `(work_abbrev, div1, div2, speaker)` to form speaker turns
3. Build 2-turn exchanges by pairing consecutive turns in each scene
4. Filter: exclude stage directions, exclude non-spoken lines (speaker IS NULL), exclude turns shorter than 4 words
5. Enrich each passage with context: `"{SPEAKER} to {NEXT_SPEAKER}: {text}"` — the addressee is inferred from who speaks next (or before, for the last turn in a scene)
6. Call Voyage AI `voyage-4` in batches (batch size ~100, respecting rate limits)
7. Store results in `passage_embeddings` table
8. Report: total turns embedded, total exchanges embedded, any skipped entries

Following the Vectorian Age paper's finding: enrichment with speaker/role context improves semantic matching for dramatic text. Domain-specific enrichment outperforms raw text embedding.

## Runtime Lookup

When the user triggers an inner-monologue gloss (visual selection + inner-monologue action):

1. Build the query text: `"{SPEAKER} to {ADDRESSEE}: {highlighted_text}"` — same enrichment format as pre-computation
2. Call Voyage AI embeddings API (single call, ~100ms)
3. Load pre-computed embeddings from `passage_embeddings` WHERE `work_abbrev != source_work`
4. Compute cosine similarity, rank descending
5. Filter: exclude same work (by `work_abbrev`)
6. Take top 10 candidates

Performance: loading ~30-40k vectors of 1024 floats from SQLite and computing cosine similarity is fast enough in Rust — pure arithmetic, no GPU needed. If loading all vectors is slow on first use, cache them in memory after the first lookup in the session.

## Echo Picker UI

New `InputMode::EchoPicker`. Reuse the existing picker widget pattern (list with j/k navigation, Enter to select, Escape to cancel).

Each row shows:
- Speaker name (small caps)
- Work title and act.scene
- First line of the passage text (truncated to fit)

User navigates with j/k, selects with Enter. Selected passage is injected into the prompt. Escape dismisses the picker and falls through to Claude finding its own echo (existing behavior).

The picker appears automatically when generating an inner-monologue gloss, between the user's visual selection and the Claude API call.

## Prompt Integration

When the user selects an echo from the picker, the user message sent to Claude gains a new section:

```
--- SUGGESTED ECHO (from semantic search) ---
Speaker: PAROLLES
Work: All's Well That Ends Well 1.1
Text: Are you meditating on virginity?
```

The system prompt instruction: "A semantic search has suggested the following passage as dramatically similar. Use it as the cross-work echo if it fits the actioning analysis. If it does not fit, find your own."

This biases Claude toward the user's selection while allowing override for bad matches.

## Dependencies

- Python: `voyageai` SDK (`pip install voyageai`)
- Rust: `reqwest` (already used for Claude API), `serde_json` (already used)
- Voyage AI API key: stored in `VOYAGE_API_KEY` environment variable

## Risks

- **Embedding quality on Early Modern English:** The Vectorian Age paper found that general-purpose embeddings struggle with historical language. Enrichment with dramatic context (speaker, addressee) mitigates this. If quality is poor, we can try `voyage-4-large` (higher quality, ~2x cost).
- **Citation accuracy:** The embedding search returns actual passages from lit.db with verified citations — this eliminates the hallucinated-citation problem entirely.
- **Storage:** ~200-250MB added to lit.db. Acceptable for a desktop app. Could move to sidecar file if needed.
- **API cost:** One-time pre-computation ~$0.02. Runtime ~$0.0001 per lookup. Negligible.
