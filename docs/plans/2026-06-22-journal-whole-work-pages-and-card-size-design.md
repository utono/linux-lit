# Q&A Journal — Whole-Work Pages, Card-Size Parity, Shared `-Amb` Journal

**Date:** 2026-06-22
**Status:** Approved
**Branch:** `feat/qa-journal-overlay` (extends the shipped Q&A journal overlay)

Follow-up to `2026-06-21-qa-journal-overlay-design.md`. The base journal is
already built: `Ctrl+j` opens an overlay scoped to the cursor's scene
`(div1, div2)`, where each *page* is one Q&A pair sent to Claude. This spec adds
three refinements requested after the first build:

1. **Card-size parity** — the journal overlay must be the same size as the main
   reading card (it currently shrinks to 0.8×).
2. **Whole-work pages** — pages that belong to the play as a whole, not to any
   one scene, reached via a separate "Work" band.
3. **Shared `-Amb` journal** — confirm `2H6` and `2H6-Amb` share one journal.

---

## Item 3 — Shared `-Amb` journal (already satisfied; no code change)

The journal already keys every save/load on `crate::app::base_work_abbrev()`
(`journal.rs:14, :100, :166`), which strips at the first `-`, so
`base_work_abbrev("2H6-Amb") == "2H6"`. This is the **established convention**:
glosses (`gloss.rs:2011`) and synopses (`synopsis.rs:132`) already key their
per-work data the same way. Therefore `2H6` and `2H6-Amb` already write to and
read from one journal.

**Scope:** No production change. Add one DB roundtrip test in `journal.rs`
asserting that a page saved under the base abbrev `"2H6"` is found when querying
`"2H6"`, documenting the contract that callers always pass `base_work_abbrev`.
(The collapsing also folds e.g. `Mac-Ep-1..6` → `Mac`; that is the pre-existing,
intended behavior of `base_work_abbrev` and is out of scope here.)

---

## Item 1 — Card-size parity with the main reading card

### Current behavior
`JournalOverlay::size_card` (`journal_overlay.rs:149`) computes
`w = card_width * 0.8`, `h = card_height * 0.8` and calls
`container.set_size_request(w, h)`. The caller (`journal.rs:34`) already passes
`content_hbox.width()` / `content_hbox.height()` — the main reading card's own
allocation — but the 0.8 multiplier shrinks the overlay below it.

### Target behavior
Size the overlay container to the passed dimensions **verbatim**, matching the
gloss overlay (`gloss_overlay.rs:658-659`, which calls
`set_width_request(card_width)` / `set_height_request(card_height)` with no
scaling). Result: the journal overlay occupies exactly the main reading card's
footprint.

### Change
In `JournalOverlay::size_card`, drop the `0.8` factors:

```rust
fn size_card(&self, card_width: i32, card_height: i32) {
    self.container.set_size_request(card_width, card_height);
    self.last_card_size.set((card_width, card_height));
    self.view.set_left_margin(self.text_margins);
    self.view.set_right_margin(self.text_margins);
    let _ = self.column_width;
}
```

`show_loading` / `show_message` already restore `last_card_size`, so they inherit
the corrected size with no further change.

### Acceptance
Visual only: with the journal open, the overlay card fills the same rectangle as
the main reading card behind it. Eyeball via a headless `cage` launch (the user
runs it; an agent cannot drive `cage` on the live dwl seat).

---

## Item 2 — Whole-work pages (the "Work" band)

### Concept
A *whole-work page* is a Q&A about the play as a whole — themes, character arcs
across acts, the ending — not tied to any scene. The journal therefore has two
**bands**:

- **Scene bands** — one per `(div1, div2)` that has pages (today's behavior).
- **A single Work band** — holds all whole-work pages for the work.

For a whole-work Q&A, the prompt sends the work's **title and author only** (no
scene text); whole-play awareness comes from Claude's training knowledge — the
same mechanism the base spec chose for scene Q&A context.

### Data model

Add a `scope` column to `journal_entries`:

```sql
scope TEXT NOT NULL DEFAULT 'scene'   -- 'scene' | 'work'
```

- `scope='scene'` (default) — existing behavior; `div1`/`div2` identify the scene.
- `scope='work'` — whole-work page; `div1`/`div2` are stored as `-1, -1` and
  ignored on read.

**Migration:** `ensure_journal_table` adds the column in the `CREATE TABLE`
body, plus an idempotent guard for any DB whose table predates the column:

```rust
// after CREATE TABLE IF NOT EXISTS ...:
let has_scope = conn
    .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='scope'")?
    .exists([])?;
if !has_scope {
    conn.execute_batch(
        "ALTER TABLE journal_entries ADD COLUMN scope TEXT NOT NULL DEFAULT 'scene';",
    )?;
}
```

`journal_entries` currently has **zero rows** (verified), so no data migration is
required; the guard exists only for robustness. `db::queries` selects the
`journal_entries` table; no `SNAPSHOT_VERSION` bump (that governs `LineMap`
serialization, unrelated).

### Query changes (`src/db/journal.rs`)

