# Shared gloss/journal footer-row builder — design

## Goal

Extract the genuinely-identical footer-row construction shared by `GlossOverlay`
and `JournalOverlay` into one helper, so the `gloss-hint` footer geometry
(margins, the left-hexpand + right-hint layout) cannot drift between them. This is
audit opportunity #5, deliberately scoped to only these two overlays.

## Scope

After surveying every footer/hint site:

- **In scope:** `gloss_overlay.rs` and `journal_overlay.rs` — their footer rows are
  structurally IDENTICAL (same box settings, same left+right label layout, same
  `gloss-hint` CSS class). This is the one real duplication.
- **Explicitly EXCLUDED** (structurally different by design — merging would be a
  forced merge):
  - `concordance_bar.rs` — a single right-anchored hint label (`concordance-bar-hint`
    CSS), no `footer_box`, no left label, appended directly to its container.
  - `vocab_popup.rs` — `definition-hint` CSS, its own footer shape.
  - `library_picker.rs` — a level-driven model (`update_footer` + `footer_hints(level)`),
    fundamentally different from a fixed hint string.
  - Any picker with no footer — out of scope (adding footers is not this refactor).

## Current duplication (the extractable core)

Both overlays build, identically:

- `footer_box`: horizontal `gtk4::Box`, `set_margin_start(text_margins)`,
  `set_margin_end(text_margins)`, `set_margin_top(12)`, `set_margin_bottom(12)`,
  `add_css_class("gloss-hint")`.
- left label: `Label::new(None)`, `set_halign(Align::Start)`, `set_hexpand(true)`,
  appended first.
- hint label: `Label::new(Some(<hint text>))`, `set_halign(Align::End)`,
  `set_margin_end(12)`, appended second.

They DIFFER in:

- The hint **text** (gloss vs journal keybind strings).
- The left label's **initial visibility**: gloss calls `set_visible(false)` on its
  `citation_label` (shown later via `set_citation`); journal leaves `footer_left`
  visible (updated via `footer_left_text`).
- gloss appends a **third** widget into the same box after the hint —
  `position_label` (the N/M counter, `halign End`, initially hidden). Journal has
  no third widget.
- Where/when the box is appended to `container`, and how each stores the labels in
  its struct (gloss: `citation_label` field; journal: `footer_left` field;
  neither stores `footer_box` as a field).

## Component

A `pub mod footer;` module at `src/ui/footer.rs` (registered in `src/ui/mod.rs`
alongside the other `pub mod` lines — match the existing `pub mod <name>;` style,
NOT `pub(crate) mod`). It is pure GTK widget construction — no `AppState`, like
`ask_card.rs`.

```rust
use gtk4::prelude::*;
use gtk4::{Align, Label};

/// Handles to the shared "gloss-hint" footer row. The caller sets the left
/// label's text/visibility and may append further widgets (e.g. the gloss
/// position counter) into `container` before/after appending it to the column.
pub(crate) struct FooterRow {
    /// The footer box (`gloss-hint` class). Append to your card column.
    pub container: gtk4::Box,
    /// Left-anchored, hexpand label (citation / work-scene). Caller sets text
    /// and visibility, and typically stores it in its own struct for updates.
    pub left: Label,
    /// Right-anchored fixed keybind hint (text set from `hint_text`).
    pub hint: Label,
}

/// Build the gloss/journal footer row: a horizontal box (text_margins sides,
/// 12px top/bottom, `gloss-hint` class) with a left-anchored hexpand label and a
/// right-anchored hint label (`halign End`, `margin_end 12`). Left then hint are
/// appended into the box. Left-label visibility and any extra widgets are the
/// caller's responsibility.
pub(crate) fn build_footer_row(text_margins: i32, hint_text: &str) -> FooterRow {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.set_margin_start(text_margins);
    container.set_margin_end(text_margins);
    container.set_margin_top(12);
    container.set_margin_bottom(12);
    container.add_css_class("gloss-hint");

    let left = Label::new(None);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    container.append(&left);

    let hint = Label::new(Some(hint_text));
    hint.set_halign(Align::End);
    hint.set_margin_end(12);
    container.append(&hint);

    FooterRow { container, left, hint }
}
```

