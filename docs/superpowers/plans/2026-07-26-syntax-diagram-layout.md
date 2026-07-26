# Syntax Diagram Layout Pass — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the band annotations the vertical room the window already has, and stop sibling bands at one depth from sharing a row.

**Architecture:** Two independent changes. `assign_rows` in `src/syntax_diagram.rs` becomes a first-fit packer keyed on label extents instead of returning `b.depth` verbatim; `draw_analysis` in `src/ui/syntax_overlay.rs` derives row height from the window's free height instead of the font's natural leading. The packer stays pure and display-free — label widths are measured by the caller and passed in.

**Tech Stack:** Rust, GTK4, Cairo/Pango.

Spec: `docs/superpowers/specs/2026-07-26-syntax-diagram-layout-design.md`

## Global Constraints

- Work on a branch off master. Master is at `0e3573f9` with the syntax diagram merged; the `feat/syntax-diagram` worktree and branch are gone.
- Per CLAUDE.md, a branch expected to span sessions starts in a worktree: `git worktree add ~/utono/linux-lit-wt/<branch> -b <branch>`. Merge back from the MAIN checkout.
- Verify with `cargo build`; do NOT run `cargo run`. The user runs the app.
- Clippy baseline is **181** warnings. Do not exceed it.
- Unit-test baseline is **1162** passing. Every task must keep them green.
- `src/syntax_diagram.rs` must stay free of GTK, Cairo, and Pango imports — it is the display-free, unit-testable seam. Label widths come in as parameters.
- Acceptance is on the **real GL renderer**, not cage. Cage disagreed with GL on every defect fixed on 2026-07-26; a cage pass is not evidence for this change.
- Do NOT change band derivation, the prompt, the POS legend, the loading state, or the scrim. All shipped and verified.

---

## File Structure

**Modify:**
- `src/syntax_diagram.rs` — `assign_rows` becomes the packer; `max_row` follows it. Tests for packing live here (pure, no display).
- `src/ui/syntax_overlay.rs` — row-height budget derives from the window; the draw loop and `band_stack_bottom` consume packed row indices instead of `b.depth`.

**No new files.** The packer belongs beside the data model it operates on, and the seam already exists.

---

## Task 1: Pack rows by label collision

The piece most likely to be subtly wrong, and the one that fixes the visible defect. Pure — no GTK, no display.

**Files:**
- Modify: `src/syntax_diagram.rs:110-120`
- Test: inline `#[cfg(test)] mod tests` in `src/syntax_diagram.rs`

**Interfaces:**
- Consumes: `Band { start_char, end_char, label, depth }` (already exists in this file).
- Produces:
  - `pub fn assign_rows(bands: &[Band], label_widths: &[f64], char_w: f64) -> Vec<usize>`
  - `pub fn max_row(bands: &[Band], label_widths: &[f64], char_w: f64) -> usize`

  `label_widths[i]` is the pixel width of `bands[i].label`, measured by the caller. `char_w` is the average pixel width of one character, so the packer can convert a band's char span into a pixel extent without touching Pango. Both are pixels.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `src/syntax_diagram.rs`:

```rust
    fn band_at(start: usize, end: usize, depth: u8, label: &str) -> Band {
        Band { start_char: start, end_char: end, label: label.to_string(), depth }
    }

    /// 10px per character keeps the arithmetic legible in these tests.
    const CW: f64 = 10.0;

    #[test]
    fn disjoint_siblings_share_a_row() {
        // Two depth-1 bands far apart, each with a label narrower than its
        // own span: no reason to split them onto separate rows.
        let bands = vec![
            band_at(0, 10, 1, "one"),
            band_at(40, 50, 1, "two"),
        ];
        let widths = vec![30.0, 30.0];
        assert_eq!(assign_rows(&bands, &widths, CW), vec![0, 0]);
    }

    #[test]
    fn siblings_whose_labels_overlap_get_separate_rows() {
        // Spans are disjoint (0..10 and 12..22) but the labels are far wider
        // than the spans, so drawn on one row they would overprint. THIS is
        // the 2026-07-26 defect: five appositives sharing one row.
        let bands = vec![
            band_at(0, 10, 1, "appositive noun phrase"),
            band_at(12, 22, 1, "appositive noun phrase"),
        ];
        let widths = vec![200.0, 200.0];
        assert_eq!(assign_rows(&bands, &widths, CW), vec![0, 1]);
    }

    #[test]
    fn a_band_never_packs_above_a_shallower_band() {
        // depth 0 is the outermost band and must stay on the bottom row;
        // depth 1 sits above it even though their labels do not collide.
        let bands = vec![
            band_at(0, 100, 0, "main clause"),
            band_at(0, 10, 1, "subject"),
        ];
        let widths = vec![80.0, 60.0];
        let rows = assign_rows(&bands, &widths, CW);
        assert!(rows[1] > rows[0], "deeper band must sit above: {rows:?}");
    }

    #[test]
    fn single_band_is_row_zero() {
        let bands = vec![band_at(0, 10, 0, "only")];
        assert_eq!(assign_rows(&bands, &[40.0], CW), vec![0]);
    }

    #[test]
    fn no_bands_yields_no_rows() {
        assert_eq!(assign_rows(&[], &[], CW), Vec::<usize>::new());
        assert_eq!(max_row(&[], &[], CW), 0);
    }

    #[test]
    fn a_label_wider_than_everything_still_places() {
        // Degenerate input must not panic or loop forever.
        let bands = vec![
            band_at(0, 2, 1, "an extravagantly long label"),
            band_at(3, 5, 1, "another extravagantly long label"),
        ];
        let widths = vec![4000.0, 4000.0];
        let rows = assign_rows(&bands, &widths, CW);
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0], rows[1], "colliding labels must not share a row");
    }

    #[test]
    fn max_row_reports_the_highest_packed_row() {
        let bands = vec![
            band_at(0, 100, 0, "main clause"),
            band_at(0, 10, 1, "appositive noun phrase"),
            band_at(12, 22, 1, "appositive noun phrase"),
        ];
        let widths = vec![80.0, 200.0, 200.0];
        // depth 0 on row 0; the two colliding depth-1 bands on rows 1 and 2.
        assert_eq!(max_row(&bands, &widths, CW), 2);
    }

    #[test]
    fn mismatched_widths_length_does_not_panic() {
        // Defensive: a caller bug must degrade, not crash the reader.
        let bands = vec![band_at(0, 10, 0, "a"), band_at(20, 30, 0, "b")];
        let rows = assign_rows(&bands, &[30.0], CW);
        assert_eq!(rows.len(), 2);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/<branch> && cargo test --bins syntax_diagram:: 2>&1 | tail -20`

Expected: FAIL to compile — `this function takes 1 argument but 3 arguments were supplied`. That confirms the tests exercise the new signature.

- [ ] **Step 3: Implement the packer**

Replace `assign_rows` and `max_row` in `src/syntax_diagram.rs`:

