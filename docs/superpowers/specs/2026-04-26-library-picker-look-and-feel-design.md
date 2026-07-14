# Library Picker Look-and-Feel Refresh

Date: 2026-04-26
Component: `src/ui/library_picker.rs`, `src/theme.rs`

## Problem

The Ctrl+P library picker (see screenshot in branch context) feels visually
unfinished:

- The 600x400 fixed-size dialog looks lost in a large window with vast empty
  surrounding canvas.
- No header — the dialog jumps straight from the search field to the list.
- Tight padding; rows feel cramped against the edges.
- Plain rows, no visual rhythm.
- Selection highlight uses the theme `cursor_bg` (coral) which clashes with
  the warm cream picker background.
- Right-aligned counts are thin grey text with no clear column alignment.
- No drop shadow — the picker does not feel elevated above the page.
- The last row is half-cut by the fixed 400px height.

## Goals

- Make the picker feel intentional and finished — "editorial" personality
  matching the bookish nature of the app.
- Establish a clear vertical hierarchy: header / search / list / footer.
- Improve responsive sizing on large windows without being unbounded.
- Replace the clashing selection highlight with a harmonized variant.
- No change to behavior, filter logic, or two-level navigation.

## Non-Goals

- New filter/match features.
- New keybindings.
- Theming changes affecting any other surface.

## Shared CSS class

`bookmark_picker` and `media_picker` both add `add_css_class("library-picker")`
to their picker box, so the new container/entry/row/scrolledwindow/selection
rules apply to them too. This is intentional in this refresh: all three
pickers share an editorial look. The header/footer rules only apply where the
matching widgets exist (LibraryPicker only). The responsive sizing hook in
`attach()` is also LibraryPicker-only — bookmark/media keep their fixed
600x400.

## Layout

The picker box is a vertical stack of four regions:

1. **Header row** — small-caps title on the left, count crumb on the right,
   hairline divider beneath.
   - Authors level: title `LIBRARY — AUTHORS`, crumb `21 authors`.
   - Works level: title `LIBRARY — <AUTHOR NAME>`, crumb `49 works`.
2. **Search entry** — slightly inset margin, focus ring on focus.
3. **Scrolled list** — vertical-expanding region, fills remaining height.
4. **Footer hint row** — small caps, mid-dot separated.
   - Authors level: `↑↓ MOVE  ·  ↵ OPEN  ·  ESC CLOSE`.
   - Works level: `↑↓ MOVE  ·  ↵ OPEN  ·  BACKSPACE BACK  ·  ESC CLOSE`.

### Sizing

Responsive within bounds:

- Width: `min(640, 0.6 * window_width)`, floor `360`.
- Height: `min(560, 0.7 * window_height)`, floor `280`.

Implementation: install a `size-allocate` (or equivalent GTK4 width/height
notification) handler on the parent overlay or window that updates
`picker_box.set_size_request(...)` whenever the window resizes. The picker
remains centered via `halign: Center, valign: Center`.

## Visual Style (CSS in `theme.rs`)

Replace the existing `.library-picker*` block (currently lines 360-364) with:

### Picker box

- Background: theme `text_bg` (cream).
- Border: hairline `1px solid {dim}`.
- Box-shadow: `0 18px 48px rgba(0, 0, 0, 0.22), 0 2px 6px rgba(0, 0, 0, 0.08)`.
- Padding: `0` on the box itself; each region handles its own padding so
  divider lines span edge-to-edge.
- Border-radius: `12px`.

### Header

New CSS classes: `.library-picker-header`, `.library-picker-title`,
`.library-picker-crumb`.

- `.library-picker-header`: padding `14px 22px 10px`; bottom border
  `1px solid {header_border}`, where `header_border` is
  `blend_colors(&theme.dim_fg, &theme.text_bg, 0.5)` for a subtler hairline.
- `.library-picker-title`: font-size `13px`, weight `600`, letter-spacing
  `2px`, text-transform `uppercase`, color `{dim}`.
- `.library-picker-crumb`: font-size `12px`, color `{dim}`.

### Search entry

- Margin: `12px 18px 8px`.
- Border: `1px solid {dim}` faint, radius `8px`, background `#ffffff` or
  theme `bg`-tinted lighter (concretely: keep `bg`, since cream over cream
  with a border still reads).
