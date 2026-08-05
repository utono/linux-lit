# Traced regeneration: BH-Barrett ch. 37 prose overflow — GEN vs POSTWALK vs RENDER

Worktree: `/home/mlj/utono/linux-lit-wt/diag/ch37-clip` at `b0ccf5c9`.
Launch: hand-rolled cage (short `/tmp/lit-ch37` runtime dir — the harness's
tempdir path exceeded `SUN_LEN` for the Wayland socket), `LIT_DEV=1
LIT_HEADLESS_TEST=1 LIT_NO_MPV=1 LIT_START_WORK=BH-Barrett
LIT_START_SCENE=37.0 LIT_GEN_PAGE_TABLE=1`, resized to 1920x1200 via
`wlr-randr` fired in a tight retry loop immediately after the wayland socket
appeared (had to win the race against the app's own first settled-layout
tick, which otherwise generates a table at cage's default 1280x720 first and
burns the one-shot `prose_page_table_gen_attempted` latch before the resize
lands). Confirmed `RESIZE_TICK: text_view.height changed 648 -> 1128` before
generation. Table regenerated fresh each run at the 1920x1200 fingerprint
(`fp=...1920x1200...pv7`), landing on 804/806/808 pages across three separate
runs (see "Note on determinism" below) — always with `PAGES_PROSE_DRIFT:
summary ... over_usable=` in the several-dozens, confirming the census still
sees the defect after the freshest possible regeneration at production
geometry.

## The three vectors, for the reproducing page

Overflowing page found by driving `x` from the ch.37 landing:
`page_top=4258 top_off=62 end=4270 bottom_head=Some(175)`, i.e. lines
4258..4269 (12 lines), total=1227 vs widget_h=1128 (**99px over**), clip
floored to 0.

`GEN_HEIGHTS` (window 4250:4275, from the pre-walk sweep,
`prose_pages.rs:594`):

```
GEN_HEIGHTS: [4250..=4275] = [68, 40, 96, 40, 96, 68, 40, 40, 96, 68, 40, 40,
  96, 40, 68, 40, 40, 181, 237, 349, 265, 124, 209, 349, 181, 181]
```

`POSTWALK_HEIGHTS` (same window, after the boundary walk,
`prose_pages.rs:730`) — **byte-identical to GEN_HEIGHTS**:

```
POSTWALK_HEIGHTS: [4250..=4275] = [68, 40, 96, 40, 96, 68, 40, 40, 96, 68, 40,
  40, 96, 40, 68, 40, 40, 181, 237, 349, 265, 124, 209, 349, 181, 181]
```

`RENDER_HEIGHTS` (`scroll.rs:1156`, captured once the overflowing page was
actually driven onto screen):

```
RENDER_HEIGHTS: page_top=4258 end=4270 top_off=62 tv_width=1050
  left_margin=20 right_margin=131 wrap_w=899
  heights=[96, 96, 40, 40, 96, 40, 96, 40, 68, 209, 293, 406]
```

## Per-line delta table (lines 4258..4269)

| line | gen | postwalk | render | delta |
|---|---|---|---|---|
| 4258 | 96  | 96  | 96  | 0 |
| 4259 | 68  | 68  | 96  | +28 |
| 4260 | 40  | 40  | 40  | 0 |
| 4261 | 40  | 40  | 40  | 0 |
| 4262 | 96  | 96  | 96  | 0 |
| 4263 | 40  | 40  | 40  | 0 |
| 4264 | 68  | 68  | 96  | +28 |
| 4265 | 40  | 40  | 40  | 0 |
| 4266 | 40  | 40  | 68  | +28 |
| 4267 | 181 | 181 | 209 | +28 |
| 4268 | 237 | 237 | 293 | +56 |
| 4269 | 349 | 349 | 406 | +57 |

Sum of positive deltas = 225px. Every non-zero delta is a multiple of the
~28-29px row pitch (1 or 2 extra wrapped rows). This is **row-quantized**: 4
lines gain exactly 1 row, 2 lines gain exactly 2 rows, 6 lines are unchanged.

## Which moment diverges

`GEN_HEIGHTS == POSTWALK_HEIGHTS` exactly — heights do **not** move during
the boundary walk. The divergence is entirely `POSTWALK` (= generation-time,
what gets pinned into the stored table) vs `RENDER` (what the reader actually
draws once the page is scrolled onto screen).

## Ruled out

- **Tags applied post-generation.** `LIT_TRACE_TAGS=4258:4269` dumped every
  tag span at both GEN and RENDER for each line. For 4264, 4266, 4267, 4268,
  4269 — 5 of the 6 divergent lines — the tag set is **byte-identical**
  between GEN and RENDER (e.g. line 4267 carries `vocab-word@396` at *both*
  moments; it was applied before generation, not after). Only line 4259
  picked up a new tag at render time (`phrase-highlight@0`, the karaoke/
  cursor-proximity tint), and that tag is `background`-only — no
  weight/size/family property — so it cannot itself cause a re-wrap. This
  rules out "a tag applied after load changes wrap width" as the general
  mechanism; it explains at most 1 of 6 lines, and even there the tag itself
  has no property that affects layout.
