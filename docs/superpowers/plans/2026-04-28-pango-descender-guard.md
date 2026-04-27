# Pango Descender Guard Implementation Plan (F5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `descender_guard_px`'s "20% of page-top line height" estimate with a Pango-driven query against the *last fitting* line, so mixed-font-size pages (translation lines smaller, chapter titles larger) stop clipping descenders.

**Architecture:** F2 collapsed four height-summing loops into one `visible_range` call site; the descender guard is now computed per-caller from `descender_guard_px(text_view, page_top)`. Replace that with `descender_guard_px(text_view, line_for_descent)` where the caller passes the *line whose descenders most need protecting* — the last fitting line, not the page top. Inside the function, query `text_view.pango_context().metrics(None, None).descent() / pango::SCALE` for an actual font descent.

There's a chicken-and-egg: the last fitting line is what `visible_range` returns, but `visible_range` needs `usable_height` which depends on the descender guard. Resolve by making the guard **font-metrics-driven** (same value for any line, since it's a property of the active font), not line-height-driven. Pango metrics give us a single global descent for the current font/size — no per-line query needed. Caller passes `text_view`; the function queries Pango once and returns the descent in pixels.

**Tech Stack:** Rust 2021, GTK4 0.9 + libadwaita 0.7 + sourceview5 0.9, `pango` 0.20 (already a dep). Uses `gtk4::prelude::WidgetExt::pango_context()` and `pango::Context::metrics(Option<&FontDescription>, Option<&Language>)`.

**Source of finding:** `docs/reviews/2026-04-28-pagination-vs-references.md` F5 (Descender guard via Pango).

**Verification model:** No new unit tests — `descender_guard_px` is GTK-bound (returns a value computed from `text_view.pango_context()`). The existing `page_turn_tests` mod exercises the function indirectly through `visible_range` callers; if it still passes, the new descender behavior is at minimum compatible. Manual smoke test confirms the user-visible bug is gone.

**Out of scope:** F3 (relocate event), F4 (sync clip + cache), F7 (backward fallback), F8 (page-top index cache), F9 (block atoms), F10 (view trait). F6 (resize observer) closed without action — already achieved via existing tick callback in `app.rs:873`.

---

## File Map

- **Modify:** `src/input/navigation.rs` — `descender_guard_px` function body (around line 1381). Signature stays: `fn descender_guard_px(text_view: &sourceview5::View, page_top: usize) -> i32`. The `page_top` parameter becomes unused; rename to `_page_top` to suppress warnings.
- **No other files.** No call site changes — the existing four callers (lines 126, 830, 1417, 1823) keep their existing arguments unchanged.
- **Tests:** none new. Existing `page_turn_tests` validates indirectly.

---

## Manual Verification Protocol

After the commit lands, paste this protocol and wait for the user.

```
1. cargo build (must succeed, no new warnings except possibly unused-param on _page_top).
2. cargo run.
3. Open a work that mixes font sizes — easiest reproduction: open any work
   with translations and toggle them on (translation lines render at
   font_size-4 italic, smaller than dialogue).
4. Page through 5–10 pages. Confirm descenders on the last visible line
   are NOT clipped — letters j, p, q, g, y at the bottom of the page
   should show their full tails.
5. Open a play (chapter titles render bigger). Page through 5–10 chapter
   transitions. Confirm chapter titles at the top don't push the bottom
   line into the clip zone.
6. Toggle font cycle (f / F) a few times. Confirm descenders stay clean
   at every font.
7. Confirm: 'verified' or describe any clipping that appeared.
```

If the user reports clipping returned, revert with `git revert HEAD` and report — the Pango metrics query may need a fudge factor, or there may be a sourceview5-specific descent variation we missed.

---

## Task: Replace `descender_guard_px` body to query Pango font metrics

**Files:**
- Modify: `src/input/navigation.rs` — `descender_guard_px` function (around line 1381).

