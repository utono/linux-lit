# Chat Panel `D` Delete Bind Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `D` in the chat panel deletes the currently shown gloss (Gloss view) or the journal entry under the cursor (Journal view), via the overlays' existing y/Esc confirmation modal.

**Architecture:** The bind reuses the existing `DeleteConfirm` modal machinery with `InputMode::ChatTranscript` as a new dispatch origin. The gloss DB+audio purge is extracted from the overlay's `delete_current_gloss` into one shared helper. Panel-side handlers mutate the panel's own lists, reconcile the two overlay caches, clean dangling `saved_id`/`revision_of` references, and re-render in place.

**Tech Stack:** Rust, GTK4, existing modal/confirm and render plumbing.

**Spec:** `docs/superpowers/specs/2026-07-20-chat-panel-delete-bind-design.md`

**Branch:** stacked directly on `feat/journal-entry-top-landing` (user's choice) — no new branch; commit on the current checkout.

## Global Constraints

- `D` is view-gated: Gloss and Journal views only; Question view stays a no-op.
- Confirmation is mandatory: `D` never deletes directly — always through `show_delete_confirmation` → `y`.
- The gloss purge helper must be behavior-preserving for the overlay's existing delete (same DB calls, same mp3 counting, same toast numbers).
- Every chat-panel bind change updates `src/ui/chat_keybinds_overlay.rs` `GROUPS` in the same change (standing user rule).
- `cargo build` only — NEVER `cargo run`. Commit after each task.

---

### Task 1: Pure helper `clear_deleted_journal_refs` (TDD)

**Files:**
- Modify: `src/input/actions/chat_rows.rs` (helper + tests; put the helper near `clamp_journal_cursor`, tests in a new `#[cfg(test)] mod delete_refs_tests` after the existing test modules)

**Interfaces:**
- Produces: `pub(crate) fn clear_deleted_journal_refs(exchanges: &mut [Exchange], revision_of: Option<i64>, deleted: i64) -> Option<i64>` — clears any `saved_id == Some(deleted)` in place and returns the new `revision_of` (None if it pointed at `deleted`, unchanged otherwise). Task 3 calls it.

- [ ] **Step 1: Write the failing tests**

Append to `src/input/actions/chat_rows.rs`:

```rust
/// Deleting a journal row from the chat panel must not leave dangling
/// references: an exchange saved to that row regains `saved_id: None` (so
/// `s` can re-save it and the SavedMark disappears on the next render), and
/// a pending `revision_of` aimed at the deleted row is cleared so Ctrl+Enter
/// cannot retarget a nonexistent entry.
#[cfg(test)]
mod delete_refs_tests {
    use super::{clear_deleted_journal_refs, Exchange};

    fn ex(saved_id: Option<i64>) -> Exchange {
        Exchange {
            question: "q".to_string(),
            answer: "a".to_string(),
            chip: String::new(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id,
        }
    }

    #[test]
    fn clears_matching_saved_id_only() {
        let mut exchanges = vec![ex(Some(45)), ex(Some(46)), ex(None)];
        let rev = clear_deleted_journal_refs(&mut exchanges, None, 45);
        assert_eq!(exchanges[0].saved_id, None);
        assert_eq!(exchanges[1].saved_id, Some(46));
        assert_eq!(exchanges[2].saved_id, None);
        assert_eq!(rev, None);
    }

    #[test]
    fn clears_revision_of_pointing_at_deleted() {
        let mut exchanges = vec![ex(Some(45))];
        assert_eq!(clear_deleted_journal_refs(&mut exchanges, Some(45), 45), None);
    }

    #[test]
    fn keeps_revision_of_pointing_elsewhere() {
        let mut exchanges = vec![ex(None)];
        assert_eq!(clear_deleted_journal_refs(&mut exchanges, Some(46), 45), Some(46));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --bin linux-lit delete_refs -- --nocapture
```

Expected: FAIL to compile — `clear_deleted_journal_refs` not defined.

- [ ] **Step 3: Implement**

Add to `src/input/actions/chat_rows.rs`, directly after `clamp_journal_cursor`:

```rust
/// Clean up in-memory references to a just-deleted journal row: clear any
/// exchange's `saved_id` that pointed at it (the exchange becomes re-savable
/// and its SavedMark disappears on the next render) and return the new
/// `revision_of` (cleared iff it pointed at the deleted row). Pure so the
/// dangling-reference contract is unit-testable without an `AppState`.
pub(crate) fn clear_deleted_journal_refs(
    exchanges: &mut [Exchange],
    revision_of: Option<i64>,
    deleted: i64,
) -> Option<i64> {
    for ex in exchanges.iter_mut() {
        if ex.saved_id == Some(deleted) {
            ex.saved_id = None;
        }
    }
    if revision_of == Some(deleted) { None } else { revision_of }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --bin linux-lit delete_refs -- --nocapture
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/chat_rows.rs
git commit -m "feat(chat): pure dangling-ref cleanup for journal deletes"
```

---

### Task 2: Extract the shared gloss purge helper

**Files:**
- Modify: `src/input/actions/gloss.rs` (`delete_current_gloss` at ~348-412; new `purge_gloss_data` beside it)

**Interfaces:**
- Consumes: existing `delete_gloss`, `delete_gloss_audio` (db/queries.rs), `gloss_audio_dir(work_abbrev, gloss_id)` (gloss.rs:2699, private to gloss.rs — the helper lives in gloss.rs so it stays private).
- Produces: `pub(crate) fn purge_gloss_data(work_abbrev: Option<&str>, gloss_id: i64) -> (usize, usize)` returning `(audio_rows, mp3_files)`. Task 3 calls it from chat.rs.

- [ ] **Step 1: Add the helper and rewire the overlay delete**

Add above `delete_current_gloss`:

```rust
/// Delete a gloss row plus its cached TTS audio: the DB row (`delete_gloss`),
/// its audio rows (`delete_gloss_audio`), and the on-disk mp3 dir. Returns
/// `(audio_rows, mp3_files)` for the caller's verification toast. Shared by
/// the gloss overlay's `D` and the chat panel's `D` so the two purge paths
/// cannot drift. `work_abbrev: None` skips the on-disk dir (no context to
/// locate it) — the DB purge still runs.
pub(crate) fn purge_gloss_data(work_abbrev: Option<&str>, gloss_id: i64) -> (usize, usize) {
    let mut audio_rows = 0usize;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::delete_gloss(&conn, gloss_id);
        audio_rows = crate::db::queries::delete_gloss_audio(&conn, gloss_id).unwrap_or(0);
    }
    let mut mp3_files = 0usize;
    if let Some(abbrev) = work_abbrev {
        let dir = gloss_audio_dir(abbrev, gloss_id);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            mp3_files = entries
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("mp3"))
                .count();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    (audio_rows, mp3_files)
}
```

In `delete_current_gloss`, replace the inline block (the `let mut audio_rows … std::fs::remove_dir_all(&dir);` span, currently gloss.rs:359-376) with:

```rust
        let abbrev = s.gloss_context.as_ref().map(|c| c.work_abbrev.clone());
        let (audio_rows, mp3_files) = purge_gloss_data(abbrev.as_deref(), gloss_id);
```

The log line, toast format, list removal, and everything after stay byte-identical.

- [ ] **Step 2: Build + full suite**

```bash
cargo build && cargo test --bin linux-lit
```

Expected: green (behavior-preserving refactor; existing delete tests unaffected).

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "refactor(gloss): extract shared purge_gloss_data helper"
```

---

### Task 3: Panel delete handlers in chat.rs

**Files:**
- Modify: `src/input/actions/chat.rs` (new functions near `copy_journal_id`/`cycle_gloss`; add `clear_deleted_journal_refs` to the `use super::chat_rows::{...}` import list)

**Interfaces:**
- Consumes: Task 1's `clear_deleted_journal_refs`, Task 2's `purge_gloss_data`, existing `clamp_journal_cursor`, `push_gloss_exchange`, `render_transcript`, `render_journal_view_inner(s, snap_to_entry)`, `crate::db::journal::delete_journal_page`, `crate::input::actions::journal::purge_journal_audio`, `crate::app::apply_reader_gloss_highlighting`.
- Produces: `pub(crate) fn delete_current_panel_item(state: &Rc<RefCell<AppState>>)` — Task 4's `y`-confirm dispatch target.

- [ ] **Step 1: Implement the three functions**

Add to `src/input/actions/chat.rs` (after `copy_journal_id`):

```rust
/// `y`-confirmed chat-panel delete (the panel's `D`, via the overlays' shared
/// DeleteConfirm modal with origin ChatTranscript): deletes what the active
/// view displays. Question view is unreachable — `show_delete_confirmation`
/// refuses to open the dialog there — the arm exists for the match only.
pub(crate) fn delete_current_panel_item(state: &Rc<RefCell<AppState>>) {
    let view = state.borrow().chat.view;
    match view {
        PanelView::Gloss => delete_panel_gloss(state),
        PanelView::Journal => delete_panel_journal_entry(state),
        PanelView::Question => {}
    }
}

/// Delete the panel's currently shown gloss: shared DB+audio purge, then
/// panel bookkeeping (list/index), overlay-cache reconciliation, transcript
/// re-render (next gloss in place, or the empty placeholder), and the
/// reader-tint recompute the overlay's own delete performs.
fn delete_panel_gloss(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let idx = s.chat.gloss_index;
    let Some(g) = s.chat.gloss_list.get(idx) else { return };
    let gloss_id = g.gloss_id;
    let abbrev = s.chat.gloss_ctx.as_ref().map(|c| c.work_abbrev.clone());
    let (_audio_rows, mp3_files) =
        crate::input::actions::gloss::purge_gloss_data(abbrev.as_deref(), gloss_id);

    s.chat.gloss_list.remove(idx);
    // Reconcile the gloss OVERLAY's separate cache (AppState.gloss_list is a
    // distinct Vec from the panel's) so the deleted row cannot resurface when
    // the overlay renders its remembered list.
    if let Some(pos) = s.gloss_list.iter().position(|og| og.gloss_id == gloss_id) {
        s.gloss_list.remove(pos);
        s.gloss_index = if s.gloss_list.is_empty() {
            0
        } else {
            s.gloss_index.min(s.gloss_list.len() - 1)
        };
    }

    if s.chat.gloss_list.is_empty() {
        // Last gloss gone: placeholder in transcript slot #1, stay in Gloss
        // view (spec decision). The empty chip renders no row; the plain text
        // renders as one label via gloss_answer_specs's no-tags fallback.
        s.chat.gloss_index = 0;
        if let Some(ex) = s.chat.exchanges.get_mut(0) {
            if ex.question.is_empty() {
                ex.answer = "No glosses for this passage".to_string();
                ex.chip = String::new();
            }
        }
        s.chat.view = PanelView::Gloss;
        render_transcript(&mut s);
    } else {
        // Show the next remaining gloss in place (same replace-in-slot path
        // Ctrl+n/p uses; it renders the transcript itself).
        s.chat.gloss_index = idx.min(s.chat.gloss_list.len() - 1);
        let text = s.chat.gloss_list[s.chat.gloss_index].gloss_text.clone();
        if let Some(ctx) = s.chat.gloss_ctx.clone() {
            push_gloss_exchange(&mut s, &ctx, &text);
        }
    }

    // The glossed-passage set changed — recompute the main-card tint, same as
    // the overlay's delete (otherwise the deleted passage's lines stay tinted).
    crate::app::apply_reader_gloss_highlighting(&mut s);
    crate::logging::log(&format!(
        "CHAT: deleted gloss {} ({} mp3 files)", gloss_id, mp3_files
    ));
    crate::input::navigation::show_chapter_toast_secs(
        &s,
        &format!(
            "Deleted gloss {} · {} mp3{}",
            gloss_id, mp3_files, if mp3_files == 1 { "" } else { "s" }
        ),
        2,
    );
}

/// Delete the journal entry under the panel cursor: DB row + cached TTS
/// audio, panel list/cursor bookkeeping, dangling saved_id/revision_of
/// cleanup, journal-overlay cache reconciliation, and a snapped re-render
/// (the empty case renders the existing placeholder row).
fn delete_panel_journal_entry(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let idx = s.chat.journal_cursor;
    let Some(id) = s.chat.journal_list.get(idx).map(|p| p.id) else { return };
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::journal::delete_journal_page(&conn, id);
        crate::input::actions::journal::purge_journal_audio(&conn, id);
    }
    s.chat.journal_list.remove(idx);
    s.chat.journal_cursor = clamp_journal_cursor(idx, s.chat.journal_list.len());
    let rev = clear_deleted_journal_refs(&mut s.chat.exchanges, s.chat.revision_of, id);
    s.chat.revision_of = rev;
    // Reconcile the journal OVERLAY's cache (s.journal.pages is a third
    // independent copy) so a stale entry cannot render there.
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == id) {
        s.journal.pages.remove(pos);
        if s.journal.page_index >= s.journal.pages.len() {
            s.journal.page_index = s.journal.pages.len().saturating_sub(1);
        }
    }
    render_journal_view_inner(&mut s, true);
    crate::logging::log(&format!("CHAT: deleted journal {}", id));
    crate::input::navigation::show_chapter_toast_secs(
        &s, &format!("Deleted journal {}", id), 2,
    );
}
```

Add `clear_deleted_journal_refs` to the `use super::chat_rows::{...}` list at the top of chat.rs.

- [ ] **Step 2: Build + full suite**

```bash
cargo build && cargo test --bin linux-lit
```

Expected: green. (The new functions are not yet reachable — wiring lands in Task 4; if dead-code warnings appear for them, that is expected at this intermediate step and resolves in Task 4. If the build errors on visibility of `render_journal_view_inner`, make it `pub(crate)` — it is already module-internal to chat.rs, so no change should be needed.)

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "feat(chat): panel gloss/journal delete handlers"
```

