# Syntax diagram overlay

## Purpose

Draw a visual diagram of how a selected passage works grammatically: the text,
its parts of speech, and nested clause bands marking what each span *is* —
main clause, appositive, relative clause. A Cairo drawing, not a text card.

The reader selects a passage in visual mode and gets a picture of its
structure, with optional prose commentary on what that structure is doing
rhetorically.

## Prior art in the repo

Three existing pieces this builds on rather than reinvents:

- **Reader visual mode** (`src/input/visual.rs`) already selects line ranges
  and offers an extensible action popup (`BUILTIN_ACTIONS`).
- **`claude_bridge::run_claude_request`** already runs a Claude call off the
  GTK thread and dispatches back on the main loop, with a required
  loading-state contract and an error path.
- **`src/ui/keybinds_overlay.rs`** is a working `DrawingArea` + `set_draw_func`
  Cairo overlay whose structure (state in `Rc<Cell>`, `queue_draw` on change)
  this follows.

Upstream, litdb's `line_syntax` table (spec:
`~/utono/litdb/docs/superpowers/specs/2026-07-25-line-syntax-layer-design.md`)
stores a spaCy dependency parse per token per line. linux-lit does not read it
today; this feature is its first reader-side consumer.

## Why bands, and why Claude derives them

**Bands over arcs.** Dependency arcs are the native shape of `line_syntax`
(`head_i` *is* an arc), but they become unreadable past roughly fifteen words.
Prose selections are longer than that. Nested horizontal bands stay legible on
a full paragraph and read the way the passage reads — left to right, with
nesting shown as depth.

**Claude derives the bands, not `line_syntax`.** `line_syntax` covers 5 of 306
works (BH ×3 narrators, TT, Ham-Arkangel; 1.39M tokens against a 950,298-line
corpus). Computing bands from `head_i` in Rust would be instant and free, but
dark on 98% of the library until a corpus backfill runs.

Claude returning bands gives one code path and no coverage cliff. Where
`line_syntax` rows exist they are sent in the prompt so Claude anchors on the
real parse instead of guessing; where they do not, the text goes alone. The
parse is an ENRICHMENT, never a gate.

This also sidesteps a known limitation recorded in the litdb spec: both spaCy
models are trained on modern English and misparse archaic syntax (`said`/`asks`
came back as `advcl` on inverted dialogue tags). Claude sees the early modern
English directly.

**Cost accepted:** every invocation is an API round-trip, so the diagram takes
a second or more to appear. This is a stop-and-study tool, not a glance.

## Components

### 1. `src/syntax_diagram.rs` — data model

Pure, no GTK, unit-testable without a display.

```rust
pub struct Band {
    pub start_char: usize,   // offsets into the selection text
    pub end_char: usize,
    pub label: String,       // "main clause", "appositive", ...
    pub depth: u8,           // 0 = outermost
}

pub struct PosTag {
    pub start_char: usize,
    pub end_char: usize,
    pub pos: String,         // ADJ, VERB, NOUN, ...
}

pub struct SyntaxAnalysis {
    pub text: String,        // the selection, exactly as sent
    pub bands: Vec<Band>,
    pub pos: Vec<PosTag>,
    pub note: Option<String>,
}
```

Char offsets match the `line_syntax` convention (offsets into
`canonical_text`), so parse-derived and Claude-derived spans share one
coordinate space.

Responsibilities: deserialize Claude's JSON, validate spans, assign bands to
display rows by depth.

### 2. `src/ui/syntax_overlay.rs` — the Cairo surface

A `DrawingArea` in its own overlay layer, following `keybinds_overlay.rs`'s
structure.

**Full-screen, not card-bound.** The diagram fills the window rather than
sizing to the reading card. It is a study surface the reader stops on, not an
annotation beside the text, and the extra width and height are what let a long
prose selection stay legible — bands need horizontal room to hold their labels
and vertical room to stack.

This is exactly `keybinds_overlay`'s geometry, inherited wholesale:
`hexpand`/`vexpand` with `Align::Fill`, a scrim covering the whole surface, and
all drawing computed against the `widget_w`/`widget_h` passed to
`set_draw_func`. No `main_card_rect` involvement, so the diagram is unaffected
by column count, card margins, or whether a two-column play is open.

Two deliberate departures from that precedent:

- **Pango, not `cr.show_text`.** The diagram renders the work's own text —
  early modern English, italic stage directions. Cairo's toy text API has no
  shaping, wrapping, or font fallback. `pangocairo` is already a dependency.
