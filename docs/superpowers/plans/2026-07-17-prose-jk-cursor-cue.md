# Prose j/k Cursor Cue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `j` / `k` on a prose work paint a visible cursor cue at the landed segment without seeking MPV, so prose navigation reads like `x` / `y` do while audio keeps playing where it was.

**Architecture:** The two `_no_seek` cursor handlers already move the cursor and skip the MPV seek (`PageChangeReason::Cursor`). Add one helper that paints the karaoke phrase tint at the new cursor line's timestamp — the same tint `seek_to_current_line` paints — but issues no `MpvCommand::Seek`. When the work has no phrase/timestamp data, fall back to the existing brief prose-flash cue. Call the helper at the tail of both handlers, gated on prose.

**Tech Stack:** Rust, GTK4 / sourceview5, libadwaita. Existing modules: `src/input/navigation.rs`, `src/input/phrase_highlight.rs`, `src/input/highlight.rs`.

## Global Constraints

- Do NOT run `cargo run` — build only (`cargo build`); the user launches the app.
- No `MpvCommand::Seek` (or any `cmd_tx` send) may be issued on the j/k path.
- Do not change `PageChangeReason::Cursor` / `should_seek()` gating.
- Do not change navigation targeting (`next_dialogue_line` / `prev_dialogue_line`) or play/verse j/k behavior.
- Existing sync-suppression rule: never SHORTEN an existing longer `suppress_sync_until`.
- Bypass interactive shell aliases in non-interactive Bash: `\cp -f`, `command rm -f`.

---

### Task 1: Add `paint_prose_nav_cue` helper (phrase tint, no seek)

Add a private helper in `navigation.rs` that paints the karaoke phrase tint at the current cursor line without seeking, and falls back to the prose flash when there is no phrase tint to paint. This is the whole behavioral change; the next task just wires it into the two handlers.

**Files:**
- Modify: `src/input/navigation.rs` (add helper near `seek_to_current_line`, ~line 2941)

**Interfaces:**
- Consumes:
  - `crate::input::phrase_highlight::paint_pending_phrase(s: &mut AppState, pos: f64) -> bool` — returns `true` when it painted the tint (phrase mode on, not translations); `false` otherwise (no phrase data / translations visible).
  - `crate::input::highlight::flash_reader_cursor(state: &mut AppState)` — brief prose-cursor flash; already no-ops for verse, when `show_cursor_line` is off, and when `PROSE_DIM_OTHER_PARAGRAPHS` is set.
  - `AppState::is_prose(&self) -> bool`, `AppState::work_line_for_buffer(usize) -> Option<usize>`, `AppState.current_work`, `AppState.translations_visible`, `AppState.mpv_playing`, `AppState.vocab_loop`, `AppState.suppress_sync_until`, `AppState.phrase_paint_hold`.
  - `SYNC_SUPPRESS_SEEK: std::time::Duration` (const in this module, `navigation.rs:96`).
- Produces:
  - `pub(crate) fn paint_prose_nav_cue(state: &mut AppState)` — call at the tail of both `_no_seek` handlers.

- [ ] **Step 1: Write the helper**

Insert directly ABOVE `pub fn seek_to_current_line` (currently `navigation.rs:2941`):

```rust
/// Prose j/k cursor cue WITHOUT an MPV seek. `j`/`k`
/// (`cursor_next/prev_dialogue_no_seek`) move the cursor but skip
/// `seek_to_current_line`, so on non-dim prose the landing has no visible
/// marking (unlike x/y, which seek and thereby paint the karaoke tint). This
/// paints the SAME karaoke tint at the new cursor line — the phrase that would
/// play there — but sends NO `MpvCommand`, so audio keeps playing where it was.
///
/// Timestamped prose: paint the pending phrase and hold it briefly so a sync
/// tick doesn't immediately overwrite it. Untimestamped prose (or works with no
/// phrase data): fall back to the brief prose-cursor flash. No-op for
/// plays/verse (they already show a persistent line tint), in translation view,
/// and while the vocab-sentence loop owns the tint.
pub(crate) fn paint_prose_nav_cue(state: &mut AppState) {
    if !state.is_prose() || state.translations_visible || state.vocab_loop.is_some() {
        return;
    }
    // Timestamp of the CURRENT (just-moved-to) cursor line, if any.
    let ts_start = state
        .work_line_for_buffer(state.current_line)
        .and_then(|wi| state.current_work.as_ref()?.lines.get(wi)?.timestamp.as_ref())
        .map(|t| t.start);

    let painted = match ts_start {
        Some(start) => crate::input::phrase_highlight::paint_pending_phrase(state, start),
        None => false,
    };

    if painted {
        // No seek → no fresh suppression window. Hold the tint for a short fixed
        // window (same as a seek's) so the next sync tick doesn't blank it, and
        // never SHORTEN an existing longer hold (e.g. a work-load window).
        let until = std::time::Instant::now() + SYNC_SUPPRESS_SEEK;
        if state.suppress_sync_until.map_or(true, |existing| until > existing) {
            state.suppress_sync_until = Some(until);
        }
        state.phrase_paint_hold = state.suppress_sync_until;
    } else {
        // No phrase tint to paint (untimestamped prose / no phrase data):
        // brief cursor-line flash so the move is still visible.
        crate::input::highlight::flash_reader_cursor(state);
    }
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -20`
Expected: builds successfully. A `dead_code` warning for `paint_prose_nav_cue` (not yet called) is acceptable at this step — Task 2 wires it in. If the build ERRORS, fix before proceeding.