- **Geometry/font drift.** `tv_width=1050`, `left_margin=20`,
  `right_margin=131`, `wrap_w=899`, and `font=Charis 13.333px` are identical
  in every `RENDER_HEIGHTS`/`TAGTRACE` line at both phases. No CSS/font
  reflow occurred between generation and render.
- **Line-spacing properties.** `pixels_above_lines`/`pixels_below_lines`/
  `pixels_inside_wrap` (`above=6 below=6 inwrap=0`) are constant across every
  sampled line in both phases.
- **The boundary walk itself.** `GEN_HEIGHTS` (pre-walk) and
  `POSTWALK_HEIGHTS` (post-walk) are byte-identical, so the walk does not
  perturb heights; whatever moves them fires later, at real render/scroll
  time.

## Named culprit: the buffer-wide `line_yrange` sweep does not fully resolve
## GTK's lazy-validation frontier for lines far outside the current viewport

At generation time the on-screen viewport was still `page_top=4223`
(`DISPLAY_WORK: resumed saved position current_line=4224 page_top=4223`,
confirmed constant through generation via repeated
`BOTTOM_CLIP_ROWFILL: ... page_top=4223 ...` lines up to the moment
`PAGES_PROSE_PANGO`/`GEN_HEIGHTS` were logged at 4137-4145ms). Lines
4258-4269 are 35-46 buffer lines below that — never scrolled into view before
generation ran.

`record_prose_pages` (`prose_pages.rs:435`) already documents this exact
failure class in its own comments: "GTK4 validates a TextView's line layout
lazily around the currently-scrolled viewport" and states the fix is a
buffer-wide `line_yrange` sweep, because "`line_yrange` validates the line
synchronously and GTK caches the result." **That belief is the residual
defect.** The built-in ground-truth probe (`LIT_TRACE_PANGO=1`, which
constructs an independent Pango layout at the correct wrap width/font and
compares its pixel height against the swept `line_yrange`) shows the sweep's
own values disagree with true wrapped metrics on the majority of the
document (`PAGES_PROSE_PANGO: lines=7300 disagree=6047 delta_sum=-1561
worst=line 4333 pango=322 yrange=293 ...`) — and crucially the disagreements
run in **both directions** (e.g. example `L1:yr=265/pango=237` — yrange
*over*-measures there by 28px, the mirror image of the under-measurement seen
at 4258-4269). This is consistent with `line_yrange()` on a
never-displayed, far-off line returning a provisional/estimated wrap that
GTK's internal cache nonetheless treats as validated — and which is later
silently replaced by a different (here, taller) value once the line is
actually scrolled into the exposed/rendered region, which is what driving
`x` forward does.

So: the sweep's synchronous-validation assumption, which the prior
font-timing fix (pv5→pv7) relied on to fix the *font-not-yet-applied* defect,
does not fully hold for buffer positions far from the viewport at generation
time — a second, distinct lazy-validation gap. `changed_between_sweeps=0`
(two back-to-back `line_yrange` sweeps agreeing) is not sufficient proof of
correctness, exactly as the sweep's own comment warns for the font-timing
case, but the residual gap here isn't about a pending CSS/tag invalidation —
it's positional: GTK's layout validation is scroll-proximity-gated in a way
a synchronous per-line `line_yrange()` call does not fully defeat for lines
tens of buffer-lines outside the current viewport, even after
`queue_resize()` + pumping the main loop to convergence.

## Row-quantization

Yes, decisively. All 6 divergent lines in the sampled window changed by
exactly 28px (1 row) or 56-57px (2 rows) — the app's own row pitch. Multi-row
paragraphs (4267: 538 chars, 4268: 732 chars, 4269: 1079 chars) show the
largest absolute deltas (28, 56, 57px) because they wrap into the most rows
and so have the most rows subject to the lazy-frontier mis-measurement; the
single-row short dialogue lines among them (4258, 4260-4263, 4265) show zero
delta, consistent with a per-ROW measurement defect rather than a per-line
constant offset.

## Note on determinism

Across three independent fresh-generation runs at the identical 1920x1200
fingerprint, the resulting page COUNT varied (804 / 806 / 808 pages) even
though `PAGES_PROSE_SWEEP: changed_between_sweeps=0 delta_sum=0` reported the
two-sweep convergence check passing every time. This is itself corroborating
evidence for the named culprit: if generation's line-height vector depended
only on deterministic inputs (content, font, width), the boundary walk would
produce the same grid every time. The variation across runs is consistent
with a race between GTK's background layout-validation completing for
lines outside the viewport and the moment the sweep samples them — a race,
not a fixed input->output function.
