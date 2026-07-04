# Two-Column Last-Line Descender Clip Fix — Implementation Plan

> **STATUS: IMPLEMENTED 2026-07-04** (commits `7f328bb` helpers,
> `cc9820b` fix + gate + diagnostics + detector calibration; docs in
> `clip-prevention.md` checklist #10). Two post-plan discoveries shaped the
> final code — see the amendments inline: the allowance is capped by the
> BOUNDARY line's own blank strip (`boundary_blank_budget`: tag scale,
> tag `pixels_above_lines`, blank-line box cap), and it collapses to 0 while
> the boundary line carries the cursor-highlight band (gated via
> `left_clip_boundary`/`right_clip_boundary` + an `update_highlight` hook).
> Headless verification used the new `LIT_DEBUG_CLIP_COLOR` env knob instead
> of hand-painting theme.rs. The `line_clipping` e2e needed two detector
> calibrations (merge ≤2px row gaps; edge row must also be shorter than every
> interior row) — both false positives exposed by the fix rendering MORE ink.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the card-colored bottom-clip box from slicing the glyph descenders (g/y/p/comma tails) off the **last line of each column** in a two-column (verse) spread on the main reading card.

**Architecture:** The two-column bottom clip runs through the `exact_end` branch of `update_bottom_clip` (`src/input/scroll.rs`). That branch sums each visible line's *logical* height via `line_yrange` and sets the clip to `widget_height - total`, so the clip's top edge lands exactly at the last line's **logical bottom** — and the descender ink renders flush to (or a px past) that logical bottom, so the clip slices it. The fix subtracts a small descender allowance from the clip so its top edge sits *below* the descenders.

**AMENDED 2026-07-04 (pre-implementation review + pixel measurement).** Two corrections to the original plan, both incorporated in the tasks below:

