# Visual-mode highlight for gloss / synopsis / journal editors — design

_2026-06-30. linux-lit._

## Goal

In the in-place vim editor for the **gloss**, **synopsis**, and **journal Q&A**
overlays, let the user visually select characters (vim Visual mode) and press one
key to **highlight** that span. The highlight persists and shows a theme-colored
marker-pen background when the gloss/synopsis/journal is later read. Pressing the
same key on an already-highlighted span removes it (toggle).

## Decisions (locked)

- **Storage:** inline markup tag `<hi>…</hi>` wrapping the span in the RAW stored
  text. Matches the existing gloss grammar (`<speaker>`, `<verse>`, `<gloss>`,
  `<p>`, `/IPA/`). Travels with the text; round-trips through the editor; no
  offset side-table.
- **Editor display:** raw — the editor shows the literal `<hi>` tag characters
  (like it already shows `<gloss>`/`<verse>`). No live colored preview. Preserves
  the "engine edits raw text 1:1" invariant.
- **Read-mode style:** background color (marker pen), via a `TextTag` background.
- **Color:** single, theme-derived. Reuse the theme's `cursor_line_bg` (the
  current-line highlight) — already tuned for readable contrast over `text_bg`,
  adapts to light/dark, no external theme-JSON change needed. Same color on all
  three surfaces. Tag carries NO color attribute: just `<hi>…</hi>`.
- **Remove:** the same key toggles. If the selection overlaps/lies inside an
  existing `<hi>`, remove that tag pair; else wrap. (Rule detailed below.)
- **Key:** `H` in Visual mode (free; mnemonic Highlight).
- **Scope:** all three surfaces. Gloss & synopsis already render through the tag
  parser; the journal gets a NARROW `<hi>`-only render pass (not the full
  grammar).

## Architecture

### 1. Pure vim engine (`src/input/vim/`)

The engine owns the raw text as a `String`, so the highlight edit goes through its
existing `snapshot()` undo + dirty tracking — same as a normal edit. No host-side
buffer mutation (that would desync undo/dirty).

- **`EditorAction::ToggleHighlight`** — new variant in `mod.rs`. Returned by the
  engine after the key; host handlers act on it (re-render + persist).
- **`H` in `handle_visual()`** (engine.rs). Taken visual keys:
  `h l k j w b e 0 ^ $ G g f t F T v V d x c s y > < "`. `H` is free. On press:
  1. compute the selection `Range` (existing `visual_range`);
  2. `snapshot()` (one undo step);
  3. **toggle** `<hi>` over the range (below);
  4. return to Normal mode, clear `visual_anchor` (like `d`/`y`);
  5. return `Outcome { buffer_changed: true, action: ToggleHighlight, … }`.

- **Toggle rule** (`toggle_highlight(range)` on the engine, pure string op):
  - If the selection **overlaps or lies inside** an existing `<hi>…</hi>` run →
    REMOVE that run's tag pair (un-highlight the whole existing span). Partial
    overlap removes the whole existing span — no nesting/overlap is ever produced.
  - Else → WRAP the selected substring in a fresh `<hi>…</hi>`.
  - After either, **coalesce** adjacent/nested `<hi>` runs so the raw text never
    accumulates `<hi><hi>` or `</hi><hi>`.
  - **Tag-boundary safety:** a wrap whose endpoint falls INSIDE another tag's
    `<…>` delimiter is clamped to the nearest safe boundary, so a highlight can
    never split another tag (e.g. selecting across a `<verse>` edge wraps only the
    visible text).

- **Tests (pure, in the `vim/` unit suite):** wrap a span; toggle it off
  (identical re-select); toggle off via partial overlap; coalesce adjacent;
  refuse to split a `<verse>` tag; undo restores pre-highlight text; round-trip
  (`buffer()` after wrap re-parses to the same highlighted span).

### 2. Host handlers (`src/input/keymap.rs`)

