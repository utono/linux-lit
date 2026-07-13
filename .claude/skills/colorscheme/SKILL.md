---
name: colorscheme
description: Use when assessing, editing, or creating linux-lit reader colors — phrase highlight, root/wallpaper color, search-match highlights, or the gloss/journal segment font colors — and you need to confirm sufficient contrast (WCAG) between a color and the card background, root, or body ink before it ships. Triggers on requests giving color values (e.g. "root #5c8da1", "vocab-fg #ffffff"), reports of unreadable/low-contrast highlights or gloss text, or building a new reader colorscheme.
argument-hint: assess|edit|create [--theme NAME] KEY #HEX [KEY #HEX ...]
---

# colorscheme

Assess / edit / create linux-lit reader colors with a **contrast gate that
reuses the app's own contrast math** (the `contrast_report_harness` test in
`src/theme.rs`), so the verdict matches what the app enforces at load via
`ensure_gloss_color_min`. No rendering — deterministic WCAG ratios.

## Colors (friendly KEY → theme meaning)

- `root` — outer wallpaper/border color
- `phrase` — cursor-segment / karaoke highlight tint
- `cursor-line` — current-line tint
- `gloss` — off-cursor gloss/journal-line **font** color
- `gloss-cursor` — gloss/journal line that is the cursor block (**font**)
- `vocab-fg` — vocabulary-word **font** color
- `search` — search-highlight (derived from `phrase`; not stored)

## Floors (mirrored from the app)

Foreground colors (`gloss`, `gloss-cursor`, `vocab-fg`, `search`) must clear:
**≥4.5:1 vs card bg**, **≥1.8:1 vs root**, **≥1.5:1 vs body ink**. A PASS means
the app won't silently rewrite the color. `root`/`phrase`/`cursor-line` get
lighter informational checks (they're surfaces/tints, not text).

## Workflow

Run the backing script (it invokes the Rust harness + does the safe JSON edit):

```bash
.claude/skills/colorscheme/color-assess.sh assess --theme NAME KEY #HEX [KEY #HEX ...]
```

- **assess** — read-only. Prints a per-pair report (`color vs surface: ratio /
  floor  PASS|FAIL`) + a summary; non-zero exit if any FAIL. Use to check a
  combo. `--theme` optional (defaults to `default`).
- **edit** — runs assess, then writes the pairs into `NAME`'s `linux-lit` block
  of `themes-unified.json`. **Refuses on any FAIL** unless `--force`. Backs up
  the file, touches only that theme's `linux-lit` block, validates JSON after.
- **create** — derive a full compatible set from a seed (e.g. only `root #hex`):
  YOU (the agent) pick the remaining colors so every pair passes `assess`,
  **report the derived scheme + its matrix to the user, get approval, THEN**
  run the script with `create --theme NAME --write ...` to persist. Never write
  a created scheme without showing it first.

Report the assess table and the file path (on write) back to the user.

## Deriving for `create`

Iterate: propose colors → `assess` → adjust any FAIL (raise contrast: darken a
font on a light bg / lighten on dark; move a tint's hue away from root) →
re-assess until the summary is PASS. Then report + await approval before
`--write`.

## Notes

- Pass the theme you're targeting — surfaces (bg/ink/root) resolve from it; a
  nonexistent name falls back to the dark default and won't match a light card.
- rgba tints are composited over the bg before measuring.
- `themes-unified.json` is shared (dwl/kitty/nvim read it too); the script edits
  only the `linux-lit` block and backs up first. Reader picks up changes on next
  launch, SIGUSR1, or Ctrl+t.
