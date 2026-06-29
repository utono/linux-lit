# Passage-scoped journal Q&A (with gloss↔journal linkage) — design

**Date:** 2026-06-24
**Status:** Approved, ready for implementation plan

## Overview

A new **passage scope** for the journal (`n`) Q&A feature: ask Claude about a
specific selected passage rather than the whole scene/work. The journal page
stores and displays the passage's source verse — with italic stage directions,
reusing the gloss overlay's rendering — above the Q&A. Journal pages and glosses
that share a passage are **linked**: from either overlay the user can VIEW the
counterpart (if it exists) or CREATE it, because both are keyed by the same
`(start_citation, end_citation)`.

## Background (current state, verified)

- Journal Q&A today (`src/input/actions/journal.rs`, `src/ui/journal_overlay.rs`,
  `src/db/journal.rs`) has two scopes via a `scope` column:
  - `JournalBand::Work` → ask about the whole work (`scope='work'`, div = the
    `JOURNAL_WORK_DIV` sentinel).
  - `JournalBand::Scene(d1,d2)` → ask about the current scene; Claude gets
    `scene_text_for(d1,d2)`.
  - Pages are stored in `journal_entries(work_abbrev, div1, div2, question,
    answer, claude_model, scope, timestamp)` — NO line range, NO source verse.
  - The overlay renders pages as plain text: `format!("{question}\n\n{answer}")`
    via `set_text` (`journal_overlay.rs:160`). No markup.
- Gloss creation from a visual selection (`src/input/visual.rs:~403`) builds
  `selected_lines` from the buffer range (now including stage `sub_line` rows),
  derives `source_text` markup via `echoes::build_source_header` (emits
  `<speaker>/<verse>/<stage>`), and computes `start_citation`/`end_citation`.
  Glosses are keyed by `(work_abbrev, start_citation, end_citation)` through the
  `passages` table (`find_glosses_by_start`, queries.rs:1518). `GlossContext`
  carries `start_citation`/`end_citation`/`source_text` (gloss.rs:499-500).
- The gloss overlay renders the stage-aware verse via `populate_gloss_buffer`
  (`gloss_overlay.rs:1805`), with the `gloss-stage` italic tag re-prioritized
  above the buffer-wide font tag in `apply_font` (the just-fixed italic
  behavior, gloss_overlay.rs:444).
- Reader-gloss main-card tint is keyed by citation range
  (`apply_reader_gloss_highlighting`, `line_in_any_passage`).

## Design — components

### 1. Data model

Extend `journal_entries` via the existing idempotent `ALTER TABLE` migration in
`ensure_journal_table` (`db/journal.rs:14`), adding three NULLABLE columns:

- `start_citation TEXT` — passage start, `ABBR.div1.div2.line_in_div` (same form
  as glosses). **The join key to glosses.**
- `end_citation TEXT` — passage end.
- `source_text TEXT` — the selected verse verbatim with `<speaker>/<verse>/
  <stage>` markup, so display is self-contained (stable even if the work's text
  later changes).

`scope='passage'` reuses the existing `scope` column. `div1,div2` carry the
passage's scene (parsed from the start citation). Legacy scene/work pages leave
the three new columns NULL.

`JournalPage` (`db/journal.rs:4`) gains `start_citation: Option<String>`,
`end_citation: Option<String>`, `source_text: Option<String>`. The existing
`find_journal_pages`/`find_work_pages`/`find_all_pages_ordered` SELECTs add the
three columns (NULL for scene/work rows). New DB functions:

- `save_passage_page(conn, work_abbrev, div1, div2, start_cit, end_cit,
  source_text, question, answer, model) -> i64`.
- `find_passage_pages(conn, work_abbrev, start_cit, end_cit) -> Vec<JournalPage>`
  (scope='passage', matched by the citation pair) — used by both the passage
  band and the gloss→journal toggle.
- `update_journal_page`/`delete_journal_page` already work by id, unchanged.

Migration is additive and idempotent (mirror the `scope`-column ALTER guard);
no snapshot/version concerns (journal data is not in the LineMap snapshot).

### 2. Entry points (creation)

Three creation flows, all converging on `save_passage_page`. Claude context for a
passage question is **the selected passage + its enclosing scene** (the passage
focuses the question; the scene gives Claude context) — built by combining the
passage `source_text` with `scene_text_for(d1,d2)`.

**(a) From a visual selection** (primary, gloss-like): in reader Visual mode,
`Return` opens the action popup (`BUILTIN_ACTIONS`, visual.rs:129). Add a
**"Journal Q&A"** item. Selecting it:
- builds `selected_lines` from the visual range (includes stage rows),
- `source_text = build_source_header(&selected_lines, speaker)`,
- `start_citation`/`end_citation` from the first/last selected line's
  `(div1,div2,line_in_div)`,
- opens the journal ask card (reuse `journal::begin_ask`'s card, in a new
  passage prompt mode), and on submit calls Claude then `save_passage_page`.

