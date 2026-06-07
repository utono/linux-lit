# Two-column translation overlay

## Problem

The current translation feature (`i` / `Action::ToggleTranslations`) renders
**interlinear**: each original verse line is inflated in-buffer with its modern
translation inserted directly beneath it (smaller, dimmer italic). It works, but
puts original and translation in one stacked column, which makes it hard to read
either side as continuous text.

We want a second, complementary way to read translations: a **two-column
layout** with the original on the left and the translation on the right,
grouped by speaker (this is dialogue — H8, plays). The interlinear view stays
as-is; the two-column view is a separate, additive feature.

## Decisions (from brainstorming)

- **Layout C — speaker-grouped blocks.** The speaker label spans the full width
  as a header; that speaker's whole speech appears as a paired block (original
  left, translation right). New speaker = new paired block.
- **Free-flow columns, equal halves.** Each column wraps independently (no
  per-line row locking); the card splits 50/50.
- **Scrolling overlay, not a paginated card.** This is NOT the e-reader reading
  card. It is a full-screen scrolling overlay (like the synopsis/gloss overlay).
  There are no pages, no page-turn boundaries, no clip invariant.
- **Lockstep scroll, one scrollbar.** Both columns live inside a single
  `ScrolledWindow`; `j`/`k` move both together. No second vadjustment to sync.
- **Opens at the cursor's speaker block.** On open, scroll so the speaker block
  containing the reader's current cursor line is at the top.
- **Current-scene scope.** The overlay shows the current scene's `(div1, div2)`
  speeches, not the whole work.
- **Separate keybind, coexists with interlinear.** Bound to `Alt+i`. The
  existing `Alt+i` → `ShowEchoes` is reassigned to `Alt+e`.

## Design

### 1. New module `src/ui/translation_overlay.rs`

A `TranslationOverlay` struct modeled on `GlossOverlay`
(`src/ui/gloss_overlay.rs`), holding:

- `overlay: gtk4::Overlay` — root of this overlay layer
- `scrim: gtk4::Box` — dimming backdrop
- `container: gtk4::Box` — the card body (`halign/valign = Center`, sized by
  `width_request`/`height_request` to the full `content_hbox` dimensions)
- `scrolled: gtk4::ScrolledWindow` — **one** scroll viewport, with
  `set_propagate_natural_height(false)` (same as `GlossOverlay`), so it defers
  to `height_request`
- `content_vbox: gtk4::Box` (Vertical) inside `scrolled` — the scrollable
  content: an alternating sequence of full-width speaker-header rows and paired
  two-column speech blocks
- per block: a `columns_hbox: gtk4::Box` (Horizontal) holding
  `orig_view: TextView` | `column_divider` | `trans_view: TextView`, each
  column at 50% width

The single shared `ScrolledWindow` is the key simplification: both columns sit
inside one scroller, so lockstep scrolling is automatic — there is no second
vadjustment to keep in sync. Speaker headers span the full card width because
they are siblings of the paired `columns_hbox` blocks inside `content_vbox`, not
inside either column.

**Attachment.** Mirror `GlossOverlay::attach` (`gloss_overlay.rs:485`): wrap the
previous overlay layer as `overlay.set_child(...)`, then `add_overlay(&scrim)`
and `add_overlay(&container)`, with `set_measure_overlay(..., false)` and
`set_clip_overlay(..., true)` for both. Insert this overlay into the same chain
as `gloss_overlay` (it wraps `gloss_overlay.overlay` or sits adjacent — exact
position chosen in the plan; it must be above the reading card and below the
window-level pickers). Store on `AppState.translation_overlay`.

### 2. Content model — speaker grouping

Content is built from `current_work.lines` (the DB-backed `Vec<Line>`), NOT from
the GTK reading buffer. `Line.speaker: Option<String>` (`db/models.rs:24`) is a
direct DB column — every dialogue line already carries its speaker, so no
backward text scan is needed.

On open, `build_translation_blocks(state)`:

1. Determine the current scene `(div1, div2)` from the cursor line:
   `work.lines[work_line_for_buffer(current_line)]`.
2. Walk the lines belonging to that scene, grouping **consecutive** lines that
   share the same `line.speaker`.
