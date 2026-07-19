# Chat-panel Journal-view `r` / `R` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the chat panel's Journal view, `r` asks a new question and `R` opens the rewrite popup on the selected saved Q&A, reusing the existing journal rewrite pipeline.

**Architecture:** Add a row cursor to the Journal view (`journal_cursor`), split it out from Question view's flat-scroll behavior, then bridge `R` into the overlay rewrite pipeline (Approach A): pre-seed `s.journal.pages`/`page_index`/band from the selected chat entry behind a scoped `rewrite_return` flag, reuse `open_rewrite_target`, and re-render the panel on completion. The `a`/`b` instruction-card paths reuse the chat panel's own input widget.

**Tech Stack:** Rust, GTK4, SQLite (lit.db); `cargo build` / `cargo test --bins`; headless cage/grim/wtype harness.

## Global Constraints

- Build with `cargo build`; do NOT run the app (`cargo run`) — the user launches it. (linux-lit CLAUDE.md)
- Work in this worktree only: `~/utono/linux-lit-wt/chat-journal-r-rewrite` on branch `feat/chat-journal-r-rewrite`. Never share `target/` or `CARGO_TARGET_DIR` with the main checkout.
- Keybind truth changes together: `keymap.rs` handler + the chat-panel Ctrl+/ legend (`src/ui/chat_keybinds_overlay.rs`). This is an overlay-context bind, NOT a `keymap_config.rs`/`keymap.json` default, so no JSON update.
- The `rewrite_return` bridge must leave every overlay-initiated rewrite path byte-for-byte unchanged (guard on the flag; default `false`).
- RPD layout: `r` and `R` are the literal keysyms (`r` unshifted, `R` = shift+r), already used by the existing arm — no keysym lookup needed.
- Pre-existing failing unit test on the clean tree: `config::last_gloss_tests::theme_cycle_defaults_to_reading_themes`. It is unrelated; treat the suite as green when only that one fails.

---

### Task 1: Add `journal_cursor` + `rewrite_return` fields to `ChatState`

**Files:**
- Modify: `src/input/actions/chat.rs` (the `ChatState` struct, ~line 106)

**Interfaces:**
- Produces: `ChatState.journal_cursor: usize` (index into `journal_list`), `ChatState.rewrite_return: bool`. Both default via `#[derive(Default)]` and reset with `s.chat = Default::default()` on panel close.

- [ ] **Step 1: Add the two fields**

In `src/input/actions/chat.rs`, inside `pub(crate) struct ChatState { ... }`, after the `journal_list` field, add:

```rust
    /// Row cursor for `PanelView::Journal`: index into `journal_list`. `j`/`k`
    /// step it, the accent bar (`.chat-cursor-row`) paints on the cursor
    /// entry's `Q:` widget row, and `R` rewrites this entry. Reset to 0 (top)
    /// on every toggle into Journal view — matches the "land at the top of the
    /// entry" behavior. A separate axis from `row_cursor` (Gloss view) and
    /// `cursor` (exchanges); never shared. Clamped to `journal_list` on every
    /// render.
    pub journal_cursor: usize,
    /// Set by `rewrite_journal_entry` while a panel-initiated `R` rewrite is in
    /// flight through the shared journal rewrite pipeline (which otherwise
    /// returns to the journal OVERLAY). The overlay-render / mode-restore sites
    /// in `journal.rs` (`rewrite_with_claude`'s success + error closures,
    /// `close_rewrite_target`, the panel instruction-card submit) guard on this
    /// to re-render the CHAT PANEL and restore `ChatTranscript` instead. Always
    /// cleared on the terminal outcome (success re-render, error, or cancel);
    /// defaults `false` and resets with the rest of `ChatState` on panel close.
    pub rewrite_return: bool,
```

- [ ] **Step 2: Build**

Run: `cd ~/utono/linux-lit-wt/chat-journal-r-rewrite && cargo build`
Expected: PASS (unused fields are allowed — `#[derive(Default)]`, and later tasks read them). If a dead-code warning appears for `rewrite_return`, that is fine; it is consumed in Task 5+.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "feat(chat): ChatState.journal_cursor + rewrite_return fields"
```

---

### Task 2: Journal-view row cursor — pure helpers + render

**Files:**
- Modify: `src/input/actions/chat.rs` (`render_journal_view` ~1255, `toggle_panel_view` ~1296, new pure helpers, new `#[cfg(test)]` cases)

