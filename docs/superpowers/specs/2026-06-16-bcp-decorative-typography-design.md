# BCP decorative typography (reader-side rendering)

**Date:** 2026-06-16
**Status:** Design (pending implementation plan)
**Related:** `2026-06-15-bcp-echo-channel-split-design.md` (this repo);
`ws-book-of-common-prayer-references/docs/superpowers/specs/2026-06-16-bcp1559-modern-spelling-design.md`
(the sibling repo that produces the BCP data this renders).

## Goal

Render Book of Common Prayer works (`BCP*` abbrevs) in the reading view with the
liturgical typography of a printed prayer book — matching the Oxford World's
Classics edition (Brian Cummings, ed.): centered decorative headings, italic
hanging-indent rubrics, small-caps for divine names and collect openings, and
ornamental flourishes on rite titles.

This is **rendering only**. The text and its structural markers are produced by
the sibling `ws-book-of-common-prayer-references` repo and stored in lit.db
`line_mapping.canonical_text`. This repo styles what those markers enable; it
never modifies the stored text.

## The data contract (what lit.db gives us)

BCP lines arrive in `canonical_text` already carrying structural markers from
the sibling repo's `extract_blocks`:

- `## Heading text` — a heading (rite title, section header).
- `[rubric text]` — a rubric (stage direction / instruction), italic by
  tradition. May begin with a `¶` pilcrow inside the brackets.
- *plain line* — body text (prayers, responses, scripture sentences).
- Blank-line grouping (separate `line_in_div` rows) marks stanza/responsory
  breaks (e.g. the threefold Kyrie).

Editorial `°` footnote anchors are already stripped upstream — nothing to do.

## Scope boundary

**In scope (derivable from the markers + simple text patterns):**

1. Headings (`## `) — centered, bold/larger, extra vertical space.
2. Rubrics (`[…]`) — italic, with a centered vs. hanging-indent distinction.
3. Small-caps for divine names (`GOD`, `LORD`) and the opening word of a
   collect/prayer where the source presents it in caps.
4. Ornamental glyphs (`❧`) flanking top-level rite titles.

**Out of scope (defer or omit):**

- Pixel-perfect two-column justified text matching the print page. The reader's
  existing column layout (`apply_column_layout`) is reused as-is; we do not add
  print-grade justification or hyphenation.
- Drop-caps as raster/vector initials. (A small-caps or larger first letter is
  acceptable; true illuminated initials are not pursued.)
- Per-page divider rules between rites beyond existing heading spacing.

## How the reader renders today (current architecture)

The reading view uses GTK4 `sourceview5::View` over a **plain-text buffer**;
styling is applied **post-hoc** as Pango `TextTag`s keyed on line-type
predicates. No Pango markup is embedded in the buffer.

- Line-type predicates live in `src/db/line_types.rs`
  (`is_speaker`, `is_stage_direction`, `is_act_scene_marker`, `is_blank`, …).
- Tags are built and applied in `apply_dialogue_formatting()`
  (`src/app.rs`, ~3529–3711).
- Existing tags already prove every primitive this spec needs:
  - speaker names: `pango::Variant::SmallCaps` (small-caps works),
  - stage directions: `pango::Style::Italic`,
  - act/scene headings: bold + `pixels_above_lines`,
  - stanza numbers: `gtk4::Justification::Center`.
- `## ` is currently **stripped** during cleaning (`strip_prefix("## ")`) and
  styled as an act/scene header; `[…]` is matched by `is_stage_direction` and
  italicized.
- Theming/CSS for non-textview widgets is generated in `src/theme.rs`
  (`generate_css`, ~388–537). Decorative text styling is done via TextTags, not
  CSS.
- Columns: `apply_column_layout` (`src/app.rs` ~905) gives 1 or 2 columns; BCP
  reuses the default (2) unchanged.

**Consequence:** most of this spec is *new TextTags + new/extended predicates*,
not new rendering machinery. The hard parts (small-caps, centered, italic,
spacing) already exist.

## Components / changes

### 1. BCP detection — a render gate

Add a single predicate to decide whether the BCP typography path runs:

- `is_bcp_work(abbrev: &str) -> bool` → `abbrev.starts_with("BCP")`. The same
  `abbrev.starts_with("BCP")` test already exists inline at
  `src/input/actions/echoes.rs:131` (and as SQL `LIKE 'BCP%'` in
  `src/db/echo_channel.rs:31`); the plan may extract a shared helper or inline
  it — match the codebase's existing convention.
