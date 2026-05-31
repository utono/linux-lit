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

---

## Updates Since Initial Design

The feature shipped with the following changes from the original design above.

### Embedding model

The pre-computation and runtime both use **`voyage-4-large`** (1024-dim),
Voyage's best general-purpose retrieval model (released Jan 2026). The model
name appears as a `MODEL` constant in both `scripts/build_embeddings.py` and
`src/voyage.rs` — the two MUST match, since cosine similarity across different
models is meaningless. (The corpus was initially built with `voyage-3-large`
and later re-embedded with `voyage-4-large`.)

### Corpus exclusion (`-Amb` and `-BBC`)

The pre-computation excludes ALL hyphenated alternate editions, not just
`-Amb`. The Shakespeare-works query is now
`abbrev NOT LIKE '%-%'` (the canonical works have no hyphen in their abbrev;
the `-Amb` Ambrose and `-BBC` radio editions are duplicates that polluted the
echo results). Final corpus: ~29k turns + ~28.3k exchanges = ~57.4k embeddings.

### Resumable pre-computation

`scripts/build_embeddings.py` is resumable: on restart it loads already-embedded
passage keys from `passage_embeddings` and skips them, so a rate-limited or
interrupted run continues without re-spending API calls. To force a full rebuild,
drop the table first. Documented in the `rebuild-echo-embeddings` skill.

### Echoes overlay (`i` on a line) — a second consumer of the search

Beyond the inner-monologue gloss flow, a standalone read-only feature was added:
pressing **`i`** on a line embeds the cursor line's **speaker turn** and shows
the most similar cross-work passages in the gloss overlay card (NOT a picker),
formatted like inner-monologue echoes (source turn header, then each echo as an
italic quote with citation indented below). See
`docs/specs/2026-05-31-echo-links-persistence-design.md` and
`docs/specs/2026-05-31-echo-jump-navigation-design.md` for the full feature.

Display refinements in this overlay:
- **Dedup** by displayed first sentence (over-fetch 60, keep highest-similarity
  unique, cap 15).
- **Sort by work title**, then act.scene — echoes group by work.
- **First complete sentence** is shown, preserving verse line breaks (not just
  the first line, not collapsed to prose).
- Each echo renders as `["sentence" — Work act.scene]`, split into an italic
  quote line and an indented citation line.

### `EchoCandidate.start_line`

`find_similar_passages` now projects `start_line` from `passage_embeddings`
into `EchoCandidate` (used downstream to resolve the echoed line for jump
navigation).

### Maintenance skill

`rebuild-echo-embeddings` skill documents how to re-run the offline
pre-computation (prerequisites, resume behaviour, verification, model-sync
warning).

---

## Sentiment/Affect Re-Rank Axis (2026-05-31)

**Status:** implemented, shipped disabled (`echo_affect_weight` defaults to
`0.0`). Optional second ranking axis layered on top of the existing semantic
cosine ranking. Activate via the `set-echo-affect-weight` skill.

### As-built notes

- **Lexicon:** NRC-VAD (3-D V/A/D), vendored to `scripts/data/NRC-VAD-Lexicon.txt`
  (gitignored — redistribution-restricted academic license). Both the offline
  build (`scripts/build_embeddings.py`) and the runtime (`src/db/affect.rs`)
  read this same file; the Rust side resolves it via `CARGO_MANIFEST_DIR`.
- **Storage:** `passage_embeddings.sentiment BLOB` — 3 little-endian f32. The
  build script migrates the column in (idempotent `ALTER TABLE`) and backfills
  all existing rows locally (no Voyage cost).
- **Parity:** `src/db/affect.rs::compute_vad` mirrors the Python `compute_vad`
  exactly (tokenizer `[a-z']+`, lowercase, mean over in-lexicon words, neutral
  `[0.5,0.5,0.5]` fallback). A unit test (`vad_matches_python_reference`)
  asserts the exact Python value to prevent drift.
- **Query side:** affect is computed from the RAW highlighted text, not the
  enriched `"SPEAKER to ADDRESSEE: ..."` query string, so speaker labels don't
  skew it — matching the document side (which scores `passage_text`).
