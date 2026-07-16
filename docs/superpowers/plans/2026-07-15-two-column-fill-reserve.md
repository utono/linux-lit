# Fuller Two-Column Play Columns Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let each column of a two-column play hold ~1 more line by shrinking the fill-path bottom reserve to what the two-column clip actually consumes, instead of the full 40px scroll margin.

**Architecture:** Add a `TWO_COLUMN_BOTTOM_MARGIN` constant used by the two-column fill decision (`column_split`) and its validator, replacing `BASE_BOTTOM_MARGIN` on that path only. The two-column `exact_end` clip is unchanged (it already sums actual line heights and reserves only a descender allowance). Bump the layout fingerprint so stale `play_pages` tables auto-regenerate at the new fill.

**Tech Stack:** Rust, GTK4/sourceview5, rusqlite (lit.db), cage/grim/wtype headless harness.

## Global Constraints

- Reserve constant `N` is chosen EMPIRICALLY (Task 2), smallest value with a clean last-line descender at 1920×1200. `0 < N ≪ 40`.
- Two-column fill path ONLY. Single-column paged paths keep `BASE_BOTTOM_MARGIN`. Prose untouched.
- Fill sites and the `validate_spreads` usable MUST use the identical reserve, or validation rejects fuller spreads and generation falls back to no-table.
- Do NOT run `cargo run` (user launches the app). Verify via `cargo build`, `cargo test --bins`, and the headless cage harness.
- Headless cage runs need `LIT_DEV=1`; cage's nested wayland socket is a DIFFERENT `wayland-N` than the host — find it via cage's log ("unable to lock … wayland-0"). Resize with `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`. Cleanup ONLY `pkill -f "cage -- ./target/debug/linux-lit"`.
- After any clipping-related change, UPDATE `docs/troubleshooting/clip-prevention.md`.

---

### Task 1: Add the `TWO_COLUMN_BOTTOM_MARGIN` constant (placeholder value)

**Files:**
- Modify: `src/input/scroll.rs` (beside `BASE_BOTTOM_MARGIN`, line 733)

**Interfaces:**
- Produces: `pub(crate) const TWO_COLUMN_BOTTOM_MARGIN: i32` — consumed by `viewport.rs` fill sites and `page_table.rs` validator.

- [ ] **Step 1: Add the constant**

In `src/input/scroll.rs`, immediately after the `BASE_BOTTOM_MARGIN` definition (line 733):

```rust
/// Bottom reserve for a TWO-COLUMN paged column's FILL decision. Unlike the
/// single-column path (whose clip covers the full descender_guard +
/// BASE_BOTTOM_MARGIN band, see the single-col reserve in update_bottom_clip),
/// the two-column `exact_end` clip sums the actual line heights and reserves
/// only a descender allowance below the last line — so the fill only needs to
/// reserve that much. Reserving the full 40px BASE_BOTTOM_MARGIN wastes ~1 line
/// per column. Value chosen empirically: the smallest reserve whose last-column
/// line keeps a clean descender at production geometry (see
/// docs/superpowers/plans/2026-07-15-two-column-fill-reserve.md, Task 2).
pub(crate) const TWO_COLUMN_BOTTOM_MARGIN: i32 = 12;
```

(12 is a starting placeholder; Task 2 replaces it with the measured value.)

- [ ] **Step 2: Verify it compiles (unused is fine for now)**

Run: `cargo build 2>&1 | tail -2`
Expected: `Finished` (an unused-const warning is acceptable at this step).

- [ ] **Step 3: Commit**

```bash
git add src/input/scroll.rs
git commit -m "feat(pagination): add TWO_COLUMN_BOTTOM_MARGIN constant"
```

---

### Task 2: Wire the constant into the two-column fill + validator, and pick N empirically

**Files:**
- Modify: `src/input/viewport.rs:1239` (left column usable)
- Modify: `src/input/viewport.rs:1376` (right column usable)
- Modify: `src/input/viewport.rs:1327` (first-spread short-opening right-column probe)
- Modify: `src/input/page_table.rs` (~379, `validate_spreads` usable)
- Modify: `src/input/scroll.rs` (finalize `TWO_COLUMN_BOTTOM_MARGIN` value)