---

### Task 4: Wire the modal, the `D` bind, and the legend

**Files:**
- Modify: `src/input/actions/gloss.rs` (`show_delete_confirmation` origin match, ~lines 425-437)
- Modify: `src/input/keymap.rs` (`handle_delete_confirm_key` ~3040-3066; `handle_chat_transcript_key` — add the `D` arm after the `"c"` arm at ~1628-1635)
- Modify: `src/ui/chat_keybinds_overlay.rs` (`GROUPS` "Transcript actions", lines 25-33)

**Interfaces:**
- Consumes: Task 3's `delete_current_panel_item`.
- Produces: the working end-to-end `D` flow.

- [ ] **Step 1: Title arm in `show_delete_confirmation`**

In the `match origin` block (gloss.rs:425-437), add before the `_ => return,` arm:

```rust
            crate::app::InputMode::ChatTranscript => {
                use crate::input::actions::chat::PanelView;
                match s.chat.view {
                    PanelView::Gloss => match s.chat.gloss_list.get(s.chat.gloss_index) {
                        Some(g) => format!("Delete gloss {}?", g.gloss_id),
                        None => return,
                    },
                    PanelView::Journal => {
                        match s.chat.journal_list.get(s.chat.journal_cursor) {
                            Some(p) => format!("Delete journal {}?", p.id),
                            None => return,
                        }
                    }
                    // No deletable item is displayed in Question view — the
                    // dialog never opens there (the panel's D is view-gated
                    // here, not at the bind).
                    PanelView::Question => return,
                }
            }
```

