# Next-scene watermark in trailing whitespace below filled content

**Date:** 2026-07-14
**Status:** Design — approved, ready for implementation plan

## Problem

The dimmed *"next: Act #, Scene #"* watermark already exists. Today it shows
**only when the right column of a two-column play spread is entirely empty** —
the scene ends on the left, the next spread opens a new scene, and the whole
right column is blank. The label is centered in that empty column.

The gap: when a scene ends **partway down the right column** — the column has
dialogue at the top but ends well before the bottom, leaving a large blank
region below — no watermark shows. The user wants the watermark to also appear
in that trailing whitespace, centered in the blank region below the content.

Concretely, the two reference screenshots:

- **Desired-result case (already working):** right column entirely empty; the
  dim *"next: Act 1, Scene 2"* is centered in it.
- **Target case (this change):** right column full of dialogue that ends
  partway down ("...Bring him to th' King."), with a large blank region below
  and no watermark.

## Existing implementation (what we extend, not rebuild)

- Entry point: `update_next_scene_watermark(state, &cs)` — `src/input/scroll.rs:455`,
  called from the two-column snap path at `scroll.rs:657`.
- The watermark is a `gtk4::Label` overlay child on `right_scrolled_overlay`
  (field `next_scene_watermark`, declared `src/app/mod.rs:313`, created
  `mod.rs:1311-1316`), currently `Align::Center` both axes.
- Styling is Pango markup, not CSS: dim italic ~120% of the reading font
  (`scroll.rs:498-505`), color `state.theme.dim_fg`.
- The next scene's label is derived authoritatively from DB metadata:
  `divs_at_buffer_line(state, cs.next_page_top)` → `scene_label_for(...)`
  (`src/app/scene_synopsis.rs:138-159`, `:455-467`).
- Scene boundaries come from `LineMap.section_starts` via
  `state.is_section_start(line)` — never inferred from text (repo rule:
  authoritative metadata, not text inference).

## Design

### 1. Trigger — relax `empty_right` into two show-cases

Current gate at `scroll.rs:484-489`:

```rust
let next_opens_scene =
    cs.next_page_top < line_count && state.is_section_start(cs.next_page_top);
let right_has_dialogue = (cs.split..=cs.page_end.min(line_count.saturating_sub(1)))
    .any(|l| ...is_dialogue_line...);
let empty_right = (cs.page_end < cs.split || (next_opens_scene && !right_has_dialogue))
    && cs.next_page_top < line_count;
```

The `!right_has_dialogue` clause is exactly what excludes the target case.
Split into two show-conditions:

- **Empty-column case (unchanged):** right column carries no dialogue
  (`page_end < split`, or `next_opens_scene && !right_has_dialogue`) and
  `next_page_top < line_count`.
- **Filled-with-trailing-whitespace case (new):**
  `next_opens_scene && right_has_dialogue && next_page_top < line_count`
  **and** the blank region below the last content line is at least a minimum
  threshold (default ~3 line-heights) so a column that ends one line short
  does not get a cramped label.

In both cases the scene-boundary requirement (`next_opens_scene`, i.e.
`next_page_top` is a DB `section_start`) stays authoritative — matching the
"only at a real scene boundary" decision. A mid-scene page turn shows nothing.

### 2. Positioning — centered in the blank region (one formula, both cases)

The blank band's top edge is the pixel Y where the last content line ends.
The right view is scrolled so `cs.split` sits at its top, so measure content
bottom the same way `scroll_right_view_to_split` measures line Y
(`scroll.rs:717-718`):

- `content_bottom_y` = viewport Y of the bottom of the last content line
  (`cs.page_end`): take `right_view.line_yrange(page_end_iter)` (y + h),
  subtract the scroll offset (the Y of `cs.split`). This is the same edge the
  `right_bottom_clip` uses to hide trailing lines, so the number is consistent
  with the clip.
- `remaining_height` = right column viewport height − `content_bottom_y`.

Place the label:

- `halign = Center`, `valign = Start` (top-anchored, margin-driven).
- `margin_top = content_bottom_y + (remaining_height − est_label_height) / 2`,
  where `est_label_height ≈ font_size * 1.2 * 1.4` (one rendered line; height
  is predictable, so the estimate reads as centered).

**One code path handles both cases.** When the right column has no visible
content (the empty-column case — `page_end < split`, or the scene-tail-only
case where the clipped trailing lines are hidden), clamp `content_bottom_y` to
the column top (0). The `margin_top` formula then degrades to "centered in the
whole column" — today's behavior. The empty case is the filled case with zero
content above; the clamp makes that explicit rather than relying on the
measured Y of clipped-away tail lines.

### 3. Recompute timing — measure against settled geometry

`update_next_scene_watermark` is called at `scroll.rs:657`, *before*
`scroll_right_view_to_split` (`:673-680`) and before layout settles, so
`line_yrange` there would read stale geometry.

- The **visibility + text** decision stays synchronous (no geometry needed).
- The **pixel positioning** (`content_bottom_y` → `margin_top`) moves into a
  deferred step that runs after the split scroll lands — mirroring the existing
  synchronous + `idle_add_local_once` + 100ms-backstop pattern already used for
  the right-view scroll (`scroll.rs:673-680`). Add the positioning re-run
  alongside those so it measures against final geometry.

Force-hide paths are unchanged: single-column / layout-not-ready
(`scroll.rs:703`) and layout transitions (`layout.rs:297`).

### 4. Styling — unchanged

Same dim italic Pango markup (`scroll.rs:498-505`): `theme.dim_fg`, italic,
~120% of reading font. `next: {label}` text and `scene_label_for` derivation
untouched. No new CSS.

### 5. Anthology suppression — unchanged

`state.is_anthology()` early-return stays (`scroll.rs:460`): anthology works
pack excerpts into both columns and have no act/scene structure.

## Files touched

- `src/input/scroll.rs`
  - `~484-489`: relax the trigger into empty-column + filled-trailing-whitespace
    show-cases, with the min-whitespace guard for the new case.
  - `update_next_scene_watermark` / the snap path: add `content_bottom_y`
    measurement and the `margin_top` formula; defer positioning to settled
    geometry via the existing idle/backstop pattern.
- `src/app/mod.rs`
  - `~1311-1316`: change the label alignment to `valign = Start`,
    `halign = Center` (margin-driven positioning).
- Tests (see below).

## Testing

Follow the clip-prevention rule — verify on the real render, not logs; the
label must never overlap the clipped trailing lines.

- **Headless e2e** (`test-headless-navigation` skill, via `scripts/e2e-env.sh`):
  drive to a play page where a scene ends mid-right-column with a following
  scene; screenshot; confirm the dim label is centered in the blank region
  below the dialogue and does not overlap it.
- **Regression:** the empty-right-column case still renders identically — same
  screenshot check on a page that ends the scene on the left with an empty
  right column.
- **Negative cases (no watermark):**
  - a mid-scene page turn (`next_page_top` is not a `section_start`);
  - a page whose right column is nearly full (trailing whitespace below the
    min threshold);
  - single-column / prose layouts;
  - anthology works.

## Non-goals (YAGNI)

- Single-column / prose trailing-whitespace watermark — out of scope; this is
  the two-column play case only.
- Any change to the label text, scene-label derivation, or theming.
- A configurable placement (centered is the chosen behavior).
