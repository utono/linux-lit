# Shared AskCard Component — Input-Card Unification

**Date:** 2026-06-22
**Status:** Approved
**Branch:** to be created off `master` (e.g. `refactor/shared-ask-card`)

Extract the synopsis/gloss "ask" input card into one reusable `AskCard`
component that both the gloss overlay and the journal overlay embed, so the two
hand-rolled input cards can no longer drift. The synopsis/gloss ask-card is the
designated standard; the journal card conforms to it.

## Background

Two multi-line "ask" input cards exist today:

- **Canonical** — built in `src/ui/gloss_overlay.rs` (~lines 382-424 for the
  widgets; methods `open_ask_card`/`open_ask_card_with` ~1646, `close_ask_card`
  ~1679, `take_ask_text` ~1691, `toggle_ask_focus` ~1702, `set_ask_focus`
  ~1717). Shared by the synopsis "ask" flow and the gloss add/edit prompts (same
  widgets, only title/hint strings differ). It manages a `card-focused`/
  `card-dimmed` highlight, re-aligns its left/right margins to `card_width/4` on
  open, and returns GTK focus to its scroll viewport (`gloss_scrolled`) when
  focus leaves the input.
- **Divergent** — built in `src/ui/journal_overlay.rs` (~lines 118-147 widgets;
  methods `open_ask_card` ~355, `close_ask_card` ~365, `toggle_ask_focus` ~370,
  `take_ask_text` ~381). Differs in ~12 ways: missing container/title/scrolled
  margins, TextView inner margins 12/8 (vs 6), extra `vexpand`+vscroll, hint
  `halign Start` (vs Center) with no bottom margin, hint wording that omits the
  "Tab switch" affordance it actually supports, **no `card-focused`/
  `card-dimmed` highlight at all**, and no `card_width/4` margin re-alignment.

Both define their own `AskFocus` enum (`Synopsis`/`Ask` vs `Page`/`Ask`).

## Design

### New component: `src/ui/ask_card.rs`

```rust
/// Which side of a "<document> + ask" overlay holds keyboard focus.
/// `Doc` = the synopsis/gloss card or journal page; `Ask` = the input field.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus { Doc, Ask }

pub struct AskCard { /* container, title, scrolled, input, hint, focus: Cell<AskFocus>, return_focus widget */ }

impl AskCard {
    /// Build the card with the canonical synopsis values. `return_focus` is the
    /// document-side widget that GTK focus returns to when leaving the input
    /// (gloss: its scroller; journal: its page view).
    pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self;

    /// The card box; the embedding overlay appends this into its own card column.
    pub fn container(&self) -> &gtk4::Box;

    /// The input TextView — exposed so each overlay applies its own font
    /// (gloss via apply_font; journal via its buffer-wide font TextTag).
    pub fn input(&self) -> &gtk4::TextView;

    /// Reveal with heading + hint, clear the field, re-align margins to
    /// card_width/4, focus the input (AskFocus::Ask + card-focused highlight).
    pub fn open(&self, title: &str, hint: &str, card_width: i32);

    /// Hide, set AskFocus::Doc, drop the highlight, return focus to return_focus.
    pub fn close(&self);

    pub fn is_open(&self) -> bool;
    pub fn focus(&self) -> AskFocus;

    /// Flip Doc<->Ask (no-op if closed). Owns the card-focused/card-dimmed
    /// highlight swap and the input grab / return-focus grab.
    pub fn toggle_focus(&self);

    /// Read and clear the input's text.
    pub fn take_text(&self) -> String;
}
```

**Widget build — canonical values, verbatim from the synopsis card:**

- container: `gtk4::Box` vertical, css `ask-card`, `margin_top 14`,
  `margin_start/end = text_margins`, `margin_bottom 14`, `set_visible(false)`.
- title: `Label`, css `gloss-header`, `halign Start`, `margin_start 16`,
  `margin_top 12`.
- scrolled: `ScrolledWindow`, `min_content_height 72`, `max_content_height 160`,
  hscrollbar `Never`, `margin_start/end 16`, `margin_top/bottom 6`.
