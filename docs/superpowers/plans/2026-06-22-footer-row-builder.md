# Shared gloss/journal footer-row builder — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the identical gloss/journal footer-row construction into one `build_footer_row` helper returning a `FooterRow` struct, with zero behavior change.

**Architecture:** New module `src/ui/footer.rs` holds `FooterRow { container, left, hint }` and `build_footer_row(text_margins, hint_text)`. `GlossOverlay` and `JournalOverlay` call it and bind the returned widgets into their existing struct fields. Pure GTK widget construction, one task.

**Tech Stack:** Rust, GTK4 (`gtk4::Box`, `gtk4::Label`, `Align`).

**Spec:** `docs/superpowers/specs/2026-06-22-footer-row-builder-design.md`

## Global Constraints

- **No behavior change.** The produced widget tree (box margins/CSS, left+right label settings, append order) must be identical to today. Reviewer diffs the helper against both originals.
- **Do NOT touch** `concordance_bar.rs`, `vocab_popup.rs`, `library_picker.rs`, or any other footer — out of scope.
- **No keybind change** → hint *text* strings are copied verbatim from current source; do NOT alter any keybind/hint wording. Do NOT touch `keybinds_overlay.rs`, `keymap_config.rs`, `keymap.json`.
- New module registered as `pub mod footer;` in `src/ui/mod.rs` (match the existing `pub mod <name>;` style, NOT `pub(crate) mod`).
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): use `rg`/`fd`, not `grep`/`find`; bypass `mv`/`cp`/`rm` aliases with `\mv -f`/`\cp -f`/`command rm -f`.

---

### Task 1: Add `build_footer_row`; convert gloss + journal footers

**Files:**
- Create: `src/ui/footer.rs`
- Modify: `src/ui/mod.rs` (register `pub mod footer;`)
- Modify: `src/ui/gloss_overlay.rs` (footer construction ~330–353)
- Modify: `src/ui/journal_overlay.rs` (footer construction ~85–107)

**Interfaces:**
- Produces: `pub(crate) struct FooterRow { pub container: gtk4::Box, pub left: gtk4::Label, pub hint: gtk4::Label }` and `pub(crate) fn build_footer_row(text_margins: i32, hint_text: &str) -> FooterRow` in `crate::ui::footer`.
- Consumes: existing struct fields `GlossOverlay.citation_label: Label` / `GlossOverlay.position_label: Label`; `JournalOverlay.footer_left: Label`.

- [ ] **Step 1: Re-read both footer blocks before editing**

Run `rg -n "footer_box|citation_label|footer_left|footer_hint|position_label" src/ui/gloss_overlay.rs src/ui/journal_overlay.rs` and read the two construction blocks (gloss ~330–357, journal ~85–107). Confirm the shared shape (box: `gloss-hint`, margins text_margins/12/12; left label `halign Start` + `hexpand`; hint label `halign End` + `margin_end 12`) and the divergences (gloss hides left + appends `position_label` third; journal left visible). Treat live code as source of truth.

- [ ] **Step 2: Create `src/ui/footer.rs`**

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

- [ ] **Step 3: Register the module in `src/ui/mod.rs`**

Add `pub mod footer;` alongside the other `pub mod <name>;` lines (place it in the grouping/ordering the file uses — e.g. near `ask_card`).

- [ ] **Step 4: Convert the gloss footer (`gloss_overlay.rs` ~330–353)**

Replace the `footer_box` + `citation_label` + `hint` construction (NOT the `position_label` lines that follow) with:

```rust
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "Esc close · A add · E edit · D delete · c copy id · Ctrl+n/p passage · Alt+n/p gloss",
        );
        let footer_box = footer.container;
        let citation_label = footer.left;
        citation_label.set_visible(false);
```

Then leave the existing `position_label` creation and its `footer_box.append(&position_label);` exactly as they are, and leave the point where `footer_box` is appended to `container` unchanged. `citation_label` continues to be stored in the struct field as before (now sourced from `footer.left`). Confirm the hint string matches the live source byte-for-byte (copy it from the file in Step 1, do not retype from memory — the middot is `·`).

- [ ] **Step 5: Convert the journal footer (`journal_overlay.rs` ~85–107)**

Replace the `footer_box` + `footer_left` + `footer_hint` construction with:

```rust
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} A add \u{00b7} E edit \u{00b7} D delete",
        );
        let footer_left = footer.left;
        container.append(&footer.container);
```

**Copy the exact hint string from the live source** (Step 1) — preserve the `Ctrl+\` (a single backslash, `\\` in the literal) and whatever middot encoding the file uses (`·` or `\u{00b7}`). `footer_left` continues to be stored in the struct field as before.

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: `Finished`, no errors. The helper is used at both sites so no dead_code. Fix any unused-import warning (e.g. if `Align` is no longer used at the top of gloss_overlay.rs/journal_overlay.rs after the move — only remove it if `rg "Align" <file>` shows no remaining use).

- [ ] **Step 7: Clippy**

Run: `cargo clippy`
Expected: no new warnings.

- [ ] **Step 8: Tests**

Run: `cargo test --bins`
Expected: same pass count as before (413 at last check), 0 failed.

- [ ] **Step 9: Confirm scope**

Run: `git diff --stat` and confirm only `src/ui/footer.rs`, `src/ui/mod.rs`, `src/ui/gloss_overlay.rs`, `src/ui/journal_overlay.rs` changed. Run `git diff src/ui/concordance_bar.rs src/ui/vocab_popup.rs src/ui/library_picker.rs` and confirm it is EMPTY.

- [ ] **Step 10: Commit**

```bash
git add src/ui/footer.rs src/ui/mod.rs src/ui/gloss_overlay.rs src/ui/journal_overlay.rs
git commit -m "refactor(ui): extract shared gloss/journal footer-row builder

New src/ui/footer.rs build_footer_row returns FooterRow { container, left,
hint } for the identical gloss+journal gloss-hint footer. Both overlays bind
the returned widgets into their existing citation_label/footer_left fields;
gloss still appends its position_label third and hides the left label.
Pure widget-construction extraction, no behavior change. concordance_bar/
vocab_popup/library_picker footers intentionally untouched.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

## Verification (after the task)

- `cargo build` + `cargo clippy` clean, `cargo test --bins` green.
- Reviewer confirms: the helper's box/label settings match both originals exactly; gloss still hides `citation_label` and appends `position_label` third; journal `footer_left` stays visible; hint strings are byte-identical to the originals; only the 4 intended files changed; the excluded footers are untouched.
- **User cage pass:** open the gloss overlay (hint far-right, citation far-left once a passage is open, N/M counter present) and the journal overlay (work/scene left, hint right) — both identical to before.
