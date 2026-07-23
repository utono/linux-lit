# Vocab `r`-key consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Consolidate all vocab functions onto the `r` key, unify the add-vocab card on `Ctrl+r` across every surface, and move gloss rewrite to `Ctrl+w` so `Ctrl+w` = rewrite everywhere.

**Architecture:** Edit the compiled reader defaults (`keymap_config.rs`), the four modal handlers in `keymap.rs` (gloss, journal, synopsis, chat), the stowed `keymap.json`, and all five affected `Ctrl+/` legend files — in lockstep. Verify with the module's own default-binding assertions plus `cargo build`/`clippy`.

**Tech Stack:** Rust, GTK4; keybind lookup via `KeyCombo`/`Keymap` in `src/input/keymap_config.rs`; per-overlay modal handlers in `src/input/keymap.rs`.

## Global Constraints

- **Source of truth is the Rust source** — never keybinds.db or prose docs.
- **RPD case:** `Ctrl+Shift+<letter>` arrives as **lowercase key name + shift=true**, NOT uppercase. Register both `ctrl_shift("r")` and `ctrl_shift("R")` (mirrors `OpenLastGloss` at `keymap_config.rs:323-324`).
- **`keymap.json` overrides compiled defaults** — update `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` in the same change, then `cd ~/tty-dotfiles && stow linux-lit`. Overlay handlers are direct key-matches in `keymap.rs`, NOT keymap.json-driven — only the main-reader binds need JSON edits.
- **Every keybind change updates all its legends in the same change** (`src/ui/*_keybinds_overlay.rs`).
- **Do NOT run the app** — only `cargo build` / `cargo test --bins` / `cargo clippy`. The user runs `cargo run`.
- Timestamps US Central. Commit messages end with the standard Co-Authored-By/Claude-Session trailer.

---

### Task 1: Reader compiled defaults — `r` becomes the vocab hub

**Files:**
- Modify: `src/input/keymap_config.rs:307-316` (the vocab bind block) and the two test fns `default_reader_bindings` assertions at `:515` and `r_cycles_vocab_and_ctrl_r_asks_journal` at `:528-548`.

**Interfaces:**
- Produces: reader `Ctrl+r` = `Action::AddVocabWord`; `Ctrl+Shift+r`/`R` = `Action::VocabJournalAsk`; `Alt+r` = `Action::ToggleVocabHighlight`. Removes `Ctrl+Alt+\` = AddVocabWord and `Alt+\` = ToggleVocabHighlight.

- [ ] **Step 1: Update the bind block.** In `src/input/keymap_config.rs`, replace lines 307-316. Current:

```rust
        (KeyCombo::plain("r"), Action::VocabPopupTap),
        // Ctrl+r: vocab journal Q&A — ask about the popup's current word
        // (gated on popup visible + a vocab word on the cursor line; was R).
        // Stored answer → journal overlay; fresh ask → held toast, then the
        // overlay on the saved entry. Reader Ctrl+n/p are unbound (the old
        // popup Q&A paging went away with the popup Journal view); the
        // pickers/overlays keep their own modal Ctrl+n/p.
        (KeyCombo::ctrl("r"), Action::VocabJournalAsk),
        (KeyCombo::alt("backslash"), Action::ToggleVocabHighlight),
        (KeyCombo::ctrl_alt("backslash"), Action::AddVocabWord),
```

Replace with:

```rust
        (KeyCombo::plain("r"), Action::VocabPopupTap),
        // The `r` key is the vocab hub: plain = tap the popup word, Ctrl =
        // add a vocab word, Ctrl+Shift = vocab journal Q&A, Alt = toggle the
        // per-work vocab highlight. AddVocabWord moved here off Ctrl+Alt+\ and
        // ToggleVocabHighlight off Alt+\ (both freed) so every vocab function
        // lives on one cap (2026-07-23 consolidation).
        (KeyCombo::ctrl("r"), Action::AddVocabWord),
        // Ctrl+Shift+r: vocab journal Q&A — ask about the popup's current word
        // (gated on popup visible + a vocab word on the cursor line). Stored
        // answer → journal overlay; fresh ask → held toast, then the overlay
        // on the saved entry. RPD emits this as lowercase "r"+shift, so bind
        // both cases (mirrors OpenLastGloss below).
        (KeyCombo::ctrl_shift("r"), Action::VocabJournalAsk),
        (KeyCombo::ctrl_shift("R"), Action::VocabJournalAsk),
        (KeyCombo::alt("r"), Action::ToggleVocabHighlight),
