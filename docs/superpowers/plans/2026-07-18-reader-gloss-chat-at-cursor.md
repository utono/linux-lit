# Reader-gloss chat from the main card (`-` at cursor) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make plain `-` in reader mode open the chat panel pinned to the reader-gloss covering the cursor line, eliminating the `V`-select step.

**Architecture:** Reuse the existing visual-mode `-` machinery unchanged. A new reader-mode handler resolves the cursor line to the reader-gloss passage that covers it, stages a transient `SelectionState` over that passage's buffer-line span (exactly as `enter_visual_block_mode` does), then calls the existing `action_reader_gloss_chat`, which reads the selection, builds the gloss context, pins the panel, exits visual mode (clearing the transient selection), and shows the stored gloss. No refactor of the pinning/cache/exchange code.

**Tech Stack:** Rust, GTK4, SQLite (rusqlite). Reader is `src/input/`.

## Global Constraints

- Glosses are keyed by `Work.canonical_abbrev` — every gloss DB lookup MUST use it (the `-BBC`/`-Amb` lookup-mismatch bug class). Copied verbatim from the spec.
- **reader-gloss only** — filter `find_glossed_passages` to `&["reader-gloss"]`, NOT the 3-type set.
- **Full passage span** — pin the gloss's authored `[start_citation, end_citation]`, never the single cursor line.
- **No-gloss → toast + no-op** — `No gloss on this line`, stay in reader.
- Plain `minus` is currently UNBOUND in the main card (asserted `None` at `src/input/keymap_config.rs:512`) — no conflict.
- `keymap.json` override shadows compiled defaults: any bind added to `keymap_config.rs` MUST also be added to `~/.config/linux-lit/keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`).
- Every main-card keybind change updates the Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`).
- Build only — never `cargo run`; the user launches the app.

---

## File Structure

- `src/input/actions/mod.rs` — add `Action::ReaderGlossChatAtCursor`.
- `src/input/actions/gloss.rs` — new `reader_gloss_passage_at_cursor(state) -> Option<(usize, usize)>` returning the inclusive buffer-line span of the reader-gloss covering the cursor (or `None`). Reuses `passage_covers` + `find_glossed_passages` (already in this file).
- `src/input/visual.rs` — new `reader_gloss_chat_at_cursor(state_rc)`: resolves the span via the gloss.rs helper, stages the transient selection, calls `action_reader_gloss_chat`. Lives beside `action_reader_gloss_chat`.
- `src/input/keymap_config.rs` — bind `plain("minus")` → `ReaderGlossChatAtCursor`; fix the freed-key comment + the `None` assertion.
- `src/input/keymap.rs` — dispatch `ReaderGlossChatAtCursor` → `reader_gloss_chat_at_cursor`.
- `src/ui/keybinds_overlay.rs` — Ctrl+/ overlay keycap + `describe()`.
- `~/.config/linux-lit/keymap.json` (via `~/tty-dotfiles/linux-lit/`) — same bind.

---

### Task 1: Resolve the reader-gloss passage span at the cursor

**Files:**
- Modify: `src/input/actions/gloss.rs` (add function near `open_gloss_at_cursor`, ~line 2903, and `passage_covers` at 2757)
- Test: `src/input/actions/gloss.rs` (inline `#[cfg(test)]` — this file already has DB-backed tests; add a pure-logic unit test for the citation→span mapping helper only if a seam exists, otherwise the behavior is covered by the Task 6 headless test)

**Interfaces:**
- Produces: `pub(crate) fn reader_gloss_passage_at_cursor(s: &AppState) -> Option<(usize, usize)>` — inclusive `(start_buf, end_buf)` buffer-line range of the reader-gloss covering `s.current_line`, or `None` if the cursor line has no covering reader-gloss / no work / no DB.
- Consumes: `passage_covers` (private, same file), `crate::app::parse_citation`, `crate::db::queries::find_glossed_passages`, `AppState::work_line_for_buffer`, `line_map.work_to_buffer`.

- [ ] **Step 1: Read the reference resolver**

