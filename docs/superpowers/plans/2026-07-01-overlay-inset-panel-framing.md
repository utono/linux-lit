# Overlay Inset Tinted Panel Framing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Frame the single prose column in the journal, gloss, and synopsis
overlays with a barely-there inset tinted rounded panel, so the wide two-column
card's empty side gutters read as deliberate matting instead of emptiness.

**Architecture:** Add a Cairo `panel_drawing` DrawingArea to each of the two
overlay widgets (gloss + journal; synopsis renders through the gloss widget), an
overlay painted *below* the existing `bar_drawing` accent-bar layer. It fills one
inset rounded rectangle in a new derived theme color `overlay_panel_bg`, aligned
to the view's live `left_margin`/`right_margin` and spanning the scroll region.
The framed prose views switch from the opaque `.gloss-text` background to a new
transparent `.overlay-prose` class so the panel shows through under the text; the
translation overlay's columns (which also use `.gloss-text`) stay opaque and
unframed.

**Tech Stack:** Rust, GTK4 (gtk4-rs), Cairo (`gtk4::cairo`), the existing
DrawingArea / `draw_page_marker_glyph` idiom.

## Global Constraints

- **Barely-there tint:** `overlay_panel_bg` is a ~3–5% luminance shift from
  `gloss_bg` — light themes darken (~3.5%), dark themes lighten (~5% toward white).
  No border, no drop shadow. Tint only.
- **Scope is exactly three prose overlays:** gloss, synopsis (via the gloss
  widget), journal. OUT of scope: the translation overlay, the reading card, all
  pickers.
- **Do NOT re-derive geometry with hand-tuned offsets.** The panel reads the
  view's live `left_margin()`/`right_margin()` and the DrawingArea's `area_w`/
  `area_h` every paint (same idiom as `bar_drawing` / `draw_page_marker_glyph`).
- **Theme-responsive:** the panel color is refreshed on every theme change through
  the existing `apply_theme_to_state` path, mirroring `set_marker_color`.
- **z-order rule:** `panel_drawing` MUST be added as an overlay BEFORE
  `bar_drawing` so the accent bar, line numbers, and page marker paint on top of
  the panel, and the transparent text (scroll child) stays legible above it.
- **Verify with:** `cargo build`, `cargo test --bins` (601 pass baseline before
  this work), `cargo clippy`. Visual acceptance is on-screen only (user runs
  `cargo run` or the e2e harness) — no unit test covers rendered pixels.

---

## File Structure

- **`src/theme.rs`** — add `overlay_panel_bg` derivation + `Theme` field; split
  `.gloss-text` background CSS into a new transparent `.overlay-prose` class;
  unit test for the color delta.
- **`src/ui/mod.rs`** — shared `draw_overlay_panel` Cairo helper (both overlays'
  panel draw funcs are byte-identical otherwise), next to `draw_page_marker_glyph`.
- **`src/ui/gloss_overlay.rs`** — `panel_color` field + `panel_drawing`
  DrawingArea + `set_panel_color`; swap `gloss_view`'s CSS class to
  `.overlay-prose`.
- **`src/ui/journal_overlay.rs`** — the identical addition on its
  `scroll_overlay` / `bar_drawing` pair; swap `view`'s CSS class.
- **`src/input/actions/settings.rs`** + **`src/app/mod.rs`** — call
  `set_panel_color` beside the two existing `set_marker_color` calls.

---

## Design decisions locked in (read before Task 1)

1. **The `.gloss-text` collision is real and MUST be handled.** The translation
   overlay's columns call `view.add_css_class("gloss-text")`
   (`src/ui/translation_overlay.rs:645`), the SAME class as the framed prose
   views. Making `.gloss-text` transparent would also make the translation
   columns transparent. Therefore the framed prose views get a NEW class
   `.overlay-prose` (transparent bg), and `.gloss-text` / `.translation-col` stay
   opaque `gloss_bg`. This is the design's step-1 resolution, now confirmed
   mandatory.