- [ ] **Step 3: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "feat(nav): add paint_prose_nav_cue (phrase tint, no seek)"
```

---

### Task 2: Wire the cue into both `_no_seek` handlers

Call `paint_prose_nav_cue` at the tail of `cursor_prev_dialogue_no_seek` and `cursor_next_dialogue_no_seek`, after `after_page_change` has repainted the highlight. This is the step that makes j/k visible on prose.

**Files:**
- Modify: `src/input/navigation.rs:1535-1555` (`cursor_prev_dialogue_no_seek`), `1559-1580` (`cursor_next_dialogue_no_seek`)

**Interfaces:**
- Consumes: `paint_prose_nav_cue(state)` from Task 1.
- Produces: nothing new; completes the behavior.

- [ ] **Step 1: Wire into `cursor_prev_dialogue_no_seek`**

In `cursor_prev_dialogue_no_seek`, the `if let Some(target) = target { … }` block ends with `after_page_change(state, PageChangeReason::Cursor);`. Add the cue call immediately after it, still inside the `if let` block:

```rust
    if let Some(target) = target {
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
        scroll_after_jump_backward(state);
        after_page_change(state, PageChangeReason::Cursor);
        paint_prose_nav_cue(state);
    }
```

- [ ] **Step 2: Wire into `cursor_next_dialogue_no_seek`**

Same edit in `cursor_next_dialogue_no_seek`, after its `after_page_change(state, PageChangeReason::Cursor);`:

```rust
    if let Some(target) = target {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Cursor);
        paint_prose_nav_cue(state);
    }
```

- [ ] **Step 3: Build to verify it compiles (no dead_code warning now)**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -20`
Expected: builds successfully, and the `dead_code` warning for `paint_prose_nav_cue` from Task 1 is gone.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "feat(nav): paint cursor cue on prose j/k (no seek)"
```

---

### Task 3: Fix stale doc comments for the Cursor/Dialogue reason and handlers

The `PageChangeReason::Cursor` / `Dialogue` doc comment and the `(h key)` / `(k key)` handler doc comments describe the OPPOSITE of the real `keymap_config.rs` bindings (verified: `j`→`CursorNextDialogueNoSeek`, `k`→`CursorPrevDialogueNoSeek`, both no-seek). Correct them so the next reader isn't misled. Comment-only; no behavior change.

**Files:**
- Modify: `src/input/navigation.rs:125-130` (reason doc), `1531-1534` and `1557-1558` (handler docs), `1582` and `1620` (`cursor_prev_line` / `cursor_next_dialogue` docs)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing.

- [ ] **Step 1: Fix the `PageChangeReason` variant docs**

Replace the `Dialogue` / `Cursor` doc comment block (currently `navigation.rs:125-130`):

```rust
    /// User pressed comma / q / Down / apostrophe for dialogue navigation
    /// (these seek MPV to the landed line).
    Dialogue,
    /// Cursor-only movement with NO audio seek — j / k (and their h / t twins)
    /// step to the prev / next segment while MPV keeps playing where it was.
    Cursor,
