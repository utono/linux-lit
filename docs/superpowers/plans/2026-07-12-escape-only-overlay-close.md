# Escape-Only Overlay Close Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Escape becomes the only key that closes a reader overlay (translation keeps its `i` toggle-close), overlay-to-overlay cross-jump/create keys are dropped in favor of the `\` cycle, and every `\` advance stops TTS.

**Architecture:** Handler-local edits in `src/input/keymap.rs`: each dropped key becomes an explicit consumed no-op arm (never a deleted arm — deletion lets the keypress fall through to chord starters, TTS arms, or block nav). Knock-on dead code in `gloss.rs`/`journal.rs` is slimmed. Legends mirror the drops. No reader-mode binds, no keymap.json, no Action enum changes.

**Tech Stack:** Rust, GTK4. Repo: linux-lit, worktree `~/utono/linux-lit-wt/feat-escape-only-overlay-close`, branch `feat/escape-only-overlay-close` (base master @ 648029a).

**Spec:** `docs/superpowers/specs/2026-07-12-escape-only-overlay-close-design.md`.

## Global Constraints

- Work ONLY in the worktree `~/utono/linux-lit-wt/feat-escape-only-overlay-close` (own `target/`). All `cd` means the worktree.
- Verify with `cargo build` / `cargo test --bins` / `cargo clippy`; never run the app. Headless cage only in Task 5; cleanup ONLY `pkill -f "cage -- ./target/debug/linux-lit"`.
- **Dropped keys become consumed no-op arms** with a one-line `// Escape-only close policy: …` comment. Never delete an arm outright.
- Untouched: the `\` cycle arms, Escape arms, translation `i` close, `R`/`e` edits, journal's own `r`, pickers, per-overlay legends' Escape/Ctrl+/ (close to parent), settings/keybinds/gamepad handlers, vim/ask-card interception, reader-mode dispatch and keymap_config bindings.
- Line numbers below were measured at 648029a and should be exact; still locate arms by the quoted code, not the number alone.
- Commit messages end with the trailer:

```
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Jqf5JrAWddoTDjXipXWkJV
```

---

### Task 1: TTS stop on every `\` advance

**Files:**
- Modify: `src/input/actions/overlay_cycle.rs`

**Interfaces:** none new; `AppState.tts.stop()` exists (used by `cycle_from_gloss` already).

- [ ] **Step 1: Add the stops**

In `cycle_from_journal`, directly after `let mut s = state.borrow_mut();`:

```rust
        // Every `\` advance silences TTS (a journal block read with s/Space
        // must not keep speaking into the gloss stop).
        s.tts.stop();
```

In `cycle_from_synopsis`, directly after `let mut s = state.borrow_mut();`:

```rust
        // Every `\` advance silences TTS (matching the other two advances).
        s.tts.stop();
```

- [ ] **Step 2: Build and test**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -2
```

Expected: clean build, 792 tests pass.

