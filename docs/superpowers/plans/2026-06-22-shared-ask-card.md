# Shared AskCard Component Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the synopsis/gloss "ask" input card into one reusable `AskCard` component embedded by both the gloss overlay and the journal overlay, so the two hand-rolled input cards can no longer drift, and the journal card conforms to the canonical synopsis card.

**Architecture:** A new `src/ui/ask_card.rs` defines `AskCard` (widgets + focus state) and the single `AskFocus { Doc, Ask }` enum. `GlossOverlay` and `JournalOverlay` each hold one `AskCard` field and delegate their existing public `*_ask_*` methods to it as thin wrappers, so call sites in `actions/` and `keymap.rs` keep their method names and only the `AskFocus` variant names they reference change (`Synopsis`/`Page` → `Doc`).

**Tech Stack:** Rust, gtk4 crate (`gtk4::prelude::*`, `Box`, `TextView`, `ScrolledWindow`, `Label`), `std::cell::Cell`.

## Global Constraints

- The synopsis/gloss ask-card is the designated standard; the journal card conforms to it. No visual change to the working synopsis/gloss card.
- Canonical widget values, copied verbatim from `src/ui/gloss_overlay.rs:382-424`:
  - container: `gtk4::Box` vertical, css `ask-card`, `margin_top 14`, `margin_start/end = text_margins`, `margin_bottom 14`, `set_visible(false)`.
  - title: `Label`, css `gloss-header`, `halign Start`, `margin_start 16`, `margin_top 12`.
  - scrolled: `ScrolledWindow`, `min_content_height 72`, `max_content_height 160`, hscrollbar `Never`, `margin_start/end 16`, `margin_top/bottom 6`. (No vscrollbar policy set, no `set_propagate_natural_height`.)
  - input: `TextView`, editable, cursor visible, wrap `Word`, all four inner margins `6`, css `gloss-text` + `ask-input`. **No `vexpand`.**
  - hint: `Label`, css `ask-hint`, `halign Center`, `margin_bottom 10`.
- The single shared enum is `crate::ui::ask_card::AskFocus { Doc, Ask }`. The two old enums (`gloss_overlay::AskFocus { Synopsis, Ask }`, `journal_overlay::AskFocus { Page, Ask }`) are deleted.
- `open(title, hint, card_width)`: set title/hint, clear buffer, if `card_width > 0` set container `margin_start/end = card_width/4`, `set_visible(true)`, focus = `Ask`.
- Focus management mirrors `set_ask_focus`: on `Ask` remove `card-dimmed` + add `card-focused` + `input.grab_focus()`; on `Doc` remove `card-focused` + add `card-dimmed` + if input has focus, `return_focus.grab_focus()`. `close()` does the `Doc` teardown and hides.
- Method names on both overlays are unchanged so callers don't churn: `open_ask_card`, `open_ask_card_with` (gloss only), `close_ask_card`, `take_ask_text`, `toggle_ask_focus`, `ask_is_open`, `ask_focus`. Each returns/accepts `crate::ui::ask_card::AskFocus`.
- Build must stay clean; `cargo test --bins` count unchanged at 413.

---

### Task 1: Create the `AskCard` component

**Files:**
- Create: `src/ui/ask_card.rs`
- Modify: `src/ui/mod.rs` (add `pub mod ask_card;`)

**Interfaces:**
- Consumes: nothing (new leaf module).
- Produces:
  - `pub enum AskFocus { Doc, Ask }` (derives `Clone, Copy, PartialEq, Eq`)
  - `pub struct AskCard`
  - `AskCard::new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> AskCard`
  - `fn container(&self) -> &gtk4::Box`
  - `fn input(&self) -> &gtk4::TextView`
  - `fn open(&self, title: &str, hint: &str, card_width: i32)`
  - `fn close(&self)`
  - `fn is_open(&self) -> bool`
  - `fn focus(&self) -> AskFocus`
  - `fn toggle_focus(&self)`
  - `fn take_text(&self) -> String`

- [ ] **Step 1: Write `src/ui/ask_card.rs`**

