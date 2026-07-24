# Journal overlay: allow Ctrl+a (ask) on a filtered single entry

**Date:** 2026-07-23
**Status:** Approved, ready to implement

## Bug

Pressing **Ctrl+a** in the journal Q&A overlay while an entry is displayed via a
one-match `journal.filter` — e.g. an entry reached through the **recent-Q&A
picker (Alt+j)** or a **Ctrl+f corpus-search hit** — shows the running-head toast
"Clear the term filter (Esc) for this key" instead of opening the ask card.

## Root cause

`open_journal_hit` (the loader the recent-Q&A picker and corpus search both use)
renders the picked entry by setting a **one-match `journal.filter`**. The Ctrl+a
handler in the journal overlay (`src/input/keymap.rs`, the `"a"` arm of the
`is_ctrl` block) is gated on `journal.filter.is_some()` and swallows the key with
the clear-filter toast. The gate's original rationale — a MULTI-match cross-work
`f` term-browse has "no clear home band" for a new Q&A — does not hold for a
single displayed entry, whose band is fully determined.

## Fix

Refine the gate so it blocks Ctrl+a only when asking would be genuinely
ambiguous, and otherwise asks against the DISPLAYED entry's own band.

In the Ctrl+a (`"a"`) arm of the journal overlay `is_ctrl` block:

- If `journal.filter.is_some()` **and** `displayed_entry_is_cross_work(s)` is
  true → keep the existing clear-filter toast. (A cross-work filter match belongs
  to a work OTHER than the one loaded; `ask_claude` grounds on `current_work`'s
  title/author/scene text, so asking would use the wrong work's context.)
- Otherwise, if `journal.filter.is_some()` → reconstruct the displayed entry's
  band from its page via the existing `band_for_rewrite(&page)` helper, set
  `s.journal_band` to it, then fall through to `begin_ask`. This covers the
  recent-Q&A picker, corpus-search hits, and any `f`-match that happens to be in
  the current work — all cases where the displayed entry is in the loaded work,
  so the ask context is correct.
- No filter → unchanged (`begin_ask` on the current band).

Rationale for the discriminator: `displayed_entry_is_cross_work` already exists
and is exactly this distinction (used by the rewrite path for the same reason —
wrong-work grounding). `band_for_rewrite` already reconstructs the precise
Work/Scene/Passage/Author band from a `JournalPage`. `displayed_journal_page`
returns the filter match's page. All three are reused; no new DB queries.

## Why set `journal_band` (not just un-gate)

`begin_ask` reads `s.journal_band` for the ask-card title AND the eventual save
targets that band. Under a filter, `journal_band` still points at the ORIGIN
cursor band, not the displayed entry's — so un-gating alone would attach the new
Q&A to the wrong band. Setting `journal_band` to `band_for_rewrite(&page)` first
makes the ask target the entry the reader is actually looking at.

## Non-goals

- No change to the multi-match `f` term-browse behavior (still gated when the
  match is cross-work).
- No change to the other filter-gated keys (`r`, `space`, `backslash`) — their
  ambiguity (new-vocab home band, TTS cache path, work-switch) is unrelated.
- No change to `open_journal_hit` / the picker load paths.

## Testing

- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Headless cage: land on a work, open the recent-Q&A picker (Alt+j), confirm an
  entry (single-match filter), press Ctrl+a → the ask card opens (INSERT) with a
  band-appropriate title; no clear-filter toast. Confirm the KEY/ACTION log and
  screenshot.
- Regression: an actual multi-match cross-work `f` term-browse still shows the
  clear-filter toast on Ctrl+a when the match is a different work.
