# Design: multi-page overlay paragraph retention + glossed-cursor color

_2026-06-29 (US Central). Status: approved design, pre-implementation._

Two display features, one spec. Implement **B first** (small, self-contained),
then **A**. Both are user-confirmed designs from a brainstorming session.

- **A — multi-page label/echo retention.** Paginated synopsis/gloss overlays
  drop the non-cursor-stop paragraphs (synopsis *labels*, gloss *echo brackets*)
  that the single-page path keeps. Attach each to a block so it survives a page
  turn.
- **B — glossed-cursor 3rd color.** On the main reading card, a glossed line that
  is ALSO the cursor block should render in a NEW per-theme color, distinct from
  both the normal body text and the existing reddish gloss tint.

---

## Feature B — glossed-cursor 3rd color (implement first)

### Problem & current behavior

Source lines covered by a reader-gloss passage are tinted with the theme's dwl
`focuscolor` (reddish; rose-pine-dawn `#c4788a`) via the `reader-gloss-line`
TextTag (`foreground = theme.focus_color`, `src/app/mod.rs` ~914). The
`reader-gloss-line` tag is added after the dim/cursor tags so its foreground wins
over the dim foreground on a glossed line.

`repaint_reader_gloss_visible` (`src/input/highlight.rs` ~346) currently, for
each line in `state.reader_gloss_lines`:
- if it is the cursor line → **removes** the gloss tag (cursor-line highlight
  wins, line reads in normal fg);
- else → **applies** the gloss tint.

### Target: three states

For a buffer line:
1. **not glossed** → normal body fg (unchanged).
2. **glossed, not the cursor block** → reddish gloss tint (`focus_color`)
   (unchanged).
3. **glossed AND the cursor block** → a NEW distinct color
   (`theme.reader_gloss_cursor`), the "opposite of the reddish."

This flips the cursor-line case from *remove the tint* to *apply a different
tint*.

### Color source — new per-theme key

`themes-unified.json` is keyed by theme name at top level (no `.themes`
wrapper); each theme's `linux-lit` block today holds only `cursor_line_bg`. The
new color is a new OPTIONAL key in that block:

```json
"rose-pine-dawn": { "linux-lit": { "cursor_line_bg": "...", "reader_gloss_cursor": "#56949f" } }
```

- **rose-pine-dawn explicit value: `#56949f`** (rosé-pine "foam" — the dawn-variant
  teal; the true complement of the `#c4788a` rose-red focuscolor, and clearly
  distinct from the `#575279` slate body text). This is the theme being verified
  on screen.
- **Fallback when a theme omits the key:** `Theme::load` derives a sensible
  complement so all 36 themes get a reasonable color WITHOUT hand-authoring 36
  entries. Derivation rule (in `theme.rs`): rotate `focus_color`'s hue by ~180°
  at the same saturation/lightness (a small `hsl` round-trip helper), yielding a
  teal/green for a red focuscolor. If parsing fails, fall back to `text_fg`
  (never panics, never invisible).

`Theme` gains `pub reader_gloss_cursor: String`. The hard-coded default `Theme`
(`theme.rs` ~186) gets a teal default too.

### Wiring

- `theme.rs` `Theme::load`: read `linux-lit.reader_gloss_cursor`; else derive
  from `focus_color`; store in the new field. Add the `hsl` complement helper
  (pure, unit-testable).
- `src/app/mod.rs`: add a second TextTag `reader-gloss-cursor-line` with
  `foreground = theme.reader_gloss_cursor`, added right after `reader-gloss-line`
  (so it also outranks dim, and is available to apply on the cursor line). The
  cursor-line tag paints a paragraph BACKGROUND, so a foreground tag on the same
  line composes fine.
- `src/input/highlight.rs` `repaint_reader_gloss_visible`: on the cursor line,
  REMOVE `reader-gloss-line` and APPLY `reader-gloss-cursor-line`; on every other
  glossed line, REMOVE `reader-gloss-cursor-line` and APPLY `reader-gloss-line`
  (so moving the cursor off a line restores the reddish tint and clears the new
  color). Helpers
  `apply_reader_gloss_cursor_tag_to_line`/`remove_…` mirror the existing pair in
  `src/app/mod.rs`.
- Theme change refresh: `src/input/actions/settings.rs` already refreshes
  `reader-gloss-line`'s color on theme change — refresh the new tag's color there
  too (set foreground from the reloaded `theme.reader_gloss_cursor`).

### What does NOT change
The reddish tint for non-cursor glossed lines; the cursor-line background
highlight; the gloss overlay's OWN cached-TTS coloring (separate path); the set
of glossed lines (`reader_gloss_lines`). No change to navigation.

### Testing (B)
- Unit: the `hsl` complement helper rotates a known red to a known teal; a
  malformed color falls back to `text_fg`.
- Unit: `Theme::load` picks up an explicit `reader_gloss_cursor` when present and
  derives one when absent (use a small in-test JSON value).
- Visual (e2e, ask user): on rose-pine-dawn, open Bleak House "In Chancery",
  move the cursor onto the glossed first paragraph → it renders teal `#56949f`;
  move off → it renders reddish; a non-glossed cursor line renders normal.

---

## Feature A — multi-page label/echo retention (implement second)

### Scope (data-confirmed)

