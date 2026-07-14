# Design — In-place vim edit for gloss & synopsis + monospace edit font

_Date: 2026-06-30 (US Central). Status: approved design, pre-plan._

## Problem / goal

Two related requests:

1. **Monospace edit font.** While editing a journal Q&A, a gloss, or a synopsis,
   the editor's font switches to a monospace family; on exit it switches back to
   the overlay's reading font (Charter).
2. **In-place vim editor for gloss & synopsis.** Today only the journal Q&A has
   an in-place modal vim editor (`InputMode::JournalEdit`). Gloss and synopsis
   "edit" through the transient ask-Claude `AskCard`. Make gloss and synopsis use
   the same in-place modal vim editor the journal uses, editing the **raw stored
   text**.

This reopens a decision previously deferred (see
`CLAUDE-activeContext.md` SCOPE NOTE and memory
`project_journal_vim_edit.md`): gloss/synopsis were kept on the ask-Claude flow
because their *rendered display* text can't reconstruct the stored markup. The
resolution here is that the editor edits the **raw stored markup/text**, not the
rendered display — which round-trips losslessly.

## Decisions (from brainstorming)

- **Edit target = raw stored text.** Gloss editor shows the raw
  `<speaker>/<verse>/<gloss>` markup (`gloss_text`); synopsis editor shows the
  plain `synopsis` text. Save writes the buffer straight back. No Claude.
- **Monospace family = `JetBrainsMono Nerd Font`** (installed; confirmed via
  `fc-list`). One `const EDIT_FONT_FAMILY` so it is changed in one place.
- **Replace `e` (edit-in-place) only; keep `r`/`R` (ask-Claude) as the separate
  `AskCard` flow.** `e` = hand-edit; `r`/`R` = AI create/rewrite.
- **Gloss edit view = plain raw markup in monospace** (no speaker/verse colors or
  per-line tints during edit); re-render the colored display on exit.
- **Synopsis edits in the synopsis overlay card** — which is the **same
  `GlossOverlay` widget** (see Architecture). Not the read-only `vocab_popup`
  sidebar.
- **Edit font size = the overlay's current reading size** (only the family
  swaps).

## Architecture

**Key fact that shapes the design:** the full-screen synopsis overlay is rendered
*through the gloss overlay widget* — `show_synopsis_overlay()` calls
`gloss_overlay.show_synopsis(...)` (`src/app/scene_synopsis.rs:371`). So the gloss
overlay and the full-screen synopsis overlay are the **same `GlossOverlay`
instance**, reused. (The synopsis *sidebar* — `vocab_popup`, via
`scene_synopsis::show_synopsis` / `update_synopsis` — is a separate read-only
popup and is **out of scope**.)

Therefore the work is **two editors, not three**:

1. **`GlossOverlay` in-place vim editor (NEW).** One editor on the existing
   `gloss_view: gtk4::TextView`, serving BOTH gloss-edit and synopsis-edit.
   Modeled on the journal editor: a `VimEngine` held in the overlay, GTK keys →
   `VimKey` via the shared `gtk_key_to_vim`, the buffer mirrors the engine
   (`editable=false`, we drive cursor/selection + a painted block cursor), a
   footer shows `-- NORMAL/INSERT/VISUAL --` or the `:` command line.
2. **`JournalOverlay` in-place vim editor (EXISTS).** Unchanged except it gains
   the font swap.

Unlike the journal editor (a two-part Q&A buffer), the gloss/synopsis editor is a
**single-text buffer** (one blob: the markup, or the synopsis text).

### New input mode

Add `InputMode::GlossEdit`, dispatched at the TOP of `handle_key` beside the
existing `InputMode::JournalEdit` early-dispatch (`src/input/keymap.rs:52`), so
Insert-mode space and printable keys reach the engine instead of being swallowed
by the global play-pause / navigation guards. Both gloss-edit and synopsis-edit
use `GlossEdit` (same widget); the **save path branches** on whether the overlay
is currently showing a gloss vs a synopsis.

### Distinguishing gloss vs synopsis at save time

`GlossOverlay` already tracks which content it is showing (it has a paginated-mode
enum and distinct `show_gloss_with_color` vs `show_synopsis` entry points). The
editor records, at enter time, which surface opened it (an
`edit_kind: Gloss | Synopsis` set by the caller, or read from the overlay's
existing mode state). The exit/save routes to the matching persistence fn.

## Components & data flow

### Shared edit lifecycle (both overlays)

