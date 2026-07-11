# Ask-Passage Keybind (Ctrl+a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Ctrl+a` in reader mode auto-selects the paragraph/speech around the cursor as a visual selection; a second `Ctrl+a` (or `Return`) opens the Journal Q&A "Ask a question about this passage" card directly, skipping the Action menu. `ToggleAuthorship` moves from `Ctrl+a` to plain `A`.

**Architecture:** A pure `block_bounds` helper finds the blank-line-delimited block; a new `enter_visual_block_mode` enters the EXISTING visual mode with that block pre-selected and a `pending_ask` flag on `SelectionState`. The visual-mode key handler gains a `Ctrl+a` confirm arm and a flag-aware `Return` arm, both funneling into the existing `action_journal_qa` → `begin_passage_ask` pipeline (untouched).

**Tech Stack:** Rust, GTK4/sourceview5. Spec: `docs/plans/2026-07-10-ask-passage-keybind-design.md`.

## Global Constraints

- Build with `cargo build`; NEVER `cargo run` (the user runs the app).
- The stow keymap `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` silently overrides compiled defaults — it MUST be updated in Task 6 or both rebinds are shadowed.
- Base commit is `a140ad0` on `master` (the Shift+1 CopyWorkInfo commit). All linux-lit work happens on branch `ask-passage`.
- Pre-existing failing test `db::queries::tests::test_load_work_hamlet` (asserts live lit.db state) is NOT caused by this work — ignore it in every test run.
- Keys used in tests/verification are on Real Programmers Dvorak; for `wtype`, `Ctrl+a` is `wtype -M ctrl -k a -m ctrl`.

---

### Task 1: `block_bounds` pure helper (TDD)

**Files:**
- Modify: `src/input/visual.rs` (add helper + tests at end of file)

**Interfaces:**
- Produces: `pub(crate) fn block_bounds(line_count: usize, cursor: usize, is_boundary: impl Fn(usize) -> bool) -> Option<(usize, usize)>` — inclusive `(start, end)` of the contiguous non-boundary block containing `cursor`; `None` when `cursor` is itself a boundary line (blank/separator) or out of range. Task 2 consumes it.

- [ ] **Step 1: Create the branch**

```bash
cd ~/utono/linux-lit && git checkout master && git checkout -b ask-passage
```

- [ ] **Step 2: Write the failing tests**

Append at the end of `src/input/visual.rs`:

```rust
#[cfg(test)]
mod block_bounds_tests {
    use super::block_bounds;

    /// Test harness: boundary = blank or separator line, same rule
    /// enter_visual_block_mode uses.
    fn bounds(lines: &[&str], cursor: usize) -> Option<(usize, usize)> {
        let texts: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        let is_boundary = |idx: usize| {
            let t = texts[idx].trim();
            t.is_empty() || crate::db::line_types::is_separator(t)
        };
        block_bounds(lines.len(), cursor, is_boundary)
    }

    #[test]
    fn paragraph_mid_buffer() {
        let lines = ["First para.", "", "Second para line 1.", "line 2.", "line 3.", "", "Third."];
        assert_eq!(bounds(&lines, 3), Some((2, 4)));
        // Every line of the block maps to the same bounds.
        assert_eq!(bounds(&lines, 2), Some((2, 4)));
        assert_eq!(bounds(&lines, 4), Some((2, 4)));
    }

    #[test]
    fn speech_includes_speaker_label() {
        // A play speech: speaker label + verse lines form one contiguous block.
        let lines = ["", "HAMLET", "To be, or not to be: that is the question:", "Whether 'tis nobler in the mind to suffer", ""];
        assert_eq!(bounds(&lines, 2), Some((1, 3)));
    }

    #[test]
    fn cursor_on_blank_line_is_none() {
        let lines = ["First.", "", "Second."];
        assert_eq!(bounds(&lines, 1), None);
    }

    #[test]
    fn block_at_buffer_start_and_end() {
        let lines = ["Line a.", "Line b.", "", "Tail line 1.", "Tail line 2."];
        assert_eq!(bounds(&lines, 0), Some((0, 1)));
        assert_eq!(bounds(&lines, 4), Some((3, 4)));
    }

    #[test]
    fn cursor_out_of_range_is_none() {
        let lines = ["Only line."];
        assert_eq!(bounds(&lines, 5), None);
        assert_eq!(bounds(&[], 0), None);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test --bins block_bounds 2>&1 | tail -5
```

Expected: compile error — `block_bounds` not found.

- [ ] **Step 4: Write the implementation**

