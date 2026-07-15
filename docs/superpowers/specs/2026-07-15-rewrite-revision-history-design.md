# Rewrite Revision History, Diff-Highlight & Browsing — Design

**Date:** 2026-07-15 (US Central)
**Status:** Approved design, ready for implementation plan
**Surfaces:** gloss overlay (Edit-Gloss custom-prompt rewrite) + journal overlay (Q&A custom-prompt rewrite)

## Problem

When a journal Q&A or a gloss is rewritten by a **custom prompt** typed into the
Edit-Gloss card or the journal ask input, the rewritten text should be
**highlighted until the user presses Escape**, so it is obvious what the rewrite
produced. Today a custom-prompt rewrite silently replaces the whole body with no
visual cue.

The request grew, in brainstorming, into three connected pillars plus a restore
action — all gated on the same trigger.

## Trigger (applies to every pillar)

A **custom-prompt AI rewrite only**:

- Gloss: `edit_gloss` (`src/input/actions/gloss.rs:1371`), reached from the
  Edit-Gloss ask card (`GlossPromptMode::Edit`) or from the gloss vim editor via
  `R` → `vim_open_rewrite`.
- Journal: `rewrite_with_claude` (`src/input/actions/journal.rs:1810`), reached
  from the journal ask input via `R` → `vim_open_rewrite` → `submit_prompt`.

**Excluded** (no versioning, no highlight): hand-edit `:w` saves in the vim
editor (`gloss::vim_save`, `update_journal_page` save-as-is), single-level undo,
and any non-custom-prompt regenerate. Both included paths already complete in an
`on_success` closure dispatched **on the GTK main loop** by
`claude_bridge::run_claude_request` (`src/input/actions/claude_bridge.rs:15`) —
so all new work runs synchronously on the UI thread inside those closures.

## Pillar 1 — Durable revision history in lit.db

Replace the current **single-level, in-memory, exit-volatile** undo
(`journal_undo` / `gloss_undo`, one `(id, …, text)` tuple each) with a durable,
append-only **full history**.

### Schema

One shared table, keyed by a surface discriminator + entry id:

```sql
CREATE TABLE IF NOT EXISTS rewrite_revisions (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL,          -- 'journal' | 'gloss'
    entry_id     INTEGER NOT NULL,          -- journal_entries.id OR glosses.id
    question     TEXT,                       -- journal only; NULL for gloss
    body         TEXT    NOT NULL,           -- the answer (journal) or gloss markup
    claude_model TEXT,
    prompt       TEXT,                       -- the custom instruction that PRODUCED
                                             -- the NEXT version (context for browsing)
    timestamp    TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_rewrite_revisions_entry
    ON rewrite_revisions(kind, entry_id, timestamp);
```

### Ownership & migration

- **litdb owns the schema.** Add a migration `.sql` under
  `~/utono/litdb/scripts/migrations/` and the table to litdb's canonical schema.
- **linux-lit auto-migrates on DB open**, mirroring `ensure_claude_model_columns`
  (`src/db/queries.rs:890`) and its `column_exists` probe (queries.rs:871): an
  idempotent `CREATE TABLE IF NOT EXISTS` + index at open time, so the feature
  works even before litdb re-runs.

### Append rule

Inside each custom-prompt `on_success`, **before overwriting the live row**,
append the current (pre-rewrite) version as a revision row (carrying the custom
`prompt` that produced the incoming version). The durable row supersedes the
in-memory `*_undo` tuple as the undo/restore source. (Existing rows predate the
table; history simply begins at first rewrite — no backfill.)

## Pillar 2 — Diff-highlight until Escape

On a custom-prompt rewrite, tint the words the rewrite changed/added, in the
freshly-rendered new text.

- **Word-level diff** between the previous version and the new version. New pure,
  unit-tested module (e.g. `src/input/rewrite_diff.rs`) returning **char ranges
  in the rendered text** for the runs that are new or changed in the NEW body.