```

- [ ] **Step 2: Fix the `_no_seek` handler docs**

Replace the doc comment on `cursor_prev_dialogue_no_seek` (currently `navigation.rs:1531-1534`):

```rust
/// Previous segment, cursor-only — NO media seek (`k` key; also `t`).
/// Mirrors `jump_to_prev_dialogue` but passes `PageChangeReason::Cursor`
/// so `after_page_change` skips `seek_to_current_line`: the highlight moves
/// to the prior segment while MPV keeps playing where it was. On prose,
/// `paint_prose_nav_cue` paints the karaoke tint at the landing (no seek).
```

Replace the doc comment on `cursor_next_dialogue_no_seek` (currently `navigation.rs:1557-1558`):

```rust
/// Next segment, cursor-only — NO media seek (`j` key; also `h`). See
/// `cursor_prev_dialogue_no_seek`.
```

- [ ] **Step 3: Fix the `cursor_prev_line` / `cursor_next_dialogue` docs**

These two are NOT on the j/k path (they seek). Correct their key annotations to remove the false `(k key)` / `(j key)` claims. Replace the doc comment on `cursor_prev_line` (currently `navigation.rs:1582`):

```rust
/// Move cursor to previous line/segment and seek media to it (`Up` path,
/// seeking twin of the no-seek `k`).
```

Replace the doc comment on `cursor_next_dialogue` (currently `navigation.rs:1620`):

```rust
/// Move cursor to next line/segment and seek media to it (`Down` / apostrophe
/// path, seeking twin of the no-seek `j`).
```

- [ ] **Step 4: Build to verify it still compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -20`
Expected: builds successfully (comment-only change).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "docs(nav): correct stale Cursor/Dialogue reason + handler key comments"
```

---

### Task 4: Headless verification on TT (phrase tint, no seek)

Drive `j` / `k` on TT headlessly, screenshot the landing, and confirm from the log that the presses produced `ACTION: CursorNextDialogueNoSeek` / `CursorPrevDialogueNoSeek` with NO `SEEK:` line (no MPV seek), and that the phrase tint is visible on the landed paragraph.

**Files:**
- No source changes. Uses the headless harness and `test-headless-navigation` skill.

**Interfaces:**
- Consumes: the built binary from Task 2.
- Produces: verification evidence (screenshots + log excerpt).

- [ ] **Step 1: Ensure a fresh build**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: builds successfully.

- [ ] **Step 2: Run the headless nav drive on TT**

Use the env wrapper with `--start-work` so the run does NOT rewrite the dev config's `last_work`:

```bash
cd ~/utono/linux-lit && ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work TT --secs 20
```

Expected: exits 0; screenshots land in `target/ui/`; full log at `/tmp/fuzz-nav.log`.

If the fuzz harness does not accept a plain j/k drive, fall back to the manual cage drive from `CLAUDE.md` "Headless Verification": launch under cage with `LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo`, `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`, wait 3s, then `wtype -k j` / `wtype -k k` a few times with `grim` captures between, and check the fresh `-{n}` log by mtime. Cleanup ONLY: `pkill -f "cage -- ./target/debug/linux-lit"`.

- [ ] **Step 3: Assert no seek on the j/k presses**

Run:

```bash
rg -n 'ACTION: CursorNextDialogueNoSeek|ACTION: CursorPrevDialogueNoSeek|SEEK:' /tmp/fuzz-nav.log | tail -40
```

Expected: `ACTION: CursorNext/PrevDialogueNoSeek` lines are present, and NO `SEEK:` line appears between a j/k ACTION line and the next ACTION line. (A `SEEK:` from a page-turn x/y press elsewhere is fine; the requirement is that j/k themselves do not seek.)

- [ ] **Step 4: Open every screenshot and confirm the cue is visible**

Open each PNG in `target/ui/` (and any `_clip.png`). Confirm: after a j/k press the landed paragraph shows the karaoke phrase tint (`phrase_highlight_bg`) on its first phrase, and the previous landing's tint has moved. Quote the on-screen text of the tinted paragraph in the report.

If the tint is too subtle to see in software-rendered cage captures, note that and hand the user the exact command for a real-renderer eyeball (Step 5).

- [ ] **Step 5: Hand the user the real-renderer check**

Because cage is software rendering and the cue is a subtle tint, give the user this to confirm on the real GL renderer:

> Launch TT, put the cursor on a body paragraph, and press `j` / `k` a few times. Each press should move the karaoke tint to the new paragraph's first phrase, and audio should keep playing where it was (no seek).

---

## Self-Review

**Spec coverage:**
- Goal (visible cue on j/k, no seek) → Tasks 1–2.
- Approach: paint phrase tint without seek → Task 1 (helper), Task 2 (wiring).
- Fallback for untimestamped prose → Task 1 (`flash_reader_cursor` branch).
- Edge cases: no work / unmapped line → `work_line_for_buffer` / `current_work` `?`-chains return early; translations visible → guarded; play/verse → `is_prose()` guard; vocab loop → `vocab_loop.is_some()` guard; suppression never shortened → `map_or(true, |e| until > e)` guard. All in Task 1.
- Testing (headless e2e, no-seek log assertion, manual eyeball) → Task 4.
- Docs/memory follow-ups: stale comments → Task 3; memory update → noted below (not a code task).

**Placeholder scan:** No TBD/TODO; all code steps show full code; all commands have expected output.

**Type consistency:** `paint_prose_nav_cue(state)` defined in Task 1, called with the same name/signature in Task 2. `paint_pending_phrase(s, pos) -> bool`, `flash_reader_cursor(state)`, `SYNC_SUPPRESS_SEEK`, and the `AppState` fields (`suppress_sync_until`, `phrase_paint_hold`, `translations_visible`, `vocab_loop`) all verified against current source.

**Post-implementation (not a task):** Update memory `project-prose-nav-flash` — prose nav binds now paint a cue again on the j/k no-seek path (phrase tint for timestamped prose, brief flash fallback otherwise); the "nav binds do NOT flash / no persistent cursor tint" note is superseded for that path.
