# Shared ask-card key intercept — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three duplicated ask-card key-intercept blocks in `src/input/keymap.rs` with one private helper, with zero behavior change.

**Architecture:** Add a private `AskIntercept` enum and a private `ask_card_intercept(...)` free function to `src/input/keymap.rs`. The helper owns only the ask-card chord keys while the card is open (Tab / Ctrl+Enter / Esc) plus the Ask-focus fall-through; each handler keeps its own closed-card overlay-close logic. Convert all three call sites (`handle_journal_key`, `handle_gloss_key`, `handle_synopsis_overlay_key`) in the same task so the change always compiles.

**Tech Stack:** Rust, GTK4 (`gtk4` crate), `Rc<RefCell<AppState>>` key routing.

**Spec:** `docs/superpowers/specs/2026-06-22-ask-card-key-intercept-design.md`

## Global Constraints

- **No behavior change.** Every key in every overlay state (card open vs closed, Doc-focus vs Ask-focus) must do exactly what it does today. Pure extraction; verify behavioral equivalence line-by-line against the three original blocks.
- **No keybind added/removed/changed**, so do **NOT** touch `src/ui/keybinds_overlay.rs`, `src/input/keymap_config.rs`, or `keymap.json`.
- Helper lives in `src/input/keymap.rs`, **not** `src/ui/ask_card.rs` (that module knows nothing about `AppState` / key names / action fns).
- The closure order is fixed everywhere: `(toggle, submit, close)`.
- `cargo build` and `cargo clippy` must be clean; `cargo test --bins` must stay green.
- **No new unit test** — the helper needs a GTK `AppState` to exercise and is not unit-testable in this harness; a fake test would assert nothing. Verification = build + clippy + reviewer equivalence check + the user's cage pass.
- Bash/CLI rules (CLAUDE.md): use `rg`/`fd`, not `grep`/`find`; bypass `mv`/`cp`/`rm` aliases with `\mv -f`/`\cp -f`/`command rm -f` if needed.

---

### Task 1: Extract `ask_card_intercept` and convert all three call sites

**Files:**
- Modify: `src/input/keymap.rs` (add helper near the other private handlers; convert blocks in `handle_journal_key` ~655–678, `handle_gloss_key` ~780–808, `handle_synopsis_overlay_key` ~1148–1191)

**Interfaces:**
- Consumes: `crate::ui::ask_card::AskFocus` (existing enum, variants `Doc` / `Ask`); the existing overlay methods `journal_overlay.ask_is_open()` / `.ask_focus()` / `.toggle_ask_focus()` and `gloss_overlay.ask_is_open()` / `.ask_focus()` / `.toggle_ask_focus()`; the existing action fns `journal::submit_prompt` / `journal::close_prompt`, `gloss::submit_gloss_prompt` / `gloss::close_gloss_prompt`, `synopsis::submit_amend_prompt` / `synopsis::close_amend_prompt`.
- Produces: a private `enum AskIntercept { Consumed, FallThrough, NotHandled }` and `fn ask_card_intercept(...)` used only within `keymap.rs`.

- [ ] **Step 1: Read the three current blocks to confirm exact wording before editing**

The implementer must read all three blocks first and treat them as the source of truth. Their current shape (for reference — do not assume, re-read):

journal (`keymap.rs` ~657–678):
```rust
    // ---- Ask/edit input card intercepts Tab / Ctrl+Enter / Escape first ----
    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.journal_overlay.ask_is_open(), s.journal_overlay.ask_focus())
    };
    if ask_open {
        if key_name == "Tab" || key_name == "ISO_Left_Tab" {
            state.borrow().journal_overlay.toggle_ask_focus();
            return true;
        }
        if is_ctrl && key_name == "Return" {
            crate::input::actions::journal::submit_prompt(state);
            return true;
        }
        if key_name == "Escape" {
            crate::input::actions::journal::close_prompt(state);
            return true;
        }
        if ask_focus == AskFocus::Ask {
            return false;
        }
    }
```

gloss (`keymap.rs` ~787–808): identical shape with `gloss_overlay`, `gloss::submit_gloss_prompt`, `gloss::close_gloss_prompt`, and a leading comment block.