Read `open_gloss_at_cursor` (`src/input/actions/gloss.rs:2903`) and `jump_to_gloss_source_start` (`:20`). The new helper mirrors `open_gloss_at_cursor`'s cursor→passage resolution (steps: cursor buffer line → work line → `(div1,div2,line_in_div)` under `canonical_abbrev`; `find_glossed_passages`; `passage_covers`) but filtered to `reader-gloss`, and then maps the passage's `[start_citation,end_citation]` to a **buffer-line span** the way `jump_to_gloss_source_start` maps one citation to a buffer index (`work.lines.position(|l| tuple == t)` then `line_map.work_to_buffer`).

- [ ] **Step 2: Write the helper**

Add near `passage_covers` (`src/input/actions/gloss.rs:2759`):

```rust
/// Inclusive `(start_buf, end_buf)` buffer-line span of the reader-gloss
/// passage covering the cursor line, or `None` when the cursor line has no
/// covering reader-gloss (or no work / DB). reader-gloss ONLY — the chat
/// panel's gloss flow is the reader-gloss flow. Used by the reader-mode `-`
/// bind (`reader_gloss_chat_at_cursor`) to stage a transient selection over
/// the gloss's authored passage without the user entering visual mode.
pub(crate) fn reader_gloss_passage_at_cursor(s: &AppState) -> Option<(usize, usize)> {
    let work = s.current_work.as_ref()?;
    // Glosses are keyed by canonical_abbrev (the -BBC/-Amb lookup rule).
    let abbrev = work.canonical_abbrev.clone();
    let wl = s.work_line_for_buffer(s.current_line)?;
    let line = work.lines.get(wl)?;
    let cur = (line.div1, line.div2, line.line_in_div);

    let conn = crate::db::queries::open_db().ok()?;
    let passages =
        crate::db::queries::find_glossed_passages(&conn, &abbrev, &["reader-gloss"])
            .unwrap_or_default();

    let passage = passages.into_iter().find(|p| {
        match (
            crate::app::parse_citation(&p.start_citation),
            crate::app::parse_citation(&p.end_citation),
        ) {
            (Some(start), Some(end)) => passage_covers(start, end, cur),
            _ => false,
        }
    })?;

    // Map the passage's start/end citations to work-line indices, then to
    // buffer lines through the line map (jump_to_gloss_source_start's pattern).
    let start_t = crate::app::parse_citation(&passage.start_citation)?;
    let end_t = crate::app::parse_citation(&passage.end_citation)?;
    let start_wi = work
        .lines
        .iter()
        .position(|l| (l.div1, l.div2, l.line_in_div) == start_t)?;
    let end_wi = work
        .lines
        .iter()
        .position(|l| (l.div1, l.div2, l.line_in_div) == end_t)?;

    let to_buf = |wi: usize| -> Option<usize> {
        if let Some(ref lm) = s.line_map {
            lm.work_to_buffer.get(wi).copied()
        } else {
            Some(wi)
        }
    };
    let a = to_buf(start_wi)?;
    let b = to_buf(end_wi)?;
    Some((a.min(b), a.max(b)))
}
```

- [ ] **Step 3: Confirm it compiles**