- input: `TextView`, editable, cursor visible, wrap `Word`, all four inner
  margins `6`, css `gloss-text` + `ask-input`. (No `vexpand`, no explicit
  vscrollbar policy — matches the canonical card, drops the journal's extras.)
- hint: `Label`, css `ask-hint`, **`halign Center`**, `margin_bottom 10`.

**`open(title, hint, card_width)`** mirrors `open_ask_card_with`: set title/hint,
clear buffer, if `card_width > 0` set container `margin_start/end = card_width/4`,
`set_visible(true)`, focus = Ask.

**`toggle_focus` / focus management** mirrors `set_ask_focus`: on `Ask` remove
`card-dimmed` + add `card-focused` + `input.grab_focus()`; on `Doc` remove
`card-focused` + add `card-dimmed` + if the input has focus,
`return_focus.grab_focus()`. `close` does the `Doc` teardown and hides.

### Gloss overlay changes (`src/ui/gloss_overlay.rs`)

- Replace the five `ask_*` fields + `ask_focus` with a single `ask: AskCard`,
  built `AskCard::new(text_margins, &gloss_scrolled)` and appended via
  `container.append(self.ask.container())` at the same point in the tree.
- Re-point the public methods to the component (keep the existing method names
  as thin wrappers so callers in `actions/synopsis.rs` and `actions/gloss.rs`
  and `keymap.rs` don't all change):
  - `open_ask_card_with(title, hint)` → `self.ask.open(title, hint, self.last_card_size.get().0)`
  - `open_ask_card()` → calls the above with the canonical synopsis title/hint.
  - `close_ask_card()` → `self.ask.close()`
  - `take_ask_text()` → `self.ask.take_text()`
  - `toggle_ask_focus()` → `self.ask.toggle_focus()`
  - `ask_is_open()` → `self.ask.is_open()`
  - `ask_focus()` → maps `self.ask.focus()` to the shared enum (see below).
- `apply_font` keeps styling the input via `self.ask.input()`.
- Behavior preserved exactly: same values, same `card_width/4` margin, same
  return-to-`gloss_scrolled`. No visual change to the working synopsis/gloss card.

### Journal overlay changes (`src/ui/journal_overlay.rs`)

- Replace the `ask_*` fields + local `AskFocus` enum with `ask: AskCard`, built
  `AskCard::new(text_margins, &view)` (return focus to the page view), appended
  via `container.append(self.ask.container())` where the journal card currently
  appends `ask_container`.
- Re-point `open_ask_card(title, hint)` → `self.ask.open(title, hint, card_width)`
  (pass the card width the journal already computes), `close_ask_card` →
  `self.ask.close()`, `toggle_ask_focus` → `self.ask.toggle_focus()`,
  `take_ask_text` → `self.ask.take_text()`, `ask_is_open`/`ask_focus` likewise.
- `apply_font` keeps applying the `journal-font` TextTag to `self.ask.input()`.
- Net: journal gains the canonical margins, centered hint, and the
  `card-focused`/`card-dimmed` focus highlight it lacked.
- Update the journal ask hint strings (in `journal_overlay.rs` default and the
  overrides in `src/input/actions/journal.rs` ~178/188) to the canonical
  convention including the **Tab switch** affordance, e.g.
  `"Ask a question … · Tab switch · Ctrl+Enter submit · Esc cancel"`.

### Unify `AskFocus`

Replace `gloss_overlay::AskFocus { Synopsis, Ask }` and
`journal_overlay::AskFocus { Page, Ask }` with the single
`ask_card::AskFocus { Doc, Ask }`. Update the keymap match arms that read these
(`handle_journal_key`, `handle_gloss_key`/synopsis paths in `keymap.rs`) and any
re-export so `AskFocus::Ask`/`AskFocus::Doc` are used. Keep `GlossOverlay::ask_focus()`/
`JournalOverlay::ask_focus()` returning `ask_card::AskFocus` so call sites read
one enum.

### Keymap (`src/input/keymap.rs`)

No routing change — Tab→`toggle_ask_focus`, Ctrl+Enter→submit, Esc→
`close_prompt`/`close_ask_card` stay per-overlay and call the (unchanged-named)
overlay methods, which now delegate to `AskCard`. Only the `AskFocus` variant
names referenced in those handlers change (`Synopsis`/`Page` → `Doc`).

## Out of scope

- The single-line picker/search `Entry` filters (different widget class) — not
  touched.
- The footer/hint-row standardization, the Picker trait, the `run_claude_request`
  bridge, and sentinel-key centralization — separate audit items, separate work.
- No change to the Claude request flow, persistence, or rendering.

## Testing / acceptance

- `cargo build` clean; `cargo test --bins` still 413 (no pure tests cover GTK
  widget construction; the keymap logic is unchanged).
- Visual (user-run via `cage`): the journal ask-card now matches the synopsis
  ask-card — centered hint with the Tab-switch text, the `card-focused` accent
  on the input when focused and `card-dimmed` when focus is on the page, correct
  margins; Tab toggles focus, Ctrl+Enter submits, Esc closes. The synopsis/gloss
  ask-card is unchanged.