synopsis (`keymap.rs` ~1150–1191): different — reads `(ask_open, ask_focus)` from `gloss_overlay`, then handles `Tab` unconditionally (consume), `Ctrl+Return` only when `ask_open` (`synopsis::submit_amend_prompt`), `Escape` two-stage (`ask_open` → `synopsis::close_amend_prompt`; else `gloss_overlay.hide()` + `input_mode = InputMode::Reader`), then `if ask_open && ask_focus == AskFocus::Ask { return false; }`.

- [ ] **Step 2: Add the `AskIntercept` enum and `ask_card_intercept` helper**

Place both immediately above `fn handle_journal_key` in `src/input/keymap.rs`. Use the existing module-level `use` of `AskFocus` or fully-qualify it.

```rust
/// Outcome of the shared ask-card key intercept.
enum AskIntercept {
    /// The helper consumed the key (Tab / Ctrl+Enter / Esc-while-open) — the
    /// calling handler must `return true`.
    Consumed,
    /// The ask card holds focus and the key is a plain character — the calling
    /// handler must `return false` so GTK delivers it to the editable input.
    FallThrough,
    /// Not an ask-card key, or the card is closed — the calling handler
    /// continues its own routing.
    NotHandled,
}

/// Intercept the ask-card chord keys when `ask_open`. `toggle` / `submit` /
/// `close` are the calling overlay's own actions. Esc-when-closed is
/// intentionally NOT handled here; the helper returns `NotHandled` so the
/// caller's existing overlay-close path runs unchanged.
fn ask_card_intercept(
    ask_open: bool,
    ask_focus: crate::ui::ask_card::AskFocus,
    key_name: &str,
    is_ctrl: bool,
    state: &Rc<RefCell<AppState>>,
    toggle: impl Fn(&Rc<RefCell<AppState>>),
    submit: impl Fn(&Rc<RefCell<AppState>>),
    close: impl Fn(&Rc<RefCell<AppState>>),
) -> AskIntercept {
    use crate::ui::ask_card::AskFocus;
    if !ask_open {
        return AskIntercept::NotHandled;
    }
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        toggle(state);
        return AskIntercept::Consumed;
    }
    if is_ctrl && key_name == "Return" {
        submit(state);
        return AskIntercept::Consumed;
    }
    if key_name == "Escape" {
        close(state);
        return AskIntercept::Consumed;
    }
    if ask_focus == AskFocus::Ask {
        return AskIntercept::FallThrough;
    }
    AskIntercept::NotHandled
}
```

- [ ] **Step 3: Convert the journal call site**

In `handle_journal_key`, replace the block from `// ---- Ask/edit input card intercepts ...` through the closing `}` of `if ask_open { ... }` (the journal block in Step 1) with:

```rust
    // ---- Ask/edit input card intercepts Tab / Ctrl+Enter / Escape first ----
    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.journal_overlay.ask_is_open(), s.journal_overlay.ask_focus())
    };
    match ask_card_intercept(
        ask_open,
        ask_focus,
        key_name,
        is_ctrl,
        state,
        |st| st.borrow().journal_overlay.toggle_ask_focus(),
        crate::input::actions::journal::submit_prompt,
        crate::input::actions::journal::close_prompt,
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::FallThrough => return false,
        AskIntercept::NotHandled => {}
    }
```

Leave the rest of `handle_journal_key` (the `"Escape" => journal::close_overlay` match arm) untouched. The `use crate::ui::ask_card::AskFocus;` at the top of the fn may now be unused there; if `cargo build`/`clippy` warns, remove that line from `handle_journal_key` (the helper has its own `use`).

- [ ] **Step 4: Convert the gloss call site**

In `handle_gloss_key`, replace the analogous `if ask_open { ... }` block (the gloss block, ~787–808; keep its leading comment) with:

```rust
    // ---- Stacked add/edit input card (A / E) ------------------------------
    // When open it behaves like the synopsis ask card: Tab toggles focus,
    // Ctrl+Enter submits, Esc closes the card; typed characters fall through to
    // the editable input while it holds focus. Handled before gloss nav keys.
    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.gloss_overlay.ask_is_open(), s.gloss_overlay.ask_focus())
    };
    match ask_card_intercept(
        ask_open,
        ask_focus,
        key_name,
        is_ctrl,
        state,
        |st| st.borrow().gloss_overlay.toggle_ask_focus(),
        crate::input::actions::gloss::submit_gloss_prompt,
        crate::input::actions::gloss::close_gloss_prompt,
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::FallThrough => return false,
        AskIntercept::NotHandled => {}
    }
```

Leave the gloss handler's `"Escape" | "n" | "bracketright"` match arm (hide + `InputMode::Reader`) untouched. Remove the now-unused `use AskFocus;` at the top of `handle_gloss_key` if build/clippy warns.

- [ ] **Step 5: Convert the synopsis call site**

In `handle_synopsis_overlay_key`, replace the block from the `let (ask_open, ask_focus) = ...` read (~1150) through the `if ask_open && ask_focus == AskFocus::Ask { return false; }` (~1191) with:

```rust
    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.gloss_overlay.ask_is_open(), s.gloss_overlay.ask_focus())
    };

    // Open-card chord keys go through the shared helper (Tab toggles focus,
    // Ctrl+Enter submits, Esc closes the card, Ask-focus falls through).
    match ask_card_intercept(
        ask_open,
        ask_focus,
        key_name,
        is_ctrl,
        state,
        |st| st.borrow().gloss_overlay.toggle_ask_focus(),
        crate::input::actions::synopsis::submit_amend_prompt,
        crate::input::actions::synopsis::close_amend_prompt,
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::FallThrough => return false,
        AskIntercept::NotHandled => {}
    }

    // Closed-card overlay-level semantics (preserved verbatim):
    // Tab is always consumed so it never reaches playback toggle.
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        return true;
    }
    // Escape with the card closed hides the overlay and returns to Reader.
    if key_name == "Escape" {
        let mut s = state.borrow_mut();
        s.gloss_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        return true;
    }
```

**Equivalence note for the implementer/reviewer:** the original synopsis block handled Tab unconditionally, Ctrl+Return only when `ask_open`, Esc two-stage, then the Ask-focus fall-through. The replacement preserves this exactly:
- Card **open**: helper returns `Consumed` for Tab/Ctrl+Return/Esc and `FallThrough` for Ask-focus, short-circuiting before the closed-card lines.
- Card **closed**: helper returns `NotHandled`; the explicit Tab line consumes Tab; the explicit Esc line hides + resets `InputMode`.
- Ctrl+Return when **closed**: original did nothing special (no `ask_open` guard hit); helper returns `NotHandled` and it falls through to the rest of the handler — same as before.

Remove the now-unused `use AskFocus;` at the top of `handle_synopsis_overlay_key` if build/clippy warns.

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: `Finished` with no errors. Resolve any unused-import warnings flagged for the three handlers by deleting their local `use crate::ui::ask_card::AskFocus;` lines (the helper carries its own).

- [ ] **Step 7: Clippy**

Run: `cargo clippy`
Expected: no new warnings introduced by this change.

- [ ] **Step 8: Pure-logic test suite stays green**

Run: `cargo test --bins`
Expected: same pass count as before the change (no new tests added).

- [ ] **Step 9: Commit**

```bash
git add src/input/keymap.rs
git commit -m "refactor(keymap): extract shared ask_card_intercept helper

Replace the three duplicated ask-card intercept blocks (journal/gloss/
synopsis) with one private ask_card_intercept(...) + AskIntercept enum.
Helper owns only the open-card chord keys (Tab/Ctrl+Enter/Esc) and the
Ask-focus fall-through; each handler keeps its closed-card overlay-close.
Pure extraction, no behavior change, no keybind change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

## Verification (after the task)

- `cargo build` + `cargo clippy` clean, `cargo test --bins` green.
- Reviewer confirms behavioral equivalence against the three original blocks (especially the synopsis closed-card Tab/Esc ordering).
- **User cage pass** (the visual/runtime acceptance criterion): in each of journal, gloss, synopsis — open the ask card, confirm Tab toggles focus, typing reaches the field, Ctrl+Enter submits, Esc closes the card; with the card closed, confirm Esc closes the overlay (and synopsis still swallows Tab).
