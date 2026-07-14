# Design — inset tinted panel for the prose overlays

_Date: 2026-07-01 (US Central)._

## Problem

In two-column (verse/play) layout the journal, gloss, and synopsis overlays
size to `main_card_rect` — `window_width * TWO_COLUMN_WIDTH_FRACTION` (0.68), a
**wide** card meant for two verse columns side by side. But each of these
overlays renders a **single prose column** with symmetric internal margins
(`prose_column_margin = card_width/5` or `card_side_margin = card_width/4` on the
gloss view's `left_margin`/`right_margin`). The result is a narrow prose column
floating in the middle of a wide card, with large empty left/right gutters that
read as emptiness rather than intentional whitespace. The user finds this
"overwhelming."

The two-column **translation** overlay does NOT have this problem — it fills its
card with two columns — and is out of scope.

## Decision (from brainstorming)

Keep the current card sizes and text margins. Add a **visual frame**: an
**inset, subtly-tinted rounded panel** behind the prose text column so the
gutters read as deliberate matting and the column reads as an inset card.

- **Tint prominence:** _barely-there_ — a ~3–5% luminance shift from the card's
  `gloss_bg` cream. Visible as a soft panel edge, never boxy/heavy on any theme.
- **No border, no drop shadow.** Tint only.

## Scope

The three prose overlays:

- **Gloss** overlay (`src/ui/gloss_overlay.rs`).
- **Synopsis** — renders THROUGH the gloss overlay widget
  (`GlossOverlay::show_synopsis`), so it is covered by the gloss change with no
  separate widget work.
- **Journal** overlay (`src/ui/journal_overlay.rs`).

Explicitly OUT of scope: the translation overlay (two full columns, no gutter),
the reading card, and all pickers.

## Architecture

### Where the panel is drawn

Each of the two overlay widgets already hosts a `bar_drawing`
`gtk4::DrawingArea` as an overlay on its scroll `Overlay`
(`gloss_scroll_overlay` / `scroll_overlay`). That DrawingArea draws the accent
bar and the page-marker glyph via Cairo, reading the view's live geometry each
paint (no allocation race). We reuse this proven pattern for the panel.

**Z-order correction.** In a `gtk4::Overlay`, overlay children paint ABOVE the
main child, and the scroll (with its now-transparent view) is the main child. So
a `panel_drawing` added as an *overlay* would paint on top of the text — wrong.
The panel must paint BELOW the transparent view. Two valid placements:

- **(preferred) Panel as the scroll overlay's MAIN CHILD, scroll on top.**
  Restructure so `panel_drawing` is the Overlay's `set_child`, and the
  `ScrolledWindow` becomes an overlay added on top (non-measuring — it already
  sizes itself). Then `bar_drawing` is added last (top). Paint order:
  panel (bottom) → transparent scroll/text → accent bar + page marker (top). This
  is the clean layering.
- **(fallback) Paint the panel in the container/scroll background** via a
  `snapshot`/`draw` on the widget behind the scroll, if restructuring the Overlay
  child proves invasive.

Prefer the first. Its `draw_func` fills one rounded rectangle:

- `x0 = view.left_margin() - PANEL_PAD`
- `x1 = area_w - view.right_margin() + PANEL_PAD`
- `y0 = 0`, `y1 = area_h` (full scroll-region height)
- corner radius `PANEL_RADIUS`
- fill = the panel tint (below)

`PANEL_PAD` (~10px) lets the panel breathe a few px outside the text ink.
`PANEL_RADIUS` ~12px matches the card's `border-radius`.

The panel spans the **scroll region**, which is the vertical extent of the prose
content area (title/headers/footer are outside it). This is the correct region
to frame — the reading text — and it aligns exactly with the view's horizontal
text margins, so the frame hugs the column on every work/theme without hand-tuned
offsets.

**The opaque-TextView-background obstacle (resolved).** The current CSS sets
`textview.gloss-text { background-color: {gloss_bg}; }` and
`textview.gloss-text text { background-color: {gloss_bg}; }` — an OPAQUE cream
fill on the view AND its text node. A panel drawn on a DrawingArea *beneath* the
ScrolledWindow would be completely hidden by that opaque fill. So the panel
approach cannot be "draw behind the view."

Resolution: the panel IS the view's background. The `panel_drawing` sits over the
scroll overlay (above the view in z-order but drawn only in the gutter-free inset
rect? no — it would cover the text). Instead, we invert the layers:

- Make the **TextView background TRANSPARENT** in the three prose overlays
  (`textview.gloss-text { background-color: transparent; }` and its `text` node),
  so whatever is painted behind the scroll shows through the text's inter-glyph
  space.
- Draw `panel_drawing` BELOW the scroll (added to the scroll overlay first, or —
  cleaner — paint the panel in the scroll overlay's own background before its
  child). The card container keeps `gloss_bg` (matting); the inset rounded rect
  is the panel tint; the transparent view lets the tint show through under the
  text.

Net: the card gutters = `gloss_bg` cream (matting); the inset rect = the panel
tint; the text floats on the panel. The barely-there delta means the text's
legibility is unaffected (contrast to text_fg is essentially unchanged by a
3–5% bg shift).

**Alternative if transparency causes a flash/artifact** (fallback, note in the
plan): instead of a Cairo panel + transparent view, keep the view opaque but set
its `background-color` to `overlay_panel_bg` and paint the *gutters* as the
matting. This is simpler CSS-wise but makes the panel a full-width band (no
inset/rounded edge) unless the view width is constrained to the column — which
reintroduces the geometry-lockstep problem. Prefer the Cairo panel + transparent
view; fall back only if transparency misbehaves on the headless/software
renderer.

### The panel color

A new derived color, `Theme::overlay_panel_bg`, computed once per theme from
`gloss_background(theme)`:

- **Light themes** (`theme.is_light`): `darken_color(gloss_bg, 0.965)` — ~3.5%
  darker, a soft matting.
- **Dark themes:** lighten by the same delta — `blend_colors("#ffffff", gloss_bg,
  0.05)` (5% toward white), since darkening a dark bg would vanish.

Both use existing helpers (`darken_color`, `blend_colors`). The exact factors are
tuning constants in one place; the barely-there target is ~3–5% luminance shift.

The `panel_drawing` reads this as an `(r,g,b)` tuple stored in an
`Rc<RefCell<(f64,f64,f64)>>` on each overlay (mirroring `marker_color`), threaded
in at construction and **repainted on theme change** through the existing
`apply_theme_to_state` path (the same path that refreshes `marker_color`).

### Why Cairo, not a CSS-styled inset widget

- The panel must align to the view's live `left_margin`/`right_margin` and span
  the scroll region's settled height. A CSS box would need a separately-sized
  widget kept in lockstep with the view margins across every work type and
  resize — the exact "each overlay re-derives geometry with hand-tuned offsets"
  bug class the codebase already consolidated. A DrawingArea reads the real
  geometry every paint.
- The overlays already own this DrawingArea idiom (accent bar + page marker), so
  this is one more layer in a familiar place, theme-wired the same way.

## Components / changes

1. **`src/theme.rs`**
   - New `fn overlay_panel_bg(theme) -> String` (light: darken; dark: lighten).
   - Add `pub overlay_panel_bg: String` to `Theme` (or compute at CSS-gen /
     apply time if `Theme` construction is centralized — follow how
     `reader_gloss` is populated). Populate it wherever the derived gloss colors
     are set.
   - **Make the prose-overlay TextView background transparent** so the panel
     shows through: change `textview.gloss-text { background-color: {gloss_bg}; }`
     and `textview.gloss-text text { background-color: {gloss_bg}; }` to
     `transparent`. (Keep `.gloss-bottom-clip` / `.translation-col` on `gloss_bg`
     — the translation overlay is unframed, and the bottom-clip must stay opaque
     to hide the partial line.) VERIFY the translation columns still read
     correctly after this (they use `.gloss-text` too — if so, give the framed
     prose views a distinct CSS class, e.g. `.overlay-prose`, and make only THAT
     transparent, leaving `.gloss-text`/`.translation-col` opaque).
   - Small unit test asserting the panel tint differs from `gloss_bg` and stays
     close (bounded luminance delta) for a light and a dark sample theme.

2. **`src/ui/gloss_overlay.rs`**
   - Add `panel_color: Rc<RefCell<(f64,f64,f64)>>` and a `panel_drawing`
     DrawingArea; add it to `gloss_scroll_overlay` FIRST (below `bar_drawing` and
     below the scroll child in paint order), non-measuring + clipping.
   - `panel_drawing.set_draw_func` fills the inset rounded rect using
     `gloss_view.left_margin()/right_margin()`, `area_w`, `area_h`, and
     `panel_color`.
   - Repaint on the scroll `vadjustment` change (same as `bar_drawing`) so it
     tracks height settle.
   - Thread the panel color at construction; update it in `apply_theme_to_state`
     (beside the existing `marker_color` refresh) and `queue_draw`.
   - **CSS class check:** ensure `gloss_view` (the framed prose) carries the
     transparent class from step 1, and the translation views do NOT.

3. **`src/ui/journal_overlay.rs`**
   - The identical addition on its `scroll_overlay` / `bar_drawing` pair; its
     `view` carries the transparent prose class.

4. **Shared helper (optional, if it reduces duplication):** a
   `crate::ui::draw_overlay_panel(cr, view, area_w, area_h, rgb, pad, radius)` in
   `src/ui/mod.rs` next to `draw_page_marker_glyph`, called by both overlays'
   `panel_drawing` draw funcs (both are byte-identical otherwise). Follow the
   `draw_page_marker_glyph` signature style.

## Data flow

theme (light/dark) → `overlay_panel_bg()` → `Theme.overlay_panel_bg` → overlay
`panel_color` (rgb) at construction and on `apply_theme_to_state` → `panel_drawing`
`draw_func` fills the inset rounded rect on every paint, reading live view margins.

## Testing

- **Unit (pure):** `overlay_panel_bg` differs from `gloss_bg` but by a small,
  bounded luminance delta, for a light sample and a dark sample. (Uses the
  existing `relative_luminance`/`contrast_ratio` helpers.)
- **On-screen (user):** open each overlay in 2-col layout (a play, e.g.
  Cymbeline) and confirm the prose column reads as a softly-framed inset panel,
  the gutters look intentional, and the frame is subtle (not boxy) — repeat after
  a theme change (super+\) on one light and one dark theme. This is a pixel-level
  visual acceptance; the headless harness can screenshot it, or the user eyeballs
  it. No unit test covers the rendered pixels.

## Risks / notes

- **Barely-there on ~40 themes:** the delta is a fixed factor; a theme with an
  unusual `gloss_bg` could make the panel invisible or (rarely) too strong. The
  unit test bounds the delta; the on-screen check on one light + one dark theme
  is the acceptance. If a specific theme reads wrong, it's a per-theme tuning
  follow-up, not a redesign.
- **Panel height = scroll region only** (not the whole card). Deliberate: we
  frame the reading text, leaving title/footer chrome on the plain card cream,
  matching the card's own header/footer rules.
- **z-order:** `panel_drawing` MUST be added before `bar_drawing` so the accent
  bar and page marker paint on top of the panel, and the text (scroll child)
  stays legible above it. Verify the accent bar + page marker still show.