```rust
/// Display row per band. Row 0 sits directly under the POS strip; deeper rows
/// stack above it, so the outermost band draws the bottom rule.
///
/// Rows are PACKED, not simply `depth`: two bands at the same depth share a
/// row only when their drawn extents — including their LABELS — do not
/// collide. Returning `b.depth` verbatim put five disjoint appositives on one
/// row and their labels overprinted each other (2026-07-26 GL check). Depth
/// still drives ordering, so nesting continues to read as depth; a band never
/// packs onto a row below a shallower band.
///
/// `label_widths[i]` is the pixel width of `bands[i].label`, and `char_w` the
/// average pixel width of one character. Both are supplied by the caller
/// because only the drawing side can measure Pango text, and this module must
/// stay display-free so the packing logic is unit-testable.
///
/// A short `label_widths` degrades to a zero-width label rather than panicking:
/// a caller bug must not crash the reader mid-draw.
pub fn assign_rows(bands: &[Band], label_widths: &[f64], char_w: f64) -> Vec<usize> {
    // The horizontal extent a band actually occupies: the wider of its rule
    // and its centered label, since the label is what collides.
    let extent = |i: usize| -> (f64, f64) {
        let b = &bands[i];
        let x0 = b.start_char as f64 * char_w;
        let x1 = b.end_char as f64 * char_w;
        let lw = label_widths.get(i).copied().unwrap_or(0.0);
        let mid = (x0 + x1) / 2.0;
        let half = (lw / 2.0).max((x1 - x0) / 2.0);
        // +4px of padding so labels that merely touch still get their own row.
        (mid - half - 4.0, mid + half + 4.0)
    };

    // Visit in (depth, start) order so packing is deterministic and a band
    // never lands below a shallower one.
    let mut order: Vec<usize> = (0..bands.len()).collect();
    order.sort_by(|&a, &b| {
        bands[a]
            .depth
            .cmp(&bands[b].depth)
            .then(bands[a].start_char.cmp(&bands[b].start_char))
    });

    let mut rows = vec![0usize; bands.len()];
    // Occupied x-extents per row, in row order.
    let mut occupied: Vec<Vec<(f64, f64)>> = Vec::new();
    // The lowest row a band of a given depth may use — enforces that deeper
    // bands never sit below shallower ones.
    let mut floor_for_depth = 0usize;
    let mut current_depth = bands.get(order[0].min(bands.len().saturating_sub(1))).map(|b| b.depth);

    for &i in &order {
        let b = &bands[i];
        if current_depth != Some(b.depth) {
            // First band of a new (deeper) depth: it may not reuse any row a
            // shallower band already occupies.
            floor_for_depth = occupied.len();
            current_depth = Some(b.depth);
        }
        let (x0, x1) = extent(i);
        let mut placed = None;
        for r in floor_for_depth..occupied.len() {
            let free = occupied[r].iter().all(|&(o0, o1)| x1 <= o0 || x0 >= o1);
            if free {
                placed = Some(r);
                break;
            }
        }
        let r = match placed {
            Some(r) => r,
            None => {
                occupied.push(Vec::new());
                occupied.len() - 1
            }
        };
        occupied[r].push((x0, x1));
        rows[i] = r;
    }

    rows
}

/// Highest row index any band occupies (0 when there are none) — the drawing
/// code sizes the band stack from this.
pub fn max_row(bands: &[Band], label_widths: &[f64], char_w: f64) -> usize {
    assign_rows(bands, label_widths, char_w)
        .into_iter()
        .max()
        .unwrap_or(0)
}
```

- [ ] **Step 4: Fix the existing in-file test that calls the old signature**

`src/syntax_diagram.rs:206` calls `assign_rows(&bands)`. Read that test, then give it widths and a char width so it compiles. If the test asserts `rows == vec![1, 0]` (row equals depth), update the expectation to what packing now yields — the two bands in that test are nested, not siblings, so they still land on different rows.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --bins syntax_diagram:: 2>&1 | tail -8`

Expected: PASS — `test result: ok`, with 8 new tests.

- [ ] **Step 6: Commit**

```bash
git add src/syntax_diagram.rs
git commit -m "feat(syntax-diagram): pack band rows by label collision, not by depth"
```

---

## Task 2: Wire the packer into the overlay

`draw_analysis` currently calls `max_row(&a.bands)` and uses `b.depth` directly for vertical placement. Both must consume packed rows or the packing has no effect.

**Files:**
- Modify: `src/ui/syntax_overlay.rs:404` (the `rows` computation), `:555-585` (the draw loop), `:655` (`drawn_entries`)

**Interfaces:**
- Consumes: `assign_rows(&[Band], &[f64], f64) -> Vec<usize>`, `max_row(&[Band], &[f64], f64) -> usize` from Task 1.
- Produces: nothing new.

- [ ] **Step 1: Measure labels and char width up front, then pack**

In `draw_analysis`, replace:

```rust
    let rows = crate::syntax_diagram::max_row(&a.bands) + 1;
    let natural_line_h = {
        let (_, _, one_line_h) = layout_text(area, "X", "Serif 20", None);
        one_line_h
    };
```

with:

```rust
    // Measure once, up front: the packer needs every label's width, and the
    // draw loop below needs the same numbers. `char_w` converts a band's char
    // span to pixels so `syntax_diagram` never has to touch Pango.
    let (natural_line_h, char_w) = {
        let (_, one_char_w, one_line_h) = layout_text(area, "X", "Serif 20", None);
        (one_line_h, one_char_w)
    };
    let label_widths: Vec<f64> = a
        .bands
        .iter()
        .map(|b| {
            let (_, lw, _) = layout_text(area, &b.label, "Sans 10", None);
            lw
        })
        .collect();
    let band_rows = crate::syntax_diagram::assign_rows(&a.bands, &label_widths, char_w);
    let rows = band_rows.iter().copied().max().unwrap_or(0) + 1;