2. **Layering — panel as the FIRST overlay, not a restructured main child.** The
   design's preferred "panel as the Overlay's main child, scroll on top" would
   force the ScrolledWindow AND the bottom-clip guard box to all become overlays
   in a specific order — invasive, and it touches the delicate `BottomClipGuard`
   attachment (`gloss_overlay.rs:409`). Instead: keep `set_child(scroll)`
   unchanged and add `panel_drawing` via `add_overlay` FIRST (before
   `bar_drawing`). GTK paints the main child (scroll) at the bottom, then overlays
   in add-order. With the framed view transparent, the paint order is:
   scroll/viewport (matting cream) → **panel_drawing (tint rect)** →
   transparent text → bar_drawing (accent bar + page marker + clip guard on top).
   The tint sits directly under the text, framed to the column, no restructure.

3. **The matting cream must still paint behind the transparent view.** With the
   view transparent, the ScrolledWindow/viewport background provides the gutter
   cream. The existing `textview { background-color: {bg}; }` base rule
   (`theme.rs:514`) and the `.gloss-overlay` container already paint cream, so the
   gutters stay cream. If a specific renderer shows the window root through the
   transparent view instead of cream, the fallback (Task 8 note) sets the
   ScrolledWindow's own bg — but do NOT pre-add that; only if the on-screen check
   shows bleed-through.

---

### Task 1: Derive the `overlay_panel_bg` theme color

**Files:**
- Modify: `src/theme.rs` — add `overlay_panel_bg` field to `Theme` (near
  `reader_gloss`, ~`src/theme.rs:20`), a derivation fn, populate it in the two
  `Theme { ... }` constructors (`src/theme.rs:188-203` and `src/theme.rs:206-223`).
- Test: `src/theme.rs` (inline `#[cfg(test)]` module, where the other theme tests
  live).

**Interfaces:**
- Consumes: existing `gloss_background(theme) -> String` (`src/theme.rs:489`),
  `darken_color(hex, factor) -> String` (`src/theme.rs:233`),
  `blend_colors(fg_hex, bg_hex, alpha) -> String` (`src/theme.rs:250`),
  `relative_luminance` / `contrast_ratio` (`src/theme.rs:389`).
