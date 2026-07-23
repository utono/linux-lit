# Journal Q&A Ask-Card Float (2-col) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Float the journal overlay's ask card to the right as a 2-column panel (identical wiring to the gloss overlay), which activates the already-shipped `Ctrl+Tab` focus toggle in `handle_journal_key` with no key-handler changes.

**Architecture:** The journal overlay currently appends its ask card into the vertical box (stacked) and calls `set_input_fill_fraction(0.75)`. The gloss overlay instead adds its ask card as an `add_overlay` sibling and calls `ask_host.enable_float(width, reserve)` with a reservation closure that shifts the journal card left / the ask panel right (centered pair). This plan converts the journal overlay from stacked to float by mirroring the gloss pattern, adds the missing `clear_focus_dim` on the journal `hide()` funnel (the stale-dim leak that becomes live once the journal floats), resets the focus bool on the `\` close funnel, and documents the now-active bind.

**Tech Stack:** Rust, GTK4 (gtk4-rs), the app's `AskCardHost`/`AskCard` float infrastructure (already supports both modes), `JournalOverlay` (`src/ui/journal_overlay.rs`), the shared cage/grim/wtype headless harness.

## Global Constraints

- Scope: JOURNAL overlay only. Do NOT touch the gloss overlay, the synopsis overlay/handler, or `keymap.rs` (the Ctrl+Tab toggle logic already handles the journal overlay — it was only dormant because `is_ask_float()` returned false).
- The journal ask card floats identically to the gloss ask: `float_w = (column_width as i32 * 5 / 8).max(360)`, right-side placement, the centered-pair reservation closure (journal card `margin_end`, ask panel `margin_start`, ask height capped to the journal card height), and the ask container added as a non-measured, clipped `add_overlay` sibling.
- Drop `ask_host.set_input_fill_fraction(0.75)` (stacked-only behavior) and remove `container.append(ask.container())` (stacked box placement).
- Required leak fix: add `self.clear_focus_dim();` to `JournalOverlay::hide()` (journal_overlay.rs:986) — the journal has the same stale-dim leak the gloss overlay had (only `close_prompt` clears the dim today; `cycle_from_journal` and the Ctrl+j `toggle_overlay` close route through `hide()` without clearing it). Also reset `ask_card_focus = true` in `cycle_from_journal` (inside its existing `borrow_mut` scope).
- The accepted visual change: the journal ask becomes a right-side floated panel (not a tall stacked box) for ALL journal asks. This is intended.
- Build with `cargo build`; do NOT run the app (the user runs it); do NOT run `cargo run`. Headless-verify via cage where a step needs on-screen confirmation.
- Shell aliases hang non-interactively: bypass with `command rm -f` / `\cp -f`.
- Commit messages end with the trailers:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01M7TTE768j8p7NxgjzDyEqQ`

---

### Task 1: Float the journal ask card (new() + attach()) and add width_request

Convert the journal overlay's ask card from stacked to floated, mirroring the gloss overlay. This is the structural core.

**Files:**
- Modify: `src/ui/journal_overlay.rs` — `new()` (container width; ask construction/placement; float wiring) and `attach()` (add ask as overlay sibling)

**Interfaces:**
- Consumes: `AskCardHost::new(ask, &scrolled, Some(footer), recompute)`, `AskCardHost::enable_float(float_w: i32, reserve: Rc<dyn Fn(i32)>)`, `AskCard::container() -> &gtk4::Box`, `column_width: u32` param of `new()`.
- Produces: after this task, `JournalOverlay::is_ask_float()` (already exists, delegates to `ask_host.is_ask_float()`) returns true whenever the journal ask card is open.

- [ ] **Step 1: Add `width_request(column_width)` to the journal container**

The reservation math needs the journal card to have a fixed width (the gloss container sets this; the journal one does not). In `new()` (`src/ui/journal_overlay.rs` ~line 312), the container is created:

```rust
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("gloss-overlay");
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_visible(false);
```

Add a width_request right after `set_valign`:

```rust
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("gloss-overlay");
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_width_request(column_width as i32);
        container.set_visible(false);
```

- [ ] **Step 2: Capture the ask container, remove the stacked append**

Find (near line 532):

```rust
        // Shared "ask" input card (canonical synopsis values), stacked last in
        // the column. Focus returns to the page view when leaving the input.
        let ask = AskCard::new(text_margins as i32, &view);
        container.append(ask.container());
```

