# Root Variants From Theme-Native Accent Candidates — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make linux-lit's root-color variants (cycled by Ctrl+`$`) be the theme's `dwl.rootcolor_candidates` used verbatim — no computed blends, no re-sort, per-theme count.

**Architecture:** `resolve_theme_variant` reads `dwl.rootcolor_candidates` directly as the ordered variant list; `Theme` gains a `root_variant_count` field carrying the per-theme count; the cycler and the out-of-range clamp read that count instead of the removed `ROOT_VARIANT_COUNT` constant.

**Tech Stack:** Rust, serde_json (theme JSON parsing), GTK4 (unaffected here), cargo test.

## Global Constraints

- Root variant list = `dwl.rootcolor_candidates` in **authored order** (no re-sort, no dedup, no candidate-equals-root skipping).
- No computed colors in the variant path: **no** `blend_colors`/`darken_color`/`relative_luminance` calls inside variant selection.
- Candidates absent/empty → single-element list `[dwl.rootcolor]`, count 1.
- Index 0 = `candidates[0]` (launch / theme-switch default).
- Out-of-range incoming variant index → clamp to **0** (reset), applied in `resolve_theme_variant`.
- `scrim_bg = darken(root_color, 0.80)` stays derived from the active root (unchanged).
- Card surface stays byte-identical across variants (only `root_color` + `scrim_bg` vary).
- Do NOT run the app (`cargo run`) — build/test only; user launches the app. Verify GUI headlessly via cage per the project's headless protocol.
- Commit message trailers (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3
  ```

**Working directory:** the worktree `~/utono/linux-lit-wt/feat-root-native-accent` (branch `feat/root-native-accent`). All paths below are relative to it.

---

### Task 1: Add `root_variant_count` to the `Theme` struct

**Files:**
- Modify: `src/theme.rs` (struct `Theme` ~line 117-140; both constructors ~line 389 and ~line 424)

**Interfaces:**
- Produces: `Theme.root_variant_count: u8` — number of root variants the active theme has (candidate list length, min 1). Read by Task 3 (resolve), Task 4 (cycler).

- [ ] **Step 1: Add the field to the struct**

In `src/theme.rs`, in `pub struct Theme { ... }`, immediately after the existing `pub root_variant: u8,` line (currently `~140`), add:

```rust
    pub root_variant: u8,         // active root-color variant (index into candidates)
    pub root_variant_count: u8,   // number of root variants (candidate count, min 1)
```

(Replace the existing `root_variant` doc-comment text `active root-color variant (0 = designed)` with `active root-color variant (index into candidates)` as shown.)

- [ ] **Step 2: Set it in the JSON constructor**

In `resolve_theme_variant`, in the `Theme { ... }` literal (near `root_variant: variant,` ~line 389), add the count field. It will be assigned a real value in Task 3; for now wire a placeholder that compiles by counting candidates inline is premature — instead, temporarily set:

```rust
        root_variant: variant,
        root_variant_count: 1,
```

- [ ] **Step 3: Set it in the fallback constructor**

In `default_theme()` (the `Theme { ... }` literal near `root_variant: 0,` ~line 424), add:

```rust
        root_variant: 0,
        root_variant_count: 1,
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles (warnings OK). The field exists; value is a placeholder until Task 3.

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "feat(theme): add root_variant_count field to Theme

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3"
```

---

### Task 2: Write failing tests for the verbatim-candidates behavior

**Files:**
- Modify: `src/theme.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `resolve_theme_variant(name, val, variant) -> Theme` (existing), `Theme.root_color`, `Theme.root_variant`, `Theme.root_variant_count`.

These tests assert the target behavior and MUST FAIL against current code (which still uses the computed 5-variant ladder + `root_variant_count: 1` placeholder).

- [ ] **Step 1: Add the new tests**

In `src/theme.rs`, inside `mod tests`, add these four tests (place them right after the existing `card_surface_is_identical_across_root_variants` test, ~line 1510). `CANDIDATES_JSON` (already defined in the module) has `rootcolor "#08526b"` and `rootcolor_candidates ["#41819b", "#286983", "#08526b"]`.

```rust
    #[test]
    fn root_variants_are_the_candidates_verbatim() {
        // Root variants ARE dwl.rootcolor_candidates, in authored order — no
        // re-sort, no dedup, no skipping the entry equal to the designed root.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let want = ["#41819b", "#286983", "#08526b"];
        for (i, w) in want.iter().enumerate() {
            let v = resolve_theme_variant("s", &json, i as u8);
            assert_eq!(v.root_color, *w, "variant {i} root_color");
            assert_eq!(v.root_variant, i as u8, "variant {i} index");
        }
    }

    #[test]
    fn root_variant_count_matches_candidate_list() {
        // count = candidate list length (3 here).
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v = resolve_theme_variant("s", &json, 0);
        assert_eq!(v.root_variant_count, 3);
    }

    #[test]
    fn out_of_range_variant_clamps_to_zero() {
        // A saved index past the candidate count resets to 0 (first candidate).
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v = resolve_theme_variant("s", &json, 5);
        assert_eq!(v.root_color, "#41819b"); // candidates[0]
        assert_eq!(v.root_variant, 0);
    }

    #[test]
    fn no_candidates_falls_back_to_designed_root() {
        // No rootcolor_candidates → single variant = dwl.rootcolor, count 1.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                  "dwl": {"rootcolor": "#08526b"},
                  "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"} }"##)
            .unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        assert_eq!(v0.root_color, "#08526b");
        assert_eq!(v0.root_variant_count, 1);
        // Any index clamps to the single root.
        let v3 = resolve_theme_variant("s", &json, 3);
        assert_eq!(v3.root_color, "#08526b");
        assert_eq!(v3.root_variant, 0);
    }
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cargo test --bins root_variants_are_the_candidates_verbatim root_variant_count_matches_candidate_list out_of_range_variant_clamps_to_zero no_candidates_falls_back_to_designed_root`
Expected: FAIL — `root_variants_are_the_candidates_verbatim` fails (current code re-sorts + injects blends, so variant 0 is a white-blend, not `#41819b`); `root_variant_count_matches_candidate_list` fails (placeholder count is 1, not 3).

- [ ] **Step 3: Commit the failing tests**

```bash
git add src/theme.rs
git commit -m "test(theme): failing tests for verbatim rootcolor_candidates variants

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3"
```

---

### Task 3: Rewrite `root_variant_color` → candidate lookup, wire count + clamp

**Files:**
- Modify: `src/theme.rs` (`ROOT_VARIANT_COUNT` const ~62-64; `root_variant_color` ~74-108; `resolve_theme_variant` clamp + assignment ~282-283 and the `Theme{}` literal ~389)

**Interfaces:**
- Produces: `fn root_variants(val: &Value) -> Vec<String>` — the ordered root-variant list for a theme JSON value (candidates verbatim, else `[rootcolor]`). Consumed only within `resolve_theme_variant`.

- [ ] **Step 1: Replace the const + `root_variant_color` with `root_variants`**

In `src/theme.rs`, delete the `ROOT_VARIANT_COUNT` const (lines ~62-64) and replace the entire `root_variant_color` function (lines ~66-107, from its doc-comment through the closing brace) with:

```rust
/// The ordered root-color variants for a theme: `dwl.rootcolor_candidates`
/// used VERBATIM (authored order — the dwl-mlj tooling already builds a
/// hue-locked, sorted native-accent family). When a theme defines no
/// candidates, the sole variant is the designed `dwl.rootcolor`. Never
/// synthesizes colors and never re-sorts. See
/// docs/superpowers/specs/2026-07-14-root-native-accent-design.md.
fn root_variants(val: &Value) -> Vec<String> {
    let designed_root = val
        .get("dwl")
        .and_then(|d| d.get("rootcolor"))
        .and_then(|c| c.as_str())
        .unwrap_or("#000000")
        .to_string();
    let candidates: Vec<String> = val
        .get("dwl")
        .and_then(|d| d.get("rootcolor_candidates"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if candidates.is_empty() {
        vec![designed_root]
    } else {
        candidates
    }
}
```

- [ ] **Step 2: Rewrite the resolve site to use the list + clamp + count**

In `resolve_theme_variant`, replace these two lines (currently ~282-283):

```rust
    let variant = variant % ROOT_VARIANT_COUNT;
    let root_color = root_variant_color(val, &designed_root, variant);
```

with:

```rust
    let variants = root_variants(val);
    let count = variants.len().max(1) as u8;
    // Out-of-range saved index resets to 0 (first candidate).
    let variant = if (variant as usize) < variants.len() { variant } else { 0 };
    let root_color = variants[variant as usize].clone();
```

Note: `designed_root` remains defined earlier in the function for other uses (focus color fallbacks etc.); leave it. If the compiler warns it is now unused, that is handled in Step 3.

- [ ] **Step 3: Set the real count in the `Theme` literal**

In the same function's `Theme { ... }` literal, change the placeholder added in Task 1:

```rust
        root_variant: variant,
        root_variant_count: count,
```

- [ ] **Step 4: Build and check for the unused `designed_root`**

Run: `cargo build`
Expected: compiles. If a `warning: unused variable: designed_root` appears (it is still used for `focus_color` fallback per the survey, so it should NOT), leave it. Do not delete `designed_root` — `focus_color` uses `str_field(dwl, "focuscolor").unwrap_or_else(|| text_fg.clone())`, which does not use it; verify by searching. If genuinely unused now, prefix rename is wrong — instead confirm with `rg -n 'designed_root' src/theme.rs` and remove only the binding if it has zero remaining uses.

Run: `rg -n 'designed_root' src/theme.rs`
Expected: shows the binding site and any remaining uses. If the only occurrence is the `let designed_root = ...` binding, delete that binding line. If other uses remain, keep it.

- [ ] **Step 5: Run the Task 2 tests to verify they pass**

Run: `cargo test --bins root_variants_are_the_candidates_verbatim root_variant_count_matches_candidate_list out_of_range_variant_clamps_to_zero no_candidates_falls_back_to_designed_root`
Expected: PASS (all four).

- [ ] **Step 6: Commit**

```bash
git add src/theme.rs
git commit -m "feat(theme): root variants from rootcolor_candidates verbatim

Replace the computed 5-variant ladder with a direct read of
dwl.rootcolor_candidates (authored order, no blends, no re-sort). Per-theme
count = candidate length; out-of-range index clamps to 0; no candidates falls
back to the single designed rootcolor. Removes ROOT_VARIANT_COUNT.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3"
```

---

### Task 4: Update the cycler to read the per-theme count

**Files:**
- Modify: `src/input/actions/settings.rs` (`cycle_root_variant` ~555-573)

**Interfaces:**
- Consumes: `Theme.root_variant`, `Theme.root_variant_count` (Task 1/3); `crate::theme::load_theme_with_fallback`.

- [ ] **Step 1: Replace the count source and the two `ROOT_VARIANT_COUNT` references**

In `src/input/actions/settings.rs`, in `cycle_root_variant`, replace:

```rust
    let count = crate::theme::ROOT_VARIANT_COUNT;
    let next = if forward {
        (s.theme.root_variant + 1) % count
    } else {
        (s.theme.root_variant + count - 1) % count
    };
```

with:

```rust
    let count = s.theme.root_variant_count.max(1);
    let next = if forward {
        (s.theme.root_variant + 1) % count
    } else {
        (s.theme.root_variant + count - 1) % count
    };
```

Then in the `notify-send` args, replace the toast denominator:

```rust
               &format!("Root [{}/{}]", next + 1, crate::theme::ROOT_VARIANT_COUNT),
```

with:

```rust
               &format!("Root [{}/{}]", next + 1, count),
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles with no reference to `ROOT_VARIANT_COUNT` remaining. Confirm:

Run: `rg -n 'ROOT_VARIANT_COUNT' src/`
Expected: no matches (the const is deleted and all call sites updated).

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/settings.rs
git commit -m "feat(input): cycle root variants over per-theme candidate count

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3"
```

---

### Task 5: Drop the modulo from `config.root_variant_for` and fix its test

**Files:**
- Modify: `src/config.rs` (`root_variant_for` ~422-424; test `root_variant_for_defaults_to_zero_and_wraps` ~735-742)

**Interfaces:**
- Produces: `Config.root_variant_for(&self, theme_name: &str) -> u8` — the raw saved index (0 if none). Clamping now happens in `resolve_theme_variant` (Task 3), so this no longer applies `% ROOT_VARIANT_COUNT`.

- [ ] **Step 1: Update the failing test first**

In `src/config.rs`, the existing test asserts modulo-5 wrapping, which no longer happens here. Replace the body of `root_variant_for_defaults_to_zero_and_wraps` (rename it too) with the new contract — `root_variant_for` returns the raw saved value unchanged:

```rust
    #[test]
    fn root_variant_for_returns_saved_index_unclamped() {
        let mut c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.root_variant_for("kindle-sepia"), 0);
        c.root_variants.insert("kindle-sepia".into(), 2);
        assert_eq!(c.root_variant_for("kindle-sepia"), 2);
        // A large saved index is returned verbatim — resolve_theme_variant
        // clamps out-of-range indices to 0 against the theme's candidate count.
        c.root_variants.insert("kindle-sepia".into(), 7);
        assert_eq!(c.root_variant_for("kindle-sepia"), 7);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --bins root_variant_for_returns_saved_index_unclamped`
Expected: FAIL — current `root_variant_for` returns `7 % 5 = 2`, not `7`.

- [ ] **Step 3: Remove the modulo in `root_variant_for`**

Replace the function body (currently `self.root_variants.get(theme_name).copied().unwrap_or(0) % crate::theme::ROOT_VARIANT_COUNT`) with:

```rust
    pub fn root_variant_for(&self, theme_name: &str) -> u8 {
        self.root_variants.get(theme_name).copied().unwrap_or(0)
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --bins root_variant_for_returns_saved_index_unclamped`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): root_variant_for returns saved index unclamped