```rust
use gtk4::prelude::*;
use gtk4::{Align, Label, ScrolledWindow, TextView};
use std::cell::Cell;

/// Which side of a "<document> + ask" overlay holds keyboard focus.
/// `Doc` = the synopsis/gloss card or journal page; `Ask` = the input field.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Doc,
    Ask,
}

/// The shared multi-line "ask" input card, stacked below a synopsis/gloss or
/// journal card. Built with the canonical synopsis values; both overlays embed
/// one and delegate their `*_ask_*` methods to it so the two cards can't drift.
pub struct AskCard {
    container: gtk4::Box,
    title: Label,
    input: TextView,
    hint: Label,
    focus: Cell<AskFocus>,
    return_focus: gtk4::Widget,
}

impl AskCard {
    /// Build the card with the canonical synopsis values. `return_focus` is the
    /// document-side widget that GTK focus returns to when leaving the input
    /// (gloss: its scroller; journal: its page view).
    pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("ask-card");
        container.set_margin_top(14);
        container.set_margin_start(text_margins);
        container.set_margin_end(text_margins);
        container.set_margin_bottom(14);

        let title = Label::new(Some(""));
        title.add_css_class("gloss-header");
        title.set_halign(Align::Start);
        title.set_margin_start(16);
        title.set_margin_top(12);
        container.append(&title);

        let scrolled = ScrolledWindow::new();
        scrolled.set_min_content_height(72);
        scrolled.set_max_content_height(160);
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_margin_start(16);
        scrolled.set_margin_end(16);
        scrolled.set_margin_top(6);
        scrolled.set_margin_bottom(6);

        let input = TextView::new();
        input.set_editable(true);
        input.set_cursor_visible(true);
        input.set_wrap_mode(gtk4::WrapMode::Word);
        input.set_top_margin(6);
        input.set_bottom_margin(6);
        input.set_left_margin(6);
        input.set_right_margin(6);
        input.add_css_class("gloss-text");
        input.add_css_class("ask-input");
        scrolled.set_child(Some(&input));
        container.append(&scrolled);

        let hint = Label::new(Some(""));
        hint.add_css_class("ask-hint");
        hint.set_halign(Align::Center);
        hint.set_margin_bottom(10);
        container.append(&hint);

        container.set_visible(false);

        Self {
            container,
            title,
            input,
            hint,
            focus: Cell::new(AskFocus::Doc),
            return_focus: return_focus.clone().upcast(),
        }
    }

    /// The card box; the embedding overlay appends this into its own card column.
    pub fn container(&self) -> &gtk4::Box {
        &self.container
    }

    /// The input TextView — exposed so each overlay applies its own font.
    pub fn input(&self) -> &TextView {
        &self.input
    }

    /// Reveal with heading + hint, clear the field, re-align margins to
    /// card_width/4, focus the input (AskFocus::Ask + card-focused highlight).
    pub fn open(&self, title: &str, hint: &str, card_width: i32) {
        self.title.set_text(title);
        self.hint.set_text(hint);
        self.input.buffer().set_text("");
        if card_width > 0 {
            let margin = card_width / 4;
            self.container.set_margin_start(margin);
            self.container.set_margin_end(margin);
        }
        self.container.set_visible(true);
        self.set_focus(AskFocus::Ask);
    }

    /// Hide, set AskFocus::Doc, drop the highlight, return focus to return_focus.
    pub fn close(&self) {
        self.container.set_visible(false);
        self.focus.set(AskFocus::Doc);
        self.container.remove_css_class("card-focused");
        self.container.remove_css_class("card-dimmed");
        if self.input.has_focus() {
            let _ = self.return_focus.grab_focus();
        }
    }

    pub fn is_open(&self) -> bool {
        self.container.is_visible()
    }

    pub fn focus(&self) -> AskFocus {
        self.focus.get()
    }

    /// Flip Doc<->Ask (no-op if closed). Owns the card-focused/card-dimmed
    /// highlight swap and the input grab / return-focus grab.
    pub fn toggle_focus(&self) {
        if !self.is_open() {
            return;
        }
        let next = match self.focus.get() {
            AskFocus::Doc => AskFocus::Ask,
            AskFocus::Ask => AskFocus::Doc,
        };
        self.set_focus(next);
    }

    fn set_focus(&self, focus: AskFocus) {
        self.focus.set(focus);
        match focus {
            AskFocus::Ask => {
                self.container.remove_css_class("card-dimmed");
                self.container.add_css_class("card-focused");
                self.input.grab_focus();
            }
            AskFocus::Doc => {
                self.container.remove_css_class("card-focused");
                self.container.add_css_class("card-dimmed");
                if self.input.has_focus() {
                    let _ = self.return_focus.grab_focus();
                }
            }
        }
    }

    /// Read and clear the input's text.
    pub fn take_text(&self) -> String {
        let buffer = self.input.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        buffer.set_text("");
        text
    }
}
```