3. For each speaker group, emit:
   - a **full-width speaker-header row** (small-caps, styled like the
     screenshot's `CRANMER` / `KING` labels) into `content_vbox`
   - a paired `columns_hbox` block: the group's `line.text` values into
     `orig_view`, and their translations into `trans_view`
4. Lines with `speaker == None` (stage directions, scene headers, separators)
   render as a **full-width** italic row between blocks, with no translation
   column.

The builder returns the emitted blocks together with, for each block, the
**source work-line range** it covers. The open path (§3) uses these ranges to
find the block containing the cursor line for the initial scroll anchor.

**Translation lookup** (confirmed chain, from `show_translations`
`app.rs:3821`): for each line,
`state.work_line_for_buffer(buf_or_idx)` → `work.lines[idx]` → `&Line` →
`state.translations.get(&line.id)` (the `translations: HashMap<i64, String>`
keyed by `line_mapping.id`, loaded in `display_work` at `app.rs:2642`). A line
with no translation entry leaves the right side blank for that line; the columns
free-flow so this is fine.

**Styling.** Original column: reader italic (matches interlinear original).
Translation column: the dimmer, ~4pt-smaller italic already used for
interlinear translations (`translation_text_tag`, `font_size - 4`,
`theme.dim_fg`). Speaker headers: small-caps in the theme's speaker color.

### 3. Open / scroll / close

- **Open (`Alt+i`)** → new `Action::ShowTranslationOverlay`
  (`src/input/actions/mod.rs`), dispatched to `show_translation_overlay(state)`
  in `app.rs`. It reads `content_hbox.width()/height()` (same as
  `show_synopsis_overlay`, `app.rs:4828`), sizes `container`, builds the
  speaker-grouped content for the current scene, makes `scrim` + `container`
  visible, and sets `state.input_mode = InputMode::TranslationOverlay` (new
  `InputMode` variant, `app.rs:377`).

- **Scroll anchor** → after building, scroll `scrolled.vadjustment()` so the
  speaker block containing the current cursor line is at the top of the
  viewport. (Record each block's source line range during build; find the block
  whose range contains `work_line_for_buffer(current_line)`; scroll to that
  block widget's allocation top.)

- **Lockstep scroll (`j`/`k`)** → new `handle_translation_overlay_key` in
  `keymap.rs`, mirroring `handle_synopsis_overlay_key` (`keymap.rs:741`),
  routed when `input_mode == InputMode::TranslationOverlay` (the
  `dispatch_action` switch at `keymap.rs:98`). `j`/`k` step the single
  `scrolled` vadjustment (reuse the `scroll_gloss` row-step pattern,
  `gloss_overlay.rs:1192`) and never reach the reader buffer.

- **Close (`Escape`)** → handled inside `handle_translation_overlay_key`: hide
  `scrim` + `container`, set `input_mode = InputMode::Reader`. The reading card
  never changed layout, so nothing is restored. This is independent of
  `escape_reader_mode`'s interlinear toggle-off (`escape.rs:12`), which is left
  untouched.

### 4. Keybind swap

In **both** `src/input/keymap_config.rs` and the stowed
`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (per the
keymap.json-takes-precedence rule in CLAUDE.md):

- Move `ShowEchoes` from `Alt+i` to `Alt+e` (`Alt+e` is currently free; plain
  `e` is `SeekShortForward`, unaffected). Mnemonic: **e**choes → `e`.
- Bind `Alt+i` → new `Action::ShowTranslationOverlay`. Mnemonic: pairs with
  plain `i` (interlinear `ToggleTranslations`).

After editing the stow source, deploy with `cd ~/tty-dotfiles && stow
linux-lit` and restart linux-lit.

## Scope / non-goals

- **No pagination, no clip invariant.** Free-scrolling overlay — none of the
  page-turn / `column_split` / bottom-clip-no-clip machinery applies. No
  nav-fuzz run is required for this change.
- **No DB / `.txt` / `LineMap` change**, no `SNAPSHOT_VERSION` bump. Reuses the
  `translations` HashMap and `line.speaker` as they already exist.
- **Does not touch** the interlinear translation feature (`i`), the e-reader
  two-column play mode (`right_view`/`column_split`), or the deferred
  lockstep-scroll-of-the-reading-card work.
- **No per-line row locking** between columns (free-flow was the explicit
  choice); no whole-work scroll (current scene only).

## Verification

- `cargo build`, `cargo test --bins` (must stay green; add a unit test for
  `build_translation_blocks` speaker grouping if it's extractable as a pure
  helper over `Vec<Line>` + translations map).
- The acceptance criterion is visual ("renders correctly on screen"): per the
  CLAUDE.md headless rule, **ask the user** to launch H8 (a work with
  translations) headless, press `Alt+i`, and confirm:
  - two columns, original left / translation right, equal halves
  - speaker headers span full width above each paired block
  - the overlay opens scrolled to the cursor's speaker block
  - `j`/`k` scroll both columns in lockstep; `Escape` returns to the card
  - interlinear `i` still works independently
  - `Alt+e` now opens echoes; `Alt+i` no longer does
