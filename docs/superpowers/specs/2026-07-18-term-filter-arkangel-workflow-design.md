# Term-filter → Arkangel-source workflow — design

**Date:** 2026-07-18
**Status:** Approved design, ready for implementation plan
**Scope:** One implementation plan.

## Problem

Pressing `f` in the reader opens the journal term filter (cross-work tag/term
Q&A search). Today that flow is journal-overlay-centric: Escape from the term
input always drops into the journal overlay, and there is no path from a
filtered journal entry back to *reading* that entry's source in the preferred
(Arkangel) edition. The user wants a complete "find a passage by term → read it
in Arkangel" workflow.

## Desired workflow

1. **Reader `f`** → open the term-browse input (already shipped).
2. **Escape from the term input, no term chosen** → return to the **opener
   context**: the reader (current work) if `f` was pressed in the reader; the
   journal overlay if `f` was pressed inside the overlay. *(Today it always
   returns to the journal overlay.)*
3. **Choose a term** → open the journal overlay showing the **filtered subset**
   of entries for that term (already what confirm + `activate_filter` do).
4. **Escape from a journal entry in the filtered subset:**
   - **Passage entry (has a source citation):** close the overlay, load the
     entry's **`<work>-Arkangel`** edition (fall back to base when no Arkangel
     exists, e.g. Bleak House), land the cursor on the entry's **source first
     line**, and load the **Arkangel media** into MPV. The filter is discarded.
   - **Non-passage entry (scene/corpus note, no citation):** close the overlay
     to the reader (current work, cursor unchanged), clear the filter, no
     edition switch, no jump.

After step 4's jump, **Ctrl+c** (the previous-work toggle, already shipped)
returns to the pre-jump work at its exact line — so the two features compose.

## Current behavior (what changes)

- **Term-input escape** — `keymap.rs:526`:
  `InputMode::JournalTermInput => { hide(); input_mode = JournalOverlay; }`.
  Always returns to the overlay. **Change:** return to the recorded opener.
- **Term-input confirm** — `journal.rs confirm_term_input`: sets
  `JournalOverlay` + `activate_filter`. **Keep** — this is step 3. (When opened
  from the reader, confirm must still land in the journal overlay showing the
  subset; it already sets that mode, so no change.)
- **Journal-overlay Escape cascade** — `keymap.rs:1871-1884`: rewrite-browse
  drop → clear diff → clear search → **clear filter (stay in overlay)** → close
  overlay. **Change:** when a filter is active and the current entry is a
  *passage* entry, jump-to-Arkangel-source instead of clear-filter; a
  non-passage filtered entry clears the filter and closes to the reader.

## Non-goals (YAGNI)

- No change to how the term filter matches (tags + FTS5 fallback) or steps
  (Ctrl+n/p through the subset) — unchanged.
- No change to `f`/`Ctrl+f` bindings, the corpus-search popup, or the previous-
  work toggle.
- Non-passage (scene/corpus) filtered entries do NOT get a best-effort scene/
  work-start Arkangel jump — they just close to the reader.
- Escape from the term input when a term IS being typed but not confirmed still
  cancels (returns to opener); only Enter confirms.

## Architecture

Three focused changes, each reusing existing machinery.

### 1. Record the term-input opener context

Mirror the existing `JournalState.picker_from_reader` precedent
(`journal.rs:291`). Add:

```rust
// journal.rs, JournalState
/// True when the term input was opened from the READING CARD (reader `f`)
/// rather than from inside the journal overlay (overlay `f`). Consumed by the
/// term-input escape/confirm paths so Escape returns to the reader.
pub term_input_from_reader: bool,
```

**Chosen shape (no signature change):** `open_term_input(state)` stays the
single public entry and derives the flag from the *current* `input_mode` at call
time — `Reader` → `term_input_from_reader = true`, anything else (the journal
overlay) → `false`. Both call sites (the `Action::OpenJournalTermInput` reader
dispatch, and the journal-overlay `f` arm at `keymap.rs:1846-1849`) already run
in their respective modes when they call `open_term_input`, so no parameter or
wrapper is needed. Set the flag as the first line of `open_term_input`, before
it mutates `input_mode` to `JournalTermInput`.

### 2. Term-input Escape returns to the opener

`keymap.rs:526`, the `JournalTermInput` Escape arm:

```rust
InputMode::JournalTermInput => {
    s.journal_term_input.hide();
    if s.journal.term_input_from_reader {
        crate::app::return_to_reader_mode(&mut s); // reader, current work
    } else {
        s.input_mode = InputMode::JournalOverlay;
    }
}
```

`return_to_reader_mode` is the existing reader-restore chokepoint (used by the
picker escape path). The confirm path (`confirm_term_input`) is unchanged: it
sets `JournalOverlay` + `activate_filter` regardless of opener, so choosing a
term always lands in the filtered overlay subset (step 3) — correct for both
openers.

### 3. Escape from a filtered journal entry → Arkangel source

A new function `escape_filtered_entry_to_source(state) -> bool` in `journal.rs`
(or `corpus_search.rs`-adjacent), wired into the journal-overlay Escape cascade
BEFORE the `clear_filter` branch:

```rust
// keymap.rs journal-overlay Escape, replacing the `filter.is_some()` branch:
} else if state.borrow().journal.filter.is_some() {
    if !escape_filtered_entry_to_source(state) {
        // non-passage entry (no citation) or resolution failed: fall back to
        // the current behavior — clear the filter, return to the reader.
        crate::input::actions::journal::clear_filter(state);
        crate::input::actions::journal::close_overlay(state);
    }
}
```

