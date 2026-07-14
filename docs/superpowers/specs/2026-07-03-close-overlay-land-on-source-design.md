# Close overlay → land the cursor on the passage's source start

**Date:** 2026-07-03
**Status:** design, approved (proceeding to implement)

## Problem

When Ctrl+Tab / Ctrl+j / Ctrl+g (and Escape) close a gloss or journal overlay
back to the main reading card, the cursor is restored to the saved *return
position* (where you were reading before you opened the overlay). If the overlay
was showing a **passage that has source text from the current work**, it's more
useful to land the cursor on the **first line of that source text** — so closing
the overlay drops you at the passage you were just studying.

## Goal

On close-to-reader, if the closing overlay was showing a passage whose source
text belongs to the current work, position the cursor on the **first dialogue
line of that source** (via the canonical spread). Otherwise fall back to the
saved return position, exactly as today. Applies to gloss AND journal, on every
close path (Ctrl+Tab, reader Ctrl+g/Ctrl+j, in-overlay Ctrl+g/Ctrl+j, Escape).

## Decisions

- **Land on the first DIALOGUE line** (reuse the gloss helper's existing rule:
  from the source start, skip a leading stage direction / speaker to the first
  `is_dialogue` line, land via `jump_to_line`'s canonical spread).
- **Escape too (uniform):** every passage-page close lands on the source. The
  gloss already does this on Escape (`close_gloss_to_reader`); the journal will
  now match.
- **"From the current work" is enforced by resolution, not an explicit check:**
  the jump resolves the citation/text against `s.current_work.lines` and returns
  `false` if not found there — so a cross-work passage naturally falls back to
  the return position.

## What already exists

`jump_to_gloss_source_start(s: &mut AppState) -> bool`
(`src/input/actions/gloss.rs:20`) already does exactly this for the gloss:
resolves `gloss_context.start_citation` (unique `(div1,div2,line_in_div)` tuple
match; citationless text-match fallback) → first `is_dialogue` line → buffer
index → `jump_to_line`. Returns `true` on success, `false` when there's no
source / it isn't in `current_work`.

- **`close_gloss_to_reader`** (gloss Escape + in-overlay Ctrl+g) ALREADY calls
  it, falling back to `gloss_return_pos`. No change needed.
- **Gloss `toggle_overlay` close half** (reader Ctrl+g + the new Ctrl+Tab) does
  NOT — it restores `gloss_return_pos`. **← fix 1.**
- **Journal** has no equivalent; its `toggle_overlay` close always restores
  `journal.return_pos`. **← fix 2 (new helper + wire).**

## Design

### Fix 1 — gloss `toggle_overlay` close half (gloss.rs:2566)

Mirror `close_gloss_to_reader`: after `return_to_reader_mode`, try
`jump_to_gloss_source_start(&mut s)`; only restore `gloss_return_pos` when it
returns `false`.

```rust
if state.borrow().input_mode == crate::app::InputMode::GlossOverlay {
    let mut s = state.borrow_mut();
    s.tts.stop();
    s.gloss_overlay.hide();
    crate::app::return_to_reader_mode(&mut s);   // recolor tint before we move
    let jumped = jump_to_gloss_source_start(&mut s);
    let pos = s.gloss_return_pos.take();
    if !jumped {
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    return;
}
```

(Take `gloss_return_pos` regardless so it doesn't leak into the next open.)

### Fix 2 — journal source-jump helper + wire (journal.rs)

New `jump_to_journal_source_start(s: &mut AppState) -> bool`, parallel to the
gloss one, reading the CURRENT page:

```rust
pub(crate) fn jump_to_journal_source_start(s: &mut AppState) -> bool {
    let page = match s.journal.pages.get(s.journal.page_index) {
        Some(p) => p,
        None => return false,
    };
    // Only passage pages carry a source citation; scene/corpus notes don't.
    let start_citation = match &page.start_citation {
        Some(c) => c.clone(),
        None => return false,
    };
    let source_text = page.source_text.clone().unwrap_or_default();
    let target = crate::app::parse_citation(&start_citation);

    let work = match s.current_work.as_ref() { Some(w) => w, None => return false };

    let by_citation = target.and_then(|t|
        work.lines.iter().position(|l| (l.div1, l.div2, l.line_in_div) == t));
    // Journal source_text carries <speaker>/<verse> markup, so the text-match
    // fallback strips tags to the first bare line; citation match is primary.
    let first_src = first_plain_source_line(&source_text);
    let start_idx = match by_citation.or_else(|| {
        if first_src.is_empty() { None }
        else { work.lines.iter().position(|l| l.text.trim() == first_src) }
    }) { Some(i) => i, None => return false };

    let work_idx = work.lines[start_idx..].iter()
        .position(|l| l.is_dialogue)
        .map(|off| start_idx + off).unwrap_or(start_idx);
    let buf_idx = match s.line_map.as_ref() {
        Some(lm) => match lm.work_to_buffer.get(work_idx) { Some(&bi) => bi, None => return false },
        None => work_idx,
    };
    crate::input::navigation::jump_to_line(s, buf_idx);
    true
}
```

`first_plain_source_line` strips a leading `<speaker>…</speaker>` /
`<verse>`/`<stage>` tag pair to the first bare text line. (If a tag-stripping
helper already exists — `parse_gloss_tags`, or the passage-doc reconstruction —
reuse it; otherwise a small local strip. Citation match is primary, so this only
matters for citationless `.txt`-only works.)

Wire it into `toggle_overlay`'s close half (which Escape reaches via
`close_overlay`, so this covers Ctrl+Tab + Ctrl+j + Escape at once):

```rust
if state.borrow().input_mode == InputMode::JournalOverlay {
    let mut s = state.borrow_mut();
    s.journal_overlay.hide();
    crate::app::return_to_reader_mode(&mut s);   // recolor reader-gloss tint first
    let jumped = jump_to_journal_source_start(&mut s);
    let pos = s.journal.return_pos.take();
    if !jumped {
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    return;
}
```

## Files touched

- `src/input/actions/gloss.rs` — gloss `toggle_overlay` close half calls
  `jump_to_gloss_source_start`.
- `src/input/actions/journal.rs` — new `jump_to_journal_source_start` (+ small
  tag-strip helper if none reusable); journal `toggle_overlay` close half wires
  it.

No new AppState, no keybind/config changes, no overlay-legend changes.

## Out of scope / YAGNI

- Highlighting the whole source range (just land the cursor on the first line).
- Any change to synopsis / echoes / translation overlays.
- Reordering which line is "first" beyond the existing first-dialogue rule.

## Interaction with the Ctrl+Tab reopen

With fix 1, closing a gloss with Ctrl+Tab now lands the cursor ON the glossed
line — so the subsequent Ctrl+Tab reopen ("fresh from cursor") re-opens that same
gloss instead of toasting "No gloss on this line" (the wrinkle noted in the
prior Ctrl+Tab work). This is a strict improvement: the flip round-trips.

## Testing

- `cargo build` + `cargo clippy` + `cargo test --bins`.
- Headless (cage, `LIT_HEADLESS_TEST=1`): open a gloss via the picker (moves the
  cursor off the passage), Ctrl+Tab → cursor lands on the glossed line; Ctrl+Tab
  again → the SAME gloss reopens (no toast). Journal: open a passage-page Q&A,
  Escape → cursor lands on the passage's first dialogue line; a scene/corpus
  note close still restores the return position.
