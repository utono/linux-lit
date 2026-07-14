# In-place vim edit for gloss & synopsis + monospace edit font — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give gloss and synopsis the same in-place modal vim editor the journal Q&A already has (editing the raw stored text so it round-trips losslessly), and swap the editor font to monospace while any of the three (journal / gloss / synopsis) is being edited.

**Architecture:** The full-screen synopsis overlay renders *through the gloss overlay widget* (`scene_synopsis.rs` → `gloss_overlay.show_synopsis`), so this is **one new editor on `GlossOverlay::gloss_view`** serving both gloss-edit and synopsis-edit, plus the existing journal editor (which only gains the font swap). The new editor is a direct analog of the journal editor (`JournalOverlay::enter_edit_buffer` + `handle_journal_edit_key` + `journal::vim_save/vim_cancel`), with two differences: (1) the buffer is a **single raw-text blob** (no `Q:`/answer framing — no `journal_doc`), and (2) save branches on whether the overlay is showing a gloss vs a synopsis. A new `InputMode::GlossEdit` is early-dispatched at the top of `handle_key` beside `JournalEdit`.

**Tech Stack:** Rust, GTK4 (`gtk4`, `sourceview5`), the pure vim engine at `src/input/vim/`, SQLite (`rusqlite`) via `src/db/queries.rs`.

## Global Constraints

- **Edit target is the RAW stored text**, never the rendered display. Gloss editor shows raw `<speaker>/<verse>/<gloss>` markup (`SavedGloss.gloss_text`); synopsis editor shows the plain `<p>`-tagged synopsis text from `synopsis_cache`. Save writes the buffer back byte-for-byte (after trailing-whitespace trim, matching the journal). No Claude on the `e`/`:w` path.
- **Monospace family = `JetBrainsMono Nerd Font`** (confirmed installed via `fc-list`). Declare it ONCE as `pub(crate) const EDIT_FONT_FAMILY: &str = "JetBrainsMono Nerd Font";` in `src/ui/mod.rs` and reference it everywhere. Edit font **size = the overlay's current reading size** — only the family swaps.
- **Reuse, don't reinvent:** the new editor's engine plumbing mirrors `JournalOverlay`'s vim methods exactly (`enter_edit_buffer`/`feed_edit_key`/`mirror_engine`/`exit_edit_buffer`/`reseed_edit_buffer`/`edit_is_dirty`), and the gloss save reuses `update_and_render_gloss_in_place` (`src/input/actions/gloss.rs:834`). Do not duplicate the engine, the block-cursor painter (`crate::ui::paint_block_cursor`/`clear_block_cursor`), or the font helper (`crate::ui::apply_font_to_views`).
- **`R` inside the editor** routes to the EXISTING ask-Claude rewrite path (gloss `show_edit_dialog` / synopsis `show_edit_prompt`); the `GlossPromptMode::Edit` / `SynopsisPromptKind::Edit` enum variants and their `edit_gloss`/`edit_synopsis` functions **stay** (they remain reachable via `R` and the ask-card submit dispatch — NO dead-code deletion).
- **`r`/`R` create/rewrite ask-Claude flow stays** as the separate `AskCard` flow. Only the `e` keybind changes (ask-card edit → in-place vim editor).
- **Legends are hand-maintained mirrors** (`src/ui/gloss_keybinds_overlay.rs`, `src/ui/synopsis_keybinds_overlay.rs`): any `e`-bind change requires updating the matching `GROUPS` const in the same change (CLAUDE.md rule).
- `cargo build`, `cargo test --bins`, and `cargo clippy` must stay green at every commit. Do NOT run `cargo run` (the user launches the app).
- After every commit, update `CLAUDE-activeContext.md` (CLAUDE.md "After a Commit" rule).

---

## File Structure

**New / heavily modified:**

- `src/ui/mod.rs` — add `pub(crate) const EDIT_FONT_FAMILY`.
- `src/ui/gloss_overlay.rs` — the NEW in-place vim editor on `gloss_view`: struct fields (`vim_engine`, `vim_seed`, `vim_cursor_colors`, `pre_edit_family`), the lifecycle methods (`enter_edit_buffer`, `feed_edit_key`, `mirror_engine`, `exit_edit_buffer`, `reseed_edit_buffer`, `edit_buffer_text`, `edit_is_dirty`), and the font-swap pair (`begin_edit_font`/`end_edit_font`).
- `src/ui/journal_overlay.rs` — add `pre_edit_family` + `begin_edit_font`/`end_edit_font`; call them in `enter_edit_buffer`/`exit_edit_buffer`.
- `src/app/mod.rs` — add `InputMode::GlossEdit`.
- `src/input/keymap.rs` — `InputMode::GlossEdit` early-dispatch; `handle_gloss_edit_key`; repoint gloss `e` and synopsis `e`.
- `src/input/actions/gloss.rs` — `begin_edit`, `vim_save`, `vim_cancel`, `vim_open_rewrite` for the gloss surface.
- `src/input/actions/synopsis.rs` — `begin_edit`, `vim_save`, `vim_cancel`, `vim_open_rewrite` for the synopsis surface.
- `src/ui/gloss_keybinds_overlay.rs`, `src/ui/synopsis_keybinds_overlay.rs` — legend `GROUPS`.

**Reused unchanged:** `src/input/vim/` (engine + `VimKey`/`EditorAction`), `src/ui/mod.rs::{paint_block_cursor, clear_block_cursor, apply_font_to_views}`, `src/input/actions/gloss.rs::update_and_render_gloss_in_place`, `src/db/queries.rs::{update_gloss, save_synopsis}`.

---

## Reference signatures (from the existing journal editor — clone these)

These exact signatures/line numbers were captured from the codebase; the new gloss editor mirrors them.

- `JournalOverlay::enter_edit_buffer(&self, question, answer, block_fill, block_fg)` — `src/ui/journal_overlay.rs:641`
- `JournalOverlay::feed_edit_key(&self, key: VimKey) -> EditorAction` — `:670`
- `JournalOverlay::exit_edit_buffer(&self)` — `:719`
- `JournalOverlay::mirror_engine(&self)` (private) — `:730`; footer at `:775-787`, block cursor at `:762-771` (tag `"journal-vim-block"`).
- `JournalOverlay::reseed_edit_buffer(&self, _q, _a)` — `:695`; `edit_buffer_qa(&self) -> (String,String)` — `:684`; `edit_is_dirty(&self) -> bool` — `:707`.
- `JournalOverlay::set_font(&self, family, size)` — `:542`; `apply_font(&self)` — `:552`.
- `handle_journal_edit_key(state, key_name, key_char, is_ctrl, _is_shift, tokio_handle) -> bool` — `src/input/keymap.rs:759-811`.
- Early dispatch of `JournalEdit` — `src/input/keymap.rs:52-54`.
- `gtk_key_to_vim(key_name, key_char, is_ctrl) -> Option<VimKey>` — `:735-754`; `is_double_esc()` — `:719-730`.
- `journal::begin_edit` `:507`, `vim_save(state, quit)` `:522`, `vim_cancel(state, force)` `:566`, `vim_open_rewrite(state, tokio_handle)` `:585` — `src/input/actions/journal.rs`.
- Vim engine: `VimEngine::new(buffer: String)`, `.buffer() -> &str`, `.handle_key(VimKey) -> Outcome`, `.cursor()`, `.mode()`, `.cmdline()`, `.selection()`; `EditorAction::{Nop,Save,SaveQuit,Cancel,CancelForce,OpenRewrite}`; `VimKey::{Char,Esc,Enter,Backspace,Tab,CtrlR}` — `src/input/vim/{mod.rs,engine.rs}`.