## Call-site changes

### `src/ui/gloss_overlay.rs` (~330–353)

Replace the `footer_box` + `citation_label` + `hint` construction with:

```rust
    let footer = crate::ui::footer::build_footer_row(
        text_margins as i32,
        "Esc close · A add · E edit · D delete · c copy id · Ctrl+n/p passage · Alt+n/p gloss",
    );
    let footer_box = footer.container;
    let citation_label = footer.left;
    citation_label.set_visible(false);
```

Then the existing `position_label` creation stays, and it is appended into
`footer_box` exactly as before (`footer_box.append(&position_label);`), and
`footer_box` is appended to `container` at the same point as today. `citation_label`
is stored in the struct field as before (now sourced from `footer.left`). The
`hint` label was never stored in a field — `footer.hint` is dropped after the box
owns it, identical to today.

### `src/ui/journal_overlay.rs` (~85–107)

Replace the `footer_box` + `footer_left` + `footer_hint` construction with:

```rust
    let footer = crate::ui::footer::build_footer_row(
        text_margins as i32,
        "Alt+w work · Ctrl+\\ pick · A add · E edit · D delete",
    );
    let footer_left = footer.left;
    container.append(&footer.container);
```

`footer_left` is stored in the struct field as before. The hint label
(`footer.hint`) is owned by the box; not stored, identical to today.

**Note on the `\\` in the journal hint:** the original source uses
`"Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} ..."`. Preserve the exact string —
the `Ctrl+\` is a single backslash in the rendered text (`\\` in a Rust string
literal). The middot may be written as `·` or `\u{00b7}`; keep whichever the live
code uses so the rendered text is byte-identical.

## Behavior preservation

The produced widget tree is identical to today: same box margins/CSS, same left
label (`halign Start`, `hexpand`), same hint label (`halign End`, `margin_end 12`),
same append order (left, then hint; gloss then appends `position_label` third).
gloss still hides its left label initially; journal's stays visible. The hint text
stays each overlay's own string, passed in. CSS class unchanged (`gloss-hint`).
The struct fields `citation_label` (gloss) and `footer_left` (journal) still hold
the left label, so the existing `set_citation` / `footer_left_text` setters keep
working unchanged.

## Global Constraints

- **No behavior change.** The widget config and tree must be identical; reviewer
  diffs the helper's settings against both originals.
- **Do NOT touch** `concordance_bar.rs`, `vocab_popup.rs`, `library_picker.rs`, or
  any other footer — out of scope.
- **No keybind change** → the hint *text* strings are copied verbatim from the
  current source; do NOT alter any keybind or hint wording. Do NOT touch
  `keybinds_overlay.rs`, `keymap_config.rs`, `keymap.json`.
- `src/ui/footer.rs` registered as `pub mod footer;` in `src/ui/mod.rs`.
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): `rg`/`fd` not `grep`/`find`; `\mv -f`/`\cp -f`/
  `command rm -f` for non-interactive overwrite/delete.

## Testing

Pure GTK widget construction; asserting geometry needs a realized widget (not
available in `cargo test --bins`), so **no new unit test** (consistent with
`ask_card.rs`; a fake test would assert nothing). Verification = build + clippy +
`cargo test --bins` green + reviewer widget-config equivalence + the user's cage
pass:

- Open the **gloss** overlay: the keybind hint sits far-right; the citation label
  renders at far-left once a passage is open (and is hidden before); the N/M
  position counter still appears.
- Open the **journal** overlay: the work/scene left label renders far-left, the
  keybind hint far-right — identical to before.

Per the headless-verification protocol, the agent cannot reliably drive cage on
the live dwl session, so this is handed to the user.

## Out of scope

- concordance_bar / vocab_popup / library_picker footers (structurally different).
- The remaining audit refactor #6 (Picker trait / `move_selection` ×17) — its own
  spec, a larger dedicated session.
