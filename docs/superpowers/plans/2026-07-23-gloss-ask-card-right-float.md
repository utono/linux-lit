# Gloss/synopsis ask card right-floated 2-column layout — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the gloss/synopsis overlay's ask card opens, float it as a bordered panel on the right with the gloss commentary full-height on the left, the pair centered — instead of shrinking the gloss scroll and stacking the ask card below it.

**Architecture:** Add a `float` mode to `AskCardHost`. In float mode `open`/`close` skip the scroll-shrink and instead (a) reparent the `AskCard` container to be a right-anchored `add_overlay` sibling of the gloss card via a host-provided closure, (b) reserve a `margin_end` on the gloss `container` equal to the ask width + seam so the gloss+ask pair centers together, and (c) size the ask card to a fixed width and the gloss card's height. The journal overlay never sets float mode, so its stacked path is byte-for-byte unchanged. A new `.gloss-ask-float` CSS class (modeled on `.chat-panel-float`) gives the floated card an opaque background + hairline border.

**Tech Stack:** Rust, GTK4 (gtk4-rs), Cairo/Pango. Existing overlay + `AskCardHost` machinery.

## Global Constraints

- Reader theme is independent of the system theme; all CSS lives in `src/theme.rs::generate_css`. (linux-lit CLAUDE.md)
- New panels are `add_overlay` layers, never links in the size-bearing widget chain. (`feedback_picker_overlay_not_chain`)
- The journal overlay's `AskCardHost` behavior must remain unchanged (only the gloss/synopsis overlay changes). (spec, Scope)
- Verify with `cargo build`; do NOT run the app (`cargo run`) — the user launches it. (CLAUDE.md, `feedback_no_cargo_run`)
- Cage is software rendering; a cage pass is not final confirmation — hand the user the e2e command for a real-GL eyeball. (CLAUDE.md)
- Timestamps US Central: `TZ='America/Chicago' date +"%Y-%m-%dT%H:%M:%SZ"`.
- Commit messages end with the Co-Authored-By / Claude-Session trailer used by this repo's recent commits.

---

## File Structure

- `src/ui/ask_card.rs` — `AskCardHost` gains a `float: Cell<bool>` + a stored
  optional reservation closure + a fixed float width; `open`/`close`/`size`
  branch on it. `AskCard` gains an `open_floated` positioning helper (fixed
  width, right-anchored margins) so float mode does not reuse the
  prose-column-margin insetting that `open_in_mode` applies.
- `src/ui/gloss_overlay.rs` — build the ask card as an `add_overlay` sibling of
  `container` on `self.overlay` (right-anchored, hidden); enable float mode on
  the host and register the gloss-card `margin_end` reservation closure; apply
  `.gloss-ask-float`. Retire `set_input_fill_fraction(0.75)`.
- `src/theme.rs` — new `.gloss-ask-float` rule in `generate_css`.
- No `keymap_config.rs`, `keymap.json`, or Ctrl+/ overlay changes (no bind moved).

Task order: (1) CSS class, (2) `AskCard` float positioning helper + unit test,
(3) `AskCardHost` float mode + unit test, (4) gloss overlay wiring, (5) headless
verification. Each task ends green and committed.

---

### Task 1: Add the `.gloss-ask-float` CSS class

**Files:**
- Modify: `src/theme.rs` (inside `generate_css`, adjacent to the `.chat-panel-float` rule at ~line 1429)

**Interfaces:**
- Consumes: the `generate_css` `{bg}` and `{fg}` format bindings (already in scope where `.chat-panel-float` is written).
- Produces: a `.gloss-ask-float` CSS class applied by Task 4.

- [ ] **Step 1: Add the CSS rule**

In `src/theme.rs`, immediately after the `.chat-panel-float {{ ... }}` block (ends ~line 1433), add a sibling rule in the same `format!` string:

