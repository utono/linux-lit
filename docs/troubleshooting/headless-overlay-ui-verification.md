# Headless overlay UI/UX verification (no manual review needed)

How an agent (or CI) verifies overlay rendering and navigation **without the
user eyeballing the screen**. Built for the journal corpus-note Markdown
overhaul (2026-07-02), where the user asked for a way to test UI and UX
without manual reviews. Companion to `headless-testing.md` (main-card
nav-fuzz and launch-stack details) and `clip-prevention.md` (what clipping
looks like and why).

## TL;DR

- The agent CAN run the cage harness from its own shell. The
  tempdir-isolated harness (`tests/harness/mod.rs`) launches its own `cage`
  with a private `XDG_RUNTIME_DIR`; it does not touch the live dwl seat.
  Run everything through the env wrapper:

```bash
./scripts/e2e-env.sh cargo test --test journal_markdown -- --ignored --nocapture
```

- Three assertion channels, in increasing cost:
  1. **Dev-log assertions** (free, exact): the app logs semantic state under
     `LIT_HEADLESS_TEST`; tests parse the log instead of pixels.
  2. **Pixel invariants** (cheap, robust): python checkers over `grim`
     screenshots, scoped by rects the APP emits (never guessed).
  3. **Agent visual review** (last mile): every capture lands in
     `target/ui/`; the agent opens each PNG and reports what it sees.
     This replaces the user's eyeball for layout/spacing judgment calls.
- One keypress = one observable state change. If a test can't tell whether
  a press did something, the app must log enough that it can (that is an
  app change, not a test workaround).

## The rect/band contract (app → test)

Never hardcode geometry in a test; the app emits it at settle time (the
vadjustment's first `changed` signal after layout — the same event the
BottomClipGuard uses). Journal overlay emissions, from `show_page`:

- `TEST_JOURNAL_VIEWPORT_RECT x y w h` — the scrolled viewport in window ==
  screenshot coordinates. Used as `--region` for pixel checks.
- `TEST_JOURNAL_CONTENT_BAND x0 x1` — the horizontal band ALL text ink must
  stay inside: panel span = `left_margin − JOURNAL_BODY_INDENT − PANEL_PAD`
  through `w − right_margin + PANEL_PAD`. This is the guard for the
  "TextTag left-margin REPLACES the view margin" bug class, where a list or
  quote block escaped the centered column and rendered at the view's
  far-left edge.

The main card has the analogous `TEST_VIEWPORT_RECT`; the gloss/synopsis
overlay `TEST_OVERLAY_VIEWPORT_RECT`; the ask card
`TEST_JOURNAL_ASK_VIEWPORT_RECT`.

## Log-based navigation assertions (the phantom-press detector)

`mark_cursor_block` logs every accent-bar move:

```
JOURNAL-CURSOR: cursor#<page-local> full#<whole-entry> bar lines [s, e]
```

- **Always compare `full#`** — the whole-entry block index. `cursor#` is
  page-local and legitimately repeats across page turns (local #2 → #2 when
  the next page opens with two chrome blocks), which produced a false
  phantom-press failure in the first version of the test.
- The j-walk loop in `tests/journal_markdown.rs`: press `j`, wait, re-read
  the log. A NEW line with a CHANGED `full#` = the press moved. No new line
  = end of entry (legit only after several successful moves). A new line
  with the SAME `full#` = a swallowed/phantom press — the original bug
  (cursor unit disagreed with rendered blocks).
- `JOURNAL-PAGINATE: page_h=… heights=[…]` logs the measured block heights
  and budget — read it to verify fill arithmetic (page 0 at 540/542 proved
  the underfill fix without any screenshot).

## Pixel invariants

`scripts/check_ink_outside.py` (fails closed, needs numpy/pillow — the env
wrapper provides them):

- `--region x,y,w,h` + `--band x0,x1`: no ink column outside the band →
  catches styled blocks escaping the column.
- `--min-fill 0.5`: ink bottom must reach ≥ 50% of the region height →
  catches gross page underfill (apply to non-last pages only).
- Ink = pixels > 60 levels darker than the region's median. Assumes the
  light default theme (what the harness runs).

`scripts/check_line_clipping.py` (pre-existing): no half-cut first/last
line in a region. Harness wrappers: `assert_ink_within_band`,
`assert_no_line_clipping` in `tests/harness/mod.rs`.

## Agent visual review (the part that replaced the user)

Pixel checks catch invariant violations; they do not judge "the header has
too much space" or "the title is too big". For those, the agent:

1. Runs the e2e test (screenshots land in `target/ui/`, auto-named:
   `journal_md_page01.png`, `journal_md_walk04.png`, …).
2. Opens EVERY capture with the Read tool and reports what it sees —
   quoting on-screen text, calling out spacing/size/alignment problems.
3. Only after both the assertions AND the visual pass does it claim the
   change is verified.

This loop found the page-1 tail-line clip that all existing assertions
missed (page budget ignored the view's own 28+28px top/bottom padding) —
the screenshot showed it immediately.

## Gotchas that cost time (do not rediscover)

- **Premature wtype is dropped.** Wait for the viewport rect, then still
  `settle` ~400ms before the first key; use the `3`-then-`Ctrl+j` flow from
  `tests/journal_clipping.rs` when entering the journal.
- **A stale live instance shares the dev log.** The user's running dev
  build appends (e.g. `GAMEPAD:` retries with huge timestamps) into the
  same `linux-lit-dev.log` the test parses. Key on distinct prefixes and
  reset the log (`Harness::reset_dev_log`) right before the phase you
  assert on.
- **`Harness::reset_dev_log` truncates**, so rect waits never read a stale
  rect from a previous phase — reset between overlay opens if you need the
  NEXT emission specifically.
- **Both `cursor#` and `full#` exist for a reason** — see above; asserting
  on the page-local one WILL false-positive at page turns.
- **The uppercase-key trap:** `wtype -M shift -k a` delivers lowercase `a`
  with shift; overlay keymaps matching `A` need `type_text("A", …)`.
- **`annotate_ui.py` may warn** (`--app` required) — it is best-effort;
  the raw PNG is what matters.

## Adding coverage for a new overlay/surface

1. Emit a `TEST_<SURFACE>_VIEWPORT_RECT` (and a content band if the surface
   centers a column) from its settle path under `LIT_HEADLESS_TEST`.
2. Log semantic state transitions (`<SURFACE>-CURSOR`, `…-PAGINATE`) with
   whole-model indices, not view-local ones.
3. Add a `wait_for_…` + test in the `tests/` pattern of
   `journal_markdown.rs`: open surface → rect/band → per-page pixel checks
   → log-asserted key walk.
4. Screenshots go to `target/ui/` via `Harness::capture` so the agent's
   visual review sweep includes them.
