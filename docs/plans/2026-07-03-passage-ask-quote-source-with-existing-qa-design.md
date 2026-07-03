# Passage ask card: quote the passage source even when a Q&A already exists

## Problem

When the user visually selects passage lines and chooses **Journal Q&A** from the
Action popup, the journal overlay opens an ask card ("Ask a question about this
passage") over a transient render of the passage being asked about.

That transient render is correct only when the passage has **no** stored Q&A yet:
`render_current` shows the selected passage source (the gloss-style
`<speaker>/<verse>` render, mirroring the gloss "Glossing…" card). But when the
passage **already** has one or more Q&A pages, the passage-source render is
skipped and the overlay instead shows the *existing* Q&A (question + answer)
behind the ask card. The user asking a new question about an already-annotated
passage sees the old Q&A, not the passage text they are asking about.

## Fix (one condition)

The passage-source branch in `render_current`
(`src/input/actions/journal.rs`) is gated on `count == 0`:

```rust
if count == 0
    && s.journal.pending_passage.as_ref().is_some_and(|pp| pp.band == s.journal_band)
{
    // render the selected passage source, return
}
```

Drop the `count == 0` clause so a pending passage ask on the current band always
renders the passage source, regardless of how many stored Q&As the passage has:

```rust
if s.journal.pending_passage.as_ref().is_some_and(|pp| pp.band == s.journal_band) {
    // render the selected passage source, return
}
```

## Why this is correctly scoped

`pending_passage` matching the *current* band exists **only** during the
transient ask-card state:

- `begin_passage_ask` sets `pending_passage` (with its band) and opens the ask card.
- `ask_claude` consumes it via `take()` on submit (then shows `show_loading`).
- Cancel drops it.

So the `pp.band == journal_band` check already isolates "an ask card is open for
this passage right now." Gating additionally on `count == 0` was the only thing
making the behavior differ between the has-Q&A and no-Q&A cases; removing it makes
them consistent.

This does **not** affect:

- **Normal journal viewing** (`Ctrl+n/p` through stored Q&As): outside the ask
  flow there is no matching `pending_passage`, so `count > 0` still renders
  `show_page` as before.
- **Submit**: `ask_claude` already `take()`s `pending_passage` and switches to
  the "Asking…" loading card; this change only touches the pre-submit render.
- **The stale-pending guard**: the `pp.band == journal_band` band check (guarding
  a cancelled ask leaking onto another band) is unchanged.

## Scope

- One `if` condition in `render_current`.
- One-line tweak to the explanatory comment above that guard (it no longer only
  applies to the empty-band case).
- No signature changes, no new functions, no DB change, no ask-card/overlay-widget
  change.

## Verification

On-screen overlay-layout change → rendered check (per project CLAUDE.md):

1. `cargo build`.
2. Headless cage launch: select passage lines that already have a stored Q&A →
   `Enter` → choose **Journal Q&A** → `grim` screenshot; confirm the passage
   source (not the old Q&A) shows behind the ask card.
3. Confirm the no-existing-Q&A case still shows the passage source (unchanged).
4. Confirm normal `Ctrl+n/p` paging of stored Q&As is unaffected.