**Interfaces:**
- Consumes: `ChatState.journal_cursor` (Task 1); `journal_view_rows(&[JournalPage]) -> Vec<TranscriptRow>` (existing); `render_rows_focused_cursor(rows, cursor_widget_index, selection)` (existing on `chat_panel`).
- Produces:
  - `fn journal_entry_qrow(entry: usize) -> usize` — the `Q:` widget-row index for `journal_list` entry `entry` (each entry is exactly two widget rows: `Q:` then answer). Returns `2 * entry`.
  - `fn clamp_journal_cursor(cursor: usize, len: usize) -> usize` — clamp to `[0, len-1]`, or `0` when `len == 0`.

- [ ] **Step 1: Write failing tests for the pure helpers**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `src/input/actions/chat.rs`:

```rust
    #[test]
    fn journal_entry_qrow_is_two_per_entry() {
        assert_eq!(journal_entry_qrow(0), 0);
        assert_eq!(journal_entry_qrow(1), 2);
        assert_eq!(journal_entry_qrow(3), 6);
    }

    #[test]
    fn clamp_journal_cursor_bounds() {
        assert_eq!(clamp_journal_cursor(0, 0), 0); // empty list
        assert_eq!(clamp_journal_cursor(5, 0), 0); // empty list, stale cursor
        assert_eq!(clamp_journal_cursor(0, 3), 0);
        assert_eq!(clamp_journal_cursor(2, 3), 2);
        assert_eq!(clamp_journal_cursor(9, 3), 2); // clamps to len-1
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bins journal_entry_qrow_is_two_per_entry clamp_journal_cursor_bounds 2>&1 | tail -5`
Expected: FAIL (`cannot find function journal_entry_qrow` / `clamp_journal_cursor`).

- [ ] **Step 3: Implement the helpers**

Add near `journal_view_rows` (before `render_journal_view`) in `src/input/actions/chat.rs`:

```rust
/// The `Q:` widget-row index for `journal_list` entry `entry`. Each entry
/// renders as exactly two widget rows (a `Q:` row then an `Answer` row — see
/// `journal_view_rows`), so entry `i` owns rows `2*i` (question) and `2*i + 1`
/// (answer). The row cursor (and `R`'s target) anchors on the `Q:` row.
fn journal_entry_qrow(entry: usize) -> usize {
    entry * 2
}

/// Clamp a Journal-view row cursor to a list of `len` entries: `[0, len-1]`, or
/// `0` for an empty list (which renders a single non-landable placeholder row).
fn clamp_journal_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        cursor.min(len - 1)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bins journal_entry_qrow_is_two_per_entry clamp_journal_cursor_bounds 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Render the cursor in `render_journal_view`**

Replace the body of `render_journal_view` (currently `render_rows_to_top`) in `src/input/actions/chat.rs` with a cursor-aware render:

```rust
fn render_journal_view(s: &mut AppState) {
    let rows = journal_view_rows(&s.chat.journal_list);
    let len = s.chat.journal_list.len();
    s.chat.journal_cursor = clamp_journal_cursor(s.chat.journal_cursor, len);
    if len == 0 {
        // Placeholder-only list: no landable row, scroll to top, no accent bar.
        s.chat_panel.render_rows_to_top(&rows);
        return;
    }
    // Land the accent bar on the cursor entry's `Q:` widget row;
    // `render_rows_focused_cursor` scrolls that row to the top. No visual
    // selection in Journal view.
    let qrow = journal_entry_qrow(s.chat.journal_cursor);
    s.chat_panel.render_rows_focused_cursor(&rows, qrow, None);
}
```

Note: `render_journal_view` now takes `&mut AppState` (was `&AppState`) because it writes the clamped cursor. Update its one caller in `toggle_panel_view` (`render_journal_view(&s)` → `render_journal_view(&mut s)`), and any other caller flagged by the build.

- [ ] **Step 6: Reset the cursor to top on toggle into Journal view**

In `toggle_panel_view`, in the `PanelView::Journal` arm (~1296), after `s.chat.journal_list = reload_journal_list(...)` and before `render_journal_view(...)`, add:

```rust
            s.chat.journal_cursor = 0;
```

- [ ] **Step 7: Build + run the full bin suite**

Run: `cargo build && cargo test --bins 2>&1 | rg "test result|error\[" | tail -5`
Expected: build PASS; test result shows only the known pre-existing `theme_cycle_defaults_to_reading_themes` failure (all new tests pass).

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "feat(chat): Journal-view row cursor render + reset-on-toggle"
```

---

### Task 3: `j` / `k` / `gg` / `G` move the Journal-view row cursor

**Files:**
- Modify: `src/input/actions/chat.rs` (`transcript_cursor_move` ~1627, `transcript_cursor_first` ~1733, `transcript_cursor_last` ~1751)