**Interfaces:**
- Consumes: `crate::input::scroll::TWO_COLUMN_BOTTOM_MARGIN` (Task 1).

- [ ] **Step 1: Swap the reserve at the left-column fill site**

`src/input/viewport.rs:1239`, in `column_split`:

```rust
        let usable = left_h - guard - crate::input::scroll::TWO_COLUMN_BOTTOM_MARGIN;
```

(was `- BASE_BOTTOM_MARGIN`)

- [ ] **Step 2: Swap the reserve at the right-column fill site**

`src/input/viewport.rs:1376`:

```rust
        let usable = right_h - guard - crate::input::scroll::TWO_COLUMN_BOTTOM_MARGIN;
```

- [ ] **Step 3: Swap the reserve at the first-spread short-opening probe**

`src/input/viewport.rs:1327` (the `page_top == 0` right-column fit check):

```rust
                    let usable = right_h - guard - crate::input::scroll::TWO_COLUMN_BOTTOM_MARGIN;
```

- [ ] **Step 4: Swap the reserve in the validator**

`src/input/page_table.rs` ~379 (`generate_and_store`, building `ValidateCtx.usable_height`):

```rust
    let usable = widget_height - guard - crate::input::scroll::TWO_COLUMN_BOTTOM_MARGIN;
```

(was `crate::input::scroll::BASE_BOTTOM_MARGIN`)

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | tail -2`
Expected: `Finished`, no unused-const warning now.

- [ ] **Step 6: Empirically choose N — headless spike**

For each candidate `N ∈ {8, 12, 16}`: set `TWO_COLUMN_BOTTOM_MARGIN = N` in `scroll.rs`, `cargo build`, then run the headless harness against `LLL-Arkangel` at 1920×1200 with the live engine (`LIT_NO_PAGE_TABLE=1`) so the new reserve applies without a stored table. Drive to a representative two-column spread and screenshot with `grim`; do one pass with `LIT_DEBUG_CLIP_COLOR='#ff0000'`.

Launch (background):
```bash
export XDG_RUNTIME_DIR=/run/user/1000 LIT_LOG_PATH=/tmp/filln.log
cd ~/utono/linux-lit
LIT_DEV=1 LIT_NO_MPV=1 LIT_NO_PAGE_TABLE=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit 2>/tmp/cage-fill.log
```
Find cage's socket (the one it could NOT lock host wayland-0 → it took the next):
```bash
grep -i "unable to lock" /tmp/cage-fill.log   # confirms it's nested
# cage's own socket is the wayland-N it created; test wayland-1:
export WAYLAND_DISPLAY=wayland-1
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```
Drive + shoot:
```bash
for i in $(seq 1 20); do wtype x; sleep 0.25; done
grim -o HEADLESS-1 /tmp/fill-N.png
```
Cleanup: `pkill -f "cage -- ./target/debug/linux-lit"`.

- [ ] **Step 7: Inspect and set N**

Open each `/tmp/fill-N.png` (and the `#ff0000` clip overlay). Verify:
1. Each column gained a line versus the 40px baseline.
2. The LAST line of each column shows CLEAN descenders (no sliced `g/y/p`/comma) — the clip-prevention.md #10 check.
3. The clip band's top edge clears the descender ink.

Set `TWO_COLUMN_BOTTOM_MARGIN` to the SMALLEST N passing all three. Rebuild.

- [ ] **Step 8: Build clean**

Run: `cargo build 2>&1 | tail -2`
Expected: `Finished`.

- [ ] **Step 9: Commit**

```bash
git add src/input/viewport.rs src/input/page_table.rs src/input/scroll.rs
git commit -m "feat(pagination): two-column fill reserves only the clip's descender band"
```

---

### Task 3: Bump the layout fingerprint so stale tables regenerate

**Files:**
- Modify: `src/input/page_table.rs` (`fingerprint_string` version tag `v2` → `v3`)