- [ ] **Step 2: Register the module in `src/ui/mod.rs`**

Add this line in the alphabetical/grouped neighborhood of the other overlay modules (after `pub mod action_popup;` keeps it near the top; exact position is cosmetic):

```rust
pub mod ask_card;
```

- [ ] **Step 3: Build to verify the component compiles**

Run: `cargo build`
Expected: clean build (warnings about the unused new module are acceptable at this task; nothing references it yet). If a `dead_code` warning appears for `AskCard`, that is expected and resolved by Tasks 2-3.

- [ ] **Step 4: Commit**

```bash
git add src/ui/ask_card.rs src/ui/mod.rs
git commit -m "feat(ui): shared AskCard component + AskFocus { Doc, Ask }"
```

---

### Task 2: Embed `AskCard` in `GlossOverlay` (no behavior change)

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

**Interfaces:**
- Consumes: `crate::ui::ask_card::{AskCard, AskFocus}` from Task 1.
- Produces (unchanged signatures; now delegating): `open_ask_card()`, `open_ask_card_with(&self, title: &str, hint: &str)`, `close_ask_card()`, `take_ask_text() -> String`, `toggle_ask_focus()`, `ask_is_open() -> bool`, `ask_focus() -> crate::ui::ask_card::AskFocus`, `apply_font()`.

This task replaces the gloss overlay's five `ask_*` fields + local `AskFocus` enum with a single `ask: AskCard`, keeping every public method name as a thin wrapper. **No visual change** — same canonical values, same `card_width/4` margin, same return-to-`gloss_scrolled`.

- [ ] **Step 1: Add the import and replace the field declarations**

At the top of `src/ui/gloss_overlay.rs`, add to the existing `use` block. `AskFocus` is named in this file's signatures and is also imported by `keymap.rs` as `crate::ui::gloss_overlay::AskFocus`; a single `pub use` both scopes it locally and re-exports it (Task 4 removes the re-export):

```rust
use crate::ui::ask_card::AskCard;
pub use crate::ui::ask_card::AskFocus;
```

Then delete the local `AskFocus` enum (currently at `src/ui/gloss_overlay.rs:102-105`):

```rust
/// Focus target while the synopsis "ask" card is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Synopsis,
    Ask,
}
```

In the struct definition (currently `src/ui/gloss_overlay.rs:86-99`), replace the five field declarations + their doc comments:

```rust
    /// "Ask about this scene" card, stacked below the synopsis card (inside the
    /// same `container`, after the footer). Hidden unless the reader pressed `A`
    /// while the synopsis card is open. `ask_input` is an editable TextView that
    /// receives typed characters when the ask card holds focus.
    ask_container: gtk4::Box,
    ask_input: gtk4::TextView,
    /// Heading + footer hint of the ask card. Mutable so the same stacked card
    /// can serve both the synopsis "ask" flow and the gloss add/edit prompts,
    /// each with its own label/hint text.
    ask_title: Label,
    ask_hint: Label,
    /// Which sub-card currently has focus while the ask card is open. Drives the
    /// `.card-focused` highlight and whether `j/k` scroll vs. type.
    ask_focus: Cell<AskFocus>,
```

with the single field:

```rust
    /// Shared "ask" input card, stacked below the synopsis/gloss card inside the
    /// same `container` (after the footer). Serves both the synopsis "ask" flow
    /// and the gloss add/edit prompts. See `crate::ui::ask_card::AskCard`.
    ask: AskCard,
```

- [ ] **Step 2: Replace the widget-build block with an `AskCard::new` + append**

Replace the ask-card widget construction (currently `src/ui/gloss_overlay.rs:378-427`, from the `// ---- "Ask about this scene" card` comment through `container.append(&ask_container);`) with:

```rust
        // ---- Shared "ask" input card, stacked below the synopsis -------------
        // Lives inside `container` so the two cards form one centered column and
        // the synopsis scroll viewport (which vexpands) shrinks to make room when
        // this card is revealed. Hidden until `A` opens it. Built from the
        // canonical values inside `AskCard`; focus returns to `gloss_scrolled`.
        let ask = AskCard::new(text_margins as i32, &gloss_scrolled);
        container.append(ask.container());
```