**`escape_filtered_entry_to_source`** (load-then-resolve shape, reusing
`jump_to_journal_source_start`'s resolution + `preferred_arkangel_abbrev` +
the corpus-search cross-work load):

1. Read the displayed filtered entry (`displayed_journal_page`): its
   `work_abbrev` (canonical) + `start_citation`. If no `start_citation`
   (scene/corpus note), return `false` (caller falls back).
2. Resolve the target edition:
   `preferred_arkangel_abbrev(conn, entry.work_abbrev)` — `<work>-Arkangel` if
   it exists, else the base abbrev.
3. Close the journal overlay, clear the filter, return to reader mode.
4. **If the target edition is already the current work:** run
   `jump_to_journal_source_start`'s resolution directly against
   `current_work` — cursor to the entry's source first line. No reload, no MPV
   change.
5. **Else (cross-work / edition switch):** load the target edition off the
   reader thread (mirror `corpus_search::select`'s `spawn_blocking` +
   `display_work_at_with_prepared(None)` with `skip_mpv_discovery = false` so
   MPV discovery loads the Arkangel media), then run the source-line resolution
   against the freshly-loaded work and `navigation::jump_to_line`.

**Source-line resolution** reuses the exact logic in
`jump_to_journal_source_start` (`journal.rs:852`): parse the entry's
`start_citation` → match `(div1, div2, line_in_div)` in `work.lines` (fallback
to first plain source line text) → advance to the first dialogue line →
`line_map.work_to_buffer` → buffer index. That function currently reads the
CURRENT page's `start_citation` and jumps in the current work; the new function
generalizes it to a chosen entry + a freshly-loaded edition. Factor the
citation→buffer-index core into a shared helper both call.

## Data flow

**Open (`f`):** dispatch records `term_input_from_reader` from the current mode;
term input shows.

**Escape (no term):** hide input → opener (reader via `return_to_reader_mode`,
else journal overlay).

**Confirm (term chosen):** hide input → `JournalOverlay` + `activate_filter`
(filtered subset shown). Ctrl+n/p step the subset (unchanged).

**Escape from a filtered entry:**
- passage entry → `escape_filtered_entry_to_source`: resolve Arkangel target →
  close overlay + clear filter → (same-work) jump cursor OR (cross-work) load
  Arkangel edition + Arkangel media + jump cursor to source first line.
- non-passage entry → clear filter + close to reader (fallback).

## Error / edge handling

- **No `start_citation`** (scene/corpus note): return `false` → fallback
  (clear filter, close to reader). No edition switch.
- **No Arkangel edition** for the work (Bleak House, etc.):
  `preferred_arkangel_abbrev` returns the base; load base. Silent.
- **Citation doesn't resolve to a line** in the loaded edition: after the load,
  if resolution fails, land at the work's saved position (the load already did
  that with `None` target) — do not crash.
- **Work load fails:** toast + stay (mirror `corpus_search::select`).
- **Borrow discipline:** gather the entry data under a short borrow, drop it,
  then load/mutate under fresh borrows across the `.await` (mirror
  `corpus_search::select` — no borrow held across `.await`).

## Testing

- **Unit:** the extracted citation→buffer-index helper — given a `Work` and a
  `start_citation`, resolves the correct buffer line (primary citation match +
  the plain-source-text fallback); returns `None` for an unresolvable citation.
  (Pure over an in-memory `Work` fixture.)
- **Unit:** `term_input_from_reader` is set true when opened in `Reader` mode,
  false in `JournalOverlay` — via the mode-derived setter (no GTK).
- **Headless e2e (cage/grim/wtype):**
  1. Reader `f` → Escape → back in the reader (current work), no journal
     overlay.
  2. Journal-overlay `f` → Escape → back in the journal overlay.
  3. Reader `f` → type a term → Enter → journal overlay shows the filtered
     subset (footer "match n of m").
  4. Escape from a Shakespeare passage entry in the subset → reader loads
     `<work>-Arkangel`, cursor on the entry's source first line, Arkangel `.m4b`
     resolved (log line); title shows "(Arkangel)".
  5. Escape from a non-passage (corpus/scene) filtered entry → reader, current
     work, cursor unchanged, filter cleared.
  6. After step 4, `Ctrl+c` → returns to the pre-jump work at its exact line
     (composition with the previous-work toggle).
  Open every capture and report on-screen per the UI review protocol.

## Key files

- `src/input/actions/journal.rs` — `JournalState.term_input_from_reader`;
  `open_term_input` sets it from the current mode; `escape_filtered_entry_to_source`;
  extract the citation→buffer-index helper from `jump_to_journal_source_start`
  and share it.
- `src/input/keymap.rs` — term-input Escape arm (opener-aware, ~line 526);
  journal-overlay Escape cascade (filtered-passage branch, ~line 1878).
- Reused unchanged: `db::queries::preferred_arkangel_abbrev`,
  `corpus_search::select`'s cross-work-load pattern (or a shared load helper),
  `app::display_work_at_with_prepared`, `navigation::jump_to_line`,
  `app::return_to_reader_mode`, `journal::displayed_journal_page` /
  `activate_filter` / `clear_filter` / `close_overlay`.

## Follow-ups (out of scope)

- If the shared cross-work-Arkangel-load logic (corpus-search select + this
  escape) starts to duplicate, extract a single `load_arkangel_edition_at`
  helper. Not required for this change; note it if the second copy appears.