- **Blend:** `find_similar_passages` takes `affect_weight: f32`; score is
  `(1-w)*semantic + w*affect_cosine`. At `w=0`, or when the lexicon is missing,
  or when a candidate has no `sentiment` blob, it falls back to pure semantic
  similarity for that candidate. Weight is clamped to `[0,1]` at config load.
- **Config:** `Config.echo_affect_weight` (default `0.0`), in
  `~/.config/linux-lit/config{,-dev}.json`.

The remainder of this section is the original design rationale.

### Motivation

The current ranking is single-axis: pure Voyage cosine similarity (see
`find_similar_passages` in `src/db/queries.rs`, the `sort_by` on
`similarity`). This captures *meaning* similarity but not *affective posture* —
two speeches that aren't lexically or semantically close but both trace the same
emotional moment (e.g. despair resolving into resolve) will not be ranked
together.

This mirrors the Vectorian Age paper's (Liebl & Burghardt, 2020,
`aclanthology.org/2020.latechclfl-1.7.pdf`) core finding: no single similarity
axis wins, and the gain comes from **interpolating multiple axes with a tunable
weight** (their `EMI` parameter mixing `fastText` and `wn2vec`). A
sentiment/affect axis is the same idea — swap the second embedding for an
*affective* vector.

### Data model

Add a small affect vector alongside the existing 1024-dim embedding:

```sql
ALTER TABLE passage_embeddings ADD COLUMN sentiment BLOB;  -- float32 vector
```

Recommended representation: a **VAD vector** (Valence, Arousal, Dominance — 3
floats) or an **NRC 8-emotion vector** (anger, fear, joy, sadness, etc.).
Either is tiny next to the 1024-dim semantic blob — negligible storage on the
~57k rows. The blob is little-endian f32, decoded with the existing
`decode_embedding` helper.

### Offline computation (`scripts/build_embeddings.py`)

Compute the affect vector in the same per-passage loop that already enriches and
embeds. Two sources:

- **Lexicon (default):** NRC-VAD / NRC-EmoLex lookup over `passage_text`,
  averaged across tokens. Free, deterministic, no extra API call. Per-word
  scores are weak but acceptable averaged over a multi-word turn.
- **Model-scored (optional, higher quality, higher cost):** ask an LLM for a VAD
  score per passage. For a one-time ~57k-row build the lexicon is the pragmatic
  default; the model path can be a flag.

The script's resumable logic (skip already-embedded keys) extends naturally:
treat a NULL `sentiment` column as "needs affect scoring."

### Runtime combination

The combination point is the ranking in `find_similar_passages`
(`src/db/queries.rs`). Today:

```rust
let sim = cosine_similarity(query_embedding, &emb);
// ... candidates.sort_by(|a, b| b.similarity.partial_cmp(...))
```

Proposed — a weighted blend, following the Vectorian's interpolation shape:

```rust
let score = (1.0 - w) * sim + w * affect_sim;
```

where `affect_sim` is cosine (or negative L2) between the query's affect vector
and the candidate's, and `w` is the affect weight in `[0, 1]`.

### Re-rank, not replace

The paper's hardest-won lesson: a low-dimensional, low-discriminative axis
*degrades* recall if over-weighted — their harmonic-mean optimizer literally
turned embeddings off when they hurt hard queries. Affect is even
lower-dimensional than their embeddings (every anguished soliloquy clusters
together), so:

- Let Voyage cosine fetch a **wide candidate set** first (the code already
  over-fetches 60 in `src/input/actions/echoes.rs`).
- Use affect only to **re-rank within that set**, never as the primary fetch.
- Default `w` **low** (~0.15–0.25). A bad affect signal must never dominate the
  semantic ranking already trusted by the shipped feature.

### Tunable weight

The Vectorian's contribution was that the optimal blend is query-dependent.
Optuna isn't feasible in a desktop reader, but `w` should be exposed via config
the way other tuning knobs already are — a `set-echo-affect-weight` skill
writing to `~/.config/linux-lit/config.json`, paralleling `set-sync-preroll`.
This allows A/B-by-feel against real glosses rather than a one-shot guess.

### Risk

VAD scored from a modern lexicon over Early Modern English is noisy — the same
historical-language problem flagged for embeddings above. Gate behind the
tunable weight defaulting low, so a poor affect signal can never override the
semantic ranking. Ship disabled (`w = 0`) and raise only if the re-rank
measurably improves echo quality.