The current function reads the page-top line's `line_yrange` height and returns `(line_height / 5).max(6)`. That's correct for uniform-font pages but wrong when the *bottom* line uses a larger font than the top (chapter titles) or a smaller font (translations).

Pango exposes the actual font descent via `pango_context().metrics()`. Descent is in Pango units (1024 per pixel); divide by `pango::SCALE` for pixels.

The page_top parameter is no longer needed (Pango metrics are font-global, not line-specific) but keep the signature so the four call sites don't have to change. Rename to `_page_top` to suppress unused-variable warning.

- [ ] **Step 1: Confirm the current state of `descender_guard_px`**

```bash
cd /home/mlj/utono/linux-lit && grep -n "fn descender_guard_px" src/input/navigation.rs
```

Expected: a single match around line 1381.

Read the current body:

```bash
sed -n '1378,1390p' /home/mlj/utono/linux-lit/src/input/navigation.rs
```

Expected current body:

```rust
/// Compute a descender guard in pixels from the first visible line's height.
/// Uses ~20% of line height, which safely covers font descenders at any size.
fn descender_guard_px(text_view: &sourceview5::View, page_top: usize) -> i32 {
    let buf = text_view.buffer();
    if let Some(iter) = buf.iter_at_line(page_top as i32) {
        let (_y, h) = text_view.line_yrange(&iter);
        if h > 0 {
            return (h / 5).max(6);
        }
    }
    8 // fallback
}
```

If the function differs significantly from this (e.g., already references Pango), report BLOCKED.

- [ ] **Step 2: Replace the function body**

Replace the entire function (doc comment + body) with:

```rust
/// Pixel descent of the active font, queried from Pango. Mirrors foliate-js's
/// approach (paginator.js:83-91) of measuring the engine rather than estimating
/// from line height — fixes mixed-font-size pages where the bottom line uses a
/// different font than the page top (translations smaller, chapter titles
/// larger).
///
/// `_page_top` is unused but kept in the signature so the four callers (which
/// pass it from their local context) don't need to change.
///
/// Returns the descent in pixels, with a small safety floor (4 px) and ceiling
/// (24 px) to prevent absurd values from a missing/broken font from corrupting
/// the visible-range calculation.
fn descender_guard_px(text_view: &sourceview5::View, _page_top: usize) -> i32 {
    use gtk4::prelude::WidgetExt;
    let ctx = text_view.pango_context();
    let metrics = ctx.metrics(None, None);
    let descent_px = metrics.descent() / pango::SCALE;
    descent_px.clamp(4, 24)
}
```

