# Two-Column E-Reader Layout

**Date:** 2026-06-01
**Status:** Approved, ready for implementation plan

## Goal

Add an Arden-style two-column page layout to linux-lit's e-reader mode. Text
flows down the left column and continues at the top of the right column (book
flow), with original verse/prose line numbers in each column's outer margin.
`Alt+[` toggles between one-column and two-column layout. Scroll mode is
unaffected.

## Motivation

The target layout (the Arden Shakespeare Third Series page) renders a play as
two side-by-side columns: read the left column top-to-bottom, then continue at
the top of the right column. linux-lit currently renders one continuous
wrapping column in a single `sourceview5::View`, paginated by scrolling that
view. Foliate gets two-column flow for free via CSS `column-count: 2;
column-fill: auto` because it renders in a WebView; GTK's `TextView` has no
multi-column equivalent, so linux-lit must manufacture the column flow.

## Scope

**In scope:**
- True two-column flow (left fills, right continues) in **e-reader mode only**.
- Per-column line-number gutters on each column's **outer** edge.
- `Alt+[` toggle between one-column and two-column layout, persisted in config.

**Out of scope (deferred, largely independent CSS/theme work):**
- Dashed top/bottom rules bounding each column.
- Centered per-page title header.
- Tan/khaki theme, serif styling, blue gloss-link styling, small-caps speakers.

Scroll mode is never two-column.

## Approach

**Approach A — two TextViews, column-balancing pager, one shared buffer.**

Considered and rejected:
- **Custom Pango-drawing widget (B):** total fidelity but requires
  reimplementing highlight, cursor, selection, gloss hit-testing, scrolling,
  and every pagination edge case (descender guard, dangling-speaker trim) from
  scratch. Too costly; discards months of edge-case work.
- **WebView like foliate (C):** columns free, but abandons the entire Rust/GTK
  TextView renderer and MPV-sync-by-line. Effectively a different app.

Approach A preserves all existing pagination, sync, highlight, and gloss
machinery and reuses the `visible_range` kernel.

## Architecture

### Widget tree (e-reader, two-column mode)

```
content_hbox
 └ columns_hbox            ← NEW (holds 1 or 2 columns)
   ├ left_scrolled → left_view   (= today's text_view; shares state.buffer)
   ├ gutter spacer
   └ right_scrolled → right_view (NEW; shares state.buffer)
```

Each view carries a line-number `GutterRendererText` on its outer (right) edge.

### Buffer model

**One shared buffer, two views.** Both `left_view` and `right_view` use
`state.buffer`. All existing tags (highlight, dim, cursor, gloss) stay on the
single buffer unchanged — a tag shows in whichever column currently displays
that line.

GTK constraint: a `TextView` always renders the whole buffer from the top, so
"start rendering at line K" is achieved by scrolling the view to line K and
clipping above/below — the same bottom-clip technique already used today,
applied per column.

### Split-point algorithm (per page turn)

`page_top_line = L` keeps its current meaning: the first line of the page.
Reuses the existing `visible_range` + `trim_trailing_speakers` kernel, run
twice in two-column mode:

1. `page_top_line = L`.
2. `visible_range(L, col_height)` → left column ends at line `S-1`; trim
   dangling speakers (existing logic).
3. `split = S` → right column starts here.
4. `visible_range(S, col_height)` → right column ends at line `E`; trim.
5. Page = lines `[L .. E]`. **Next `page_top = E+1`.**
6. `left_view` scrolls to `L`, clips below `S-1`; `right_view` scrolls to `S`,
   clips below `E`.

In one-column mode, steps 3–4 are skipped; the page is `[L .. S-1]`, identical
to today's behavior.

Backward turn: pop `page_back_stack` (already stores prior `L` values). No new
math required.

### Alt+[ toggle

- New `Action::ToggleColumnLayout`.
- New persisted config field `column_count: 1 | 2`.
- Bound to **`Alt+bracketleft`** — verified free today (plain `bracketleft` =
  `JumpToPrevChapter`, unaffected by adding the Alt-modified variant).