Run: `cargo build 2>&1 | rg -n "error|warning: unused" | rg reader_gloss_passage_at_cursor`
Expected: no errors referencing the new function (a dead-code warning until Task 3 wires it is acceptable).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): resolve reader-gloss passage span at cursor

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VnfsDeCWYrvJPW4tGMiYJZ"
```

---

### Task 2: Reader-mode handler that stages a selection and reuses `action_reader_gloss_chat`

**Files:**
- Modify: `src/input/visual.rs` (add beside `action_reader_gloss_chat`, ~line 741)
- Test: covered by Task 6 headless drive (this handler mutates GTK/DB state; no pure seam).

**Interfaces:**
- Produces: `pub(crate) fn reader_gloss_chat_at_cursor(state_rc: &Rc<RefCell<AppState>>)`.
- Consumes: `crate::input::actions::gloss::reader_gloss_passage_at_cursor` (Task 1); `SelectionState`; `action_reader_gloss_chat` (same file); `show_chapter_toast_secs`.

- [ ] **Step 1: Write the handler**

Add after `action_reader_gloss_chat` (ends `src/input/visual.rs:741`):

```rust
/// Reader-mode `-`: open the chat panel pinned to the reader-gloss covering
/// the cursor line and show the stored gloss — the same end state as
/// visual-mode `-`, WITHOUT the `V`-select step. No-op (toast) when no
/// reader-gloss covers the cursor line.
///
/// Reuses `action_reader_gloss_chat` verbatim by staging a transient
/// `SelectionState` over the gloss's authored passage span (the
/// `enter_visual_block_mode` pattern). `action_reader_gloss_chat` ->
/// `open_chat_pinned_to_selection` reads that selection, pins, then
/// `exit_visual_mode` clears it — so the transient selection never outlives
/// this call.
pub(crate) fn reader_gloss_chat_at_cursor(
    state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>,
) {
    let span = crate::input::actions::gloss::reader_gloss_passage_at_cursor(&state_rc.borrow());
    let Some((start, end)) = span else {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No gloss on this line", 2);
        return;
    };
    {
        let mut s = state_rc.borrow_mut();
        s.visual_selection = Some(SelectionState {
            anchor_line: start,
            cursor_line: end,
            pending_ask: false,
        });
        s.input_mode = crate::app::InputMode::Visual;
    }
    // Reads the staged selection, builds the reader-gloss context, pins the
    // panel, exits visual mode (clearing the selection), shows the cached gloss.
    action_reader_gloss_chat(state_rc);
}
```

- [ ] **Step 2: Verify `action_reader_gloss_chat`'s early-return leaves no dangling visual mode**

Read `action_reader_gloss_chat` (`src/input/visual.rs:667`). Confirm: if it early-returns BEFORE `open_chat_pinned_to_selection` (e.g. `build_context_for_type` returns `None`), the staged `visual_selection` + `InputMode::Visual` would be left set. `reader_gloss_passage_at_cursor` only returns a span when a covering reader-gloss EXISTS, so `build_context_for_type(work, &lines, "reader-gloss")` over that exact span should succeed — but to be fail-safe, confirm by reading whether `build_context_for_type` can return `None` for a non-empty in-range passage. If it can, note it for Step 3; otherwise no guard is needed.

- [ ] **Step 3: Add a fail-safe only if Step 2 found a gap**

If Step 2 showed `action_reader_gloss_chat` can early-return with the selection staged, wrap the call so a failure restores reader mode:

```rust
    let opened_before = state_rc.borrow().chat_layout_open;
    action_reader_gloss_chat(state_rc);
    // If the panel didn't open AND we're still in the staged visual mode, the
    // ctx build failed — clear the transient selection so the reader isn't
    // stranded in visual mode.
    let mut s = state_rc.borrow_mut();
    if !opened_before
        && !s.chat_layout_open
        && s.input_mode == crate::app::InputMode::Visual
    {
        exit_visual_mode(&mut s);
    }
```

(If Step 2 showed the ctx build always succeeds for a real passage, skip this step — YAGNI.)

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | rg -c "^error"`
Expected: `0`

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat(chat): reader_gloss_chat_at_cursor stages selection, reuses '-' flow

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VnfsDeCWYrvJPW4tGMiYJZ"
```

---

### Task 3: Add the Action variant, keybind, and dispatch

**Files:**
- Modify: `src/input/actions/mod.rs` (Action enum)
- Modify: `src/input/keymap_config.rs` (bind + assertion at :512)
- Modify: `src/input/keymap.rs` (dispatch arm, near the other chat dispatch at :3699)

**Interfaces:**
- Produces: `Action::ReaderGlossChatAtCursor`.
- Consumes: `reader_gloss_chat_at_cursor` (Task 2).

- [ ] **Step 1: Add the Action variant**

In `src/input/actions/mod.rs`, add to the `Action` enum near the other chat actions (`ToggleChatLayout`, `CloseChatLayout`):

```rust
    /// Reader-mode `-`: open the chat panel pinned to the reader-gloss covering
    /// the cursor line (no `V` needed). No-op toast if the line isn't glossed.
    ReaderGlossChatAtCursor,
