# Pinned source-turn header in the echoes overlay

**Date:** 2026-05-31
**Status:** Approved design, pending implementation plan

## Problem

In the echoes overlay the quoted source turn and the echo list live in one
scrolling `TextView`, so stepping through echoes scrolls the source turn off the
top. The user wants the source turn **pinned** as a fixed header with the
horizontal rule fixed beneath it; only the echo list scrolls underneath.

## Current structure (verified in source)

- `GlossOverlay` (`src/ui/gloss_overlay.rs`): `container` (vertical Box) holds
  title/headers, then `gloss_scroll_overlay` (an `Overlay` wrapping `gloss_scrolled:
  ScrolledWindow` → `gloss_view: TextView`, plus `bar_drawing: DrawingArea` as a
  clipped overlay), then a footer. `gloss_view` holds the **entire** echoes
  document (source turn + echo list).
- `bar_drawing`'s draw_func paints: the selected-echo accent bar (from
  `bar_ranges`), line numbers, AND the source/echo **rule** (from
  `echo_lines.first()`), all by mapping buffer lines to window-y at paint time.
- `render_echoes` (`src/input/actions/echoes.rs:538`) builds the doc as
  `echo_overlay_source` (the `<speaker>`/`<verse>` source turn, from
  `build_source_header`) followed by one `<gloss>[…]` line per echo, then calls
  `show_echoes(&doc, …)`.
- `show_echoes` (`gloss_overlay.rs:364`) calls `populate_gloss_buffer_ex(&gloss_view,
  doc, …)`, which parses `<speaker>`/`<verse>`/`<gloss>` and returns `bar_ranges`,
  line numbers, and `echo_lines` (buffer line of each echo's quote).
- Show methods revealing the scroll overlay: `show_gloss_with_color` (355),
  `show_echoes` (398), `show_synopsis` (471). Methods hiding it: `show` (315),
  `show_loading_message` (493), constructor default (238). `hide` (520) hides the
  whole `container`.

## Design (two-view split)

Split the echoes presentation into a **fixed header** (source turn) + a **fixed
rule** + the **scrolling echo list** (the existing `gloss_view`). Other overlay
modes (gloss, synopsis, loading, diff) are unchanged.

### New widgets (created in `GlossOverlay::new`, hidden by default)

Add to the struct and construct in `new`:

- `echo_header_view: gtk4::TextView` — non-editable, non-focusable, word-wrap,
  NOT inside a ScrolledWindow (sized to its content, so it never scrolls). Holds
  the source-turn `<speaker>`/`<verse>` text.
- `echo_rule: gtk4::Separator` (horizontal) — the fixed rule between header and
  list, replacing the Cairo-drawn rule for the echoes path.

**Append order in `container`:** … existing title/headers …, then
`echo_header_view`, then `echo_rule`, then `gloss_scroll_overlay` (unchanged
position), then footer. The header and rule sit directly above the scrolling
region. All three echoes-only widgets default to `set_visible(false)`.

### `show_echoes` signature + body

Change the signature to take the source and echo docs separately (avoids
string-splitting):

```rust
pub fn show_echoes(
    &self,
    source_doc: &str,   // the <speaker>/<verse> source turn
    echo_doc: &str,     // only the <gloss> lines
    card_height: i32,
    root_color: Option<&str>,
    dim_color: Option<&str>,
    selected: usize,
)
```

Body:
1. Hide the diff/title labels as today; set the hint text.
2. Populate `echo_header_view` by calling the existing
   `populate_gloss_buffer_ex(&echo_header_view, source_doc, self.text_margins,
   bar_left, &[], None, dim_color)` and ignoring its returns. That function
   already builds the full tag table (speaker/verse + echo tags) and parses
   `<speaker>`/`<verse>`; a source-only doc (no `<gloss>`) renders the speaker/verse
   lines and returns empty `bar_ranges`/`echo_lines`. No new tag helper is needed —
   both views reuse the same function.
3. Show `echo_header_view` and `echo_rule`.
4. Populate `gloss_view` with `echo_doc` via `populate_gloss_buffer_ex`. Because
   the buffer now contains **only** echoes, `echo_lines[i]` and `bar_ranges` are
   indexed from the echo list's first line (no source lines to offset past).
5. Set `bar_color`/`bar_x`, store `bar_ranges`/`line_numbers`/`echo_lines`, defer
   `bar_drawing.queue_draw()` to idle (keep the existing post-layout deferral).
6. Show the scroll overlay; reset its vadjustment to 0.

### `render_echoes` change (`echoes.rs`)

Build the echo-only doc separately from the source doc and pass both:

```rust
let source_doc = s.echo_overlay_source.clone();
let mut echo_doc = String::new();
for link in &s.echo_overlay_links { echo_doc.push_str(&format!("<gloss>[…]</gloss>\n", …)); }
…
s.gloss_overlay.show_echoes(&source_doc, &echo_doc, h, Some(&root), Some(&dim), s.echo_overlay_index);
```

(`echo_overlay_source` already holds exactly the source-turn doc; only the
`<gloss>` accumulation moves into its own string.)

### Remove the Cairo rule

Delete the source/echo rule block in `bar_drawing`'s draw_func (the
`echo_lines.first()` … `rule_y` … `cr.move_to/line_to/stroke` section). The rule
is now the `echo_rule: Separator` widget — always correctly positioned, no
geometry math, no scroll/layout race. The accent bar and line-number drawing in
the draw_func stay (they operate on `gloss_view`, now echo-only).

### Hide the header/rule in non-echo modes

In `show` (315), `show_gloss_with_color` (325), `show_synopsis` (447), and
`show_loading_message` (483): add `self.echo_header_view.set_visible(false);
self.echo_rule.set_visible(false);` so the pinned header only appears in the
echoes view. (`hide` hides the whole container, so no change needed there.)

### Scrolling / navigation adjustments (`echoes.rs`, `gloss_overlay.rs`)

- `scroll_echo_into_view(echo_index)` now scrolls `gloss_scrolled` to bring
  `echo_lines[echo_index]` into view within the echo-only buffer. The existing
  deferred-idle geometry computation stays, but indices are echo-relative.
- `select_first_echo` (`gg`): with a pinned header, the source turn is always
  visible, so `gg` simply scrolls the echo list to its top (select index 0,
  `scroll_gloss_to_top` on the echo view). Update its comment/behavior to "scroll
  echo list to first echo" (the header no longer needs revealing).
- `select_last_echo` (`G`): unchanged in intent (select last, scroll into view).

## Out of scope

- Gloss/synopsis/diff overlay modes (untouched).
- The accent-bar logic itself (still overlay-drawn on `gloss_view`; only its
  buffer is now echo-only, which simplifies indices).
- `a`/`Tab`/`n`/`p` playback behavior (unchanged).

## Testing

- Manual (user runs `cargo run`): open the echoes overlay; step with `n`/`p` and
  scroll — the source turn + rule stay fixed at the top; only the echo list moves
  beneath the rule; the accent bar tracks the selected echo within the scrolling
  region; `gg` shows the first echo at the top of the list (header still pinned);
  `G` reveals the last echo.
- `cargo build` + `cargo clippy` clean; `cargo test` shows only the 2 known
  pre-existing `block_atom_tests` failures.

## Open items for the plan

- Header view height: it should size to its content (the source turn is short).
  Confirm it does not get a fixed height request that would clip multi-line turns;
  let it size naturally within the vertical Box. (The header may set a top margin
  to match the existing `gloss_view` top margin of 24px.)
