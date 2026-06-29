# rpd-keybinds — Design Spec

**Date:** 2026-03-31
**Status:** Approved

## Purpose

Standalone GTK4 Rust app that renders Cairo-drawn RPD keyboard layouts showing keybindings for multiple apps (linux-lit, lit, mpv, nvim-code, and others). Each app can have multiple drawings (e.g., normal mode, visual mode). The user cycles through drawings with keybinds.

## Repository

- **Path:** `~/utono/rpd-keybinds`
- **Remote:** Private repo on github.com/utono/rpd-keybinds

## Architecture: Hybrid

RPD physical keyboard layout defined as Rust constants (rows, key positions, sizes). Per-app keybind definitions loaded from TOML files at runtime. A generic renderer merges the two and draws with Cairo.

### Directory Structure

```
rpd-keybinds/
  Cargo.toml
  CLAUDE.md
  configs/
    linux-lit-normal.toml
    linux-lit-visual.toml
    mpv.toml
    nvim-code-normal.toml
    nvim-code-visual.toml
    lit-normal.toml
  src/
    main.rs          -- Entry point, GTK4 app setup
    config.rs        -- TOML parsing, keybind loading
    layout.rs        -- RPD physical layout constants
    renderer.rs      -- Cairo drawing, merges layout + keybinds
    app.rs           -- GTK4 window, DrawingArea, cycling logic
```

### Dependencies

- `gtk4` — windowing and DrawingArea
- `cairo-rs` — drawing (via gtk4)
- `toml` + `serde` — config parsing

## TOML Config Format

Each file defines one drawing. Key names match RPD unshifted characters. Special keys use GTK names (`Tab`, `Esc`, `Space`, `Up`, `Down`, `Left`, `Right`, `BackSpace`). Sequences like `gg` are recognized by length > 1.

```toml
[drawing]
name = "linux-lit normal"
app = "linux-lit"
order = 1

[keys]
a = { action = "play from ts" }
o = { action = "seek -3.5", shift = "O: -60" }
v = { shift = "V: visual mode" }
u = { action = "start time", modifiers = [["C-u", "pg back"]] }
f = { action = "font ->", shift = "F: <-", modifiers = [["C-f", "pg fwd"], ["M-f", "font info"]] }
slash = { action = "search", modifiers = [["C-/", "keybinds"]] }
Tab = { action = "play/pause" }
Esc = { action = "clear AB" }
Space = { action = "vocab popup" }
Up = { modifiers = [["C-Up", "vol +"]] }
gg = { action = "go to start" }
G = { shift = "go to end" }
```

Keys not mentioned in the TOML render as unbound.

## Physical Layout (layout.rs)

Same geometry as linux-lit's `keybinds_overlay.rs`:

- **Key sizes:** 68x66px standard, Tab 102px, Esc 120px, Backspace 94px, Arrows 54x50px
- **Gap:** 4px between keys
- **Corner radius:** 5px
- **Padding:** 20px outer

### Rows

- **Number row:** `$ + [ { ( & = ) } ] * ! |` + Backspace
- **Upper row:** Tab + `; , . p y f g c r l / @ \`
- **Home row:** Esc + `a o e u i d h t n s -`
- **Bottom row:** Shift + `' q j k x b m w v z`
- **Spacebar row:** Ctrl, Fn (below Shift), Win, Alt, Space (j-to-m width), Alt, Ctrl
- **Sequence row:** `gg`, `G`
- **Arrow keys:** Inverted T on far right

## Color Scheme: Rose Pine Dawn

Applied to both rpd-keybinds and linux-lit's Ctrl+/ overlay.

### Palette

- Base: `#faf4ed`, Surface: `#fffaf3`, Overlay: `#f2e9e1`
- Text: `#575279`, Subtle: `#797593`, Muted: `#9893a5`
- Pine: `#286983`, Gold: `#ea9d34`, Rose: `#d7827e`
- Foam: `#56949f`, Iris: `#907aa9`, Love: `#b4637a`

### Application

- **Overall background:** `#575279` (dark, so light keys pop)
- **Bound key background:** `#f2e9e1` with `#dfdad9` border
- **Unbound key background:** `#fffaf3` with `#f2e9e1` border
- **Key character (bound):** `#575279`
- **Key character (unbound):** `#9893a5`
- **Bare action label:** `#286983` (pine)
- **Shift action label:** `#907aa9` (iris)
- **Modifier tooltip text:** `#b4637a` (love)
- **Hovered key border:** `#d7827e` (rose)

### Legend

Displayed at the bottom of each drawing:
- Bound key swatch + "bare key" in pine
- Bound key swatch + "shift only" in iris
- Unbound key swatch + "unbound" in muted
- Bullet + "Ctrl/Alt modifier" in love
- Right-aligned: "Esc to close / j/k cycle" in subtle

## Window and Interaction

- Borderless GTK4 window, dark background (`#575279`)
- DrawingArea scaled to fill 92% of window width, centered
- Window title shows current drawing name
- Mouse hover shows modifier tooltips for keys that have them

### Keybinds

- `j` or `n` — next drawing
- `k` or `p` — previous drawing
- `q` or `Escape` — quit

### Cycle Order

Drawings sorted by `app` name alphabetically, then by `order` field within each app. Wraps around at both ends.

## Startup

1. Scan `configs/` for all `*.toml` files
2. Parse each into a `Drawing` struct
3. Sort by app name, then order
4. Display first drawing
5. Wait for key input to cycle or quit

## Scope Exclusions

- No QWERTY or other layout support (RPD only)
- No image export (GUI only)
- No runtime config editing
- No theme switching (Rose Pine Dawn only)