In `src/input/visual.rs`, directly after the `SelectionState` impl block (after line ~25):

```rust
/// Inclusive `(start, end)` of the contiguous block of non-boundary lines
/// containing `cursor`. A "boundary" line (blank line or separator, decided by
/// the caller's closure) delimits the block: prose paragraphs and play
/// speeches are both blank-line-delimited in the reader buffer, so this yields
/// the paragraph (prose) or the speech including its speaker label (plays).
/// Returns `None` when `cursor` is out of range or is itself a boundary line —
/// callers fall back to a single-line selection.
pub(crate) fn block_bounds(
    line_count: usize,
    cursor: usize,
    is_boundary: impl Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    if cursor >= line_count || is_boundary(cursor) {
        return None;
    }
    let mut start = cursor;
    while start > 0 && !is_boundary(start - 1) {
        start -= 1;
    }
    let mut end = cursor;
    while end + 1 < line_count && !is_boundary(end + 1) {
        end += 1;
    }
    Some((start, end))
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --bins block_bounds 2>&1 | tail -5
```

Expected: `5 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat: block_bounds helper for paragraph/speech selection"
```

---

### Task 2: `pending_ask` flag + `enter_visual_block_mode`

**Files:**
- Modify: `src/input/visual.rs` (`SelectionState` at lines 5–25, `enter_visual_mode` at ~line 77)

