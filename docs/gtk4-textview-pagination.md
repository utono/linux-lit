# GTK4 TextView Pagination for E-Reader Mode

## Problem

linux-lit uses a GTK4 `sourceview5::View` (subclass of `GtkTextView`) inside a `ScrolledWindow` for e-reader-style paginated display. Page turns snap the scroll position to show a specific line at the top of the viewport. The challenge: ensuring no lines are clipped at the top or bottom of each "page."

## Key Discovery: `iter_location` vs `line_yrange`

GTK4's `GtkTextView` has two methods for getting a line's position:

- **`iter_location(&iter)`** — Returns the bounding box of the character's *glyph area* only. The `y` coordinate is the top of the text glyphs. `pixels_above_lines` spacing sits above this y, and `pixels_below_lines` sits below `y + height`. Neither is included.

- **`line_yrange(&iter)`** — Returns `(y, height)` for the *full visual extent* of the line, including `pixels_above_lines` and `pixels_below_lines`. This is the authoritative measurement of the complete line area.

```
Layout of a single buffer line:

  line_yrange.y ──►  ┌─────────────────────────┐
                     │  pixels_above_lines      │
  iter_location.y ─► ├─────────────────────────┤
                     │  text glyphs (ascenders  │
                     │  to descenders)          │
                     ├─────────────────────────┤
                     │  pixels_below_lines      │
  line_yrange.y+h ─► └─────────────────────────┘
```

## Failed Approaches

Several approaches were tried before discovering `line_yrange`:

- **Overlay bars**: Opaque `gtk4::Box` widgets placed via `gtk4::Overlay` at the top/bottom of the scrolled window to mask clipped content. This fights GTK's scroll system — the text view doesn't know about the overlay, so `visible_rect()` and scroll coordinates don't account for it.

- **Manual pixel offsets**: Subtracting fixed amounts (16px, 24px, etc.) from `iter_location.y()` to create top padding. Fragile because `iter_location` excludes line spacing, and the offset varies with font size.

- **`hide_clipped_lines`**: Applying a text tag with `foreground = background_color` to lines partially behind overlay bars. Performance-heavy (ran on every cursor movement) and unreliable because `visible_rect()` may not update synchronously after `adj.set_value()`.

- **CSS padding on ScrolledWindow**: `padding-top`/`padding-bottom` on `.text-card` — GTK4 `ScrolledWindow` ignores CSS padding for scrollable content.

- **Card spacer widgets**: `gtk4::Box` spacers inside a vertical box wrapper with the text-card class. The spacers get the card background but don't reduce the scrolled window's viewport — text still scrolls to the edges.

## Correct Approach

Use `line_yrange()` everywhere instead of `iter_location()`:

### `scroll_value_for_line`

```rust
fn scroll_value_for_line(state: &AppState, line: usize) -> f64 {
    let (y, _h) = state.text_view.line_yrange(&iter);
    (y as f64).max(0.0).min(max)
}
```

Scrolling to `line_yrange.y` places the line's `pixels_above_lines` spacing at the viewport top edge, so the text starts below it — never clipped.

### `is_line_fully_visible`

```rust
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    let (y, h) = state.text_view.line_yrange(&iter);
    y >= scroll_y && y + h <= scroll_y + page_height
}
```

Checks that the entire line area (including below-spacing) fits in the viewport. A line partially extending past the bottom is correctly identified as not fully visible, triggering a page turn.

### `lines_per_page`

```rust
fn lines_per_page(state: &AppState) -> usize {
    let (start_y, _) = state.text_view.line_yrange(&start_iter);
    let limit = start_y + page_size;
    for i in start..line_count {
        let (y, h) = state.text_view.line_yrange(&iter);
        if y + h > limit { break; }
        count += 1;
    }
    count
}
```

Counts lines whose full extent fits within the viewport height.

## Coordinate System

The `vadjustment` coordinate system matches `line_yrange` / `iter_location` buffer coordinates:

- `adj.value()` = buffer y-coordinate of the viewport's top edge
- `adj.page_size()` = viewport height in pixels
- `adj.upper()` = total buffer height
- A line is visible when `line_yrange.y >= adj.value()` and `line_yrange.y + h <= adj.value() + page_size`

## Other Notes

- `set_top_margin()` / `set_bottom_margin()` on the text view add space at the very start/end of the document only — they don't affect mid-document page turns.
- Foliate (the Linux e-reader) uses WebKit with CSS multi-column layout, not GtkTextView — a fundamentally different approach.
- `scroll_to_iter` / `scroll_to_mark` with `within_margin` provide minimum-scroll behavior but don't snap to whole-line boundaries, making them unsuitable for e-reader pagination.
