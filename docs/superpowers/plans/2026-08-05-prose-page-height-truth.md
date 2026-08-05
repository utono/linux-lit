# Prose Page Height Truth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make prose page generation measure true wrapped line heights, so stored tables stop pinning pages that overflow the card.

**Architecture:** `record_prose_pages` builds the page grid from a buffer-wide `line_yrange` sweep whose synchronous-validation premise fails for lines far from the viewport. An independent Pango measurement already exists in the file as a diagnostic (`LIT_TRACE_PANGO`). Task 1 measures whether Pango is trustworthy on known-good lines; that evidence selects Option A (adopt Pango) or Option B (force validation first). Everything downstream is shared.

**Tech Stack:** Rust, GTK4 + `sourceview5`, `pango`, SQLite (`rusqlite`), `cargo test`, headless e2e via cage/grim/wtype.

**Spec:** `docs/superpowers/specs/2026-08-05-prose-page-height-truth-design.md`
**Evidence:** `ROOT-CAUSE.md`, `TRACE-FINDINGS.md`, `DIAGNOSIS.md` (this worktree)

## Global Constraints

- **Target is `master`.** Work in a worktree off master, not in the user's main checkout `/home/mlj/utono/linux-lit`.
- **Never run `cargo run`** — the user launches the app. Verify with `cargo build` / `cargo test`.
- **A `cargo build` reporting "Finished" in 0.1–0.2s after an edit is usually a PRIOR build.** Confirm with `stat -c '%Y' target/debug/linux-lit src/input/prose_pages.rs`.
- `cargo clippy` must stay clean. `cargo clippy --all-targets` has a PRE-EXISTING deny-level error at `src/db/queries.rs:2456`, unrelated — plain `cargo clippy` is the gate.
- **Play/verse `play_pages` is OUT OF SCOPE and must not change behavior.** Verified by the nav-fuzz at the end.
- **Cage-backed test binaries run ONE AT A TIME, single-threaded, with a pause between.** A failure in a batch run is not evidence until it reproduces in isolation.
- **Never poll for a launch with a bare `until … done`** — bound every wait with `timeout`, or orphaned shells outlive the run.
- Find debug logs BY MTIME (`ls -lt *.log`); they clear per launch and instance slots ≥2 get `-{n}` suffixes.
- Cleanup: `pkill -f "cage -- ./target/debug/linux-lit"` (scoped — a bare `pkill -f target/debug/linux-lit` kills the user's own instance).
- **Generation writes to lit.db.** Do not run other lit.db writers concurrently.
- Production geometry is **1920x1200 → `text_view` 1128, `uh1071`** for the fuzz/live path. The cargo harness uses 1236 → 1098. **Never port the number between harnesses**; assert the achieved height in the log.

---

## File Structure

**Modified:**
- `src/input/prose_pages.rs` — the measurement source, the convergence guard, the fingerprint bump
- `tests/prose_page_fit.rs` — deep-chapter repro + determinism test (read before editing; it is the existing prose suite)

**Created:**
- none expected. If Task 1 selects Option B, it may add a helper module — decide then.

---

### Task 1: Measure whether Pango is the oracle (EVIDENCE ONLY — no fix)

This task changes no production behavior. It answers the one question the spec refuses to assume, and its result selects Option A or B for Task 2.

**The question:** on lines that have been **fully displayed** — where `line_yrange` is known good — does the Pango layout agree with it?

**Files:**
- Modify: `src/input/prose_pages.rs` (extend the existing `LIT_TRACE_PANGO` block at ~line 490)

**Interfaces:**
- Consumes: nothing.
- Produces: a log line partitioning disagreement by whether the line was displayed. No API.

- [ ] **Step 1: Read the existing probe**

```bash
rg -n "LIT_TRACE_PANGO" -A 55 src/input/prose_pages.rs
```

It builds a Pango layout per line at `wrap_w = tv.width() - left_margin - right_margin`, sets the font from the buffer-wide `font-size` tag (NOT the view context — that reports the CSS default and would measure the wrong face), and computes `pango_h = layout.pixel_size().1 + above + below`. Note that formula; Task 2 reuses it verbatim.

- [ ] **Step 2: Partition the comparison by display state**

Extend the probe to classify each line as DISPLAYED or NOT, and report the two populations separately. "Displayed" means the line has been inside the rendered region — approximate it by the visible range around the viewport at generation time (`state.page_top.line()` ± the visible line count), and say in the log which definition you used.

Emit, alongside the existing summary:

```
PAGES_PROSE_PANGO_SPLIT: displayed_lines=N displayed_disagree=K displayed_delta_sum=D \
  offscreen_lines=M offscreen_disagree=J offscreen_delta_sum=E \
  displayed_ex=[...] offscreen_ex=[...]
```

- [ ] **Step 3: Run it at production geometry on the failing chapter**

Follow `tests/prose_page_fit.rs`'s `drive_forward_from` launch pattern. Env:

```
LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_START_WORK=BH-Barrett LIT_START_SCENE=37.0
LIT_GEN_PAGE_TABLE=1 LIT_TRACE_PANGO=1
```

Resize to 1920x1200 and CONFIRM `RESIZE_TICK: text_view.height changed … -> 1128` before trusting anything.

- [ ] **Step 4: Interpret and record the verdict**

- **`displayed_disagree` ≈ 0** → `line_yrange` and Pango agree wherever `line_yrange` is trustworthy, so the offscreen disagreements are `line_yrange` being wrong. **Pango is the oracle → Option A.**
- **`displayed_disagree` is large** → the two measure different things (Pango is likely missing a tag or spacing the view applies). **Option B**, and report exactly what the systematic difference looks like.

Write the verdict and the raw log lines to `PANGO-VERDICT.md` at the repo root. This file is the input to Task 2.

- [ ] **Step 5: Commit**

```bash
cargo build 2>&1 | tail -3
cargo clippy 2>&1 | tail -3
git add src/input/prose_pages.rs PANGO-VERDICT.md
git commit -m "diag(pagination): split the Pango probe by display state

Answers whether Pango agrees with line_yrange where line_yrange is known
good, which decides whether Pango can be the generation-time measurement."
```

---

### Task 2A: Adopt Pango as the generation measurement (ONLY IF Task 1 says Option A)

> **If Task 1 selected Option B, SKIP this task entirely and do Task 2B.**

**Files:**
- Modify: `src/input/prose_pages.rs` — the `sweep1` construction at ~line 474

**Interfaces:**
- Consumes: Task 1's verdict.
- Produces: `prose_pages::measure_line_heights(state) -> Vec<i32>`, the single source of generation-time heights. Task 3's guard and Task 4's census both call it.

- [ ] **Step 1: Write the failing test**

The determinism property is the one that proves this fix. Add to `tests/prose_page_fit.rs` (read the file first and match its harness conventions):

```rust
/// Generation must be a pure function of content + geometry. Before the Pango
/// fix the same fingerprint produced 801, 806 and 808 pages on three runs,
/// because the height sweep read whatever GTK happened to have validated.
#[test]
#[ignore]
fn prose_generation_is_deterministic_across_runs() {
    let mut counts = Vec::new();
    for _ in 0..3 {
        let log = generate_table_at(1920, 1236, "BH-Barrett", Some("37.0"));
        counts.push(parse_generated_page_count(&log)
            .expect("PAGES_PROSE: generated N pages must appear"));
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    assert!(
        counts.iter().all(|c| *c == counts[0]),
        "generation is not deterministic: {counts:?} — same content, same \
         geometry, different tables"
    );
}
```

You must write `generate_table_at` and `parse_generated_page_count` against the file's real harness API (`Harness::start_app`, `set_output_size`, `read_dev_log`). Do NOT invent harness functions — verify each against `tests/harness/mod.rs`.

- [ ] **Step 2: Run it and confirm it FAILS**

```bash
./scripts/e2e-env.sh cargo test --test prose_page_fit \
  prose_generation_is_deterministic_across_runs -- --ignored --test-threads=1 --nocapture 2>&1 | tail -30
```

Expected: FAIL with differing counts. If it passes on master, STOP and report — the race may be geometry-specific and the premise needs re-examining.

- [ ] **Step 3: Extract the measurement**

Add above `record_prose_pages`:

```rust
/// True wrapped height of every line, measured independently of GTK's
/// viewport-gated layout validation.
///
/// `line_yrange` returns a provisional estimate for lines far from the
/// viewport and GTK's cache treats it as final, so a sweep taken at generation
/// under-measures exactly the lines that have never been displayed — by whole
/// rows. Two sweeps agree with each other because they read the same stale
/// cache, which is why the convergence guard could not see it.
///
/// Pango is what the view itself renders from, and a layout built here does not
/// depend on scroll position, so generation becomes a pure function of content
/// and geometry.
pub(crate) fn measure_line_heights(state: &crate::app::AppState) -> Vec<i32> {
    use gtk4::prelude::{TextBufferExt, WidgetExt};
    let tv = &state.text_view;
    let wrap_w = tv.width() - tv.left_margin() - tv.right_margin();
    let above = tv.pixels_above_lines();
    let below = tv.pixels_below_lines();
    // The body font is the buffer-wide `font-size` TextTag, NOT the view's
    // context (which still reports the CSS default). Measuring with the wrong
    // face makes every line disagree.
    let font = pango::FontDescription::from_string(&crate::ui::font_string(
        state.config.font_family.as_str(),
        state.config.font_size as i32,
    ));
    let line_count = state.effective_line_count();
    (0..line_count)
        .map(|i| {
            let Some(start) = state.buffer.iter_at_line(i as i32) else {
                return 0;
            };
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            let text = state.buffer.text(&start, &end, false);
            let layout = tv.create_pango_layout(Some(text.as_str()));
            layout.set_width(wrap_w * pango::SCALE);
            layout.set_wrap(pango::WrapMode::WordChar);
            layout.set_font_description(Some(&font));
            layout.pixel_size().1 + above + below
        })
        .collect()
}
```

- [ ] **Step 4: Use it as the sweep**

Replace the `sweep1` construction (~line 474) with:

```rust
    let sweep1: Vec<i32> = measure_line_heights(state);
```

Keep the surrounding comments but UPDATE them — the old text asserts that `line_yrange` "validates the line synchronously and GTK caches the result," which this change exists to refute. Leave the font-in-effect preamble above it intact; it is a separate, still-valid fix.

- [ ] **Step 5: Confirm the test now PASSES**

```bash
cargo build 2>&1 | tail -3
./scripts/e2e-env.sh cargo test --test prose_page_fit \
  prose_generation_is_deterministic_across_runs -- --ignored --test-threads=1 --nocapture 2>&1 | tail -20
```

- [ ] **Step 6: Measure the cost**

Per-line Pango across ~7,300 lines may be slow, and generation is on the load/resize path. Time it before and after:

```bash
rg -n "PAGES_PROSE: generated" *.log | tail -3
```

Report generation wall-clock both ways in your report. If it regressed by more than ~2x, say so explicitly rather than letting the user discover it.

- [ ] **Step 7: Commit**

```bash
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | tail -3
git add src/input/prose_pages.rs tests/prose_page_fit.rs
git commit -m "fix(pagination): measure prose line heights with Pango, not line_yrange

line_yrange returns a provisional estimate for lines far from the viewport
and GTK caches it as final, so generation under-measured exactly the lines
never displayed — by whole rows. Two sweeps agreed because both read the
same stale cache.

Pango does not depend on scroll position, so generation is now a pure
function of content and geometry. Covered by a determinism test that failed
before this change (801/806/808 pages for one fingerprint)."
```

---

### Task 2B: Force validation before sweeping (ONLY IF Task 1 says Option B)

> **If Task 1 selected Option A, SKIP this task — Task 2A covers it.**

If Pango does not agree with `line_yrange` on displayed lines, adopting it would trade one wrong measurement for another. Instead force GTK to validate every line before the sweep.

**Files:**
- Modify: `src/input/prose_pages.rs`

- [ ] **Step 1: Write the same determinism test as Task 2A Step 1** (identical code — it is the property, not the mechanism, that is being asserted) and confirm it FAILS.

- [ ] **Step 2: Find a validation lever.** Candidates, in order of preference:
  - a GTK API that validates a range synchronously (search the `gtk4`/`sourceview5` bindings for a `validate`-style call reachable from safe Rust);
  - scrolling the view through the buffer in viewport-sized steps before sweeping, pumping the main context at each step so layout settles.

  **Report which lever you used and why**, and note that scrolling is O(buffer) full re-layouts — the doc comment above `record_prose_pages` says this was already rejected once as far too slow on a novel. If it is unacceptably slow, STOP and report rather than shipping a regression.

- [ ] **Step 3: Sweep only after validation**, keep the existing `sweep1` shape, and confirm the determinism test passes.

- [ ] **Step 4: Measure and report generation wall-clock**, as in Task 2A Step 6.

- [ ] **Step 5: Commit** with a message naming the lever and the measured cost.

---

### Task 3: Make the convergence guard compare against ground truth

The guard currently sweeps twice and compares the results. Both sweeps read the same cache, so it proves self-consistency and nothing more — it logged `changed_between_sweeps=0` and `delta_sum=0` on the very run whose render disagreed by 225px.

**Files:**
- Modify: `src/input/prose_pages.rs` — the convergence-guard block (search `CONVERGENCE GUARD`)

**Interfaces:**
- Consumes: `measure_line_heights` (Task 2A) or the validated sweep (Task 2B).
- Produces: no new API; changes when `VALIDATE_FAIL` fires.

- [ ] **Step 1: Read the guard**

```bash
rg -n "CONVERGENCE GUARD" -A 40 src/input/prose_pages.rs
```

- [ ] **Step 2: Write a unit test for the comparison**

The comparison is pure arithmetic over two vectors — test it as such, without GTK. Follow the file's existing `#[cfg(test)] mod <subject>_tests` convention (there is no single `mod tests`).

```rust
#[cfg(test)]
mod height_agreement_tests {
    use super::heights_disagree;

    #[test]
    fn identical_vectors_agree() {
        assert!(!heights_disagree(&[40, 68, 96], &[40, 68, 96], 2));
    }

    #[test]
    fn a_whole_row_of_drift_is_a_disagreement() {
        // +28 is one wrapped row — the exact signature of the lazy-validation
        // bug this guard exists to catch.
        assert!(heights_disagree(&[40, 68, 96], &[40, 96, 96], 2));
    }

    #[test]
    fn sub_tolerance_jitter_is_accepted() {
        assert!(!heights_disagree(&[40, 68], &[41, 69], 2));
    }

    #[test]
    fn a_length_mismatch_is_a_disagreement() {
        assert!(heights_disagree(&[40, 68], &[40], 2));
    }
}
```

- [ ] **Step 3: Run it, confirm it fails** (`cannot find function heights_disagree`).

- [ ] **Step 4: Implement**

```rust
/// Whether two per-line height vectors disagree beyond `tolerance_px` on any
/// line. Used to refuse storing a table built on heights that do not match an
/// independent measurement — a stronger check than the old two-sweep
/// comparison, which read the same cache twice and so always agreed.
pub(crate) fn heights_disagree(a: &[i32], b: &[i32], tolerance_px: i32) -> bool {
    a.len() != b.len()
        || a.iter().zip(b).any(|(x, y)| (x - y).abs() > tolerance_px)
}
```

Then change the guard: compare the generation heights against the **other** measurement (`line_yrange` if Task 2A adopted Pango, or vice versa) rather than against a second identical sweep. On disagreement, log `VALIDATE_FAIL` with the worst offending line and fall back to the live engine instead of storing.

Keep the existing `PAGES_PROSE_SWEEP:` log line's shape where practical so existing log-greps keep working, but make its numbers mean the new comparison.

- [ ] **Step 5: Confirm tests pass, then commit**

```bash
cargo test --bins height_agreement_tests 2>&1 | tail -10
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | tail -3
git add src/input/prose_pages.rs
git commit -m "fix(pagination): validate generated heights against an independent measure

The old guard swept twice and compared the results — both reads hit the same
GTK cache, so it reported delta_sum=0 on a run whose render disagreed by
225px. validated=1 must mean validated against ground truth."
```

---

### Task 4: Bump the fingerprint to pv8

Every stored prose table was generated from the flawed sweep, so all are suspect. The bump is bookkeeping — it only matters because Tasks 2–3 made generation deterministic.

**Files:**
- Modify: `src/input/prose_pages.rs` — `prose_layout_fingerprint` (~line 292)

- [ ] **Step 1: Change the version**

The function currently ends with `format!("{base}|uh{usable}|cw{cw}|pv7")`. Change `pv7` → `pv8` and add a comment in the style of the existing pv-history block:

```rust
    // pv8: generation measures true wrapped heights independently of GTK's
    // viewport-gated layout validation. EVERY table at pv7 or earlier was built
    // from a `line_yrange` sweep that under-measured lines never displayed at
    // generation time — by whole rows — so those tables pin pages that render
    // taller than the card. The bump evicts them. Note the pv7 bump alone did
    // NOT fix this: regeneration re-rolled the same race, which is why the
    // determinism test, not the bump, is what proves this fixed.
```

- [ ] **Step 2: Confirm the existing fingerprint tests still pass**

```bash
rg -n "pv7|prose_layout_fingerprint" tests/ src/ --type rust | rg -i "test|assert" | head
cargo test --bins 2>&1 | tail -3
```

If any test asserts the literal `pv7`, update it to `pv8` — that is a legitimate update, not weakening a test.

- [ ] **Step 3: Commit**

```bash
git add src/input/prose_pages.rs
git commit -m "fix(pagination): bump prose fingerprint to pv8

Evicts every table built from the pre-Pango sweep. Bookkeeping only — the
determinism fix is what makes regeneration trustworthy."
```

---

### Task 5: Deep-chapter regression test

The prior fix's acceptance test lands on chapter 26, which passes; chapter 37 was never re-verified, and 22 pages there overflow by 13–114px. That is exactly how this defect stayed green.

**Files:**
- Modify: `tests/prose_page_fit.rs`

- [ ] **Step 1: Read the existing deep test**

```bash
rg -n "LIT_START_SCENE|drive_forward_from|26\.0" tests/prose_page_fit.rs
```

- [ ] **Step 2: Write the failing test**

```rust
/// Chapter 37 overflowed 22 pages by 13-114px while the chapter-26 test above
/// passed green. A deep sample of ONE chapter is not coverage — this pins the
/// chapter that actually failed for the user.
#[test]
#[ignore]
fn chapter_37_pages_never_overflow_the_card() {
    let log = drive_forward_from(20, Some("37.0"));
    let overflows: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("CLIP_WARN") && l.contains("OVERFLOW"))
        .collect();
    assert!(
        overflows.is_empty(),
        "chapter 37 overflowed {} page(s):\n{}",
        overflows.len(),
        overflows.join("\n")
    );
}
```

- [ ] **Step 3: Confirm it FAILS on the pre-fix code**

Stash your Task 2–4 changes, run it, confirm it reports overflows, then restore:

```bash
git stash && ./scripts/e2e-env.sh cargo test --test prose_page_fit \
  chapter_37_pages_never_overflow_the_card -- --ignored --test-threads=1 --nocapture 2>&1 | tail -20
git stash pop
```

A test that has never been seen red proves nothing.

- [ ] **Step 4: Confirm it passes with the fix, then commit**

---

### Task 6: Whole-table census and verse regression

- [ ] **Step 1: Assert the census, not just a sampled drive**

A 20-turn drive visits ~20 of ~800 pages. `record_prose_pages` already emits a whole-table census:

```
PAGES_PROSE_DRIFT: summary pages=N over_usable=K worst=page P at Xpx usable=U slack=S
```

Add an assertion that `over_usable == 0` after generation. **Caveat to state in your report:** historically this census read the same heights generation used, so it agreed with itself and could not see this bug. It is meaningful now only because Task 2 changed the measurement — say so, and keep the render-side chapter-37 test (Task 5) as the independent check.

- [ ] **Step 2: Verify verse/play pagination is untouched**

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Cym --secs 120
```

`--start-work` is REQUIRED — without it the run rewrites the dev config's `last_work`. Expect no UNBALANCED or short-column failures. This harness wants **1920x1200 → text_view 1128**; verify the achieved height in the log rather than porting the cargo harness's 1236/1098.

- [ ] **Step 3: Run the full e2e suite, one binary at a time**

```bash
for t in prose_page_fit prose_row_fill line_clipping niri_smoke; do
  ./scripts/e2e-env.sh cargo test --test $t -- --ignored --test-threads=1 2>&1 | tail -5
  sleep 3
done
```

- [ ] **Step 4: Open every capture in `target/ui/`** and report what you see — quote on-screen text, and confirm the bottom line sits clear of the card rule. A passing exit code is not enough.

- [ ] **Step 5: Commit any test additions**

---

### Task 7: Documentation and hand-off

- [ ] **Step 1: Update the clip-prevention ledger**

`docs/troubleshooting/clip-prevention.md` — APPEND an entry (do not restructure or renumber; the file has recent entries from two sessions). It must record:
- the tell: `CLIP_WARN … OVERFLOW` on a `table hit` page, deep in a prose work;
- the root cause: `line_yrange` returns provisional heights for lines far from the viewport, and a second sweep cannot detect it because both read the same cache;
- the diagnostic: the three-way `GEN_HEIGHTS` / `POSTWALK_HEIGHTS` / `RENDER_HEIGHTS` diff via `LIT_TRACE_HEIGHTS=<a>:<b>`, and `LIT_TRACE_PANGO=1` for ground truth;
- the giveaway that it is THIS bug and not a stale table: **the same fingerprint yields different page counts across runs.**

- [ ] **Step 2: Note the lesson in `CLAUDE.md`** if it is not already covered: a convergence check that compares a measurement against a second copy of itself proves self-consistency, not correctness.

- [ ] **Step 3: Hand the user the real-renderer check**

Cage is software rendering and has disagreed with the real GL renderer before. Give exact steps: open BH-Barrett, jump to chapter 37, confirm the bottom line sits clear of the rule, and check a few later chapters too.

- [ ] **Step 4: Commit the docs**

---

## Notes for the implementer

- **Do not add a third sweep.** Two already agree with each other while both are wrong; a third would too. The whole point is an *independent* measurement.
- **The font-in-effect preamble in `record_prose_pages` is a SEPARATE, still-valid fix** (pv5→pv7). Leave it. This plan addresses the lazy-validation frontier, which that fix's own comment explicitly names as a distinct problem it does not solve.
- **`page_px` and `exact_page_content_height` are algebraically identical at `end_off == 0`.** If they ever disagree, the heights differ between the two moments — never the arithmetic. Do not go looking for an arithmetic bug.
- **The determinism test is the one that matters.** Page-count equality across runs is the property that distinguishes "fixed" from "re-rolled the dice." If it is flaky, the root cause is not addressed, whatever the other tests say.
- Log lines this work touches: `PAGES_PROSE_PANGO`, `PAGES_PROSE_PANGO_SPLIT`, `PAGES_PROSE_SWEEP`, `PAGES_PROSE_DRIFT`, `GEN_HEIGHTS`, `POSTWALK_HEIGHTS`, `RENDER_HEIGHTS`, `CLIP_WARN`.