Replace with (capture the container clone for the reservation closure; do NOT append it to the box — it becomes an overlay sibling in `attach()`):

```rust
        // Shared "ask" input card. Floats to the RIGHT of the journal card in a
        // 2-column layout (mirrors the gloss overlay), so Ctrl+Tab can toggle
        // focus between the two. Added as an add_overlay sibling in `attach()`,
        // NOT appended to the column.
        let ask = AskCard::new(text_margins as i32, &view);
        let ask_container_for_reserve = ask.container().clone();
```

- [ ] **Step 3: Replace set_input_fill_fraction with enable_float**

Find (near line 549):

```rust
        let ask_host =
            AskCardHost::new(ask, &scrolled, Some(footer_container.clone()), recompute);
        // The journal Q&A input box fills 3/4 of the overlay height (the reading
        // page shrinks to the remaining quarter above it). Gloss/synopsis keep
        // the default compact input.
        ask_host.set_input_fill_fraction(0.75);
```

Replace with (drop the fill-fraction; add the gloss-identical float wiring):

```rust
        let ask_host =
            AskCardHost::new(ask, &scrolled, Some(footer_container.clone()), recompute);
        // Journal ask floats to the RIGHT of the journal card (2-column layout,
        // mirroring the gloss overlay). Fixed float width. On open the journal
        // card reserves margin_end = ask_width + seam (shifts it left by half);
        // the ask panel reserves margin_start = journal_width + seam (shifts it
        // right by half). Both children are halign=Center, so the two mirrored
        // allocation boxes yield equal L/R gutters (the centered pair).
        let float_w = (column_width as i32 * 5 / 8).max(360);
        {
            let container_for_reserve = container.clone();
            let reserve = Rc::new(move |px: i32| {
                // `px` is the reservation (ask_width + seam) on open, 0 on close.
                container_for_reserve.set_margin_end(px);
                if px > 0 {
                    let journal_w = container_for_reserve
                        .width()
                        .max(container_for_reserve.width_request());
                    let seam = px - float_w; // reservation minus ask width
                    ask_container_for_reserve.set_margin_start(journal_w + seam);
                    // Cap the ask panel height to the journal card's height so the
                    // two columns are top/bottom aligned.
                    let h = container_for_reserve
                        .height()
                        .max(container_for_reserve.height_request());
                    ask_container_for_reserve.set_height_request(h.max(200));
                } else {
                    ask_container_for_reserve.set_margin_start(0);
                    ask_container_for_reserve.set_height_request(-1);
                }
            }) as Rc<dyn Fn(i32)>;
            ask_host.enable_float(float_w, reserve);
        }
```

- [ ] **Step 4: Add the ask container as an overlay sibling in attach()**

Find the journal `attach()` (near line 651):

```rust
    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }
```

Replace with (mirror the gloss `attach()`):

```rust
    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
        // The ask card floats as a right-anchored sibling of the journal card
        // (2-column ask layout). Added last so it paints above the card; hidden
        // until an ask flow opens it. Not measured (its size is fixed by the
        // float width + height reservation) and clipped like the container.
        let ask_container = self.ask_host.card().container();
        self.overlay.add_overlay(ask_container);
        self.overlay.set_measure_overlay(ask_container, false);
        self.overlay.set_clip_overlay(ask_container, true);
    }
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: `Finished` with no errors. (If a borrow/move error appears on `ask` — the `AskCard::new` result — confirm Step 2 captured `ask.container().clone()` BEFORE `AskCardHost::new(ask, ...)` consumes `ask`, exactly as gloss_overlay.rs does.)

- [ ] **Step 6: Check for a now-dead `size`/fill-fraction dependency**

The stacked layout used `set_input_fill_fraction`; float sizing comes from the reservation closure. Run:
`rg -n "input_fill_fraction|ask_host.size\(" src/ui/journal_overlay.rs`
If `set_input_fill_fraction` no longer appears (correct) and an `ask_host.size(...)` call remains, leave it — `size` is the open-time viewport/footer bookkeeping shared by both modes and is not fill-fraction-specific. Do NOT remove `ask_host.size(...)`. This step is a verification, not an edit.

- [ ] **Step 7: Commit**

```bash
git add src/ui/journal_overlay.rs
git commit -m "feat(journal): float the ask card 2-col (mirror gloss); enables Ctrl+Tab"
```

---

### Task 2: Fix the stale-dim leak on journal close funnels

Now that the journal ask floats, the `card-unfocused` dim can actually be applied — so the same leak the gloss overlay had becomes live: `JournalOverlay::hide()` does not clear the dim, and two close paths route through it without clearing.

**Files:**
- Modify: `src/ui/journal_overlay.rs` — `hide()`
- Modify: `src/input/actions/overlay_cycle.rs` — `cycle_from_journal`

**Interfaces:**
- Consumes: `JournalOverlay::clear_focus_dim(&self)` (already exists), `AppState.ask_card_focus: bool` (already exists).

- [ ] **Step 1: Add clear_focus_dim to JournalOverlay::hide()**

Find `hide()` (`src/ui/journal_overlay.rs` ~line 986). It currently hides the container/scrim and closes the ask card. Add `self.clear_focus_dim();` as the last statement of the method body. Example (match the actual current body — it may also reset other widgets; just append the clear_focus_dim call at the end):

```rust
    pub fn hide(&self) {
        // ... existing hide body (container/scrim hidden, ask card closed) ...
        // Universal close funnel: clear any stale focus-dim (card-unfocused)
        // left by a Ctrl+Tab focus toggle so the overlay never reopens dimmed.
        self.clear_focus_dim();
    }