`gloss_scrolled` is built at `src/ui/gloss_overlay.rs:162` (well before this block), so `&gloss_scrolled` is in scope here. The append order in `container` must remain: title/header, synopsis scrolled (`gloss_scroll_overlay`, appended at line 344), `footer_box` (line 376), **then the ask card last**.

- [ ] **Step 3: Update the struct initializer**

In the `Self { … }` initializer (currently `src/ui/gloss_overlay.rs:471-473` lists `ask_container, ask_input, ask_title, …`), remove the five fields:

```rust
            ask_container,
            ask_input,
            ask_title,
            ask_hint,
            ask_focus: Cell::new(AskFocus::Synopsis),
```

and add:

```rust
            ask,
```

- [ ] **Step 4: Rewrite the public methods as thin wrappers**

Replace the contiguous block at `src/ui/gloss_overlay.rs:1633-1739` — the `ask_is_open` accessor (1633), `ask_focus` accessor (1638), then `open_ask_card`, `open_ask_card_with`, `close_ask_card`, `take_ask_text`, `toggle_ask_focus`, and `set_ask_focus` (through 1739) — with:

```rust
    /// Reveal the ask card below the synopsis with the canonical heading + hint.
    pub fn open_ask_card(&self) {
        self.open_ask_card_with(
            "ASK ABOUT THIS SCENE",
            "Ask a question; the synopsis will be expanded to answer it  ·  Tab switch  ·  Ctrl+Enter submit  ·  Esc cancel",
        );
    }

    /// Reveal the stacked input card below the open synopsis/gloss card with the
    /// given heading and footer hint. Shared by the synopsis "ask" flow and the
    /// gloss add/edit prompts.
    pub fn open_ask_card_with(&self, title: &str, hint: &str) {
        let (card_width, _) = self.last_card_size.get();
        self.ask.open(title, hint, card_width);
        self.apply_font();
    }

    /// Hide the ask card and return focus + highlight to the synopsis.
    pub fn close_ask_card(&self) {
        self.ask.close();
    }

    /// Read and clear the ask input's text.
    pub fn take_ask_text(&self) -> String {
        self.ask.take_text()
    }

    /// Flip focus between the synopsis and the ask card. No-op if closed.
    pub fn toggle_ask_focus(&self) {
        self.ask.toggle_focus();
    }

    pub fn ask_is_open(&self) -> bool {
        self.ask.is_open()
    }

    pub fn ask_focus(&self) -> AskFocus {
        self.ask.focus()
    }
```

The block at 1633-1739 is contiguous, so this single replacement covers every method. After it, confirm exactly one definition of each method name remains: `rg -n "fn ask_is_open|fn ask_focus|fn open_ask_card|fn close_ask_card|fn take_ask_text|fn toggle_ask_focus|fn set_ask_focus" src/ui/gloss_overlay.rs` — `set_ask_focus` must have zero matches; the rest exactly one each.

- [ ] **Step 5: Update `apply_font` to style `self.ask.input()`**

In `apply_font` (`src/ui/gloss_overlay.rs:490`), wherever it currently applies the font tag to `self.ask_input`, change the reference to `self.ask.input()`. Find it with:

```bash
rg -n "ask_input" src/ui/gloss_overlay.rs
```

After this task, `ask_input`, `ask_title`, `ask_hint`, `ask_container`, `ask_focus`, and `set_ask_focus` must have zero remaining references in the file.

- [ ] **Step 6: Build and check for stragglers**

Run: `cargo build`
Expected: clean. The `pub use crate::ui::ask_card::AskFocus;` added in Step 1 keeps `keymap.rs`'s existing `use crate::ui::gloss_overlay::AskFocus;` resolving, so this task builds on its own.