```

- [ ] **Step 2: Update the `default_reader_bindings` test assertions at `:515`.** The `Ctrl+a` = None assertion (line 515) is unchanged and stays. No edit needed there. (VocabJournalAsk did NOT move to Ctrl+a — it went to Ctrl+Shift+r.) Confirm line 515 still reads `assert_eq!(m.get(&KeyCombo::ctrl("a")), None);` — leave it.

- [ ] **Step 3: Update the `r_cycles_vocab_and_ctrl_r_asks_journal` test (`:528-548`).** Replace the vocab assertions. Current relevant lines:

```rust
        assert_eq!(m.get(&KeyCombo::ctrl("r")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::plain("R")), None);
```
and later:
```rust
        assert_eq!(m.get(&KeyCombo::ctrl_shift("R")), None);
```

Replace those three assertions with:

```rust
        assert_eq!(m.get(&KeyCombo::ctrl("r")), Some(&Action::AddVocabWord));
        assert_eq!(m.get(&KeyCombo::plain("R")), None);
```
and:
```rust
        assert_eq!(m.get(&KeyCombo::ctrl_shift("r")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("R")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::alt("r")), Some(&Action::ToggleVocabHighlight));
```

Also rename the test fn to `r_is_the_vocab_hub` for clarity (update the `#[test] fn` name at line 529).

- [ ] **Step 4: Check for any other test asserting the old `Alt+\`/`Ctrl+Alt+\` vocab binds.** Run:

```bash
cd ~/utono/linux-lit && rg -n 'alt\("backslash"\)|ctrl_alt\("backslash"\)' src/input/keymap_config.rs
```
Expected: matches only in the bind table (now removed) and possibly a test. If a test asserts `alt("backslash") => ToggleVocabHighlight` or `ctrl_alt("backslash") => AddVocabWord`, update it to assert `None` (both chords are now free). If no test references them, nothing to change.

- [ ] **Step 5: Build + run the module tests.**

```bash
cd ~/utono/linux-lit && cargo test --bins keymap_config 2>&1 | tail -20
```
Expected: PASS (the updated assertions match the new binds).

- [ ] **Step 6: Commit.**

```bash
cd ~/utono/linux-lit && git add src/input/keymap_config.rs && git commit -m "feat(keybinds): r key = vocab hub (Ctrl+r add, Ctrl+Shift+r Q&A, Alt+r highlight)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 2: Gloss overlay handler — Ctrl+r = add vocab, rewrite → Ctrl+w