- Focus ring: `box-shadow: 0 0 0 3px {focus_ring}`, where `focus_ring` is a
  new variable in the `theme.rs` `format!` call computed as
  `blend_colors(&theme.cursor_bg, &theme.text_bg, 0.4)`. GTK CSS does not
  parse `rgba(<named-color>, alpha)`, so we use a solid blended hex value
  in place of an alpha tint.
- Font: inherits.

### List

- Outer padding: `4px 8px 10px`.
- Row padding: `8px 14px`, radius `6px`, gap `12px` between name and count.
- **Selected row**: background uses tinted `cursor_bg`. New variable
  `picker_selection_bg = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.5)`.
  Text color stays `cursor_fg`.
- Count column: `font-variant-numeric: tabular-nums`, color `{dim}`,
  right-aligned, `min-width: 32px`.

### Footer

New class: `.library-picker-footer`.

- Top border: `1px solid {header_border}`.
- Padding: `8px 22px 12px`.
- Font-size: `11px`, color `{dim}`, letter-spacing `1.5px`,
  text-transform `uppercase`.
- Items separated by ` · ` mid-dot in the markup.

### Scrim

Unchanged: `background-color: rgba(0, 0, 0, 0.3)`.

## Code Structure (`src/ui/library_picker.rs`)

### New struct fields

```rust
pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    header_box: GtkBox,
    header_title: Label,
    header_crumb: Label,
    search_entry: Entry,
    list_box: ListBox,
    footer_box: GtkBox, // children rebuilt on level change
    scrim: GtkBox,
    groups: Vec<AuthorGroup>,
    level: PickerLevel,
}
```

### New private helpers

- `build_header() -> (GtkBox, Label, Label)` — constructs the header box and
  returns title/crumb labels.
- `build_footer_for(level: &PickerLevel) -> GtkBox` — constructs a fresh
  footer for the given level. Called from `update_footer()`.
- `update_header(&self)` — updates `header_title` and `header_crumb` text
  from `self.level` and `self.groups`.
- `update_footer(&self)` — replaces `footer_box`'s children with hints
  appropriate to `self.level`.

### Header text computation

```text
match level {
    Authors => ("LIBRARY — AUTHORS", format!("{} authors", groups.len()))
    Works(author) => (
        format!("LIBRARY — {}", author.to_uppercase()),
        format!("{} works", group_for(author).works.len())
    )
}
```

When the filter at the Authors level matches both authors and works
across authors (the existing cross-author search UX), the title stays
`LIBRARY — AUTHORS` and the crumb stays `<n> authors` (total in
`self.groups`, not the filtered row count).

At the Works level, the crumb shows the total number of works for the
selected author and does not change when the filter narrows the visible
rows. This keeps the header stable as the user types.

### Hooked into existing API

- `populate_list()` calls `update_header()` at start (header reflects
  current level/group, not the filtered count).
- `enter_author()` and `go_back_to_authors()` are followed by
  `refresh_after_level_change()`, which now also calls `update_footer()` and
  `update_header()`.
- `show_finish()` calls both as well.

### Sizing

In `attach()`, install a resize handler on the overlay (or its toplevel
window via `picker_box.connect_realize` -> `root().connect_default_height_notify`,
etc.) that updates the picker_box size request. Keep the floor values
(360x280) on initial construction.

### Public API

Unchanged. No callers in `src/app.rs` or `src/input/` need modification.

## Testing

- Existing tests (`test_group_works_*`, `test_subsequence_match_*`) continue
  to pass — the changes are purely UI plumbing.
- No new automated tests; visual change verified manually:
  1. Launch app, press Ctrl+P.
  2. Confirm header reads `LIBRARY — AUTHORS` with `<n> authors` crumb.
  3. Confirm footer reads `↑↓ MOVE  ·  ↵ OPEN  ·  ESC CLOSE`.
  4. Confirm selection highlight is muted (tinted), not the full cursor red.
  5. Type to filter — confirm focus ring on entry is subtle.
  6. Drill into an author. Confirm header switches to
     `LIBRARY — SHAKESPEARE` (uppercase) with `<n> works` crumb and footer
     adds `BACKSPACE BACK`.
  7. Resize window from small to large; picker grows up to 640x560 then
     stops, stays centered.
  8. Esc closes cleanly.

## Out-of-scope follow-ups

- Apply the same editorial style to the other pickers (`bookmark_picker`,
  `media_picker`, `concordance_picker`). Track separately if desired.
- Add per-platform shadows that match dark vs light themes more carefully.