**Enter (`e`):**
1. Capture the overlay's current reading `font_family` into a new
   `pre_edit_family: RefCell<Option<String>>`.
2. `set_font(EDIT_FONT_FAMILY, <current size>)` — only the typeface changes.
3. Load the **raw stored text** into the editable buffer as plain text; seed a
   `VimEngine` (NORMAL mode); show the block cursor + mode footer.
4. Set the overlay's edit input mode (`GlossEdit` / `JournalEdit`).

**Exit (`:w`, `:wq`, `:q`, `:q!`, double-Esc):**
1. On save (`:w`/`:wq`): persist the buffer via the per-surface save fn (below);
   snapshot pre-edit text for single-level `u` undo.
2. On cancel (`:q`/Esc): if the buffer is dirty (engine buffer ≠ seed) and not
   forced, warn (`:q!` forces); otherwise discard.
3. Restore the stashed reading font (`set_font(<saved family>, <size>)`); clear
   `pre_edit_family`. Idempotent.
4. Re-render the formatted/colored display; return to the overlay's normal input
   mode (`GlossOverlay` / `SynopsisOverlay` / `JournalOverlay`).

`:w` (save, stay) keeps the editor open and re-seeds the dirty baseline to the
just-saved buffer — same as the journal editor does today.

### Per-surface raw-text source & save

**Gloss** (editor showing a gloss):
- **Load:** `gloss_list[gloss_index].gloss_text` (raw markup).
- **Save:** reuse the body of the existing
  `update_and_render_gloss_in_place()` (`src/input/actions/gloss.rs:834`): it
  already does `update_gloss` → `delete_gloss_audio` + remove the mp3 dir → patch
  the in-memory `gloss_list` row → re-render `show_gloss_with_color`. Call it
  with the hand-edited buffer instead of Claude's output. **Saving a hand-edit
  drops that gloss's cached TTS** (text changed → cached audio is stale; next
  playback re-synthesizes) — confirmed acceptable.
- **Undo:** the existing `gloss_undo` snapshot + `u` confirm flow wraps
  `update_gloss`, so hand-edits get single-level undo for free.

**Synopsis** (same editor, showing a synopsis):
- **Load:** `synopsis_cache[(div1,div2)]` (plain text).
- **Save:** `restore_synopsis_text(conn, abbrev, div1, div2, new_text)`
  (`src/db/queries.rs:445`, exists) → update `synopsis_cache` → re-render via the
  synopsis card path (`show_synopsis` / `prose_synopsis_card`).
- **Undo:** existing `synopsis_undo` snapshot + `u`.

**Journal** (existing `JournalOverlay` editor): save path unchanged
(`update_journal_page` + `journal_undo`); already loads/saves raw Q&A. Only the
font swap is added.

### Font swap mechanism

Both overlays already apply fonts via
`apply_font_to_views(views, "<family> <size>", tag_name)` driven by a
`set_font(family, size)` method backed by `font_family`/`font_size` fields. Add to
each overlay (gloss + journal):

- `begin_edit_font()`: stash `font_family` into `pre_edit_family`, then
  `set_font(EDIT_FONT_FAMILY, <current size>)`.
- `end_edit_font()`: if `pre_edit_family` is `Some`, restore it via `set_font`
  and clear; else no-op (idempotent across redundant exit paths).

Save-and-restore the captured family rather than hardcoding "Charter" on exit, so
a non-default overlay font would not be silently overridden. Net effect today is
identical (overlays read in Charter).

If `JetBrainsMono Nerd Font` is ever absent, Pango falls back to a default
monospace — degraded, not broken.

### Keybinds, routing & legends

**Routing (`src/input/keymap.rs`):**
- New `handle_gloss_edit_key(...)`, a near-clone of `handle_journal_edit_key`
  (keymap.rs:759): GTK key → `VimKey`, `feed_edit_key`, match `EditorAction` →
  gloss/synopsis save/cancel. Double-Esc reuses `is_double_esc()`.
- `InputMode::GlossEdit` early-dispatched at the top of `handle_key` beside
  `JournalEdit`.

**`e` bind:**
- Gloss overlay (`handle_gloss_key`, keymap.rs:1151): repoint `e` from
  `show_edit_dialog` (ask-card) to a new `gloss::begin_edit` (in-place editor).
- Synopsis overlay (`handle_synopsis_overlay_key`, keymap.rs:1444): repoint `e`
  from `synopsis::show_edit_prompt` (ask-card) to a new `synopsis::begin_edit`
  entering the same `GlossOverlay` editor.

