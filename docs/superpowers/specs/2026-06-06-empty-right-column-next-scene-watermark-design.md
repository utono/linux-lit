# Empty right-column "Next: Act N, Scene M" watermark

## Summary

When a two-column spread's **right column is empty because the current scene
ended within the left column**, render a dim, vertically-centered label in the
empty right column reading **`Next: Act N, Scene M`** — the act/scene that opens
the next canonical spread. Hidden in every other case.

This is the case shown in the H8 1.3 → 1.4 screenshot: the scene ends on
`I am your Lordship's.` + `[They exit.]` in the left column, and the right
column is blank. The next spread opens at Act 1, Scene 4, so the right column
shows a dim `Next: Act 1, Scene 4`.

## Behavior

- **Trigger:** the right column is empty AND a next scene exists.
  Computed from the `ColumnSplit` already returned by `column_split`:
  `cs.page_end < cs.split && cs.split < line_count && cs.next_page_top < line_count`.
  This is exactly the `at_break && !is_final_section` scene-break branch
  (`viewport.rs:1170`, return at `viewport.rs:1204-1205`), where
  `cs.next_page_top == hi` (the scene-marker line that opens the next spread).
- **Label text:** `Next: ` + `scene_label(div1, div2)`, e.g.
  `Next: Act 1, Scene 4`, `Next: Prologue`, `Next: Act 2, Chorus`.
- **Position:** vertically and horizontally centered in the right column.
- **Styling:** dim foreground (theme `dim_fg`, the same 40%-blended color the
  `dim_tag` uses, so it tracks light/dark theme), slightly smaller, italic — a
  margin-note look.

## Scope — what does NOT get a label (hidden)

- **Right column has content** — `cs.page_end >= cs.split`.
- **Empty-left first-spread mirror** (Prologue/Induction placed in the right
  column with an empty *left* column, `viewport.rs:1183-1197`) — out of scope by
  decision; `split == 0`, the right column is non-empty, so the trigger above is
  already false.
- **End-of-work empty right** (`cs.split >= line_count`, so
  `cs.next_page_top == line_count`) — no next scene exists; trigger is false.
- **Prose works** — prose is single-column and never enters the two-column path.

## Deriving the next scene (authoritative metadata, never text inference)

Per the project's authoritative-boundary principle (CLAUDE.md → "Pagination &
Scene Boundaries"), the act/scene is read from the DB `(div1, div2)` columns,
never re-inferred from buffer text.

`column_split` already returns `cs.next_page_top` — the first buffer line of the
next spread (the scene marker `hi`). To get its act/scene:

1. Walk forward from `cs.next_page_top` to the first DB-mapped buffer line
   (`state.work_line_for_buffer(bl)` returns `Some(work_idx)`), exactly as
   `current_scene_divs` (`app.rs:4477-4505`) does — the marker/`=====` chrome
   lines are unmapped.
2. Read `lines[work_idx].div1` / `.div2`.
3. `scene_label(div1, div2)` (`app.rs:4638`) → the human label.

Factor this walk into a small helper (e.g. `divs_at_buffer_line(state, bl)`)
so it does not duplicate `current_scene_divs`'s body inline.

## Widget & placement

- A single `gtk4::Label`, `next_scene_watermark`, stored on `AppState`.
- Added once as an overlay child on `state.right_scrolled_overlay`
  (`app.rs:1051-1061`), alongside the existing `right_bottom_clip`.
  `halign = Center`, `valign = Center`, `set_visible(false)` initially.
- It is NOT buffer text (buffer text would be measured by `visible_range` and
  corrupt the right-column clip height) and NOT in the size-bearing widget chain
  — consistent with the project's "pickers: overlay, not chain link" rule and
  the existing toast labels (`chapter_toast`, `search_toast`).
- Dim styling via `set_markup` with a `<span foreground="…">` using the
  resolved theme `dim_fg`, set whenever the label text is updated (so it
  re-colors on theme change, which already re-runs the render path).

## Update point

One helper, `update_next_scene_watermark(state, &cs)`, called from the single
place where the `ColumnSplit` and the right-column widgets coexist: the
`if let Some(cs) = cs { … }` block in `snap_scroll_to_line`
(`scroll.rs:446-485`). `snap_scroll_to_line` runs on every page render, so the
label stays correct as the user pages around. When the trigger is false the
helper hides the label.

Also hide the watermark on the single-column path (prose, or
`snap_scroll_to_line` called with `cs == None`) so it never lingers from a prior
two-column spread.

## Files touched

- `src/app.rs` — add `next_scene_watermark: gtk4::Label` field to `AppState`;
  build it and `add_overlay` it onto `right_scrolled_overlay`; add the
  `divs_at_buffer_line` helper (or expose the existing walk).
- `src/input/scroll.rs` — `update_next_scene_watermark` helper + call site in
  the `Some(cs)` block of `snap_scroll_to_line`; hide on the `None` path.
- Reuse: `scene_label` (`app.rs:4638`), `work_line_for_buffer`
  (`app.rs:411`), theme `dim_fg` (`theme.rs:133`).

## Testing / verification

- `cargo build` and `cargo test --bins` (pure-logic suites stay green; this is a
  render-only change with no new pure-logic surface).
- The visible result is verified live by the user on the H8 1.3 spread
  (`Next: Act 1, Scene 4` centered in the empty right column), per the
  "render correctly on screen" criterion in CLAUDE.md → "When to ASK THE USER to
  run e2e".

## Non-goals

- No new keybind, config option, or persisted state.
- No change to pagination math, spread boundaries, or `column_split`'s return
  values — the watermark reads `cs` but never alters it.
- No label for the empty-left mirror or end-of-work cases.