1. **The allowance must be capped at `pixels_above_lines`, NOT the full `descender_guard_px`.** The original safety argument ("there is no following line rendered in this column") is wrong: both column views render the *shared buffer*, so the next line (`end` / the next spread's top) IS rendered immediately below the last line's logical bottom — it is merely hidden by the clip. Pixel measurement on the failing MND spread shows the next line's ink begins only ~5px below the clip's top edge (the next line's `pixels_above_lines` spacing); a full `descender_guard_px` allowance (4–24px) would reveal its ascender tops as a ghost sliver. The blank window that is *guaranteed* ink-free is the next line's `pixels_above_lines` (ink cannot start above `box_top + pixels_above_lines`), so the allowance is `descender_guard_px(...).min(view.pixels_above_lines().max(0))`. With the default `line_spacing` 5–6 this yields 5–6px — measured to be exactly the descender overhang plus margin — and degrades safely to 0 (today's behavior) if a user sets `line_spacing: 0`.
2. **Broadened to the non-`exact_end` paged branch too.** The single-column/prose final clip (`clip = widget_height - total_height + scroll_offset`) has the identical structure — top edge at the summed logical bottom — so the same allowance is applied there. The visual-row paths are deliberately NOT touched: the over-tall `count == 0` branch, scroll-mode (`scrolloff_bottom_clip_widgets`), and the overlays' `display_rows`/`bottom_clip_height` all clip at *wrapped visual-row* boundaries, where rows tile with zero inter-row spacing — there is no blank budget to reveal, and lowering those clips would immediately expose the next row's ink. (Their per-row rects already include the full logical row, which is why the overlays have not shown this bug since the clip unification.)

**Tech Stack:** Rust, GTK4 / sourceview5, `cargo test` (pure unit tests), headless `cage` + `grim` for pixel verification.

## Global Constraints

- Do NOT run the app with `cargo run` — the user launches it (`crll`). Agent verification uses the headless `cage` + `grim` flow only (see `CLAUDE.md` → *Headless Verification*).
- The fix touches ONLY the two `line_yrange`-summing clip computations in `update_bottom_clip` (the `exact_end` branch and the final non-`exact_end` clip). Do not alter the `count == 0` over-tall branch, scroll-mode, the overlay helpers, or `column_split`'s line-fitting math (see the amendment above for why the visual-row paths must not get an allowance).
- Any new arithmetic goes in a **pure** helper (no GTK types) so it is unit-tested without a display, mirroring the existing `overtall_clip` helper and its `overtall_clip_tests` module in `src/input/scroll.rs`.
- Read `docs/troubleshooting/clip-prevention.md` before editing — this is the MAIN reading card's *paged* clip (checklist #2 "uniform/logical geometry cuts descenders", in the two-column `exact_end` guise), NOT a free-scroll overlay clip.

---

## Background: the confirmed diagnosis (do not re-investigate)

Reproduced headlessly on `MND` (A Midsummer Night's Dream), two-column verse, with the clip box painted red (`.card-bottom { background-color: #ff0000 }`) for one run:

- Right column last line "To what, my love, shall I compare thine eyne?": the 'y' in "my" and 'p' in "compare" are sliced by the red band's TOP edge. The band starts at the line's logical bottom.
- Below the band there is ~52px of red = `descender_guard (~12px) + BASE_BOTTOM_MARGIN (40px)` — proving the *reserve* exists; the clip's **top edge** is simply too high by the descender overhang.
- Same on the left column's last line.

The `exact_end` branch (`src/input/scroll.rs`, currently ~lines 751-776) sets `clip = (widget_height - total).max(0)` where `total = Σ line_yrange(i).1` for `i in page_top..=last`. `line_yrange` returns the logical line box, whose bottom excludes the part of the glyph that hangs below the baseline+logical-descent. So the clip covers those pixels.

The fix: subtract the descender allowance from the clip.
`clip = (widget_height - total - descender_allowance).max(0)`.
`descender_allowance = descender_guard_px(text_view, page_top).min(text_view.pixels_above_lines().max(0))` — the font's clamped descent, capped at the inter-line spacing above the next line's ink (see amendment #1; the uncapped guard would reveal the next line's ascenders, since the shared buffer renders the next line right below the clip's top edge).

Why this is safe (no ghost-line leak): the region revealed is `[total, total + allowance)`. The next line's box starts at `total`, and GTK cannot draw its ink above `total + pixels_above_lines`; the allowance never exceeds `pixels_above_lines`, so the revealed strip contains only the last line's descender overhang and blank spacing. And since `column_split` guaranteed `widget_height - total ≥ guard + BASE_BOTTOM_MARGIN`, the clip stays well above 0.

**Pixel measurements backing this (headless cage, MND at the failing spread, band painted red):** line boxes tile at 28px pitch; full-line ink spans 24px ending flush at the logical box bottom (band top); the next line's ink starts ~5px below the band top. So the descender cut is real (~0–1px in the cairo render, worse on the real display) and the safe reveal budget is ~5px — matching `min(guard, pixels_above_lines)`, not the uncapped guard.

---

## File Structure

- `src/input/scroll.rs` — the ONLY source file changed.
  - Add a pure helper `exact_end_clip(widget_height, total, descender_allowance) -> i32` next to `overtall_clip` (~line 653).
  - Call it from the `exact_end` branch of `update_bottom_clip` (~line 771), passing `descender_guard_px(text_view, page_top)`.
  - Add a `exact_end_clip_tests` unit module next to `overtall_clip_tests` (~line 1306).
- `docs/troubleshooting/clip-prevention.md` — append a short note documenting the `exact_end` descender allowance (checklist context), so the next debugger finds it.

---

### Task 1: Pure `paged_bottom_clip` + `descender_allowance` helpers + unit tests

**Files:**
- Modify: `src/input/scroll.rs` (add helpers near `overtall_clip`, ~line 653; add test module near `overtall_clip_tests`)
- Test: `src/input/scroll.rs` (`#[cfg(test)] mod paged_bottom_clip_tests`)

**Interfaces:**
- Produces: `pub(crate) fn paged_bottom_clip(space: i32, total: i32, allowance: i32) -> i32`
  — returns `(space - total - allowance).max(0)`. Used by BOTH `line_yrange`-summing clips in `update_bottom_clip` (Task 2).
- Produces: `pub(crate) fn descender_allowance(guard: i32, spacing_above: i32) -> i32`
  — returns `guard.min(spacing_above.max(0))`: the descent-sized reveal, capped at the next line's guaranteed-blank `pixels_above_lines` spacing.

- [ ] **Step 1: Write the failing tests**

Add this module next to the existing `overtall_clip_tests` module in `src/input/scroll.rs`:

```rust
#[cfg(test)]
mod paged_bottom_clip_tests {
    use super::{descender_allowance, paged_bottom_clip};

    #[test]
    fn subtracts_descender_allowance_from_slack() {
        // widget 1112, content sums to 1000 → 112px of slack below the last
        // line. A 6px descender allowance moves the clip top edge 6px below
        // the logical line bottom, revealing the flush descender ink.
        assert_eq!(paged_bottom_clip(1112, 1000, 6), 106);
    }

    #[test]
    fn never_negative_when_content_fills_widget() {
        // Degenerate: content already equals/overflows the space (the last
        // page, or pre-layout heights). Clip floors at 0.
        assert_eq!(paged_bottom_clip(1112, 1112, 6), 0);
        assert_eq!(paged_bottom_clip(1112, 1110, 6), 0);
    }

    #[test]
    fn zero_allowance_is_the_old_behavior() {
        // With allowance 0 the helper reduces to space - total, the pre-fix
        // clip — proves the change is purely additive.
        assert_eq!(paged_bottom_clip(1112, 1000, 0), 112);
    }

    #[test]
    fn allowance_is_capped_at_the_inter_line_spacing() {
        // The next line's ink can start as little as pixels_above_lines below
        // the clip's top edge (measured ~5px on the failing MND spread), so
        // the reveal must never exceed that spacing — a full descender_guard
        // (up to 24) would expose the next line's ascender tops.
        assert_eq!(descender_allowance(12, 6), 6);
        assert_eq!(descender_allowance(4, 6), 4);
        // line_spacing 0 → no reveal budget → old behavior, never negative.
        assert_eq!(descender_allowance(12, 0), 0);
        assert_eq!(descender_allowance(12, -3), 0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin linux-lit paged_bottom_clip_tests 2>&1 | tail -20`
Expected: FAIL — cannot find `paged_bottom_clip` / `descender_allowance` (helpers not defined yet).

- [ ] **Step 3: Add the helpers**

Insert directly ABOVE `pub(crate) fn overtall_clip(` in `src/input/scroll.rs`:

```rust
/// Bottom-clip height for a paged column whose content is a sum of
/// `line_yrange` LOGICAL line heights (`total`): `space` minus `total`, minus a
/// small `allowance` so the clip's top edge sits BELOW the last line's glyph
/// descenders (which render flush to the logical line bottom) instead of
/// slicing them. Floored at 0. Pure so the arithmetic is unit-tested without
/// GTK geometry.
pub(crate) fn paged_bottom_clip(space: i32, total: i32, allowance: i32) -> i32 {
    (space - total - allowance).max(0)
}

/// How far the paged clip's top edge may drop below the last line's logical
/// bottom: the font's descent (`guard`, what the descenders need), capped at
/// the inter-line spacing above the next line's ink (`spacing_above` =
/// `pixels_above_lines`). The shared buffer renders the NEXT line immediately
/// below the logical bottom, and GTK cannot draw its ink above
/// `box_top + pixels_above_lines` — so a reveal within that spacing shows only
/// descender overhang and blank space, never the next line's ascenders.
/// Degrades to 0 (the pre-fix clip) when line_spacing is 0.
pub(crate) fn descender_allowance(guard: i32, spacing_above: i32) -> i32 {
    guard.min(spacing_above.max(0))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin linux-lit paged_bottom_clip_tests 2>&1 | tail -20`
Expected: PASS — `test result: ok. 4 passed` (filtered).

- [ ] **Step 5: Commit**

```bash
git add src/input/scroll.rs
git commit -m "feat(scroll): add pure paged_bottom_clip + descender_allowance helpers"
```

---

### Task 2: Apply the descender allowance to both `line_yrange`-summing clips

**Files:**
- Modify: `src/input/scroll.rs` — the `exact_end` branch of `update_bottom_clip` AND the final non-`exact_end` clip computation.

**Interfaces:**
- Consumes: `paged_bottom_clip` + `descender_allowance` (Task 1); `descender_guard_px` (existing, already computed at the top of `update_bottom_clip` as `descender_guard`); `text_view.pixels_above_lines()`.

- [ ] **Step 1: Apply in the `exact_end` branch**

Replace:

```rust
        let clip = (widget_height - total).max(0);
```

with:

```rust
        // Drop the clip's top edge below the last line's logical bottom so its
        // flush descender ink (g/y/p/comma tails) isn't sliced — capped at the
        // next line's pixels_above_lines, the only strip guaranteed free of the
        // next line's ink (the shared buffer renders it right below the split).
        let allowance = descender_allowance(descender_guard, text_view.pixels_above_lines());
        let clip = paged_bottom_clip(widget_height, total, allowance);
```

(`descender_guard` is already computed above the branch from `descender_guard_px(text_view, page_top)`.)

- [ ] **Step 2: Apply in the non-`exact_end` final clip**

The single-column/prose clip has the identical logical-bottom defect. Replace:

```rust
    let clip = (widget_height - display_range.total_height + scroll_offset.round() as i32).max(0);
```

with:

```rust
    // Same flush-descender allowance as the exact_end branch: the clip's top
    // edge otherwise lands at the last line's logical bottom and slices its
    // descender ink.
    let allowance = descender_allowance(descender_guard, text_view.pixels_above_lines());
    let clip = paged_bottom_clip(
        widget_height + scroll_offset.round() as i32,
        display_range.total_height,
        allowance,
    );
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build 2>&1 | tail -3`
Expected: `Finished ... dev` with no NEW errors (pre-existing dead-code warnings are fine).

- [ ] **Step 4: Run the full pure-logic test suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS — no regressions. In particular the existing `overtall_clip_tests`, `page_turn_lock_tests`, and any `viewport` pagination tests stay green (this change does not touch line-fitting, only the final clip pixel height).

- [ ] **Step 5: Commit**

```bash
git add src/input/scroll.rs
git commit -m "fix(scroll): reserve descender room on two-column last line clip

The exact_end bottom-clip clipped at the last line's line_yrange logical
bottom, slicing g/y/p descenders off the bottom line of each column in a
two-column verse spread. Subtract descender_guard_px so the clip top edge
sits below the descenders; the >=40px BASE_BOTTOM_MARGIN slack guarantees
this never exposes a next-page row."
```

---

### Task 3: Headless pixel verification (agent self-check)

**Files:** none (verification only). Uses the headless flow from `CLAUDE.md`.

This task PROVES the descenders are no longer cut. Reproduces the exact spread that failed, first with the clip painted red to see the band edge, then normally.

- [ ] **Step 1: Temporarily paint the clip box red**

In `src/theme.rs`, find `.card-bottom {{ background-color: {bg};` and change `{bg}` to `#ff0000`:

```rust
         .card-bottom {{ background-color: #ff0000; border-radius: 0 0 12px 12px; }} \
```

Then: `cargo build 2>&1 | tail -1`

- [ ] **Step 2: Launch MND headless at the failing spread**

```bash
cd ~/utono/linux-lit
rm -rf /tmp/xdg-mnd && mkdir -p /tmp/xdg-mnd && chmod 700 /tmp/xdg-mnd
XDG_RUNTIME_DIR=/tmp/xdg-mnd LIT_HEADLESS_TEST=1 LIT_START_WORK=MND LIT_START_POS=1534 GSK_RENDERER=cairo \
  ./scripts/e2e-env.sh cage -- ./target/debug/linux-lit 2>/tmp/cage-mnd.log &
```

Wait ~8s for the surface to map (check `ls /tmp/xdg-mnd/wayland-0`). Note: the wrapper's `dbus-run-session` is REQUIRED — a bare `cage` launch of this app exits immediately (no session bus). Use an isolated `XDG_RUNTIME_DIR` (`/tmp/xdg-mnd`) so cage does not collide with the live session's `wayland-0`.

- [ ] **Step 3: Screenshot and inspect the red band**

```bash
XDG_RUNTIME_DIR=/tmp/xdg-mnd WAYLAND_DISPLAY=wayland-0 grim /tmp/mnd-verify-red.png
stat -c%s /tmp/mnd-verify-red.png   # must be tens-to-hundreds of KB, not ~2 bytes
```

Read `/tmp/mnd-verify-red.png`. Expected: a GAP of ~`descender_guard` px between each column's last-line descenders and the red band's top edge — the 'y'/'p'/'g' descenders of the bottom line are FULLY above the red, no glyph touching it. (Before the fix, the descenders were sliced by the red top edge.)

Optional precise check:

```bash
python3 - <<'PY'
from PIL import Image
import numpy as np
img = np.array(Image.open('/tmp/mnd-verify-red.png').convert('RGB'))
R,G,B = (img[:,:,i].astype(int) for i in range(3))
red = (R>180)&(G<80)&(B<80)
gray = np.array(Image.open('/tmp/mnd-verify-red.png').convert('L'))
for name,(xa,xb) in {'left':(40,580),'right':(640,1180)}.items():
    rr = np.where(red[:,xa:xb].sum(axis=1)>50)[0]
    band_top = rr.min() if len(rr) else -1
    ink = np.where((gray[:band_top,xa:xb]<120).sum(axis=1)>2)[0]
    last_ink = ink.max() if len(ink) else -1
    print(f'{name}: red band top={band_top} last ink={last_ink} gap={band_top-last_ink}')
PY
```

Expected: `gap` ≈ the descender allowance (roughly 8–20px at the dev font), and the last-ink row is the FULL glyph including descenders. A gap of ~1px (the pre-fix value) means the fix did not take — re-check Task 2.

- [ ] **Step 4: Revert the red diagnostic and rebuild**

In `src/theme.rs`, change `#ff0000` back to `{bg}`:

```rust
         .card-bottom {{ background-color: {bg}; border-radius: 0 0 12px 12px; }} \
```

Then: `cargo build 2>&1 | tail -1`

- [ ] **Step 5: Re-launch normally and confirm descenders intact**

Kill the red instance, relaunch, screenshot:

```bash
pkill -f "cage -- ./target/debug/linux-lit"
rm -rf /tmp/xdg-mnd && mkdir -p /tmp/xdg-mnd && chmod 700 /tmp/xdg-mnd
XDG_RUNTIME_DIR=/tmp/xdg-mnd LIT_HEADLESS_TEST=1 LIT_START_WORK=MND LIT_START_POS=1534 GSK_RENDERER=cairo \
  ./scripts/e2e-env.sh cage -- ./target/debug/linux-lit 2>/tmp/cage-mnd.log &
sleep 8
XDG_RUNTIME_DIR=/tmp/xdg-mnd WAYLAND_DISPLAY=wayland-0 grim /tmp/mnd-verify.png
```

Read `/tmp/mnd-verify.png`. Expected: the last line of BOTH columns shows complete descenders (compare the 'g' in "thighs"/"night", 'p' in "tapers"/"compare", 'y' in "my"/"eyne") — no slicing, with a comfortable gap to the card's bottom rounded corner.

- [ ] **Step 6: Clean up the headless instance**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

(Kill ONLY this cage-scoped pattern — never a bare `pkill -f target/debug/linux-lit`, which also kills the user's live `crll` instance.)

- [ ] **Step 7: Report the screenshots inline**

Per the *UI review protocol* in `CLAUDE.md`, describe what both screenshots show in the reply: quote the bottom line of each column and confirm the descenders are intact. Do NOT claim "verified" from the log alone — the pixel screenshot is the acceptance criterion.

---

### Task 4: Document the fix in clip-prevention.md

**Files:**
- Modify: `docs/troubleshooting/clip-prevention.md` (append to the failure checklist / the paged-clip section)

**Interfaces:** none.

- [ ] **Step 1: Add a checklist entry**

In `docs/troubleshooting/clip-prevention.md`, find the "The failure checklist" section and add a new numbered item after #9 (renumber is unnecessary — append as #10):

```markdown
10. **MAIN CARD, two-column only — the last line of a column has its descenders
    (g/y/p/comma tails) sliced by the card-colored bottom clip.** Tell: on a
    verse two-column spread the bottom line of the LEFT and/or RIGHT column is
    cut through its descenders, while a ~40px+ gap still sits below the clip.
    Cause: the paged `update_bottom_clip` clips at `widget_height - total`
    where `total` sums `line_yrange` (LOGICAL) heights, so the clip's top edge
    lands at the last line's logical bottom — and the descender ink renders
    flush to that bottom. The fix subtracts a small allowance from the clip
    (`paged_bottom_clip` + `descender_allowance` in `src/input/scroll.rs`):
    the font's descent capped at `pixels_above_lines`, the only strip below the
    logical bottom guaranteed free of the NEXT line's ink (the shared buffer
    renders the next line right there, merely hidden by the clip).
    This is NOT the free-scroll partial-row mask and NOT the HIGHLIGHT band
    (#9): it happens on the LAST line of a column regardless of the cursor.
    Diagnose by painting `.card-bottom` red for one run — the band's top edge
    visibly crosses the descenders.
```

- [ ] **Step 2: Cross-reference from the paged-clip bullet**

In the "Not the same as the paged clip" section, on the **Paged clip** bullet, add a sentence noting the two-column case:

Find the bullet starting `- **Paged clip** (`scroll.rs::update_bottom_clip`)` and append to its paragraph:

```markdown
  Both its `line_yrange`-summing clips (the two-column `exact_end` branch and the
  single-column final clip) additionally subtract a descender allowance
  (`paged_bottom_clip` + `descender_allowance`: the font descent capped at
  `pixels_above_lines`) so the last visible line keeps its flush descender ink —
  see failure-checklist #10.
```

- [ ] **Step 3: Commit**

```bash
git add docs/troubleshooting/clip-prevention.md
git commit -m "docs(clip): note two-column last-line descender allowance (exact_end_clip)"
```

---

## Self-Review

**1. Spec coverage:**
- Root cause (exact_end clips at logical line bottom) → Task 1 + Task 2. ✓
- Both columns fixed (left `Some(cs.split)` and right `Some(right_end)` both hit the branch) → Task 2 changes the shared branch, covering both. ✓
- No partial-row leak → guaranteed by the `≥ guard + 40` slack argument; asserted indirectly by Task 3 Step 5 (no next-page line appears). ✓
- Pixel acceptance → Task 3. ✓
- Documentation → Task 4. ✓

**2. Placeholder scan:** No TBD/TODO; all code shown in full; exact commands with expected output. ✓

**3. Type consistency:** `paged_bottom_clip(space: i32, total: i32, allowance: i32) -> i32` and `descender_allowance(guard: i32, spacing_above: i32) -> i32` defined in Task 1 and called identically in Task 2. `descender_guard_px(text_view, page_top) -> i32` and `pixels_above_lines() -> i32` are the existing signatures. ✓

## Notes / open questions for the implementer

- **Allowance size (superseded by amendment #1).** The allowance is `min(descender_guard_px, pixels_above_lines)` — do NOT raise it past `pixels_above_lines`: the next line's ascender ink starts right after that spacing (measured ~5px below the clip top on the failing spread), so any larger reveal shows a ghost sliver of the next line. If verification shows descenders still grazing, the correct lever is the view's line spacing (`pixels_below_lines` gives the descenders room inside the line box), not a bigger reveal.
- **Why not fix it in `column_split`?** `column_split` already reserves the room; the room exists (52px measured). The defect is purely that the clip's top edge is placed at the wrong y. Fixing it at the clip is the minimal, local change and does not perturb pagination/tiling.
- **Highlight band (#9) is separate.** The main card already sets `pixels_below_lines = line_spacing`, so the *highlight* band does not cut descenders mid-page. This plan is about the *clip box*, which is why the bottom line specifically was affected. If, after this fix, a mid-page highlighted line still shows cut descenders, that is checklist #9 and out of scope here.
