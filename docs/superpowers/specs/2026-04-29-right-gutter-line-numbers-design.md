# Right Gutter Line Numbers for Plays and Verse

**Date:** 2026-04-29
**Status:** Approved

## Goal

Display verse line numbers in a right-side gutter for plays and poems, matching the convention used in scholarly editions like the Arden Shakespeare (Kindle screenshot reference). Numbers appear every 5th line, reset per scene, and are always visible for qualifying work types.

## Data Source

`line_mapping.line_in_div` in `lit.db` stores the verse line number within each scene (plays) or division (poems). This field is already loaded into every `Line` struct via `Line::line_in_div`. No database changes needed.

Speaker names and stage directions are not separate rows in the database — they live in the `speaker` column of verse lines. In text_file mode, they appear as separate buffer lines that map to `None` in `buffer_to_work`, so they naturally produce blank gutter cells.

## Display Rules

- Show the number only when `line_in_div % 5 == 0` (i.e., lines 5, 10, 15, 20...)
- Active when `!is_prose_work(work_type)` — covers `"play"` and `"poem"` types
- Numbering resets per scene — `line_in_div` already handles this
- Always visible for qualifying works — no toggle keybind
- Speaker name lines, stage directions, and blank lines show nothing (blank gutter cell)

## Implementation

### New function: `setup_line_number_gutter()` in `gutter.rs`

A new public function following the same pattern as `setup_timestamp_gutter()`:

- Creates a `GutterRendererText` on `TextWindowType::Right`
- `xalign = 1.0` (right-aligned within the gutter column)
- Width: fixed, enough for 3 digits plus padding (~36px)
- `query_data` callback:
  1. Get buffer line index from the callback's `line` parameter
  2. Look up `line_in_div` for that buffer line (via a pre-built `Vec<Option<i64>>`)
  3. If `Some(n)` and `n % 5 == 0`, render `n` with Pango markup (smaller size, `dim_fg` color)
  4. Otherwise, set empty string

### Data vector: `line_numbers: Rc<RefCell<Vec<Option<i64>>>>`

Built at work load time (not lazily). For each buffer line index, stores `Some(line_in_div)` if the line maps to a work line, or `None` otherwise.

Construction follows the same `buffer_to_work` pattern used for `has_timestamp`:
- **Text file mode:** iterate `buffer_to_work`, map each `Some(work_idx)` to the corresponding `Line::line_in_div`
- **Direct mode (no text file):** each buffer line `i` maps to `lines[i].line_in_div`

### Styling

- Font size: ~80% of body text, via Pango `size` attribute
- Color: `theme.dim_fg` (the dimmed text foreground used for non-current lines), via Pango `foreground` attribute
- Right-aligned within the gutter column

### Lifecycle in `app.rs`

**New AppState field:**
- `line_number_renderer: Option<sourceview5::GutterRendererText>` — stores the right gutter renderer
- `line_numbers: Rc<RefCell<Vec<Option<i64>>>>` — per-buffer-line data

**Setup (in `display_work_at_with_prepared()`):**
After the buffer is populated and `line_map` is built, if the work is not prose:
1. Build the `line_numbers` vector
2. Call `setup_line_number_gutter()` with the view, data vector, and theme color
3. Store the renderer in `state.line_number_renderer`

**Teardown (on work switch):**
Before loading a new work, if `line_number_renderer.is_some()`, remove it from the right gutter and set to `None`. Same pattern as the existing left gutter teardown.

**No rebuild needed on monocle/tiled transitions** — the right gutter width is fixed (not derived from margins), so layout changes don't affect it.

### Interaction with existing layout

- The right gutter lives inside the `sourceview5::View`'s right text window, so it doesn't affect `content_hbox` layout or margins
- `EXTRA_RIGHT_MARGIN` (28px) provides visual separation between text and the gutter
- The gutter does not interact with the left gutter, sign column toggle, or chunk renderer

## Files Changed

- `src/gutter.rs` — add `setup_line_number_gutter()` and `remove_line_number_renderer()` functions
- `src/app.rs` — add `line_number_renderer` and `line_numbers` fields to `AppState`, build data vector and call setup during work display, teardown on work switch

## Out of Scope

- Configurable interval (hardcoded to 5)
- Toggle keybind for line numbers
- Line numbers for prose works
- Through-line-numbering (TLN) — the `tln` column exists in the schema but is not populated
