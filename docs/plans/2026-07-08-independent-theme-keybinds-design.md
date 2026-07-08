# Design: Independent theme + theme-cycle keybinds

Date: 2026-07-08
Status: approved

## Goal

linux-lit holds its own theme, fully independent of the system-wide theme
system. Default theme is `kindle-sepia`. New reader keybinds cycle through a
small curated list of reading themes. System-wide theme changes (dwl binds,
`set-theme.sh`, auto-switcher) no longer affect linux-lit.

## Background (current behavior)

- Theme is resolved from `~/utono/themes/.config/themes/themes-unified.json`
  keyed by the SHARED state file `.current_theme` (`src/theme.rs:40-51`),
  read at startup (`src/app/mod.rs:962`) and on SIGUSR1 (`src/main.rs:580`).
- `set-theme.sh` ends with `pkill -USR1 linux-lit`, so every system theme
  change rethemes the reader.
- `apply_theme_to_state` (`src/input/actions/settings.rs:266-316`) is the
  single re-theme chokepoint (settings overlay + SIGUSR1). It currently
  WRITES the shared `.current_theme` (`settings.rs:298-302`) — an in-app
  theme change clobbers system-wide theme state (bug, fixed by this design).
- No theme `Action` exists; the only in-app theme control is the settings
  overlay (`Ctrl+comma`, row 0 cycles all themes).
- `Config` (`src/config.rs`) has no theme field; it has atomic `save()`.

## Design

### Persistence (`src/config.rs`)

- New `Config` fields:
  - `theme: Option<String>` — absent means effective default `kindle-sepia`.
  - `theme_cycle: Option<Vec<String>>` — absent means compiled default:
    `["kindle-sepia", "kindle-green", "zenbones-light", "zenwritten-light"]`.
- Users customize the cycle list by editing `config.json`; no other list
  location (nothing added to themes-unified.json).

### Theme resolution

- Startup and the SIGUSR1 handler resolve the theme name from `Config`
  instead of `.current_theme`. linux-lit stops reading `.current_theme`
  entirely.
- SIGUSR1 semantics become "re-read my own config and re-apply" — external
  control remains possible: edit `config.json` (jq), then
  `kill -USR1 <linux-lit pid>`.
- Fallback chain: configured theme missing from JSON → `kindle-sepia` →
  existing `default_theme()`.

### Keybinds

- New actions `Action::ThemeNext` and `Action::ThemePrev`
  (`src/input/actions/mod.rs`).
- Default binds in `display_bindings()` (`src/input/keymap_config.rs`):
  - `Alt+t` → ThemeNext
  - `Alt+Shift+T` → ThemePrev
- Also added to the stowed override `~/tty-dotfiles/linux-lit/.config/
  linux-lit/keymap.json` (keymap.json overrides compiled defaults — both
  must be updated, per CLAUDE.md).
- Handler logic: locate `state.theme.name` in the cycle list; if not found
  (e.g. current theme was set to a non-list theme via the settings overlay),
  jump to the first entry. Load next/prev theme, route through
  `apply_theme_to_state`, persist `config.theme`. Feedback: reuse the same
  visual feedback the settings-overlay theme cycle produces (the retheme
  itself); no new toast mechanism is added.
- Update the `Ctrl+/` keybinds overlay via the `update-cairo-keybinds-overlay`
  skill, plus any affected legends.

### Chokepoint change (`src/input/actions/settings.rs`)

- `apply_theme_to_state` stops writing the shared `.current_theme`
  (`settings.rs:298-302`) and instead sets and saves `config.theme`.
- The settings overlay theme row keeps cycling all 44 themes, now persisting
  per-app only.

### Themes repo change

- `~/utono/themes/.config/themes/set-theme.sh`: remove the trailing
  `pkill -USR1 linux-lit` line.
- Update the pipeline description in the themes repo CLAUDE.md.

## Testing

- Unit test for cycle logic: next/prev, wraparound, current-not-in-list
  jumps to first entry, empty/absent list uses compiled default.
- `cargo build` + existing keymap tests (bind-count assertion) pass.
- Headless verification (per repo docs): edit `config.json` theme, send
  SIGUSR1, observe retheme; run `set-theme.sh <other-theme>` and confirm
  linux-lit does NOT change; restart app with no `theme` field in config and
  confirm kindle-sepia loads.

## Docs to update

- linux-lit `CLAUDE.md` (keymap section, theming notes).
- `docs/guides/theming-and-external-theme-changes.md` (independence, new
  SIGUSR1 semantics).
- themes repo `CLAUDE.md` (apply pipeline no longer signals linux-lit).

## Out of scope (YAGNI)

- No follow-system toggle mode.
- No reset-to-default keybind (cycle only).
- No per-theme "reading" flags or ordering metadata in themes-unified.json.
