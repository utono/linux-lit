# Follow-up: Unify the Gloss-Display Path (and fix the drift bugs)

> **Status:** captured, not scheduled. Deferred deliberately so it doesn't
> couple a medium-risk overlay refactor to the small Alt+g feature
> (`2026-06-20-alt-g-most-recent-gloss-design.md`). Do this AFTER Alt+g ships.

## Why this exists

There are two parallel implementations of "show a gloss in the overlay":

- `open_gloss_overlay` (`src/input/actions/gloss.rs:1852`) — the canonical path,
  used by cursor-open, the gloss picker, and (after Alt+g ships) `open_last_gloss`.
- Six hand-rolled display sites in `src/input/visual.rs` — the three visual-mode
  gloss actions, each with a cached-open and a freshly-generated branch.

The six sites drifted from the canonical path. An audit (2026-06-20) found that
**all six omit** five things `open_gloss_overlay` does:

- `gloss_passages`, `gloss_passage_index` — *latent*; masked because the
  passage-nav handler (`gloss.rs:83-96`) lazily reloads `gloss_passages` from
  the DB when empty and re-derives the index from `gloss_context`. Alt+n/Alt+p
  still work, but the state is inconsistent.
- `recolor_cached_blocks(s)` — the exact omission `open_gloss_overlay`'s own
  doc-comment warns about (cached synthesized blocks left uncolored).
- **`gloss_active_voice = 0` — LIVE BUG.** Not reset on a fresh visual-mode
  gloss, so it retains the stale value from the previous overlay and is later
  read at `gloss.rs:1075`/`1175` for TTS voice selection. A freshly created
  gloss can narrate with the wrong voice.
- `gloss_opened_from_picker` — not reset; affects the Escape return path
  (another latent bug).

After Alt+g ships, the recording logic (`record_last_gloss`) is also called at
seven sites; unification collapses it to one.

## The blocker that makes this non-trivial

`open_gloss_overlay` builds its `GlossContext` from a `GlossedPassage`, which has
**no per-line numbers**: it hard-codes `source_line_numbers: Vec::new()`
(`gloss.rs:1874`) and passes an empty `source_lines` to `show_gloss_with_color`.
The visual sites pass `ctx.source_line_pairs()` built from the live visual
selection. Routing the visual sites through `open_gloss_overlay` as-written would
**drop source-line coloring**. So the function must be widened, not just reused.

What is NOT lost on unification:
- inner-monologue `verified_text` — saved to DB by `save_gloss`, so a re-load via
  `find_glosses_by_start` recovers it.
- `hash` / `gloss_type` — the real hash is only needed by `save_gloss`, which
  runs before the open call.

## Proposed design

Widen `open_gloss_overlay` to accept:

1. an explicit open index (replace the hard-coded `0`; the Alt+g feature already
   introduces `desired_type` → `start_gloss_idx`, so this may already be done —
   reconcile with whatever shipped),
2. an optional pre-built `GlossContext` (fall back to building one from the
   passage when `None`),
3. `source_lines: &[(String, i64)]` for source-line coloring,

and have it set `gloss_original_text` itself.

Then all six visual sites collapse to: build/obtain the ctx (cached: from the
existing `all_glosses[idx]`; fresh: after `save_gloss`, re-load via
`find_glossed_passages` + `find_glosses_by_start`) and call the shared fn. This
also fixes, for free: the five missing `recolor_cached_blocks` calls, the stale
`gloss_active_voice`, and `gloss_opened_from_picker`.

## Acceptance / verification

This is a render-correctness change → requires the user to run the headless e2e
or `cargo run`. Verify after refactor:

- Source-line coloring still renders on fresh visual-mode glosses (the blocker).
- A freshly created gloss narrates with the correct (reset) voice — confirms the
  `gloss_active_voice` fix.
- Alt+n/Alt+p passage navigation works from a fresh visual-mode gloss.
- recolor: cached synthesized blocks are colored on a fresh gloss.
- Escape returns to the correct place (from_picker reset).
- Alt+g (last gloss), cursor-open, and picker-open all still work (shared path
  regression).

## Files

- `src/input/actions/gloss.rs` — widen `open_gloss_overlay`; reconcile with the
  Alt+g `desired_type` param.
- `src/input/visual.rs` — collapse the six sites onto the shared fn.
- `src/gloss.rs` — `GlossContext` / `source_line_pairs` (reference).
- Possibly `GlossedPassage` if line numbers are pushed onto it instead of passed
  separately.
