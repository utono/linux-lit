# Testing Pinned Play Pagination

How page-navigation testing works once the pinned play page tables land
(design: `docs/plans/2026-07-04-pinned-play-pagination-design.md`; plan:
`docs/plans/2026-07-04-pinned-play-pagination.md`). The short version:
testing moves below the GUI — the slow headless fuzz becomes the last line of
defense instead of the primary proof.

## Tier 1 — data-level audits (no app, no display)

Once pages are rows in lit.db (`play_pages` / `play_pages_meta`, keyed by
`line_mapping` citation ids + a layout fingerprint), most of what the
nav-fuzz used to prove by driving the GUI for ~330 seconds per work becomes a
data audit that runs in seconds for every play at once:

- full coverage — every dialogue line inside exactly one page interval
- monotone, non-overlapping, gap-free page intervals
- tail reachability — the last page reaches the work's last dialogue line
- sane boundaries — `left_start ≤ split ≤ end`, empty-right (watermark)
  pages only where the next page opens a real `(div1,div2)` section
- row/meta consistency (page counts, validated flag)

Run it with the skill (read-only; never writes lit.db):

```bash
.claude/skills/validate-play-pages/validate-play-pages.sh --all
```

```bash
.claude/skills/validate-play-pages/validate-play-pages.sh MND
```

Run it after any lit.db re-import, font/layout change, or suspected
pagination drift. FAIL means delete that work's rows (the script prints the
command) and let the app regenerate on next load. Staleness against the
current text (`db_fingerprint`) and the geometric fit/determinism invariants
are enforced by the app itself at load/generation time — a stale table logs
`PAGES: fallback (...)` and is replaced on the next load.

## Tier 2 — pure unit and property tests (no GTK)

Navigation over the table is index arithmetic, so the properties that
historically only the fuzz could check become millisecond `cargo test`
targets in `src/input/page_table.rs`:

- the invariant suite itself (`validate_spreads`) — coverage, tail, fit,
  watermark sanity, each with a failing-case test
- `page_for_line` binary search — every line findable, containment exact
- fingerprint composition — deterministic, sensitive to every layout input
- round-trip properties — `x` then `y` returns to the same page; `G` is
  idempotent; cursor-landing rules pick an on-page line

```bash
cargo test --bin linux-lit page_table
```

## Tier 3 — headless e2e (the watchdog)

The cage + grim + wtype fuzz stays, with two additions:

- a new assertion: the `PAGES: page N/M` log line must move by exactly ±1 on
  `x`/`y` (or pin at the first/last page)
- two env flags select the engine under test:
  `LIT_GEN_PAGE_TABLE=1` forces table generation at the current (headless)
  geometry so the fuzz exercises the TABLE path;
  `LIT_NO_PAGE_TABLE=1` forces the LIVE engine so the fallback path (font
  changes, translations, scroll mode, other machines) keeps its own coverage.

```bash
LIT_GEN_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
```

```bash
LIT_NO_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
```

Pixel-level clip checks are unchanged (`tests/line_clipping.rs`,
`tests/overlay_clipping.rs` via `./scripts/e2e-env.sh cargo test --test
line_clipping --test overlay_clipping -- --ignored --nocapture`).

## Headless runs at PRODUCTION geometry

Cage's virtual output defaults to **1280×720** (the wlroots headless
backend's built-in mode; cage has no size flag). A 720p run paginates
differently than the real 1920×1200 session — historically this meant
headless tests could only approximate production pagination, and with page
tables it would mean exercising only the fallback engine (the fingerprint
wouldn't match).

Cage implements the wlr-output-management protocol, so the output can be
resized live (verified 2026-07-04 — the app re-lays-out and `grim` captures
at the new size):

```bash
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

Issue it after the cage launch (then give the app a few seconds to
re-paginate) whenever the acceptance criterion depends on production
geometry: page-table generation/consumption, spread boundaries, spread
balance. This closes the gap between what headless tests exercise and what
the user actually reads with.

## What each tier catches

- unreachable tail, non-idempotent `G`, page gaps/overlaps → Tier 1
  (instantly, all plays) and Tier 2 (per property)
- navigation landing/cursor rules, engine selection, fallback behavior →
  Tier 2 + Tier 3
- rendering, clipping, focus/input, reveal timing → Tier 3 only (pixels)

Generated test tables carry a headless fingerprint key, so they can never
clobber the production rows; clean them up after a run with:

```bash
sqlite3 ~/utono/litdb/data/lit.db "DELETE FROM play_pages WHERE layout_fingerprint LIKE '%|1280x720|%'; DELETE FROM play_pages_meta WHERE layout_fingerprint LIKE '%|1280x720|%';"
```
