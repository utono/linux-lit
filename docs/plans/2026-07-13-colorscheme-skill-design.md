# `colorscheme` skill — reader color assess/edit/create with contrast testing

> Design/spec. A skill (not a code feature). Next: build the skill under
> `.claude/skills/colorscheme/` + its backing contrast harness, TDD the harness.

## Goal

A skill to assess, edit, and create reader colorschemes — phrase highlight,
root color, and search highlights — that **headlessly tests color compatibility
by computing WCAG contrast ratios**, using the app's OWN contrast machinery so
the verdict matches production. Ensures sufficient contrast, notably for the
font color of gloss/journal segments against the card, root, and body ink.

Accepts `key #hex` argument pairs (e.g. `root #5c8da1`, `vocab-fg #ffffff`).

## Managed colors (linux-lit theme keys)

Friendly key → `themes-unified.json` `linux-lit` key:

- `root` → `root_color` (outer wallpaper/border + the Root variant indicator)
- `phrase` → `phrase_highlight_bg` (cursor-segment / karaoke tint)
- `cursor-line` → `cursor_line_bg`
- `gloss` → `reader_gloss` (off-cursor gloss/journal-line font color)
- `gloss-cursor` → `reader_gloss_cursor` (gloss/journal line that is the cursor)
- `vocab-fg` → `vocab_fg`
- `search` → drives the search-highlight pair (derived from `phrase_highlight_bg`
  via `Theme::search_highlight_colors`)

Store: `~/utono/themes/.config/themes/themes-unified.json`, per-theme
`linux-lit` block (sparse overrides; absent keys derive in `theme.rs`).

## Architecture: contrast gate reuses the app's machinery

The contrast test is a **Rust harness compiled from the app's own `theme.rs`**,
so the verdict is byte-identical to production — no reimplemented WCAG math to
drift. It calls the real `relative_luminance` / `contrast_ratio` and the real
floor constants:

- `READER_GLOSS_MIN_CONTRAST` = 4.5 (gloss/journal font, vocab-fg vs card bg)
- `READER_GLOSS_ROOT_MIN_CONTRAST` = 1.8 (font vs root, hue-or-contrast)
- `READER_GLOSS_INK_MIN_CONTRAST` = 1.5 (font vs body ink)
- `VOCAB_WORD_MIN_CONTRAST` = 4.5, `VOCAB_POPUP_MIN_CONTRAST` = 7.0

No rendering — deterministic, fast. (The design deliberately chose compute-WCAG
over render+screenshot: faster, and it matches the exact rule the app enforces.)

### Contrast matrix (mirrors `ensure_gloss_color_min`)

Each FOREGROUND color (gloss font, gloss-cursor font, vocab-fg, search-highlight)
is checked against:

- card background — ≥ 4.5:1
- root — ≥ 1.8:1 (hue-OR-contrast: a far-enough hue also passes)
- body ink — ≥ 1.5:1
- "avoid" — not too close to sibling font colors

A PASS means the app will NOT silently rewrite the color at load via
`ensure_gloss_color_min`. The user's named case — gloss/journal segment font
color — is `reader_gloss` / `reader_gloss_cursor` vs bg, root, ink.

Translucent tints (`phrase_highlight_bg`, `cursor_line_bg`) are contrast-checked
against their composited-over-bg result (reuse `rgba_str_to_rgb` / the app blend).

## Modes

Invocation: `colorscheme <mode> [theme] <key #hex> [<key #hex> ...]`

- **assess** (read-only): compute the full contrast matrix for the given colors
  against the resolved theme's bg/ink/root. Report each pair PASS/FAIL with
  **ratio + floor**. For each color show the RAW verdict AND whether
  `ensure_gloss_color_min` would REWRITE it (so "my #ffffff got darkened" is
  visible). Non-zero exit if any FAIL. `[theme]` optional (defaults to a chosen
  reference theme).
- **edit** (write): run assess first; if all pass, write the pairs into
  `[theme]`'s `linux-lit` block. Refuse on FAIL unless `--force`. Report the
  file path + changed keys. `[theme]` required.
- **create** (generate): from a seed (e.g. just `root #5c8da1`), DERIVE a full
  compatible set (gloss/journal font, phrase/cursor tint, vocab-fg,
  search-highlight) so everything passes the matrix; honor any explicit pairs as
  constraints. **Derive → REPORT the full scheme + its matrix → wait for user
  approval → then write.** Never auto-writes. `[theme]` = the new theme name.

## Output

Compact per-pair report: `<color> vs <surface>: <ratio> / <floor> PASS|FAIL`,
a summary verdict, and (edit/create) the file path + changed keys. No raw JSON
dumped.

## Safety (shared JSON)

`themes-unified.json` is also read by dwl/kitty/nvim/firefox. The skill:

- touches ONLY the `linux-lit` block of the named theme,
- writes via parse → modify → serialize (never a blind text patch),
- backs up the file before writing, validates JSON after,
- reports the path + changed keys.

## Edge cases

- Unknown key → error listing valid keys. Malformed hex → error. Never write on
  a parse error.
- edit/create refuse on FAIL unless `--force`; `--force` still prints the failing
  pairs + notes the app will rewrite them at load.
- rgba vs hex both handled by the harness.

## Testing (TDD)

The backing harness is the testable core (mechanical → strongest test):

- RED: run the contrast harness on a KNOWN-BAD combo (e.g. a low-contrast gloss
  font on cream) → expect FAIL with the right ratio; on a KNOWN-GOOD theme →
  expect all PASS. Watch it produce the right verdicts before wiring the skill.
- The harness reuses the app's already-unit-tested `contrast_ratio` /
  `relative_luminance`, so the math is trusted; the harness test covers arg
  parsing, the matrix wiring, and the report format.
- Skill retrieval/application: run `assess` on a known theme (all PASS) and a
  bad combo (FAIL); confirm `edit` refuses on FAIL and writes on PASS; `create`
  derives + waits for approval.

## Files

- `.claude/skills/colorscheme/SKILL.md` — thin: name, "Use when…" description,
  argument-hint, the assess/edit/create workflow invoking the harness.
- `.claude/skills/colorscheme/color-assess.sh` — arg parse + JSON read/write
  (backup, parse→modify→serialize) + invokes the Rust contrast harness + formats
  the report.
- Rust contrast harness: least-invasive exposure of `theme.rs`'s contrast check
  to the script — likely a `cargo test`-runnable harness reading colors via env,
  or a small `src/bin/color_contrast.rs`. Chosen at build time favoring NOT
  adding production surface (prefer a test-only path).