**Interfaces:**
- Consumes: nothing new. Changes the stored `layout_fingerprint` string so old rows no longer match.

- [ ] **Step 1: Find the version tag**

Run: `rg -n "\"v2\"|v2\\||version|fn fingerprint_string" src/input/page_table.rs`
Expected: locate the `v2` literal inside `fingerprint_string`.

- [ ] **Step 2: Bump to v3**

Change the `v2` literal to `v3` in `fingerprint_string`. (This is the leading token of the fingerprint, e.g. `v2|Charter|17|…` → `v3|Charter|17|…`.)

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -2`
Expected: `Finished`.

- [ ] **Step 4: Verify unit tests still pass (fingerprint tests may assert the prefix)**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: `test result: ok`. If a fingerprint test asserts `v2`, update its expected string to `v3` in the same commit.

- [ ] **Step 5: Commit**

```bash
git add src/input/page_table.rs
git commit -m "feat(pagination): bump play_pages fingerprint to v3 (regenerate at new fill)"
```

---

### Task 4: Verify — unit tests, validator, nav-fuzz, pixel e2e

**Files:** none (verification only).

- [ ] **Step 1: Clip-invariant unit tests**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: `test result: ok` (all). These guard the clip arithmetic (clip-prevention.md "Verifying").

- [ ] **Step 2: Regenerate + structurally validate a play**

Launch headless once with `LIT_GEN_PAGE_TABLE=1` on `LLL-Arkangel` at 1920×1200 to write a fresh `v3` table, then run the litdb `validate-play-pages` audit.
Run: `.claude/skills/validate-play-pages/…` per that skill (read it first).
Expected: PASS (coverage, ordering, fit against the NEW usable).

- [ ] **Step 3: nav-fuzz on the two-column play**

Run:
```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work LLL-Arkangel --secs 60
```
Expected: no UNBALANCED / short-column / G-idempotency FAILs. (A scene-edge UNBALANCED that reproduces at the OLD reserve too is pre-existing, not a regression — note it, don't block.)

- [ ] **Step 4: Pixel line-clipping e2e (main card)**

Run:
```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture 2>&1 | tail -20
```
Expected: PASS (fail-closed line-clipping assertion — catches a #10 slice).

- [ ] **Step 5: Before/after screenshots**

Capture `LLL-Arkangel` Act 3 Scene 1 spread (`top=1154`) at 1920×1200 before (git stash) and after. Open both PNGs and confirm each column gained a line and the last line's descenders are clean. Attach for the user.

- [ ] **Step 6: Commit any test fixture updates (if the e2e baseline changed legitimately)**

```bash
git add -A && git commit -m "test(pagination): update e2e baseline for fuller two-column fill"
```
(Skip if nothing changed.)

---

### Task 5: Update clip-prevention.md + hand off for real-display confirmation

**Files:**
- Modify: `docs/troubleshooting/clip-prevention.md` (add the fill-vs-clip reserve asymmetry note)

- [ ] **Step 1: Add the new consideration**

Append a failure-checklist entry (next number) documenting: on a two-column play, the FILL reserve (`TWO_COLUMN_BOTTOM_MARGIN`) is intentionally smaller than `BASE_BOTTOM_MARGIN` because the two-column `exact_end` clip consumes only a descender allowance; if the last column line's descenders slice, N is too small (raise it), and if columns underfill, N drifted back toward 40. Note that fill sites and `validate_spreads` must share the constant.

- [ ] **Step 2: Commit**

```bash
git add docs/troubleshooting/clip-prevention.md
git commit -m "docs(clip): two-column fill reserve vs clip band asymmetry"
```

- [ ] **Step 3: Hand the user the real-display command**

cage is software rendering; the final descender-slice check is the real GL renderer. Give the user the exact launch/e2e command and the acceptance criterion: "open `LLL-Arkangel`, page to a two-column spread, confirm each column gained a line and the bottom line's `g/y/p`/comma tails are intact." Ask whether to merge after they confirm (per the finishing-a-branch convention).
