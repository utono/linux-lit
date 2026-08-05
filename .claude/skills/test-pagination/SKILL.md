---
name: test-pagination
description: Use when testing page-turn pagination for an author's works or a single work — runs headless pagination tests to verify pages fit the card with breathing room, never overflow, tile without text loss, and have no dangling speakers, orphaned stage directions, or split stanzas
argument-hint: <author-path> | <work-abbrev> | fit
---

# Test Pagination

Two layers, and they catch different bugs. Run BOTH before calling a
pagination change verified.

- **Pixel-fit e2e (cage)** — drives the real app and checks that pages
  actually FIT the card. Catches overflow and flush-to-the-edge pages.
- **Pure/synthetic suites (`cargo test`)** — line-count pagination and
  structural rules. Fast, no compositor.

## Layer 1: pixel fit, headless (start here)

**This is the layer that catches "text touches the bottom edge" and
"last line is half-cut."** The synthetic suites below CANNOT: they
paginate by line COUNT, while these bugs are about real GTK pixel
heights.

```bash
./scripts/e2e-env.sh cargo test --test prose_page_fit -- --ignored --nocapture
./scripts/e2e-env.sh cargo test --test prose_row_fill -- --ignored --nocapture
```

Run them ONE AT A TIME with a pause between — cage-backed binaries
contend over the compositor and a batch failure is not evidence until it
reproduces in isolation (see CLAUDE.md):

```bash
for t in prose_page_fit prose_row_fill; do
  ./scripts/e2e-env.sh cargo test --test $t -- --ignored --test-threads=1
  sleep 3
done
```

`prose_page_fit.rs` asserts:

- **No page overflows the card** — `total <= widget_h` on every rendered
  page, plus zero `CLIP_WARN ... prose-1col OVERFLOW` lines. An overflow
  floors the bottom clip to 0 and the last line renders unmasked.
- **Every page keeps bottom breathing room** — zero stored pages packed
  past the fill budget `usable`.

