# Bottom Clip & Descender Clipping

This document consolidates the history of a recurring problem: the bottom edge
of the e-reader page sometimes clips descenders, sometimes shows a partial
line poking through, and sometimes leaves a large empty gap. Every fix so far
has resolved one facet while re-creating another.

## The Widget Stack

```
card_vbox                (vertical Box)
├─ top_spacer            (fixed height; one line_h)
├─ scrolled_overlay      (vexpand — grows to fill remaining space)
│  ├─ scrolled_window
│  │  └─ text_view       (sourceview5::View)
│  └─ bottom_clip        (Overlay child, valign: End, covers bottom N px)
└─ bottom_spacer         (contains the page-line label at valign: End)
```

Key invariant: `scrolled_overlay.height = card_vbox.height - top_spacer.height - bottom_spacer.height`. Any change to `bottom_spacer` reflows `scrolled_overlay`, which changes `text_view.height()`, which changes how many lines fit, which changes the clip height.

## Facet 1 — Descenders Clipped (original bug)

**Symptom:** The last visible line on a page has its descenders (g, p, y, j, q) clipped by the bottom clip overlay.

**Root cause:** `update_bottom_clip` originally computed `clip = widget_height % line_height`. `scroll_to_iter` introduces sub-pixel offsets so the modulo didn't match the actual leftover space. A 7px clip couldn't cover a 15–20px partial line.

A second circular dependency existed: `lines_per_page` walked the buffer calling `is_line_fully_visible`, which checked against the clip — the clip was too small because the page showed too many lines, and vice versa.

**Fix (commit predating this doc):**

- `update_bottom_clip` sums actual `line_yrange` heights from `page_top` until the next line would exceed `widget_height - descender_guard`, then sets `clip = widget_height - total_height`. `line_yrange` values are buffer-absolute so they are independent of scroll position.
- `descender_guard` is ~20% of the first visible line's height (min 6px), reserved so the last line's descenders always have clearance below the baseline.
- `lines_per_page` caps its walk at the same point.
- `is_line_fully_visible` uses a plain range check: `line >= page_top && line < page_top + lpp - 1`.

**Why not `text_view.set_bottom_margin()`?** The clip overlay sits on top of the scrolled window as a sibling overlay and would cover any margin. The margin approach is incompatible with the overlay-based architecture.

**Why not `buffer_to_window_coords`?** It returns stale values inside the `idle_add_local_once` callback where the clip is updated — the scroll from `scroll_to_iter` hasn't fully committed yet. `line_yrange` avoids this.

## Facet 2 — Partial Line on Startup

**Symptom:** On app launch, the last line at the bottom is partially visible (clipped). Pressing `q` (page forward) fixes it; all subsequent pages render correctly.

**Root causes in `update_highlight_and_show`:**

1. `update_bottom_clip` was called with `current_line` (the highlighted line) instead of `page_top_line`. On startup these differ because the cursor lands on the first dialogue line while `page_top_line` is often one line above (to show the speaker).
2. The clip update ran in the same idle tick that made the scrolled window visible. GTK hadn't completed its layout pass, so `line_yrange` returned stale heights.

**Fix:**

- Pass `page_top_line` (not `current_line`).
- Call `scrolled_window.set_visible(true)` first, then defer `update_bottom_clip` to a nested `glib::idle_add_local_once` so GTK has one frame to lay out the visible widget before heights are queried.

`snap_scroll_to_line` (used by page turns) already passed the correct anchor after the widget was visible, which is why only the startup path was affected.

## Facet 3 — Oversized Bottom Gap on Plays (2026-04-14)

**Symptom:** In Tim-Amb (Timon of Athens, Ambrose) the top and bottom margins look 2–4× larger than on Tit-Amb (Titus Andronicus, Ambrose) despite identical layout code.

**Investigation:**