```

If the enum has a string-name mapping used by `keymap.json` (grep for `CloseChatLayout` in the loader), add `"ReaderGlossChatAtCursor" => Action::ReaderGlossChatAtCursor` there too.

Run: `rg -n 'CloseChatLayout' src/input/keymap_config.rs src/input/actions/mod.rs` to find whether a name-string arm exists and where.

- [ ] **Step 2: Bind plain `minus` and fix the assertion**

In `src/input/keymap_config.rs`, near the minus binds (`:438`–`:446`), add:

```rust
        // Plain `-` opens the chat panel on the reader-gloss covering the
        // cursor line (reader mode; no V-select). Ctrl+- / Ctrl+Shift+- keep
        // their vocab-jump binds below.
        (KeyCombo::plain("minus"), Action::ReaderGlossChatAtCursor),
```

Then update the assertion at `:512` from:

```rust
        assert_eq!(m.get(&KeyCombo::plain("minus")), None);
```
to:
```rust
        assert_eq!(
            m.get(&KeyCombo::plain("minus")),
            Some(&Action::ReaderGlossChatAtCursor)
        );
```

Update the nearby freed-key comment (`:510`) so it no longer claims plain minus is free.

- [ ] **Step 3: Dispatch**

In `src/input/keymap.rs`, near `ToggleChatLayout =>` (`:3699`), add:

```rust
        ReaderGlossChatAtCursor => {
            crate::input::visual::reader_gloss_chat_at_cursor(state)
        }
```

(`state` here is the `&Rc<RefCell<AppState>>`; match the surrounding arms' exact receiver — check whether siblings pass `state` or `&state`.)

- [ ] **Step 4: Confirm no reader-mode `"minus"` arm intercepts before dispatch**

Reader-mode keys route through the dispatch table, but some keys are special-cased in `handle_key`/`handle_reader_key` before dispatch. Confirm plain `minus` (no ctrl) is NOT caught earlier in reader context:

Run: `rg -n '"minus"' src/input/keymap.rs`
Expected: the only reader-relevant `"minus"` arms are `Ctrl`-gated (`:3234` is `if is_ctrl`) or in visual mode (`:1356`, `:3366`). If a plain reader-mode `"minus"` arm exists, it must fall through to dispatch — fix if not.

- [ ] **Step 5: Build + keymap test**

Run: `cargo build 2>&1 | rg -c "^error"`
Expected: `0`

Run: `cargo test --bins keymap 2>&1 | tail -20`
Expected: PASS, including the updated `plain("minus")` assertion.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs
git commit -m "feat(keymap): bind reader-mode '-' to ReaderGlossChatAtCursor

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VnfsDeCWYrvJPW4tGMiYJZ"
```

---

### Task 4: Mirror the bind in keymap.json (stow)

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (source of the stow symlink; confirm exact path with `readlink ~/.config/linux-lit/keymap.json`)

- [ ] **Step 1: Locate the real file behind the symlink**

Run: `readlink -f ~/.config/linux-lit/keymap.json`
Expected: a path under `~/tty-dotfiles/linux-lit/`. Edit THAT file, not the symlink target directly, so the change is version-controlled.

- [ ] **Step 2: Check whether plain `-` is already present**

Run: `rg -n 'minus|"-"|ReaderGlossChatAtCursor' "$(readlink -f ~/.config/linux-lit/keymap.json)"`
Expected: no existing plain-minus entry (Ctrl+minus vocab entries may be present — leave them).

- [ ] **Step 3: Add the entry**