```rust
         /* Gloss/synopsis ask card floated to the RIGHT of the gloss card (the \
            2-column ask layout). The ask card is lifted out of the gloss \
            container and shown as an add_overlay sibling, so — like \
            .chat-panel-float — it needs an OPAQUE background (it would else \
            blend with the scrim/card behind it) plus a hairline border to read \
            as a distinct panel. Same {bg}/{fg} card surface as the gloss card. */ \
         .gloss-ask-float {{ background-color: {bg}; \
           border: 1px solid alpha({fg}, 0.25); \
           border-radius: 12px; \
           padding: 0; }} \
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds (no `error[` lines). A `format!` brace-mismatch would fail here — the `{{`/`}}` escaping must match the surrounding rules.

- [ ] **Step 3: Commit**

```bash
git add src/theme.rs
git commit -m "feat(theme): .gloss-ask-float class for right-floated gloss ask card

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

### Task 2: `AskCard` fixed-width right-float positioning helper

`open_in_mode` currently re-sets `container.margin_start/end` to the overlay
prose-column margin (card_width/5 or /8), which centers the card in a
full-width column. Float mode needs the container at a FIXED width, so the ask
card is a narrow right panel — not a full-width inset. Add a helper that sizes
the container to a fixed width and does NOT apply the prose-column insets.

**Files:**
- Modify: `src/ui/ask_card.rs` (`impl AskCard`, near `set_input_height` ~line 157)
- Test: `src/ui/ask_card.rs` (`#[cfg(test)]` module at end of file — create if absent)

**Interfaces:**
- Consumes: `AskCard.container` (a `gtk4::Box`).
- Produces:
  - `pub fn set_float_width(&self, width: i32)` — pins the container's width_request; `width <= 0` clears it (`-1`).
  - `pub fn float_width(&self) -> i32` — returns the last set float width (0 if unset). Used by the host to compute the gloss reservation.
  - A `float_width: Cell<i32>` field on `AskCard`, default 0.

- [ ] **Step 1: Write the failing test**

Add to `src/ui/ask_card.rs` (create the test module if none exists). GTK needs init; use the project's existing test-init pattern — grep `gtk4::init` or `#[gtk4::test]` in the repo first and mirror it. If the repo has no GTK test harness for widgets, make this a pure logic test instead (see Step 1b).

```rust
#[cfg(test)]
mod float_tests {
    use super::*;

    #[test]
    fn set_float_width_roundtrips() {
        // Pure state check — no widget realization needed.
        // AskCard::new requires a return_focus widget; if GTK isn't initialized
        // in tests, fall back to the Step 1b logic-only test and delete this.
        let _ = gtk4::init();
        let anchor = gtk4::Label::new(None);
        let card = AskCard::new(80, &anchor);
        assert_eq!(card.float_width(), 0);
        card.set_float_width(560);
        assert_eq!(card.float_width(), 560);
        card.set_float_width(0);
        assert_eq!(card.float_width(), 0);
    }
}
```

- [ ] **Step 1b: If GTK cannot init in unit tests, replace with a logic-only test**

If `cargo test` panics on `AskCard::new` (GTK not initialized under the test
binary), the widget-touching test can't run headless. In that case DELETE the
`float_tests` module above and instead cover the reservation arithmetic in
Task 3's pure helper (`gloss_reservation_width`), which needs no GTK. Note in
the commit message that `AskCard::set_float_width` is exercised by the headless
e2e in Task 5, not a unit test.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib float_tests -- --nocapture 2>&1 | tail -20`
Expected: FAIL — `no method named float_width` / `set_float_width`. (Or the GTK-init panic that sends you to Step 1b.)

- [ ] **Step 3: Add the field**

In `AskCard`'s struct (after `prose_reading: Cell<bool>,` ~line 48):

```rust
    /// Fixed container width for the right-floated ask card (gloss 2-column
    /// layout). 0 = unset (the card uses the overlay's prose-column insets in
    /// `open_in_mode`). Set by `AskCardHost` in float mode.
    float_width: Cell<i32>,
```

In `AskCard::new`'s returned `Self { ... }` (after `prose_reading: Cell::new(false),` ~line 140):

```rust
            float_width: Cell::new(0),
```

- [ ] **Step 4: Add the accessors and honor the width in `open_in_mode`**

Add methods in `impl AskCard` (after `set_input_height`, ~line 168):

```rust
    /// Pin the ask-card container to a FIXED width (the right-floated gloss
    /// layout). `width <= 0` clears the pin (`width_request = -1`), restoring
    /// the default full-width-column behavior. Also skips the prose-column
    /// margin insetting in `open_in_mode` (a fixed-width float panel is inset by
    /// its overlay margin, not by card_width/5).
    pub fn set_float_width(&self, width: i32) {
        self.float_width.set(width.max(0));
        self.container.set_width_request(if width > 0 { width } else { -1 });
    }

    /// The last float width set (0 if unset). The host uses it to size the
    /// gloss-card margin reservation.
    pub fn float_width(&self) -> i32 {
        self.float_width.get()
    }
```

In `open_in_mode`, guard the prose-column inset so float mode keeps the fixed
width. Change the `if card_width > 0 {` block (~line 220) to:

```rust
        if self.float_width.get() > 0 {
            // Float mode: fixed-width right panel. Keep the container's pinned
            // width; use a uniform inner inset (the panel border is the card
            // edge, not a card/5 column margin).
            self.container.set_margin_start(16);
            self.container.set_margin_end(16);
        } else if card_width > 0 {
            // Match the host overlay's text column: card/8 when the journal is
            // showing a prose work at the main reading card's margin, card/5
            // otherwise (see `prose_reading`).
            let margin = if self.prose_reading.get() {
                crate::ui::prose_reading_card_margin(card_width)
            } else {
                crate::ui::prose_column_margin(card_width)
            };
            self.container.set_margin_start(margin);
            self.container.set_margin_end(margin);
        }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib float_tests -- --nocapture 2>&1 | tail -20`
Expected: PASS (or, if you took Step 1b, this module no longer exists — run `cargo build` instead and expect a clean build).

- [ ] **Step 6: Commit**

```bash
git add src/ui/ask_card.rs
git commit -m "feat(ask-card): fixed-width right-float positioning helper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

### Task 3: `AskCardHost` float mode

Add a float mode to `AskCardHost`. When enabled, `open` reveals the ask card at
its fixed float width and fires a host-registered reservation closure with the
reservation width (so the gloss overlay adds `margin_end` + re-centers); it does
NOT shrink the scroll. `close` clears the reservation (fires the closure with
0) and does NOT restore a shrunk scroll height (nothing shrank). `size` in float
mode skips `pin_scroll_height` (the gloss scroll keeps its own full height set
elsewhere — see Task 4).

**Files:**
- Modify: `src/ui/ask_card.rs` (`struct AskCardHost` ~line 448, `impl AskCardHost` ~line 483)
- Test: `src/ui/ask_card.rs` (pure helper `gloss_reservation_width`)

**Interfaces:**
- Consumes: `AskCard::set_float_width`, `AskCard::float_width` (Task 2); the existing `AskCardHost.card_width`/`card_height` cells.
- Produces:
  - `pub fn enable_float(&self, float_width: i32, reserve: Rc<dyn Fn(i32)>)` — turns on float mode, stores the fixed ask width and the reservation callback (`reserve(reservation_px)` on open, `reserve(0)` on close).
  - `pub fn is_float(&self) -> bool`.
  - `fn gloss_reservation_width(float_width: i32, seam: i32) -> i32` — pure: `float_width + seam` (unit-tested).
  - New fields on `AskCardHost`: `float: Cell<bool>`, `float_width: Cell<i32>`, `reserve: RefCell<Option<Rc<dyn Fn(i32)>>>`.

- [ ] **Step 1: Write the failing test for the reservation arithmetic**

Add to the `float_tests` module in `src/ui/ask_card.rs` (or create it — this
test is pure, no GTK):

```rust
#[cfg(test)]
mod host_float_tests {
    use super::*;

    #[test]
    fn reservation_is_width_plus_seam() {
        assert_eq!(gloss_reservation_width(560, 24), 584);
        assert_eq!(gloss_reservation_width(0, 24), 24);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib host_float_tests -- --nocapture 2>&1 | tail -20`
Expected: FAIL — `cannot find function gloss_reservation_width`.

- [ ] **Step 3: Add the pure helper**

Add a free function in `src/ui/ask_card.rs` (module scope, above `impl AskCardHost`):

```rust
/// Width the gloss card must reserve (`margin_end`) so the floated ask card of
/// `float_width` sits beside it with a `seam` gap. Pure so it is unit-tested
/// without GTK. The gloss overlay adds this to its container's `margin_end` and
/// re-centers the widened footprint.
pub(crate) fn gloss_reservation_width(float_width: i32, seam: i32) -> i32 {
    float_width + seam
}
```

- [ ] **Step 4: Add the float fields + methods to `AskCardHost`**

In `struct AskCardHost` (after `input_fill_fraction: Cell<Option<f32>>,` ~line 480):

```rust
    /// Float mode (gloss/synopsis 2-column ask): `open`/`close` reserve room on
    /// the gloss card and reveal the ask card as a right panel INSTEAD of
    /// shrinking the scroll. The journal overlay leaves this false → its stacked
    /// behavior is unchanged.
    float: Cell<bool>,
    /// Fixed ask-card width in float mode (0 = derive a default in `enable_float`).
    float_width: Cell<i32>,
    /// Called on open with the reservation px, and on close with 0. The gloss
    /// overlay uses it to set/clear its card's `margin_end` and re-center.
    reserve: std::cell::RefCell<Option<Rc<dyn Fn(i32)>>>,
```

In `AskCardHost::new`'s returned `Self { ... }` (after `input_fill_fraction: Cell::new(None),` ~line 499):

```rust
            float: Cell::new(false),
            float_width: Cell::new(0),
            reserve: std::cell::RefCell::new(None),
```

Add methods in `impl AskCardHost` (after `set_input_fill_fraction`, ~line 508):

```rust
    /// Enable float mode: the ask card floats at `float_width` to the right of
    /// the gloss card, which reserves room via `reserve`. `reserve(px)` is
    /// called with the reservation width on open and `reserve(0)` on close.
    /// Pins the card's float width now so `open` reveals it narrow.
    pub fn enable_float(&self, float_width: i32, reserve: Rc<dyn Fn(i32)>) {
        self.float.set(true);
        self.float_width.set(float_width.max(0));
        self.ask.set_float_width(float_width);
        *self.reserve.borrow_mut() = Some(reserve);
    }

    pub fn is_float(&self) -> bool {
        self.float.get()
    }
```

- [ ] **Step 5: Branch `size`, `open`, `close` on float mode**

In `AskCardHost::size` (~line 525), skip the scroll-pin in float mode (the gloss
overlay sets the scroll height itself; see Task 4). Wrap the existing body:

```rust
    pub fn size(&self, card_width: i32, card_height: i32, fixed_chrome_h: i32, footer_h: i32) {
        self.card_width.set(card_width);
        self.card_height.set(card_height);
        self.fixed_chrome_h.set(fixed_chrome_h);
        if self.float.get() {
            // Float mode: the gloss scroll keeps its own full height; the ask
            // card is a fixed-width right panel whose height matches the card.
            self.ask.set_float_width(self.float_width.get());
            return;
        }
        let scroll_h = (card_height - fixed_chrome_h - footer_h).max(80);
        self.closed_scroll_h.set(scroll_h);
        self.pin_scroll_height(scroll_h);
    }
```

In `AskCardHost::open` (~line 559), take the float branch before the shrink math:

```rust
    pub fn open(&self, title: &str, hint: &str, legend: &str, block_fill: &str, block_fg: &str) {
        if self.float.get() {
            // Reveal the ask card at its pinned float width and reserve room on
            // the gloss card. No scroll shrink — the commentary stays full height.
            self.ask.set_float_width(self.float_width.get());
            self.ask
                .open(title, hint, legend, 0, block_fill, block_fg);
            if let Some(reserve) = self.reserve.borrow().as_ref() {
                reserve(gloss_reservation_width(self.float_width.get(), FLOAT_SEAM));
            }
            self.recompute_now_and_idle();
            return;
        }
        self.ask
            .open(title, hint, legend, self.card_width.get(), block_fill, block_fg);
        if let Some(f) = &self.footer {
            f.set_visible(false);
        }
        if let Some(frac) = self.input_fill_fraction.get() {
            self.ask.set_input_height(160);
            let (_, base) = self.ask.container().preferred_size();
            let chrome = (base.height() - 160).max(0);
            let target = ((self.card_height.get() as f32 * frac).round() as i32) - chrome;
            self.ask.set_input_height(target.max(80));
        }
        let (_, ask_size) = self.ask.container().preferred_size();
        let scroll_h =
            (self.card_height.get() - self.fixed_chrome_h.get() - ask_size.height()).max(80);
        self.pin_scroll_height(scroll_h);
        self.recompute_now_and_idle();
    }
```

Note: `open` passes `card_width = 0` in float mode so `AskCard::open_in_mode`
keeps the pinned float width (the `float_width > 0` guard from Task 2 fires
regardless of `card_width`, but passing 0 avoids re-insetting).

In `AskCardHost::close` (~line 587), clear the reservation in float mode:

```rust
    pub fn close(&self) {
        self.ask.close();
        if self.float.get() {
            if let Some(reserve) = self.reserve.borrow().as_ref() {
                reserve(0);
            }
            self.recompute_now_and_idle();
            return;
        }
        if let Some(f) = &self.footer {
            f.set_visible(true);
        }
        self.pin_scroll_height(self.closed_scroll_h.get().max(80));
        self.recompute_now_and_idle();
    }
```

Add the seam const near the top of the `AskCardHost` region (module scope):

```rust
/// Gap between the gloss card's right edge and the floated ask panel. Matches
/// the chat panel's card↔panel seam so the two 2-column designs read alike.
const FLOAT_SEAM: i32 = 24;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib -- ask_card 2>&1 | tail -20`
Expected: PASS for `host_float_tests` (and `float_tests` if it survived Task 2).

- [ ] **Step 7: Verify the whole crate still builds**

Run: `cargo build 2>&1 | tail -5`
Expected: clean build.

- [ ] **Step 8: Commit**

```bash
git add src/ui/ask_card.rs
git commit -m "feat(ask-card): AskCardHost float mode (right panel + gloss reservation)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

### Task 4: Wire the gloss overlay to float its ask card

Reparent the ask card out of the vertical `container` into an `add_overlay`
sibling on `self.overlay`, right-anchored; enable float mode on the host with a
reservation closure that sets the gloss `container`'s `margin_end` and
re-centers the pair; give the floated card `.gloss-ask-float`; retire
`set_input_fill_fraction(0.75)`; keep the gloss scroll full-height when the ask
card opens.

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — the `new` builder (ask-card build ~line 555-575; the overlay assembly) and the `attach` method (~line 1419).

**Interfaces:**
- Consumes: `AskCardHost::enable_float`, `AskCardHost::is_float`, `gloss_reservation_width` (Task 3); `AskCard::container` / `float_width`.
- Produces: no new public signature — `open_ask_card_with` / `close_ask_card` / `take_ask_text` / `ask_is_open` keep their signatures; only internal layout changes.

- [ ] **Step 1: Stop appending the ask card into `container`; keep the field**

In `src/ui/gloss_overlay.rs::new`, the block at ~line 550-556 currently does:

```rust
        let ask = AskCard::new(text_margins as i32, &gloss_scrolled);
        container.append(ask.container());
```

Change it to NOT append into `container` (the float overlay will host it) and
mark the container for float styling. Keep `ask` for the host:

```rust
        // The ask card is NOT appended into `container`: in the gloss overlay it
        // FLOATS as a right-side add_overlay sibling of the card (the 2-column
        // ask layout). It is added to `self.overlay` after the scrim/container
        // are attached (see `attach`). Here we only build it and style it as the
        // opaque floating panel.
        let ask = AskCard::new(text_margins as i32, &gloss_scrolled);
        ask.container().add_css_class("gloss-ask-float");
        ask.container().set_halign(Align::End);
        ask.container().set_valign(Align::Fill);
```

- [ ] **Step 2: Enable float mode on the host with the reservation closure**

Replace the `set_input_fill_fraction(0.75)` block (~line 571-575):

```rust
        let ask_host =
            AskCardHost::new(ask, &gloss_scrolled, Some(footer_box.clone()), recompute);
        // The gloss/synopsis ask card (add-question, edit gloss, fix-IPA, inner
        // monologue) fills 3/4 of the overlay height, matching the journal Q&A.
        ask_host.set_input_fill_fraction(0.75);
```

with float enablement. The reservation closure sets the container's `margin_end`
(reservation on open, 0 on close). The float width is a fraction of the
construction column width (a sensible fixed default; the reservation re-centers
the pair regardless):

```rust
        let ask_host =
            AskCardHost::new(ask, &gloss_scrolled, Some(footer_box.clone()), recompute);
        // Gloss/synopsis ask card floats to the RIGHT of the gloss card (2-column
        // ask layout). Fixed float width; the card reserves margin_end = width +
        // seam so the gloss+ask pair centers together (the container is an
        // add_overlay child, halign=Center by default, so a right margin shifts
        // it left and the ask panel fills the freed right space).
        let float_w = (column_width as i32 * 5 / 8).max(360);
        {
            let container_for_reserve = container.clone();
            let reserve = Rc::new(move |px: i32| {
                container_for_reserve.set_margin_end(px);
            }) as Rc<dyn Fn(i32)>;
            ask_host.enable_float(float_w, reserve);
        }
```

- [ ] **Step 3: Add the floated ask card to the overlay in `attach`**

The scrim + container are added to `self.overlay` inside `attach` via
`picker_attach::attach_overlay_panel`. The floated ask card must be added AFTER
them (so it paints on top of the container) and positioned right. Extend
`attach` (~line 1419):

```rust
    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
        // The ask card floats as a right-anchored sibling of the gloss card (the
        // 2-column ask layout). Added last so it paints above the card; hidden
        // until an ask flow opens it. Not measured (its size is fixed) and
        // clipped like the container.
        let ask_container = self.ask_host.card().container();
        self.overlay.add_overlay(ask_container);
        self.overlay.set_measure_overlay(ask_container, false);
        self.overlay.set_clip_overlay(ask_container, true);
    }
```

Confirm `AskCardHost` exposes `card()` (it does: `ask_card.rs:512`) and that
`container()` returns the `gtk4::Box`.

- [ ] **Step 4: Anchor the floated card vertically to the gloss card**

The container is `halign=End valign=Fill`, so it fills the overlay's full
height. To match the gloss CARD's height (not the whole screen), set its
`height_request` to the card height in the show paths that size the card. The
simplest correct point is where the container height is already known. Add,
inside `size`-equivalent show paths OR once in `attach`, a height match. Since
the card height is `last_card_size.1`, set it whenever the ask opens by having
the reservation closure ALSO cap the ask height. Update the closure from Step 2
to size height too:

```rust
        let float_w = (column_width as i32 * 5 / 8).max(360);
        {
            let container_for_reserve = container.clone();
            let ask_container_for_reserve = ask.container().clone();
            // NOTE: `ask` was moved into AskCardHost::new below — capture the
            // container clone BEFORE the move. Reorder so this closure is built
            // from `ask.container()` before `AskCardHost::new(ask, ...)`.
            let reserve = Rc::new(move |px: i32| {
                container_for_reserve.set_margin_end(px);
                // Match the ask panel height to the gloss card's current height
                // (px>0 = opening). The card height is the container's allocated
                // height; request it so the ask panel is top/bottom aligned.
                if px > 0 {
                    let h = container_for_reserve.height().max(container_for_reserve.height_request());
                    ask_container_for_reserve.set_height_request(h.max(200));
                } else {
                    ask_container_for_reserve.set_height_request(-1);
                }
            }) as Rc<dyn Fn(i32)>;
            ask_host.enable_float(float_w, reserve);
        }
```

**Ordering caveat:** `AskCardHost::new(ask, ...)` consumes `ask` by value. The
closure needs `ask.container().clone()` — capture it (and build the
`recompute`) BEFORE `AskCardHost::new`. Structure the block as:
1. build `recompute` (already before, ~line 563),
2. `let ask_container_clone = ask.container().clone();`
3. `let ask_host = AskCardHost::new(ask, ...);`
4. build `reserve` from the clones,
5. `ask_host.enable_float(float_w, reserve);`

Verify the final ordering compiles (a use-after-move is a hard error, so the
compiler enforces this).

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | tail -15`
Expected: clean build. If a borrow/move error fires, apply the ordering caveat above.

- [ ] **Step 6: Verify the journal overlay is untouched**

Run: `git diff --stat` — expected files: `src/ui/gloss_overlay.rs`, and (from earlier tasks) `src/ui/ask_card.rs`, `src/theme.rs`. `src/ui/journal_overlay.rs` MUST NOT appear.

Run: `rg -n "enable_float|is_float|gloss-ask-float" src/ui/journal_overlay.rs`
Expected: no matches (journal never floats).

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): float the ask card right in a 2-column layout

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