Of the three originally-suspected dropped paragraph kinds, only TWO are real:

- **Synopsis labels** (`is_label_paragraph`, e.g. "Shakespearean parallels:") —
  ~67 synopses have one. The DOMINANT case. A label HEADS the block below it →
  attach to the **following** block as a lead.
- **Gloss echo brackets** (`<gloss>` matching `split_echo`, e.g.
  `["quote" — Work x.y]`) — only ~4 glosses have them; each trails its source
  block → attach to the **preceding** block as a trail.
- **`<pron>` notes are OUT OF SCOPE** — `populate_verse_buffer`
  (`src/ui/gloss_render.rs` ~381) already drops them from display entirely
  (IPA is TTS-only), so they are not on screen single-page OR multi-page. Nothing
  to retain.

### Root cause

Both overlays paginate over `all_blocks`, which holds ONLY cursor-stop blocks
(`synopsis_blocks` / `gloss_blocks` / `gloss_block_markups` skip labels, echoes,
pron). The single-page render path renders the FULL original text/markup (so
labels/echoes appear); the multi-page path concatenates per-block
`display`/markup, so the non-block paragraphs vanish. They live in no block, so
no page can carry them.

### Approach A — attach as block-level lead/trail (chosen)

`GlossBlock` gains one field:

```rust
/// Non-cursor-stop paragraphs that ride with this block on a paginated page so
/// they are not dropped at a page boundary. Display-only; the block stays the
/// sole cursor stop. Synopsis: LeadLabel(s) rendered bold above the body. Gloss:
/// TrailEcho(s) rendered below the body via the echo render path. Empty in the
/// common case.
pub attached: Vec<Attachment>,
```

```rust
pub enum Attachment {
    /// Bold label paragraph that heads this block (synopsis).
    LeadLabel(String),
    /// Echo-bracket markup ("<gloss>[...]</gloss>") that trails this block (gloss).
    TrailEcho(String),
}
```

Block builders attach instead of dropping:
- `synopsis_blocks`: buffer label paragraph(s); attach buffered labels as
  `LeadLabel` to the NEXT emitted block. A trailing label after the last block
  attaches as a `LeadLabel` to the LAST block (so a final header is not lost) —
  it renders bold above that block's body, accepted as a minor placement quirk in
  the rare trailing-label case.
- `gloss_block_markups`: an echo `<gloss>` attaches its
  `<gloss>...</gloss>` markup as `TrailEcho` to the PRECEDING block's markup
  (an echo before any block, none observed, attaches to the first). `gloss_blocks`
  is unchanged in COUNT (echoes are still not cursor stops); only the markups
  carry the echo. The 1:1 count between `gloss_blocks()` and `gloss_block_markups()`
  is preserved (the echo rides inside an existing block's markup string, it does
  not add an entry).

### Pagination
`gloss_block_height` (`gloss_overlay.rs`) measures each block INCLUDING its
attachments (lead label text height; trail echo quote+citation line heights), so
page-fit stays honest and nothing clips. Block + attachments are one indivisible
unit — already true for gloss via `paginate_grouped` (a Source starts a unit);
for synopsis each block is its own unit and its labels measure inside it.

### Multi-page render arms (`gloss_overlay.rs`)
- `render_synopsis_page` (multi-page branch): for each block emit its
  `LeadLabel` line(s) then its `display`, joined by `\n\n`; track each label's
  char-offset range in the page text and set `synopsis_label_ranges` to those
  (instead of clearing to empty), then `apply_synopsis_label_bold`. The cursor
  stays on the block body (labels are not cursor stops).
- `render_gloss_page` (multi-page branch): the per-block markup already carries
  its `TrailEcho`, so the existing `markups[start..end].join("\n")` →
  `populate_gloss_buffer` renders the echo via the existing echo path with NO
  further change.

### What does NOT change
Cursor model (`cursor_full`, indices, `all_blocks` count), nav keys, single-page
paths, cursor-block bar ranges, the synopsis-recolor fix from `2497ac0`.
Attachments are display-only and never become cursor stops.

### Testing (A)
Pure unit tests in `gloss_block.rs`:
- `synopsis_blocks` attaches a label as `LeadLabel` to the following block;
  trailing-label fallback attaches to the last block; legacy untagged unchanged;
  block count + indices unchanged from today.
- `gloss_block_markups` attaches an echo to the preceding block; an all-echo
  gloss keeps one echo per source block; `len() == gloss_blocks().len()` still
  holds; the existing `block_markups_match_blocks_count_and_order` test still
  passes (echo no longer absent — assert it now rides in a block's markup).
- Existing block tests must stay green.
Visual (e2e, ask user): a MULTI-PAGE synopsis with a label paragraph shows the
bold label on its page; a MULTI-PAGE gloss with an echo shows the echo bracket.

---

## Sequencing & verification

1. Implement **B**, `cargo build` + `cargo test --bins` + clippy, commit, ask
   user to eyeball on rose-pine-dawn.
2. Implement **A**, same gates, commit, ask user to eyeball a multi-page
   synopsis/gloss.

Both features' visual acceptance needs the headless e2e (`scripts/e2e-env.sh`) or
a manual launch — the agent cannot launch on the live dwl seat. Logic is covered
by `cargo test --bins`; color/layout correctness is pixel-level.