Run: `rg -n "ask_input|ask_title|ask_hint|ask_container|set_ask_focus" src/ui/gloss_overlay.rs`
Expected: no matches (all replaced).

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "refactor(gloss): embed shared AskCard; delete local ask widgets/enum"
```

---

### Task 3: Embed `AskCard` in `JournalOverlay` (conform to canonical card)

**Files:**
- Modify: `src/ui/journal_overlay.rs`
- Modify: `src/input/actions/journal.rs:178,188` (hint strings)

**Interfaces:**
- Consumes: `crate::ui::ask_card::{AskCard, AskFocus}` from Task 1.
- Produces (unchanged names, now delegating): `open_ask_card(&self, title: &str, hint: &str)`, `close_ask_card()`, `toggle_ask_focus()`, `take_ask_text() -> String`, `ask_is_open() -> bool`, `ask_focus() -> crate::ui::ask_card::AskFocus`, `apply_font()`.

Net effect: the journal card gains the canonical margins, centered hint, and the `card-focused`/`card-dimmed` focus highlight it currently lacks. The journal `open_ask_card` keeps its `(title, hint)` signature; it passes the journal's own `last_card_size.0` as `card_width` to `AskCard::open`.

- [ ] **Step 1: Replace imports + delete the local enum**

At the top of `src/ui/journal_overlay.rs`, replace:

```rust
use gtk4::prelude::*;
use gtk4::{Label, Overlay};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Page,
    Ask,
}
```

with:

```rust
use crate::ui::ask_card::AskCard;
use gtk4::prelude::*;
use gtk4::{Label, Overlay};
use std::cell::{Cell, RefCell};

// `AskFocus` is named in this file's method signatures AND imported by keymap.rs
// as `crate::ui::journal_overlay::AskFocus`. A single `pub use` both brings the
// type into local scope and re-exports it, so this task builds clean on its own.
// Task 4 repoints keymap and removes this re-export.
pub use crate::ui::ask_card::AskFocus;
```

Keep `use std::cell::{Cell, RefCell};` — the overlay's other fields (`font_size`, `last_card_size`, `font_family`) still need `Cell`/`RefCell`.

- [ ] **Step 2: Replace the five `ask_*` fields with one `ask: AskCard`**

In the struct (`src/ui/journal_overlay.rs:26-30`), replace:

```rust
    ask_container: gtk4::Box,
    ask_input: gtk4::TextView,
    ask_title: Label,
    ask_hint: Label,
    ask_focus: Cell<AskFocus>,
```

with:

```rust
    ask: AskCard,
```

- [ ] **Step 3: Replace the ask-card build block**

Replace the widget construction (`src/ui/journal_overlay.rs:118-147`, from `let ask_container = …` through `container.append(&ask_container);`) with:

```rust
        // Shared "ask" input card (canonical synopsis values), stacked last in
        // the column. Focus returns to the page view when leaving the input.
        let ask = AskCard::new(text_margins as i32, &view);
        container.append(ask.container());
```

`view` is already in scope at this point (the journal page TextView, built at `src/ui/journal_overlay.rs:70`).

- [ ] **Step 4: Update the struct initializer**

In `Self { … }` (`src/ui/journal_overlay.rs:164-168`), replace:

```rust
            ask_container,
            ask_input,
            ask_title,
            ask_hint,
            ask_focus: Cell::new(AskFocus::Page),
```

with:

```rust
            ask,
```

- [ ] **Step 5: Fix `hide()` to use the component**

In `hide()` (`src/ui/journal_overlay.rs:253-258`), replace the two ask lines:

```rust
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Page);
```

with:

```rust
        self.ask.close();
```

The `show_page`, `show_loading`, and `show_message` methods call `self.ask_container.set_visible(false)` to hide the card when re-rendering — replace each of those three lines with `self.ask.close();`. Find them with:

```bash
rg -n "ask_container" src/ui/journal_overlay.rs
```

- [ ] **Step 6: Update `apply_font` to style `self.ask.input()`**

In `apply_font` (`src/ui/journal_overlay.rs:325-345`), the loop iterates `[&self.view, &self.ask_input]`. Change it to `[&self.view, self.ask.input()]`.

- [ ] **Step 7: Rewrite the public ask methods as thin wrappers**

Replace `ask_is_open`, `ask_focus`, `open_ask_card`, `close_ask_card`, `toggle_ask_focus`, `take_ask_text` (`src/ui/journal_overlay.rs:347-389`) with:

```rust
    pub fn ask_is_open(&self) -> bool {
        self.ask.is_open()
    }

    pub fn ask_focus(&self) -> AskFocus {
        self.ask.focus()
    }

    pub fn open_ask_card(&self, title: &str, hint: &str) {
        let (card_width, _) = self.last_card_size.get();
        self.ask.open(title, hint, card_width);
        self.apply_font();
    }

    pub fn close_ask_card(&self) {
        self.ask.close();
    }

    pub fn toggle_ask_focus(&self) {
        self.ask.toggle_focus();
    }

    pub fn take_ask_text(&self) -> String {
        self.ask.take_text()
    }
