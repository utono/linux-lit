# GTK4 TextView Pagination for E-Reader Mode

## Problem

linux-lit uses a GTK4 `sourceview5::View` (subclass of `GtkTextView`) inside a `ScrolledWindow` for e-reader-style paginated display. Page turns snap the scroll position to show a specific line at the top of the viewport. The challenge: ensuring no lines are clipped at the top or bottom of each "page."

## Key API Methods

### `iter_location(&iter)` vs `line_yrange(&iter)`

- **`iter_location(&iter)`** — Returns the bounding box of the character's *glyph area* only. `y` is the top of the text glyphs. `pixels_above_lines` and `pixels_below_lines` are NOT included.

- **`line_yrange(&iter)`** — Returns `(y, height)`. Per GTK docs: "Gets the y coordinate of the top of the line containing `iter`, and the height of the line. The coordinate is a buffer coordinate; convert to window coordinates with `buffer_to_window_coords()`."

**Important caveat**: when `pixels_above_lines=0` and `pixels_below_lines=0` (which happens when the user's config sets `line_spacing: 0`), `line_yrange` returns identical values to `iter_location`. There is no inherent spacing to work with.

### `scroll_to_iter`

```
gtk_text_view_scroll_to_iter(
    text_view, iter,
    within_margin,  // [0.0, 0.5) fraction of screen as margin
    use_align,      // TRUE = use xalign/yalign positioning
    xalign,         // 0.0=left, 1.0=right
    yalign          // 0.0=top, 1.0=bottom
)
```

- `within_margin` shrinks the effective viewport from all four edges by this fraction. A value of 0.03 means 3% margin on each side (~25px on a typical screen).
- When `use_align=TRUE` and `yalign=0.0`, the iter is placed at the top of the effective viewport (after the margin).
- **Caveat**: may not work correctly before layout is computed. `scroll_to_mark` is more reliable but requires creating a mark.
- Returns TRUE if scrolling occurred.

### `scroll_to_mark`

Same parameters as `scroll_to_iter` but deferred until line heights are validated (idle handler). More reliable for initial positioning.

Sources:
- https://docs.gtk.org/gtk4/method.TextView.scroll_to_iter.html
- https://docs.gtk.org/gtk4/method.TextView.get_line_yrange.html
- https://docs.gtk.org/gtk4/method.TextView.get_iter_location.html

## Failed Approaches

Several approaches were tried before finding the working solution:

- **Manual `adj.set_value()` with `iter_location.y()`**: Clips text because `iter_location` returns glyph bounds without spacing. Subtracting fixed pixel offsets is fragile across font sizes.

- **Manual `adj.set_value()` with `line_yrange.y()`**: When `pixels_above/below_lines=0`, identical to `iter_location` — still clips.

- **Overlay bars**: Opaque `gtk4::Box` widgets placed via `gtk4::Overlay` at the top/bottom of the scrolled window to mask clipped content. The text view doesn't know about the overlay, so `visible_rect()` and scroll coordinates don't account for it.

- **`hide_clipped_lines`**: Applying a text tag with `foreground = background_color` to lines partially behind overlay bars. Performance-heavy and unreliable because `visible_rect()` may not update synchronously after `adj.set_value()`.

- **CSS padding on ScrolledWindow**: GTK4 `ScrolledWindow` ignores CSS padding for scrollable content.

- **Card spacer widgets**: `gtk4::Box` spacers inside a vertical box wrapper. The spacers don't reduce the scrolled window's viewport.

- **`scroll_to_iter` with `yalign=0.0` on the target line**: Places the iter's glyph rectangle at the viewport top, but ascenders of the top line extend above `iter_location.y()` and get clipped.

## Working Solution

### Page Turn Scroll Positioning

Use `scroll_to_iter` on the **end of a line before the target**, not on the target line itself:

```rust
fn set_page(state: &mut AppState, new_top: usize) {
    state.page_top_line = new_top;
    // Scroll to end of line (new_top - 2) so line (new_top - 1) is
    // the overlap line and new_top is the first content line.
    let scroll_line = new_top.saturating_sub(2);
    if let Some(iter) = state.buffer.iter_at_line(scroll_line as i32) {
        let mut end = iter;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        // within_margin=0.03 creates ~3% padding at top/bottom
        state.text_view.scroll_to_iter(&mut end, 0.03, true, 0.0, 0.0);
    }
}
```

Why this works: by scrolling to the END of a line 2 before the target, the viewport top is positioned at the bottom of that line's glyph area. The next line (overlap/padding) appears fully below, and the target content line appears below that. The `within_margin` adds additional breathing room.

### Visibility Check for Page Turn Trigger

```rust
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    let (y, h) = state.text_view.line_yrange(&iter);
    // Require one full line height below for bottom padding
    y >= scroll_y && y + h + h <= scroll_y + page_height
}
```

Requiring space for an additional line height below the last content line ensures the page turns before any line gets clipped at the bottom.

### Page Turn Trigger

```rust
fn needs_page_turn_down(state: &AppState, line: usize) -> bool {
    if !is_line_fully_visible(state, line) { return true; }
    let line_count = state.effective_line_count();
    if line + 1 >= line_count { return false; }
    !is_line_fully_visible(state, line + 1)
}
```

## Coordinate System

The `vadjustment` coordinate system matches `line_yrange` / `iter_location` buffer coordinates:

- `adj.value()` = buffer y-coordinate of the viewport's top edge
- `adj.page_size()` = viewport height in pixels
- `adj.upper()` = total buffer height

## Other Notes

- `set_top_margin()` / `set_bottom_margin()` add space at the very start/end of the document only — they don't affect mid-document page turns.
- Foliate (the Linux e-reader) uses WebKit with CSS multi-column layout, not GtkTextView.
- `scroll_to_mark` is preferred over `scroll_to_iter` when called before layout is validated (e.g., during `display_work`).