**Interfaces:**
- Consumes: `clamp_journal_cursor` (Task 2), `render_journal_view(&mut AppState)` (Task 2), `ChatState.journal_cursor` (Task 1).
- Produces: Journal-view stepping via `journal_cursor`; Question view unchanged (still flat-scroll).

- [ ] **Step 1: Add a pure step test**

Add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn step_journal_cursor_clamps_no_wrap() {
        // down from 0 in a 3-entry list
        assert_eq!(step_journal_cursor(0, 1, 3), 1);
        // up from 0 stays at 0 (no wrap)
        assert_eq!(step_journal_cursor(0, -1, 3), 0);
        // down from last stays at last (no wrap)
        assert_eq!(step_journal_cursor(2, 1, 3), 2);
        // empty list stays 0
        assert_eq!(step_journal_cursor(0, 1, 0), 0);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins step_journal_cursor_clamps_no_wrap 2>&1 | tail -5`
Expected: FAIL (`cannot find function step_journal_cursor`).

- [ ] **Step 3: Implement `step_journal_cursor`**

Add near `clamp_journal_cursor` in `src/input/actions/chat.rs`:

```rust
/// Step a Journal-view row cursor by `delta` (±1) within a `len`-entry list,
/// clamped with NO wrap: already at the first/last entry stays put. Empty list
/// stays at 0.
fn step_journal_cursor(cursor: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = (cursor as i64 + delta as i64).clamp(0, len as i64 - 1);
    next as usize
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins step_journal_cursor_clamps_no_wrap 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Split Journal out of the flat-scroll guard in `transcript_cursor_move`**

In `transcript_cursor_move`, replace the combined guard:

```rust
    if s.chat.view == PanelView::Journal || s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_step(delta as f64);
        return;
    }
```

with a Journal-specific branch above the Question flat-scroll:

```rust
    if s.chat.view == PanelView::Journal {
        let len = s.chat.journal_list.len();
        if len == 0 {
            s.chat_panel.scroll_transcript_step(delta as f64);
            return;
        }
        s.chat.journal_cursor = step_journal_cursor(s.chat.journal_cursor, delta, len);
        render_journal_view(s);
        return;
    }
    if s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_step(delta as f64);
        return;
    }
```

- [ ] **Step 6: Journal `gg`/`G` move the cursor to first/last entry**

In `transcript_cursor_first`, replace:

```rust
    if s.chat.view == PanelView::Journal || s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_to_edge(false);
        return;
    }
```

with:

```rust
    if s.chat.view == PanelView::Journal {
        if !s.chat.journal_list.is_empty() {
            s.chat.journal_cursor = 0;
            render_journal_view(s);
        } else {
            s.chat_panel.scroll_transcript_to_edge(false);
        }
        return;
    }
    if s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_to_edge(false);
        return;
    }
```

In `transcript_cursor_last`, replace the analogous guard with:

```rust
    if s.chat.view == PanelView::Journal {
        let len = s.chat.journal_list.len();
        if len != 0 {
            s.chat.journal_cursor = len - 1;
            render_journal_view(s);
        } else {
            s.chat_panel.scroll_transcript_to_edge(true);
        }
        return;
    }
    if s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_to_edge(true);
        return;
    }
```

- [ ] **Step 7: Build + full bin suite**

Run: `cargo build && cargo test --bins 2>&1 | rg "test result|error\[" | tail -5`
Expected: build PASS; only the known pre-existing failure.

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "feat(chat): j/k/gg/G step the Journal-view row cursor"
```

---

### Task 4: `r` asks a new question in Journal view (keymap split)

**Files:**
- Modify: `src/input/keymap.rs` (the `"r" | "R"` arm in `handle_chat_transcript_key`, ~1502)

**Interfaces:**
- Consumes: `chat::focus_prompt_insert(&mut AppState)` (existing), `chat::regloss_pinned(&Rc<RefCell<AppState>>)` (existing), `chat::PanelView` (existing, `pub(crate)`).
- Produces: nothing new; `R` still routes to `regloss_pinned` here until Task 5 replaces it.

- [ ] **Step 1: Split the `"r" | "R"` arm — `r` in Journal view opens the ask input**

In `src/input/keymap.rs`, replace:

```rust
        "r" | "R" => {
            crate::input::actions::chat::regloss_pinned(state);
            true
        }
```

with:

```rust
        "r" => {
            // Journal view: `r` asks a NEW question (the panel's own ask input,
            // same as `a`), matching the main journal overlay's `r`. Other
            // views keep regloss.
            if state.borrow().chat.view == crate::input::actions::chat::PanelView::Journal {
                crate::input::actions::chat::focus_prompt_insert(&mut state.borrow_mut());
            } else {
                crate::input::actions::chat::regloss_pinned(state);
            }
            true
        }
        "R" => {
            // Journal view: `R` opens the rewrite popup on the selected entry
            // (Task 5). Other views keep regloss. Until Task 5 lands,
            // regloss_pinned is the fallback for BOTH branches.
            crate::input::actions::chat::regloss_pinned(state);
            true
        }
```

Note: `PanelView` must be reachable as `crate::input::actions::chat::PanelView`. It is declared `pub(crate)` in `chat.rs`, so this path resolves.

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg "^error|error\[|Finished" | tail -5`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(chat): r asks a new question in Journal view"
```

---

### Task 5: `R` rewrites the selected Journal entry — the Approach-A bridge

**Files:**
- Modify: `src/input/actions/chat.rs` (new `rewrite_journal_entry`, new panel instruction-card submit helpers)
- Modify: `src/input/actions/journal.rs` (`close_rewrite_target` ~1790, `rewrite_with_claude` success + error closures ~2100/2108, `rewrite_question_path`'s `both` branch ~1858–1864)
- Modify: `src/input/keymap.rs` (Task 4's `"R"` arm → `rewrite_journal_entry` in Journal view; a panel branch in `handle_rewrite_target_key` for `a`/`b`)

**Interfaces:**
- Consumes: `chat.gloss_ctx: Option<GlossContext>` (has `work_abbrev`, `act`, `scene`, `start_citation`, `end_citation`), `chat.journal_list`, `chat.journal_cursor`, `chat.rewrite_return`; `journal::open_rewrite_target`, `journal::reload...` (chat's own `reload_journal_list`), `JournalBand::Passage { div1, div2, start, end }`.
- Produces:
  - `chat::rewrite_journal_entry(&Rc<RefCell<AppState>>)` — seeds overlay state + `rewrite_return`, opens the rewrite popup.
  - `chat::finish_panel_rewrite(&mut AppState)` — re-sync `journal_list`, re-render Journal view keeping cursor on the entry (by `id`), restore `ChatTranscript`, clear `rewrite_return`. Called from `journal.rs`'s guarded sites.
  - `chat::open_rewrite_instruction_input(&mut AppState)` + `chat::submit_panel_rewrite(&Rc<RefCell<AppState>>)` — the panel instruction card for the `a`/`b` paths.

- [ ] **Step 1: Write `rewrite_journal_entry`**

Add to `src/input/actions/chat.rs`:

```rust
/// `R` in the chat panel's Journal view: rewrite the SELECTED saved Q&A by
/// reusing the journal overlay's rewrite popup + pipeline (Approach A). Seeds
/// `s.journal.pages`/`page_index`/band from the cursor'd `journal_list` entry so
/// `displayed_journal_page` resolves it, sets `rewrite_return` so the pipeline's
/// overlay-render sites re-render THIS panel instead, then opens the popup.
///
/// No-op (toast) with no `gloss_ctx` (panel opened via Tab, never glossed) or an
/// empty `journal_list` — mirrors `toggle_panel_view`/`regloss_pinned`.
pub(crate) fn rewrite_journal_entry(state_rc: &Rc<RefCell<AppState>>) {
    {
        let mut s = state_rc.borrow_mut();
        if s.chat.view != PanelView::Journal {
            return;
        }
        let Some(ctx) = s.chat.gloss_ctx.clone() else {
            crate::input::navigation::show_chapter_toast_secs(&s, "No passage to rewrite", 2);
            return;
        };
        if s.chat.journal_list.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, "No journal entry to rewrite", 2);
            return;
        }
        let cursor = clamp_journal_cursor(s.chat.journal_cursor, s.chat.journal_list.len());
        s.chat.journal_cursor = cursor;
        // Seed the overlay page state so displayed_journal_page() resolves the
        // selected entry. Clear any stale filter (the panel has none, but the
        // pipeline reads journal.filter first).
        s.journal.filter = None;
        s.journal.pages = s.chat.journal_list.clone();
        s.journal.page_index = cursor;
        s.journal_band = crate::app::JournalBand::Passage {
            div1: ctx.act,
            div2: ctx.scene,
            start: ctx.start_citation.clone(),
            end: ctx.end_citation.clone(),
        };
        s.chat.rewrite_return = true;
    }
    // Opens the q/a/b popup (InputMode::RewriteTargetChoice); the pipeline runs
    // unchanged, and the rewrite_return guards in journal.rs route completion
    // back to this panel.
    crate::input::actions::journal::open_rewrite_target(state_rc);
}
```

- [ ] **Step 2: Write `finish_panel_rewrite`**

Add to `src/input/actions/chat.rs`:

```rust
/// Return a panel-initiated `R` rewrite to the chat panel: reload the pinned
/// passage's journal list from lit.db, re-render Journal view with the cursor
/// still on the rewritten entry (re-found by `id` so a timestamp bump can't
/// strand it), restore `ChatTranscript`, and clear `rewrite_return`. Called by
/// journal.rs's rewrite-completion / cancel sites when `rewrite_return` is set.
/// `rewritten_id` is the entry that changed (`None` on cancel — keep the cursor
/// where it is).
pub(crate) fn finish_panel_rewrite(s: &mut AppState, rewritten_id: Option<i64>) {
    if let Some(ctx) = s.chat.gloss_ctx.clone() {
        s.chat.journal_list =
            reload_journal_list(&ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation);
    }
    if let Some(id) = rewritten_id {
        if let Some(pos) = s.chat.journal_list.iter().position(|p| p.id == id) {
            s.chat.journal_cursor = pos;
        }
    }
    s.chat.journal_cursor = clamp_journal_cursor(s.chat.journal_cursor, s.chat.journal_list.len());
    s.chat.view = PanelView::Journal;
    render_journal_view(s);
    s.input_mode = crate::app::InputMode::ChatTranscript;
    s.chat.rewrite_return = false;
}
```

- [ ] **Step 3: Guard `close_rewrite_target` for the panel return**

In `src/input/actions/journal.rs`, `close_rewrite_target` currently ends with `s.input_mode = InputMode::JournalOverlay;`. Replace that tail so a panel-initiated cancel (Esc) — which never reaches the success closure — restores the panel:

```rust
    // Panel-initiated R (rewrite_return): a cancel (Esc) at the popup never
    // reaches the rewrite success closure, so restore the chat panel here. The
    // q/a/b dispatch paths ALSO call close_rewrite_target first, but they then
    // run the rewrite, whose success closure calls finish_panel_rewrite and
    // re-clears the flag — so setting ChatTranscript here is harmless for them
    // (immediately overwritten) and correct for the Esc path.
    if s.chat.rewrite_return {
        s.input_mode = InputMode::ChatTranscript;
        // Esc-cancel: no entry changed. If a rewrite is actually running, its
        // success closure will call finish_panel_rewrite and re-render; if this
        // was a cancel, we still owe a re-render + flag clear.
        // We cannot know here whether a rewrite will follow, so DON'T clear the
        // flag yet — the q/a/b handlers set it fresh, and the Esc arm clears it
        // explicitly (see keymap Task step 6). Just set the mode.
        return;
    }
    s.input_mode = InputMode::JournalOverlay;