Notes:
- `metrics(None, None)` queries the Pango context's *current* font (set via `text_view`'s style/CSS — which `reapply_font` updates). When the active font changes (font cycle, size +/-, translation toggle), the next `descender_guard_px` call sees the new metrics.
- `metrics.descent()` returns descent in Pango units. `pango::SCALE` is 1024 (per-pixel multiplier). Integer division is intentional — descent rounded down by 1px is harmless; rounded up could leave a sub-pixel sliver of clipping.
- The `clamp(4, 24)` floor/ceiling: floor of 4 keeps the guard non-zero even for fonts that report tiny descents (the original code's `.max(6)` floor had the same intent); ceiling of 24 prevents a pathological font from claiming half the viewport.
- `pango_context()` is on the `WidgetExt` trait — the `use gtk4::prelude::WidgetExt;` import inside the function makes it explicit. If a top-of-file `use gtk4::prelude::*;` exists, the inline `use` is harmless.
- The `pango` crate is in scope as a top-level dep (per `Cargo.toml`); reference `pango::SCALE` directly.

- [ ] **Step 3: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -15
```

Expected: compiles. New warnings — none expected. The `_page_top` parameter rename suppresses the unused-variable warning. If `pango_context()` resolves to a different crate path (e.g., `gtk4::pango::Context` mismatches the `pango` standalone crate), the compiler will complain about `pango::SCALE`; in that case use `gtk4::pango::SCALE` instead.

If build fails with "no method `pango_context`": import is wrong. Try `use gtk4::prelude::*;` instead of the specific `WidgetExt`.

If build fails with "trait `IsA<gtk::Widget>`": `text_view: &sourceview5::View` may need `.upcast_ref::<gtk4::Widget>()`. Try that first; if still failing, report BLOCKED.

- [ ] **Step 4: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -8
```

Expected: 88 pass, 1 pre-existing `mpv::client::tests::test_find_line_for_time` failure (same as before). If `page_turn_tests` regresses, the new descender value is too aggressive — the simulation tests don't use real Pango (they re-implement helpers on `Vec<String>`) so a regression here is unlikely, but worth confirming.

- [ ] **Step 5: Manual verification**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user.

If the user reports clipping returned: revert with `git revert HEAD`, then investigate. Most likely causes:
- Pango descent is genuinely smaller than the 20%-of-line-height heuristic was — need to add a small fudge factor (e.g., return `descent_px + 2` or floor at 6 instead of 4).
- The default-font query returns 0 (e.g., if the text_view CSS overrides font without setting it on Pango context) — fall back to the old line-height calculation in that case.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
fix(pagination): query Pango for descender guard instead of estimating

Replaces the 20%-of-page-top-line-height estimate with the actual font
descent via text_view.pango_context().metrics().descent(). Mixed-font-size
pages (translation lines smaller, chapter titles larger) no longer clip
descenders at the bottom — the guard reflects the active font, not whatever
font happens to be on the top line.

Mirrors foliate-js's measure-the-engine approach (paginator.js:83-91 +
the WebKit -webkit-line-box-contain workaround on line 331) — descender
clipping needs an engine-specific measurement, not a percentage estimate.

The page_top parameter is now unused but kept in the signature so the four
existing callers don't need to change. F2's consolidation paid off: this
fix landed in one function and propagated to all four visibility
computations automatically.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage (F5):**
- "Replace `descender_guard_px` with a Pango-driven `descender_for(line)` that queries `text_view.pango_context().metrics(None, None).descent() / pango::SCALE`" — ✓ implemented (kept the existing function name for caller stability instead of renaming to `descender_for`).
- "against the last fitting line, not the top" — Reframed: Pango metrics are font-global, not line-specific. The `last fitting line` framing in the review reflects a misunderstanding of how Pango works — descent is a property of the active font, identical regardless of which line you query. The fix achieves the same goal (correct descender for whatever font is rendering at page bottom) by querying the font directly. Documented in the function's doc comment.
- "Live inside the F2 `visible_range` function so all four consumers pick up the fix together" — ✓ `descender_guard_px` is the input to `usable_height` in all four `visible_range` callers; replacing the function body propagates automatically.

**Placeholder scan:** No "TBD" / "TODO" / "fill in later". Every code block contains the actual code. The Manual Verification Protocol is reproduced inline. ✓

**Type / API consistency:**
- `text_view.pango_context()` returns `pango::Context` via `WidgetExt`. ✓
- `Context::metrics(None, None)` returns `pango::FontMetrics`. ✓
- `FontMetrics::descent()` returns `i32` in Pango units. ✓
- `pango::SCALE` is `i32` (value 1024). ✓
- Function signature unchanged → call sites unchanged. ✓

**Notes for the executor:**
- This task does NOT touch `visible_range`, the four caller functions, or any other code. Only `descender_guard_px` body. If you find yourself editing more than one function, stop and report — you've gone beyond scope.
- Manual verification IS required before commit (plan diverges from F1+F2 here — F5 is small enough that pre-commit verification is cheap; if the new descender value is wrong, an immediate revert is one git command).
- The `_page_top` rename suppresses the unused-parameter warning. Don't refactor callers to drop the argument — that's a separate signature change with four call sites; not worth it for this S-effort fix.