`prose_row_fill.rs` asserts stored pages TILE (page N's exclusive end ==
page N+1's start — no text lost between pages).

### Two things that make this suite pass vacuously

Both were hit while writing it; check them before trusting a green run.

**1. Geometry.** cage defaults to 1280x720, which gives a ~591px text
view and a completely different page grid — the bugs simply do not exist
there. `prose_page_fit` calls `set_output_size(1920, 1236)` and asserts
the resize took effect. **1236, not 1200**: only 1236 reproduces
production's text-view height. Never remove that resize to "simplify."

**2. Sampling.** A drive visits ~14 pages; a novel has ~760. The first
version of this test asserted only on the pages it drove through, passed
green, and missed all 34 over-budget pages in the same table. So the
breathing-room check reads the **whole-table census** the app logs at
generation time:

```
PAGES_PROSE_DRIFT: summary pages=761 over_usable=36 worst=page 103 at 1114px usable=1107 slack=14
```

Individually flagged pages (those past `usable + slack`, which will
actually floor the clip) get their own line:

```
PAGES_PROSE_DRIFT: page N (l,o)..(l,o) px=… > usable=… (+slack …) — stored anyway; render will log CLIP_WARN
```

`log_generation_height_drift` (`src/input/prose_pages.rs`) emits both;
it is debug-gated and does no GTK work, so it stays in permanently as a
tripwire. The census count varies by a page or two between runs at the
same geometry — treat a small delta as normal, a jump as a regression.

### Interpreting a fit failure

- `over_usable > 0` — pages are stored over the fill budget, so their
  last paragraph renders flush against the card's bottom rule. **Read the
  overshoot size first, it names the bug:** 1..=`pixels_below_lines + 2`
  px means the INK-vs-LINE-BOX grid mismatch (`clip-prevention.md` #2c);
  a whole-row overshoot means the fill decision itself. Fix the boundary
  decision, never the `prose_fit_slack` tolerance that hides it.
- **Trace one page's boundary walk** with
  `LIT_TRACE_BOUNDARY=<page-top-buffer-line>` — logs `BWALK:` lines
  (`ly`/`lh`/`first_row_top`/`total`/`used` per line, then `raw`,
  `snapped`, `row_fit`, `end`). This is the tool that settles which step
  overshoots; two plausible hypotheses were disproven by it before the
  real cause (2026-08-05). Get the page's top line from the
  `over page N (l,o)..(l,o)` census line.
- `CLIP_WARN ... OVERFLOW` on a table generated THIS run — not
  staleness. Generation and render compute algebraically identical sums
  (`page_px` vs `scroll::exact_page_content_height`), so a disagreement
  means the per-line HEIGHTS differed between the two moments.
- Deleting a work's `prose_pages` rows clears the symptom for that work
  only and it will come back. A fresh table is NOT proof a fit bug is
  fixed — see `docs/troubleshooting/clip-prevention.md` #12.

## Layer 2: synthetic suites

All Shakespeare works at 3 page sizes (25, 35, 45 lines per page):

```bash
cargo test headless_pagination -- --nocapture
```

Single page size:

```bash
cargo test shakespeare_pagination_35lpp -- --nocapture
```

All page-turn tests (navigation.rs suite — different from the viewport.rs
pagination suite above):

```bash
cargo test -- page_turn
```

The viewport.rs `headless_pagination_tests` module validates:

- **No dangling stage directions**: page top never starts with an exit stage direction without preceding speaker context
- **No dangling speakers**: page bottom never ends with a speaker name without dialogue
- **Page advancement**: every x press strictly advances page_top (no infinite loops)
- **Round-trip**: backward pass using forward page_tops produces the same validation results
- **Section breaks**: act/scene markers push content to next page without creating blank pages
- **Stanza atomicity**: verse stanzas (with optional stanza numbers) are never split across pages

The navigation.rs `page_turn_tests` module (run via `cargo test -- page_turn`):

- Forward progress, y round-trip, structural jump return, no mid-page scene breaks, cursor on dialogue, coverage (see `/test-play-navigation` for full list)

**These suites cannot see a pixel-fit bug.** They paginate by line count
against synthetic heights, so a page that overflows the real card, or
sits flush against the bottom rule, passes every one of them. That gap
is exactly what Layer 1 exists to close.

## Adding an Author

In `src/input/viewport.rs`, `headless_pagination_tests` module:

```rust
#[test]
fn chaucer_pagination_35lpp() {
    run_author_pagination("chaucer-geoffrey", 35);
}
```

## Adding a Single Work

```rust
#[test]
fn hamlet_pagination() {
    let path = "/home/mlj/utono/literature/shakespeare-william/folger-cleaned/hamlet.txt";
    if !std::path::Path::new(path).exists() { return; }
    let result = run_pagination_test(path, false, 35);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}
```

Second argument `is_prose`: `false` for plays/poetry, `true` for novels.

## Preserving a run's log

The cage harness writes to a tempdir deleted on Drop, so a failure (or a
suspicious pass) leaves nothing to read. `prose_page_fit` copies its log
out when told where:

```bash
PROSE_FIT_LOG_DIR=/tmp ./scripts/e2e-env.sh cargo test --test prose_page_fit -- --ignored
```

Then grep `PAGES_PROSE_DRIFT:`, `BOTTOM_CLIP_EXACT:`, `CLIP_WARN` in
`/tmp/prose-fit-run.log`.

## In-App GTK Test Harness

For real pixel-height testing with actual GTK layout in the live app, use
Ctrl+Shift+T. See `docs/troubleshooting/page-turning-mechanics.md`.

## Key Files

- `tests/prose_page_fit.rs` — pixel-fit e2e (overflow + breathing room)
- `tests/prose_row_fill.rs` — no-text-loss tiling e2e
- `src/input/prose_pages.rs` — `log_generation_height_drift`, `validate_prose_pages`, `page_px`, `prose_fit_slack`
- `src/input/navigation.rs` — `prose_next_boundary` (the boundary walk that spends the slack)
- `src/input/scroll.rs` — `exact_page_content_height`, `paged_bottom_clip`, `CLIP_WARN`
- `src/input/viewport.rs` — `headless_pagination_tests`, `trim_visible_range`, `clamp_at_section_break`
- `src/db/line_types.rs` — line classifiers

## When to Use

- After changing pagination logic in `viewport.rs` or `navigation.rs`
- After changing `prose_next_boundary`, `prose_fit_slack`, or the bottom-clip math
- After changing `back_up_for_speaker`, `clamp_at_section_break`, or `trim_block_atoms`
- After changing line classification in `line_types.rs`
- After a font, margin, or card-sizing change (these move the fill budget)
- When a screenshot shows text flush to the card's bottom edge, or a half-cut last line
- Before committing pagination changes