`handle_journal_edit_key` and `handle_gloss_edit_key` already match on
`EditorAction`. Add a `ToggleHighlight` arm:

- **Journal:** call `journal::vim_toggle_highlight(state)` — the engine already
  mutated its buffer, so the overlay just re-mirrors (the raw `<hi>` tags now
  show in the editor) and marks the buffer dirty. Persistence happens on the
  normal `:w` (the saved Q&A text now contains `<hi>`).
- **Gloss/synopsis:** same, routed by `is_showing_synopsis()` to
  `gloss::vim_toggle_highlight` / `synopsis::vim_toggle_highlight` (mirrors how
  Save/Cancel/Rewrite already branch).

No separate DB write: the highlight is part of the raw text, persisted by the
existing `:w` path (`update_gloss` / `save_synopsis` / `update_ln_page`). The
editor edits raw text, so `ToggleHighlight` only needs to re-mirror + keep dirty.

### 3. Read-mode render

**Theme color:** a helper reads `theme.cursor_line_bg` where the overlays already
pull `cursor_bg`/accent (the `vim_cursor_colors` threading point is the model).
Applied as a named `gloss-hi` `TextTag` `background`. Priority below the font tag
so family/size still apply but the background shows.

**Gloss & synopsis** (`gloss_render.rs` / `gloss_block.rs`):
- Add `<hi>` to `parse_gloss_tags` as a new inline element.
- In `populate_verse_buffer`, apply the `gloss-hi` background `TextTag` over each
  `<hi>` span's char range — same mechanism as `gloss-speaker`/`gloss-pron`.
- The synopsis path honors `<hi>` the same way it already handles `<p>`.

**Journal Q&A** (`journal_overlay.rs` `render_page`, currently `set_text(&body)`):
- New narrow `<hi>`-only pass: scan the body for `<hi>…</hi>`, build the visible
  text with the tags stripped, record each highlight's `(start_char, end_char)`,
  `set_text`, then apply the `gloss-hi` background over those ranges — the pattern
  `synopsis_label_ranges` already uses for bold. Nothing else in the journal gains
  the gloss grammar; only `<hi>`.

**Re-assertion:** `gloss-hi` is re-applied wherever `apply_font` / cached-audio
coloring is re-asserted, so a page turn or `!`/`|` font change can't drop it (same
discipline as `synopsis_label_ranges` and the audio tints).

**Tests:** render-pass unit tests that a `<hi>foo</hi>` body yields visible text
`foo` + one recorded highlight range, and that the editor still shows the raw
`<hi>` tags. Pixel-level background is verified e2e via the headless harness.

### 4. Keybind legends

Add `H  highlight (toggle)` to the "Vim edit mode (after e)" group (next to
`y p P / v V`) in all three legends — `gloss_keybinds_overlay.rs`,
`synopsis_keybinds_overlay.rs`, `journal_keybinds_overlay.rs` — per the
mandatory legend-sync rule. (These overlay legends are hand-maintained mirrors;
the reader-card Ctrl+/ overlay is NOT involved — these binds are handled in the
overlay vim handlers, not `keymap_config.rs`.)

## Out of scope (YAGNI)

- Multiple highlight colors / a color picker (single theme color only).
- Live colored preview inside the editor (raw tags only).
- Highlighting outside the three editors (no reader-card highlighting).
- A separate remove key (the same key toggles).
- An external `themes-unified.json` schema change (reuse `cursor_line_bg`).

## Risks

- **Tag-boundary splitting** is the main correctness risk — covered by the
  clamp rule + the "refuse to split `<verse>`" engine test.
- **Re-assertion drift** (highlight dropped on page turn/font change) — covered by
  applying `gloss-hi` at every re-assertion point, mirroring existing tints.
- **Journal pagination** splits the Q&A into blocks; a `<hi>` must not straddle a
  page boundary in a way that loses it. The journal pass records ranges against
  the rendered page body, so each page re-derives its own highlight ranges (same
  as the per-page bar/label re-derivation).