- In the formatting entry point, branch: when the current work is a BCP work,
  call a new `apply_bcp_formatting(state)`; otherwise the existing
  `apply_dialogue_formatting(state)` runs unchanged. Shakespeare is untouched.

### 2. Line-type predicates for BCP (`src/db/line_types.rs`)

- `is_rubric(text: &str) -> bool` → starts with `[` and ends with `]`. Distinct
  from `is_stage_direction` so BCP rubrics get the rubric styling, not the
  generic stage-direction style. (For BCP the two largely coincide; keeping a
  separate predicate lets the rubric centered/hanging logic live in one place.)
- `is_bcp_heading(text: &str) -> bool` → starts with `## ` (same marker, but the
  BCP path keeps the heading visible & centered rather than the act/scene
  treatment).
- `rubric_is_centered(inner: &str) -> bool` → heuristic separating the two
  rubric layouts seen in the Oxford text:
  - **Centered** (short transition/speaker cues): the de-bracketed, de-pilcrow'd
    inner text is short (≤ `RUBRIC_CENTER_MAX_WORDS`, default 8) AND has no
    sentence-internal period. Examples: "The Priest.", "The Answer.",
    "Then likewise he shall say.".
  - **Hanging indent** (instructional prose): everything else — the long
    ¶-led rubric paragraphs. Example: "At the beginning both of Morning
    Prayer…".
  - This is a display heuristic only; getting an edge case "wrong" misplaces a
    rubric's alignment, never its text.

### 3. BCP formatting pass (`apply_bcp_formatting` in `src/app.rs`)

Mirror `apply_dialogue_formatting`'s structure (iterate lines, apply tags). New
TextTags (built once, reused):

- `bcp-heading`: `Justification::Center`, `weight(700)`, modest `scale` (~1.1),
  `pixels_above_lines`/`below`. The `## ` prefix is stripped before the text is
  placed in the buffer (same mechanism as today).
- `bcp-rubric-hanging`: `Style::Italic`, left indent with negative first-line
  indent (hanging) via `indent`/`left_margin` on the tag; the `[`/`]` and inner
  `¶` are stripped for display, the `¶` re-rendered as a leading glyph.
- `bcp-rubric-centered`: `Style::Italic`, `Justification::Center`.
- `bcp-divine-name`: `Variant::SmallCaps`, applied to in-line spans matching
  whole-word `GOD`/`LORD` (and the all-caps opening word of a prayer). This is
  **word-level** tag application (compute byte offsets of the match within the
  line), the one place this spec needs sub-line tagging — the existing code
  applies tags to whole lines, so this is the genuinely new technique.

### 4. Ornaments on rite titles

For a top-level rite-title heading, render a flanking ornament: prepend/append
`❧` (U+2767) to the displayed heading text for BCP headings that are rite
titles. Two viable approaches — the plan picks one:

- **(a) Display-text injection**: when building buffer text for a BCP rite-title
  heading, wrap as `❧  <title>  ❧`. Simplest; the ornament is real buffer text
  (won't interfere with search if BCP search excludes heading lines, which it
  should). 
- **(b) Overlay/gutter glyph**: draw the ornament outside the buffer. More work,
  avoids buffer-text pollution.

Recommend (a) for the first cut; the ornament only decorates the small set of
rite-title headings.

## Theming

Decorative styles are TextTags (programmatic), consistent with how the reader
already styles speakers/stage directions — not CSS. Any color choices follow the
active theme's foreground/secondary colors from `src/theme.rs` so light/dark
both work; rubric color may use the existing secondary/dimmed token.

## Testing

Unit tests (Rust, alongside `line_types` tests):

- `is_rubric` / `is_bcp_heading` / `is_bcp_work` truth tables.
- `rubric_is_centered`: "The Priest." → centered; the long Morning-Prayer
  opening rubric → hanging; a ¶-led short cue → centered.
- Divine-name span finder: returns correct byte ranges for `GOD`/`LORD` as whole
  words, ignores `god`/`good`/`Lordes` partials.

Visual verification (the standing rule is the user runs `cargo run`; agent uses
the headless self-check path in CLAUDE.md): load `BCP1559M` Morning Prayer and
confirm centered titles, italic hanging rubrics, small-caps divine names against
the Oxford screenshots.

## Open questions for the plan

- Exact `RUBRIC_CENTER_MAX_WORDS` threshold (start 8, tune against real rites).
- Whether divine-name small-caps applies to `BCP1559M` (modern spelling) only,
  or also the original-spelling `BCP1559`/`1549`/`1662` (recommend: all BCP
  works — it keys on caps in the text, not on edition).
- Ornament approach (a) vs (b).