- **Offset mapping:** diff on the **rendered plain text** the TextView shows —
  for gloss that is the post-`strip_hi_spans` / post-markup text
  (`gloss_render::populate_gloss_buffer`), NOT the raw `<verse>/<gloss>` markup —
  so ranges align with the buffer. Journal answers render more directly.
- **Apply:** a dedicated **ephemeral TextTag per overlay**, distinct from the
  `<hi>` and search tags so it clears independently, but **reusing the
  search-match color** (`set_search_colors` wiring: gloss_overlay.rs:658,
  journal_overlay.rs:904; theme values from `src/theme.rs`). Applied right after
  the re-render in the `on_success` closure (gloss: after
  `show_gloss_with_color`, `gloss.rs:1036`; journal: after
  `render_current` / `render_filtered_match`, `journal.rs:1908-1911`).
- **Clear on** (each removes the tag over the whole buffer, ala
  `overlay_search::gtk_ops::clear`):
  - **Escape** — new first-precedence step in each overlay's Escape handler
    (gloss keymap.rs ~1968; journal keymap.rs ~1681), gating before
    `close_*_to_reader`, parallel to how `clear_overlay_search` gates there.
  - **Navigating** to another gloss/entry (gloss `navigate_gloss`, journal
    block/band nav).
  - **Closing** the overlay.
  - **Another rewrite** (superseded by the new highlight).

## Pillar 3 — History navigation (Ctrl+Shift+n / Ctrl+Shift+p)

Same pair in **both** overlays; **view-only browsing**.

- `Ctrl+Shift+n` → newer, `Ctrl+Shift+p` → older (matching the existing n/p
  "next/prev" sense in these overlays).
- Re-renders the selected version **read-only**, each shown with its own
  diff-highlight versus its immediate predecessor. The live DB entry is **never
  mutated** by browsing.
- A small transient/footer cue indicates position in history (e.g. "rev 2/5")
  and the `prompt` that produced the next step, if present.

## Pillar 4 — Restore this version (Ctrl+Shift+r)

While browsing (Pillar 3), `Ctrl+Shift+r` promotes the **currently-viewed**
older version to be the live entry:

- Append the current head as a new revision first (restore is itself a change —
  nothing is lost), then write the viewed version back as the new head via the
  normal update path (`update_journal_page` / `update_gloss`), re-render, and
  exit browse mode.
- Toast "Restored".

## Keybind routing notes (for the plan)

- `Ctrl+Shift+n/p/r` are otherwise free in both overlays; only `Ctrl+Shift+L`
  exists globally (keymap.rs:103).
- The existing `"n" if is_ctrl` / `"p" if is_ctrl` arms (library picker
  keymap.rs:396; synopsis keymap.rs:2300) **do not check `is_shift`**, so the new
  Ctrl+Shift arms must be matched **first**, or those arms gain `&& !is_shift`.
- RPD emits shifted letters as the shifted glyph with `is_shift=true` — handle
  both `"n"`/`"N"` (etc.) forms, per
  `project_shift_comma_emits_less` / `feedback_rpd_keybind_prompts`.
- Update the **Ctrl+/ overlay legends** for gloss and journal
  (`src/ui/{gloss,journal}_keybinds_overlay.rs`) with the three new binds, per
  the house rule that every keybind change updates the overlay
  (`update-cairo-keybinds-overlay` / the overlay-specific legends).

## Testing

- **Unit (`cargo test`, no GUI):** the word-diff module (ranges on
  representative old/new pairs incl. verse markup); the revision-append and
  restore ordering logic; offset mapping through the gloss markup strip.
- **Headless cage e2e:** on-screen diff-highlight after a rewrite, Escape
  clears it, `Ctrl+Shift+n/p` browsing re-renders with per-step highlight,
  `Ctrl+Shift+r` restores. (MPV-isolated; see the project's headless harness.)

## Out of scope

- Per-word deletion markers (deletions leave no span; only insertions/changes in
  the NEW text are highlighted).
- Cross-entry / global revision browsing (history is per-entry).
- Pruning/retention limits (full history kept; revisit if growth becomes a
  concern).