**Interfaces:**
- Consumes: `block_bounds` from Task 1.
- Produces: `SelectionState.pending_ask: bool` field (Task 4 reads it); `pub fn enter_visual_block_mode(state: &mut AppState)` (Task 3's dispatch arm calls it).

- [ ] **Step 1: Add the `pending_ask` field**

In `src/input/visual.rs`, change the struct and constructor (keep the existing doc comment):

```rust
/// Tracks the visual selection range (anchor..cursor).
pub struct SelectionState {
    pub anchor_line: usize,
    pub cursor_line: usize,
    /// True when visual mode was entered via Ctrl+a (AskPassage): Return then
    /// confirms the Journal Q&A ask directly instead of opening the Action
    /// menu. Extending the selection with j/k/G/gg keeps the flag.
    pub pending_ask: bool,
}

impl SelectionState {
    pub fn new(line: usize) -> Self {
        Self {
            anchor_line: line,
            cursor_line: line,
            pending_ask: false,
        }
    }
```

(The existing `range()` method below `new` is unchanged. `V`-entered selections keep `pending_ask: false` via `new`; `move_selection_cursor`/`extend_to_*` only touch `cursor_line`, so extending never clears the flag.)

- [ ] **Step 2: Verify no other constructors exist**

```bash
rg -n "SelectionState" src/ | rg -v "visual.rs"
```

Expected: only type references (e.g. `Option<SelectionState>` in app state), no struct-literal constructions outside `visual.rs`. If any construction sites appear, add `pending_ask: false` there.

- [ ] **Step 3: Add `enter_visual_block_mode`**

In `src/input/visual.rs`, directly after `enter_visual_mode` (~line 82):

```rust
/// Ctrl+a (AskPassage): enter visual mode with the blank-line-delimited block
/// around the cursor pre-selected (prose paragraph / play speech incl. speaker
/// label) and `pending_ask` set, so a second Ctrl+a or Return opens the
/// Journal Q&A ask card directly. On a blank/separator line, falls back to a
/// single-line selection (same as V), still flagged pending-ask.
pub fn enter_visual_block_mode(state: &mut AppState) {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    let cursor = state.current_line;
    let bounds = {
        let buffer = &state.buffer;
        block_bounds(line_count, cursor, |idx| {
            let text = crate::input::viewport::buffer_line_text(buffer, idx);
            let t = text.trim();
            t.is_empty() || crate::db::line_types::is_separator(t)
        })
    };
    let (start, end) = bounds.unwrap_or((cursor, cursor));
    state.visual_selection = Some(SelectionState {
        anchor_line: start,
        cursor_line: end,
        pending_ask: true,
    });
    state.current_line = end;
    state.input_mode = crate::app::InputMode::Visual;
    crate::input::navigation::update_highlight_and_ensure_visible(state);
    crate::logging::log(&format!(
        "VISUAL: ask-block entered {}..{} (cursor was {})", start, end, cursor
    ));
}
```

Notes for the implementer:
- `crate::input::viewport::buffer_line_text(&sourceview5::Buffer, usize) -> String` already exists (`src/input/viewport.rs:704`); `state.buffer` is that buffer type.
- Anchor = block start, cursor = block end, so `j` extends downward — mirror of how `enter_visual_mode` + `move_selection_cursor` behave. The selection highlight is applied by the `update_highlight_and_ensure_visible` path, same as `enter_visual_mode` (no explicit `apply_selection_highlight` call).

- [ ] **Step 4: Build and run the unit suite**

```bash
cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -5
```

Expected: build OK; only the pre-existing `test_load_work_hamlet` failure (see Global Constraints).

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat: enter_visual_block_mode with pending_ask flag"
```

---

### Task 3: `Action::AskPassage` + compiled bindings + dispatch arm

**Files:**
- Modify: `src/input/actions/mod.rs` (enum ~line 135, `category()` ~line 297, `name()` ~line 399)
- Modify: `src/input/keymap_config.rs` (`display_bindings` ~line 337, `selection_bindings` ~line 352, tests ~line 427)
- Modify: `src/input/keymap.rs` (`dispatch_action` ~line 3099)

**Interfaces:**
- Consumes: `visual::enter_visual_block_mode` from Task 2.
- Produces: `Action::AskPassage` (serde name `"AskPassage"`, parsed from keymap.json automatically via the derived `Deserialize`); compiled binds `ctrl("a") → AskPassage`, `plain("A") → ToggleAuthorship`.

- [ ] **Step 1: Add the enum variant**

In `src/input/actions/mod.rs`, in the `// Visual / selection` group after `EnterVisualMode,` (line 135):

```rust
    /// Ctrl+a: auto-select the paragraph/speech around the cursor and enter
    /// visual mode pending a Journal Q&A ask; a second Ctrl+a (or Return)
    /// opens the "Ask a question about this passage" card directly.
    AskPassage,
```

In `category()`, extend the Selection arm (~line 297):

```rust
            Action::EnterVisualMode
            | Action::AskPassage
            | Action::WordCycleCopy
            | Action::WordCollectCopy
            | Action::OpenSegmentVim => Category::Selection,
```

In `name()` after the `EnterVisualMode` arm (~line 399):

```rust
            Action::AskPassage => "AskPassage",
```

- [ ] **Step 2: Rebind in keymap_config.rs**

In `display_bindings()` (~line 337), replace:

```rust
        (KeyCombo::ctrl("a"), Action::ToggleAuthorship),
```

with:

```rust
        // Authorship moved off Ctrl+a (now AskPassage). plain("A") is the
        // shifted `a` (cf. plain("G") normalization above).
        (KeyCombo::plain("A"), Action::ToggleAuthorship),
```

In `selection_bindings()` (~line 352), add after the `EnterVisualMode` line:

```rust
        // Ctrl+a: paragraph/speech ask — pre-selects the block, second Ctrl+a
        // or Return opens the Journal Q&A ask card.
        (KeyCombo::ctrl("a"), Action::AskPassage),
```

- [ ] **Step 3: Update the keymap_config tests**

At ~line 427, replace:

```rust
        assert_eq!(m.get(&KeyCombo::ctrl("a")), Some(&Action::ToggleAuthorship));
```

with:

```rust
        assert_eq!(m.get(&KeyCombo::ctrl("a")), Some(&Action::AskPassage));
        assert_eq!(m.get(&KeyCombo::plain("A")), Some(&Action::ToggleAuthorship));
```

(The `ctrl_shift("A") → PickAttributionSet` assertion on the next line stays.)

- [ ] **Step 4: Add the dispatch arm**

In `src/input/keymap.rs`, in `dispatch_action` after the `EnterVisualMode` arm (line 3099):

```rust
        AskPassage => crate::input::visual::enter_visual_block_mode(&mut state.borrow_mut()),
```

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1 | tail -3 && cargo test --bins keymap 2>&1 | tail -5
```

Expected: build OK, keymap tests pass (the compile enforces the new variant is covered in `category()`/`name()` since both match exhaustively).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs
git commit -m "feat: Ctrl+a AskPassage action; authorship moves to Shift+A"
```

---

### Task 4: visual-mode Ctrl+a confirm + flag-aware Return

**Files:**
- Modify: `src/input/visual.rs` (`action_journal_qa` visibility, line 404)
- Modify: `src/input/keymap.rs` (`handle_visual_key` ~line 2849, call site line 167)

**Interfaces:**
- Consumes: `SelectionState.pending_ask` (Task 2); `action_journal_qa` (existing, made `pub(crate)`).
- Produces: visual-mode behavior — `Ctrl+a` always confirms; `Return` confirms iff `pending_ask`, else opens the Action menu.

- [ ] **Step 1: Make `action_journal_qa` callable from keymap.rs**

In `src/input/visual.rs` line 404, change:

```rust
fn action_journal_qa(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
```

to:

```rust
pub(crate) fn action_journal_qa(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
```

- [ ] **Step 2: Thread `is_ctrl` into `handle_visual_key`**

In `src/input/keymap.rs` line 167, change the dispatch to:

```rust
            crate::app::InputMode::Visual => handle_visual_key(state, key_state, key_name, is_ctrl, tokio_handle),
```

And the signature at ~line 2849:

```rust
fn handle_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
```

- [ ] **Step 3: Add the Ctrl+a arm and rewrite the Return arm**

In the `match key_name` block of `handle_visual_key`, add a first arm (guards must precede the plain `"j"`/`"k"` arms — a guarded `"a"` arm anywhere before the catch-all works, but putting it first keeps modifier arms together):

```rust
        // Ctrl+a — open the Journal Q&A ask card for the selection directly
        // (skips the Action menu). Works for ask-entered AND V-entered
        // selections, so the menu is never required for Journal Q&A.
        "a" if is_ctrl => {
            crate::input::visual::action_journal_qa(state);
            true
        }
```

Replace the existing `"Return"` arm (lines 2881–2884):

```rust
        "Return" => {
            // Ask-entered selection (Ctrl+a): Return is a direct confirm.
            // V-entered selection: Return opens the Action menu (unchanged).
            let pending_ask = state
                .borrow()
                .visual_selection
                .as_ref()
                .is_some_and(|s| s.pending_ask);
            if pending_ask {
                crate::input::visual::action_journal_qa(state);
            } else {
                crate::input::visual::open_action_popup(&mut state.borrow_mut());
            }
            true
        }
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -5
```

Expected: build OK; only the pre-existing `test_load_work_hamlet` failure.

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs src/input/keymap.rs
git commit -m "feat: visual-mode Ctrl+a confirm; Return confirms ask-entered selections"
```

---

### Task 5: Ctrl+/ keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (HOME_ROW `a` keycap line 69; `describe()` arms near line 507)

**Interfaces:**
- Consumes: label conventions — shift labels are `"<glyph>: <label>"` and `strip_shift_prefix` (line 560) makes `"A: authorship"` match the existing `"authorship"` describe arm; modifier labels match `describe()` arms verbatim.

- [ ] **Step 1: Update the `a` keycap**

Replace line 69:

```rust
    key("a", "A", "play/pause", "", &[("C-a", "authorship"), ("S-C-a", "attr set")]),
```

with:

```rust
    key("a", "A", "play/pause", "A: authorship", &[("C-a", "ask passage"), ("S-C-a", "attr set")]),
```

- [ ] **Step 2: Add the `describe()` arm**

Near the existing `"authorship"` arm (~line 507), add:

```rust
        "ask passage" => "Auto-select the paragraph (prose) or speech (plays) \
around the cursor as a visual selection; Ctrl+a again (or Return) opens the \
Journal Q&A ask card directly — j/k extend the selection first, Escape \
cancels. -> AskPassage arm -> visual::enter_visual_block_mode — \
src/input/visual.rs, src/input/keymap.rs",
```

(The `"authorship"` arm itself is unchanged — plain `A` now triggers it, and `"A: authorship"` resolves to it via `strip_shift_prefix`.)

- [ ] **Step 3: Three-pass cross-reference (update-cairo-keybinds-overlay skill discipline)**

Verify by reading, not assuming:
1. No blank slot hides a real binding: the `a` keycap now shows `play/pause` (plain), `A: authorship` (shift), `C-a ask passage`, `S-C-a attr set` — matching `keymap_config.rs` exactly (plain `a` TogglePause, plain `A` ToggleAuthorship, ctrl `a` AskPassage, ctrl_shift `A` PickAttributionSet).
2. No label names the wrong action.
3. Every new/changed label has a `describe()` arm: `"ask passage"` (new arm), `"A: authorship"` (via strip → `"authorship"`). Run:

```bash
rg -n '"ask passage"' src/ui/keybinds_overlay.rs
```

Expected: two hits (keycap + describe arm).

- [ ] **Step 4: Build and commit**

```bash
cargo build 2>&1 | tail -3
git add src/ui/keybinds_overlay.rs
git commit -m "feat: keybinds overlay reflects Ctrl+a ask passage / Shift+A authorship"
```

---

### Task 6: stow keymap.json rebind

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (lines 10–12)

**Interfaces:**
- Consumes: serde name `"AskPassage"` from Task 3 (parsed by `parse_action` via derived `Deserialize` — no loader change needed).

- [ ] **Step 1: Edit the stow source**

`~/.config/linux-lit/keymap.json` is a stow symlink to this file, so editing the source is live on next app launch. Replace line 11 and add the `A` bind so the block reads:

```json
    {"key": "a", "action": "TogglePause"},
    {"key": "a", "ctrl": true, "action": "AskPassage"},
    {"key": "A", "action": "ToggleAuthorship"},
    {"key": "A", "ctrl": true, "shift": true, "action": "PickAttributionSet"},
```

- [ ] **Step 2: Validate the JSON**

```bash
jq . ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json > /dev/null && echo OK
```

Expected: `OK`.

- [ ] **Step 3: Commit in tty-dotfiles**

```bash
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "linux-lit: Ctrl+a ask passage; authorship moves to Shift+A" && cd ~/utono/linux-lit
```

---

### Task 7: headless e2e verification + merge

**Files:**
- No source changes (verification + merge only).

**Interfaces:**
- Consumes: the full feature from Tasks 1–6.

- [ ] **Step 1: Launch the reader headless on Bleak House**

Per CLAUDE.md Headless Verification (isolated from the live session):

```bash
cd ~/utono/linux-lit && cargo build
LINUX_LIT_WORK=BH LIT_HEADLESS_TEST=1 LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 5 && ls /run/user/1000/wayland-*
```

Use the new socket (normally `wayland-1`) for all captures below; resize to production geometry:

```bash
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200 && sleep 2
```

- [ ] **Step 2: Verify `Ctrl+a` selects the paragraph**

```bash
wtype "5" && sleep 1              # enter a chapter (front matter has no prose block)
wtype "j" && wtype "j" && sleep 1 # land on a prose line
wtype -M ctrl -k a -m ctrl && sleep 2
grim /tmp/ask1.png && stat -c%s /tmp/ask1.png
```

Read `/tmp/ask1.png` (a real capture is tens of KB — retry after `sleep 3` if ~2 bytes). Expected: the full paragraph containing the cursor is highlighted with the visual-selection tint (like Image #9's selection, but no menu).

- [ ] **Step 3: Verify the second `Ctrl+a` opens the ask card**

```bash
wtype -M ctrl -k a -m ctrl && sleep 2
grim /tmp/ask2.png
```

Expected in `/tmp/ask2.png`: the "Ask a question about this passage" card with the passage text above it and `-- NORMAL -- (Ctrl+Enter submit)` in the footer. Also check the log:

```bash
rg "VISUAL: ask-block entered|JOURNAL-QA: opened ask card" ~/utono/linux-lit/linux-lit-dev*.log | tail -2
```

(Use the `-2` suffixed log if the user's live instance holds slot 1.) Then `wtype -k Escape` twice to return to the reader.

- [ ] **Step 4: Verify Return confirms an ask-entered selection**

```bash
wtype -M ctrl -k a -m ctrl && sleep 1
wtype -k Return && sleep 2
grim /tmp/ask3.png
```

Expected: the ask card again (NOT the Action menu). Escape twice back to the reader.

- [ ] **Step 5: Verify `V`-entered Return still opens the Action menu**

```bash
wtype "V" && sleep 1 && wtype -k Return && sleep 2
grim /tmp/ask4.png
```

Expected: the Action popup (Journal Q&A / Reader Gloss / ... as in Image #11). Escape twice.

- [ ] **Step 6: Verify plain `A` triggers authorship**

```bash
wtype "A" && sleep 1 && grim /tmp/ask5.png
```

Expected: the "No authorship data for this work" toast (Bleak House has none) — proof the rebind dispatches `ToggleAuthorship`.

- [ ] **Step 7: Clean up the cage**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

(ONLY this pattern — never a bare `pkill -f target/debug/linux-lit`, which kills the user's live instance.)

- [ ] **Step 8: Review all screenshots inline**

Open every `/tmp/ask*.png` with Read and report what's on screen (per the UI review protocol). Any mismatch is a bug: stop and fix before merging.

- [ ] **Step 9: Merge per finishing-a-branch**

```bash
cd ~/utono/linux-lit && cargo test --bins 2>&1 | tail -3 && git status --porcelain
git checkout master && git merge --no-ff ask-passage -m "Merge branch 'ask-passage': Ctrl+a paragraph/speech ask card"
cargo build 2>&1 | tail -3 && git push origin master && git branch -d ask-passage
```

Expected: only `test_load_work_hamlet` failing, clean tree, merge + push OK.