Also update the function's doc comment (gloss.rs:414-417): "Records `origin` (gloss vs journal)" → "Records `origin` (gloss overlay, journal overlay, or chat transcript)".

- [ ] **Step 2: Dispatch arm in `handle_delete_confirm_key`**

In keymap.rs (~3050), extend the `match origin`:

```rust
            match origin {
                Some(crate::app::InputMode::JournalOverlay) => {
                    crate::input::actions::journal::delete_current(state);
                }
                Some(crate::app::InputMode::ChatTranscript) => {
                    crate::input::actions::chat::delete_current_panel_item(state);
                }
                _ => {
                    crate::input::actions::gloss::delete_current_gloss(state);
                }
            }
```

- [ ] **Step 3: `D` bind in `handle_chat_transcript_key`**

After the `"c"` arm (keymap.rs ~1635), add:

```rust
        // `D`: delete the displayed item — Gloss view: the current gloss;
        // Journal view: the selected saved Q&A — via the overlays' shared
        // y/Esc confirmation modal. Origin ChatTranscript routes `y` to
        // chat::delete_current_panel_item and Esc back to this mode. In
        // Question view show_delete_confirmation refuses to open (no
        // deletable item is displayed), so this is a no-op there.
        "D" => {
            crate::input::actions::gloss::show_delete_confirmation(
                state,
                crate::app::InputMode::ChatTranscript,
            );
            true
        }
```