- Per the keybind-override rule, the binding is added to **both**
  `src/input/keymap_config.rs` (compiled default) **and**
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (stow source),
  or the JSON silently overrides the compiled default.
- On toggle: flip `column_count`, show/hide `right_scrolled`, recompute the
  page from the current `page_top_line`, and keep `current_line` visible.
  No-op in scroll mode.

### Visual mode

Visual mode selection flows across the column boundary, mirroring selecting
across a page break in a book. The selection model needs **no new mechanism**;
it inherits two-column behavior from the shared buffer.

- **Selection model:** unchanged. `SelectionState` stays `anchor_line ..
  cursor_line` over absolute buffer indices. A selection spanning the break
  (anchor in the left column, cursor in the right) is the contiguous run
  `[start ..= end]` that `range()` already returns.
- **`j` / `k`:** unchanged. `move_selection_cursor(±1)` walks buffer indices;
  the bottom of the left column (`S-1`) → top of the right column (`S`) is
  just `+1`. No column-jump special case.
- **Highlight rendering:** the only behavioral change, and it is automatic.
  `apply_selection_highlight` tags `[start ..= end]` on the one shared buffer;
  because `left_view` shows `L .. S-1` and `right_view` shows `S .. E`, each
  tagged line renders in whichever column currently displays it. No per-column
  tagging code.
- **`ensure_visible` during selection:** `move_selection_cursor` calls
  `update_highlight_and_ensure_visible`, which must page-turn correctly when
  the cursor leaves the visible two-column page (cursor past `E` → next page).
  This reuses the `is_line_on_screen` / `is_line_fully_visible` "span both
  columns" change already listed below — no additional work specific to visual
  mode.

`gg` / `G` extension (`extend_to_start` / `extend_to_end`) is unchanged; they
set `cursor_line` to `0` / `line_count-1` and rely on the same page recompute.

## What stays the same

- One buffer; all tags (highlight, dim, cursor, gloss) unchanged.
- `visible_range` / `trim_trailing_speakers` reused as-is.
- MPV sync addresses absolute line index; gains only a "which column is line N
  in?" lookup to pick the view for scroll/highlight.
- Scroll mode: 100% unchanged.
- `gg` / `G`, `page_back_stack`, descender guard.

## What changes

- `set_page` / `snap_scroll_to_line`: scroll N views and compute the split.
- `update_bottom_clip`: clip both columns (left at `split`, right at page end).
- `is_line_on_screen` / `is_line_fully_visible`: span both columns.
- New helpers: `column_split(state)` (steps 1–5) and `which_column(state, line)`.
- A second `GutterRendererText` for `right_view`.
- Page-turn animation snapshot targets `columns_hbox` (capture currently targets
  `card_vbox`; extends cleanly).
- New `Action::ToggleColumnLayout`, `config.column_count`, and keymap entries in
  both files.

## Testing

- Existing headless pagination tests (`test-pagination`, `test-play-navigation`,
  `test-prose-navigation`) must pass unchanged in one-column mode.
- Add two-column coverage: forward/backward paging produces no gaps, repeats, or
  non-dialogue highlights; the right column starts exactly where the left ends;
  `next page_top = E+1`.
- Toggle test: `Alt+[` flips layout, keeps `current_line` visible, is a no-op in
  scroll mode, and persists across restart.
- MPV sync: highlight and page turns land in the correct column.
- Visual mode: a selection started in the left column and extended with `j`
  past `S-1` flows into the right column as one contiguous run; the highlight
  renders in both columns; `range()` yields `[anchor ..= cursor]`; extending
  past the visible page page-turns and keeps the cursor visible.

## Open risks

- Both views must share the buffer and stay scroll-synced; per-column clip math
  is the main new surface for bugs (mirrors the existing single-column
  bottom-clip edge cases, now doubled).
- Line-height measurement (`line_yrange`) is read per column at the same font
  metrics; column width affects wrapping, so `col_height` fit must be measured
  against the actual column width, not the full card width.
