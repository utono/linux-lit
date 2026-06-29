# Journal Overlay Footer — work/act/scene + keybind hints

**Date:** 2026-06-22
**Status:** Approved
**Branch:** `feat/qa-journal-overlay`

Add a footer rule to the journal overlay (mirroring the gloss overlay's
`footer_box`): the current Q&A page's location on the left, fixed keybind hints
on the right.

## Behavior

- **Footer-left** — identifies the current page:
  - Scene band → `<abbrev> <div1>.<div2>` (e.g. `2H6 1.4`).
  - Work band → `<abbrev> · whole work` (e.g. `2H6 · whole work`).
  - `<abbrev>` is `base_work_abbrev(&work.abbrev)` (so `2H6-Amb` shows `2H6`).
- **Footer-right** — fixed hint set on every page:
  `Alt+w work · Ctrl+\ pick · a add · e edit`.
- The footer shows whenever a page (or empty-band card) is displayed
  (`show_page`). It is irrelevant to `show_loading` / `show_message` (transient
  states) — those may leave the last footer text in place; not required to
  update it.

## Design

Mirror `GlossOverlay`'s footer exactly (`gloss_overlay.rs:346-374`): a
horizontal `gloss-hint` box appended to `container` after the scrolling
viewport, with `margin_start/end = text_margins`, `margin_top/bottom = 12`.

- A left `Label` (`halign Start`, `hexpand true`) holds the location string.
- A right `Label` (`halign End`, `margin_end 12`) holds the fixed hint string,
  set once at construction.

`show_page` gains a `footer_left: &str` parameter; the caller (`render_current`
in `src/input/actions/journal.rs`) builds it from the band:

```rust
let footer_left = match s.journal_band {
    JournalBand::Work => format!("{} · whole work", work_abbrev),
    JournalBand::Scene(d1, d2) => format!("{} {}.{}", work_abbrev, d1, d2),
};
```

`work_abbrev` is the `base_work_abbrev` string `render_current` already computes.

A pure helper makes the format unit-testable:

```rust
// src/input/actions/journal.rs
fn footer_left_text(abbrev: &str, band: JournalBand) -> String {
    match band {
        JournalBand::Work => format!("{} · whole work", abbrev),
        JournalBand::Scene(d1, d2) => format!("{} {}.{}", abbrev, d1, d2),
    }
}
```

## Out of scope
- No new keybinds (the hints describe existing ones).
- No footer on the picker (the picker has its own list UI).
- Page-position counter ("page N of M") stays where it is (the existing
  `position_label` under the title) — not moved into the footer.