- [ ] **Step 4: Legend entry (standing rule: same change as the bind)**

In `chat_keybinds_overlay.rs` "Transcript actions" (after the `("c", …)` row at line 30):

```rust
        ("D", "delete: Gloss view → current gloss · Journal view → Q&A (y/Esc confirm)"),
```

- [ ] **Step 5: Build + clippy + full suite**

```bash
cargo build && cargo clippy && cargo test --bin linux-lit
```

Expected: build green, no NEW clippy warnings, all tests pass (994 expected: 991 + Task 1's 3).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/gloss.rs src/input/keymap.rs src/ui/chat_keybinds_overlay.rs
git commit -m "feat(chat): D deletes current gloss/journal entry with confirm"
```

---

### Task 5: Verification and finish

**Files:** none (verification only)

- [ ] **Step 1: Full suite + clippy**

```bash
cargo test --bin linux-lit && cargo clippy
```

Expected: all green, no new warnings.

- [ ] **Step 2: Testing gate (REQUIRED before calling this done)**

This branch is stacked on `feat/journal-entry-top-landing`, which is itself awaiting the user's manual test. Offer the user one combined manual pass (or headless, their choice) covering both features. D-bind checklist: `D` on a gloss → confirm dialog names the id → `y` deletes, next gloss appears, reader tint updates, gloss overlay shows no stale entry; `D` on the LAST gloss → placeholder "No glosses for this passage", still Gloss view; `D` in Journal view → entry gone, cursor on neighbor, SavedMark cleared if that exchange was saved, `s` re-saves; `Escape`/`n` cancels; `D` in Question view does nothing; Ctrl+/ legend shows the `D` row.

- [ ] **Step 3: Finish (after testing passes)**

Merge `feat/journal-entry-top-landing` (now carrying both features) to master per the house flow:

```bash
git checkout master && git merge --no-ff feat/journal-entry-top-landing
cargo build && cargo test --bin linux-lit
git push origin master && git branch -d feat/journal-entry-top-landing
```