Add to the bindings array (match the file's existing key spelling — confirm whether it uses `"minus"` or `"-"` by inspecting sibling entries):

```json
    { "key": "minus", "action": "ReaderGlossChatAtCursor" }
```

- [ ] **Step 4: Validate JSON + redeploy stow**

Run:
```bash
jq empty "$(readlink -f ~/.config/linux-lit/keymap.json)" && echo "json ok"
cd ~/tty-dotfiles && stow linux-lit
```
Expected: `json ok`, stow reports no conflicts (symlink already in place).

- [ ] **Step 5: Commit the dotfiles repo**

```bash
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json \
  && git commit -m "linux-lit: bind reader-mode '-' to ReaderGlossChatAtCursor"
```

(Separate repo from linux-lit — commit there.)

---

### Task 5: Update the Ctrl+/ keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (keycap strip const + `describe()` arm)

- [ ] **Step 1: Invoke the overlay skill**

This project has a dedicated skill for overlay edits with a three-pass cross-reference. Invoke it: `update-cairo-keybinds-overlay`. Follow its passes to add the `-` keycap and a `describe()` detail arm reading roughly:

> `-` — open the chat panel on the reader-gloss covering the cursor line (reader mode). No-op if the line has no reader-gloss. (In visual `V` mode, `-` glosses the selection instead.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg -c "^error"`
Expected: `0`

- [ ] **Step 3: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): add reader-mode '-' reader-gloss chat to Ctrl+/ legend

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VnfsDeCWYrvJPW4tGMiYJZ"
```

---

### Task 6: Headless verification

**Files:** none (test drive only)

- [ ] **Step 1: Build**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: `Finished`.

- [ ] **Step 2: Identify a work with a known reader-gloss and its glossed lines**

Run (read-only, find a reader-glossed passage + its citation span):
```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT work_abbrev, start_citation, end_citation FROM glosses WHERE gloss_type='reader-gloss' LIMIT 5;"
```
Pick one (note its work abbrev + start line). Confirm the exact `glosses` column/table names first with `.schema glosses` if the query errors.

- [ ] **Step 3: Drive it headless**

Use the `test-headless-navigation` skill's cage/grim/wtype flow. Launch with `LIT_DEV=1` (loads `config-dev.json`), navigate to the glossed work, move the cursor onto a glossed line (the reader-gloss tint marks it), press `-`, screenshot.

Expected: the chat panel opens beside the card showing the stored reader-gloss text; focus is in the transcript (not an empty ask input); no `Glossing…` spinner lingers (cache hit).

- [ ] **Step 4: Drive the no-gloss case**

Move the cursor to an un-tinted (unglossed) line, press `-`, screenshot.
Expected: a bottom-center `No gloss on this line` toast; NO panel opens; still in reader mode.

- [ ] **Step 5: Open every PNG and report inline**

Per the UI review protocol: open each `target/ui/*.png`, quote the on-screen gloss text, confirm the panel/toast by eye. A passing exit code is not enough.

- [ ] **Step 6: Cleanup**

Run: `pkill -f "cage -- ./target/debug/linux-lit"`
(Scoped — never a bare `pkill -f target/debug/linux-lit`.)

---

### Task 7: Final hand-off for on-screen eyeball

**Files:** none

- [ ] **Step 1: Provide the user the exact e2e command** for a final look on the real GL renderer (cage is software rendering), and the manual steps: open a reader-glossed work, put the cursor on a glossed line, press `-`, confirm the chat panel shows the gloss; press `-` on an unglossed line, confirm the toast.

- [ ] **Step 2: Finish the branch** per the project convention (merge `--no-ff` to master, rebuild, push, delete branch) once the user confirms the on-screen result — do NOT auto-merge before that confirmation.

---

## Notes for the implementer

- **`SelectionState` is the type**, exported as `crate::input::visual::SelectionState` (aliased as `s.visual_selection: Option<crate::input::visual::SelectionState>`). Field names: `anchor_line`, `cursor_line`, `pending_ask`.
- **`action_reader_gloss_chat` already handles the cache hit vs fresh-gloss branch** (`visual.rs:727`–`740`) — the cursor path inherits it for free; do not re-implement.
- **Do not touch visual-mode `-`** — it keeps building its ctx from the selection and calling `action_reader_gloss_chat`.
- **`parse_citation`** returns `Option<(i64,i64,i64)>` from `ABBR.div1.div2.line_in_div`, stripping the abbrev — see `jump_to_gloss_source_start` (`gloss.rs:29`).