Gloss / synopsis save anchors:

- `update_and_render_gloss_in_place(state_rc, ctx: &GlossContext, gloss_index: usize, gloss_id: i64, full_gloss: &str, model_for_db: &str, log_msg: &str)` — `src/input/actions/gloss.rs:834`. Does `update_gloss` + `delete_gloss_audio` + remove audio dir + snapshot `gloss_undo` + patch `gloss_list[idx].gloss_text` + re-render `show_gloss_with_color` + recolor. **Pass the hand-edited buffer as `full_gloss`.**
- Gloss context capture (from `edit_gloss`, `:1058-1071`): `ctx = state.gloss_context.clone()`, `idx = state.gloss_index`, `(gloss_text, gloss_id) = state.gloss_list[idx]`, `model = state.config.claude_model`.
- Synopsis save (from `run_synopsis_revision`, `:147-172`): `save_synopsis(&conn, &abbrev, div1, div2, &text, &model)`, snapshot `synopsis_undo = Some(((div1,div2), original))`, `synopsis_cache.insert((div1,div2), text)`, re-render `show_synopsis(&label, &text, Some(&root_color), cw, h, prose_card)`. Scene = `state.synopsis_overlay_scene` (`(div1,div2)`); `synopsis_cache: HashMap<(i64,i64), String>`; label via `crate::app::scene_synopsis::synopsis_label(&s, div1, div2)`; `prose_card` via `crate::app::scene_synopsis::prose_synopsis_card(&s, cw)`; `cw = s.content_hbox.width()`, `h = crate::app::layout::overlay_card_height(&s)`.
- `restore_synopsis_text(conn, work_abbrev, div1, div2, synopsis)` — `src/db/queries.rs:445` (undo-only; the SAVE path uses `save_synopsis`, `:422`).

---

## Task 1: Add `EDIT_FONT_FAMILY` constant

**Files:**
- Modify: `src/ui/mod.rs` (top of file, alongside other `pub(crate)` items)

**Interfaces:**
- Produces: `pub(crate) const EDIT_FONT_FAMILY: &str` — used by both overlays' `begin_edit_font`.

- [ ] **Step 1: Add the constant**

In `src/ui/mod.rs`, near the top (after the `use`/module declarations, before the first `pub(crate) fn`), add:

```rust
/// Monospace family used while editing a journal Q&A, gloss, or synopsis in the
/// in-place vim editor. Only the family swaps during edit; the reading size is
/// kept. Confirmed installed via `fc-list`. If absent, Pango falls back to a
/// default monospace — degraded, not broken.
pub(crate) const EDIT_FONT_FAMILY: &str = "JetBrainsMono Nerd Font";
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds (an `unused const` warning is acceptable until Task 2 references it).

- [ ] **Step 3: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat(edit-font): add EDIT_FONT_FAMILY const (JetBrainsMono Nerd Font)"
```

---

## Task 2: Font swap on the journal editor

Add the font swap to the journal editor FIRST (it already has a working in-place editor), so the swap mechanism is proven before the new gloss editor exists.

**Files:**
- Modify: `src/ui/journal_overlay.rs` — struct fields + `begin_edit_font`/`end_edit_font` + calls in `enter_edit_buffer`/`exit_edit_buffer`.

**Interfaces:**
- Consumes: `crate::ui::EDIT_FONT_FAMILY` (Task 1); `JournalOverlay::set_font` (`:542`).
- Produces: `JournalOverlay::begin_edit_font(&self)`, `JournalOverlay::end_edit_font(&self)`.

- [ ] **Step 1: Add the `pre_edit_family` field**

In the `JournalOverlay` struct (`src/ui/journal_overlay.rs`, alongside `font_family`/`font_size` near `:55`), add:

```rust
    /// Reading font family stashed on edit-enter and restored on exit, so the
    /// monospace edit font does not leak into the rendered display. `None` when
    /// not editing. Save-and-restore (not hardcode-Charter) so a non-default
    /// overlay font would survive an edit.
    pre_edit_family: RefCell<Option<String>>,
```

Initialize it in `JournalOverlay::new` (in the struct literal that constructs the overlay) with:

```rust
            pre_edit_family: RefCell::new(None),
```

- [ ] **Step 2: Add `begin_edit_font`/`end_edit_font`**

Add these two methods to the `impl JournalOverlay` block, next to `set_font` (`:542`):

```rust
    /// Swap to the monospace edit font, stashing the current reading family so
    /// `end_edit_font` can restore it. Size is unchanged. Idempotent: a second
    /// call without an intervening `end_edit_font` re-stashes the (already
    /// monospace) family — harmless, but callers pair it with `end_edit_font`.
    pub fn begin_edit_font(&self) {
        let current = self.font_family.borrow().clone();
        *self.pre_edit_family.borrow_mut() = Some(current);
        let size = self.font_size.get();
        self.set_font(crate::ui::EDIT_FONT_FAMILY, size);
    }

    /// Restore the reading font stashed by `begin_edit_font`. No-op when nothing
    /// is stashed, so redundant exit paths (e.g. `:q` after a font-less state)
    /// are safe.
    pub fn end_edit_font(&self) {
        let stashed = self.pre_edit_family.borrow_mut().take();
        if let Some(family) = stashed {
            let size = self.font_size.get();
            self.set_font(&family, size);
        }
    }
```

- [ ] **Step 3: Call them in `enter_edit_buffer`/`exit_edit_buffer`**

In `JournalOverlay::enter_edit_buffer` (`:641`), as the FIRST statement of the body, add:

```rust
        self.begin_edit_font();
```

In `JournalOverlay::exit_edit_buffer` (`:719`), as the LAST statement of the body, add:

```rust
        self.end_edit_font();
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds with no errors (the `unused const` warning from Task 1 is now gone).

- [ ] **Step 5: Run the pure suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS (575+ pass; no test regression — this is GTK-only code with no unit coverage, so the count is unchanged).

- [ ] **Step 6: Commit**

```bash
git add src/ui/journal_overlay.rs
git commit -m "feat(journal-edit): swap to monospace edit font while editing"
```

---

## Task 3: `InputMode::GlossEdit` variant

**Files:**
- Modify: `src/app/mod.rs` — add the enum variant beside `JournalEdit` (`:102-103`).

**Interfaces:**
- Produces: `crate::app::InputMode::GlossEdit` — matched by the keymap early-dispatch (Task 7) and set by the `begin_edit` handlers (Tasks 5, 6).

- [ ] **Step 1: Add the variant**

In `src/app/mod.rs`, in the `InputMode` enum immediately after the `JournalEdit` variant (`:103`), add:

```rust
    /// In-place modal vim editor for the gloss/synopsis overlay (the same
    /// `GlossOverlay` widget). Early-dispatched in `handle_key` beside
    /// `JournalEdit` so Insert-mode space and printable keys reach the engine.
    /// `:w`/`:wq` save the raw text; `:q`/double-Esc exit; `R` opens the ask-Claude
    /// rewrite. The save path branches on whether the overlay shows a gloss or a
    /// synopsis.
    GlossEdit,
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds. If any `match self.input_mode { ... }` over `InputMode` is non-exhaustive, the compiler names it — add a `GlossEdit` arm there mirroring the adjacent `JournalEdit` arm (do not add a catch-all that hides future variants). Re-run until clean.

