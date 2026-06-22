# Q&A Journal Overlay — Design

**Date:** 2026-06-21
**Status:** Approved design, pending implementation plan

## Summary

A new reader overlay that serves as a per-work **journal** of questions and
answers about the play. The reader types a question about the scene they are
reading; the question (plus the work's title/author and the full scene text) is
sent to the Claude API; the answer is stored and displayed. Each Q&A pair is a
**page**. One journal per work; each scene owns zero or more pages.

This is a sibling of the existing **gloss overlay** and reuses its proven
machinery (input-mode key capture, nested widget-chain overlay, clip-free
scroll path, and the `glib::spawn_future_local` + `tokio_handle.spawn` Claude
bridge). It does **not** reuse the `GlossOverlay` widget itself (that widget
already triple-duties as gloss/synopsis/echoes with a tag-based block model that
does not fit Q&A content) — it gets its own `JournalOverlay` widget that copies
the rendering/scroll machinery.

## Concepts

- **Journal** — all journal entries for one work (`work_abbrev`, `-Amb`
  normalized so editions share a journal).
- **Scene** — a `(div1, div2)` pair (act, scene). The established "one card per
  scene" key, identical to the synopsis overlay's `synopsis_overlay_scene`.
- **Page** — a single question + its answer. A scene owns zero or more pages.
  One `journal_entries` row = one page.

## Q&A scope (what Claude receives)

For each question:

- **System prompt** — a new `JOURNAL_QA_PROMPT` (`LazyLock<String>` in
  `src/gloss.rs`), loaded from the lit.db `prompts` table via the existing
  `template_or` / `active_prompt` mechanism with a compiled fallback. Frames
  Claude as a literary interlocutor answering a reader's question about a
  specific scene. Crucially, the prompt **encourages situating the scene within
  the whole play** — drawing on Claude's knowledge of the complete work to trace
  foreshadowing, echoes, and thematic arcs both backward and forward (e.g.
  connecting Hamlet's "what dreams may come" ruminations in 3.1 to his "the
  readiness is all" / "undiscovered country" resolve in 5.2). Reader-focused,
  substantive.
- **User message** — assembled from:
  - the work's **title and author**,
  - the **full current scene text** (all lines for the current `(div1, div2)`,
    with speakers, grouped from `work.lines`),
  - the **question**.
- **Model** — `state.config.claude_model` (currently `claude-opus-4-8`), same as
  glosses.

**Whole-play awareness comes from Claude's training knowledge**, not from
sending the full work. The verbatim scene text grounds the answer in *this*
moment; Claude's deep familiarity with the canonical play supplies the
cross-scene connections. This is cheapest and works well for the canonical
Shakespeare corpus Claude knows thoroughly. (Known limitation: for lesser-known
or heavily-edited works Claude knows less well, whole-play connections will be
weaker; sending per-scene `scene_synopses` as a play map, or the full work text,
are possible future enhancements if this proves insufficient — see Future
phases.)

**No spoiler/progress gating.** Unlike Kindle "Ask This Book" / Google Play
Books "Book Insights" (which restrict answers to the reader's current position),
this journal is a study tool for a reader engaging the whole work — it
deliberately permits and encourages forward-looking connections across the
play.

## Data model

One new linux-lit-owned table in `lit.db`:

```sql
CREATE TABLE IF NOT EXISTS journal_entries (
  id          INTEGER PRIMARY KEY,
  work_abbrev TEXT    NOT NULL,
  div1        INTEGER NOT NULL,   -- act
  div2        INTEGER NOT NULL,   -- scene
  question    TEXT    NOT NULL,
  answer      TEXT    NOT NULL,
  claude_model TEXT,
  timestamp   TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_journal_work_scene
  ON journal_entries(work_abbrev, div1, div2, timestamp);
```

- One row = one page.
- `(work_abbrev, div1, div2)` is the scene key. `normalize_abbrev` strips
  `-Amb` before reads/writes so editions share a journal (same as glosses).
- Created at startup by a new `ensure_journal_entries` near the other
  `ensure_*` calls (`app.rs` ~2671). Unlike `glosses`/`passages`/`scene_synopses`
  (externally managed by the gloss-db repo), this table is owned by linux-lit,
  so the app creates it.
- No FK to `passages` — the journal is scene-scoped, independent of the gloss
  tables.

### Query functions (new, in `src/db/queries.rs`)

- `save_journal_entry(work_abbrev, div1, div2, question, answer, model) -> id`
  — insert one page; returns its id.
- `find_journal_pages(work_abbrev, div1, div2) -> Vec<JournalPage>` — all pages
  for a scene, ordered by `timestamp ASC` (chronological).
- `find_journal_scenes(work_abbrev) -> Vec<(i64, i64)>` — distinct
  `(div1, div2)` that have at least one page, in scene order (for Alt+n/Alt+p
  scene jumps).
- `delete_journal_entry(id)`.
- `update_journal_entry(id, question, answer, model)` — for edit.
- Struct `JournalPage { id, div1, div2, question, answer, claude_model, timestamp }`.

## Navigation

The journal opens to the **current scene under the reading cursor** (mirrors the
synopsis overlay). Within `input_mode == JournalOverlay`, key capture is owned by
a new `handle_journal_key` (cloned from `handle_gloss_key`), so reader-mode binds
(e.g. `Ctrl+p` = library picker) do not collide.

- **`j` / `k`** — scroll the current page (long answers), using the row-snap
  clip-free scroll path. A page is one Q&A, so no block-cursor model is needed.
- **`Ctrl+n` / `Ctrl+p`** — flip to the next/prev **page within the current
  scene** (chronological order). Clamps at the first/last page of the scene (no
  wrap, no auto-advance into the next scene — use `Alt+n`/`Alt+p` for that).
- **`Alt+n` / `Alt+p`** — jump to the next/prev **scene that has pages** (skips
  empty scenes), landing on that scene's **first** page.
- **`Escape`** — close, restore `input_mode = Reader`, return the reading cursor
  to where reading left off (`journal_return_pos`).

### Open semantics

- Bind: **`Ctrl+j`** in reader mode → `Action::ToggleJournalOverlay` →
  `journal::toggle_overlay`. (`Ctrl+j` confirmed free in `keymap_config.rs`.)
- Determine the current scene via `current_scene_divs(state)` (the same helper
  the synopsis overlay uses).
- If the scene has pages: show its **first** page (chronological).
- If the scene has **zero** pages: show an **empty page** for that scene —
  header + "No pages yet — press `a` to ask." Asking creates the scene's first
  page.

## Asking / editing / deleting

- **`a`** — open the stacked input "ask card" (reuse the gloss
  `ask_container` pattern: `Tab`/`Ctrl+Enter`/`Escape` intercepted first). Type a
  question; `Ctrl+Enter` submits.
  - Show loading state.
  - `glib::spawn_future_local(async { tokio_handle.spawn(claude::send_message(
    JOURNAL_QA_PROMPT, user_message, model)).await ... })`.
  - On `Ok(Ok(answer))`: re-borrow `AppState`, `save_journal_entry(...)`, reload
    the scene's pages, and **land on the new page** (appended at the end,
    chronological). Error arm shows the error text in the card.
- **`e`** — edit the current page's question and re-ask (overwrites the page's
  answer via `update_journal_entry`). Reuses the gloss edit-card pattern.
- **`d`** — delete the current page via a delete-confirm sub-overlay (reuse the
  gloss `delete_confirm_*` pattern). After delete, land on the previous page in
  the scene, or the empty-scene state if none remain.

## Rendering

New widget `JournalOverlay` (`src/ui/journal_overlay.rs`), attached in the
`build_window` overlay chain the same way the gloss/synopsis overlay is (own
`scrim` + `container` + `gtk4::TextView` + `gtk4::ScrolledWindow` +
`bottom_clip`). It copies the gloss overlay's clip-free scroll/row-snap path
(`scroll`, `scroll_to_top/bottom`, `update_bottom_clip`, `snap_value_to_line`).

A page card shows, top to bottom:

1. **Header** — scene label, e.g. "Hamlet — Act 1, Scene 2" (via the existing
   `scene_label` / `synopsis_label` helpers).
2. **Page position** — e.g. "page 2 of 3 in this scene".
3. **Question** — styled as a heading.
4. **Answer** — paragraph(s) below, scrollable.

Empty scene → header + position ("page 0 of 0") + the "No pages yet — press `a`
to ask." hint.

## AppState fields (new, in `src/app.rs`)

Modeled on the `gloss_*` fields:

- `journal_overlay: JournalOverlay` — the widget.
- `journal_scene: (i64, i64)` — current `(div1, div2)` page key (analogue of
  `synopsis_overlay_scene`).
- `journal_pages: Vec<JournalPage>` — pages for the current scene.
- `journal_page_index: usize` — current index into `journal_pages`.
- `journal_return_pos: Option<(usize, usize)>` — saved
  `(current_line, page_top_line)` for Escape return.
- `journal_prompt_active: bool` (or reuse a `JournalPromptMode { Ask, Edit }`) —
  whether the ask/edit card is open.
- `journal_delete_confirm_*` — the d-key confirm sub-overlay (or reuse the
  shared confirm overlay if feasible).

Open/close keyed off `input_mode == InputMode::JournalOverlay` (new variant in
the `InputMode` enum, `app.rs:48`).

## Keybind plumbing (project rules)

All three must be updated for every new/changed bind:

1. `src/input/keymap_config.rs` — compiled defaults: `Ctrl+j` →
   `ToggleJournalOverlay`.
2. `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — the stow source
   (JSON overrides compiled defaults; both must agree).
3. `src/ui/keybinds_overlay.rs` — the `Ctrl+/` overlay: add the `Ctrl+j` cap +
   `describe()` arm (use the `update-cairo-keybinds-overlay` skill). The
   in-overlay binds (`a`/`e`/`d`/`j`/`k`/`Ctrl+n`/`Ctrl+p`/`Alt+n`/`Alt+p`/
   `Escape`) are documented as journal-overlay-mode binds.

New `Action` variant: `Action::ToggleJournalOverlay` in
`src/input/actions/mod.rs`, dispatched in `keymap.rs` to
`journal::toggle_overlay`. In-overlay keys routed by a new `handle_journal_key`
branch on `input_mode == JournalOverlay` (and a sub-branch for the ask/edit card
and delete-confirm).

## New / changed files

- `src/ui/journal_overlay.rs` — new widget (copies gloss overlay scroll/clip).
- `src/input/actions/journal.rs` — new module: `toggle_overlay`, `open`, `close`,
  `ask`, `edit`, `delete`, page/scene navigation, the Claude bridge.
- `src/db/queries.rs` — `JournalPage`, `save/find/find_scenes/update/delete`.
- `src/gloss.rs` (or a small new prompt module) — `JOURNAL_QA_PROMPT`.
- `src/app.rs` — `InputMode::JournalOverlay`, AppState fields,
  `ensure_journal_entries`, overlay construction in `build_window`,
  `JournalPromptMode` if used.
- `src/input/keymap.rs` — `handle_journal_key`, dispatch arm.
- `src/input/keymap_config.rs` + `keymap.json` + `keybinds_overlay.rs` — binds.

## Future phase: tagging (theme / mood / character)

Not built in the first version, but sketched here so the data model doesn't
paint us into a corner. Reading-app precedent: StoryGraph's fixed mood
vocabulary, Glasp's per-highlight tags, Ryan Holiday's theme-per-notecard, and
Obsidian per-character backlinks; Marvin's "Deep View" per-character
concordance is the closest analogue and dovetails with linux-lit's existing
concordance system.

Design sketch:

- **Storage** — a `journal_tags(entry_id, tag)` side table (many-to-many),
  rather than a column on `journal_entries`, so a page can carry multiple tags
  and tags can be queried independently. `entry_id` FK → `journal_entries.id`.
- **Tag kinds** — free-form tags, with optional convention prefixes the UI can
  group on: `theme:mortality`, `mood:foreboding`, `char:Hamlet`. Character tags
  could be auto-suggested from the scene's speakers (already on `Line.speaker`)
  and reconciled with the concordance's character list.
- **Keyboard tagging** — a `t` key in the journal overlay opens a small tag
  input/picker on the current page (mirrors the gloss add-card pattern).
- **Filtering / cross-scene grouping** — a tag filter (e.g. in a future journal
  picker) collects every page across the work carrying a tag — a per-character
  or per-theme reading thread that cuts across scenes, the journal analogue of
  the concordance's cross-work word navigation.
- **Export** — tags travel with each entry in any future export.

This stays out of the first implementation; the only forward-compatible
requirement is that `journal_entries.id` be a stable primary key a `journal_tags`
table can reference later (already the case).

## Out of scope (deferred)

- TTS / audio for journal answers (glosses have it; journal does not, initially).
- Cross-work journal navigation (journal is per-work).
- Multi-turn threaded conversations on a page (each page is one independent Q&A).
- A journal picker (analogous to the gloss picker) — could be added later if
  flipping pages by keyboard proves insufficient. (A tag filter, per the tagging
  future phase, would likely live here.)
- Sending full work text or per-scene synopses as Claude context — whole-play
  awareness comes from training knowledge in v1 (see Q&A scope); these are
  fallbacks only if that proves insufficient.
- Tagging by theme/mood/character — designed as a future phase (see above), not
  built initially.
- Spoiler/progress gating — deliberately omitted (see Q&A scope).
- Clickable line-ID citations, preset question prompts, journal export, and
  spaced review/resurfacing — researched but not selected for this design;
  candidates for later iterations.

## Verification

- `cargo build` / `cargo test --bins` for the pure logic (queries, scene
  grouping, navigation state machine).
- Runtime/visual checks (overlay layout, scroll clipping, ask-card geometry,
  reveal timing) require the headless e2e harness; per project rule, the **user**
  runs `./scripts/e2e-env.sh ...` and the manual `cage` + `grim` launch, since an
  agent cannot drive cage on the live session. The agent will build, run the
  pure suite, and state plainly that runtime verification is user-gated.
