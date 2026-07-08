---
name: edit-theme-cycle
description: Use when adding, removing, or reordering themes in linux-lit's Alt+t theme cycle, or setting the reader's active theme — edits the theme / theme_cycle fields in the app's own config.json (independent of the system-wide theme)
argument-hint: add <theme> | remove <theme> | set <theme> | list (no args = usage + current state)
---

# Edit Theme Cycle

linux-lit themes itself from its OWN config (independent of the system theme
since e64f8d2). Two fields, both optional:

- `theme` — the active theme. Absent/null → `kindle-sepia` (`DEFAULT_THEME`,
  `src/config.rs`).
- `theme_cycle` — the ordered list Alt+t / Alt+Shift+T walk. Absent/null →
  compiled default: `kindle-sepia`, `kindle-green`, `zenbones-light`,
  `zenwritten-light`.

Theme names must be keys of
`~/utono/themes/.config/themes/themes-unified.json` (color palettes are still
read from there; only the *selection* is per-app). An unknown name silently
falls back to the built-in default theme, so always validate first:

```bash
jq -e 'has("THEME")' ~/utono/themes/.config/themes/themes-unified.json
```

List all valid names: `jq -r 'keys[]' .../themes-unified.json`.

## Two config files

- `~/.config/linux-lit/config.json` — release build
- `~/.config/linux-lit/config-dev.json` — dev build (`cargo run` / `crll`)

Default: apply the change to BOTH files so dev and release stay in sync,
unless the user names one. The user normally runs the dev build (`crll`).

## No argument

Given no argument, do NOT edit anything. Show usage (the four subcommands
with one-line descriptions), then print the current state of each config
file that exists:

```bash
for f in ~/.config/linux-lit/config.json ~/.config/linux-lit/config-dev.json; do
  [ -f "$f" ] && echo "== ${f:t}" && jq '{theme, theme_cycle}' "$f"
done
```

Render the result as a short bulleted report per file — active `theme`, then
the cycle themes in order, spelling out the compiled default list when
`theme_cycle` is null/absent (and saying it's the default).

## Steps

1. Validate every theme name against `themes-unified.json` (jq `has()`). On
   failure, show close matches from `keys[]` and stop.
2. For each target config file:
   - `add <theme>` / `remove <theme>`: if `theme_cycle` is null or absent,
     materialize the compiled default list first, then append/remove.
     Removing the active `theme` from the cycle is fine — `theme` is a
     separate field.
   - `set <theme>`: write the `theme` field (the next Alt+t continues from
     that theme if it's in the cycle, else from the cycle's start).
   - `list`: report `theme` + effective cycle (spell out the default when
     null) per file; skip the edit steps.
3. Edit with jq to a temp file, then `\cp -f tmp config` (never `mv` — the
   shell aliases prompt).
4. **If an instance is running** (`pgrep -x linux-lit`), signal it so the
   edit takes effect AND survives exit: `kill -USR1 $(pgrep -x linux-lit)`.
   A running instance rewrites its config on exit — an unsignaled edit gets
   clobbered. No instance running → nothing to signal.
5. Report the resulting `theme` / `theme_cycle` per file. No `cargo build`
   needed — this is pure config.

## Gotchas

- Never write `.current_theme` or run `set-theme.sh` — the reader no longer
  reads or writes the system theme state.
- `theme_cycle: null` and a missing key both mean "compiled default"; keep
  whichever form the file already has when not editing that field.
- Alt+t persists the new theme to the config on each cycle, so the file may
  legitimately change between your read and write — re-read before editing.