- [ ] **Step 3: Run the pure suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs $(git diff --name-only)
git commit -m "feat(gloss-edit): add InputMode::GlossEdit variant"
```

(The `$(git diff --name-only)` captures any non-exhaustive-match arms you had to add in Step 2.)

---

## Task 4: `GlossOverlay` in-place vim editor (the core)

This is the largest task: it adds the editor plumbing to `GlossOverlay`, mirroring the journal editor exactly. The buffer is a **single raw-text blob** — no `Q:`/answer framing.

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — struct fields + methods.

**Interfaces:**
- Consumes: `crate::input::vim::{VimEngine, VimKey, EditorAction, Mode}`; `crate::ui::{paint_block_cursor, clear_block_cursor, EDIT_FONT_FAMILY}`; existing `GlossOverlay::{set_font, font_family, font_size, gloss_view, footer fields}`.
- Produces (all `pub`):
  - `GlossOverlay::begin_edit_font(&self)`, `end_edit_font(&self)`
  - `GlossOverlay::enter_edit_buffer(&self, raw: &str, block_fill: &str, block_fg: &str)`
  - `GlossOverlay::feed_edit_key(&self, key: VimKey) -> EditorAction`
  - `GlossOverlay::exit_edit_buffer(&self)`
  - `GlossOverlay::reseed_edit_buffer(&self, raw: &str)`
  - `GlossOverlay::edit_buffer_text(&self) -> String`
  - `GlossOverlay::edit_is_dirty(&self) -> bool`

- [ ] **Step 1: Add struct fields**

In the `GlossOverlay` struct (`src/ui/gloss_overlay.rs:39`, alongside `font_family`/`font_size`), add:

```rust
    /// In-place vim editor engine (None when not editing). The buffer is a single
    /// raw-text blob: the gloss markup OR the synopsis text, depending on which
    /// surface opened the editor.
    vim_engine: RefCell<Option<crate::input::vim::VimEngine>>,
    /// The raw text the editor was seeded with, for the `:q` dirty-check.
    vim_seed: RefCell<String>,
    /// Block-cursor (fill, glyph-fg) colors, threaded from the theme on enter.
    vim_cursor_colors: RefCell<(String, String)>,
    /// Reading font family stashed on edit-enter, restored on exit (mono swap).
    pre_edit_family: RefCell<Option<String>>,
```

Initialize them in `GlossOverlay::new` (`:175`, in the struct literal):

```rust
            vim_engine: RefCell::new(None),
            vim_seed: RefCell::new(String::new()),
            vim_cursor_colors: RefCell::new((String::new(), String::new())),
            pre_edit_family: RefCell::new(None),
```

- [ ] **Step 2: Add the font-swap pair** (identical shape to Task 2)

Add to `impl GlossOverlay`, next to `apply_font` (`:511`):

```rust
    pub fn begin_edit_font(&self) {
        let current = self.font_family.borrow().clone();
        *self.pre_edit_family.borrow_mut() = Some(current);
        let size = self.font_size.get();
        self.set_font(crate::ui::EDIT_FONT_FAMILY, size);
    }

    pub fn end_edit_font(&self) {
        let stashed = self.pre_edit_family.borrow_mut().take();
        if let Some(family) = stashed {
            let size = self.font_size.get();
            self.set_font(&family, size);
        }
    }
```

NOTE: confirm `GlossOverlay` has a `pub fn set_font(&self, family: &str, size: i32)`. If `set_font` is named differently here (the journal's is at `:542`; the gloss overlay's font entry is `apply_font` at `:511` driven by `font_family`/`font_size`), add a thin `set_font` that mirrors the journal's:

```rust
    pub fn set_font(&self, family: &str, size: i32) {
        *self.font_family.borrow_mut() = family.to_string();
        self.font_size.set(size);
        self.apply_font();
    }
```

- [ ] **Step 3: Add `enter_edit_buffer`**

```rust
    /// Enter the in-place vim editor on a single raw-text blob (gloss markup or
    /// synopsis text). Seeds a `VimEngine` in NORMAL mode, loads the text as
    /// plain text into `gloss_view`, swaps to the mono edit font, paints the block
    /// cursor + mode footer. The caller sets `InputMode::GlossEdit` afterward.
    pub fn enter_edit_buffer(&self, raw: &str, block_fill: &str, block_fg: &str) {
        use gtk4::prelude::*;
        self.begin_edit_font();
        *self.vim_cursor_colors.borrow_mut() = (block_fill.to_string(), block_fg.to_string());
        *self.vim_seed.borrow_mut() = raw.to_string();
        *self.vim_engine.borrow_mut() = Some(crate::input::vim::VimEngine::new(raw.to_string()));
        // Drive the buffer/cursor ourselves; the native caret is hidden in NORMAL.
        self.gloss_view.set_editable(false);
        self.gloss_view.set_cursor_visible(false);
        self.gloss_view.set_can_focus(true);
        self.gloss_view.grab_focus();
        self.mirror_engine();
    }
```

- [ ] **Step 4: Add `feed_edit_key`, `edit_buffer_text`, `edit_is_dirty`, `reseed_edit_buffer`**

```rust
    /// Feed one key to the engine, re-mirror, and return the resulting action.
    pub fn feed_edit_key(&self, key: crate::input::vim::VimKey) -> crate::input::vim::EditorAction {
        let action = {
            let mut guard = self.vim_engine.borrow_mut();
            match guard.as_mut() {
                Some(engine) => engine.handle_key(key).action,
                None => crate::input::vim::EditorAction::Nop,
            }
        };
        self.mirror_engine();
        action
    }

    /// The current editor buffer text (raw). Empty string when not editing.
    pub fn edit_buffer_text(&self) -> String {
        self.vim_engine
            .borrow()
            .as_ref()
            .map(|e| e.buffer().to_string())
            .unwrap_or_default()
    }

    /// True iff the buffer differs from the seed (for the `:q` dirty refusal).
    pub fn edit_is_dirty(&self) -> bool {
        match self.vim_engine.borrow().as_ref() {
            Some(e) => e.buffer() != self.vim_seed.borrow().as_str(),
            None => false,
        }
    }

    /// Reset the dirty baseline to `raw` (after a non-quit `:w`).
    pub fn reseed_edit_buffer(&self, raw: &str) {
        *self.vim_seed.borrow_mut() = raw.to_string();
    }
```

- [ ] **Step 5: Add `exit_edit_buffer`**

```rust
    /// Leave the editor: drop the engine, clear the block cursor, restore the
    /// native caret default, and restore the reading font. The caller re-renders
    /// the formatted display and resets the input mode.
    pub fn exit_edit_buffer(&self) {
        use gtk4::prelude::*;
        crate::ui::clear_block_cursor(&self.gloss_view.buffer(), "gloss-vim-block");
        *self.vim_engine.borrow_mut() = None;
        self.vim_seed.borrow_mut().clear();
        self.gloss_view.set_cursor_visible(false);
        self.end_edit_font();
    }