**(b) From the gloss overlay** (new): a key (`J`) creates a journal page for the
gloss's CURRENT source text. The overlay's `gloss_context` already holds
`start_citation`/`end_citation`/`source_text` — reuse them directly (no new
selection), open the ask card, save a passage page with the **same citations as
the gloss** (guaranteeing linkage). Must update the Ctrl+/ keybinds overlay and
keymap.json per the repo keybind rules.

**(c) From a journal passage page** (vice versa): a key creates a gloss for the
page's source text, reusing the stored `start_citation`/`source_text` to drive
the existing reader-gloss creation path. Must update the keybinds overlay.

### 3. Display

For a **passage page** (one whose `source_text` is non-empty), the journal
overlay renders:
- **Source verse on top** — speaker small-caps + indented verse + ITALIC stage
  directions, by REUSING the gloss overlay's stage-aware render rather than
  duplicating it. Extract the shared routine (the `populate_gloss_buffer`
  tag-building + element render + the `apply_font` italic-priority re-assertion)
  into a small shared helper (e.g. `ui::gloss_render`) that BOTH overlays call,
  so the italic-tag fix is inherited, not re-implemented.
- a thin rule,
- then **Question / Answer** below (as today).

Scene/work pages (no `source_text`) render exactly as now — unchanged
`format!("{question}\n\n{answer}")` path. The position/footer label shows
"passage div1.div2.start–end" for passage pages (mirroring the scene label).

### 4. Passage band + gloss↔journal toggle

- **Passage band:** passage pages live in the SAME journal overlay, in a new
  `JournalBand::Passage(start_citation, end_citation)` alongside `Work`/`Scene`.
  The band's pages come from `find_passage_pages`. Opening a passage page (from
  creation, the picker, or a toggle) sets this band. Band navigation
  (`nav_scene`/`nav_to_work_band`) is extended only as needed to reach passage
  pages; the existing picker (`JournalQaPicker`) lists passage pages too (grouped
  after scene pages, labeled by citation).

- **View-counterpart toggle (both directions):**
  - In a journal passage page, a key looks up glosses for its
    `(start_citation,end_citation)` via `find_glosses_by_start`. If found, open
    the gloss overlay on that passage; else toast "No gloss for this passage".
  - In the gloss overlay, a key looks up `find_passage_pages` for the gloss's
    citations. If found, open the journal overlay's passage band on them; else
    toast "No journal page for this passage".
  - Toggle does nothing destructive; it only switches overlays when the
    counterpart exists, else toasts.

Toggle and create are DISTINCT keys (view-counterpart vs create-counterpart),
both reciprocal. All four key additions (create-journal-from-gloss,
create-gloss-from-journal, view-gloss-from-journal, view-journal-from-gloss)
must be reflected in `keymap.rs`, `keymap.json` (stow source), and the Ctrl+/
keybinds overlay (`update-cairo-keybinds-overlay` skill).

## Out of scope

- Re-deriving source verse from live work lines (we store it verbatim).
- Linking via the `passages` table FK (we use the citation pair as a soft key,
  matching how glosses are already looked up).
- Cross-work passage Q&A (a passage is within one work/scene).
- Side-by-side verse/Q&A layout (single-column, verse-on-top).

## Testing

Pure unit tests (`cargo test --bins`, mirroring the existing `db/journal.rs`
in-memory tests):
- `save_passage_page` + `find_passage_pages` round-trip; passage pages excluded
  from scene/work queries and vice versa.
- The new columns migrate idempotently (extend `ensure_table_is_idempotent`).
- `JournalPage` carries the citations/source_text through load.
- Citation parsing for the band/toggle reuses `app::parse_citation` (already
  tested).
- The shared render helper extraction keeps the gloss overlay's existing tests
  green (including the `gloss-stage` priority test).

Headless/visual (user-run per CLAUDE.md): a passage page shows the verse with
italic stage directions above the Q&A; the gloss↔journal create and view toggles
work both directions; the "no counterpart" toasts appear.

## Risks

- **Shared-render extraction** is the main structural change — it must preserve
  the gloss overlay's exact behavior (the `gloss-stage` priority test guards
  it). Done as a pure refactor step (extract, both overlays call it, gloss tests
  stay green) before the journal consumes it.
- **Band navigation** in the journal overlay is the most intricate existing
  code; the passage band must not break scene/work navigation. Add it
  additively, with the picker as the primary way to reach passage pages.
- **Keybind coverage:** four reciprocal keys across two overlays — all must land
  in keymap.rs + keymap.json + the Ctrl+/ overlay, or they drift.

## Acceptance criteria

- A visual selection → "Journal Q&A" creates a passage page; Claude answer saved
  with `(scope='passage', start/end citation, source_text)`.
- The page displays the source verse (italic stage directions) above the Q&A.
- From the gloss overlay, a key creates a journal page for the gloss's passage
  (same citations); from a journal passage page, a key creates a gloss — both
  reciprocal.
- From either, a view-toggle key opens the existing counterpart, or toasts when
  none exists.
- `cargo build`, `cargo test --bins`, `cargo clippy` pass; the gloss overlay's
  existing tests (incl. the stage-italic priority test) stay green.
