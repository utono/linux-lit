# Journal question-improve (create + R target Q/A/both) — Design

> Design/spec. Next: `superpowers:writing-plans` → implementation plan under
> `docs/plans/`.

## Goal

The reader's raw question phrasing should be improved: (1) automatically at
entry creation, and (2) on demand later via `R`. Today creation saves the
verbatim question and `R` rewrites only the ANSWER — so a "rewrite the question"
instruction just expanded the answer. Fix both.

## Decisions (settled in brainstorming)

- **`R` first picks a target: question / answer / both.**
  - *answer* → today's flow (type a free instruction → revise the answer).
  - *question* → AUTO-IMPROVE the question's phrasing (no instruction typed) →
    then REGENERATE the answer for the improved question.
  - *both* → improve the question, then the free-instruction answer rewrite.
- **On creation, auto-improve the question** before saving; the stored entry
  gets the polished question + its answer.
- **Implementation: a SEPARATE `improve_question` Claude call** (Option B) — do
  NOT restructure the tuned prose `journal_qa_prompt` into JSON. Keep the answer
  prompt exactly as-is (plain prose); add a small dedicated improve-question
  prompt. One reusable primitive for both create and `R`. (This means creation
  makes two calls — improve-Q then answer — accepted to protect the answer
  prompt.)

## Components

### 1. `improve_question` primitive (`src/input/actions/journal.rs`)

```rust
/// Improve the phrasing of a reader's journal question (clarify/tighten,
/// preserve intent). Fire the small `journal.improve-question` prompt (active
/// api_prompts row, else a hardcoded fallback — mirrors spawn_retag). Returns
/// the reworded question (fence-stripped, trimmed); FALLS BACK to the original
/// on empty/error so the question is never lost.
```

Uses `run_claude_request` (async) + `active_prompt("journal.improve-question")`
with a `FALLBACK_IMPROVE_QUESTION_PROMPT` constant. Reply parsing reuses the
fence-strip logic (`journal_tags` has `strip_code_fence`-equivalent; factor a
shared helper or inline). The prompt returns ONLY the improved question text
(no JSON needed — it's a single string).

### 2. Creation flow (`ask_claude`)

Before saving: `improve_question(raw)` → improved Q; then the EXISTING plain-prose
answer call, framed with the improved Q; `save_*` with improved Q + answer. Two
calls. Answer prompt unchanged. Guard: empty/failed improve → use the raw Q.

### 3. `R` target picker (`keymap.rs` + `journal.rs`)

`R` opens a small chooser (question / answer / both) — reuse a confirm-style
prompt or a tiny picker.
- *answer* → existing `begin_rewrite` / `rewrite_with_claude`.
- *question* → `improve_question` on the DISPLAYED entry → re-answer (reuse the
  answer call with the improved Q) → save both → re-render.
- *both* → improve Q, then the answer-rewrite instruction flow with the new Q.

### 4. Filter-aware

Resolve the target entry via `displayed_journal_page(&s)` (already built);
writes go through `update_journal_page(id)`; re-render via `render_filtered_match`
when a filter is active, else `render_current`. In-memory filter match's q/a
updated so the filtered view shows the new text.

## Edge cases

- improve_question empty/error → keep the ORIGINAL question (no data loss) +
  toast on error.
- No double `Q:` prefix — reuse `prefix_question`.
- Regenerate-answer uses the same grounding context (`rewrite_context` /
  `ask_claude`'s framing) as create/rewrite.
- `both` ordering: improve Q first, then the answer rewrite uses the NEW Q.
- Under a filter, the question path updates the cross-work displayed entry, not
  the origin band (via `displayed_journal_page`).

## Testing

- Unit: the improve-question reply parse (fence-strip; empty→original fallback),
  like `parse_terms`.
- Headless e2e (real Claude): (a) create a Q&A with a clumsy question → the
  stored question is improved (DB check); (b) `R` → question → the entry's
  question changes AND the answer regenerates; (c) on a filtered `f`-entry, `R`
  → question updates the correct cross-work row + re-renders the filtered view;
  (d) `R` → answer still works as before; (e) `R` → both. Back up + restore any
  real entry touched.

## Files

- `src/input/actions/journal.rs` — `improve_question` + `FALLBACK_IMPROVE_QUESTION_PROMPT`;
  the R-target branch; create-flow change; a re-answer helper (factor from
  `ask_claude`/`rewrite_with_claude` if clean).
- `src/input/keymap.rs` — the R-target chooser wiring (new `InputMode` or a
  confirm-style chooser) + its arms.
- `src/app/mod.rs` — new `InputMode` variant if a chooser mode is used.
- Reuse: `active_prompt`, `displayed_journal_page`, `render_filtered_match`,
  `prefix_question`, the fence-strip helper.

## Non-goals

- Not restructuring `journal_qa_prompt` into JSON (Option A rejected).
- Not changing how the ANSWER is prompted/rendered.