```

- [ ] **Step 2: Use the packed row in the draw loop**

The loop is `for b in &a.bands`. It needs each band's index to look up its packed row, so change it to enumerate. Replace:

```rust
    for b in &a.bands {
```

with:

```rust
    for (band_index, b) in a.bands.iter().enumerate() {
        let band_row = band_rows[band_index];
```

Then replace the depth-based offset:

```rust
            let depth_offset =
                (rows as f64 - 1.0 - b.depth as f64) * row_h_for_line(*line_index);
```

with:

```rust
            // Packed row, not raw depth — two siblings at one depth can now
            // occupy different rows (see `assign_rows`).
            let depth_offset =
                (rows as f64 - 1.0 - band_row as f64) * row_h_for_line(*line_index);
```

And the `drawn_entries` push:

```rust
            drawn_entries.push((line_index, b.depth));
```

with:

```rust
            drawn_entries.push((line_index, band_row as u8));
```

Leave `let fade = 1.0 - (b.depth as f64 * 0.15).min(0.6);` alone — fade should track semantic DEPTH (how nested the band is), not the display row, so nesting still reads through color.

- [ ] **Step 3: Verify build, tests, clippy**

Run: `cargo build 2>&1 | rg -c '^error'; cargo test --bins 2>&1 | rg 'test result'; cargo clippy 2>&1 | rg -c '^warning'`

Expected: `0` errors (or no output from the count); `1170 passed` (1162 + 8 new); clippy `181`.

- [ ] **Step 4: Commit**

```bash
git add src/ui/syntax_overlay.rs
git commit -m "feat(syntax-overlay): place bands by packed row instead of raw depth"
```

---

## Task 3: Derive row height from the window, not the text's leading

With packing in place the stack needs more rows, so it must also get more room. This is the half of the spec that reclaims the empty 83%.

**Files:**
- Modify: `src/ui/syntax_overlay.rs:21-22` (constants), `:409` (`line_spacing_for` call), `:497` (`rh` computation)

**Interfaces:**
- Consumes: `band_rows` / `rows` from Task 2.
- Produces: nothing new.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `src/ui/syntax_overlay.rs`:

```rust
    #[test]
    fn row_height_uses_the_window_budget_not_the_floor() {
        // The 2026-07-26 measurement: a 1920x1200 window left 963px free
        // below the diagram while rows were compressed toward the 16px floor.
        // A realistic band count must now get rows at the TARGET height.
        let budget = 900.0;
        for rows in 1..=8 {
            let h = row_height(rows, budget);
            assert_eq!(
                h, BAND_ROW_H,
                "{rows} rows in {budget}px must use the full target height"
            );
        }
        // Only a pathological stack falls back toward the floor.
        let deep = row_height(80, budget);
        assert!(deep < BAND_ROW_H, "80 rows in {budget}px must shrink");
        assert!(deep >= MIN_BAND_ROW_H, "but never below the legibility floor");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --bins row_height_uses_the_window 2>&1 | tail -12`

Expected: FAIL. With `BAND_ROW_H = 26.0`, `row_height(8, 900.0)` returns 26 already — but `rows=1..8` at the CURRENT `BAND_ROW_H` passes trivially, so this test only starts failing after Step 3 raises the target. If it passes here, that is expected; the assertion that matters is that it still passes at the raised target.

- [ ] **Step 3: Raise the row target and feed it the window budget**

In `src/ui/syntax_overlay.rs`, raise the target:

```rust
/// Natural height of one band row.
///
/// Raised from 26 to 40 (2026-07-26 layout pass): the stack now draws into the
/// window's free space rather than the font's leading, and at 26px a five-band
/// diagram used 7% of a 1200px window while 963px sat empty. This is a TARGET,
/// not a ceiling — `row_height` still shrinks toward `MIN_BAND_ROW_H` when a
/// stack is genuinely deep enough to exhaust the budget.
const BAND_ROW_H: f64 = 40.0;
```

Then make the stack's height come from the window rather than the text leading. Replace:

```rust
    let line_h = line_spacing_for(rows, natural_line_h, LABEL_CLEARANCE + STACK_TOP_OFFSET);
```

with:

```rust
    // Interior wrapped lines still need enough leading that their own stack
    // does not overstrike the NEXT line — that is what `line_spacing_for`
    // guards. But the stack's row height itself now comes from the window's
    // free space (`onscreen_budget` below), not from this leading, so a deep
    // stack grows DOWN into the empty part of the window instead of being
    // compressed into the gap between two lines.
    let line_h = line_spacing_for(rows, natural_line_h, LABEL_CLEARANCE + STACK_TOP_OFFSET);
```

and replace the `rh` computation:

```rust
    let rh = interior_row_height(rows, line_h, LABEL_CLEARANCE + STACK_TOP_OFFSET)
        .min(row_height(rows, onscreen_budget));
```

with:

```rust
    // The window budget is the PRIMARY constraint now, not a cap on a
    // leading-derived height. `interior_row_height` still applies on a
    // MULTI-LINE selection, where a line's stack must fit above the next
    // line; on a single-line selection there is no next line, so the full
    // on-screen budget is available.
    let rh = if pango_lines.len() > 1 {
        interior_row_height(rows, line_h, LABEL_CLEARANCE + STACK_TOP_OFFSET)
            .min(row_height(rows, onscreen_budget))
    } else {
        row_height(rows, onscreen_budget)
    };
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --bins 2>&1 | rg 'test result|FAILED'`

Expected: `1171 passed` (1162 + 8 from Task 1 + 1 here). If `interior_row_height_fits_the_reported_regression` or `band_row_height_shrinks_to_fit_available_space` now fail, read them: they pin geometry at the OLD `BAND_ROW_H`. Update their expected values to the new target — do not delete them, and do not lower `BAND_ROW_H` to make them pass.

- [ ] **Step 5: Verify clippy**

Run: `cargo clippy 2>&1 | rg -c '^warning'`

Expected: `181`.

- [ ] **Step 6: Commit**

```bash
git add src/ui/syntax_overlay.rs
git commit -m "feat(syntax-overlay): size band rows from the window, not the text leading"
```

---

## Task 4: On-screen verification

Mandatory per CLAUDE.md and the spec, and the ONLY gate that has caught these defects — build, clippy and 1162 unit tests were all green while the diagram was unreadable.

**Files:**
- Modify: `docs/superpowers/plans/2026-07-26-syntax-diagram-layout.md` (record results)

- [ ] **Step 1: Build and launch headless**

```bash
cd ~/utono/linux-lit-wt/<branch> && cargo build
```

Launch with the env wrapper, which mints a fresh `XDG_RUNTIME_DIR` (a bare cage run on `/run/user/1000` screenshots the USER'S desktop). Use the harness `run_in_background` — a detached or `timeout`-wrapped launch dies immediately:

```bash
./scripts/land-on.sh BH-Barrett 3.0
```

Note: BH-Barrett uses `div2=0`, so `3.0` is valid and `1.1` is not. Take the printed `XDG_RUNTIME_DIR` from the output; do not assume it.

- [ ] **Step 2: Drive to a diagram**

```bash
export XDG_RUNTIME_DIR=<printed>  WAYLAND_DISPLAY=wayland-0
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 2
wtype -k Escape          # re-send after resize: the first chord is dropped
for i in $(seq 1 12); do wtype -k j; sleep 0.25; done
wtype -k minus
sleep 1
wtype -k Return
sleep 30
grim -o HEADLESS-1 /tmp/layout-1.png
```

- [ ] **Step 3: Measure the vertical extent**

The spec's headline claim is quantitative, so verify it that way rather than by eye:

```bash
python3 -c "
from PIL import Image
im = Image.open('/tmp/layout-1.png').convert('RGB')
W,H = im.size
bg = im.getpixel((50, H//2))
rows=[]
for y in range(40, H):
    row = [im.getpixel((x,y)) for x in range(300, 1600, 4)]
    if any(abs(p[0]-bg[0])+abs(p[1]-bg[1])+abs(p[2]-bg[2]) > 40 for p in row):
        rows.append(y)
print('content bands:', rows[0], '..', rows[-1])
"
```

Expected: the diagram's own band (excluding the legend near y=1137) now extends well past y=174 — the pre-change measurement was y=91..174, 7% of the window.

- [ ] **Step 4: Open the PNG and report what you see**

Per the UI review protocol a passing exit code is not enough. Read the capture and state inline:

1. The leftmost band's label clears its rule — the defect that motivated this plan.
2. No label overprints another label, a rule, or a POS tag.
3. The stack uses the vertical space rather than the top 7%.
4. The POS legend at the bottom is clear of the stack.

- [ ] **Step 5: Verify the wrapped multi-line case**

Land on a longer sentence (the "For a pipe," says Mr. George… passage wraps to two lines) and confirm each line's stack sits under ITS OWN line, not over the next. This is the regression `line_spacing_for` guards and the one Task 3 is most likely to break.

- [ ] **Step 6: Clean up**

Scoped only — a bare `pkill -f target/debug/linux-lit` kills the user's live instance. Run as its own step; `pkill` exits nonzero on no match and aborts an `&&` chain:

```bash
pkill -f "cage -- target/debug/linux-lit" || true
```

- [ ] **Step 7: Record results and commit**

Append a "## Verification results" section to this plan with the measured extent and what each capture showed, then:

```bash
git add docs/superpowers/plans/2026-07-26-syntax-diagram-layout.md
git commit -m "docs(plan): syntax diagram layout verification results"
```

- [ ] **Step 8: Hand off for real-renderer confirmation**

Cage is software rendering and disagreed with GL on **every** defect fixed on 2026-07-26. Give the user the command and the four criteria from Step 4:

```bash
cd ~/utono/linux-lit-wt/<branch> && cargo run
```

Do NOT merge before the user confirms on their renderer.

---

## Task 5: Update the clipping ledger

Required by CLAUDE.md: any change addressing a clipping/overlap defect updates `clip-prevention.md` in the SAME change.

**Files:**
- Modify: `docs/troubleshooting/clip-prevention.md` (entry 15)

- [ ] **Step 1: Append the resolution**

Entry 15 currently ends with the GL-vs-cage lesson. Append:

```markdown
    **RESOLVED (2026-07-26 layout pass).** The remaining sibling-label
    collision was not a spacing bug at all — `assign_rows` returned `b.depth`
    verbatim, so every band at one depth shared a row regardless of horizontal
    position, and five disjoint appositives overprinted each other. Packing
    rows by LABEL extent (not rule extent, and not depth) fixed it at the
    source. The generalizable lesson: when repeated spacing fixes each expose
    another collision, check whether the ROW ASSIGNMENT is wrong before tuning
    the geometry further — six constants were adjusted before the actual cause
    was found one layer up. The second half of the same pass stopped deriving
    row height from the font's leading (which starved the stack to 7% of the
    window while 83% sat empty) and derived it from the window's free height
    instead.
```

- [ ] **Step 2: Commit**

```bash
git add docs/troubleshooting/clip-prevention.md
git commit -m "docs(clip-prevention): record the row-packing root cause"
```

---

## Self-Review

**Spec coverage.** Section 1 (row height from available space) → Task 3. Section 2 (pack rows by collision) → Tasks 1 and 2. Section 3 (boundaries: `assign_rows` stays pure, widths measured by caller) → Task 1's signature and Task 2 Step 1. Section 4 (unit + on-screen testing) → Tasks 1 and 4. Section 5 (non-goals) → Global Constraints. The ledger update is required by CLAUDE.md rather than the spec, hence Task 5.

**Placeholder scan.** No TBDs. Every code step shows the actual code; every command shows expected output. Task 1 Step 4 says "read that test, then…" rather than quoting it, because the test at `src/syntax_diagram.rs:206` must be read at execution time to see its current assertion — that is a genuine read-then-edit, not a vague instruction, and the expected outcome is stated.

**Type consistency.** `assign_rows(&[Band], &[f64], f64) -> Vec<usize>` and `max_row(&[Band], &[f64], f64) -> usize` are defined in Task 1 and called with exactly those types in Task 2. `band_rows: Vec<usize>` is produced in Task 2 Step 1 and indexed in Step 2. `char_w` and `label_widths` are introduced together in Task 2 Step 1, before their first use.

**One deliberate non-obvious decision** flagged for the reviewer: Task 2 keeps `fade` keyed on `b.depth` while placement moves to `band_row`. Depth is the semantic quantity (how nested), the row is a display artifact — tying color to the row would make two siblings at one depth render in different shades.

**Known risk.** Task 3 changes how `rh` is derived on single-line selections. The multi-line guard is retained explicitly, and Task 4 Step 5 exists to catch a regression there — that is the one place this plan could reintroduce the overstrike defect `line_spacing_for` was written for.
