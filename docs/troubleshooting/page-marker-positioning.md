# Page-marker positioning: the Label-in-Overlay saga

The floating page marker is the small glyph at the bottom-center of a paginated
overlay page: **`⌄`** when more pages follow, **`•`** on the last page, nothing on
single-page content. It sits just below the last rendered line. Both the
**journal** and **gloss/synopsis** overlays have one.

This note records why it is **drawn with Cairo on the accent-bar `DrawingArea`**
rather than positioned as a `Label`, because the Label approach failed in a way
that took several rounds to diagnose and the wrong fix is tempting.

## TL;DR

- **Do NOT** re-introduce a `Label` positioned by `set_margin_top` inside the
  scroll `Overlay`. Its **allocation lags the margin change by several frames**, so
  on a page turn to a shorter page the glyph paints at the *previous* page's y
  (off the short page) until an unrelated relayout.
- The marker is drawn in `ui::draw_page_marker_glyph` from **both overlays'
  `bar_drawing` `set_draw_func`**. The draw reads live `buffer_to_window_coords`
  every paint, so there is no allocation step and no timing race.
- Render the glyph via **Pango** (`pangocairo::functions::show_layout`), NOT
  `cairo::Context::show_text`. Cairo's toy text API does no font fallback, so
  `⌄` (U+2304) rendered as a **tofu box** on fonts lacking the glyph.

## The symptoms (in the order they appeared)

1. **Chevron stranded mid-page.** The marker sat far above the last line.
2. **Chevron missing on the last page** until the user pressed `j` (block-nav),
   which forced a fresh render.
3. **Reappeared after toggling the dwl tag** with the app — i.e. an unrelated
   full relayout fixed it. This was the decisive clue: the *geometry* was right,
   the *timing/allocation* was wrong.
4. After switching to Cairo: **tofu box** instead of the glyph.

## Root causes (there were three, stacked)

### 1. `marker.preferred_size()` as the footer reserve (stranding)

The clamp `top = (bottom+gap).min(viewport_h - reserve)` used
`marker.preferred_size().height()` as `reserve`. For an `Overlay` child with
`set_measure_overlay(false)`, that measured height **balloons to the whole overlay
allocation (~800px)**, so `viewport_h - reserve` went tiny and `top` was clamped
far above the last line. (Fixed at the time with a fixed 28px reserve; now moot.)

### 2. Overlay-child allocation lags `set_margin_top` (missing / late)

This is the core bug. The Label was `valign=Start` inside the scroll `Overlay`;
its y was `margin_top`. On a page turn we measured the new last-line bottom and
called `set_margin_top(new_y)`. **Logging proved** `set_margin_top(449)` was
called, but the Label's *allocation* stayed at `y=810` (the previous full page's
bottom) for several frames:

```
MARKER-POS: bottom=441 top=449 ... alloc=(762,810,12,25)   # margin says 449, alloc still 810
```

`queue_resize()` did not force a synchronous re-allocation — GTK batches layout,
and an `Overlay` child's allocation is driven by the parent's layout pass, which
had not run yet. A single `idle_add_local_once` reposition **races the reflow**;
and because these overlays always render a page that FITS, the scroll range does
not change between same-fitting pages, so the `vadjustment::changed` "settle"
hook never fires to correct it. Only an unrelated relayout (the dwl tag toggle)
re-allocated the child — hence "reappears after toggling the tag."

Attempts that did **not** fully fix it (don't repeat these):

- One-shot idle reposition — races the reflow.
- Tick-callback that stops on the first non-zero measurement — accepts the stale
  *previous-page* geometry (a valid non-zero value) before the reflow.
- Tick-callback that waits for two stable frames — correct but "slow to appear."
- `queue_resize()` after `set_margin_top` — the allocation still lagged.

The real problem is not *when we measure* but that **`set_margin_top` on an
overlay child does not take effect this frame**. GTK-rs does not expose
`OverlayLayout` / a `get_child_position` vfunc, so there is no declarative way to
place the child synchronously either.

### 3. Cairo toy fonts don't fall back (tofu)

After moving to Cairo drawing, `cr.show_text("⌄")` produced a missing-glyph box:
`cairo::Context::show_text` uses the toy font API with **no font substitution**,
and the selected face lacked U+2304. The fix is to render a `pango::Layout` (which
does automatic font fallback, exactly like the old CSS-styled Label did) via
`pangocairo::functions::show_layout`. This added the `pangocairo` dependency
(0.20, matching `pango` 0.20).

## The fix (current design)

- `ui::measure_last_line_bottom(view)` — last text line's bottom in **widget
  coords** (`line_yrange(end_iter)` → `buffer_to_window_coords(Widget, …)`), the
  same scroll-aware path the accent bar uses.
- `ui::draw_page_marker_glyph(cr, view, area_w, glyph, rgb, alpha, gap)` — draws
  the glyph centered horizontally, `gap` px below the last line, via a Pango
  layout at ~20px in the theme dim color. No-op when `glyph` is `None` or geometry
  isn't up yet (the next repaint catches it).
- Each overlay holds `marker_glyph: Rc<RefCell<Option<&'static str>>>` and
  `marker_color`, drawn at the **top** of its `bar_drawing` draw func (before the
  selection-bar early-return, so it shows while editing too).
- `update_page_marker` sets `marker_glyph` from `pagination::page_marker`, then
  `bar_drawing.queue_draw()` **plus** an `idle_add_local_once(queue_draw)` so the
  bar also repaints after the page-turn reflow (the scroll range may be unchanged).
- Color is `theme.dim_fg`, threaded via `set_marker_color` at startup and in
  `apply_theme_to_state` (responsive to a dwl theme change).

The accent bar has always been reliable because it draws this same way; the marker
now inherits that reliability.

## Rules for the future

- **Keep the marker in the Cairo draw path.** If you need to move/restyle it, edit
  `draw_page_marker_glyph` and the per-overlay `marker_glyph`/`marker_color` state
  — do not add a positioned widget.
- **Never position a floating overlay child by `set_margin_top` and expect it to
  take effect the same frame.** The allocation lags. Draw it, or accept a
  multi-frame settle.
- **Draw text over the text view with Pango, not `cairo::show_text`** — you need
  font fallback for non-ASCII glyphs.
- Any change here is **pixel-level**: verify on screen (page down to a *short* last
  page, page up, and after a highlight `;wq`), not from logs. See the headless
  harness in `CLAUDE.md` if you cannot use the live session.
