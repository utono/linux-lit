# Syntax diagram layout pass

## Purpose

Give the band annotations the vertical room the window already has. Every
annotation-collision defect fixed on 2026-07-26 — labels printed on their own
rules, POS tags struck through, sibling labels overprinting, labels suppressed
entirely — has one root cause: the stack is compressed into the font's natural
leading while most of the window sits empty.

## The measurement

On a 1920x1200 window, rendering a 5-band Bleak House sentence:

- Diagram content occupies **y=91..174** — 83px of 1160 usable, about **7%**.
- The POS legend sits at y=1137..1150.
- Between them, **963px — 83% of the window — is empty.**

Meanwhile `interior_row_height` was shrinking rows toward the 16px legibility
floor, and five sibling bands were sharing a single row because
`assign_rows` maps row directly to depth.

The fixes already committed (`736f838b`, `56b4f712`) are correct and stay. They
were tuning a starved layout.

## Two changes

### 1. Row height derives from available space, not from the text's leading

Today `line_spacing_for(rows, natural_line_h, clearance)` widens the TEXT
leading until the band stack fits in the gap under each wrapped line. That
couples annotation space to font metrics: deep nesting forces compression no
matter how much window is free.

Instead, the stack's height budget comes from the window:

```
budget = h - text_bottom - note_reserve - LEGEND_RESERVE
```

with a generous per-row target (~40px, against today's 16px floor). The stack
extends DOWN into the empty space rather than being squeezed into the leading.

`BAND_ROW_H` becomes a target rather than a ceiling reached only in the easy
case. The floor still exists and still degrades gracefully, but it now engages
only when a sentence is deep enough to exhaust 900+px — which a realistic band
count never does.

Text leading returns to natural. `line_spacing_for`'s widening was a workaround
for the starvation and is no longer the mechanism that makes stacks fit; it
stays only as the guard for the pathological case where a stack under an
INTERIOR wrapped line would otherwise overstrike the next line.

**Unchanged, because they were never wrong:** the label offset
(`lh + LABEL_GAP`), `MIN_BAND_ROW_H`, `LABEL_H`, `STACK_TOP_OFFSET`,
`POS_ROW_H`, `LEGEND_RESERVE`.

### 2. Rows pack by collision, not by depth

`assign_rows` currently returns `b.depth` verbatim:

```rust
pub fn assign_rows(bands: &[Band]) -> Vec<usize> {
    bands.iter().map(|b| b.depth as usize).collect()
}
```

So every band at depth 1 shares one row regardless of horizontal position.
Five disjoint appositives land on one line and their labels collide — the
defect still visible on the leftmost band after all the spacing fixes.

Replace with a first-fit packer. Iterate bands in (depth, start_char) order;
place each on the LOWEST row where its label extent clears everything already
on that row; open a new row when none fits.

Two properties this must hold:

- **Depth still drives ordering**, so nesting continues to read as depth. A
  band never packs onto a row above a shallower band.
- **The extent tested is the LABEL's, not the rule's.** Rules are narrow and
  rarely collide; labels are wide and routinely do. Testing rule extents would
  reproduce the current bug.

This makes the existing label-suppression logic (the `collides` check and the
width test in `draw_analysis`) a backstop for pathological input rather than
the routine mechanism. Labels stop disappearing — the over-correction recorded
in clip-prevention entry 15, where a strict width test suppressed 5 of 6
labels, becomes impossible to hit in normal use.

## Boundaries

`assign_rows` is already the right seam: pure, in `syntax_diagram.rs`, no GTK,
no Cairo. It gains one parameter — per-band label width — and keeps returning
`Vec<usize>` of row indices.

Label width must be measured by the CALLER: only the draw side can measure
Pango text, and `syntax_diagram.rs` must stay display-free so it remains
unit-testable. `draw_analysis` measures each label once (it already calls
`layout_text` per band) and passes the widths in.

Everything downstream keeps working on row indices unchanged: `max_row`,
`depth_offset`, `band_stack_bottom`, and the drawing loop.

Nothing else moves between modules. `syntax_overlay.rs` keeps its geometry
constants; `syntax_diagram.rs` keeps the pure data model.

## Testing

**Unit (`cargo test --bins`, no display), on the packer:**

- Disjoint siblings at one depth share a row.
- Siblings whose LABELS overlap get separate rows, even when their rules do not.
- Depth ordering is preserved: a band never lands above a shallower one.
- A single band is row 0; an empty band list yields no rows.
- A band whose label is wider than the whole content column still places
  (degrades, does not panic or loop).

**Unit, on row height:** the budget derives from window height, and a
realistic band count yields rows at the target height rather than the floor.

**On screen — the real GL renderer, not cage.** Cage disagreed with GL on every
one of these defects; a cage pass is not evidence here. Criteria:

1. The leftmost band's label clears its rule.
2. No label overprints another label, a rule, or a POS tag.
3. The stack uses the vertical space rather than the top 7%.
4. The POS legend stays clear of the stack.
5. A wrapped multi-line sentence (the Mr. George case) still puts each line's
   stack under ITS OWN line, not over the next.

## Non-goals

- **No change to band derivation or the prompt.** Same bands, laid out better.
- **No dependency arcs.** Bands only, per the original spec.
- **No scrolling.** The "never clips or scrolls" promise stands; the floor
  still degrades a pathological stack rather than clipping it.
- **No change to the POS legend, loading state, or scrim.** Shipped and
  verified this session.
- **Not the ambiguity-signalling gap.** An LLM returns one parse confidently
  with no signal the sentence was ambiguous. Still open, still needs its own
  spec.
