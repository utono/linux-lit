# Theming: external theme changes + how overlays consume the theme

This guide is for agents working on linux-lit's visual styling. It explains:

1. how the app picks up a theme change made **externally** (the dwl `super+\`
   family of binds, `darkman`, the theme scripts), and
2. how overlays/pickers get their colors from `~/utono/themes/.config/themes`,
   so a new overlay's colors track the active theme **and** live-update when the
   theme changes.

**The one rule that makes both work:** put all overlay colors in
`theme::generate_css` using the theme tokens (`{bg}`, `{fg}`, `{dim}`, `{root}`,
…). Everything in that one stylesheet is regenerated and swapped on every theme
change — so a correctly-authored overlay is automatically dynamic. Hardcoded
colors (or a separate CssProvider) break this.

## The theme source of truth

- `~/utono/themes/.config/themes/themes-unified.json` — every theme, keyed by
  name. Each theme carries `kitty` (background/foreground → reader card colors),
  `dwl.rootcolor` (the desktop/wallpaper color) + `dwl.focuscolor`, and a `lit`
  section. `theme.rs::load_theme(name)` parses one theme into a `Theme` struct.
- `~/utono/themes/.config/themes/.current_theme` — a one-line file holding the
  active theme NAME. `theme.rs::current_theme_name()` reads it. The theme scripts
  write it (`set-theme.sh`); linux-lit also writes it from its own settings
  overlay (`apply_theme_to_state`).
- Paths are hardcoded in `theme.rs` (`themes_path()`, `current_theme_path()`).

## How an EXTERNAL theme change reaches the app (SIGUSR1)

The dwl theme binds and scripts do NOT talk to linux-lit directly — they signal
it. Full chain:

1. **dwl bind** (`super+\`, `super+shift+\`, etc.) runs a theme script:
   - `~/utono/themes/.config/themes/set-theme.sh <name>` — full theme switch:
     writes `.current_theme`, applies kitty/dwl/litecli/fzf/etc., then
     **`pkill -USR1 linux-lit`** (set-theme.sh:157).
   - the wallpaper/rootcolor cyclers (`cycle-wallpaper.sh`, dwl-mlj's
     `n.sh`/`n-palette.sh`) mutate `dwl.rootcolor` in `themes-unified.json` and
     likewise `pkill -USR1 linux-lit`.
2. **linux-lit catches SIGUSR1** in a tokio signal listener
   (`src/main.rs` ~line 90): `sig.recv()` → sends `MpvEvent::ThemeChanged` on the
   event channel.
3. The **event loop** handles it (`src/main.rs` ~line 404, `MpvEvent::ThemeChanged`):
   reads `current_theme_name()`, `load_theme(...)`, then calls
   `settings::apply_theme_to_state(&mut s, &theme)`.
4. **`apply_theme_to_state`** (`src/input/actions/settings.rs`) is the single
   re-theme entry point. It:
   - rebuilds the WHOLE stylesheet: `generate_css(theme, font, size)` →
     `state.css_provider.load_from_string(css)` (one provider, registered once at
     startup via `style_context_add_provider_for_display`), and
   - updates the handful of colors carried on `TextTag`s rather than CSS (dim,
     cursor-line, reader-gloss tint = `focus_color`, vocab, selection), and
   - stores `state.theme = theme.clone()` and writes `.current_theme`.

Because step 4 reloads the entire CSS, **any overlay styled through
`generate_css` recolors automatically** — no per-overlay re-theme code.

> To test the path without the dwl bind: `pkill -USR1 linux-lit` after editing
> `.current_theme`. (Do NOT do this against the user's live session unless asked
> — see the no-cargo-run rule.)

## How overlays/pickers consume the theme

All overlay CSS lives in **one** function: `theme::generate_css(theme, font,
size)`. It is a big `format!` whose args are theme-derived colors. The
substitution tokens you will use:

- `{bg}` = `theme.text_bg` — the reader/parchment card background.
- `{fg}` = `theme.text_fg` — body text.
- `{dim}` = `theme.dim_fg` — borders, muted text (a blend of fg/bg).
- `{root}` = `theme.root_color` — the dwl `rootcolor` (wallpaper/desktop color);
  used for scrims and full-bleed chrome (e.g. `.gloss-scrim`, toasts).
- `{gloss_bg}`, `{cursor_bg}`, `{cursor_fg}`, `{focus_ring}`,
  `{picker_selection_bg}`, `{header_border}`, `{toast_fg}` (= `contrast_on(root)`)
  — derived helpers; see the arg list at the bottom of `generate_css`.

### The reference pattern: the library picker (`Ctrl+p`)

`src/ui/library_picker.rs` adds CSS classes (`library-picker`,
`library-picker-header`, `library-picker-title`, `library-picker-footer`,
`library-picker-scrim`); the rules live in `generate_css`:

```
.library-picker      { background-color: {bg}; color: {fg};
                       border: 1px solid {dim}; box-shadow: …; }
.library-picker-title{ color: {fg}; opacity: 0.75; … }
.library-picker row:selected { background-color: {picker_selection_bg};
                               color: {cursor_fg}; }
.library-picker-scrim{ background-color: rgba(0,0,0,0.3); }  /* translucent dim */
```

This is why the Ctrl+p picker recolors live on `super+\`: its colors are
theme tokens in the shared provider.

### The keybind legends (gloss / synopsis / journal Ctrl+/)

The per-overlay Ctrl+/ legends follow the same pattern. The widgets
(`src/ui/{gloss,synopsis,journal}_keybinds_overlay.rs`) share
`ui::keybinds_legend::build_legend`, which applies classes
`legend-box` / `legend-title` / `legend-key` / `legend-action` and a
`legend-scrim`. Their rules in `generate_css` mirror the library picker:

```
.legend-box   { background-color: {bg}; color: {fg}; border: 1px solid {dim}; … }
.legend-title { color: {fg}; opacity: 0.75; … }
.legend-key   { color: {fg}; opacity: 0.85; }
.legend-action{ color: {fg}; opacity: 0.65; }
.legend-scrim { background-color: rgba(0,0,0,0.3); }
```

They therefore live-update with the theme exactly like the picker — no extra
wiring. The scrim is a **translucent** dim so the parent overlay shows through;
the OPAQUE `.gloss-scrim` ({root}) fully hides what's behind it (use that only
when full occlusion is intended).

## Checklist: adding a new overlay/picker that must track the theme

1. Add CSS classes to **`theme::generate_css`** using theme tokens
   (`{bg}`/`{fg}`/`{dim}`/…), copying the `library-picker` or `legend-*` block.
   Do NOT hardcode hex colors and do NOT create a second `CssProvider`.
2. In the widget, `add_css_class("your-class")` — no inline color setters.
3. If a color must live on a `TextTag` (not CSS), also set it in
   `apply_theme_to_state` so it re-applies on theme change (see the dim/cursor/
   reader-gloss tag updates there).
4. Verify: launch, `super+\` to switch theme, confirm the overlay recolors. If it
   doesn't, it's almost always (a) a hardcoded color, (b) CSS outside
   `generate_css`, or (c) a TextTag color not re-applied in
   `apply_theme_to_state`.

## Key files

- `src/theme.rs` — `Theme`, `load_theme`, `current_theme_name`, `generate_css`
  (the single stylesheet; token arg list at the end).
- `src/input/actions/settings.rs` — `apply_theme_to_state` (the one re-theme
  entry point: CSS reload + TextTag color updates + `.current_theme` write).
- `src/main.rs` — SIGUSR1 listener (~L90) → `MpvEvent::ThemeChanged` handler
  (~L404).
- `src/app/mod.rs` — startup CSS provider registration
  (`style_context_add_provider_for_display`).
- `~/utono/themes/.config/themes/` — `themes-unified.json`, `.current_theme`,
  `set-theme.sh` (the `pkill -USR1 linux-lit` site), `apply-dwl.sh`,
  `cycle-wallpaper.sh`.