**Files:**
- Modify: `src/input/keymap.rs:2446-2452` (gloss `Ctrl+r` arm) and `:2321-2326` (inline `Ctrl+Alt+\` check in `handle_gloss_key`).

**Interfaces:**
- Consumes: `crate::input::actions::vocab_add::open`, `crate::input::actions::gloss::begin_rewrite` (existing fns).
- Produces: gloss `Ctrl+r` → add-vocab card; gloss `Ctrl+w` → begin_rewrite.

- [ ] **Step 1: Replace the gloss `Ctrl+r` arm.** In `src/input/keymap.rs`, current lines 2446-2452:

```rust
            // Ctrl+r: ask-Claude rewrite of the displayed gloss (moved off plain
            // `R`, which is now reserved for the vocab surface, mirroring the
            // main card). Opens the rewrite prompt in INSERT.
            "r" => {
                crate::input::actions::gloss::begin_rewrite(state);
                return true;
            }
```

Replace with:

```rust
            // Ctrl+r: add a vocab word (uniform with the reader + every overlay
            // — 2026-07-23 consolidation). Gloss rewrite moved to Ctrl+w below.
            "r" => {
                crate::input::actions::vocab_add::open(state);
                return true;
            }
            // Ctrl+w: ask-Claude rewrite of the displayed gloss (moved off
            // Ctrl+r, joining the journal/chat rewrite family so Ctrl+w =
            // rewrite on every overlay). Opens the rewrite prompt in INSERT.
            "w" => {
                crate::input::actions::gloss::begin_rewrite(state);
                return true;
            }
```

- [ ] **Step 2: Remove the inline `Ctrl+Alt+\` add-vocab check** at `src/input/keymap.rs:2321-2326`. Current:

```rust
    // Ctrl+Alt+\: open the dedicated add-vocab card OVER the gloss overlay. It
    // floats above the whole overlay chain and restores this mode on close.
    if is_ctrl && is_alt && key_name == "backslash" {
        crate::input::actions::vocab_add::open(state);
        return true;
    }
```

Delete these six lines entirely (add-vocab is now on Ctrl+r in this handler).

- [ ] **Step 3: Verify the gloss handler doesn't already bind `Ctrl+w` to something else.**

```bash
cd ~/utono/linux-lit && rg -n '"w"' src/input/keymap.rs | sed -n '1,40p' | rg '2[34][0-9][0-9]'
```
Expected: the only `"w"` arm in `handle_gloss_key` (roughly lines 2292-2719) is the new one you just added. If an existing `"w"` arm appears in that range, STOP and reconcile before proceeding.

- [ ] **Step 4: Build.**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -15
```
Expected: compiles clean.

- [ ] **Step 5: Commit.**

```bash
cd ~/utono/linux-lit && git add src/input/keymap.rs && git commit -m "feat(keybinds): gloss overlay Ctrl+r=add vocab, rewrite moves to Ctrl+w

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 3: Journal overlay handler — Ctrl+r = add vocab

**Files:**
- Modify: `src/input/keymap.rs:2005-2007` (journal `Ctrl+r` no-op arm) and `:1897-1900` (inline `Ctrl+Alt+\` check in `handle_journal_key`).

**Interfaces:**
- Produces: journal `Ctrl+r` → add-vocab card (was a consumed no-op).

- [ ] **Step 1: Replace the journal `Ctrl+r` no-op arm.** Current lines 2005-2007:

```rust
            // Ctrl+r: moved to Ctrl+a. Consumed no-op so the chord can't fall
            // through to the term-filter intercept or the plain-r vocab arm.
            "r" => return true,
```

Replace with:

```rust
            // Ctrl+r: add a vocab word (uniform with the reader + every overlay
            // — 2026-07-23 consolidation). Ask-a-new-Q&A is on Ctrl+a above.
            "r" => {
                crate::input::actions::vocab_add::open(state);
                return true;
            }
```

- [ ] **Step 2: Remove the inline `Ctrl+Alt+\` add-vocab check** at `src/input/keymap.rs:1895-1900`. Current:

```rust
    // Checked BEFORE the plain Ctrl+\ picker below so the three-modifier chord
    // wins. It floats above the whole overlay chain and restores this mode.
    if is_ctrl && is_alt && key_name == "backslash" {
        crate::input::actions::vocab_add::open(state);
        return true;
    }
```

Delete these lines (add-vocab is now on Ctrl+r here). **Verify the plain `Ctrl+\` picker block that follows (`if is_ctrl && key_name == "backslash"`) still stands** — it must remain, so only remove the `is_ctrl && is_alt` block above it.

- [ ] **Step 3: Build.**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -15
```
Expected: compiles clean.

- [ ] **Step 4: Commit.**

```bash
cd ~/utono/linux-lit && git add src/input/keymap.rs && git commit -m "feat(keybinds): journal overlay Ctrl+r = add vocab word

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 4: Synopsis overlay handler — Ctrl+r = add vocab (split from plain r)

**Files:**
- Modify: `src/input/keymap.rs:2955-2957` (synopsis `r` no-op arm) and `:2926-2932` (inline `Ctrl+Alt+\` check in `handle_synopsis_overlay_key`).

**Interfaces:**
- Produces: synopsis `Ctrl+r` → add-vocab card; plain `r` stays a consumed no-op.

**IMPORTANT:** In `handle_synopsis_overlay_key` the `match key_name` block is NOT gated on `is_ctrl`, so the current `"r" => true` swallows BOTH plain `r` and `Ctrl+r`. Split by adding a ctrl-guarded arm BEFORE the plain arm.

- [ ] **Step 1: Split the synopsis `r` arm.** Current lines 2955-2957:

```rust
        // r: dropped (cross-create: scene ask card; asking happens from the
        // reader). Consumed no-op.
        "r" => true,
```

Replace with:

```rust
        // Ctrl+r: add a vocab word (uniform with the reader + every overlay —
        // 2026-07-23 consolidation; replaces the old Ctrl+Alt+\ trigger).
        "r" if is_ctrl => {
            crate::input::actions::vocab_add::open(state);
            true
        }
        // plain r: dropped (cross-create: scene ask card; asking happens from
        // the reader). Consumed no-op.
        "r" => true,
```

- [ ] **Step 2: Remove the inline `Ctrl+Alt+\` add-vocab check** at `src/input/keymap.rs:2926-2932`. Current:

```rust
    // Ctrl+Alt+\: open the dedicated add-vocab card OVER the synopsis overlay
    // (same as the reader/gloss/journal/chat arms). It floats above the whole
    // overlay chain and restores this mode on close.
    if is_ctrl && is_alt && key_name == "backslash" {
        crate::input::actions::vocab_add::open(state);
        return true;
    }
```

Delete these lines (add-vocab is now on Ctrl+r here).

- [ ] **Step 3: Build.**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -15
```
Expected: compiles clean.

- [ ] **Step 4: Commit.**

```bash
cd ~/utono/linux-lit && git add src/input/keymap.rs && git commit -m "feat(keybinds): synopsis overlay Ctrl+r = add vocab (plain r still no-op)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 5: Chat panel handler — Ctrl+r = add vocab

**Files:**
- Modify: `src/input/keymap.rs:1709-1720` (chat `"r" if is_ctrl` arm in `handle_chat_transcript_key`).

**Interfaces:**
- Produces: chat `Ctrl+r` → add-vocab card. Orphans nothing: gloss-view regloss stays on `Ctrl+w`, journal-view ask stays on plain `a`.

- [ ] **Step 1: Replace the chat `Ctrl+r` arm.** Current lines 1709-1720:

```rust
        // Ctrl+r: re-gloss / ask (the OLD plain-`r` body, moved off `r` — which
        // is now the vocab surface, mirroring the gloss/journal overlays + main
        // card). Journal view asks a NEW question (the panel's own ask input,
        // same as `a`); other views regloss. Ctrl+r is free in this handler.
        "r" if is_ctrl && !is_shift => {
            if state.borrow().chat.view == crate::input::actions::chat::PanelView::Journal {
                crate::input::actions::chat::focus_prompt_insert(&mut state.borrow_mut());
            } else {
                crate::input::actions::chat::regloss_pinned(state);
            }
            true
        }
```

Replace with:

```rust
        // Ctrl+r: add a vocab word (uniform with the reader + every overlay —
        // 2026-07-23 consolidation). Nothing is orphaned: journal-view ask is
        // still on plain `a` (focus_prompt_insert), and gloss-view regloss is
        // still on Ctrl+w below.
        "r" if is_ctrl && !is_shift => {
            crate::input::actions::vocab_add::open(state);
            true
        }
```

- [ ] **Step 2: Build.**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -15
```
Expected: compiles clean.

- [ ] **Step 3: Commit.**

```bash
cd ~/utono/linux-lit && git add src/input/keymap.rs && git commit -m "feat(keybinds): chat panel Ctrl+r = add vocab word

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 6: keymap.json overrides (stowed)

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` lines 10, 82-83 (and add new r entries).

**Interfaces:**
- Produces: JSON reader binds matching the compiled defaults from Task 1.

- [ ] **Step 1: Update the `r` entries (lines 82-83).** Current:

```json
    {"key": "r", "action": "VocabPopupTap"},
    {"key": "r", "ctrl": true, "action": "VocabJournalAsk"},
```

Replace with:

```json
    {"key": "r", "action": "VocabPopupTap"},
    {"key": "r", "ctrl": true, "action": "AddVocabWord"},
    {"key": "r", "ctrl": true, "shift": true, "action": "VocabJournalAsk"},
    {"key": "r", "alt": true, "action": "ToggleVocabHighlight"},
```

- [ ] **Step 2: Remove the `Alt+\` ToggleVocabHighlight entry (line 10).** Current:

```json
    {"key": "backslash", "alt": true, "action": "ToggleVocabHighlight"},
```

Delete this line. (`Ctrl+Alt+\` AddVocabWord is not present in the JSON per the earlier survey, so nothing to remove there; if `rg '"backslash".*ctrl.*alt|AddVocabWord' keymap.json` finds one, remove it too.)

- [ ] **Step 3: Validate JSON + deploy.**

```bash
cd ~/tty-dotfiles && python3 -m json.tool linux-lit/.config/linux-lit/keymap.json > /dev/null && echo "JSON OK" && stow linux-lit && echo "stowed"
```
Expected: `JSON OK` then `stowed`. Confirm the symlink resolves:

```bash
rg -n '"key"\s*:\s*"r"' ~/.config/linux-lit/keymap.json
```
Expected: the four new r entries appear (symlink points at the stowed file).

- [ ] **Step 4: Commit the dotfiles repo.**

```bash
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "linux-lit: keymap.json — r key vocab hub (add/Q&A/highlight)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 7: Update all five Ctrl+/ legends

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (main card: lines 60, 63, and the describe()/short-label arms 330-336, 485-486), `src/ui/gloss_keybinds_overlay.rs:14-18`, `src/ui/journal_keybinds_overlay.rs:10,21`, `src/ui/synopsis_keybinds_overlay.rs:11,16`, `src/ui/chat_keybinds_overlay.rs:33`.

**Interfaces:**
- Produces: legends matching the new binds. No runtime code — text only.

- [ ] **Step 1: Main-card keycap strip (`keybinds_overlay.rs:60` and `:63`).** Line 60 current:

```rust
    key("r", "R", "vocab tap", "", &[("C-r", "vocab Q&A")]),
```
Replace with:

```rust
    key("r", "R", "vocab tap", "", &[("C-r", "add vocab"), ("S-C-r", "vocab Q&A"), ("M-r", "vocab hi")]),
```

Line 63 current:

```rust
    key("\\", "#", "cycle overlays", "", &[("C-\\", "lib picker"), ("M-\\", "vocab hi"), ("C-M-\\", "add vocab")]),
```
Replace with:

```rust
    key("\\", "#", "cycle overlays", "", &[("C-\\", "lib picker")]),
```

- [ ] **Step 2: Main-card describe() arms.** The `"vocab hi"`, `"add vocab"`, `"vocab tap"`, `"vocab Q&A"` describe strings (lines 330-336) already exist and remain valid (the labels are reused). No change needed there — they key off the label text, which still appears. Verify:

```bash
cd ~/utono/linux-lit && rg -n '"add vocab"|"vocab Q&A"|"vocab hi"' src/ui/keybinds_overlay.rs
```
Expected: the describe() long-form (330-336) and short-label (485-486) arms all still present. No edit — the labels `add vocab`, `vocab Q&A`, `vocab hi` are all now used on the `r` key strip and resolve correctly.

- [ ] **Step 3: Gloss legend (`gloss_keybinds_overlay.rs:14-18`).** Current:

```rust
    ("Ctrl+r", "begin_rewrite: Claude rewrite of this gloss"),
    ("Ctrl+Shift+r", "browse_restore: the viewed revision"),
    ("Ctrl+Shift+n / Ctrl+Shift+p", "browse_step: rewrite_revisions (view-only)"),
    ("r", "vocab_popup (rr toggles · r next word)"),
    ("Ctrl+Alt+\\", "vocab_add_card: add a vocab word"),
```
Replace with:

```rust
    ("Ctrl+r", "vocab_add_card: add a vocab word"),
    ("Ctrl+w", "begin_rewrite: Claude rewrite of this gloss"),
    ("Ctrl+Shift+r", "browse_restore: the viewed revision"),
    ("Ctrl+Shift+n / Ctrl+Shift+p", "browse_step: rewrite_revisions (view-only)"),
    ("r", "vocab_popup (rr toggles · r next word)"),
```

- [ ] **Step 4: Journal legend (`journal_keybinds_overlay.rs:10,21`).** Update the doc comment at line 10-11 — current:

```rust
/// work in this overlay's handler — Ctrl+r and Alt+g are consumed no-ops in
/// handle_journal_key (moved to Ctrl+a / dropped), so they are NOT listed.
```
Replace with:

```rust
/// work in this overlay's handler — Alt+g is a consumed no-op in
/// handle_journal_key (dropped), so it is NOT listed. Ctrl+r = add vocab word.
```

And line 21 — current:

```rust
    ("Ctrl+Alt+\\", "vocab_add_card: add a vocab word"),
```
Replace with:

```rust
    ("Ctrl+r", "vocab_add_card: add a vocab word"),
```

- [ ] **Step 5: Synopsis legend (`synopsis_keybinds_overlay.rs:11,16`).** Update the doc comment at line 10-13 — current:

```rust
/// work in this overlay's handler — of the shared MRU set only Alt+g does
/// (r, Ctrl+r, Ctrl+Shift+n/p/r, Ctrl+a, Ctrl+f, and `\` are consumed no-ops
/// in handle_synopsis_overlay_key; the synopsis left the `\` cycle lap), so
/// only it is listed.
```
Replace with:

```rust
/// work in this overlay's handler — of the shared MRU set Alt+g and Ctrl+r do
/// (r, Ctrl+Shift+n/p/r, Ctrl+a, Ctrl+f, and `\` are consumed no-ops in
/// handle_synopsis_overlay_key; the synopsis left the `\` cycle lap), so only
/// those are listed. Ctrl+r = add vocab word.
```

And line 16 — current:

```rust
    ("Ctrl+Alt+\\", "vocab_add_card: add a vocab word"),
```
Replace with:

```rust
    ("Ctrl+r", "vocab_add_card: add a vocab word"),
```

- [ ] **Step 6: Chat legend (`chat_keybinds_overlay.rs:33`).** Current:

```rust
        ("Ctrl+r", "Gloss view: regloss_pinned · Journal view: ask"),
```
Replace with:

```rust
        ("Ctrl+r", "add vocab word"),
```

- [ ] **Step 7: Build.**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -15
```
Expected: compiles clean (legends are const data — a typo would fail to build).

- [ ] **Step 8: Commit.**

```bash
cd ~/utono/linux-lit && git add src/ui/keybinds_overlay.rs src/ui/gloss_keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs src/ui/synopsis_keybinds_overlay.rs src/ui/chat_keybinds_overlay.rs && git commit -m "docs(legends): reflect r-key vocab hub + Ctrl+w gloss rewrite

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01KC7tPXTGfSoq8A9pgTiK4M"
```

---

### Task 8: Final verification sweep

**Files:** none (verification only).

- [ ] **Step 1: Full build + clippy + bin tests.**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -15 && cargo clippy 2>&1 | tail -10
```
Expected: build clean, tests PASS, clippy no new warnings on the touched files.

- [ ] **Step 2: Grep-confirm no stale `Ctrl+Alt+\` add-vocab checks remain in the four overlay handlers.**

```bash
cd ~/utono/linux-lit && rg -n 'is_ctrl && is_alt && key_name == "backslash"' src/input/keymap.rs
```
Expected: NO matches (all four inline checks removed). If any remain, they belong to a handler not in scope — verify before concluding.

- [ ] **Step 3: Grep-confirm no legend still advertises the old chords.**

```bash
cd ~/utono/linux-lit && rg -n 'Ctrl\+Alt\+\\\\|vocab Q&A.*C-r|C-r.*vocab Q&A|begin_rewrite.*Ctrl\+r' src/ui/*_keybinds_overlay.rs
```
Expected: NO matches tying `Ctrl+r` to rewrite/Q&A or `Ctrl+Alt+\` to add-vocab.

- [ ] **Step 4: Run the three-pass cross-reference** via the `update-cairo-keybinds-overlay` skill guidance — confirm `keymap_config.rs`, all `ui/*_keybinds_overlay.rs`, and the stowed `keymap.json` agree on the r/w binds.

- [ ] **Step 5: Update the consistency-guide change log** if not already accurate (it was written pre-implementation; confirm the 2026-07-23 entry matches what shipped). No code change.

- [ ] **Step 6: On-screen acceptance (headless).** Per the project's testing rule, drive the reader headlessly OR hand off to the user. Headless: launch under cage, tap a vocab word (plain `r`), press `Ctrl+r`, confirm the add-vocab card opens; open the gloss overlay (`Ctrl+g`), press `Ctrl+w`, confirm the rewrite prompt opens. Use the `test-headless-navigation` harness / cage flow from CLAUDE.md. If headless won't launch, hand the user exact steps.

- [ ] **Step 7: No separate commit** — all code committed per-task. This task is verification only.