Clamping moved to resolve_theme_variant (per-theme candidate count); drop the
% ROOT_VARIANT_COUNT that no longer has a constant to reference.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3"
```

---

### Task 6: Retire the computed-ladder tests; retarget count-based tests

**Files:**
- Modify: `src/theme.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `resolve_theme_variant`, `Theme.root_variant_count`.

- [ ] **Step 1: Delete tests that assert removed behavior**

In `src/theme.rs`, delete these five test functions in their entirety (each `#[test] fn ... { ... }` block):

- `all_five_roots_sorted_lightest_to_darkest_including_base` (~1512)
- `computed_fallback_without_candidates` (~1532)
- `short_candidate_list_fills_remaining_computed` (~1555)
- `card_surface_pinned_on_extended_slots` (~1688) — asserts slots 3-4 exist; `CANDIDATES_JSON` has only 3 variants now
- `variant_index_wraps_modulo_count` (~1701) — asserts `5 % 5 == 0`; superseded by `out_of_range_variant_clamps_to_zero`

- [ ] **Step 2: Retarget the surviving `0..ROOT_VARIANT_COUNT` loops**

Any remaining test that loops `for variant in 0..ROOT_VARIANT_COUNT` or `(0..ROOT_VARIANT_COUNT)` must instead use the theme's actual count. In `card_surface_is_identical_across_root_variants`, change:

```rust
        for variant in 0..ROOT_VARIANT_COUNT {
```

to:

```rust
        let count = resolve_theme_variant("s", &json, 0).root_variant_count;
        for variant in 0..count {
```

Apply the same substitution to any OTHER surviving test that references `ROOT_VARIANT_COUNT` (find them in Step 3). For tests that iterate variants of the `COLORFUL_INK_JSON`/`CANDIDATES_JSON` fixtures for contrast (`vocab_popup_ink_contrasts_with_root_variants` ~1287, `card_surface_never_varies` ~1618, `cursor_colors_do_not_vary_with_root_variant` ~1746, and the vocab/gloss loops ~1591-1680), replace any `0..ROOT_VARIANT_COUNT` bound with `0..resolve_theme_variant(<name>, &json, 0).root_variant_count` using that test's own fixture name and json var.

- [ ] **Step 3: Confirm no `ROOT_VARIANT_COUNT` remains anywhere**

Run: `rg -n 'ROOT_VARIANT_COUNT' src/`
Expected: no matches. If any remain, replace each per Step 2's pattern.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test --bins`
Expected: PASS (0 failed). Note the pass count for the commit.

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "test(theme): retire computed-ladder tests; count-based variant loops

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01DZ77rngeQuvamiaHXVvce3"
```

---

### Task 7: Headless verification of the native-accent cycle

**Files:** none (verification only). Produces a short findings note appended to the plan's PR/summary, not a code change.

**Interfaces:** Consumes the built binary; drives Ctrl+`$` (`RootVariantNext`, bound to `dollar`).

- [ ] **Step 1: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 2: Confirm the JSON candidate lists for the target themes**

Run:
```bash
F=~/utono/themes/.config/themes/themes-unified.json
jq -c '.["kindle-sepia"].dwl.rootcolor_candidates' "$F"
jq -c '.["sepia-lightest"].dwl.rootcolor_candidates' "$F"
```
Expected: kindle-sepia → 6 entries; sepia-lightest → 3 entries. Record them — the cycle must walk exactly these, in this order, starting at index 0.

- [ ] **Step 3: Launch headless cage on a 6-candidate theme**

Per the project's headless protocol (LIT_DEV=1, LIT_NO_MPV=1, GSK_RENDERER=cairo, cage; resize to 1920x1200). The dev config's active theme is whatever `config-dev.json` holds; if it is not kindle-sepia, cycle the theme with `wtype -k t -M ctrl -m ctrl` (Ctrl+t = ThemeNext) until the title/toast shows kindle-sepia, OR set `config-dev.json` `theme` to `kindle-sepia` while NO instance runs.

- [ ] **Step 4: Cycle root variants and screenshot each step**

`dollar` is the RPD `<TLDE>` cap; drive `RootVariantNext` with `wtype -k dollar`. Press it 6 times; after each press `sleep 0.6` and `grim` a screenshot. Read each PNG.
Expected: the root/wallpaper field steps through the 6 kindle-sepia candidates in authored order; the `Root [n/6]` toast shows the right N (1..6) and wraps back to 1 on the 6th→next. No computed off-family colors appear.

- [ ] **Step 5: Verify a 3-candidate theme wraps at 3**

Switch to sepia-lightest (Ctrl+t cycling or config edit while stopped). Press `dollar` 3 times; confirm the toast reads `Root [n/3]` and wraps at 3, walking the 3 authored candidates.

- [ ] **Step 6: Clean up**

Run: `pkill -f "cage -- ./target/debug/linux-lit"`
Confirm: `pgrep -af 'cage -- ./target/debug/linux-lit'` returns nothing; the user's live instance (if any) is untouched.

- [ ] **Step 7: Record findings**

Write a 3-4 line note: which candidate hexes were observed per theme, that they matched the JSON order, that the toast N was correct, and any contrast concern spotted by eye (vocab word / reader-gloss legibility on the more saturated candidates). No commit needed.

---

## Self-Review

**Spec coverage:**
- Verbatim candidates, authored order → Task 3 (`root_variants`), Task 2 (tests). ✓
- Per-theme count on `Theme` → Task 1, Task 3. ✓
- Cycler reads count → Task 4. ✓
- Index 0 = candidates[0] → Task 3 (direct index), Task 2 (`root_variants_are_the_candidates_verbatim`). ✓
- Out-of-range clamps to 0 → Task 3 (clamp), Task 2 (`out_of_range_variant_clamps_to_zero`). ✓
- No-candidates → single root → Task 3 (`root_variants` fallback), Task 2 (`no_candidates_falls_back_to_designed_root`), Task 1 (`default_theme` count 1). ✓
- `config.root_variant_for` drops modulo → Task 5. ✓
- Remove `ROOT_VARIANT_COUNT` → Task 3 (delete), Task 4/6 (call sites). ✓
- Delete computed-ladder tests → Task 6. ✓
- Card-surface / contrast invariants retained → Task 6 (retarget). ✓
- Headless verification → Task 7. ✓

**Placeholder scan:** No TBD/TODO. Task 1 uses an intentional placeholder count `1` explicitly replaced in Task 3 Step 3 (called out). No "handle edge cases" hand-waves — the clamp, empty-list, and fallback are shown in code.

**Type consistency:** `root_variants(&Value) -> Vec<String>` (Task 3) is the only new function; consumed only inside `resolve_theme_variant`. `Theme.root_variant_count: u8` named identically in Tasks 1, 3, 4, 6. `root_variant_for -> u8` unchanged signature (Task 5). `count` is `u8` in the cycler (Task 4) and `.len()...as u8` at resolve (Task 3) — consistent.