- Produces: `Theme.overlay_panel_bg: String` (a `#rrggbb` hex);
  `fn overlay_panel_bg(theme: &Theme) -> String`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/theme.rs`:

```rust
#[test]
fn overlay_panel_bg_is_a_small_bounded_delta_from_gloss_bg() {
    // A light sample theme (cream gloss bg) and a dark sample theme.
    let light = Theme {
        is_light: true,
        text_bg: "#fbf1c7".to_string(),
        ..default_theme()
    };
    let dark = Theme {
        is_light: false,
        text_bg: "#282828".to_string(),
        ..default_theme()
    };

    for theme in [&light, &dark] {
        let gloss_bg = gloss_background(theme);
        let panel = overlay_panel_bg(theme);
        // Barely-there: distinct from the card bg...
        assert_ne!(panel, gloss_bg, "panel tint must differ from gloss_bg");
        // ...but close — contrast ratio between panel and gloss_bg is tiny
        // (both are near-identical luminance; ratio ~1.0, well under 1.15).
        let ratio = contrast_ratio(&panel, &gloss_bg);
        assert!(
            ratio > 1.0 && ratio < 1.15,
            "panel tint delta out of the barely-there band: ratio={ratio} \
             (panel={panel}, gloss_bg={gloss_bg}, is_light={})",
            theme.is_light
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib overlay_panel_bg_is_a_small_bounded_delta_from_gloss_bg 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'overlay_panel_bg'` (and `Theme` has no
`overlay_panel_bg` field yet if you already added the `..default_theme()` spread
— add the fn first in Step 3).

- [ ] **Step 3: Add the derivation fn**

Add near the other private color helpers in `src/theme.rs` (e.g. right after
`gloss_background`, ~`src/theme.rs:499`):

```rust
/// The inset-panel tint for the prose overlays: a barely-there (~3–5%) luminance
/// shift from the card's `gloss_bg` cream. Light themes darken; dark themes
/// lighten (darkening a dark bg would vanish). Never boxy on any theme.
fn overlay_panel_bg(theme: &Theme) -> String {
    let gloss_bg = gloss_background(theme);
    if theme.is_light {
        // ~3.5% darker.
        darken_color(&gloss_bg, 0.965)
    } else {
        // ~5% toward white.
        blend_colors("#ffffff", &gloss_bg, 0.05)
    }
}
```

- [ ] **Step 4: Add the `Theme` field and populate both constructors**

In the `Theme` struct definition (after `reader_gloss_cursor`, ~`src/theme.rs:21`):

```rust
    pub overlay_panel_bg: String, // inset prose-overlay panel tint (barely-there)
```

`overlay_panel_bg` derives from `is_light` + `text_bg`, both already set in the
`Theme { ... }` literal, so it cannot be computed inside the same literal that is
still being constructed. Build the `Theme` into a `let mut theme = Theme { ... };`
then set the field. In the main constructor (`load_theme`-style fn, the
`Theme { ... }` at `src/theme.rs:188`), change:

```rust
    Theme {
        name: name.to_string(),
        // ... all existing fields ...
        reader_gloss,
        reader_gloss_cursor,
        overlay_panel_bg: String::new(), // set below
    }
```

to build-then-populate:

```rust
    let mut theme = Theme {
        name: name.to_string(),
        // ... all existing fields ...
        reader_gloss,
        reader_gloss_cursor,
        overlay_panel_bg: String::new(),
    };
    theme.overlay_panel_bg = overlay_panel_bg(&theme);
    theme
}
```

Do the SAME in `default_theme()` (`src/theme.rs:206`):

```rust
fn default_theme() -> Theme {
    let mut theme = Theme {
        name: "default".to_string(),
        // ... all existing fields ...
        reader_gloss: ensure_gloss_color("#d4be98", "#282828", &["#d4be98"]),
        reader_gloss_cursor: ensure_gloss_color(&complement_hex("#d4be98"), "#282828", &["#d4be98"]),
        overlay_panel_bg: String::new(),
    };
    theme.overlay_panel_bg = overlay_panel_bg(&theme);
    theme
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib overlay_panel_bg 2>&1 | tail -20`
Expected: PASS (1 passed).

- [ ] **Step 6: Build to confirm every `Theme { ... }` literal compiles**

Run: `cargo build 2>&1 | rg -n "missing field .overlay_panel_bg|error" | head` 
Expected: no output (no missing-field errors). If any OTHER `Theme { ... }`
literal exists (e.g. in a test module), add `overlay_panel_bg: String::new()` (or
a real value) there too until the build is clean.

- [ ] **Step 7: Commit**

```bash
git add src/theme.rs
git commit -m "feat(theme): derive overlay_panel_bg — barely-there inset-panel tint

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
```

---

### Task 2: Split `.gloss-text` bg into a transparent `.overlay-prose` class

**Files:**
- Modify: `src/theme.rs` — the CSS-generation string (`generate_css`,
  `src/theme.rs:626-640` region).

**Interfaces:**
- Consumes: nothing new.
- Produces: a `.overlay-prose` CSS class with a TRANSPARENT background (both the
  `textview` node and its `text` node), leaving `.gloss-text` and
  `.translation-col` opaque `gloss_bg`. The gloss/journal prose views will adopt
  `.overlay-prose` in Tasks 4–5.

- [ ] **Step 1: Locate the current opaque rules**

The current rules (`src/theme.rs:626-640`) are:

```
         .gloss-bottom-clip {{ background-color: {gloss_bg}; }} \
         textview.gloss-text {{ background-color: {gloss_bg}; }} \
         textview.gloss-text text {{ background-color: {gloss_bg}; }} \
         ...
         .gloss-text {{ font-family: {font}; font-size: {size}pt; }} \
         textview.translation-col text {{ color: {dim}; font-style: italic; }} \
         textview.translation-col {{ background-color: {gloss_bg}; }} \
```

- [ ] **Step 2: Add the transparent `.overlay-prose` rules**

Immediately AFTER the two `textview.gloss-text` bg rules, add (keep the
`.gloss-text` rules — the translation columns and echo header still use that
class opaque):

```
         textview.overlay-prose {{ background-color: transparent; }} \
         textview.overlay-prose text {{ background-color: transparent; }} \
```

The final grouped block reads:

```rust
         .gloss-bottom-clip {{ background-color: {gloss_bg}; }} \
         textview.gloss-text {{ background-color: {gloss_bg}; }} \
         textview.gloss-text text {{ background-color: {gloss_bg}; }} \
         textview.overlay-prose {{ background-color: transparent; }} \
         textview.overlay-prose text {{ background-color: transparent; }} \
```

Leave the `.gloss-text` font rule (`src/theme.rs:638`) and the `.translation-col`
rules (`src/theme.rs:639-640`) unchanged. The framed prose views (Tasks 4–5) will
carry BOTH `.gloss-text` (for font-family/size) AND `.overlay-prose` (for the
transparent bg override); CSS specificity: `textview.overlay-prose` and
`textview.gloss-text` are equal-specificity, so ensure `.overlay-prose` bg is
listed AFTER `.gloss-text` bg (it is, per the ordering above) so `transparent`
wins on views carrying both classes.

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: `Finished` (CSS is a runtime string; only a `{}`-escaping typo would
break the build).

- [ ] **Step 4: Commit**

```bash
git add src/theme.rs
git commit -m "feat(theme): add transparent .overlay-prose class (framed prose bg)

Splits the opaque .gloss-text background so framed prose views can be
transparent (panel shows through) while translation columns and the echo
header keep the opaque gloss_bg.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
```

---

### Task 3: Shared `draw_overlay_panel` Cairo helper

**Files:**
- Modify: `src/ui/mod.rs` — add the fn next to `draw_page_marker_glyph`.

**Interfaces:**
- Consumes: `gtk4::TextView` (for `left_margin()`/`right_margin()`),
  `&gtk4::cairo::Context`, the DrawingArea `area_w`/`area_h`.
- Produces:
  `pub fn draw_overlay_panel(cr: &gtk4::cairo::Context, view: &gtk4::TextView, area_w: i32, area_h: i32, rgb: (f64, f64, f64), pad: f64, radius: f64)`
  — fills ONE inset rounded rect from
  `view.left_margin() - pad` to `area_w - view.right_margin() + pad`, y `0..area_h`.

- [ ] **Step 1: Read the neighbor for signature/style**

Read `draw_page_marker_glyph` in `src/ui/mod.rs` (it is `pub fn`, takes `cr`, a
`&gtk4::TextView`, an `i32` width, and colors) to match its parameter style and
the module's Cairo idioms.

- [ ] **Step 2: Add the helper**

Add to `src/ui/mod.rs` right after `draw_page_marker_glyph`:

```rust
/// Fill the inset tinted panel behind a prose overlay's text column. Draws ONE
/// rounded rectangle aligned to the view's live text margins (so it hugs the
/// column on every work/theme with no hand-tuned offsets) and spanning the full
/// DrawingArea height (the scroll region). Barely-there tint only — no border,
/// no shadow. Painted BELOW the accent bar / page marker (added as an earlier
/// overlay) and below the transparent text, so it reads as the text's backdrop.
pub fn draw_overlay_panel(
    cr: &gtk4::cairo::Context,
    view: &gtk4::TextView,
    area_w: i32,
    area_h: i32,
    rgb: (f64, f64, f64),
    pad: f64,
    radius: f64,
) {
    let x0 = (view.left_margin() as f64 - pad).max(0.0);
    let x1 = (area_w as f64 - view.right_margin() as f64 + pad).min(area_w as f64);
    let y0 = 0.0_f64;
    let y1 = area_h as f64;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let w = x1 - x0;
    let h = y1 - y0;
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);

    // Rounded-rectangle path (four arcs).
    let (r0, g0, b0) = rgb;
    cr.new_sub_path();
    cr.arc(x1 - r, y0 + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x1 - r, y1 - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x0 + r, y1 - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x0 + r, y0 + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
    cr.set_source_rgb(r0, g0, b0);
    let _ = cr.fill();
}
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: `Finished`. (No unit test — it is pure Cairo drawing, verified visually
in the overlay tasks. This step's deliverable is a compiling shared helper the
next two tasks consume.)

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat(ui): add draw_overlay_panel — inset rounded-rect panel helper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
```

---

### Task 4: Gloss overlay — panel_drawing + transparent view + set_panel_color

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — struct field, construction, draw func,
  overlay wiring, CSS class swap, `set_panel_color` method.

**Interfaces:**
- Consumes: `crate::ui::draw_overlay_panel` (Task 3),
  `crate::theme::hex_to_rgb`-equivalent (see the existing `set_marker_color` body
  at `src/ui/gloss_overlay.rs:602` for how a hex string is parsed to `(f64,f64,f64)`).
- Produces: `GlossOverlay::set_panel_color(&self, hex: &str)` (mirrors
  `set_marker_color`), consumed by Task 6.

- [ ] **Step 1: Add the `panel_color` field**

In the `GlossOverlay` struct (near `marker_color`, `src/ui/gloss_overlay.rs:71`):

```rust
    panel_color: Rc<RefCell<(f64, f64, f64)>>,
```

- [ ] **Step 2: Create the DrawingArea + color state, before `bar_drawing` is added**

In `new`, alongside where `marker_color` is created (`src/ui/gloss_overlay.rs:290`),
add:

```rust
        let panel_color: Rc<RefCell<(f64, f64, f64)>> =
            Rc::new(RefCell::new((0.95, 0.93, 0.86))); // placeholder; set at startup
        let panel_drawing = gtk4::DrawingArea::new();
        panel_drawing.set_can_target(false);
```

- [ ] **Step 3: Set the panel draw func (reads live view margins)**

After the `bar_drawing.set_draw_func(...)` block closes (`src/ui/gloss_overlay.rs:380`),
add:

```rust
        {
            let view_for_panel = gloss_view.clone();
            let panel_color_clone = panel_color.clone();
            panel_drawing.set_draw_func(move |_area, cr, w, h| {
                crate::ui::draw_overlay_panel(
                    cr,
                    &view_for_panel,
                    w,
                    h,
                    *panel_color_clone.borrow(),
                    10.0, // PANEL_PAD — breathe a few px outside the text ink
                    12.0, // PANEL_RADIUS — matches the card border-radius
                );
            });
        }
```

- [ ] **Step 4: Add `panel_drawing` as the FIRST overlay (below `bar_drawing`)**

The current wiring (`src/ui/gloss_overlay.rs:395-398`) is:

```rust
        gloss_scroll_overlay.set_child(Some(&gloss_scrolled));
        gloss_scroll_overlay.add_overlay(&bar_drawing);
        gloss_scroll_overlay.set_measure_overlay(&bar_drawing, false);
        gloss_scroll_overlay.set_clip_overlay(&bar_drawing, true);
```

Insert the panel overlay BEFORE `bar_drawing` (add-order = paint-order, first
added paints lowest):

```rust
        gloss_scroll_overlay.set_child(Some(&gloss_scrolled));
        gloss_scroll_overlay.add_overlay(&panel_drawing);
        gloss_scroll_overlay.set_measure_overlay(&panel_drawing, false);
        gloss_scroll_overlay.set_clip_overlay(&panel_drawing, true);
        gloss_scroll_overlay.add_overlay(&bar_drawing);
        gloss_scroll_overlay.set_measure_overlay(&bar_drawing, false);
        gloss_scroll_overlay.set_clip_overlay(&bar_drawing, true);
```

- [ ] **Step 5: Repaint the panel on scroll settle**

The `bar_drawing` already repaints on `vadjustment().connect_value_changed`
(`src/ui/gloss_overlay.rs:389-392`). Add the panel to that same closure so it
tracks the settled scroll-region height:

```rust
        {
            let bar_for_scroll = bar_drawing.clone();
            let panel_for_scroll = panel_drawing.clone();
            gloss_scrolled.vadjustment().connect_value_changed(move |_| {
                bar_for_scroll.queue_draw();
                panel_for_scroll.queue_draw();
            });
        }
```

- [ ] **Step 6: Swap the view's CSS class to the transparent prose class**

At `src/ui/gloss_overlay.rs:280` the view has `add_css_class("gloss-text")`. Add
the transparent class (keep `gloss-text` for the font rule):

```rust
        gloss_view.add_css_class("gloss-text");
        gloss_view.add_css_class("overlay-prose");
```

Do NOT add `overlay-prose` to `echo_header_view` (`src/ui/gloss_overlay.rs:427`)
— the echoes header is not framed and should keep its opaque bg.

- [ ] **Step 7: Store `panel_color` and `panel_drawing` in the struct literal**

In the `GlossOverlay { ... }` construction (near `marker_color,`,
`src/ui/gloss_overlay.rs:516`) add:

```rust
            marker_color,
            panel_color,
```

(`panel_drawing` does not need to be stored unless a later method queues it
directly — `set_panel_color` queues via a stored clone; store it too if the
borrow checker needs it. Prefer storing it: add `panel_drawing,` to the struct
def near `bar_drawing` at `src/ui/gloss_overlay.rs:60` and to the literal.)

- [ ] **Step 8: Add `set_panel_color` (mirror `set_marker_color`)**

`set_marker_color` (`src/ui/gloss_overlay.rs:602-606`) parses the hex via the
local `parse_hex_color(hex) -> Option<(f64,f64,f64)>` helper. Use the SAME helper:

```rust
    pub fn set_panel_color(&self, hex: &str) {
        if let Some(rgb) = parse_hex_color(hex) {
            *self.panel_color.borrow_mut() = rgb;
            self.panel_drawing.queue_draw();
        }
    }
```

- [ ] **Step 9: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: `Finished`. Fix any borrow/move error by cloning the `Rc` before the
draw closure (the pattern the file already uses for `marker_color_clone`).

- [ ] **Step 10: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss-overlay): inset tinted panel behind the prose column

Adds panel_drawing below bar_drawing, makes gloss_view transparent
(.overlay-prose), threads panel_color via set_panel_color. Synopsis renders
through this widget, so it is framed too.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
```

---

### Task 5: Journal overlay — the identical panel addition

**Files:**
- Modify: `src/ui/journal_overlay.rs` — struct field, construction, draw func,
  overlay wiring, CSS class swap, `set_panel_color` method.

**Interfaces:**
- Consumes: `crate::ui::draw_overlay_panel` (Task 3), the same hex→rgb parse as
  gloss (Task 4 Step 8).
- Produces: `JournalOverlay::set_panel_color(&self, hex: &str)`, consumed by Task 6.

- [ ] **Step 1: Add the `panel_color` field**

In the `JournalOverlay` struct (near `marker_color`,
`src/ui/journal_overlay.rs:92`):

```rust
    panel_color: Rc<RefCell<(f64, f64, f64)>>,
```

- [ ] **Step 2: Create the DrawingArea + color, before `bar_drawing` is added**

Alongside where `marker_color` / `bar_drawing` are created
(`src/ui/journal_overlay.rs:196-198`):

```rust
        let panel_color: Rc<RefCell<(f64, f64, f64)>> =
            Rc::new(RefCell::new((0.95, 0.93, 0.86))); // placeholder; set at startup
        let panel_drawing = gtk4::DrawingArea::new();
        panel_drawing.set_can_target(false);
```

- [ ] **Step 3: Set the panel draw func**

After the `bar_drawing.set_draw_func(...)` block closes
(the journal draw func ends around `src/ui/journal_overlay.rs:250`), add:

```rust
        {
            let view_for_panel = view.clone();
            let panel_color_clone = panel_color.clone();
            panel_drawing.set_draw_func(move |_area, cr, w, h| {
                crate::ui::draw_overlay_panel(
                    cr,
                    &view_for_panel,
                    w,
                    h,
                    *panel_color_clone.borrow(),
                    10.0,
                    12.0,
                );
            });
        }
```

(The journal view local is `view` — confirm the binding name; it is
`view.add_css_class("gloss-text")` at `src/ui/journal_overlay.rs:169`.)

- [ ] **Step 4: Add `panel_drawing` as the FIRST overlay (below `bar_drawing`)**

The current wiring (`src/ui/journal_overlay.rs:257-259`) is:

```rust
        scroll_overlay.add_overlay(&bar_drawing);
        scroll_overlay.set_measure_overlay(&bar_drawing, false);
        scroll_overlay.set_clip_overlay(&bar_drawing, true);
```

Insert the panel overlay BEFORE it. Note `set_child` was already called at
`src/ui/journal_overlay.rs:181`, so only the overlay-add order matters:

```rust
        scroll_overlay.add_overlay(&panel_drawing);
        scroll_overlay.set_measure_overlay(&panel_drawing, false);
        scroll_overlay.set_clip_overlay(&panel_drawing, true);
        scroll_overlay.add_overlay(&bar_drawing);
        scroll_overlay.set_measure_overlay(&bar_drawing, false);
        scroll_overlay.set_clip_overlay(&bar_drawing, true);
```

- [ ] **Step 5: Repaint the panel on scroll settle**

The journal repaints `bar_drawing` on `vadjustment().connect_value_changed`
(`src/ui/journal_overlay.rs:252-255`). Add the panel:

```rust
        {
            let bar_for_scroll = bar_drawing.clone();
            let panel_for_scroll = panel_drawing.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                bar_for_scroll.queue_draw();
                panel_for_scroll.queue_draw();
            });
        }
```

- [ ] **Step 6: Swap the view's CSS class**

At `src/ui/journal_overlay.rs:169`:

```rust
        view.add_css_class("gloss-text");
        view.add_css_class("overlay-prose");
```

- [ ] **Step 7: Store `panel_color` (+ `panel_drawing`) in the struct literal**

Add `panel_color,` near `marker_color,` (`src/ui/journal_overlay.rs:358`), and
`panel_drawing,` near `bar_drawing,` in both the struct def
(`src/ui/journal_overlay.rs:20`) and the literal (`src/ui/journal_overlay.rs:332`).

- [ ] **Step 8: Add `set_panel_color`**

After `set_marker_color` (`src/ui/journal_overlay.rs:658-662`), using the same
local `parse_hex_color` helper `set_marker_color` uses:

```rust
    pub fn set_panel_color(&self, hex: &str) {
        if let Some(rgb) = parse_hex_color(hex) {
            *self.panel_color.borrow_mut() = rgb;
            self.panel_drawing.queue_draw();
        }
    }
```

- [ ] **Step 9: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 10: Commit**

```bash
git add src/ui/journal_overlay.rs
git commit -m "feat(journal-overlay): inset tinted panel behind the prose column

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
```

---

### Task 6: Thread `overlay_panel_bg` at startup and on theme change

**Files:**
- Modify: `src/app/mod.rs:1210-1211` (startup, beside the two `set_marker_color`).
- Modify: `src/input/actions/settings.rs:278-279` (theme change, beside the two
  `set_marker_color`).

**Interfaces:**
- Consumes: `GlossOverlay::set_panel_color` / `JournalOverlay::set_panel_color`
  (Tasks 4–5), `theme.overlay_panel_bg` (Task 1).
- Produces: nothing (side-effecting glue).

- [ ] **Step 1: Startup wiring**

At `src/app/mod.rs:1210-1211`, after:

```rust
    gloss_overlay.set_marker_color(&theme.dim_fg);
    journal_overlay.set_marker_color(&theme.dim_fg);
```

add:

```rust
    gloss_overlay.set_panel_color(&theme.overlay_panel_bg);
    journal_overlay.set_panel_color(&theme.overlay_panel_bg);
```

- [ ] **Step 2: Theme-change wiring**

At `src/input/actions/settings.rs:278-279`, after:

```rust
    state.gloss_overlay.set_marker_color(&theme.dim_fg);
    state.journal_overlay.set_marker_color(&theme.dim_fg);
```

add:

```rust
    state.gloss_overlay.set_panel_color(&theme.overlay_panel_bg);
    state.journal_overlay.set_panel_color(&theme.overlay_panel_bg);
```

- [ ] **Step 3: Build + full test suite + clippy**

Run: `cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3`
Expected: `Finished`; `test result: ok. 602 passed` (601 baseline + the new
`overlay_panel_bg` test); clippy no NEW warnings (baseline is pre-existing drift).

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs src/input/actions/settings.rs
git commit -m "feat(overlays): thread overlay_panel_bg at startup + on theme change

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
```

---

### Task 7: On-screen verification (user / e2e)

**Files:** none (verification only).

This is the pixel-level visual acceptance the plan cannot unit-test. The agent
CANNOT launch cage from the live dwl session — hand this to the user with the
exact steps, OR run the e2e harness through the env wrapper if it renders.

- [ ] **Step 1: Ask the user to verify (or run e2e)**

Give the user these exact commands and checks:

```bash
cd ~/utono/linux-lit && cargo build
cargo run
```

In the running app, on a two-column play (e.g. Cymbeline):
- Open the **gloss** overlay (Ctrl+g on a glossed line): the prose column reads
  as a softly-framed inset panel; the side gutters look like intentional matting;
  the frame is subtle (not boxy), no border/shadow.
- Open the **synopsis** overlay (`h` on a chapter): same soft frame (it renders
  through the gloss widget).
- Open the **journal** overlay (Ctrl+j): same soft frame.
- Confirm the accent bar, line numbers, and page marker (`⌄`/`•`) still paint ON
  TOP of the panel and the text is fully legible (contrast unaffected).
- Change the theme (super+\) to one LIGHT and one DARK theme and re-check each
  overlay: the panel stays barely-there and visible on both.
- Open the **translation** overlay (`i`): confirm it is UNCHANGED — two opaque
  columns, no transparent bleed-through, no panel.

The headless alternative (if it renders in the agent's env):

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

The clipping invariant must still pass (the panel must not shift or clip any
line). Then open `target/ui/*.png` and eyeball the frame per the UI review
protocol in CLAUDE.md.

- [ ] **Step 2: Record the outcome in `ac`**

Update `CLAUDE-activeContext.md` with the verification result (pass, or any
per-theme tuning follow-up if a specific theme reads too strong / invisible —
that is a one-line factor tweak in `overlay_panel_bg`, not a redesign).

---

### Task 8: Finish the branch

**Files:** none (integration).

- [ ] **Step 1: Confirm clean + tested**

Run: `cargo build && cargo test --bins && git status --short`
Expected: build ok, tests green, working tree clean.

- [ ] **Step 2: Merge, re-verify, push, delete (global "Finishing a Branch" rule)**

```bash
git checkout master
git merge --no-ff <feature-branch> -m "Merge branch '<feature-branch>'

feat(overlays): inset tinted panel framing for the prose overlays

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VLBThK3FZbWYM3ZbSHE7XY"
cargo build && cargo test --bins
git push origin master
git branch -d <feature-branch>
```

- [ ] **Step 3: Update `ac`** — new master HEAD, branch deleted, push state, and
  the on-screen-verification status from Task 7.

**Fallback note (only if Task 7 shows gutter bleed-through):** if the transparent
view lets the window root show through instead of the card cream, add an explicit
opaque cream bg to the ScrolledWindow in `generate_css` — e.g. a
`.overlay-prose-scroll { background-color: {gloss_bg}; }` class on the two
`gloss_scrolled` / `scrolled` ScrolledWindows — so the matting is guaranteed cream
beneath the transparent view. Do NOT pre-add this; only if the screenshot shows
bleed-through.

---

## Self-Review

**1. Spec coverage** (design → task):
- Problem (wide card, single prose column, empty gutters) → framing panel, all tasks.
- Decision: inset tinted rounded panel, ~3–5% shift, no border/shadow → Task 1
  (color), Task 3 (rounded rect, no stroke), Global Constraints.
- Scope: gloss + synopsis (via gloss) + journal; translation/reading-card/pickers
  out → Tasks 4–5 only; translation stays `.gloss-text` opaque (Task 2 + Task 7
  check).
- Architecture: DrawingArea below bar_drawing, reads live margins → Task 3–5.
- z-order correction → panel added as FIRST overlay (Tasks 4–5 Step 4).
- Opaque-TextView obstacle → `.overlay-prose` transparent class (Task 2), applied
  Tasks 4–5 Step 6. The `.gloss-text` collision with translation columns is
  RESOLVED with a distinct class (design's step-1 resolution), confirmed
  mandatory by `translation_overlay.rs:645`.
- Panel color derived + theme-responsive → Task 1 + Task 6.
- Cairo-not-CSS rationale → honored (Task 3).
- Components list (theme.rs / gloss / journal / shared helper) → Tasks 1–6.
- Testing (unit color delta + on-screen) → Task 1 Step 1 + Task 7.
- Risk: barely-there across ~40 themes → Task 1 bounded-delta test + Task 7
  light+dark on-screen check; per-theme tuning noted as a follow-up, not a redesign.

**2. Placeholder scan:** every code step shows real code; the only "placeholder"
value is the initial `panel_color` `(0.95,0.93,0.86)` tuple, immediately
overwritten by `set_panel_color` at startup (Task 6) — documented inline.

**3. Type consistency:** `set_panel_color(&self, hex: &str)` is defined
identically in Tasks 4 and 5 and called with `&theme.overlay_panel_bg` in Task 6;
`overlay_panel_bg` is a `String` field (Task 1) matching the `&str` param;
`draw_overlay_panel`'s signature in Task 3 matches its two call sites (Tasks 4–5).
The hex→rgb parser is the real local helper `parse_hex_color(hex) ->
Option<(f64,f64,f64)>` that `set_marker_color` already uses in both overlays
(verified in source) — no invented symbol.
