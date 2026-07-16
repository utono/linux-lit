# Final-Spread Anchor Coverage Implementation Plan

> **SUPERSEDED (2026-07-16)** — see the design doc's superseded banner. The
> anchor was never wrong (it covers the last DIALOGUE line; the tail is a
> trailing stage direction). The real fix was a nav-fuzz assertion exemption,
> not `last_page_top`. Do NOT execute this plan; kept for the record.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make `last_page_top` (the two-column final-page anchor) always cover the work's last line, even when the fuller columns leave only a 1–2 line tail — fixing the G/search orphan regression the fill-reserve change exposed.

**Architecture:** Add a coverage fallback in `last_page_top`'s two-column branch: after the existing "prefer non-empty right" pull picks `chosen`, if `chosen`'s spread doesn't reach the last line, replace it with the earliest page whose `column_split(top).page_end >= line_count - 1` (empty right allowed). Idempotent by construction; inert on the common case. `record_spreads` inherits the fix via `last_page_top`.

**Tech Stack:** Rust, GTK4/sourceview5, cage/grim/wtype headless harness.

## Global Constraints

- Design + verify at production 1920×1200. Verify across LLL-Arkangel + ≥2 other two-column plays.
- `last_page_top` MUST stay idempotent (nav-fuzz `JUMP-TO-END not idempotent` guard). The fallback's "earliest covering top" is a pure function of layout + line_count, never of start/target.
- Do NOT run `cargo run`. Verify via `cargo build`, `cargo test --bins`, headless cage.
- Cage's nested wayland socket is a DIFFERENT `wayland-N` than host; resize with `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`. Cleanup ONLY `pkill -f "cage -- ./target/debug/linux-lit"`.
- Table gen is one-shot per session and happens on load at the CURRENT geometry — to test at 1920×1200, use `LIT_NO_PAGE_TABLE=1` (live engine, recomputes at current geometry) OR resize before load.

---

### Task 1: Add the coverage fallback to `last_page_top`

**Files:**
- Modify: `src/input/navigation.rs` — `last_page_top`, right after `let chosen = last_full.unwrap_or(top);` (~line 567), before the `clamp_page_top_to_scroll_ceiling` comment/call.

**Interfaces:**
- Consumes: `super::viewport::column_split`, `super::viewport::next_page_top`, `line_count`, `chosen` (all in scope).
- Produces: a possibly-updated `chosen` that always satisfies `column_split(chosen).page_end >= line_count - 1`.

- [ ] **Step 1: Reproduce the failure (baseline)**

Run the fixed-seed nav-fuzz on the branch to confirm the orphan still fails:
```bash
cd ~/utono/linux-lit && timeout 90 ./scripts/e2e-env.sh \
  .claude/skills/test-headless-navigation/run-fuzz.sh \
  --start-work LLL-Arkangel --secs 60 --seed 11400714819323198485 2>&1 | rg "\[fuzz\] done"
rg -i "NAV_TEST: FAIL" /tmp/fuzz-nav.log
```
Expected: `1 failures`, `SearchJump viewport fill 8% < 10% (top=4248 last=4249 ...)`.

- [ ] **Step 2: Implement the coverage fallback**

In `src/input/navigation.rs`, change `let chosen = last_full.unwrap_or(top);` to `let mut chosen = ...` and insert the fallback immediately after:

```rust
        let mut chosen = last_full.unwrap_or(top);
        // COVERAGE FALLBACK: the anchor MUST render through the work's last line.
        // The pull-forward above prefers a NON-EMPTY-right final spread; when the
        // fuller two-column fill (TWO_COLUMN_BOTTOM_MARGIN) leaves only a 1-2 line
        // tail, no such spread reaches the end, so `chosen` stops one spread short
        // and G / search / page-forward land on a degenerate tail (the fill-reserve
        // regression). If `chosen` does not cover the last line, fall back to the
        // EARLIEST spread whose left column reaches it — accepting an empty right
        // column (a short tail in a lone left column, the pre-reserve-change shape).
        // Earliest (not last) maximizes how full that final left column is.
        // Idempotent: the covering spread's page_end already >= last_line, so a
        // recompute re-selects it; the search is a pure function of layout +
        // line_count, never of the start position.
        let last_line = line_count - 1;
        if super::viewport::column_split(state, chosen).page_end < last_line {
            // Walk the forward chain from line 0 to the FIRST spread that covers
            // the last line. Starting at 0 (a genuine boundary) keeps the result
            // start-independent — the idempotency requirement.
            let mut t = 0usize;
            let mut cover_guard = 0;
            loop {
                if super::viewport::column_split(state, t).page_end >= last_line {
                    chosen = t;
                    break;
                }
                let n = super::viewport::next_page_top(state, t).new_top;
                if n <= t || n >= line_count {
                    // Chain exhausted without covering (shouldn't happen for an
                    // in-bounds last line) — keep the original chosen.
                    break;
                }
                t = n;
                cover_guard += 1;
                if cover_guard > line_count {
                    break;
                }
            }
        }
```