### Task 5: Headless verification + clippy

Confirm on-screen that the gloss commentary stays full-height on the left and
the ask input floats as a bordered panel on the right, centered as a pair — for
BOTH a reader-gloss Ask and the synopsis Edit ask.

**Files:** none (verification only). If a regression is found, fix in the
relevant task's file and re-commit.

- [ ] **Step 1: Clippy stays clean**

Run: `cargo clippy 2>&1 | rg -c "^warning|^error" || echo 0`
Expected: no NEW warnings vs. baseline. If clippy count rose, address the new lint.

- [ ] **Step 2: Line-clipping e2e still passes (gloss card clip is a no-regression)**

Run: `./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture 2>&1 | tail -20`
Expected: PASS. The gloss scroll is full-height in float mode, so its clip
invariant is unchanged.

- [ ] **Step 3: Headless cage drive — open a gloss, open its ask card, screenshot**

Follow the `test-headless-navigation` / Headless Verification protocol in
CLAUDE.md. Build, launch cage with `LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo`,
resize to 1920x1200, open a work with a known reader-gloss, open the gloss
overlay (`Ctrl+g` or the gloss key per `keymap_config.rs` — confirm the current
bind before scripting), then open the ask card (the reader-gloss Ask key), and
`grim` a screenshot into `target/ui/`.

