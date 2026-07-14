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

### Color source — derive BOTH gloss colors with guaranteed contrast

A 36-theme audit (2026-06-29) showed the contrast problem is broader than the
new feature: the EXISTING off-cursor tint (raw `dwl.focuscolor`) is itself dim
against `text_bg` or near `text_fg` on **13 themes** (dayfox, melange-light,
everforest-light-{hard,medium,soft}, solarized-light, modus-operandi,
gruvbox-material-light-soft, kanagawa-lotus, tokyonight-day, several
everforest-dark, …), and the naive 180° complement has its OWN contrast failures
on a different set. Using raw `focuscolor` / raw complement does NOT guarantee a
legible, distinct color.

The codebase already solves exactly this for vocab words via `choose_vocab_fg`
(`theme.rs`): pick a color with enough hue distance from the body text, else
derive one by rotating hue and clamping S/L. We reuse that pattern for BOTH gloss
colors.

**Two derived colors, both guarded** (and both new OPTIONAL per-theme keys —
explicit value wins, derivation is the default):

- **`reader_gloss` — off-cursor tint.** Base hue = `focus_color`. Adjust so it
  (a) contrasts with `text_bg` (WCAG ratio ≥ 3.0) and (b) is distinct from
  `text_fg` (contrast ≥ ~1.4 OR hue distance ≥ 40°). If the raw focuscolor
  already satisfies both, keep it (so themes that look right today are unchanged
  — e.g. rose-pine-dawn, gruvbox-material). Else clamp lightness away from the bg
  and raise saturation at the same hue; if still indistinct, rotate hue like
  `choose_vocab_fg`'s last resort.
- **`reader_gloss_cursor` — on-cursor color.** Base hue = the 180° complement of
  the (derived) off-cursor tint. Run through the SAME guard against `text_bg`,
  `text_fg`, AND the derived off-cursor tint, so the three states (body /
  off-cursor / on-cursor) are mutually distinct and all legible.

`themes-unified.json` is keyed by theme name at top level (no `.themes` wrapper);
each theme's `linux-lit` block today holds only `cursor_line_bg`. Both colors may
be overridden there:

```json
"rose-pine-dawn": { "linux-lit": { "cursor_line_bg": "...", "reader_gloss_cursor": "#56949f" } }
```

- **rose-pine-dawn `reader_gloss_cursor` explicit value: `#56949f`** (rosé-pine
  "foam"). Its off-cursor tint (`focus_color #c4788a`) already passes the guard
  (contrast-vs-bg 3.0, distinct from body), so `reader_gloss` is NOT overridden
  there — the derivation keeps the current look.
- **No per-theme `reader_gloss`/`reader_gloss_cursor` entries are required** for
  the other 35 — the guarded derivation gives every theme a legible, distinct
  pair automatically. (This is the whole point: a new theme added later is safe.)

`Theme` gains `pub reader_gloss: String` AND `pub reader_gloss_cursor: String`.
The existing `focus_color` field stays (still used elsewhere if any), but the
reader-gloss TINT tag now uses `theme.reader_gloss` (the guarded value), not the
raw `focus_color`. The hard-coded default `Theme` gets both, derived from its
`#d4be98` focuscolor.

### Guard helper (new, in theme.rs, pure + unit-tested)

```
fn ensure_gloss_color(base_hex, bg_hex, avoid: &[&str]) -> String
```
Returns a color at `base_hex`'s hue that has WCAG contrast ≥ 3.0 vs `bg_hex` and
is distinct (hue distance ≥ 40° OR contrast ≥ 1.4) from every color in `avoid`.
Strategy: if `base_hex` already qualifies, return it; else adjust L (toward the
side of `bg` with more headroom) and raise S at the same hue and re-check; if it
still collides on hue with an `avoid` color, rotate hue +150° (the
`choose_vocab_fg` last resort) and clamp S/L. Reuses existing `hex_to_rgb`,
`rgb_to_hsl`, `hsl_to_rgb`, `rgb_to_hex`, `hue_distance`, and a small WCAG
`contrast_ratio(a,b)` (extract the luminance math already inside `contrast_on`).