Leave the existing `clamp_page_top_to_scroll_ceiling(state, chosen)` call unchanged after this block.

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | rg -i "^error|Finished" | tail -2`
Expected: `Finished`.

- [ ] **Step 4: Confirm the fixed-seed fuzz now passes**

```bash
timeout 90 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh \
  --start-work LLL-Arkangel --secs 60 --seed 11400714819323198485 2>&1 | rg "\[fuzz\] done"
rg -i "NAV_TEST: FAIL" /tmp/fuzz-nav.log
```
Expected: `0 failures`, no FAIL lines.

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "fix(pagination): final anchor must cover the last line (empty-right tail)"
```

---

### Task 2: Verify coverage at production geometry (temporary probe)

**Files:**
- Temporarily modify then revert: `src/input/navigation.rs` (a one-line `ANCHORDBG` log).

- [ ] **Step 1: Add a temporary coverage log**

After the fallback block, insert:
```rust
        crate::logging::log(&format!(
            "ANCHORDBG: chosen={chosen} page_end={} last_line={} covers={}",
            super::viewport::column_split(state, chosen).page_end, line_count - 1,
            super::viewport::column_split(state, chosen).page_end >= line_count - 1));
```

- [ ] **Step 2: Build + probe each play at 1920×1200**

For each of LLL-Arkangel and two other two-column plays: set `last_work` in `~/.config/linux-lit/config-dev.json`, launch with `LIT_NO_PAGE_TABLE=1`, resize to 1920×1200, wait ~5s for settle, press G, read the ANCHORDBG line.
```bash
export XDG_RUNTIME_DIR=/run/user/1000 LIT_LOG_PATH=/tmp/anchorchk.log
LIT_DEV=1 LIT_NO_MPV=1 LIT_NO_PAGE_TABLE=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &   # background
# after STARTUP: WAYLAND_DISPLAY=wayland-1 wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
# sleep 5; wtype G; sleep 2; rg ANCHORDBG /tmp/anchorchk.log | tail -2
# cleanup: pkill -f "cage -- ./target/debug/linux-lit"
```
Expected: every play logs `covers=true` at `widget_h=1052`.

- [ ] **Step 3: Remove the probe**

Run: `git checkout src/input/navigation.rs` is WRONG here (would revert Task 1). Instead, delete ONLY the `ANCHORDBG` log lines by editing, then:
Run: `cargo build 2>&1 | rg -i "^error|Finished" | tail -1`
Expected: `Finished`, no ANCHORDBG in `rg ANCHORDBG src/`.

- [ ] **Step 4: Commit (no-op if probe fully removed and nothing else changed)**

Skip — Task 1's commit already captured the fix; the probe was temporary.

---

### Task 3: Convergence + idempotency across several plays

**Files:** none (verification).

- [ ] **Step 1: Full unit tests**

Run: `cargo test --bins 2>&1 | rg -i "test result|FAILED" | tail -5`
Expected: `test result: ok` (all), including the pagination/idempotency invariants.

- [ ] **Step 2: G / search / page-forward convergence per play**

For LLL-Arkangel + two others, headlessly at 1920×1200: (a) press G, note the final page top from the log; (b) reload, `/`-search a word near the end, note the landing; (c) reload, page forward to the end, note the landing. Confirm all three match and the page shows the work's last line (screenshot each with grim; open the PNG and quote the last line).

- [ ] **Step 3: nav-fuzz across the tested plays (fixed seed) vs master**

For each play, run the fuzz on the branch and on master with the SAME seed; branch must have no NEW failures vs master.
```bash
for w in LLL-Arkangel <play2> <play3>; do
  timeout 90 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh \
    --start-work "$w" --secs 60 --seed 11400714819323198485 2>&1 | rg "\[fuzz\] done"
done
```
Expected: `0 failures` each (or only failures that ALSO occur on master with the same seed).

- [ ] **Step 4: Idempotency spot-check**

Confirm the nav-fuzz logged no `JUMP-TO-END not idempotent` for any tested play (`rg "not idempotent" /tmp/fuzz-nav.log` → empty).

---

### Task 4: Document + hand off

**Files:**
- Modify: `docs/troubleshooting/page-turning-mechanics.md`

- [ ] **Step 1: Document the coverage fallback**

In the "final spread is special" / `last_page_top` section, add: the anchor must cover the work's last line even when the tail is a short empty-right spread; the coverage fallback selects the earliest spread whose `column_split(top).page_end >= line_count - 1`; it is inert when the tail fills a right column; it depends only on layout + line_count (idempotent).

- [ ] **Step 2: Commit**

```bash
git add docs/troubleshooting/page-turning-mechanics.md
git commit -m "docs(pagination): final anchor coverage fallback for short tails"
```

- [ ] **Step 3: Hand the user the real-display command**

Give the exact launch command and acceptance criterion: "open each tested two-column play, press G, confirm the final page shows the play's last line in a full-looking page (not a 2-line sliver); repeat with a `/`-search near the end." Ask whether to merge the whole feature (both specs) to master after they confirm.