Confirm the exact keys from source first:

```bash
rg -n "GlossAsk|OpenGloss|ReaderGloss|\"a\"|open_ask|show_prompt_dialog" src/input/keymap_config.rs src/input/keymap.rs | head
```

- [ ] **Step 4: Open every screenshot and report what is on screen**

Per the UI review protocol: open each `target/ui/*.png`, quote the on-screen
text, and confirm:
- gloss commentary is on the LEFT at full card height (NOT shrunk to the top);
- the ask input is a distinct BORDERED panel on the RIGHT (`.gloss-ask-float`);
- the gloss+ask footprint is centered (gloss card visibly shifted left vs. its
  closed position);
- no clipping at the gloss card's bottom edge.

If the layout is wrong (e.g. ask panel full-screen height instead of card
height, or overlapping the gloss text), fix in Task 4's file and re-run.

- [ ] **Step 5: Repeat for the synopsis Edit ask (shared path)**

In cage, open a synopsis (`h`), press the synopsis Edit key (`show_edit_prompt`;
confirm the bind), screenshot, and confirm the SAME right-float layout — proving
the shared `open_ask_card_with` path floats for every gloss/synopsis prompt.

- [ ] **Step 6: Hand the user the real-GL eyeball command**

Cage is software rendering. Provide the user the exact command to see it on the
real renderer, e.g.:

```bash
cd ~/utono/linux-lit && cargo run
# open a reader-glossed passage → open the gloss overlay → open the ask card;
# expect gloss commentary left (full height), ask input floated right (bordered),
# the pair centered.
```

- [ ] **Step 7: Final commit (if any fixes landed in Step 4/5)**

```bash
git add -A
git commit -m "fix(gloss): correct floated ask-card geometry from headless review

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

## Self-Review

**Spec coverage:**
- Float, not split → Tasks 3-4. ✓
- All five prompts, one code path → the shared `open_ask_card_with` on the gloss
  overlay drives `AskCardHost::open`; float mode is set once in `new`, so every
  prompt floats. ✓ (Task 4)
- Pair re-centers together → reservation closure sets `container.margin_end`;
  the container is `add_overlay`'d halign=Center, so a right margin shifts it
  left and centers the widened footprint. ✓ (Task 4 Step 2/4)
- Gloss scroll full-height (clip math untouched) → `size`/`open` skip
  `pin_scroll_height` in float mode. ✓ (Task 3) — verified by line_clipping e2e
  (Task 5 Step 2).
- `.gloss-ask-float` opaque + border → Task 1, applied Task 4 Step 1. ✓
- Journal overlay unchanged → float never enabled there; verified Task 4 Step 6. ✓
- No bind/keymap.json/overlay changes → none of those files touched. ✓
- Focus lands in ask input on open → unchanged (`AskCard::open` grabs input
  focus; the per-prompt `feed_ask_vim_key(Char('i'))` still runs). ✓

**Placeholder scan:** every code step shows concrete code; no TBD/TODO/"handle
edge cases". The one branch (widget unit test vs. logic-only) is resolved
explicitly by Step 1b with a decision rule. ✓

**Type consistency:** `set_float_width`/`float_width` (Task 2) ↔ used in Task 3
`open`/`size`/`enable_float`. `enable_float(float_width, reserve)`,
`is_float()`, `gloss_reservation_width(float_width, seam)`, `FLOAT_SEAM` — all
defined in Task 3, consumed in Task 4. `card()` accessor confirmed at
`ask_card.rs:512`. Reservation closure signature `Rc<dyn Fn(i32)>` consistent
across `enable_float`'s param and the gloss overlay's construction. ✓

## Out of scope (carried from spec)

- Journal overlay ask card; config persistence of float; changes to what the ask
  card sends; a left/right side toggle; reader-level `Ctrl+a` / visual-mode ask
  triggers.