- Both are Ambrose-format plays with identical rendering paths.
- Tit-Amb pages end mid-dialogue; Tim-Amb pages in the screenshots end on the last line of a speech immediately before a speaker name.
- `update_bottom_clip` contains a second loop that trims trailing speaker names and blank lines from the fit region so a speaker never dangles at the bottom of a page without its dialogue. Trimming reduces `total_height`, which grows `clip` by the removed height (~60–120px on pages with trimmed speaker+blank).
- `bottom_clip` is an overlay at the bottom of the scrolled area, *plus* `bottom_spacer` sits below it — so total visible bottom gap = `clip_height + bottom_spacer_height`. Pages with heavy trimming showed `~100px clip + 55px spacer ≈ 155px` ≈ 3 line heights.

## Attempted Fix 1 — Subtract Clip Excess from Spacer

**Idea:** Keep total bottom gap ≈ one line by making the spacer absorb only the remainder after the clip's "useful" portion.

```rust
// First attempt
let spacer_h = (line_h_ref - clip).max(0);
```

**Result:** Broke Facet 1. When `clip` was small (~22px descender guard), `spacer_h` shrank to ~10px. But shrinking the spacer makes `scrolled_overlay` grow (because it's `vexpand`), which grows `text_view.height()`, which lets the next line partially fit. The clip was computed on the *old* widget_height and was too small to hide the newly-exposed line. Descenders of the last line were clipped again.

## Attempted Fix 2 — Preserve Descender Guard, Recompute on Reflow

**Idea:** Only subtract the clip *excess beyond* the descender guard. Then after the spacer change reflows the scrolled area, re-run a clip-only pass so the clip matches the new `widget_height`.

```rust
let extra_clip = (clip - descender_guard).max(0);
let spacer_h = (line_h_ref - extra_clip).clamp(descender_guard, line_h_ref);
bottom_spacer.set_height_request(spacer_h);
bottom_clip.set_height_request(clip);

// Second-pass: recompute clip after layout reflows.
glib::idle_add_local_once(move || {
    recompute_bottom_clip_only(...);
});
```

**Result (still broken, as of screenshot 2026-04-14 19:07):** A partial line pokes through at the bottom. The log shows widget_height and spacer_h oscillating indefinitely:

```
widget_h=1085 total_h=1072 clip=13 spacer=53  (page_top=29)
widget_h=1070 total_h=1043 clip=27 spacer=39
widget_h=1084 total_h=1072 clip=12 spacer=54
widget_h=1069 total_h=1043 clip=26 spacer=40
widget_h=1083 total_h=1072 clip=11 spacer=55
widget_h=1068 total_h=1043 clip=25 spacer=41
widget_h=1082 total_h=1043 clip=39 spacer=27
widget_h=1096 total_h=1072 clip=24 spacer=42
widget_h=1081 total_h=1043 clip=38 spacer=28
widget_h=1095 total_h=1072 clip=23 spacer=43
...
```

Each recomputation fires a new reflow because the spacer changes, which fires another recomputation. The clamp to `[descender_guard, line_h]` isn't tight enough to stop the oscillation — the system has two stable-ish attractors (clip≈11, clip≈25) and bounces between them.

## Why the Spacer-Absorbs-Clip Approach Is Structurally Wrong

The two quantities we're trying to balance are coupled through the GTK layout:

```
spacer_h → scrolled_overlay.height → text_view.height → which lines fit → clip_h
```

Any fix that reads `clip` and writes `spacer_h` (or vice versa) creates a feedback loop. `idle_add_local_once` doesn't break the loop — it just adds a frame of delay between iterations. The only way to avoid oscillation is to decouple the two quantities.

## Options for a Permanent Fix

1. **Fixed spacer, accept variable bottom gap.** Set `bottom_spacer = line_h` unconditionally (the pre-2026-04-14 behavior). Pages with trimmed trailing speaker names will show ~3 line-heights of bottom gap; pages without trimming show ~1. This is the current `master`-style behavior. Visually uneven, but stable — no oscillation, descenders are always safe.

2. **Eliminate the clip's role in covering trimmed lines.** Instead of letting `clip` grow to hide trimmed speaker+blank lines, *re-anchor `page_top`* so those lines don't render at all on this page and are shown on the next page. This decouples clip from trim — clip only ever covers the descender guard, and the variable content is handled by pagination, not clipping. Requires changing where `snap_scroll_to_line` seeks and adjusting the line_count loops. Larger change, but removes the root coupling.

3. **Pin `scrolled_overlay.height` explicitly.** Use `set_size_request` on the scrolled widget itself instead of relying on `vexpand` to fight with the spacer. If `scrolled_overlay.height` is fixed, changing `bottom_spacer` no longer reflows `text_view.height`, breaking the feedback loop. The spacer can then freely absorb excess clip without side-effects. The cost is that `card_vbox`'s total height must be known up-front (via a resize signal) and pushed down to the scrolled widget on every window resize.

4. **Drop the bottom_spacer entirely.** Move the page-line label into a separate layer (e.g., an overlay on the page_turn_overlay) so the card ends at the scrolled overlay's bottom edge. Bottom gap = clip height alone; top gap = top_spacer alone. No coupling because there's no second variable. Requires redesigning label placement but is the simplest to reason about.

## Recommendation

Option 4 (drop bottom_spacer) is the cleanest — one fewer coupled quantity means the feedback loop can't exist. Option 3 (pin scrolled_overlay height) is a good middle ground if the label's current layout matters.

Option 1 is a safe fallback to ship the current page reliably while we design one of 3 or 4. It is the state these notes should be viewed against: revert attempted fixes 1 and 2, live with the uneven bottom gap until a structural solution is built.

## Resolution (2026-04-14, Option 4)

Attempted fixes 1 and 2 were reverted and Option 4 was implemented.

**Changes:**

- `bottom_spacer` removed from `card_vbox`, from `AppState`, and from widget construction in `app.rs`.
- `scrolled` (the text view's `ScrolledWindow`) now carries the `card-bottom` CSS class, so the card's rounded bottom corners are drawn on the scrolled area itself rather than on a dedicated bottom box.
- `bottom_clip` overlay also carries `card-bottom` so it matches the card's rounded corners when visible.
- `page_line_label` was moved out of `bottom_spacer` and is now an overlay (`valign: End`) on `scrolled_overlay`, with `margin_bottom = 10px` providing the breathing room below it. Tile/monocle alignment in `apply_tiled_mode` still works because the label is an overlay child with its own `halign` / `margin_start`.
- `update_bottom_clip` reverted to its pre-facet-3 signature (no `bottom_spacer` param, no second-pass recompute helper).
- `apply_tiled_mode` and `update_spacer_heights` no longer touch `bottom_spacer`.

**Why this breaks the feedback loop:** there is no longer a widget whose height is derived from `clip`, so the scrolled area's height is stable across clip updates. The scrolled_overlay's height is determined solely by `card_vbox.height - top_spacer.height`, both of which are independent of the clip.

**Variable bottom gap is now intentional and absorbed.** Pages with trimmed trailing speaker names show a slightly larger visual gap at the bottom (because `bottom_clip` is taller). That gap is inside the card's rounded bottom and reads as natural breathing room, not as a broken layout.

## Relevant Code

- `src/input/navigation.rs`:
  - `update_bottom_clip` — main clip computation, trailing-speaker trim loop
  - `recompute_bottom_clip_only` — second-pass clip update (attempted fix 2)
  - `descender_guard_px` — per-line-height guard computation
  - `is_line_fully_visible`, `lines_per_page` — pagination using the same cap
- `src/app.rs`:
  - `apply_tiled_mode` — sets `top_spacer` height; previously set `bottom_spacer` too
  - `update_spacer_heights` — now only updates top_spacer
  - Widget construction at ~line 560 — `top_spacer`, `bottom_spacer`, `bottom_clip` setup