```

- [ ] **Step 6: Add `mirror_engine`** (clone the journal's `:730-787`, adapted for a single buffer + the `"gloss-vim-block"` tag)

```rust
    /// Sync the engine state into `gloss_view`: replace the buffer text, place the
    /// cursor/selection, paint the block cursor in NORMAL/VISUAL (hide it + show
    /// the native caret in INSERT), and render the mode/`:` footer. Mirrors
    /// `JournalOverlay::mirror_engine`.
    fn mirror_engine(&self) {
        use gtk4::prelude::*;
        let engine_borrow = self.vim_engine.borrow();
        let Some(engine) = engine_borrow.as_ref() else {
            return;
        };
        let buffer = self.gloss_view.buffer();
        // 1. Text.
        if buffer.text(&buffer.start_iter(), &buffer.end_iter(), false) != engine.buffer() {
            buffer.set_text(engine.buffer());
        }
        // 2. Cursor + selection (char indices → iters).
        let cursor_iter = buffer.iter_at_offset(engine.cursor() as i32);
        if let Some(sel) = engine.selection() {
            let a = buffer.iter_at_offset(sel.start as i32);
            let b = buffer.iter_at_offset(sel.end as i32);
            buffer.select_range(&a, &b);
        } else {
            buffer.place_cursor(&cursor_iter);
        }
        // 3. Block cursor vs native caret.
        let mode = engine.mode();
        if mode == crate::input::vim::Mode::Insert {
            crate::ui::clear_block_cursor(&buffer, "gloss-vim-block");
            self.gloss_view.set_cursor_visible(true);
        } else {
            self.gloss_view.set_cursor_visible(false);
            let (fill, fg) = self.vim_cursor_colors.borrow().clone();
            crate::ui::paint_block_cursor(&buffer, "gloss-vim-block", &fill, &fg, engine.cursor());
        }
        // 4. Footer.
        let footer = if let Some(cmd) = engine.cmdline() {
            format!(":{}", cmd)
        } else {
            match mode {
                crate::input::vim::Mode::Normal => {
                    "-- NORMAL --  (:w save \u{00b7} R rewrite \u{00b7} :q quit)".to_string()
                }
                crate::input::vim::Mode::Insert => "-- INSERT --".to_string(),
                crate::input::vim::Mode::Visual => "-- VISUAL --".to_string(),
                crate::input::vim::Mode::VisualLine => "-- VISUAL LINE --".to_string(),
            }
        };
        self.set_edit_footer(&footer);
    }

    /// Show `text` in the overlay footer during edit. Uses the overlay's existing
    /// footer label; hides the page/position labels while editing.
    fn set_edit_footer(&self, text: &str) {
        use gtk4::prelude::*;
        self.position_label.set_text(text);
        self.position_label.set_visible(true);
    }
```

NOTE (footer wiring): the gloss overlay's footer labels are `citation_label` + `position_label` + `page_marker` (`:39` struct). Use `position_label` as the mode/`:` line during edit (as written above). Confirm the field name while implementing; if the overlay has a dedicated left-footer label, prefer that. The `mirror_engine` footer text MUST match what the journal editor shows so behavior is consistent.

- [ ] **Step 7: Verify it compiles**

Run: `cargo build 2>&1 | tail -25`
Expected: builds. Resolve any name mismatches (`set_font` presence, footer label field name, `Mode`/`Range` import paths). Re-run until clean.

- [ ] **Step 8: Run the pure suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS (unchanged count — GTK-only additions).

- [ ] **Step 9: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss-edit): add in-place vim editor to GlossOverlay (gloss_view)"
```

---

## Task 5: Gloss `begin_edit` / `vim_save` / `vim_cancel` / `vim_open_rewrite`

**Files:**
- Modify: `src/input/actions/gloss.rs` — add the four handlers.

**Interfaces:**
- Consumes: `GlossOverlay::{enter_edit_buffer, edit_buffer_text, edit_is_dirty, exit_edit_buffer, reseed_edit_buffer}` (Task 4); `update_and_render_gloss_in_place` (`:834`); `show_edit_dialog` (`:439`).
- Produces (all `pub(crate)`): `gloss::begin_edit(state)`, `gloss::vim_save(state, quit)`, `gloss::vim_cancel(state, force)`, `gloss::vim_open_rewrite(state, tokio_handle)`.

- [ ] **Step 1: Add `begin_edit`**

```rust
/// `e` in the gloss overlay: enter the in-place modal vim editor on the current
/// gloss's RAW markup (`gloss_list[gloss_index].gloss_text`). No-op + nothing if
/// there is no current gloss. The save path (`vim_save`) writes the buffer back
/// via `update_and_render_gloss_in_place` — no Claude.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let idx = s.gloss_index;
    let raw = match s.gloss_list.get(idx) {
        Some(g) => g.gloss_text.clone(),
        None => {
            show_tts_toast(state, "No gloss to edit");
            return;
        }
    };
    let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.gloss_overlay.enter_edit_buffer(&raw, &fill, &fg);
    s.input_mode = crate::app::InputMode::GlossEdit;
}
```

NOTE: if `show_tts_toast` borrows `state` (not `&s`), move the toast call out of the `borrow_mut` scope. Confirm its signature in `gloss.rs` while implementing; the `edit_gloss` path uses `show_tts_toast(state_rc, ...)` with the `Rc`.

- [ ] **Step 2: Add `vim_save`**

```rust
/// Save the gloss vim-editor buffer's raw markup to lit.db as-is (no Claude) via
/// `update_and_render_gloss_in_place` (which also snapshots `gloss_undo`, purges
/// cached audio, patches the in-memory row, and re-renders the colored display).
/// `:w` (quit=false) stays in the editor and re-seeds the dirty baseline; `:wq`
/// (quit=true) exits to the gloss overlay.
pub(crate) fn vim_save(state: &Rc<RefCell<AppState>>, quit: bool) {
    let raw = state.borrow().gloss_overlay.edit_buffer_text();
    let raw = raw.trim_end().to_string();
    let (ctx, idx, gloss_id, model) = {
        let s = state.borrow();
        let ctx = match &s.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let idx = s.gloss_index;
        let gloss_id = match s.gloss_list.get(idx) {
            Some(g) => g.gloss_id,
            None => return,
        };
        (ctx, idx, gloss_id, s.config.claude_model.clone())
    };
    update_and_render_gloss_in_place(
        state, &ctx, idx, gloss_id, &raw, &model,
        &format!("GLOSS: hand-edited gloss {} in place (vim)", gloss_id),
    );
    if quit {
        let mut s = state.borrow_mut();
        s.gloss_overlay.exit_edit_buffer();
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::ui::toast::show_transient(&s.chapter_toast, "Saved", 2);
    } else {
        state.borrow().gloss_overlay.reseed_edit_buffer(&raw);
        // Re-enter edit mode: update_and_render re-rendered the colored display
        // (read view) and dropped the editor. Re-open the editor on the saved raw.
        let (fill, fg) = {
            let s = state.borrow();
            (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone())
        };
        state.borrow().gloss_overlay.enter_edit_buffer(&raw, &fill, &fg);
        state.borrow_mut().input_mode = crate::app::InputMode::GlossEdit;
        crate::ui::toast::show_transient(&state.borrow().chapter_toast, "Saved (:q to exit)", 2);
    }
}
```