- **Theme colors, not literals.** `keybinds_overlay` hardcodes its scrim
  (`set_source_rgba(0.341, 0.322, 0.475, 0.95)`) and so ignores the theme
  cycle. This is a reading surface: it reads `state.theme`, and band tints
  derive from the theme's accent ramp by depth. Reuse `theme.rs`'s existing
  contrast helpers so labels stay legible on every root variant.

Layout, top to bottom, within a centered content column capped at a maximum
width (the `panel_w = (widget_w - 2.0 * margin).min(1240.0)` pattern
`keybinds_overlay` already uses) so text does not run edge to edge on a wide
display:

1. The selection text, Pango-wrapped to the content width.
2. A POS row beneath the text.
3. Band rows stacked by depth — outermost lowest, so nesting reads as a stack.

A band spanning a line wrap breaks into segments, one per visual row.

Deep nesting is bounded by the window height rather than the card's. If the
stack still overflows, the diagram scales the row height down to fit; it never
clips or scrolls, so the whole structure is always visible at once.

### 3. Band derivation

`run_claude_request` supplies off-thread execution, main-loop dispatch, and the
error path. The system prompt lives in lit.db `api_prompts` under
`syntax.diagram` with a compiled fallback, following the `journal_qa_prompt`
pattern. It requests a strict JSON object matching `SyntaxAnalysis`.

When the work has `line_syntax` rows, the user message includes them as a token
table (word, POS, dep, head). When it does not, the text goes alone.

New: `src/db/syntax.rs` with
`load_line_syntax(conn, work_abbrev, line_range) -> Vec<Token>`.

### 4. Selection → both surfaces

- **Reader visual mode**: a new `"Syntax"` entry in `BUILTIN_ACTIONS`
  (`visual.rs:235`) plus its handler arm in `execute_action`. That file already
  warns the array and the `match` are coupled POSITIONALLY — both change
  together or an item fires the wrong action.
- **Overlay visual mode**: `BlockVisualCfg` gains a field so `GlossVisual` and
  `SynopsisVisual` fire it on a key. `handle_journal_visual_key` is a parallel
  function fixed to a different widget type, so it needs the same arm added
  separately.

Both converge on one entry point taking `(text, Option<Vec<Token>>)`.

Overlay selections are gloss/journal text with no `line_mapping` rows, so they
always take the text-only branch — which is why text-only had to be a
first-class path rather than a fallback.

### 5. Modal state

`InputMode::SyntaxDiagram`. Escape dismisses; a key toggles the commentary
(default hidden, mirroring the vocab popup's Definition/Gloss toggle). As a
modal reading surface it gets its own `Ctrl+/` legend, updated in the same
change as the binds per the project's keybind rules.

## Error handling

Three failure modes, each with a defined result:

- **API error or timeout** — `run_claude_request`'s `on_error` clears the
  loading state and toasts. The bridge contract requires showing a loading
  state before the call, so there is no stuck-spinner path.
- **Malformed JSON** — expected, not exceptional. `serde_json` into
  `SyntaxAnalysis`; on failure, toast and do not open. No partial diagram.
- **Invalid spans** — an offset past `text.len()`, or bands that partially
  overlap (nesting requires containment or disjointness). Validate before
  drawing: clamp to bounds, drop bands that neither nest nor sit disjoint. A
  dropped band loses information; a bad one draws garbage.

## Testing

`syntax_diagram.rs` is pure — JSON parsing, span validation, and depth-to-row
assignment are `cargo test --bins`, including fixtures for out-of-range and
partially-overlapping spans.

Drawing requires the cage/grim e2e. Per this project's rules the on-screen
check is mandatory regardless of any review waiver, and cage's software
rendering can disagree with the real GL renderer on layout, so the final
acceptance is on the user's renderer or a pixel-measured screenshot.

Two acceptance criteria, one per derivation path:

- **Enriched path** — a selection with a set-off modifier (BH-Barrett's
  "…in the dark room, irresolute, makes him start and say", the case that
  motivated `line_syntax`) draws `irresolute` as its own labelled band nested
  inside the main clause.
- **Text-only path** — the same check on a work with NO `line_syntax` rows
  (any of the other 301) draws a well-formed band stack. BH-Barrett is one of
  the five parsed works, so it exercises only the enriched path; both must be
  verified or the majority path ships untested.
- **Full-screen geometry** — the diagram fills the window, not the card. Verify
  on a two-column play, where a card-bound surface would visibly size to one
  column. Pixel-measure the scrim's extent rather than judging by eye, per the
  project's clipping rules.

## Non-goals

- **No caching.** Ephemeral like the vocab popup. Revisit if re-asking the same
  passage becomes common.
- **No band editing.**
- **No per-word dependency arcs.** Bands only.
- **No changes to `line_syntax` or litdb.** Read-only consumer.
- **No corpus backfill.** Coverage is deliberately not a prerequisite.
