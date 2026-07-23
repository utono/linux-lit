# Ctrl+Tab Ask-Card Focus Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In 2-column float layout, `Ctrl+Tab` toggles input focus between the ask card and the gloss/journal card, so a reader can navigate the left card while composing a question, then return to typing with the draft preserved.

**Architecture:** A single `AppState.ask_card_focus: bool` gates whether the ask-card vim intercept runs (`true`, today's behavior) or is skipped so keys reach the overlay read-nav handler (`false`). `Ctrl+Tab` flips it, but only while the ask card is open in float mode (`AskCard::float_width > 0`). Escape on the left card returns focus to the ask card. The unfocused card gets a `.card-unfocused` CSS class dimming it to `opacity: 0.55`.

**Tech Stack:** Rust, GTK4 (gtk4-rs), the app's existing `handle_gloss_key`/`handle_journal_key` mode handlers and `AskCardHost`/`AskCard` widgets, `theme.rs generate_css`.

## Global Constraints

- Applies to BOTH the gloss overlay and the journal overlay; ONLY when the ask card is open in 2-col float (`AskCard::float_width > 0`). 1-col stacked ask layout is unchanged (Ctrl+Tab stays a no-op there).
- `ask_card_focus` is a single field on the shared `AppState` (at most one overlay is open at a time). Default `true` on ask open; reset to `true` on ask close/submit.
- Unfocused-card dim is exactly `opacity: 0.55` via a `.card-unfocused` class.
- Escape while the left card is focused returns focus to the ask card and does NOT close the overlay. Escape while the ask card is focused is unchanged (single = vim Normal, double = close).
- Keybind lockstep (required by CLAUDE.md): update the gloss + journal overlay Ctrl+/ legends in the same change. Do NOT touch `keymap_config.rs` or the main-card Ctrl+/ overlay — this bind lives only in the overlay modal handlers.
- Build with `cargo build`; do NOT run the app (the user runs it). Verify headless via cage where a step needs on-screen confirmation.
- Bypass shell alias hangs with `command rm -f` / `\cp -f` if needed. Commit messages end with the repo's Co-Authored-By / Claude-Session trailers.

---

### Task 1: `AskCard::is_float()` float-mode query

Expose whether the ask card is in 2-col float layout, so the key handlers can gate the toggle on it. `AskCard` already stores `float_width: Cell<i32>` (set > 0 only in float mode via `set_float_width`).

**Files:**
- Modify: `src/ui/ask_card.rs` (add `AskCard::is_float`; add `AskCardHost::is_ask_float` passthrough)
- Test: `src/ui/ask_card.rs` (inline `#[cfg(test)]`) — NOT added; see Step 1 note.

**Interfaces:**
- Produces: `AskCard::is_float(&self) -> bool` (true iff `float_width.get() > 0`); `AskCardHost::is_ask_float(&self) -> bool` (delegates to `self.card().is_float()`).

- [ ] **Step 1: Add the query methods**

`AskCard::is_float` is a one-line getter over an existing `Cell<i32>`; a GTK-widget unit test would require a GTK init harness this file doesn't have, so this task is verified by compilation + its consumers' headless test in Task 6 (documented deviation from strict TDD per the repo's "state genuinely live-only" allowance). Add to `impl AskCard` (near `container()` at line 150):

```rust
    /// True when the ask card is in 2-column float layout (a fixed-width panel
    /// floated beside the gloss/journal card). False in 1-col stacked layout.
    pub fn is_float(&self) -> bool {
        self.float_width.get() > 0
    }
```

Add to `impl AskCardHost` (near `card()` at line 569):

```rust
    /// True when the hosted ask card is in 2-column float layout.
    pub fn is_ask_float(&self) -> bool {
        self.card().is_float()
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: `Finished` with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/ui/ask_card.rs
git commit -m "feat(ask-card): is_float() query for 2-col float layout"
```

---

### Task 2: `AppState.ask_card_focus` field

Add the focus-state flag to `AppState`, defaulting to `true` (ask card focused).

**Files:**
- Modify: `src/app/mod.rs` (struct field near other overlay state ~line 243–540; initializer in the `AppState { ... }` literal ~line 2053–2340)

**Interfaces:**
- Produces: `AppState.ask_card_focus: bool` — `true` = ask card has input; `false` = left (gloss/journal) card has input. Only meaningful while an ask card is open in float mode.

- [ ] **Step 1: Add the field to the struct**

In `pub struct AppState { ... }` (`src/app/mod.rs`), add near the other overlay-related booleans (e.g. just after the `chapter_toast_*` group around line 787, or wherever overlay flags cluster):

```rust
    /// Ask-card focus in 2-col float layout: `true` = the ask card (vim editor)
    /// has input; `false` = the gloss/journal card underneath has input for
    /// read-nav. Toggled by Ctrl+Tab; only meaningful while an ask card is open
    /// in float mode. Reset to `true` on every ask open and close.
    pub ask_card_focus: bool,
```

- [ ] **Step 2: Initialize it in the AppState literal**

In the `AppState { ... }` construction (`src/app/mod.rs` ~line 2053), add alongside the other simple bool inits (e.g. near `input_mode: InputMode::Reader,` ~line 2339):

```rust
        ask_card_focus: true,
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: `Finished` (no "missing field" error).

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(state): ask_card_focus flag (default true)"
```

---

### Task 3: `.card-unfocused` dim CSS + overlay dim helpers

Add the dim class and the per-overlay helper that applies it to the unfocused card.

**Files:**
- Modify: `src/theme.rs` (`generate_css` — add the `.card-unfocused` rule)
- Modify: `src/ui/gloss_overlay.rs` (add `set_ask_focus_dim`)
- Modify: `src/ui/journal_overlay.rs` (add `set_ask_focus_dim`)

**Interfaces:**
- Consumes: gloss/journal `container: gtk4::Box` (the main card box); `AskCardHost::card().container() -> &gtk4::Box` (the ask float box).
- Produces:
  - CSS class `card-unfocused` (`opacity: 0.55`).
  - `GlossOverlay::set_ask_focus_dim(&self, ask_focused: bool)` — when `ask_focused`, dims the ask float and un-dims the gloss card; when `!ask_focused`, the reverse.
  - `GlossOverlay::clear_focus_dim(&self)` — removes the class from both.
  - Same two methods on `JournalOverlay`.

- [ ] **Step 1: Add the dim CSS**

In `src/theme.rs`, inside the `generate_css` string (alongside the other overlay classes), add:

```rust
               .card-unfocused {{ opacity: 0.55; }} \
```

(Match the existing `{{ }}`-escaped, backslash-continued format of neighboring rules in that string.)

- [ ] **Step 2: Add the gloss helper**

In `impl GlossOverlay` (`src/ui/gloss_overlay.rs`), add:

```rust
    /// Dim whichever of the two 2-col cards does NOT have input focus.
    /// `ask_focused` true → dim the gloss card (left), un-dim the ask float.
    pub fn set_ask_focus_dim(&self, ask_focused: bool) {
        let ask = self.ask_host.card().container();
        if ask_focused {
            self.container.add_css_class("card-unfocused");
            ask.remove_css_class("card-unfocused");
        } else {
            self.container.remove_css_class("card-unfocused");
            ask.add_css_class("card-unfocused");
        }
    }

    /// Remove the focus-dim from both cards (on ask close/submit).
    pub fn clear_focus_dim(&self) {
        self.container.remove_css_class("card-unfocused");
        self.ask_host.card().container().remove_css_class("card-unfocused");
    }
```

- [ ] **Step 3: Add the journal helper**

In `impl JournalOverlay` (`src/ui/journal_overlay.rs`), add the identical pair (the journal overlay's main box is also named `container`):

```rust
    /// Dim whichever of the two 2-col cards does NOT have input focus.
    /// `ask_focused` true → dim the journal card (left), un-dim the ask float.
    pub fn set_ask_focus_dim(&self, ask_focused: bool) {
        let ask = self.ask_host.card().container();
        if ask_focused {
            self.container.add_css_class("card-unfocused");
            ask.remove_css_class("card-unfocused");
        } else {
            self.container.remove_css_class("card-unfocused");
            ask.add_css_class("card-unfocused");
        }
    }

    /// Remove the focus-dim from both cards (on ask close/submit).
    pub fn clear_focus_dim(&self) {
        self.container.remove_css_class("card-unfocused");
        self.ask_host.card().container().remove_css_class("card-unfocused");
    }
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: `Finished`. (If `self.container` is private but methods are in the same `impl`/module, it compiles; if the field name differs, grep `struct JournalOverlay`/`struct GlossOverlay` and use the actual main-box field.)

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs src/ui/gloss_overlay.rs src/ui/journal_overlay.rs
git commit -m "feat(overlay): card-unfocused dim + set_ask_focus_dim helpers"
```

---

### Task 4: Reset focus + clear dim on ask open/close (both overlays)

Ensure `ask_card_focus` returns to `true` and the dim clears every time an ask card opens or closes, so no stale focus/dim leaks across sessions.

**Files:**
- Modify: `src/input/actions/gloss.rs` (`close_gloss_prompt`; and the gloss ask-open sites — `begin_passage_ask`-equivalent / `open_ask_card` callers)
- Modify: `src/input/actions/journal.rs` (`close_prompt`; `begin_ask`, `begin_passage_ask`)

**Interfaces:**
- Consumes: `AppState.ask_card_focus` (Task 2); `GlossOverlay::clear_focus_dim` / `JournalOverlay::clear_focus_dim` (Task 3).

- [ ] **Step 1: Reset on gloss ask open**

In `src/input/actions/gloss.rs`, find each site that opens the ask card (search `open_ask_card` / `open_ask_card_with` / `open_passage_qa_float`). In each, after opening, set focus true (the state is already borrowed mut at these sites; add the line in that scope):

```rust
        s.ask_card_focus = true;
```

If a site holds only `&Rc<RefCell<AppState>>`, use `state.borrow_mut().ask_card_focus = true;`.

- [ ] **Step 2: Reset + clear dim on gloss ask close**

In `close_gloss_prompt` (`src/input/actions/gloss.rs` ~line 3691), before/after the `close_ask_card()` call, add:

```rust
    {
        let mut s = state.borrow_mut();
        s.ask_card_focus = true;
        s.gloss_overlay.clear_focus_dim();
    }
```

(Place it so it does not conflict with an existing borrow in that fn; if the fn already holds a borrow, add the two statements inside it.)

- [ ] **Step 3: Reset on journal ask open**

In `src/input/actions/journal.rs`, in `begin_ask` (~1616) and `begin_passage_ask` (~1581), where `s` is already borrowed mut, add:

```rust
    s.ask_card_focus = true;
```

- [ ] **Step 4: Reset + clear dim on journal ask close**

In `close_prompt` (`src/input/actions/journal.rs` ~2211), add:

```rust
    {
        let mut s = state.borrow_mut();
        s.ask_card_focus = true;
        s.journal_overlay.clear_focus_dim();
    }
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: `Finished` (watch for double-borrow panics — these are compile-clean but must use non-overlapping borrow scopes; if a site already has `let mut s = state.borrow_mut()`, add the field set inside that scope instead of a new borrow).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/gloss.rs src/input/actions/journal.rs
git commit -m "feat(ask-card): reset focus + clear dim on ask open/close"
```

---

### Task 5: Ctrl+Tab flip + intercept skip + Escape-return (both handlers)

The core behavior: in each of `handle_gloss_key` and `handle_journal_key`, catch `Ctrl+Tab` (float + ask-open) to flip focus, skip `ask_vim_intercept` when the left card is focused, and route Escape-on-left-card back to the ask card.

**Files:**
- Modify: `src/input/keymap.rs` (`handle_gloss_key` ~line 2280; `handle_journal_key` ~line 1836)

**Interfaces:**
- Consumes: `AppState.ask_card_focus` (Task 2); `GlossOverlay::is_ask_float`/`JournalOverlay::is_ask_float` — add these thin passthroughs (see Step 1); `set_ask_focus_dim` (Task 3).
- Produces: no new public API; behavior only.

- [ ] **Step 1: Add `is_ask_float` passthrough on both overlays**

In `impl GlossOverlay` (`src/ui/gloss_overlay.rs`) and `impl JournalOverlay` (`src/ui/journal_overlay.rs`), add:

```rust
    /// True when the ask card is open in 2-col float layout.
    pub fn is_ask_float(&self) -> bool {
        self.ask_host.is_ask_float()
    }
```

- [ ] **Step 2: Gloss handler — Ctrl+Tab flip + intercept skip**

In `handle_gloss_key` (`src/input/keymap.rs` ~2287), replace the ask-intercept block:

```rust
    let ask_open = state.borrow().gloss_overlay.ask_is_open();
    match ask_vim_intercept(
        ask_open,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().gloss_overlay.feed_ask_vim_key(k),
        crate::input::actions::gloss::submit_gloss_prompt,
        crate::input::actions::gloss::close_gloss_prompt,
        |st, t| st.borrow().gloss_overlay.paste_ask_text(t),
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::NotHandled => {}
    }
```

with:

```rust
    let ask_open = state.borrow().gloss_overlay.ask_is_open();
    let ask_float = ask_open && state.borrow().gloss_overlay.is_ask_float();
    // Ctrl+Tab toggles focus between the ask card and the gloss card (2-col
    // float only). Caught BEFORE ask_vim_intercept, which would map Ctrl+Tab to
    // a vim Tab and swallow it.
    if ask_float && is_ctrl && (key_name == "Tab" || key_name == "ISO_Left_Tab") {
        let mut s = state.borrow_mut();
        s.ask_card_focus = !s.ask_card_focus;
        let focused = s.ask_card_focus;
        s.gloss_overlay.set_ask_focus_dim(focused);
        return true;
    }
    let ask_focused = state.borrow().ask_card_focus;
    // Escape while the gloss card is focused (left of a 2-col ask) returns focus
    // to the ask card — it does NOT close the overlay.
    if ask_float && !ask_focused && key_name == "Escape" && !is_ctrl {
        let mut s = state.borrow_mut();
        s.ask_card_focus = true;
        s.gloss_overlay.set_ask_focus_dim(true);
        return true;
    }
    // Run the ask-card vim intercept ONLY when the ask card has focus. When the
    // gloss card is focused (Ctrl+Tab in 2-col), fall through to gloss read-nav.
    if ask_open && (!ask_float || ask_focused) {
        match ask_vim_intercept(
            ask_open,
            key_name,
            key_char,
            is_ctrl,
            state,
            |st, k| st.borrow().gloss_overlay.feed_ask_vim_key(k),
            crate::input::actions::gloss::submit_gloss_prompt,
            crate::input::actions::gloss::close_gloss_prompt,
            |st, t| st.borrow().gloss_overlay.paste_ask_text(t),
        ) {
            AskIntercept::Consumed => return true,
            AskIntercept::NotHandled => {}
        }
    }
```

- [ ] **Step 3: Journal handler — same transformation**

In `handle_journal_key` (`src/input/keymap.rs` ~1850), replace the ask-intercept block:

```rust
    let ask_open = state.borrow().journal_overlay.ask_is_open();
    match ask_vim_intercept(
        ask_open,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().journal_overlay.feed_ask_vim_key(k),
        crate::input::actions::journal::submit_prompt,
        crate::input::actions::journal::close_prompt,
        |st, t| st.borrow().journal_overlay.paste_ask_text(t),
    ) {
        AskIntercept::Consumed => return true,
        AskIntercept::NotHandled => {}
    }
```

with:

```rust
    let ask_open = state.borrow().journal_overlay.ask_is_open();
    let ask_float = ask_open && state.borrow().journal_overlay.is_ask_float();
    // Ctrl+Tab toggles focus between the ask card and the journal card (2-col
    // float only). Caught BEFORE ask_vim_intercept (which maps Ctrl+Tab to vim
    // Tab).
    if ask_float && is_ctrl && (key_name == "Tab" || key_name == "ISO_Left_Tab") {
        let mut s = state.borrow_mut();
        s.ask_card_focus = !s.ask_card_focus;
        let focused = s.ask_card_focus;
        s.journal_overlay.set_ask_focus_dim(focused);
        return true;
    }
    let ask_focused = state.borrow().ask_card_focus;
    // Escape while the journal card is focused returns focus to the ask card;
    // it does NOT close the overlay.
    if ask_float && !ask_focused && key_name == "Escape" && !is_ctrl {
        let mut s = state.borrow_mut();
        s.ask_card_focus = true;
        s.journal_overlay.set_ask_focus_dim(true);
        return true;
    }
    // Run the ask-card vim intercept ONLY when the ask card has focus.
    if ask_open && (!ask_float || ask_focused) {
        match ask_vim_intercept(
            ask_open,
            key_name,
            key_char,
            is_ctrl,
            state,
            |st, k| st.borrow().journal_overlay.feed_ask_vim_key(k),
            crate::input::actions::journal::submit_prompt,
            crate::input::actions::journal::close_prompt,
            |st, t| st.borrow().journal_overlay.paste_ask_text(t),
        ) {
            AskIntercept::Consumed => return true,
            AskIntercept::NotHandled => {}
        }
    }
```

- [ ] **Step 4: Remove/replace the stale Ctrl+Tab consumed-no-op arms**

The Ctrl block in each handler still has `"Tab" | "ISO_Left_Tab" => return true` (the old consumed no-op). It is now only reached in the ask-card-focused path (the new flip guard runs first for the float+unfocused case). Leave the arm in place BUT update its comment to note the new Ctrl+Tab toggle takes precedence in 2-col float. In `handle_journal_key` (near the `// Ctrl+Tab: dropped inside overlays` comment) and the gloss equivalent, change the comment to:

```rust
            // Ctrl+Tab: in 2-col float it toggles ask/card focus (handled above,
            // before the ask intercept). Elsewhere (1-col, or reader-side reopen)
            // consumed here so it can't fall through to the plain Tab arm.
            "Tab" | "ISO_Left_Tab" => return true,
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: `Finished` with no errors.

- [ ] **Step 6: Run the full bin test suite (no regressions)**

Run: `cargo test --bins 2>&1 | rg "test result"`
Expected: `ok. N passed; 0 failed` (N ≈ 1050).

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs src/ui/gloss_overlay.rs src/ui/journal_overlay.rs
git commit -m "feat(overlay): Ctrl+Tab toggles ask-card/left-card focus (2-col)"
```

---

### Task 6: Headless verification (gloss + journal 2-col)

Confirm the behavior on-screen via the cage harness: focus toggles, left card navigates, draft preserved, dim visible, Escape returns to ask.

**Files:**
- No source changes. Uses the cage/grim/wtype flow from `linux-lit/CLAUDE.md`.

**Interfaces:**
- Consumes: the built `target/debug/linux-lit`.

- [ ] **Step 1: Build and launch under cage**

```bash
cd ~/utono/linux-lit && cargo build
LIT_LOG_PATH=/tmp/asktoggle.log LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 4
```

Export the cage socket and resize:

```bash
export WAYLAND_DISPLAY=$(command ls -t /run/user/1000/wayland-* | grep -vE '\.lock|wayland-0$' | head -1 | xargs basename) XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

- [ ] **Step 2: Open a gloss rewrite ask in 2-col, type, toggle, navigate**

Drive (confirm exact keys against `keymap_config.rs`; open the gloss overlay, start a rewrite/passage-ask so the float appears, type a few chars, then Ctrl+Tab):

```bash
# (exact open sequence depends on the current work/binds — e.g. open gloss overlay,
#  Ctrl+a passage-ask; type "test"; then:)
wtype -M ctrl -k Tab -m ctrl   # toggle focus to gloss card
sleep 1
wtype -k G                     # left card: jump to end (read-nav)
sleep 1
grim -o HEADLESS-1 /tmp/asktoggle-left.png
```

Read `/tmp/asktoggle-left.png` and confirm: the LEFT gloss card scrolled to the end, the ask float still shows the typed "test" draft, and the ASK FLOAT is dimmed (unfocused).

- [ ] **Step 3: Toggle back + Escape-return**

```bash
wtype -M ctrl -k Tab -m ctrl   # back to ask card
sleep 1
grim -o HEADLESS-1 /tmp/asktoggle-ask.png   # gloss card now dimmed, ask un-dimmed
wtype -M ctrl -k Tab -m ctrl   # to left card again
sleep 1
wtype -k Escape                # returns focus to ask (must NOT close overlay)
sleep 1
grim -o HEADLESS-1 /tmp/asktoggle-esc.png   # ask focused again, overlay still open
```

Read all three PNGs. Confirm: Step-3 first shot has the gloss card dimmed; the Escape shot shows the ask card focused (un-dimmed) with the overlay still open and draft intact.

- [ ] **Step 4: Repeat for the journal Q&A ask (2-col)**

Open a journal Q&A ask (`Ctrl+a` in the journal overlay) so the ask float appears, type, and repeat the Ctrl+Tab / G / Ctrl+Tab / Escape sequence, screenshotting each state. Confirm identical behavior.

- [ ] **Step 5: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 6: Commit (verification notes only, if any harness tweaks were needed)**

No source commit expected. If a nav-fuzz/e2e assertion was extended, commit it here.

---

### Task 7: Keybind-legend lockstep (gloss + journal Ctrl+/ overlays)

Document the new Ctrl+Tab bind in both overlay legends, replacing any stale "Ctrl+Tab consumed no-op" note.

**Files:**
- Modify: `src/ui/gloss_keybinds_overlay.rs` (GROUPS + describe/detail)
- Modify: `src/ui/journal_keybinds_overlay.rs` (GROUPS + describe/detail)

**Interfaces:**
- Consumes: nothing (documentation of Task 5 behavior).

- [ ] **Step 1: Inspect both legend files**

Run: `rg -n "Ctrl.?Tab|C-Tab|Tab|GROUPS|MRU" src/ui/gloss_keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs`
Note the GROUPS array shape and any existing Tab entry.

- [ ] **Step 2: Add/replace the gloss legend entry**

In `src/ui/gloss_keybinds_overlay.rs`, add to the appropriate GROUP a `("C-Tab", "focus ask/card")` entry (match the file's existing tuple/label convention exactly), and its describe/detail string:

```rust
        "focus ask/card" => "Ctrl+Tab (2-col ask): toggle input focus between \
the ask card and the gloss card. Left-card focus is read-nav; Escape there \
returns to the ask card. — src/input/keymap.rs (handle_gloss_key)",
```

- [ ] **Step 3: Add/replace the journal legend entry**

In `src/ui/journal_keybinds_overlay.rs`, add `("C-Tab", "focus ask/card")` to the appropriate GROUP and its describe/detail:

```rust
        "focus ask/card" => "Ctrl+Tab (2-col ask): toggle input focus between \
the ask card and the journal card. Left-card focus is read-nav; Escape there \
returns to the ask card. — src/input/keymap.rs (handle_journal_key)",
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: `Finished`.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs
git commit -m "docs(keybinds): gloss+journal legends note Ctrl+Tab ask/card focus"
```

---

## Self-Review

**Spec coverage:**
- Two-state focus model → Task 2 (field) + Task 5 (gate). ✓
- Ctrl+Tab flip, float-only, before intercept → Task 5 Steps 2–3. ✓
- Skip intercept when left-focused → Task 5 (the `if ask_open && (!ask_float || ask_focused)` guard). ✓
- Escape returns to ask card → Task 5 Steps 2–3. ✓
- Dim unfocused card at 0.55 → Task 3 (CSS + helpers) wired in Task 5 flips + Task 4 clear-on-close. ✓
- Both overlays, 2-col only → `is_ask_float` gate in Task 1/5. ✓
- Init true on open, reset on close → Task 4. ✓
- Keybind legend lockstep, no reader-table touch → Task 7. ✓
- Testing (headless + no-regression) → Task 5 Step 6 + Task 6. ✓
- Non-goals (1-col, chat panel untouched) → the `ask_float` gate leaves 1-col on the old path; no chat-panel file is modified. ✓

**Placeholder scan:** No TBD/TODO; every code step shows the code; the one skipped unit test (Task 1) is justified (GTK-widget getter, covered by headless) per the repo's live-only allowance.

**Type consistency:** `is_float`/`is_ask_float`/`is_ask_float` chain consistent (AskCard → AskCardHost → overlay). `ask_card_focus` bool used identically across Tasks 2/4/5. `set_ask_focus_dim(bool)` / `clear_focus_dim()` signatures match between definition (Task 3) and calls (Tasks 4/5). Main-box field named `container` on both overlays (verified). ✓
