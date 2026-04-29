---
name: set-text-margins
description: Use when adjusting the left margin, verse left offset, or prose left offset for the text card layout — controls how far from the card edge text begins
argument-hint: <constant> <value> | show
---

# Set Text Margins

Adjusts the text layout margins that control horizontal positioning of content within the card.

## No arguments or `show`

Print current values and their effects:

1. Read the constants from `src/config.rs` and `src/app.rs`
2. Print a summary like:

```
Text layout margins:
  DEFAULT_TEXT_MARGINS = 40    (src/config.rs:83)  — base left/right padding inside card for all works
  EXTRA_RIGHT_MARGIN   = 28   (src/config.rs:84)  — additional right padding (prose only; plays use line number gutter)
  VERSE_LEFT_OFFSET    = 200  (src/app.rs:325)     — extra left indent for plays/verse in monocle mode (0 in tiled)
  PROSE_LEFT_OFFSET    = 120  (src/app.rs:326)     — extra left indent for prose in monocle mode (0 in tiled)

Total left margin in monocle:
  Plays/verse: DEFAULT_TEXT_MARGINS + VERSE_LEFT_OFFSET = 240px
  Prose:       DEFAULT_TEXT_MARGINS + PROSE_LEFT_OFFSET = 160px

Total left margin in tiled mode:
  All works:   DEFAULT_TEXT_MARGINS = 40px (offsets are disabled)
```

Read the actual values from source before printing — do not use hardcoded numbers.

## With arguments: `<constant> <value>`

Change a specific constant. Accepted constant names (case-insensitive):

- `text-margins` or `margins` — changes `DEFAULT_TEXT_MARGINS` in `src/config.rs`
- `verse-offset` or `verse` — changes `VERSE_LEFT_OFFSET` in `src/app.rs`
- `prose-offset` or `prose` — changes `PROSE_LEFT_OFFSET` in `src/app.rs`
- `right-margin` or `right` — changes `EXTRA_RIGHT_MARGIN` in `src/config.rs`

Steps:

1. Parse constant name and new value from arguments
2. Edit the constant in the appropriate file using `Edit` with `replace_all: false`
3. Run `cargo build` to verify
4. Print the old and new values