### Wiring

- `theme.rs` `resolve_theme`: compute
  `reader_gloss = linux-lit.reader_gloss override OR ensure_gloss_color(focus_color, text_bg, &[text_fg])`,
  then
  `reader_gloss_cursor = override OR ensure_gloss_color(complement_hex(&reader_gloss), text_bg, &[text_fg, &reader_gloss])`.
  Store both. `default_theme` does the same from its constants.
- `src/app/mod.rs`: the existing `reader-gloss-line` tag uses
  `theme.reader_gloss` (was `theme.focus_color`). Add a second TextTag
  `reader-gloss-cursor-line` with `foreground = theme.reader_gloss_cursor`, right
  after it (outranks dim; applied on the cursor line). The cursor-line tag paints
  a paragraph BACKGROUND, so a foreground tag on the same line composes fine.
- `src/input/highlight.rs` `repaint_reader_gloss_visible`: on the cursor line,
  REMOVE `reader-gloss-line` and APPLY `reader-gloss-cursor-line`; on every other
  glossed line, REMOVE `reader-gloss-cursor-line` and APPLY `reader-gloss-line`.
  Helpers `apply_reader_gloss_cursor_tag_to_line`/`remove_…` mirror the existing
  pair in `src/app/mod.rs`.
- Theme change refresh: `src/input/actions/settings.rs` refreshes
  `reader-gloss-line` (set foreground to `theme.reader_gloss`, was `focus_color`)
  AND the new `reader-gloss-cursor-line` (foreground `theme.reader_gloss_cursor`).

### What does NOT change
The cursor-line background highlight; the gloss overlay's OWN cached-TTS coloring
(separate path); the set of glossed lines (`reader_gloss_lines`). No change to
navigation. NOTE: the off-cursor tint COLOR does change on the ~13 themes where
the raw focuscolor was dim/indistinct (that is the fix); on themes that already
looked right (rose-pine-dawn, gruvbox-material, …) the guard returns the raw
focuscolor unchanged, so they are visually identical to today.

### Testing (B)
- Unit: `complement_hex` rotates a known red to a teal hue; malformed input is
  safe (returns a valid `#rrggbb`).
- Unit: `contrast_ratio` matches known WCAG pairs (white/black ≈ 21; equal
  colors = 1).
- Unit: `ensure_gloss_color` — (a) returns the base unchanged when it already
  passes (rose-pine-dawn `#c4788a` on `#faf4ed` avoiding `#575279`); (b) for a
  dim base on a light bg (dayfox `#7b6b99` on `#f6f2ee`) returns a color with
  contrast-vs-bg ≥ 3.0; (c) the result is distinct (hue ≥ 40° or contrast ≥ 1.4)
  from every `avoid` color.
- Unit: `resolve_theme` — explicit `reader_gloss_cursor` override wins; absent →
  derived; `reader_gloss` defaults to the guarded focuscolor; the derived
  off-cursor and on-cursor colors are mutually distinct.
- Property-ish: for EVERY theme in `themes-unified.json`, `resolve_theme`'s
  `reader_gloss` and `reader_gloss_cursor` each pass contrast-vs-bg ≥ 3.0 and are
  mutually distinct (a loop over `load_all_themes()` asserting the invariant — the
  guard against future-theme regressions the audit found).
- Visual (e2e, ask user): on the previously-failing themes (dayfox,
  melange-light, everforest-light-medium) the off-cursor glossed paragraph now
  reads clearly tinted (not washed out), and moving the cursor onto it shows the
  distinct on-cursor color; on rose-pine-dawn the on-cursor color is teal
  `#56949f` and off-cursor is the rose `#c4788a` (unchanged).

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
