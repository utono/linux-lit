---
name: set-echo-affect-weight
description: Use when adjusting how much the sentiment/affect (NRC-VAD) axis influences echo re-ranking in semantic echo search — blending emotional posture with semantic similarity, or turning the affect axis off
argument-hint: <weight 0.0-1.0>
---

# Set Echo Affect Weight

Sets `echo_affect_weight` in the linux-lit config JSON. This controls how much
the affect (NRC-VAD: Valence/Arousal/Dominance) axis blends with semantic
cosine similarity when ranking cross-work echoes:

```
score = (1 - w) * semantic_cosine + w * affect_cosine
```

- `0.0` = pure semantic ranking; the affect axis is inert (default)
- `0.15`–`0.25` = conservative re-rank; affect nudges echoes toward the same
  emotional posture without overriding meaning (recommended starting range)
- `1.0` = affect only (NOT recommended — affect is low-dimensional and clusters
  every anguished passage together)

Background and rationale:
`docs/specs/2026-05-30-semantic-echo-search-design.md`
(section "Sentiment/Affect Re-Rank Axis").

## Why low values

The NRC-VAD lexicon is modern English scored over Early Modern English, so the
affect signal is noisy ("thou", "prithee", archaic senses are mis-scored or
absent). Keep `w` low so a weak affect vote can never dominate the semantic
ranking the shipped feature relies on. Raise it only if a before/after
comparison shows the re-rank measurably improves echo quality.

## Prerequisites

The affect axis only engages if BOTH are true:
1. `echo_affect_weight > 0.0`
2. The `passage_embeddings.sentiment` column is populated (run
   `scripts/build_embeddings.py` once — it backfills affect vectors locally,
   no API cost). See the `rebuild-echo-embeddings` skill.

If the column is NULL or the NRC-VAD lexicon file is missing, ranking silently
falls back to pure semantic similarity regardless of `w`.

## Steps

1. Parse the argument as a float and clamp to `[0.0, 1.0]`.
2. Determine the active config file:
   - dev build (`cargo run`) reads `~/.config/linux-lit/config-dev.json`
   - release build reads `~/.config/linux-lit/config.json`
   - When unsure, set the value in BOTH files.
3. Set the `echo_affect_weight` key to the parsed value using `jq` (create the
   key if absent):
   ```bash
   f=~/.config/linux-lit/config-dev.json
   tmp=$(mktemp)
   jq --argjson w "$WEIGHT" '.echo_affect_weight = $w' "$f" > "$tmp" && mv "$tmp" "$f"
   ```
4. Tell the user to restart linux-lit — the value is read (and clamped) at
   startup in `src/config.rs::load`.
5. Report the change and the active config file edited.

## Location

The value is read at startup in `src/config.rs`:

```rust
#[serde(default = "default_echo_affect_weight")]
pub echo_affect_weight: f32,   // default 0.0, clamped to [0,1] on load
```

It is consumed in `src/db/queries.rs::find_similar_passages` (the blended
`sort_by`), with query-side affect computed in `src/db/affect.rs`.
