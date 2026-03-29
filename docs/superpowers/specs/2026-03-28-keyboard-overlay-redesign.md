# Keyboard Overlay Redesign

Replace the current row-header list layout of the keybinds popup (Ctrl+/) with a visual representation of the Real Programmers Dvorak keyboard layout from `~/utono/rpd`.

## Layout

The overlay renders the full RPD keyboard as a physical keyboard mockup using GTK4 widgets (GtkBox, GtkGrid, Labels, CSS classes). No custom drawing.

### Rows

Each row is a horizontal box of key widgets with staggered left margins to simulate physical keyboard offset:

- **Number row** (13 character keys + Backspace): `$ + [ { ( & = ) } ] * ! |` then Backspace
- **Upper row** (Tab + 13 keys): `Tab ; , . p y f g c r l / @ \`
- **Home row** (Caps/Esc + 11 keys): `Esc a o e u i d h t n s -`
  - Caps Lock position renders as "Esc" (RPD maps Caps Lock to Escape)
- **Bottom row** (10 keys): `' q j k x b m w v z`
  - Shifted right so `'` starts slightly past the `a` key above it
- **Sequences row**: `gg` and `G` (multi-key sequences that don't map to a single physical key)
- **Arrow keys**: Inverted-T cluster on the far right
  - Up arrow on the same row as gg/G, centered above down arrow
  - Left/down/right on the row below

### Key widths

- Regular keys: standard width (uniform)
- Tab: wider than regular keys
- Caps Lock (Esc): wider than Tab
- Backspace: wider than regular keys
- Arrow keys: slightly smaller than regular keys

### Key rendering

Each key widget shows:

- **Shifted character** — small text, top-right corner (e.g., `~` on the `$` key, `P` on the `p` key)
- **Unshifted character** — larger bold text, left side
- **Action label** (for bare-key bindings) — small text at the bottom of the key
- **Shift action** (if the shifted variant has a different binding) — smaller text below the action, in the shift color (e.g., `P: nudge +0.2` below `nudge -0.2`)

### Modifier combos (Ctrl+, Alt+, Ctrl+Alt+)

Modifier bindings are **not** shown inline. They appear as **tooltips on hover**. When the user hovers a key that has modifier bindings, a tooltip appears above the key showing:

- Ctrl+ bindings in blue (e.g., `C-p -> picker`)
- Alt+ bindings in purple (e.g., `M-f -> font info`)
- Ctrl+Alt+ bindings in blue (e.g., `C-M-l -> save+quit`)

A key with modifier bindings but no bare-key action still shows the tooltip on hover.

### Color coding

Four states, each with a distinct background color:

- **Bound (bare key)** — green tint. Key has a bare-key binding.
- **Bound (shift only)** — blue tint. Only the shifted character has a binding (e.g., `V` for visual mode on the `v` key).
- **Both / has modifier** — diagonal split gradient (green/blue). Key has bare + shift bindings, or bare + modifier bindings.
- **Unbound** — dark gray, dimmed text. No bindings at all.

### Legend

A horizontal bar below the keyboard showing the four color swatches with labels. Also shows "Alt+" indicator in purple. Includes "Esc to close / C-/ to toggle" hint.

### Scaling

Preserve the existing Ctrl+Up/Ctrl+Down font scaling behavior so the user can resize the overlay.

## Architecture

### File changes

- **`src/ui/keybinds_overlay.rs`** — Complete rewrite of the data structures and widget construction. Replace the current flat `KeybindEntry` arrays and row-header layout with:
  - A `KeyDef` struct holding: unshifted char, shifted char, bare action, shift action, list of modifier bindings (modifier + action)
  - Const arrays for each physical row matching the RPD layout
  - Widget builder that creates key-shaped boxes with proper staggering, shifted chars, action labels, and CSS-driven tooltips
  - Arrow key cluster as a separate sub-layout
- **`src/ui/themes.rs`** (CSS) — Add/update CSS classes for key shapes, color states, tooltip positioning, hover behavior, and the keyboard background container

### Data model

```
struct KeyDef {
    unshifted: &'static str,      // "a", "[", "Tab", "Esc"
    shifted: &'static str,        // "A", "2", "", ""
    action: &'static str,         // bare-key action, "" if unbound
    shift_action: &'static str,   // shifted-key action, "" if none
    modifiers: &'static [(&'static str, &'static str)],  // [("C-p", "picker"), ("M-f", "font info")]
}
```

### CSS approach

- Key shapes: fixed width/height, rounded corners, flex column layout
- Color states: `.key-bound`, `.key-bound-shift`, `.key-bound-both`, `.key-unbound`
- Shifted char: absolute positioned top-right
- Tooltip: hidden child element, shown on `:hover` via CSS (no JS-like logic needed — GTK CSS supports hover)
- Row stagger: margin-left on each row's container box
- Arrow cluster: nested vertical/horizontal boxes

### What stays the same

- Toggle behavior (Ctrl+/ to show/hide, Esc to close)
- Overlay attachment mechanism (GTK Overlay on top of the text view)
- Font scaling with Ctrl+Up/Ctrl+Down
- The overlay is built once at startup, not rebuilt dynamically

## Keybind inventory

Complete list of bindings to render (from `keymap.rs`):

**Number row:**
- `+` toggle speed, `[` prev chapter, `{` next chapter, `*`/`0` reset font, `!` font minus, `|` font plus, Backspace delete timestamp

**Upper row:**
- Tab play/pause, `,` prev dialogue (C-, settings), `.` set chapter, `p` nudge -0.2 / `P` nudge +0.2 (C-p picker), `y` prev chunk, `f` font forward / `F` font backward (C-f pg fwd, M-f font info), `l` toggle signs (C-M-l save+quit), `/` search (C-/ keybinds)

**Home row:**
- Esc clear AB loop, `a` play from timestamp, `o` seek -3.5 / `O` seek -60, `e` seek +3.5 / `E` seek +60, `u` start time (C-u pg back), `i` set end time (M-i translations), `d` (C-d pg fwd), `n` next match / `N` prev match

**Bottom row:**
- `q` next dialogue, `j` cursor down, `k` cursor up, `x` next chunk, `b` (C-b pg back), `m` media picker, `V` visual mode

**Sequences:**
- `gg` go to start, `G` go to end

**Arrow keys:**
- Left delete timestamp, Right set start time, Up (C-Up vol up), Down (C-Down vol down)