**`R` inside the editor:** the engine emits `EditorAction::OpenRewrite` on `R`.
Route it to the existing ask-Claude rewrite for gloss/synopsis (so AI rewrite is
reachable without leaving the edit surface), mirroring journal.

**`r`/`R` create/rewrite ask-Claude flow stays** (still `AskCard`). The dead
gloss/synopsis ask-card *edit* path (`GlossPromptMode::Edit`, `show_edit_dialog`;
`SynopsisPromptKind::Edit`, `show_edit_prompt`) is removed where `e` no longer
uses it — but only if nothing else references it (verify; `Edit` enum variants
may need to stay if matched elsewhere).

**Legends (hand-maintained mirrors — CLAUDE.md requires updating in the same
change):**
1. `src/ui/gloss_keybinds_overlay.rs` `GROUPS` — `e` description → "edit in place
   (vim)"; add the vim-edit keys note (`:w`/`:q`/`R`/Esc).
2. `src/ui/synopsis_keybinds_overlay.rs` `GROUPS` — same for synopsis `e`.
3. Reader-card `Ctrl+/` overlay (`src/ui/keybinds_overlay.rs`) — no `e` there;
   verify via the `update-cairo-keybinds-overlay` skill cross-reference, likely
   no change.

Journal's legend already documents its vim editor — unchanged.

## Error handling / edge cases

- **DB open failure on save:** mirror the existing handlers — best-effort
  (`if let Ok(conn) = open_db_rw()`), toast on the no-op; do not crash.
- **Empty buffer save:** allow (a gloss/synopsis can legitimately be cleared);
  the existing flows already tolerate it. Trim trailing whitespace as journal
  does.
- **Dirty cancel:** warn-then-`:q!` like journal.
- **Entering edit with no current gloss/synopsis:** no-op + toast (the `e`
  handlers already guard "no current item").
- **Overlay closed while editing:** edit modes are early-dispatched, so the
  normal close binds are not reachable until the editor exits; no special-case
  needed (matches journal).

## Testing

- **Pure engine:** already covered by the 41 `src/input/vim/` unit tests
  (`semicolon_enters_command_mode`, etc.). The new editor reuses the same engine,
  so no engine changes are expected; if any helper is added, add a unit test.
- **Round-trip (new pure tests where possible):** a gloss `gloss_text` string and
  a synopsis string fed through the editor's load→buffer→save transform return
  byte-identical text (no markup mangling). This is logic-testable without GTK if
  the load/save transform is a free function.
- **Headless / on-screen (user-run):** font swap and the rendered colored→raw→
  colored transition are pixel/geometry acceptance criteria → the user runs
  `./scripts/e2e-env.sh` and/or a manual `cage` launch:
  - `e` on a gloss shows raw markup in monospace; `:wq` re-renders colored
    display in Charter.
  - `e` on a synopsis shows plain text in monospace; `:wq` re-renders in Charter.
  - `e` on a journal Q&A now shows monospace; `:wq`/`:q` restores Charter.
  - double-Esc and `:q!` exit and restore the reading font.
- `cargo test --bins` and `cargo clippy` stay green.

## Out of scope

- The synopsis sidebar popup (`vocab_popup`).
- Any change to the ask-Claude create/rewrite UX (`r`/`R`).
- Syntax-highlighting the raw markup during edit (explicitly chosen: plain mono).
- Font cycling of overlays / changing the reading font.

## Files (anticipated)

- `src/ui/gloss_overlay.rs` — new in-place editor (enter/exit/mirror/feed/
  block-cursor/footer), `begin_edit_font`/`end_edit_font`, `pre_edit_family`,
  `EDIT_FONT_FAMILY`.
- `src/ui/journal_overlay.rs` — `begin_edit_font`/`end_edit_font` +
  `pre_edit_family`; call them in `enter_edit_buffer`/`exit_edit_buffer`.
- `src/input/keymap.rs` — `InputMode::GlossEdit` early-dispatch,
  `handle_gloss_edit_key`, repoint gloss/synopsis `e`.
- `src/input/actions/gloss.rs` — `begin_edit`, vim save/cancel/rewrite for gloss;
  reuse `update_and_render_gloss_in_place`.
- `src/input/actions/synopsis.rs` — `begin_edit`, vim save/cancel/rewrite for
  synopsis; reuse `restore_synopsis_text` + re-render.
- `src/app/mod.rs` — `InputMode::GlossEdit` variant.
- `src/ui/gloss_keybinds_overlay.rs`, `src/ui/synopsis_keybinds_overlay.rs` —
  legend `GROUPS`.