- [ ] **Step 3: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && git add src/input/actions/overlay_cycle.rs && git commit -m "feat: every \\ cycle advance stops TTS"
```

---

### Task 2: Escape-only in gloss + journal handlers, Ctrl+Tab slimming

**Files:**
- Modify: `src/input/keymap.rs` (`handle_journal_key` ~1202–1486, `handle_gloss_key` ~1487–2015)
- Modify: `src/input/actions/gloss.rs` (`toggle_overlay` ~2634, `toggle_last_overlay` ~2757)
- Modify: `src/input/actions/journal.rs` (dead helper check)
- Modify: `src/input/actions/mod.rs` (`ToggleLastOverlay` doc ~line 140)
- Modify: `src/ui/keybinds_overlay.rs` (describe `"last overlay"` ~line 267)

**Interfaces:**
- Consumes: existing close fns (unchanged): `close_gloss_to_reader`, `journal::close_overlay`.
- Produces: `gloss::toggle_overlay` becomes open-only (same signature; now delegates to `open_gloss_at_cursor`); `gloss::toggle_last_overlay` becomes Reader-mode-only reopen (same signature).

- [ ] **Step 1: Journal handler — four no-ops**

In `handle_journal_key`:

1a. Alt block (`if is_alt { match key_name {`): replace the `"g"` arm (currently calls `journal::action_gloss_from_journal_passage(state)`, ~line 1288) with:

```rust
            // Alt+g: dropped (cross-create: reader-gloss from the journal
            // passage). Consumed so Alt+g can't start a gg chord below.
            "g" => return true,
```

1b. Ctrl block: replace the `"j"` arm (currently `journal::close_overlay(state)`, ~line 1311) with:

```rust
            // Ctrl+j: Escape-only close policy — consumed no-op (was: close
            // the journal). Consumed so Ctrl+j can't fall through to the
            // plain j block-nav arm.
            "j" => return true,
```

1c. Ctrl block: replace the `"Tab" | "ISO_Left_Tab"` arm (currently `gloss::toggle_last_overlay(state)`, ~line 1319) with:

```rust
            // Ctrl+Tab: dropped inside overlays (reader-side Ctrl+Tab still
            // reopens the last overlay). Consumed so it can't fall through
            // to the plain Tab TTS arm.
            "Tab" | "ISO_Left_Tab" => return true,
```

1d. Ctrl block: replace the `"g"` arm (currently `journal::view_gloss_from_journal(state)`, ~line 1332) with:

```rust
            // Ctrl+g: dropped (cross-jump to the gloss view — the \ cycle is
            // the only overlay-to-overlay navigation). Consumed so it can't
            // start a gg chord below.
            "g" => return true,
```

The `"Escape"` arm (~1479) and the `"backslash"` cycle arm (~1475) stay byte-identical.

- [ ] **Step 2: Gloss handler — five no-ops**

In `handle_gloss_key`:

2a. Ctrl block: replace the `"j"` arm (currently `journal::view_journal_from_gloss(state)`, ~line 1604) with:

```rust
            // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the
            // only overlay-to-overlay navigation). Consumed no-op.
            "j" => return true,
```

2b. Ctrl block: replace the `"g"` arm (currently `close_gloss_to_reader(state)`, ~line 1613) with:

```rust
            // Ctrl+g: Escape-only close policy — consumed no-op (was: close
            // same as Escape). Consumed so it can't start a gg chord below.
            "g" => return true,
```

2c. Ctrl block: replace the `"Tab" | "ISO_Left_Tab"` arm (currently `gloss::toggle_last_overlay(state)`, ~line 1621) with the same consumed no-op arm as 1c (identical comment).

2d. Plain match: split the `"Escape" | "n"` arm (~line 1765). `"Escape"` keeps the existing body verbatim (`close_gloss_to_reader`); add directly after it:

```rust
        // n: Escape-only close policy — was Escape's close alias; consumed
        // no-op so it can't leak past the handler.
        "n" => true,
```

2e. Plain match: replace the `"r"` arm's body (currently hides the overlay and calls `journal::begin_passage_ask(...)`, ~lines 1794–1851) with:

```rust
        // r: dropped (cross-create: journal ask card for the gloss passage;
        // asking happens from the reader). Consumed no-op.
        "r" => true,
```

- [ ] **Step 3: Slim the dead code**

3a. `gloss::toggle_last_overlay` (gloss.rs ~2748–2778): the GlossOverlay/JournalOverlay close arms are now unreachable (reader dispatch fires only in Reader mode; the in-overlay Ctrl+Tab arms are gone). Replace the whole fn with:

```rust
/// Reopen whichever toggleable overlay (gloss/journal) was last open
/// (`AppState.last_overlay`, recorded at every close via
/// `return_to_reader_mode`). Reader-only: overlays close via Escape alone,
/// so this no longer doubles as an in-overlay close. Toasts when nothing is
/// remembered. Bound to Ctrl+Tab (`ToggleLastOverlay`).
pub(crate) fn toggle_last_overlay(state: &Rc<RefCell<AppState>>) {
    use crate::app::{InputMode, LastOverlay};
    let (mode, last) = {
        let s = state.borrow();
        (s.input_mode, s.last_overlay)
    };
    if mode != InputMode::Reader {
        return;
    }
    match last {
        Some(LastOverlay::Gloss) => toggle_overlay(state),
        Some(LastOverlay::Journal) => crate::input::actions::journal::toggle_overlay(state),
        None => show_tts_toast(state, "No overlay to reopen"),
    }
}
```

3b. `gloss::toggle_overlay` (gloss.rs ~2634): its close half (the `if input_mode == GlossOverlay` block) has no remaining caller path — Escape closes via `close_gloss_to_reader`, and 3a only calls this from Reader mode. Replace the fn with:

```rust
/// Open the gloss overlay for the cursor line (reader Ctrl+g /
/// `Action::ToggleGlossOverlay`, and the Ctrl+Tab reopen). Open-only since
/// the Escape-only close policy: the overlay closes via Escape
/// (`close_gloss_to_reader`), never by re-pressing the toggle.
pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    open_gloss_at_cursor(state);
}
```

(Keep the name — `Action::ToggleGlossOverlay` and keymap.json stay untouched. `journal::toggle_overlay` KEEPS its close half: Escape routes through `journal::close_overlay` → `toggle_overlay`.)

3c. Dead helpers: run `rg -n 'view_gloss_from_journal|view_journal_from_gloss|action_gloss_from_journal_passage' src/`. For each fn whose ONLY remaining hits are its definition (and doc comments), delete the fn. If an unexpected caller exists, leave the fn and note it in your report. Do NOT touch `begin_passage_ask`/`begin_scene_ask` in this task.

3d. `Action::ToggleLastOverlay` doc comment (mod.rs ~140–143): replace with:

```rust
    /// Reader-only: reopen whichever of the gloss/journal overlays was last
    /// open (fresh from the cursor). No-op with a toast if none remembered.
    /// Overlays themselves close via Escape only.
    ToggleLastOverlay,
```

3e. `keybinds_overlay.rs` describe() `"last overlay"` arm (~line 267): replace the string with:

```rust
        "last overlay" => "Action::ToggleLastOverlay (reader only: reopens \
the last-closed gloss/journal overlay; overlays close via Escape) \
— src/input/actions/gloss.rs",
```

- [ ] **Step 4: Build, test, clippy**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -2 && cargo clippy 2>&1 | rg -c warning
```

Expected: clean build, 792 pass, no NEW clippy warnings (compare count to master's 157 if unsure; deleted fns may REDUCE the count).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && git add src/input/keymap.rs src/input/actions/gloss.rs src/input/actions/journal.rs src/input/actions/mod.rs src/ui/keybinds_overlay.rs && git commit -m "feat: Escape-only close in gloss/journal overlays; drop cross-jumps + in-overlay Ctrl+Tab"
```

---

### Task 3: Escape-only in synopsis, translation, echoes handlers

**Files:**
- Modify: `src/input/keymap.rs` (`handle_synopsis_overlay_key` ~2016, `handle_translation_overlay_key` ~1902, `handle_echoes_overlay_key` ~2633)
- Modify: `src/input/actions/journal.rs` or wherever `begin_scene_ask` lives (dead-check only)

**Interfaces:** none new.

- [ ] **Step 1: Synopsis handler — four no-ops**

1a. The standalone Ctrl+g guard (~line 2061, `if key_name == "g" && is_ctrl`, body clears the chord then hides + returns to reader): KEEP the guard and the chord clear, drop the close:

```rust
    // Ctrl+g: Escape-only close policy — consumed no-op (was: close, same
    // as Escape). Still clears a pending gg chord so a held Ctrl isn't
    // swallowed as the second g.
    if key_name == "g" && is_ctrl {
        key_state.borrow_mut().chord = ChordState::None;
        return true;
    }
```

1b. Plain match: replace the `"h"` arm's body (currently hides + `return_to_reader_mode`, ~line 2089) with:

```rust
        // h: Escape-only close policy — consumed no-op (was: close; h still
        // OPENS the synopsis from the reader).
        "h" => true,
```

1c. Replace the `"r"` arm's body (currently hides, returns to reader, then `journal::begin_scene_ask(...)`, ~line 2114) with:

```rust
        // r: dropped (cross-create: scene ask card; asking happens from the
        // reader). Consumed no-op.
        "r" => true,
```

1d. Replace the Ctrl+j arm/guard body (currently hides + `open_journal_scene`, ~line 2166) with a consumed no-op (same shape as its current syntax — arm `=> true` or guard `return true;`), comment:

```rust
        // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the only
        // overlay-to-overlay navigation). Consumed no-op.
```

The Escape guard (~2050), Tab swallow (~2046), `\` arm (~2097), Alt+g picker, `R`, `e` all stay byte-identical.

- [ ] **Step 2: Translation handler — one no-op**

Replace the Ctrl+j guard body (keymap.rs:1916–1924) with:

```rust
    // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the only
    // overlay-to-overlay navigation). Still checked before the plain-`j`
    // dialogue-step so Ctrl+j can't step the cursor.
    if key_name == "j" && is_ctrl {
        return true;
    }
```

The `i` toggle-close (1905–1910) and Escape arm (1926–1931) stay byte-identical (user-retained).

- [ ] **Step 3: Echoes handler — two no-ops**

In the ctrl block (keymap.rs:2660):

3a. Replace the `"g"` arm (2667–2670, `close_echoes_to_reader`) with:

```rust
            // Ctrl+g: Escape-only close policy — consumed no-op (was: close,
            // same as Escape).
            "g" => return true,
```

3b. Replace the `"j"` arm (2675–2679, close + `open_journal_scene`) with:

```rust
            // Ctrl+j: dropped (cross-jump to journal — the \ cycle is the
            // only overlay-to-overlay navigation). Consumed no-op.
            "j" => return true,
```

The Escape arm (~2760) stays byte-identical.

- [ ] **Step 4: Dead-check `begin_scene_ask`**

```bash
rg -n 'begin_scene_ask' src/
```

If its only remaining hits are the definition, delete the fn; if other callers exist (e.g. reader-side ask paths), leave it. Do NOT delete `begin_passage_ask` (the reader Ctrl+a ask-passage path uses it).

- [ ] **Step 5: Build, test, clippy**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -2 && cargo clippy 2>&1 | rg -c warning
```

Expected: clean build, 792 pass, no new warnings.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && git add src/input/keymap.rs src/input/actions/ && git commit -m "feat: Escape-only close in synopsis/translation/echoes overlays; drop cross-jumps"
```

---

### Task 4: Legend mirrors

**Files:**
- Modify: `src/ui/gloss_keybinds_overlay.rs`
- Modify: `src/ui/journal_keybinds_overlay.rs`
- Modify: `src/ui/synopsis_keybinds_overlay.rs`
- Modify: `src/ui/echo_keybinds_overlay.rs`

**Interfaces:** display data only. Every row edit below mirrors a Task 2/3 handler change — nothing else moves.

- [ ] **Step 1: Gloss legend**

In `GROUPS`:
- Drop the row `("Ctrl+j", "view journal for passage")` (~line 50).
- Close row (~line 58): `("Esc / n / Ctrl+g", "close (jump to source)")` → `("Esc", "close (jump to source)")`.
- Drop the `r` ask row (search `("r"` in this file; its text mentions creating a journal Q&A for the passage). Keep the `R` rewrite row.
- Keep the `\` cycle row.

- [ ] **Step 2: Journal legend**

- "Cross-reference" group: drop `("Ctrl+g", "view gloss for passage")` and `("Alt+g", "gloss this passage")`; keep `("Ctrl+\\", "pick a Q&A")` and the `\` cycle row.
- "Close" group: `("Esc / Ctrl+j", "close → reader")` → `("Esc", "close → reader")`.
- Keep the "Editing" group's `r`/`R`/`e` rows (native journal asks).

- [ ] **Step 3: Synopsis legend**

- "Journal" group: drop `("r", "new journal Q&A for scene")` and `("Ctrl+j", "go to journal for scene")`; keep `("Alt+g", "work glosses")` (group keeps its one remaining row).
- "View" group: `("h / Esc / Ctrl+g", "close")` → `("Esc", "close")`; keep the `\` cycle row and `("Ctrl+/", "close this legend")`.
- Keep the "Editing" group's `R`/`e`/`u`/`c` rows.

- [ ] **Step 4: Echo legend**

In `BINDS` (flat array):
- Drop `("Ctrl+j", "go to journal for turn")` (~line 27).
- `("Esc / Ctrl+g", "close echoes → reader")` → `("Esc", "close echoes → reader")` (~line 28).

- [ ] **Step 5: Build and test**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -2
```

Expected: clean build, tests pass.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && git add src/ui/gloss_keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs src/ui/synopsis_keybinds_overlay.rs src/ui/echo_keybinds_overlay.rs && git commit -m "docs(ui): legends mirror Escape-only close + dropped cross-jumps"
```

---

### Task 5: Headless e2e acceptance

**Files:** none created (screenshots to the scratchpad). Read the "Headless Verification" and "UI review protocol" sections of the main checkout's CLAUDE.md first. Cleanup ONLY `pkill -f "cage -- ./target/debug/linux-lit"`. lit.db queries read-only (`file:...?mode=ro`).

- [ ] **Step 1: Launch** — build in the worktree; cage launch per CLAUDE.md (`LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman`); pick a segment with journal Q&A + gloss + synopsis (same SQL approach as the cycle branch's e2e; the loaders are `find_scene_band_pages`, `find_glossed_passages`, and the synopsis loader in `scene_synopsis.rs`).

- [ ] **Step 2: Per-overlay inert-key + Escape checks** (screenshot after every keypress; a dropped key PASSES if the overlay is visibly unchanged):
- Journal (open with Ctrl+j from the reader): press Ctrl+j, Ctrl+Tab, Ctrl+g, Alt+g — all inert; Escape closes to the reader.
- Gloss (Ctrl+g from the reader on a glossed line): press n, Ctrl+g, Ctrl+Tab, Ctrl+j, r — all inert; Escape closes (jump-to-source behavior unchanged).
- Synopsis (h from the reader): press h, Ctrl+g, Ctrl+j, r — all inert; Escape closes.
- Translation (u from the reader — needs a work with translations; if the chosen work lacks them, find one via lit.db, else report the check as NOT RUN with the reason): press Ctrl+j — inert; then `i` closes; reopen; Escape closes.
- Echoes (Alt+w / Ctrl+e per keymap_config on a line with echo data; if no echo data is reachable, report NOT RUN with the reason): press Ctrl+g, Ctrl+j — inert; Escape closes.

- [ ] **Step 3: Cycle regression** — one full `\` lap (journal → gloss → synopsis → journal) on the target segment still works; Escape restores the reading position.

- [ ] **Step 4: Reader-side Ctrl+Tab** — from the reader after closing the journal via Escape, Ctrl+Tab reopens the journal (reopen path still alive).

- [ ] **Step 5: Cleanup + suites**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
cd ~/utono/linux-lit-wt/feat-escape-only-overlay-close && cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3
```

Open every PNG and report what you see; a passing exit code is not acceptance.

---

### Task 6: Final review + finish the branch

- [ ] **Step 1:** Final whole-branch code review (requesting-code-review template, most capable model).
- [ ] **Step 2:** Invoke `superpowers:finishing-a-development-branch`: merge master into the branch first if master moved, re-verify, then `git merge --no-ff` into master FROM THE MAIN CHECKOUT (do not disturb its dirty files or build in it), remove the worktree, delete the branch. No push (pushes wait for the user's mr flow). Remind the user to restart `crll`.