```

Note: keep `close_rewrite_target`'s existing `borrow_mut` + weakref teardown above this tail unchanged; only the final mode assignment is replaced.

- [ ] **Step 4: Guard `rewrite_with_claude`'s success + error closures**

In `src/input/actions/journal.rs`, in `rewrite_with_claude`'s success closure, replace the non-filter render tail:

```rust
            } else {
                render_current(&mut s);
                land_on_current_band_id(&mut s, id);
            }
```

with a panel-aware branch:

```rust
            } else if s.chat.rewrite_return {
                // Panel-initiated R: re-render the chat panel, not the hidden
                // overlay. finish_panel_rewrite reloads journal_list, keeps the
                // cursor on entry `id`, restores ChatTranscript, clears the flag.
                crate::input::actions::chat::finish_panel_rewrite(&mut s, Some(id));
            } else {
                render_current(&mut s);
                land_on_current_band_id(&mut s, id);
            }
```

In the same function's error closure (the `move |st, msg|` at the end), replace:

```rust
        move |st, msg| {
            let s = st.borrow();
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 4);
        },
```

with:

```rust
        move |st, msg| {
            let mut s = st.borrow_mut();
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 4);
            // Panel-initiated R that errored: don't strand the panel in a stale
            // flag / wrong mode. Re-render the (unchanged) journal list and
            // restore ChatTranscript.
            if s.chat.rewrite_return {
                crate::input::actions::chat::finish_panel_rewrite(&mut s, None);
            }
        },
