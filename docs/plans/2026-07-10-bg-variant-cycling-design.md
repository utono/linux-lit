# Ctrl+t Background-Variant Cycling — Design

Date: 2026-07-10
Status: approved (brainstormed with user)

## Problem

The user wants, for every theme, three background colors — each a variant of
that theme's background color family — cycled with Ctrl+t. The karaoke
(phrase-highlight) tint must follow the background so the highlight always
sits in the same color family as the page. Today the only way to get a
lighter background is to author a whole sibling theme (kindle-sepia →
sepia-light → sepia-lightest were built by hand this way).

## Decisions (from brainstorming)

- Variants are **computed automatically** by default, stepping **toward
  white** (the sepia ladder shape), with an optional **hand-authored
  override** per theme (needed because sepia-lightest's second variant is a
  cool grey `#f0f0f0`, not a lighter warm cream — sampled from the user's
  kitty prompt-highlight band, which reads light-blue against warm chrome).
- The chosen variant **persists per theme** in the app config.
- Variant 0 is always the theme's designed background, untouched.

## Variant model

Each theme has exactly 3 variants, index 0–2:

- **Index 0** — the designed background; resolution is byte-identical to
  today's behavior.
- **Index 1–2, hand-authored** — read from
  `<theme>."linux-lit".bg_variants` in `themes-unified.json`: an array of up
  to 2 objects:

```json
"bg_variants": [
  { "bg": "#f0f0f0",
    "phrase_highlight_bg": "rgba(69, 89, 100, 0.14)",
    "cursor_line_bg": "rgba(69, 89, 100, 0.12)" }
]
```

  Only `bg` is required; omitted tints fall back to the computed rule below.
  If the array has 1 entry, variant 2 is computed.
- **Index 1–2, computed** — `bg` blended toward `#ffffff` at 65% (variant 1)
  and 90% (variant 2). These ratios approximate the approved sepia ladder
  (`#e7dec7 → #f7f2e2 → #fdfbf2`) — the hand-picked hexes are not exact
  uniform blends, and exact reproduction is not a goal. Alpha-based tints keep their hue and
  scale alpha: `cursor_line_bg` and `phrase_highlight_bg` at ×0.7
  (variant 1) and ×0.5 (variant 2) of the theme's value.

### Application point: substitute before derivation

The variant is applied **inside theme resolution**, not as a post-hoc patch:
`resolve_theme` produces the base values, then a pure function
`apply_bg_variant(theme_json, base, idx) -> Theme` substitutes `text_bg`
(and the two explicit tints when authored) and re-runs the existing
derivation pipeline against the new background:

- `dim_fg`, `sign_fg` — fg/bg blends, re-derived.
- `overlay_panel_bg`, `scrim_bg` — re-derived.
- `reader_gloss`, `reader_gloss_cursor` — contrast guards re-run against the
  new bg (guards already exist; no new logic).
- `phrase_highlight_bg` (karaoke) — authored value, else alpha-scaled from
  the theme's value. This satisfies "karaoke matches the background family":
  a cool variant carries a cool tint, warm carries warm.

`root_color`, `focus_color`, `text_fg`, `cursor_bg/fg`, `vocab_fg` are
unchanged by variants (foreground family is stable across variants).

## Persistence

`config.json` / `config-dev.json` gain one field:

```json
"bg_variants": { "sepia-lightest": 1, "kindle-green": 2 }
```

- Map of theme name → variant index (0–2). Absent theme or absent map = 0.
- Written through the existing config save path (merge-on-save aware:
  the field must be marked dirty when changed, per the multi-instance
  config rules).
- Alt+t theme switches look up the NEW theme's saved index and apply it —
  each theme remembers its own variant.
- SIGUSR1 config reload applies the stored index like any other setting.

## Keybind and dispatch

- New `Action::BgVariantNext` — cycles the active theme's index 0→1→2→0.
- Bound to **Ctrl+t** (currently unbound in the reader) in BOTH
  `src/input/keymap_config.rs` defaults and the stowed
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.
- Handler lives with the other theme actions in
  `src/input/actions/settings.rs`: compute the new `Theme` via
  `apply_bg_variant`, funnel through the existing `apply_theme_to_state`
  (repaints CSS, tags, overlays), persist the index, toast
  `Background: base` / `lighter` / `lightest` (or `variant 1/2` for
  hand-authored ones).
- `apply_theme_to_state` must NOT reset the variant when called from Alt+t —
  it applies the target theme's saved variant instead.

## Seed data (themes-unified.json)

- `sepia-lightest.linux-lit.bg_variants` =
  `[{ bg: "#f0f0f0", phrase_highlight_bg: "rgba(69, 89, 100, 0.14)",
  cursor_line_bg: "rgba(69, 89, 100, 0.12)" }]` — the exact screenshot
  color with cool slate tints. Variant 2 stays computed.
- No other theme gets seed data; all others use the computed ladder.

## UI mirrors to update

- Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs`): add Ctrl+t to the
  right `KeyDef` row + a `describe()` arm — run the
  `update-cairo-keybinds-overlay` skill's three-pass cross-reference.

## Testing

- Unit tests (pure, no GTK) in `theme.rs`:
  - blend-toward-white math hits the sepia ladder values;
  - alpha scaling of rgba strings;
  - `bg_variants` JSON parsing (0, 1, 2 entries; missing tints; malformed
    entries skipped);
  - index 0 returns a `Theme` identical to the un-varied resolution;
  - contrast guards re-run (gloss tint differs between variant 0 and a
    hand-authored cool variant).
- Config round-trip test for the `bg_variants` map (absent = empty).
- Headless cage screenshot of sepia-lightest at variant 1 as visual
  acceptance: cool `#f0f0f0` page with cool karaoke tint during playback
  sync (or with a phrase tag forced visible).

## Out of scope

- Removing the hand-built sepia-light / sepia-lightest themes from the
  Alt+t cycle (kindle-sepia + Ctrl+t roughly reproduces them; user decides
  later).
- Reverse-cycling bind (Ctrl+Shift+T) — YAGNI until asked.
- System-wide (kitty/nvim/firefox) variant support — this is a linux-lit
  reader feature only.