```

- [ ] **Step 2: Reset ask_card_focus in cycle_from_journal**

Find `cycle_from_journal` (`src/input/actions/overlay_cycle.rs` ~line 49). It holds `let mut s = state.borrow_mut();` and calls `s.journal_overlay.hide();`. Add `s.ask_card_focus = true;` inside that same borrow scope, right after the `hide()` call:

```rust
    s.journal_overlay.hide();
    // Ctrl+Tab focus toggle: closing the overlay resets ask-card focus.
    s.ask_card_focus = true;
```

(This mirrors the gloss branch's `cycle_from_gloss` reset. Do NOT open a new borrow — a nested RefCell borrow is a RUNTIME PANIC. Confirm `s` is the live `borrow_mut` at this point.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: `Finished` with no errors.

- [ ] **Step 4: Run the full bin test suite (no regressions)**

Run: `cargo test --bins 2>&1 | rg "test result"`
Expected: `ok. N passed; 0 failed` (N ≈ 1050).

- [ ] **Step 5: Commit**

```bash
git add src/ui/journal_overlay.rs src/input/actions/overlay_cycle.rs
git commit -m "fix(journal): clear focus-dim on hide() + reset focus on cycle close"
```

---

### Task 3: Headless verification (journal 2-col float + toggle + submit)

Confirm on-screen: the journal ask floats 2-col, Ctrl+Tab toggles focus + dims, Escape returns to ask, and — the primary regression risk — the answer renders full-width after submit.

**Files:**
- No source changes. Uses the cage/grim/wtype flow from `linux-lit/CLAUDE.md`.

**Interfaces:**
- Consumes: the built `target/debug/linux-lit` (worktree binary).

- [ ] **Step 1: Build and launch under cage**

```bash
cd <worktree-root> && cargo build
pkill -f "cage -- ./target/debug/linux-lit" 2>/dev/null; sleep 1
LIT_LOG_PATH=/tmp/journalfloat.log LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 5
export WAYLAND_DISPLAY=$(command ls -t /run/user/1000/wayland-* | grep -vE '\.lock|wayland-0$' | head -1 | xargs basename) XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

- [ ] **Step 2: Open the journal overlay + ask float, confirm 2-col**

Confirm current binds in `keymap_config.rs` before scripting (Ctrl+j opens the journal overlay; Ctrl+a opens the ask in the journal overlay handler). Drive:

```bash
wtype -M ctrl -k j -m ctrl; sleep 2          # open journal overlay
wtype -M ctrl -k a -m ctrl; sleep 2          # open the ask (should now FLOAT)
wtype "journal float test"; sleep 1
grim -o HEADLESS-1 /tmp/jf-typed.png
```

Read `/tmp/jf-typed.png`: confirm the ask card is a RIGHT-SIDE floated panel beside a full-height journal card (a centered pair with roughly equal L/R gutters), NOT a stacked box filling 3/4 of the height. Confirm the typed "journal float test" draft is visible.

- [ ] **Step 3: Ctrl+Tab toggle + dim + Escape-return**