```

- [ ] **Step 8: Update the journal hint strings to the canonical convention**

In `src/input/actions/journal.rs`, replace the two hints so they carry the **Tab switch** affordance the shared card now supports.

Line 178 — change:

```rust
        .open_ask_card(title, "Ctrl+Enter to ask · Esc to cancel");
```

to:

```rust
        .open_ask_card(title, "Tab switch  ·  Ctrl+Enter submit  ·  Esc cancel");
```

Line 188 — change:

```rust
        .open_ask_card("Edit: ask a new question for this page", "Ctrl+Enter · Esc");
```

to:

```rust
        .open_ask_card(
            "Edit: ask a new question for this page",
            "Tab switch  ·  Ctrl+Enter submit  ·  Esc cancel",
        );
```

- [ ] **Step 9: Build and check for stragglers**

Run: `cargo build`
Expected: clean.

Run: `rg -n "ask_input|ask_title|ask_hint|ask_container|ask_focus: Cell" src/ui/journal_overlay.rs`
Expected: no matches.

- [ ] **Step 10: Commit**

```bash
git add src/ui/journal_overlay.rs src/input/actions/journal.rs
git commit -m "refactor(journal): embed shared AskCard; gain focus highlight + canonical hint"
```

---

### Task 4: Unify `AskFocus` references in `keymap.rs`

**Files:**
- Modify: `src/input/keymap.rs:655,675,775,800,1143,1184`

**Interfaces:**
- Consumes: `crate::ui::ask_card::AskFocus` (variant `Ask`; `Synopsis`/`Page` no longer exist).
- Produces: nothing new.

The handlers compare `ask_focus == AskFocus::Ask`. The variant `Ask` is unchanged, so the only required change is the `use` import path and removing reliance on `Synopsis`/`Page`. Because Tasks 2 and 3 added `pub use crate::ui::ask_card::AskFocus;` re-exports, the existing imports already resolve — but point them at the canonical module to avoid confusion and to let the re-exports be removed later.

- [ ] **Step 1: Repoint the three `use` imports**

`src/input/keymap.rs:655` — change:

```rust
    use crate::ui::journal_overlay::AskFocus;
```

to:

```rust
    use crate::ui::ask_card::AskFocus;
```

`src/input/keymap.rs:775` and `src/input/keymap.rs:1143` — change both:

```rust
    use crate::ui::gloss_overlay::AskFocus;
```

to:

```rust
    use crate::ui::ask_card::AskFocus;
```

The three comparisons (`AskFocus::Ask` at lines 675, 800, 1184) need no change — `Ask` is a variant of the unified enum.

- [ ] **Step 2: Remove the now-unused re-exports**

Delete the `pub use crate::ui::ask_card::AskFocus;` lines added to `gloss_overlay.rs` (Task 2 Step 6) and `journal_overlay.rs` (Task 3 Step 1), since `keymap.rs` now imports directly from `ask_card`. Confirm nothing else imports `AskFocus` from the overlay modules:

```bash
rg -n "gloss_overlay::AskFocus|journal_overlay::AskFocus" src/
```

Expected: no matches. If any remain (e.g. in `actions/`), repoint them to `crate::ui::ask_card::AskFocus` before deleting the re-exports.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: clean.

- [ ] **Step 4: Run the bins test suite**

Run: `cargo test --bins`
Expected: PASS, 413 tests (unchanged — no pure tests cover GTK widget construction or the keymap logic, which is unchanged).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/ui/gloss_overlay.rs src/ui/journal_overlay.rs
git commit -m "refactor: unify AskFocus on ask_card module; drop overlay re-exports"
```

---

## Out of scope

- The single-line picker/search `Entry` filters (different widget class).
- Footer/hint-row standardization, the Picker trait, the `run_claude_request` bridge, sentinel-key centralization — separate audit items.
- No change to the Claude request flow, persistence, or rendering.

## Final verification (manual / user-run)

`cargo build` clean and `cargo test --bins` at 413 are the automated gates. The visual acceptance — the journal ask-card now matching the synopsis card (centered Tab-switch hint, `card-focused` accent when focused / `card-dimmed` on the page, correct margins; Tab toggles, Ctrl+Enter submits, Esc closes; synopsis/gloss card unchanged) — requires a `cage` run and is for the user to confirm per the project's headless-verification protocol.
