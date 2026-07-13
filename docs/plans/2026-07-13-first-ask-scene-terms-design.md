# Term-ground the first ask's phrasing from scene text — design

**Date:** 2026-07-13
**Repos:** `linux-lit` (code) + `claude-api-prompts` (new prompt master)
**Builds on:** `2026-07-13-journal-rewrite-uses-key-terms-design.md` (the `R`-path
feature; reuses `improve_question(terms)` and `improve_terms_line`).

## Problem

The `R`-rewrite path now grounds the improve-question (phrasing) step on a saved
entry's `journal_tags`. A **brand-new** ask has no saved entry and no tags yet,
so it passes `&[]` and its phrasing step is ungrounded. We want the first ask's
phrasing sharpened around the passage's terms of art too.

## Goal

On a new Scene/Passage ask, derive candidate terms from the scene text the
reader is looking at and feed them to the improve-question call, so a vague first
question is sharpened around those terms — matching the `R` behavior for saved
entries.

## Decisions (locked)

- **Term source:** extract from the scene text (already assembled in `ask_claude`).
- **Sequencing:** sequential — `scene → extract-terms` finishes, then
  `improve_question(terms)`, then `ask_claude`. The reader accepts the added
  latency (one extra round-trip before phrasing). The loading card is already up.
- **Prompt:** a NEW `journal.scene-terms` master, worded for "terms of art a
  reader working through this passage might ask about" — NOT the off-label
  `journal.extract-terms` (which is worded for a Q&A entry that explains a term).
  Same `{"terms":[...]}` output contract so `parse_terms` works unchanged.
- **Scope:** Scene and Passage bands only. Work and Author bands have no scene
  text → pass `&[]`, behaving exactly as today. Empty/unresolvable scene text →
  skip the extra call, fall through to ungrounded improve.
- **Fallback symmetry:** a compiled `FALLBACK_SCENE_TERMS_PROMPT` mirrors the
  master, so a missing `api_prompts` row does not silently disable the feature.

## Cost (acknowledged)

A Scene/Passage new-ask becomes **3 blocking round-trips** (extract → improve →
answer) plus the existing background retag (#4). Partially redundant with that
post-save retag — but the retag runs too late to shape phrasing, which is the
point. Work/Author asks stay at 2 round-trips.

## Design

### Part 1 — prompt master (`claude-api-prompts`)

New `prompts/journal.scene-terms.md`, `has_placeholders: false`. Output contract
identical to `journal.extract-terms` (`{"terms":[...]}`, ≤8, canonical phrasing,
`{"terms":[]}` when none) so the existing `parse_terms` handles it verbatim.
Wording reframed for passage input: extract the terms of art (legal, rhetorical,
historical, prosodic, theological) a reader working through THIS passage might
want to ask about; exclude ordinary vocabulary, character names, the work title.

Sync: commit master → `sync-to-db.py journal.scene-terms` →
`render-prompt.py` / `list-versions.py` to verify active.

### Part 2 — linux-lit

`src/input/actions/journal.rs`:

- **Factor `current_scene_text(&AppState) -> String`** out of `ask_claude`'s
  borrow block (the `band` + `return_pos`-anchor + `scene_text_windowed` logic).
  `ask_claude` calls it, removing the duplication. Returns empty for Work/Author
  and unresolvable positions (as today).
- **New `extract_scene_terms(state, question, on_done)`** — models `spawn_retag`
  but feeds terms FORWARD instead of writing them:
  - Read `current_scene_text(&s)` and `config.tag_extract_model` under one borrow.
  - If scene text is empty → immediately `on_done(state, question, vec![])`
    (no API call — Work/Author or unresolvable).
  - Else fire one `run_claude_request` with the `journal.scene-terms` prompt
    (DB active → `FALLBACK_SCENE_TERMS_PROMPT`) over the scene text, using
    `tag_extract_model`. On success → `parse_terms(&reply)`; on error → `vec![]`.
    Either way call `on_done(state, question, terms)`.
- **`submit_prompt`'s new-ask branch** routes through `extract_scene_terms`:

  ```rust
  state.borrow().journal_overlay.show_loading(&text);
  extract_scene_terms(state, text, move |st, question, terms| {
      improve_question(st, question, &terms, move |st2, improved| {
          ask_claude(st2, &improved);
      });
  });
  ```

  (The `R` path is unchanged — it already grounds on saved `journal_tags`.)

Reuses from the prior feature: `improve_question(state, question, terms, on_done)`
and `improve_terms_line`. Reuses `crate::journal_tags::parse_terms`.

### Part 3 — skill

Add `journal.scene-terms` to the `update-api-prompt` skill's key list +
frontmatter description.

## Data flow (new Scene/Passage ask)

`submit_prompt` → `extract_scene_terms`:
assemble scene text → (empty? → terms=[]) else extract-terms call → `parse_terms`
→ `improve_question(question, terms)` → improved question → `ask_claude` (answer,
already scene-grounded) → save → background `spawn_retag` (unchanged).

## Error / empty handling

- Work/Author band or empty scene text → no extra call; `terms = []`; identical
  to today.
- Extract-terms API error or unparseable reply → `terms = []`; ungrounded improve
  (never blocks the ask).
- Missing `journal.scene-terms` DB row → `FALLBACK_SCENE_TERMS_PROMPT`.

## Testing

- `AppState` is a GTK-holding god-struct with NO test constructor (built once at
  app init), so `current_scene_text` / `extract_scene_terms` gating cannot be
  unit-fixtured — consistent with the existing module (scene_synopsis notes its
  windowing is "covered by code inspection"). Test the PURE pieces
  (`parse_terms`, `improve_terms_line` — already covered) and verify the wiring
  by compile + a behavioral pass.
- DB-active `journal.scene-terms` prompt renders and `parse_terms` accepts its
  contract (Python check against lit.db, mirroring the prior feature's check).
- Behavioral: with `auto_tag`-style extract model configured, a Scene ask on a
  term-rich passage (e.g. Rom 3.1) produces a sharpened question; a Work-band ask
  is unaffected. (Live, nondeterministic — a sanity pass, not an assertion.)

## Follow-up / non-goals

- Not caching extracted scene terms — each new ask re-extracts. A future
  optimization could reuse `terms_for_entry`-style tags from the work's prior
  entries to skip the call, but that was explicitly deferred.