```

- [ ] **Step 5: Route the `both` improve path's card to the panel**

In `rewrite_question_path`'s `improve_question` `on_done` (the `if both { ... begin_rewrite_with(...) }` branch, ~1858), the instruction card must be the PANEL's when `rewrite_return` is set. Replace:

```rust
        if both {
            crate::input::navigation::show_chapter_toast_secs(&st.borrow(), "Question improved", 2);
            begin_rewrite_with(st, id, &improved_q, &answer);
        } else {
```

with:

```rust
        if both {
            crate::input::navigation::show_chapter_toast_secs(&st.borrow(), "Question improved", 2);
            let to_panel = st.borrow().chat.rewrite_return;
            if to_panel {
                // Stash the (id, improved_q, answer, Both) tuple and open the
                // PANEL's instruction card (the overlay's is on a hidden widget).
                st.borrow_mut().journal.vim_rewrite =
                    Some((id, improved_q.clone(), answer.clone(), RewriteTarget::Both));
                crate::input::actions::chat::open_rewrite_instruction_input(&mut st.borrow_mut());
            } else {
                begin_rewrite_with(st, id, &improved_q, &answer);
            }
        } else {
```

- [ ] **Step 6: Write the panel instruction card + submit**

Add to `src/input/actions/chat.rs`:

```rust
/// Open the chat panel's own input as a rewrite-INSTRUCTION card for a
/// panel-initiated `R` on the `a` (answer) or `b` (both) path. The overlay's
/// instruction card lives on the hidden journal_overlay widget, so the panel
/// must show its own. `submit_panel_rewrite` (Ctrl+Enter) reads the typed
/// instruction and runs the stashed `journal.vim_rewrite` tuple. Opens in vim
/// NORMAL (matching the overlay card) so the empty-Ctrl+Enter meaning is read
/// first; press `i` to type.
pub(crate) fn open_rewrite_instruction_input(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatPrompt;
    s.chat_panel.open_input(
        "Rewrite instruction",
        "Ctrl+Enter rewrite \u{00b7} empty = afresh \u{00b7} Esc cancel",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
        false,
    );
    s.chat_panel.flash_input();
}

/// Ctrl+Enter in the panel rewrite-instruction card: mirror journal
/// `submit_prompt`'s rewrite branch, but read the PANEL's input text and keep
/// `rewrite_return` set so the completion re-renders the panel. No-op when no
/// `vim_rewrite` is stashed (defensive — this is only opened by the `a`/`b`
/// panel path, which always stashes first).
pub(crate) fn submit_panel_rewrite(state_rc: &Rc<RefCell<AppState>>) {
    let text = state_rc.borrow().chat_panel.take_input_text();
    let rewrite = state_rc.borrow_mut().journal.vim_rewrite.take();
    state_rc.borrow().chat_panel.close_input();
    let Some((id, question, answer, target)) = rewrite else {
        // Nothing stashed: fall back to transcript focus.
        focus_transcript(&mut state_rc.borrow_mut());
        return;
    };
    let instruction = text.trim();
    let instruction = if instruction.is_empty() {
        "No further instruction was given; answer this question afresh under the standard guidance, grounded as before."
    } else {
        instruction
    };
    crate::input::actions::journal::rewrite_with_claude(
        state_rc, id, &question, &answer, instruction, target,
    );
}
```

Note: `rewrite_with_claude` is currently private (`fn`) in journal.rs. Make it `pub(crate) fn rewrite_with_claude` so the panel submit can call it. `RewriteTarget` is already `pub(crate)`.

- [ ] **Step 7: Wire the `a` path + panel Ctrl+Enter/Esc in the keymap**

In `src/input/keymap.rs`:

(a) Task 4's `"R"` arm — route Journal view to the new function:

```rust
        "R" => {
            if state.borrow().chat.view == crate::input::actions::chat::PanelView::Journal {
                crate::input::actions::chat::rewrite_journal_entry(state);
            } else {
                crate::input::actions::chat::regloss_pinned(state);
            }
            true
        }
```

(b) In `handle_rewrite_target_key`, the `a` path opens the instruction card — send it to the panel when `rewrite_return` is set. Replace the `"a"` arm:

```rust
        "a" => {
            crate::input::actions::journal::close_rewrite_target(state);
            if state.borrow().chat.rewrite_return {
                // Stash (id, q, a, Answer) from the seeded page + open the PANEL card.
                let stashed = {
                    let s = state.borrow();
                    crate::input::actions::journal::displayed_journal_page(&s)
                        .map(|p| (p.id, p.question.clone(), p.answer.clone()))
                };
                if let Some((id, q, a)) = stashed {
                    state.borrow_mut().journal.vim_rewrite =
                        Some((id, q, a, crate::input::actions::journal::RewriteTarget::Answer));
                    crate::input::actions::chat::open_rewrite_instruction_input(&mut state.borrow_mut());
                }
            } else {
                crate::input::actions::journal::begin_rewrite(state);
            }
            true
        }
```

Note: `RewriteTarget` is defined in `src/input/actions/journal.rs` (line 41), so the path is `crate::input::actions::journal::RewriteTarget` (verified). It is `pub enum`, so reachable.

(c) `handle_chat_prompt_key` (keymap.rs ~1359) does NOT have separate Ctrl+Enter/Escape arms — it routes both through `ask_vim_intercept`, passing a `submit` closure (fired on Ctrl+Enter) and a `close` closure (fired on double-Esc / `:q`). Make BOTH rewrite-aware. Replace the `ask_vim_intercept(...)` call's two closures:

Currently the call passes `crate::input::actions::chat::submit_chat_prompt` as `submit` and an inline `close` closure. Replace the whole `match ask_vim_intercept( ... ) { ... }` block with:

```rust
    match ask_vim_intercept(
        true,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().chat_panel.feed_input_vim_key(k),
        // Ctrl+Enter: a stashed vim_rewrite (panel `R` → a/b instruction card)
        // means this text is a REWRITE INSTRUCTION, not a new question.
        |st| {
            if st.borrow().journal.vim_rewrite.is_some() {
                crate::input::actions::chat::submit_panel_rewrite(st);
            } else {
                crate::input::actions::chat::submit_chat_prompt(st);
            }
        },
        // Double-Esc / :q: cancel a pending panel rewrite (clear the stash and
        // return to Journal view) if one is armed; else the normal "hide input,
        // focus transcript".
        |st| {
            let cancel_rewrite = st.borrow().journal.vim_rewrite.is_some()
                && st.borrow().chat.rewrite_return;
            if cancel_rewrite {
                st.borrow_mut().journal.vim_rewrite = None;
                st.borrow().chat_panel.close_input();
                let mut s = st.borrow_mut();
                crate::input::actions::chat::finish_panel_rewrite(&mut s, None);
                return;
            }
            let mut s = st.borrow_mut();
            s.chat_panel.close_input();
            crate::input::actions::chat::focus_transcript(&mut s);
        },
        |st, t| st.borrow().chat_panel.paste_input_text(t),
    ) {
        AskIntercept::Consumed => true,
        AskIntercept::NotHandled => true, // prompt focus consumes everything
    }
```

(d) The RewriteTargetChoice Escape arm (`handle_rewrite_target_key`'s `"Escape"`) must clear `rewrite_return` for the panel cancel. Replace:

```rust
        "Escape" => {
            crate::input::actions::journal::close_rewrite_target(state);
            true
        }
```

with:

```rust
        "Escape" => {
            crate::input::actions::journal::close_rewrite_target(state);
            // Panel-initiated cancel: close_rewrite_target set ChatTranscript
            // mode but left rewrite_return set (it can't tell cancel from
            // dispatch). Finish the panel return now.
            if state.borrow().chat.rewrite_return {
                let mut s = state.borrow_mut();
                crate::input::actions::chat::finish_panel_rewrite(&mut s, None);
            }
            true
        }
```

- [ ] **Step 8: Build**

Run: `cargo build 2>&1 | rg "^error|error\[|Finished" | tail -15`
Expected: `Finished`. Resolve any `RewriteTarget` path / visibility errors per the notes above (make `rewrite_with_claude` `pub(crate)`; match the `RewriteTarget` import path to journal.rs's own usage).

- [ ] **Step 9: Run the full bin suite**

Run: `cargo test --bins 2>&1 | rg "test result|error\[" | tail -5`
Expected: only the known pre-existing `theme_cycle_defaults_to_reading_themes` failure.

- [ ] **Step 10: Commit**

```bash
git add src/input/actions/chat.rs src/input/actions/journal.rs src/input/keymap.rs
git commit -m "feat(chat): R rewrites the selected Journal entry via the shared pipeline"
```

---

### Task 6: Chat-panel Ctrl+/ legend — Journal-view r/R

**Files:**
- Modify: `src/ui/chat_keybinds_overlay.rs` (GROUPS)

**Interfaces:**
- Consumes: nothing (data-only legend).

- [ ] **Step 1: Add the Journal-view r/R rows**

Open `src/ui/chat_keybinds_overlay.rs`, find the GROUPS const. Add to the group that lists `r`/`R` (or the Editing/Journal group; if none, add to the most relevant group) two rows:

```rust
        ("r", "Journal view: ask a new question about this passage"),
        ("R", "Journal view: rewrite the selected Q&A (q/a/b popup)"),
```

Keep the existing regloss `r`/`R` description if one exists; if the legend currently documents `r`/`R` = regloss, reword it to note the view split (e.g. `("r / R", "Gloss view: regloss \u{00b7} Journal view: ask / rewrite")`). Match the surrounding rows' phrasing style.

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg "^error|error\[|Finished" | tail -5`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add src/ui/chat_keybinds_overlay.rs
git commit -m "docs(chat): Ctrl+/ legend notes Journal-view r/R"
```

---

### Task 7: Headless verification + manual hand-off

**Files:** none (verification only).

- [ ] **Step 1: Build for headless**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 2: Drive the panel headlessly**

Follow the `test-headless-navigation` skill / linux-lit CLAUDE.md headless recipe (cage + `GSK_RENDERER=cairo` + `LIT_NO_MPV=1` + `LIT_DEV=1`, resize to 1920x1200). Open a work with a passage that has ≥2 saved journal Q&As, open the chat panel floating (`-` on a visual selection, or the pinned-passage path), toggle to Journal view (`\`), then:
  - Press `j`/`k` → screenshot each: the `.chat-cursor-row` accent bar moves between entries.
  - Press `R` → screenshot: the "Rewrite target" popup (`q · a · b · Esc`) is visible.
  - Press `q` → screenshot after the round-trip: the list re-renders, cursor still on the entry, panel back in transcript focus.
  - Reopen `R` → `a` → screenshot: the panel's "Rewrite instruction" input card is visible (NOT a hidden/absent card); type an instruction, Ctrl+Enter, screenshot the rewritten entry.
  - Press `r` → screenshot: the panel ask input opens in INSERT.

Open every PNG and report what you see inline (per the UI review protocol). A green exit code is not enough.

- [ ] **Step 3: Manual hand-off to the user**

Give the user the exact `crll` steps to eyeball on the real GL renderer: open the panel, `\` to Journal, `j`/`k` to select, `R` → `q`/`a`/`b`, confirm the correct entry changed and the cursor stayed on it; `r` asks a new question. State that cage is software rendering, so this final eyeball is on the real renderer.

---

## Self-Review

**Spec coverage:**
- Row cursor (spec §1) → Tasks 1–3. ✓
- `r` = ask (spec §2) → Task 4. ✓
- `R` = rewrite bridge (spec §3) → Task 5 (seed state, `rewrite_return`, `finish_panel_rewrite`, guarded sites). ✓
- Keymap split (spec §4) → Tasks 4–5. ✓
- Legend (spec §5) → Task 6. ✓
- Instruction-card gap (resolved after spec: reuse panel input) → Task 5 steps 5–7. ✓ (Spec addendum: the `a`/`b` instruction step uses the chat panel's own input, wired via `open_rewrite_instruction_input` + `submit_panel_rewrite`.)
- Testing (spec) → unit in Tasks 2–3, headless in Task 7. ✓
- Error/cancel handling (spec) → Task 5 steps 3, 4, 7. ✓

**Placeholder scan:** No TBD/TODO; every code step shows the code. The one deliberate "match the existing import path" note (RewriteTarget) is a verification instruction with a concrete grep, not a placeholder.

**Type consistency:** `journal_cursor: usize`, `rewrite_return: bool`, `finish_panel_rewrite(&mut AppState, Option<i64>)`, `rewrite_journal_entry(&Rc<RefCell<AppState>>)`, `open_rewrite_instruction_input(&mut AppState)`, `submit_panel_rewrite(&Rc<RefCell<AppState>>)`, `journal_entry_qrow(usize)->usize`, `clamp_journal_cursor(usize,usize)->usize`, `step_journal_cursor(usize,i32,usize)->usize` — names used consistently across tasks. `render_journal_view` signature change to `&mut AppState` is called out in Task 2 step 5 and used by Task 3.

**Known risk to watch during execution:** the panel rewrite routing lives in the `submit`/`close` closures passed to `ask_vim_intercept` inside `handle_chat_prompt_key` (keymap.rs ~1383), NOT in separate key arms — Task 5 step 7c replaces that whole call block. The `RewriteTarget` path is `crate::input::actions::journal::RewriteTarget` (verified line 41). `rewrite_with_claude` must be promoted to `pub(crate)`. When re-seeding the overlay page state for `R`, `open_rewrite_target` reads `displayed_journal_page` which reads `s.journal.pages[page_index]` — the seed in Task 5 step 1 sets both, so the popup's "Nothing to rewrite" guard passes.