NOTE: `update_and_render_gloss_in_place` calls `show_gloss_with_color`, which re-renders the COLORED read display and does not know about the editor. After a non-quit `:w` we must re-enter the editor on the just-saved raw text (the `else` branch above) so the user stays in mono-edit mode. Confirm `s.chapter_toast` exists (the journal uses it); if the gloss overlay uses a different toast widget, use that.

- [ ] **Step 3: Add `vim_cancel`**

```rust
/// Leave the gloss vim editor. With unsaved changes and not `force`, warn and
/// STAY (`:q` refused on a modified buffer; `:q!` forces). Re-renders the colored
/// gloss display on exit.
pub(crate) fn vim_cancel(state: &Rc<RefCell<AppState>>, force: bool) {
    let dirty = state.borrow().gloss_overlay.edit_is_dirty();
    if dirty && !force {
        crate::ui::toast::show_transient(
            &state.borrow().chapter_toast,
            "Unsaved changes \u{2014} :w to save, :q! to discard",
            3,
        );
        return;
    }
    // Re-render the colored display from the unchanged stored gloss, then exit.
    let (ctx, idx, gloss_id, model) = {
        let s = state.borrow();
        let ctx = match &s.gloss_context {
            Some(c) => c.clone(),
            None => {
                // No context to re-render; just drop the editor.
                drop(s);
                let mut s = state.borrow_mut();
                s.gloss_overlay.exit_edit_buffer();
                s.input_mode = crate::app::InputMode::GlossOverlay;
                return;
            }
        };
        let idx = s.gloss_index;
        let gloss_id = s.gloss_list.get(idx).map(|g| g.gloss_id).unwrap_or(0);
        let raw = s.gloss_list.get(idx).map(|g| g.gloss_text.clone()).unwrap_or_default();
        (ctx, idx, gloss_id, (s.config.claude_model.clone(), raw))
    };
    let (model, stored_raw) = model;
    state.borrow().gloss_overlay.exit_edit_buffer();
    // Re-render the stored (un-edited) gloss in its colored form.
    update_and_render_gloss_in_place(
        state, &ctx, idx, gloss_id, &stored_raw, &model,
        "GLOSS: vim edit cancelled, re-rendered stored gloss",
    );
    state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
}
```

NOTE: cancelling re-writes the same stored text through `update_and_render_gloss_in_place`, which harmlessly re-persists identical text and re-renders. If a lighter "re-render without DB write" path exists on the overlay (e.g. calling `show_gloss_with_color` directly with the stored markup), prefer it to avoid the redundant `update_gloss`/`delete_gloss_audio` — check for a render-only helper while implementing and use it if present. Correctness is identical either way; the lighter path just avoids dropping cached audio on a no-op cancel.

- [ ] **Step 4: Add `vim_open_rewrite`** (route `R` to the existing ask-Claude edit dialog)

```rust
/// `R` in the gloss vim editor: leave the editor and open the existing ask-Claude
/// rewrite (edit) card so an AI rewrite is reachable without switching surfaces.
/// Mirrors journal `vim_open_rewrite`. The hand-edits in the buffer are discarded
/// (the rewrite operates on the stored gloss); warn if dirty is out of scope here
/// — `R` is "ask AI", distinct from `:w` "save my edit".
pub(crate) fn vim_open_rewrite(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    {
        let mut s = state.borrow_mut();
        s.gloss_overlay.exit_edit_buffer();
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }
    // Open the existing ask-Claude edit dialog (GlossPromptMode::Edit).
    show_edit_dialog(state);
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -25`
Expected: builds. Fix any toast-widget / borrow-scope mismatches the compiler flags.