- `save_journal_page(conn, abbrev, div1, div2, q, a, model, scope)` — gains a
  `scope: &str` argument; writes it.
- `update_journal_page` — unchanged signature (editing never changes scope).
- `find_journal_pages(conn, abbrev, div1, div2)` — add `AND scope='scene'`.
- `find_work_pages(conn, abbrev)` — **new**:
  `WHERE work_abbrev=?1 AND scope='work' ORDER BY timestamp ASC, id ASC`.
- `find_journal_scenes(conn, abbrev)` — add `AND scope='scene'` so the Work
  band never appears as a phantom scene in `Alt+n/p` scene iteration.

### State model (`src/app.rs`)

Replace the bare `journal_scene: (i64, i64)` "current location" with an explicit
band:

```rust
pub enum JournalBand {
    Work,
    Scene(i64, i64),
}
```

- `AppState.journal_band: JournalBand` — the band currently shown. Initialized
  to `JournalBand::Scene(0, 0)` (mirrors the old `(0,0)` default).
- `journal_page_index`, `journal_return_pos`, `journal_prompt_mode` — unchanged.
- `journal_pages: Vec<JournalPage>` — now holds whichever band's pages were last
  loaded (scene or work).

`Ctrl+j` open behavior unchanged: it sets the band to `Scene(current_scene_divs)`
and renders. (Opening always lands on the cursor's scene, never the Work band.)

### Navigation (`handle_journal_key`, `src/input/keymap.rs`)

All keys stay inside the journal overlay's own keyspace.

- **`Alt+w`** — switch to `JournalBand::Work`; load + render its pages
  (empty-band card if none).
- **`Alt+n` / `Alt+p`** — jump to next / prev *scene* with pages. From the Work
  band, `Alt+n` lands on the first scene with pages, `Alt+p` on the last
  (i.e. the Work band sorts before all scenes). Within scenes, behavior is
  unchanged. There is exactly one Work band, so `Alt+w` from the Work band is a
  no-op (re-renders).
- **`Ctrl+n` / `Ctrl+p`** — flip pages within the current band (work or scene),
  unchanged semantics.
- **`a`** — ask, adding a page to the **current band**:
  - In a scene band → `scope='scene'`, prompt includes the scene text (today's
    path).
  - In the Work band → `scope='work'`, prompt sends **title + author only**
    (new `ask_claude` path; reuses `JOURNAL_QA_PROMPT`, omitting the scene-text
    block).
- **`e` / `d` / `j` / `k` / `g`/`G` / `Escape`** — unchanged; operate on the
  current band's current page. `e` (edit) re-asks within the same band/scope.

### Action layer (`src/input/actions/journal.rs`)

- `render_current` — branch on `s.journal_band`: load via `find_work_pages` for
  `Work`, `find_journal_pages` for `Scene`. Title/position label:
  - Work band → title "Whole work" (or the work title) and
    "page N of M for the whole work".
  - Scene band → existing scene synopsis label and "page N of M in this scene".
- `nav_scene` — operate over `find_journal_scenes` (scene-only); add the Work→
  first/last-scene entry/exit rule.
- New `nav_to_work_band(state)` — sets `journal_band = Work`, resets index, renders.
- `begin_ask` / `ask_claude` — read `s.journal_band`; for `Work`, skip the
  scene-text assembly (`scene_text_for`) and write `scope='work'`,
  `div1=div2=-1`. For `Scene`, unchanged.
- `delete_current` — deletes the current band's current page by `id` (scope is
  implicit in the row); unchanged.

The page-vanish invariant from the base spec still holds: save and reload use the
same `base_work_abbrev` and, within a band, the same scope + key.

### Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs`)

Per the mandatory overlay-sync rule, update the journal detail panel to document
the in-overlay binds: `Alt+w` (Work band), `Alt+n`/`Alt+p` (scene jump),
`Ctrl+n`/`Ctrl+p` (pages), `a`/`e`/`d`, `j`/`k`/`g`/`G`, `Escape`. These are
overlay-internal (handled in `handle_journal_key`), not reader binds, so
`keymap.json` / `keymap_config.rs` are **not** touched — only the descriptive
overlay. Run the `update-cairo-keybinds-overlay` skill's cross-reference.

### Acceptance
- `cargo test --bins` — journal DB tests cover: scene/work scope isolation
  (`find_journal_pages` excludes work pages and vice versa), `find_work_pages`
  roundtrip, `find_journal_scenes` excludes the Work band, shared-`-Amb` (Item 3).
- Visual (user-run via `cage`): `Alt+w` shows the Work band; `a` there asks a
  title+author-only question; `Alt+n` returns to a scene; scene and work pages
  don't bleed into each other.

---

## Out of scope
- Tagging (theme/mood/character) — still a future phase per the base spec.
- Multi-scene / scene-set pages — explicitly not built (whole-work is the only
  non-scene scope).
- Changing `base_work_abbrev`'s collapsing semantics for non-`-Amb` suffixes.
- Surfacing `claude_model` / `timestamp` provenance in the UI.
- Journal-overlay font parity with the reader (deferred, cosmetic).