```bash
wtype -M ctrl -k Tab -m ctrl; sleep 1
wtype -k G; sleep 1                          # left journal card nav
grim -o HEADLESS-1 /tmp/jf-left.png          # ask float dimmed, draft preserved
wtype -M ctrl -k Tab -m ctrl; sleep 1
grim -o HEADLESS-1 /tmp/jf-ask.png           # journal card dimmed, ask un-dimmed
wtype -M ctrl -k Tab -m ctrl; sleep 1
wtype -k Escape; sleep 1
grim -o HEADLESS-1 /tmp/jf-esc.png           # returns to ask, overlay still open
```

Read all three. Confirm: `jf-left` — ask float dimmed (0.55), journal card full; draft "journal float test" intact. `jf-ask` — journal card dimmed, ask full. `jf-esc` — ask focused (un-dimmed), overlay still open. Cross-check `/tmp/journalfloat.log` shows `KEY: name=Tab ctrl=true ... mode=JournalOverlay` for the toggles and the final Escape stays `mode=JournalOverlay` (did not close).

- [ ] **Step 4: Submit and confirm the answer renders full-width (Risk 1)**

```bash
wtype -M ctrl -k Return -m ctrl; sleep 6     # submit; wait for improve-Q + answer
grim -o HEADLESS-1 /tmp/jf-answer.png
```

Read `/tmp/jf-answer.png`: confirm the ask float is gone, the journal card is restored to FULL WIDTH (margin_end reset to 0), and the answer text renders full-width and un-clipped in the journal overlay (this is the primary regression risk of dropping the stacked layout). If the answer is offset/narrow/clipped, that is a FAIL — report it (the `reserve(0)` close path did not fully restore the card).

- [ ] **Step 5: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 6: No commit (verification only)**

No source commit expected. If a harness/e2e assertion was extended, commit it here.

---

### Task 4: Journal keybind-legend entry

Document the now-active `Ctrl+Tab` toggle in the journal overlay's Ctrl+/ legend (the prior feature deliberately withheld it because the journal toggle was dead; it now works).

**Files:**
- Modify: `src/ui/journal_keybinds_overlay.rs`

**Interfaces:**
- Consumes: nothing (documentation of behavior now live via Task 1).

- [ ] **Step 1: Add the MRU row**

In `src/ui/journal_keybinds_overlay.rs`, the `MRU` const (near line 12) has:

```rust
    ("Ctrl+a", "begin_ask: new Q&A in this band"),
```

Add immediately AFTER that row (the toggle is used during the Ctrl+a ask flow):

```rust
    ("Ctrl+Tab", "toggle focus: ask card ↔ journal card (2-col)"),
```

Match the file's exact indentation, tuple style, and trailing-comma convention. Use the `↔` arrow (the file already uses `→`, so unicode is fine). Do not modify any other row.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add src/ui/journal_keybinds_overlay.rs
git commit -m "docs(keybinds): journal legend notes Ctrl+Tab ask/journal focus toggle"
```

---

## Self-Review

**Spec coverage:**
- Float the journal ask (new() enable_float + reservation) → Task 1 Steps 2–3. ✓
- Ask as add_overlay sibling, remove stacked append → Task 1 Steps 2 & 4. ✓
- Container width_request for centered pair → Task 1 Step 1. ✓
- Drop set_input_fill_fraction(0.75) → Task 1 Step 3. ✓
- Stale-dim leak fix (hide() + cycle_from_journal reset) → Task 2. ✓
- Post-submit full-width answer (Risk 1) → Task 3 Step 4. ✓
- Ctrl+Tab toggle/dim/Escape verification (journal) → Task 3 Steps 2–3. ✓
- Journal legend entry → Task 4. ✓
- No keymap.rs / gloss / synopsis changes → none of the tasks touch them. ✓

**Placeholder scan:** No TBD/TODO. Every code step shows the exact before/after. Task 1 Step 6 and the leak fix's `hide()` body reference "the actual current body" because `hide()` may reset extra widgets — the instruction is to APPEND one call, which is unambiguous.

**Type consistency:** `float_w: i32`, `reserve: Rc<dyn Fn(i32)>`, `ask_container_for_reserve` captured before `AskCardHost::new` consumes `ask` (matches gloss). `enable_float(i32, Rc<dyn Fn(i32)>)`, `is_ask_float()`, `clear_focus_dim()`, `ask_card_focus: bool` all match existing signatures. `container.set_width_request(column_width as i32)` uses the `new()` param type (`u32` → `i32` cast, as gloss does).