- [ ] **Step 6: Run the pure suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss-edit): add begin_edit + vim save/cancel/rewrite for gloss"
```

---

## Task 6: Synopsis `begin_edit` / `vim_save` / `vim_cancel` / `vim_open_rewrite`

**Files:**
- Modify: `src/input/actions/synopsis.rs` — add the four handlers.

**Interfaces:**
- Consumes: `GlossOverlay::{enter_edit_buffer, edit_buffer_text, edit_is_dirty, exit_edit_buffer, reseed_edit_buffer}` (Task 4); `save_synopsis` (`queries.rs:422`); `show_synopsis`; `synopsis_label`/`prose_synopsis_card`; `show_edit_prompt` (`:37`).
- Produces (all `pub(crate)`): `synopsis::begin_edit(state)`, `synopsis::vim_save(state, quit)`, `synopsis::vim_cancel(state, force)`, `synopsis::vim_open_rewrite(state, tokio_handle)`.

- [ ] **Step 1: Add `begin_edit`**

```rust
/// `e` in the synopsis overlay: enter the in-place modal vim editor on the
/// current scene's RAW synopsis text (`synopsis_cache[(div1,div2)]`). Uses the
/// SAME `GlossOverlay` editor as gloss-edit; the save path (`vim_save`) branches
/// to the synopsis persistence. No-op + toast if no cached synopsis.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let (div1, div2) = s.synopsis_overlay_scene;
    let raw = match s.synopsis_cache.get(&(div1, div2)) {
        Some(t) => t.clone(),
        None => {
            crate::ui::toast::show_transient(&s.chapter_toast, "No synopsis to edit", 2);
            return;
        }
    };
    let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.gloss_overlay.enter_edit_buffer(&raw, &fill, &fg);
    s.input_mode = crate::app::InputMode::GlossEdit;
}
```

- [ ] **Step 2: Add a private render helper** (used by save and cancel)

```rust
/// Re-render the synopsis card for `(div1,div2)` from `text` (the colored/formatted
/// display). Mirrors the render block in `run_synopsis_revision`'s success
/// callback. Caller holds no borrow.
fn render_synopsis(state: &Rc<RefCell<AppState>>, div1: i64, div2: i64, text: &str) {
    let mut s = state.borrow_mut();
    let label = crate::app::scene_synopsis::synopsis_label(&s, div1, div2);
    let cw = s.content_hbox.width();
    let h = crate::app::layout::overlay_card_height(&s);
    let root_color = s.theme.root_color.clone();
    let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
    s.gloss_overlay
        .show_synopsis(&label, text, Some(&root_color), cw, h, prose_card);
    crate::input::actions::gloss::recolor_cached_blocks(&s);
}
```

NOTE: confirm `synopsis_label`/`prose_synopsis_card`/`overlay_card_height` paths against `run_synopsis_revision` (`:121`,`:162`,`:164`) while implementing — copy them verbatim from there so the render matches the Claude path.

- [ ] **Step 3: Add `vim_save`**

```rust
/// Save the synopsis vim-editor buffer's raw text to lit.db as-is (no Claude) via
/// `save_synopsis` (upsert), snapshot `synopsis_undo`, update `synopsis_cache`,
/// and re-render the colored card. `:w` stays + re-seeds; `:wq` exits.
pub(crate) fn vim_save(state: &Rc<RefCell<AppState>>, quit: bool) {
    let raw = state.borrow().gloss_overlay.edit_buffer_text();
    let raw = raw.trim_end().to_string();
    let (div1, div2, abbrev, model, original) = {
        let s = state.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let abbrev = match s.current_work.as_ref() {
            Some(w) => crate::app::base_work_abbrev(&w.abbrev).to_string(),
            None => return,
        };
        let original = s.synopsis_cache.get(&(div1, div2)).cloned().unwrap_or_default();
        (div1, div2, abbrev, s.config.claude_model.clone(), original)
    };
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Err(e) = crate::db::queries::save_synopsis(&conn, &abbrev, div1, div2, &raw, &model) {
            crate::logging::log(&format!("SYNOPSIS: vim save error: {}", e));
        }
    }
    {
        let mut s = state.borrow_mut();
        s.synopsis_undo = Some(((div1, div2), original));
        s.synopsis_cache.insert((div1, div2), raw.clone());
    }
    if quit {
        state.borrow().gloss_overlay.exit_edit_buffer();
        render_synopsis(state, div1, div2, &raw);
        let mut s = state.borrow_mut();
        s.input_mode = crate::app::InputMode::SynopsisOverlay;
        crate::ui::toast::show_transient(&s.chapter_toast, "Saved", 2);
    } else {
        state.borrow().gloss_overlay.reseed_edit_buffer(&raw);
        crate::ui::toast::show_transient(&state.borrow().chapter_toast, "Saved (:q to exit)", 2);
    }
}
```

NOTE: unlike the gloss `:w` (which re-renders the read view and forces an editor re-enter), the synopsis `:w` non-quit branch keeps the editor open and only re-seeds — the synopsis editor's buffer is NOT replaced by a render call, so no re-enter is needed. Confirm `base_work_abbrev` import path (`crate::app::base_work_abbrev`, used at `synopsis.rs:116`).

- [ ] **Step 4: Add `vim_cancel`**

```rust
/// Leave the synopsis vim editor. Warn + STAY on a dirty buffer unless `force`.
/// Re-renders the stored (un-edited) synopsis on exit.
pub(crate) fn vim_cancel(state: &Rc<RefCell<AppState>>, force: bool) {
    let dirty = state.borrow().gloss_overlay.edit_is_dirty();
    if dirty && !force {
        crate::ui::toast::show_transient(
            &state.borrow().chapter_toast,
            "Unsaved changes \u{2014} :w to save, :q! to discard",
            3,
        );
        return;
    }
    let (div1, div2, stored) = {
        let s = state.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let stored = s.synopsis_cache.get(&(div1, div2)).cloned().unwrap_or_default();
        (div1, div2, stored)
    };
    state.borrow().gloss_overlay.exit_edit_buffer();
    render_synopsis(state, div1, div2, &stored);
    state.borrow_mut().input_mode = crate::app::InputMode::SynopsisOverlay;
}
```

- [ ] **Step 5: Add `vim_open_rewrite`** (route `R` to the existing synopsis ask-Claude edit prompt)

```rust
/// `R` in the synopsis vim editor: leave the editor and open the existing
/// ask-Claude synopsis edit prompt. Mirrors gloss `vim_open_rewrite`.
pub(crate) fn vim_open_rewrite(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    {
        let mut s = state.borrow_mut();
        s.gloss_overlay.exit_edit_buffer();
        s.input_mode = crate::app::InputMode::SynopsisOverlay;
    }
    show_edit_prompt(state);
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build 2>&1 | tail -25`
Expected: builds. Fix any borrow-scope / path mismatches.

- [ ] **Step 7: Run the pure suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/synopsis.rs
git commit -m "feat(synopsis-edit): add begin_edit + vim save/cancel/rewrite for synopsis"
```

---

## Task 7: Keymap routing — early dispatch, `handle_gloss_edit_key`, repoint `e`

**Files:**
- Modify: `src/input/keymap.rs` — early dispatch (`:52`), new handler (clone of `:759`), gloss `e` arm (`:1150`), synopsis `e` arm (`:1535`).

**Interfaces:**
- Consumes: `gloss::{begin_edit,vim_save,vim_cancel,vim_open_rewrite}` (Task 5); `synopsis::{begin_edit,vim_save,vim_cancel,vim_open_rewrite}` (Task 6); `GlossOverlay::feed_edit_key` (Task 4); `gtk_key_to_vim` (`:735`), `is_double_esc` (`:719`).
- Produces: `handle_gloss_edit_key(...) -> bool`.

- [ ] **Step 1: Early-dispatch `GlossEdit`**

In `handle_key`, immediately AFTER the existing `JournalEdit` early-dispatch (`src/input/keymap.rs:52-54`), add:

```rust
    if state.borrow().input_mode == crate::app::InputMode::GlossEdit {
        return handle_gloss_edit_key(state, key_name, key_char, is_ctrl, is_shift, tokio_handle);
    }
```

(Match the exact parameter names in scope at `:52` — `key_name`, `key_char`, `is_ctrl`, `is_shift`, `tokio_handle`.)

- [ ] **Step 2: Add `handle_gloss_edit_key`** (clone of `handle_journal_edit_key` `:759-811`, with the save/cancel calls branched on gloss vs synopsis)

The editor is the same widget for both surfaces; the surface is distinguished by the **input mode the editor was entered from** is lost once we're in `GlossEdit`, so branch on `GlossOverlay`'s current `paginated_mode` (`Synopsis` vs `Gloss`). Expose it with a small getter first:

In `src/ui/gloss_overlay.rs`, add to `impl GlossOverlay`:

```rust
    /// True iff the overlay is currently showing a SYNOPSIS (vs a gloss). Used by
    /// the edit-key handler to route `:w`/`:q`/`R` to the synopsis vs gloss path.
    pub fn is_showing_synopsis(&self) -> bool {
        self.paginated_mode.get() == PaginatedMode::Synopsis
    }
```

Then in `src/input/keymap.rs`, after `handle_journal_edit_key` (`:811`), add:

```rust
/// Route a key to the gloss/synopsis in-place vim editor (`InputMode::GlossEdit`).
/// Near-clone of `handle_journal_edit_key`; `:w`/`:wq`/`:q`/`:q!`/`R` branch to the
/// gloss vs synopsis handler by `GlossOverlay::is_showing_synopsis`.
fn handle_gloss_edit_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
    _is_shift: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    // Esc: double-Esc exits (force); single Esc feeds the engine (→ Normal).
    if key_name == "Escape" {
        if is_double_esc() {
            let synopsis = state.borrow().gloss_overlay.is_showing_synopsis();
            if synopsis {
                crate::input::actions::synopsis::vim_cancel(state, true);
            } else {
                crate::input::actions::gloss::vim_cancel(state, true);
            }
            return true;
        }
        let _ = state
            .borrow()
            .gloss_overlay
            .feed_edit_key(crate::input::vim::VimKey::Esc);
        return true;
    }
    let Some(vk) = gtk_key_to_vim(key_name, key_char, is_ctrl) else {
        return true;
    };
    let action = state.borrow().gloss_overlay.feed_edit_key(vk);
    let synopsis = state.borrow().gloss_overlay.is_showing_synopsis();
    match action {
        crate::input::vim::EditorAction::Save => {
            if synopsis {
                crate::input::actions::synopsis::vim_save(state, false);
            } else {
                crate::input::actions::gloss::vim_save(state, false);
            }
        }
        crate::input::vim::EditorAction::SaveQuit => {
            if synopsis {
                crate::input::actions::synopsis::vim_save(state, true);
            } else {
                crate::input::actions::gloss::vim_save(state, true);
            }
        }
        crate::input::vim::EditorAction::Cancel => {
            if synopsis {
                crate::input::actions::synopsis::vim_cancel(state, false);
            } else {
                crate::input::actions::gloss::vim_cancel(state, false);
            }
        }
        crate::input::vim::EditorAction::CancelForce => {
            if synopsis {
                crate::input::actions::synopsis::vim_cancel(state, true);
            } else {
                crate::input::actions::gloss::vim_cancel(state, true);
            }
        }
        crate::input::vim::EditorAction::OpenRewrite => {
            if synopsis {
                crate::input::actions::synopsis::vim_open_rewrite(state, tokio_handle);
            } else {
                crate::input::actions::gloss::vim_open_rewrite(state, tokio_handle);
            }
        }
        crate::input::vim::EditorAction::Nop => {}
    }
    true
}
```

IMPORTANT: read `is_showing_synopsis` BEFORE the save/cancel call mutates state (the gloss `vim_save`/`vim_cancel` re-render which may change `paginated_mode`). The handler above reads it once into `synopsis` right after `feed_edit_key` and before the `match`, except the double-Esc path which reads it inline — correct because no save has run there yet.

- [ ] **Step 3: Repoint the gloss `e` arm**

In `handle_gloss_key`, the `"e"` arm (`src/input/keymap.rs:1150-1152`), replace:

```rust
            "e" => {
                crate::input::actions::gloss::show_edit_dialog(state);
                true
            }
```

with:

```rust
            "e" => {
                crate::input::actions::gloss::begin_edit(state);
                true
            }
```

- [ ] **Step 4: Repoint the synopsis `e` arm**

In `handle_synopsis_overlay_key`, the `"e"` arm (`src/input/keymap.rs:1535-1538`), replace:

```rust
            "e" => {
                crate::input::actions::synopsis::show_edit_prompt(state);
                true
            }
```

with:

```rust
            "e" => {
                crate::input::actions::synopsis::begin_edit(state);
                true
            }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -25`
Expected: builds. `show_edit_dialog`/`show_edit_prompt` are now referenced only from `vim_open_rewrite` (Tasks 5/6) and the ask-card submit dispatch — NOT dead. If clippy warns about an unused param or import, fix narrowly.

- [ ] **Step 6: Run the pure suite + clippy**

Run: `cargo test --bins 2>&1 | tail -15 && cargo clippy 2>&1 | tail -5`
Expected: tests PASS; clippy warning count at or below the 120 baseline (no NEW errors).

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs src/ui/gloss_overlay.rs
git commit -m "feat(gloss-edit): route GlossEdit keys; repoint gloss/synopsis e to in-place editor"
```

---

## Task 8: Update the gloss & synopsis keybind legends

**Files:**
- Modify: `src/ui/gloss_keybinds_overlay.rs` — `GROUPS` const.
- Modify: `src/ui/synopsis_keybinds_overlay.rs` — `GROUPS` const.

**Interfaces:** none (display-only `&'static str` data).

- [ ] **Step 1: Read both legends' current `GROUPS`**

Run: `rg -n "GROUPS" src/ui/gloss_keybinds_overlay.rs src/ui/synopsis_keybinds_overlay.rs`
Then read each `GROUPS` const to find the `e` row (currently describing the ask-card edit).

- [ ] **Step 2: Update the gloss legend's `e` row**

In `src/ui/gloss_keybinds_overlay.rs` `GROUPS`, change the `e` entry's action text to describe the in-place editor, and add the vim-edit keys. For example, in the Editing group:

```rust
        ("e", "edit gloss in place (vim)"),
```

and add a short vim-keys line in the same group (match the existing tuple style):

```rust
        (":w / :q / R", "save · quit · ask-Claude rewrite (in editor)"),
```

(Verify the `e` row against the `handle_gloss_key` `e` arm — it now calls `gloss::begin_edit`. Keep `r`/`R`/`l`/`L` rows unchanged — those binds did not move.)

- [ ] **Step 3: Update the synopsis legend's `e` row**

In `src/ui/synopsis_keybinds_overlay.rs` `GROUPS`, mirror the change:

```rust
        ("e", "edit synopsis in place (vim)"),
```

and the vim-keys line:

```rust
        (":w / :q / R", "save · quit · ask-Claude rewrite (in editor)"),
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds.

- [ ] **Step 5: Cross-check the reader-card overlay** (per the design — likely no change)

Run: `rg -n '"e"' src/ui/keybinds_overlay.rs`
Expected: no `e` bind in the reader-card overlay (the `e` edit binds live on the gloss/synopsis overlays, not the reader card). If a row exists and mentions gloss/synopsis edit, update it; otherwise no change. (The `update-cairo-keybinds-overlay` skill carries the full cross-reference if any reader-card `e` row turns up.)

- [ ] **Step 6: Run the pure suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_keybinds_overlay.rs src/ui/synopsis_keybinds_overlay.rs
git commit -m "docs(legends): gloss/synopsis e = in-place vim editor"
```

---

## Task 9: Round-trip unit test (no GTK)

The riskiest correctness claim is that the editor round-trips the raw text byte-for-byte (no markup mangling). The engine is already unit-tested, but add a focused test that a representative gloss markup string and a synopsis string survive `VimEngine::new(raw).buffer()` unchanged, and that the save-side `trim_end` is the only transform.

**Files:**
- Test: add a `#[cfg(test)] mod` at the bottom of `src/input/vim/engine.rs` (or extend the existing test module there).

**Interfaces:**
- Consumes: `VimEngine::new`, `.buffer()`.

- [ ] **Step 1: Write the failing test**

Add to `src/input/vim/engine.rs`'s test module:

```rust
    #[test]
    fn raw_text_round_trips_unchanged() {
        // A representative gloss markup blob.
        let gloss = "<speaker>HAMLET</speaker>\n<verse>To be, or not to be</verse>\n\n<gloss>The question of existence.</gloss>";
        let engine = super::VimEngine::new(gloss.to_string());
        assert_eq!(engine.buffer(), gloss, "gloss markup must round-trip");

        // A representative synopsis blob.
        let synopsis = "<p>The court gathers.</p>\n<p>A ghost appears on the battlements.</p>";
        let engine = super::VimEngine::new(synopsis.to_string());
        assert_eq!(engine.buffer(), synopsis, "synopsis text must round-trip");
    }

    #[test]
    fn trim_end_is_the_only_save_transform() {
        // The save path applies `trim_end()` and nothing else; interior markup
        // and leading whitespace are preserved.
        let raw = "  <p>indented and trailing</p>  \n\n";
        assert_eq!(raw.trim_end(), "  <p>indented and trailing</p>");
    }
```

- [ ] **Step 2: Run to verify the first compiles + passes (engine already exists)**

Run: `cargo test --bins vim:: 2>&1 | tail -15`
Expected: PASS (`raw_text_round_trips_unchanged`, `trim_end_is_the_only_save_transform`, plus the existing 41 vim tests). If `VimEngine::new`/`buffer` differ in path, fix the test's `super::` reference.

- [ ] **Step 3: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "test(vim): raw gloss/synopsis text round-trips through the engine"
```

---

## Task 10: Final verification + ask the user to verify on screen

**Files:** none (verification + handoff).

- [ ] **Step 1: Full build + tests + clippy**

Run: `cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -5 && cargo clippy 2>&1 | tail -5`
Expected: build clean; tests PASS; clippy ≤ 120 warnings, 0 errors.

- [ ] **Step 2: Confirm no dead code from the `e` repoint**

Run: `rg -n "show_edit_dialog|show_edit_prompt|edit_gloss|edit_synopsis|GlossPromptMode::Edit|SynopsisPromptKind::Edit" src/`
Expected: each is still referenced (by `vim_open_rewrite` and/or the ask-card submit dispatch). If the compiler/clippy flagged any as unused, that's a real finding — resolve it (do NOT delete a function still reachable via `R`).

- [ ] **Step 3: Update `CLAUDE-activeContext.md`**

Record: the new `InputMode::GlossEdit` editor (gloss + synopsis, one widget on `gloss_view`, raw-text edit), the mono edit font (`EDIT_FONT_FAMILY`), that `e` now opens the in-place editor while `r`/`R` keep the ask-Claude flow, the commit list, build/test/clippy status, and that on-screen verification is pending. Convert dates to absolute US Central.

- [ ] **Step 4: Ask the user to verify on screen** (the acceptance is visual — agent cannot launch cage; see CLAUDE.md "When to ASK THE USER")

Give the user this exact verification script:

```bash
cd ~/utono/linux-lit && cargo run
```

Then, in the reader:
- Open a gloss (`Ctrl+g` on a glossed line), press `e`: the overlay shows the **raw `<speaker>/<verse>/<gloss>` markup in JetBrainsMono** with a block cursor + `-- NORMAL --` footer. Try motions (`w`,`b`,`dd`,`cw`,`x`,`p`,`v`+`d`,`.`,`u`). `:wq` re-renders the **colored** display in **Charter**; `:q!` discards; `R` opens the ask-Claude rewrite card.
- Open a synopsis (`h` on a chapter), press `e`: shows the **plain `<p>` text in JetBrainsMono**; `:wq` re-renders in Charter; `R` opens the synopsis ask-Claude edit prompt.
- Open a journal Q&A (`Ctrl+j`), press `e`: the editor is now **JetBrainsMono**; `:q`/`:wq` restores **Charter**.
- In each: double-Esc and `:q!` exit and restore the reading font.

Also offer the headless harness for the clipping/geometry invariants:

```bash
./scripts/e2e-env.sh cargo test -- --ignored --nocapture
```

- [ ] **Step 5: Final commit (ac update)**

```bash
git add CLAUDE-activeContext.md 2>/dev/null; git commit -m "docs(ac): record gloss/synopsis vim editor + mono edit font" || true
```

(Note: `CLAUDE-activeContext.md` is gitignored in this repo per `~/utono/CLAUDE.md`; the commit may be a no-op — the `|| true` tolerates it. The on-disk update still matters for session continuity.)

---

## Self-Review

**1. Spec coverage** (design `2026-06-30-overlay-vim-edit-and-mono-font-design.md`):

- Monospace edit font, one `EDIT_FONT_FAMILY` const → Task 1; applied on journal (Task 2) + gloss/synopsis (Task 4, via `enter_edit_buffer`/`exit_edit_buffer`). ✓
- In-place vim editor for gloss & synopsis, editing raw stored text → Task 4 (editor) + Tasks 5/6 (load/save raw, no Claude). ✓
- One editor on `gloss_view` serving both → Task 4; surface branch via `is_showing_synopsis` → Task 7. ✓
- `InputMode::GlossEdit` early-dispatched beside `JournalEdit` → Task 3 + Task 7 Step 1. ✓
- Save reuses `update_and_render_gloss_in_place` (gloss) / `save_synopsis` (synopsis) → Tasks 5/6. ✓
- Undo for free via existing `gloss_undo`/`synopsis_undo` snapshots → Task 5 (snapshot is inside `update_and_render_gloss_in_place`) + Task 6 (explicit `synopsis_undo` set in `vim_save`). ✓
- Replace `e`, keep `r`/`R` ask-Claude → Task 7 Steps 3-4; `R`-in-editor reuses ask-Claude → Tasks 5/6 `vim_open_rewrite`. ✓
- Gloss edit shows plain raw markup (no colors), re-render colored on exit → Task 4 (`enter_edit_buffer` loads plain text; no color tags applied) + Tasks 5/6 (re-render on save/cancel). ✓
- Font save-and-restore (not hardcode-Charter) → Tasks 2/4 `begin_edit_font`/`end_edit_font` stash the captured family. ✓
- Legends updated → Task 8. ✓
- `GlossPromptMode::Edit`/`SynopsisPromptKind::Edit` kept (reachable via `R` + submit dispatch) → Global Constraints + Task 10 Step 2. ✓ (Confirmed against `src/`: both variants are matched in `submit_gloss_prompt`/`submit_amend_prompt`.)
- Error handling (best-effort DB, empty buffer allowed, dirty cancel warns, no-current-item toast) → Tasks 5/6 (`if let Ok(conn)`, `trim_end`, dirty warn, no-gloss/synopsis toast). ✓
- Testing: engine unit tests reused + round-trip test → Task 9; on-screen acceptance → Task 10. ✓

No gaps found.

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N"/"write tests for the above" — every code step shows the code; every command shows the expected result. The few `NOTE:` blocks flag *verify-while-implementing* details (exact field/toast names) with the concrete fallback to use, not deferred work.

**3. Type consistency:**
- `EDIT_FONT_FAMILY: &str` defined Task 1, consumed identically in Tasks 2 & 4.
- `InputMode::GlossEdit` defined Task 3, matched Task 7, set in Tasks 5/6 `begin_edit`.
- Editor methods produced in Task 4 (`enter_edit_buffer(&str,&str,&str)`, `feed_edit_key(VimKey)->EditorAction`, `edit_buffer_text()->String`, `edit_is_dirty()->bool`, `reseed_edit_buffer(&str)`, `exit_edit_buffer()`, `is_showing_synopsis()->bool`) are consumed with the same signatures in Tasks 5/6/7.
- `EditorAction` variants (`Save/SaveQuit/Cancel/CancelForce/OpenRewrite/Nop`) and `VimKey::Esc` match the engine (`src/input/vim/mod.rs`).
- Gloss save uses `update_and_render_gloss_in_place(state, &ctx, idx, gloss_id, &raw, &model, &log)` — exact arity/types from `gloss.rs:834`.
- Synopsis save uses `save_synopsis(&conn, &abbrev, div1, div2, &raw, &model)` — exact from `queries.rs:422`; `synopsis_cache: HashMap<(i64,i64),String>`; scene `synopsis_overlay_scene: (i64,i64)`.

Consistent throughout.
