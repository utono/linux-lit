# Multi-page overlay paragraph retention + glossed-cursor color — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a distinct per-theme color for a glossed line that is the cursor block (Feature B), and stop multi-page synopsis/gloss overlays from dropping label/echo paragraphs the single-page path keeps (Feature A).

**Architecture:** B derives BOTH reader-gloss tints (off-cursor + on-cursor) through a contrast/distinctness guard (`ensure_gloss_color`, modeled on the existing `choose_vocab_fg`) so they are legible on the reading bg and distinct from body text and each other — fixing the raw-focuscolor tint that is dim/indistinct on ~13 themes — then adds a second TextTag and flips `repaint_reader_gloss_visible` to apply the on-cursor color on the cursor line. A adds a display-only `attached: Vec<Attachment>` field to `GlossBlock`; the block builders attach label/echo paragraphs to a block instead of dropping them, and the multi-page render arms emit them.

**Tech Stack:** Rust, GTK4 (gtk4-rs), serde_json, SQLite (rusqlite). Pure-logic tests via `cargo test --bins`.

**Design doc:** `docs/plans/2026-06-29-overlay-paragraph-retention-and-gloss-cursor-color-design.md`

## Global Constraints

- Build with `cargo build`; never run the app (`cargo run`) — the user runs it. (CLAUDE.md)
- Pure-logic verification: `cargo build` clean + `cargo test --bins` green + `cargo clippy` parity (no NEW warnings; baseline is 122).
- Visual acceptance (color on screen, multi-page layout) is pixel-level and CANNOT be self-verified on the live dwl seat — ASK the user to run `./scripts/e2e-env.sh` or a manual launch, per CLAUDE.md "When to ASK THE USER to run e2e-env.sh".
- Timestamps US Central (`TZ='America/Chicago'`).
- US English spelling in code/comments (existing file convention: "color", not "colour").
- Commit messages end with the Co-Authored-By + Claude-Session trailer (see CLAUDE.md / existing commits).
- `~/.config/linux-lit/config-dev.json` is the dev config; do not touch it.

---

# FEATURE B — glossed-cursor color + contrast-guaranteed gloss tints (implement first)

> Revised after a 36-theme audit: the raw `focuscolor` off-cursor tint is dim or
> near-body-color on 13 themes, and the naive complement has its own failures. So
> BOTH gloss colors are derived through a contrast/distinctness guard
> (`ensure_gloss_color`), modeled on the existing `choose_vocab_fg`. Themes that
> already look right are returned unchanged by the guard.

## Task B1: `complement_hex` + `contrast_ratio` helpers in theme.rs

**Files:**
- Modify: `src/theme.rs` — add `complement_hex` and `contrast_ratio` near the other color helpers (after `hsl_to_rgb`/`hue_distance`, ~line 346); add unit tests in the in-file `#[cfg(test)] mod tests` (it exists — `choose_vocab_fg` etc. are tested there; if not, create `mod tests` at end of file).

**Interfaces:**
- Consumes: existing `hex_to_rgb`, `rgb_to_hsl`, `hsl_to_rgb`, `rgb_to_hex` (all in `src/theme.rs`).
- Produces:
  - `fn complement_hex(hex: &str) -> String` — `hex` with hue rotated 180°, same S/L.
  - `fn contrast_ratio(a_hex: &str, b_hex: &str) -> f64` — WCAG contrast ratio (1.0–21.0) between two hex colors.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/theme.rs`:

```rust
#[test]
fn complement_rotates_hue_180() {
    let c = complement_hex("#c4788a");
    let (h_in, _, _) = rgb_to_hsl(hex_to_rgb("#c4788a").0, hex_to_rgb("#c4788a").1, hex_to_rgb("#c4788a").2);
    let (h_out, _, _) = rgb_to_hsl(hex_to_rgb(&c).0, hex_to_rgb(&c).1, hex_to_rgb(&c).2);
    let diff = ((h_out - h_in).abs() - 0.5).abs();
    assert!(diff < 0.02, "expected ~0.5 hue rotation, in={h_in} out={h_out} ({c})");
    assert!((0.33..=0.70).contains(&h_out), "complement of a red should be teal/green, got {h_out} ({c})");
}

#[test]
fn complement_malformed_is_safe() {
    let c = complement_hex("nope");
    assert!(c.starts_with('#') && c.len() == 7, "got {c}");
}

#[test]
fn contrast_ratio_known_pairs() {
    assert!((contrast_ratio("#ffffff", "#000000") - 21.0).abs() < 0.1);
    assert!((contrast_ratio("#888888", "#888888") - 1.0).abs() < 0.01);
    // a mid case is between
    let c = contrast_ratio("#c4788a", "#faf4ed");
    assert!(c > 2.5 && c < 3.5, "rose on cream ~3.0, got {c}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins theme::tests::complement theme::tests::contrast -- --nocapture`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement the helpers**

Add after `hue_distance` (~346) in `src/theme.rs`:

```rust
/// Return `hex` with its hue rotated 180° (the color-wheel complement), keeping
/// saturation and lightness. Malformed input degrades to the complement of black
/// (still a valid `#rrggbb`); never panics.
fn complement_hex(hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let (nr, ng, nb) = hsl_to_rgb((h + 0.5) % 1.0, s, l);
    rgb_to_hex(nr, ng, nb)
}

/// WCAG relative-luminance contrast ratio between two hex colors, 1.0 (identical)
/// to 21.0 (black on white). Used to keep the gloss tints legible on the reading
/// background and distinct from body text.
fn contrast_ratio(a_hex: &str, b_hex: &str) -> f64 {
    let lin = |c: f64| if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let lum = |hex: &str| {
        let (r, g, b) = hex_to_rgb(hex);
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    };
    let (la, lb) = (lum(a_hex) + 0.05, lum(b_hex) + 0.05);
    if la > lb { la / lb } else { lb / la }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --bins theme::tests -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "$(cat <<'EOF'
feat(theme): complement_hex + contrast_ratio helpers

Hue complement (180°) and WCAG contrast ratio, composed from the existing
hex/hsl helpers. Used next to derive contrast-guaranteed gloss tints.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task B2: `ensure_gloss_color` contrast/distinctness guard

**Files:**
- Modify: `src/theme.rs` — add `ensure_gloss_color` near `choose_vocab_fg` (~351); tests in the `tests` module.

**Interfaces:**
- Consumes: `contrast_ratio` (B1), `hue_distance`, `hex_to_rgb`, `rgb_to_hsl`, `hsl_to_rgb`, `rgb_to_hex` (existing).
- Produces: `fn ensure_gloss_color(base_hex: &str, bg_hex: &str, avoid: &[&str]) -> String` — a color at `base_hex`'s hue with WCAG contrast ≥ 3.0 vs `bg_hex`, distinct (hue ≥ 40° OR contrast ≥ 1.4) from every color in `avoid`. Returns `base_hex` unchanged when it already qualifies.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn ensure_keeps_already_good_color() {
    // rose-pine-dawn focuscolor on cream, avoiding slate body text: already good.
    let c = ensure_gloss_color("#c4788a", "#faf4ed", &["#575279"]);
    assert_eq!(c, "#c4788a", "a color that already passes must be returned unchanged");
}

#[test]
fn ensure_fixes_dim_color_on_light_bg() {
    // dayfox: a muted purple focuscolor on a near-white bg is too dim.
    let c = ensure_gloss_color("#7b6b99", "#f6f2ee", &["#3d2b5a"]);
    assert!(contrast_ratio(&c, "#f6f2ee") >= 3.0,
        "fixed color must contrast with bg, got {} ({c})", contrast_ratio(&c, "#f6f2ee"));
}

#[test]
fn ensure_result_is_distinct_from_avoid() {
    let c = ensure_gloss_color("#7b6b99", "#f6f2ee", &["#3d2b5a"]);
    let distinct = hue_distance(&c, "#3d2b5a") >= 40.0 || contrast_ratio(&c, "#3d2b5a") >= 1.4;
    assert!(distinct, "result {c} must be distinct from the avoid color");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins theme::tests::ensure -- --nocapture`
Expected: FAIL — `ensure_gloss_color` not found.

- [ ] **Step 3: Implement**

Add near `choose_vocab_fg` in `src/theme.rs`:

```rust
/// Return a color at `base_hex`'s hue that is legible on `bg_hex` (WCAG contrast
/// ≥ 3.0) and visually distinct (hue distance ≥ 40° OR contrast ≥ 1.4) from each
/// color in `avoid`. If `base_hex` already qualifies it is returned unchanged, so
/// themes that already look right do not move. Otherwise lightness is pushed away
/// from the background and saturation raised at the same hue; as a last resort the
/// hue is rotated 150° (the `choose_vocab_fg` strategy) and S/L clamped. Used to
/// derive both reader-gloss tints so they never wash out or blend into body text.
fn ensure_gloss_color(base_hex: &str, bg_hex: &str, avoid: &[&str]) -> String {
    let ok = |c: &str| {
        contrast_ratio(c, bg_hex) >= 3.0
            && avoid.iter().all(|a| hue_distance(c, a) >= 40.0 || contrast_ratio(c, a) >= 1.4)
    };
    if ok(base_hex) {
        return base_hex.to_string();
    }
    let (br, bg_, bb) = hex_to_rgb(base_hex);
    let (h, s, _l) = rgb_to_hsl(br, bg_, bb);
    let bg_is_light = contrast_ratio(bg_hex, "#000000") > contrast_ratio(bg_hex, "#ffffff");
    // Push lightness toward the side with headroom against the bg; raise S.
    let s2 = s.max(0.50);
    for &l in if bg_is_light {
        &[0.42_f64, 0.36, 0.30, 0.24][..]   // darker, for a light bg
    } else {
        &[0.62_f64, 0.68, 0.74, 0.80][..]   // lighter, for a dark bg
    } {
        let (r, g, b) = hsl_to_rgb(h, s2, l);
        let cand = rgb_to_hex(r, g, b);
        if ok(&cand) {
            return cand;
        }
    }
    // Last resort: rotate hue 150° (matches choose_vocab_fg) and clamp.
    let new_h = (h + 150.0 / 360.0) % 1.0;
    let l = if bg_is_light { 0.36 } else { 0.70 };
    let (r, g, b) = hsl_to_rgb(new_h, s.max(0.50), l);
    rgb_to_hex(r, g, b)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --bins theme::tests -- --nocapture`
Expected: PASS (all three new + existing theme tests).

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "$(cat <<'EOF'
feat(theme): ensure_gloss_color contrast/distinctness guard

Returns a color at the base hue that is legible on the bg (WCAG >= 3.0) and
distinct from given avoid colors, leaving an already-good color unchanged.
Modeled on choose_vocab_fg. Used next to derive both reader-gloss tints.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task B3: `reader_gloss` + `reader_gloss_cursor` on Theme + all-themes invariant

**Files:**
- Modify: `src/theme.rs` — `Theme` struct (~19, after `vocab_fg`); `resolve_theme` return (~164); `default_theme` (~180); tests.

**Interfaces:**
- Consumes: `ensure_gloss_color` (B2), `complement_hex` (B1), existing `focus_color`/`text_bg`/`text_fg`/`lit`/`str_field`.
- Produces:
  - `Theme.reader_gloss: String` — the off-cursor gloss tint (guarded focuscolor).
  - `Theme.reader_gloss_cursor: String` — the on-cursor color (guarded complement).
  Each: `linux-lit.<key>` override if present, else derived.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn reader_gloss_cursor_explicit_wins() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{ "dwl": {"focuscolor": "#c4788a"},
             "linux-lit": {"reader_gloss_cursor": "#56949f"},
             "kitty": {"background": "#faf4ed", "active_tab_foreground": "#575279"} }"#,
    ).unwrap();
    let t = resolve_theme("rose-pine-dawn", &json);
    assert_eq!(t.reader_gloss_cursor, "#56949f");
    // off-cursor tint: focuscolor already passes -> unchanged.
    assert_eq!(t.reader_gloss, "#c4788a");
}

#[test]
fn reader_gloss_colors_are_legible_and_distinct_for_all_themes() {
    // The audit invariant: every shipped theme yields a legible, mutually
    // distinct pair. Guards against a future theme regressing.
    for t in load_all_themes() {
        let cvb_off = contrast_ratio(&t.reader_gloss, &t.text_bg);
        let cvb_cur = contrast_ratio(&t.reader_gloss_cursor, &t.text_bg);
        assert!(cvb_off >= 3.0, "{}: off-cursor tint {} dim on bg {} ({cvb_off:.2})", t.name, t.reader_gloss, t.text_bg);
        assert!(cvb_cur >= 3.0, "{}: on-cursor color {} dim on bg {} ({cvb_cur:.2})", t.name, t.reader_gloss_cursor, t.text_bg);
        let distinct = hue_distance(&t.reader_gloss, &t.reader_gloss_cursor) >= 40.0
            || contrast_ratio(&t.reader_gloss, &t.reader_gloss_cursor) >= 1.4;
        assert!(distinct, "{}: off {} and on {} not distinct", t.name, t.reader_gloss, t.reader_gloss_cursor);
    }
}
```

NOTE: `load_all_themes()` reads the real `themes-unified.json`. If it is missing in CI the call returns `[default_theme()]` (see theme.rs:48) — the loop still runs over the default, so the test never hard-fails on a missing file.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins theme::tests::reader_gloss -- --nocapture`
Expected: FAIL — no `reader_gloss`/`reader_gloss_cursor` fields.

- [ ] **Step 3: Implement**

In `src/theme.rs`:

1. Add fields to `struct Theme` after `vocab_fg: String,` (~19):

```rust
    pub reader_gloss: String,        // off-cursor glossed-line tint (guarded)
    pub reader_gloss_cursor: String, // glossed line that is ALSO the cursor block
```

2. In `resolve_theme`, after `let lit = ...` and the `focus_color`/`text_bg`/`text_fg` lines are all in scope (~134), add:

```rust
    // Reader-gloss tints, contrast-guaranteed (raw focuscolor is dim/indistinct
    // on ~13 themes). Off-cursor = guarded focuscolor; on-cursor = guarded
    // complement, also kept distinct from the off-cursor tint.
    let reader_gloss = str_field(&lit, "reader_gloss")
        .unwrap_or_else(|| ensure_gloss_color(&focus_color, &text_bg, &[&text_fg]));
    let reader_gloss_cursor = str_field(&lit, "reader_gloss_cursor").unwrap_or_else(|| {
        ensure_gloss_color(&complement_hex(&reader_gloss), &text_bg, &[&text_fg, &reader_gloss])
    });
```

3. Add `reader_gloss,` and `reader_gloss_cursor,` to the returned `Theme { ... }` (after `vocab_fg,` ~176).

4. In `default_theme` (~193, after `vocab_fg`), add:

```rust
        reader_gloss: ensure_gloss_color("#d4be98", "#282828", &["#d4be98"]),
        reader_gloss_cursor: ensure_gloss_color(&complement_hex("#d4be98"), "#282828", &["#d4be98"]),
```

(NOTE: default's focuscolor == text_fg == `#d4be98`; the guard will rotate it to a distinct, legible color — that is correct, the default theme had no real gloss color before.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --bins theme:: -- --nocapture`
Expected: PASS, including the all-themes invariant. If a specific theme fails the invariant, the assert message names it + its colors — tighten `ensure_gloss_color`'s lightness ladder for that bg until it passes (do NOT special-case the theme).

- [ ] **Step 5: Build + bins + commit**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
git add src/theme.rs
git commit -m "$(cat <<'EOF'
feat(theme): contrast-guaranteed reader_gloss + reader_gloss_cursor

Both reader-gloss tints are now derived through ensure_gloss_color so they
are legible on the reading bg and distinct from body text and each other —
fixing the dim/washed-out off-cursor tint on ~13 themes (dayfox,
melange-light, everforest-light-*, solarized-light, ...). Either may be
overridden per theme via linux-lit.reader_gloss / .reader_gloss_cursor. A
new test asserts the invariant for every shipped theme.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task B4: rose-pine-dawn explicit cursor value in themes-unified.json

**Files:**
- Modify: `~/utono/themes/.config/themes/themes-unified.json` — `rose-pine-dawn.linux-lit` (separate repo).

**Interfaces:**
- Produces: `rose-pine-dawn.linux-lit.reader_gloss_cursor == "#56949f"` (foam). `reader_gloss` is NOT set there — the guard returns `#c4788a` unchanged.

- [ ] **Step 1: Add the key**

Edit `rose-pine-dawn` → `linux-lit` to:

```json
"linux-lit": { "cursor_line_bg": "rgba(196, 120, 138, 0.2)", "reader_gloss_cursor": "#56949f" }
```

- [ ] **Step 2: Verify**

Run: `jq '."rose-pine-dawn"."linux-lit".reader_gloss_cursor' ~/utono/themes/.config/themes/themes-unified.json`
Expected: `"#56949f"`

- [ ] **Step 3: Commit in the themes repo**

```bash
cd ~/utono/themes && git add .config/themes/themes-unified.json && git commit -m "$(cat <<'EOF'
feat(rose-pine-dawn): reader_gloss_cursor #56949f (foam) for linux-lit

The on-cursor glossed color for linux-lit's reading card: rosé-pine "foam",
the complement of the #c4788a focuscolor gloss tint.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
cd ~/utono/linux-lit
```

---

## Task B5: tags + helpers + state field (use theme.reader_gloss)

**Files:**
- Modify: `src/app/mod.rs` — `AppState` field (~392, after `reader_gloss_tag`); the existing `reader-gloss-line` tag (~914) now uses `theme.reader_gloss` (was `theme.focus_color`); add the second tag; state construction (~1550); apply/remove helpers (~3818).

**Interfaces:**
- Consumes: `theme.reader_gloss`, `theme.reader_gloss_cursor` (B3).
- Produces: `AppState.reader_gloss_cursor_tag: gtk4::TextTag`; `apply_reader_gloss_cursor_tag_to_line` / `remove_reader_gloss_cursor_tag_from_line`.

Verified by `cargo build`; behavior consumed by B6.

- [ ] **Step 1: Point the existing tint tag at theme.reader_gloss**

In `src/app/mod.rs` ~916, change:

```rust
        .foreground(&theme.focus_color)
```

(inside the `reader-gloss-line` builder) to:

```rust
        .foreground(&theme.reader_gloss)
```

Update its comment (~906–913): the tint is now the contrast-guarded gloss color, not the raw dwl focuscolor.

- [ ] **Step 2: Add the AppState field**

After `pub reader_gloss_tag: gtk4::TextTag,` (~392):

```rust
    /// Foreground tag for a glossed line that is ALSO the cursor block — a
    /// distinct, contrast-guarded color (theme.reader_gloss_cursor) so it reads
    /// differently from both body text and the off-cursor gloss tint. Applied by
    /// `repaint_reader_gloss_visible` on the cursor line.
    pub reader_gloss_cursor_tag: gtk4::TextTag,
```

- [ ] **Step 3: Create the tag**

After `buffer.tag_table().add(&reader_gloss_tag);` (~918):

```rust
    // The on-cursor glossed tint: same role as reader-gloss-line but a distinct
    // color, applied while a glossed line is the cursor block. Added after
    // reader-gloss-line so it outranks it on the cursor's own line.
    let reader_gloss_cursor_tag = gtk4::TextTag::builder()
        .name("reader-gloss-cursor-line")
        .foreground(&theme.reader_gloss_cursor)
        .build();
    buffer.tag_table().add(&reader_gloss_cursor_tag);
```

- [ ] **Step 4: Store it in AppState**

After `reader_gloss_tag,` in construction (~1550):

```rust
        reader_gloss_cursor_tag,
```

- [ ] **Step 5: Add apply/remove helpers**

After `remove_reader_gloss_tag_from_line` (~3838):

```rust
/// Apply the on-cursor glossed tint to a single buffer line.
pub(crate) fn apply_reader_gloss_cursor_tag_to_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.apply_tag(&state.reader_gloss_cursor_tag, &start, &end);
    }
}

/// Remove the on-cursor glossed tint from a single buffer line.
pub(crate) fn remove_reader_gloss_cursor_tag_from_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.remove_tag(&state.reader_gloss_cursor_tag, &start, &end);
    }
}
```

- [ ] **Step 6: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: Finished. No commit yet — commit with B6.

---

## Task B6: flip `repaint_reader_gloss_visible` + theme-change refresh

**Files:**
- Modify: `src/input/highlight.rs` — `repaint_reader_gloss_visible` (~346–357) + its doc comment.
- Modify: `src/input/actions/settings.rs` — theme-change refresh (~285).

**Interfaces:**
- Consumes: B5's helpers + `state.reader_gloss_cursor_tag`; `theme.reader_gloss`, `theme.reader_gloss_cursor`.
- Produces: three-state behavior (normal / off-cursor tint / on-cursor color), live on theme change.

Verified by `cargo build` + `cargo test --bins` parity + the user's screenshot.

- [ ] **Step 1: Flip the cursor-line case**

Replace the loop body in `src/input/highlight.rs` (~350–356):

```rust
    for &buf_idx in &state.reader_gloss_lines {
        if buf_idx == state.current_line {
            crate::app::remove_reader_gloss_tag_from_line(state, buf_idx);
        } else {
            crate::app::apply_reader_gloss_tag_to_line(state, buf_idx);
        }
    }
```

with:

```rust
    for &buf_idx in &state.reader_gloss_lines {
        if buf_idx == state.current_line {
            // Cursor on a glossed line: show the distinct on-cursor color.
            crate::app::remove_reader_gloss_tag_from_line(state, buf_idx);
            crate::app::apply_reader_gloss_cursor_tag_to_line(state, buf_idx);
        } else {
            // Off-cursor glossed line: the gloss tint; clear any stale on-cursor color.
            crate::app::remove_reader_gloss_cursor_tag_from_line(state, buf_idx);
            crate::app::apply_reader_gloss_tag_to_line(state, buf_idx);
        }
    }
```

Update the fn doc comment (~339–345): the cursor line now gets the distinct
on-cursor color (was: left un-tinted).

- [ ] **Step 2: Refresh both tags on theme change**

In `src/input/actions/settings.rs`, change the existing line (~285):

```rust
    state.reader_gloss_tag.set_property("foreground", &theme.focus_color);
```

to:

```rust
    state.reader_gloss_tag.set_property("foreground", &theme.reader_gloss);
    state.reader_gloss_cursor_tag.set_property("foreground", &theme.reader_gloss_cursor);
```

(Update the nearby comment: the tints are the guarded gloss colors now.)

- [ ] **Step 3: Build + bins + clippy**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c '^warning'   # expect 122 baseline (no new)
```
Expected: build Finished; bins green (517 baseline + new theme tests); clippy 122.

- [ ] **Step 4: Commit B5 + B6 together**

```bash
git add src/app/mod.rs src/input/highlight.rs src/input/actions/settings.rs
git commit -m "$(cat <<'EOF'
feat(reader): distinct on-cursor color for a glossed line; guarded tints

A glossed line now has three states on the reading card: normal body text;
the contrast-guarded off-cursor gloss tint (theme.reader_gloss, was the raw
focuscolor); and a distinct on-cursor color (theme.reader_gloss_cursor) when
it is the cursor block. Adds the reader-gloss-cursor-line tag + helpers, flips
repaint_reader_gloss_visible to apply it on the cursor line, and refreshes both
tints on theme change.

Logic-verified (cargo test --bins, incl. the all-themes contrast invariant);
visual acceptance needs e2e screenshots on the previously-dim themes — see ac.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

- [ ] **Step 5: ASK THE USER to verify on screen**

Per CLAUDE.md the agent cannot launch on the live dwl seat. Ask the user to open Bleak House → "In Chancery" and confirm, on at least:
- **rose-pine-dawn:** cursor ON the glossed first paragraph → teal `#56949f`; OFF → rose `#c4788a`; non-glossed cursor line → normal slate.
- **dayfox** and **melange-light** (previously dim): the OFF-cursor glossed paragraph now reads clearly tinted (not washed out), and ON-cursor shows a distinct color.

Provide the manual single-work launch from CLAUDE.md "Headless Verification" + `grim` per theme (the user switches theme with super+\ / their theme toggle), or the e2e smoke `./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture`.

---

# FEATURE A — multi-page label/echo retention (implement second)

## Task A1: `Attachment` enum + `attached` field on GlossBlock

**Files:**
- Modify: `src/ui/gloss_block.rs` — add `Attachment` enum near `BlockKind` (~71); add `attached: Vec<Attachment>` to `GlossBlock` (~79); update ALL `GlossBlock { ... }` literals in the file to add `attached: Vec::new()` (in `synopsis_blocks` ~159 & ~172, `gloss_blocks`' `flush_source` ~198 and explication push ~222).

**Interfaces:**
- Produces:
  - `pub enum Attachment { LeadLabel(String), TrailEcho(String) }` (derive `Clone`, `Debug`, `PartialEq`, `Eq`).
  - `GlossBlock.attached: Vec<Attachment>` — display-only paragraphs riding with the block.

This is a pure data change; it compiles and existing tests still pass (every block just gets an empty `attached`). Its own test asserts the default is empty.

- [ ] **Step 1: Write the failing test**

Add to the `block_tests` module in `src/ui/gloss_block.rs`:

```rust
#[test]
fn blocks_default_to_no_attachments() {
    let g = "<speaker>X</speaker>\n<verse>a line</verse>\n<gloss>note</gloss>";
    for b in gloss_blocks(g) {
        assert!(b.attached.is_empty(), "fresh block must have no attachments");
    }
    for b in synopsis_blocks("<p>One.</p><p>Two.</p>") {
        assert!(b.attached.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: FAIL — no field `attached` on `GlossBlock`.

- [ ] **Step 3: Implement the enum + field**

In `src/ui/gloss_block.rs`, after `BlockKind` (~75):

```rust
/// A non-cursor-stop paragraph that rides WITH a block on a paginated page so it
/// is not dropped at a page boundary (the single-page render keeps it; without
/// this the multi-page render would lose it). Display-only — never a cursor stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// A bold label paragraph that HEADS the block (synopsis, e.g.
    /// "Shakespearean parallels:"). Rendered above the block body.
    LeadLabel(String),
    /// An echo-bracket markup ("<gloss>[...]</gloss>") that TRAILS the block
    /// (gloss). Rendered below the block body via the echo render path.
    TrailEcho(String),
}
```

Add to `GlossBlock` after `pub display: String,` (~90):

```rust
    /// Non-cursor-stop paragraphs that ride with this block on a paginated page.
    /// Empty in the common case. See `Attachment`.
    pub attached: Vec<Attachment>,
```

Add `attached: Vec::new(),` to EVERY `GlossBlock { ... }` literal in the file:
- `synopsis_blocks` legacy single block (~159) and the per-`<p>` push (~172),
- `gloss_blocks`'s `flush_source` closure source push (~198) and the explication push (~222).

(Find them all: `rg -n "GlossBlock \{" src/ui/gloss_block.rs` — there should be exactly 4. Add the field to each.)

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: PASS (new test + all existing block/synopsis tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(gloss-block): Attachment enum + GlossBlock.attached (empty default)

Display-only paragraphs that ride with a block across a page turn. No
behavior change yet — every block gets an empty attached; the builders
populate it in the next tasks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A2: `synopsis_blocks` attaches labels as LeadLabel

**Files:**
- Modify: `src/ui/gloss_block.rs` — `synopsis_blocks` (~140–181).

**Interfaces:**
- Consumes: `Attachment::LeadLabel` (A1); existing `is_label_paragraph`.
- Produces: `synopsis_blocks` returns the SAME blocks/indices as before, but each block's `attached` carries any label paragraph(s) immediately preceding it as `LeadLabel`. A trailing label (after the last block) attaches to the LAST block.

- [ ] **Step 1: Write the failing test**

Add to `synopsis_blocks_tests` in `src/ui/gloss_block.rs`:

```rust
#[test]
fn label_attaches_as_lead_to_following_block() {
    let syn = "<p>First paragraph of action.</p>\
               <p>Shakespearean parallels:</p>\
               <p>Second paragraph continues.</p>";
    let blocks = synopsis_blocks(syn);
    // Still 2 cursor-stop blocks, indices unchanged.
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].index, 0);
    assert_eq!(blocks[1].index, 1);
    // Block 0 has no lead; the label heads block 1.
    assert!(blocks[0].attached.is_empty());
    assert_eq!(
        blocks[1].attached,
        vec![super::Attachment::LeadLabel("Shakespearean parallels:".to_string())]
    );
}

#[test]
fn trailing_label_attaches_to_last_block() {
    let syn = "<p>Only paragraph.</p><p>Afterword:</p>";
    let blocks = synopsis_blocks(syn);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].attached,
        vec![super::Attachment::LeadLabel("Afterword:".to_string())]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins synopsis_blocks_tests -- --nocapture`
Expected: FAIL — `attached` is empty (labels still skipped).

- [ ] **Step 3: Implement label buffering**

Rewrite the block-emitting loop in `synopsis_blocks` (~166–180) so a label is buffered and attached to the next emitted block; a trailing label attaches to the last:

```rust
    let mut blocks: Vec<GlossBlock> = Vec::new();
    let mut index = 0i32;
    let mut pending_labels: Vec<Attachment> = Vec::new();
    for p in &paras {
        if is_label_paragraph(p) {
            pending_labels.push(Attachment::LeadLabel(p.clone()));
            continue;
        }
        blocks.push(GlossBlock {
            kind: BlockKind::Explication,
            index,
            text: p.clone(),
            display: p.clone(),
            attached: std::mem::take(&mut pending_labels),
        });
        index += 1;
    }
    // A label after the last block: attach to the last block (so it is not lost).
    if !pending_labels.is_empty() {
        if let Some(last) = blocks.last_mut() {
            last.attached.append(&mut pending_labels);
        }
    }
    blocks
```

(Import `Attachment` is already in-module — same file. No `use` needed.)

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: PASS (new tests + existing `each_p_becomes_one_explication_block_skipping_labels`, which still sees 2 blocks).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(synopsis): attach label paragraphs as LeadLabel to the next block

synopsis_blocks no longer drops a label paragraph; it buffers it and
attaches it to the block it heads (trailing label -> last block). Block
count and indices are unchanged; the multi-page render reads attached.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A3: `gloss_block_markups` attaches echoes as TrailEcho

**Files:**
- Modify: `src/ui/gloss_block.rs` — `gloss_block_markups` (~261–301). NOTE: `gloss_block_markups` returns `Vec<String>` (markup per block), NOT `Vec<GlossBlock>`. The echo must be appended to the PRECEDING block's markup STRING (so the existing `markups[start..end].join` carries it). `gloss_blocks` (the `Vec<GlossBlock>`) stays unchanged — echoes are still not cursor stops there.

**Interfaces:**
- Consumes: existing `split_echo`, `parse_gloss_tags`.
- Produces: `gloss_block_markups` returns one markup per cursor-stop block (count == `gloss_blocks().len()`, unchanged), but a markup now also contains any echo `<gloss>[...]</gloss>` that trailed it (appended after the block's own markup, separated by `\n`).

- [ ] **Step 1: Write the failing test**

Replace/extend the existing `block_markups_match_blocks_count_and_order` test's echo assertion. Add a NEW test to `block_tests`:

```rust
#[test]
fn echo_attaches_to_preceding_block_markup() {
    let gloss = "<speaker>CRANMER</speaker>\n\
                 <verse>Ah, my good Lord of Winchester, I thank you.</verse>\n\
                 <gloss>Cranmer opens with cutting irony.</gloss>\n\
                 <gloss>[\"a quote\" — Macbeth 1.1]</gloss>";
    let blocks = gloss_blocks(gloss);
    let markups = gloss_block_markups(gloss);
    // Count still 1:1 with cursor-stop blocks (echo is NOT a new entry).
    assert_eq!(markups.len(), blocks.len());
    // The echo rides in the LAST block's markup (the explication it trails).
    assert!(
        markups.last().unwrap().contains("a quote"),
        "echo must be appended to the preceding block markup, got {:?}",
        markups.last()
    );
}
```

ALSO update `block_markups_match_blocks_count_and_order` (~643): its final assertion is `assert!(!markups.iter().any(|m| m.contains("a quote")));` — that is now WRONG (the echo is retained). Change it to assert the echo IS present in some markup:

```rust
        // The echo bracket now RIDES in the preceding block's markup (retained
        // across page turns) rather than being dropped.
        assert!(markups.iter().any(|m| m.contains("a quote")),
            "echo should be retained in a block markup");
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: FAIL — echo absent (`block_markups_match_blocks_count_and_order` now fails its flipped assert; the new test fails).

- [ ] **Step 3: Implement echo attachment**

In `gloss_block_markups`, change the echo arm so the echo is appended to the last pushed markup instead of `continue`-dropping. Replace the `Gloss` arm (~289–295):

```rust
            GlossElement::Gloss(text) => {
                if split_echo(&text).is_some() {
                    // Echo bracket: not a cursor stop, but RETAIN it by appending
                    // to the preceding block's markup so a page turn keeps it.
                    // (An echo before any block — not observed — flushes the
                    // pending source run first, then attaches to it.)
                    flush_source(&mut markups, &mut pending, &mut pending_has_body);
                    let echo = format!("<gloss>{}</gloss>", text);
                    if let Some(last) = markups.last_mut() {
                        last.push('\n');
                        last.push_str(&echo);
                    } else {
                        markups.push(echo);
                    }
                    continue;
                }
                flush_source(&mut markups, &mut pending, &mut pending_has_body);
                markups.push(format!("<gloss>{}</gloss>", text));
            }
```

WARNING — count invariant: when an echo trails a SOURCE block with no explication between (the all-echo gloss case, e.g. the `["..." — ...]`-only glosses in lit.db), `flush_source` first pushes the source markup, then the echo appends to it — so count stays 1:1 with `gloss_blocks` (which also makes one Source block there). The "echo before any block" `else` branch pushes a lone markup; this would break 1:1, but no such data exists (every observed echo follows a source). Keep the `else` as a safety net but it should never fire for real data.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: PASS — including `all_echo_gloss_has_only_source_block` (gloss_blocks unchanged), `block_markups_match_blocks_count_and_order` (flipped assert), and the new test. If `block_markups_lone_pron_yields_nothing` or the all-echo count tests fail, the `else` branch fired — re-check.

VERIFY the all-echo case count explicitly by adding:

```rust
#[test]
fn all_echo_gloss_markups_count_matches_blocks() {
    // The lit.db echoes-only style: every gloss is an echo trailing its source.
    let g = "<speaker>PARIS</speaker>\n<verse>Come you to make confession?</verse>\n\
             <gloss>[\"q1\" — Ado 4.1]</gloss>\n\
             <speaker>JULIET</speaker>\n<verse>To answer that.</verse>\n\
             <gloss>[\"q2\" — Oth 1.1]</gloss>";
    assert_eq!(gloss_block_markups(g).len(), gloss_blocks(g).len());
    // Each echo rides with its own source block.
    let m = gloss_block_markups(g);
    assert!(m[0].contains("q1") && m[1].contains("q2"), "got {m:?}");
}
```

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(gloss): retain echo brackets in the preceding block's markup

gloss_block_markups no longer drops echo <gloss>[...] brackets; it appends
each to the markup of the block it trails, so a paginated gloss keeps its
echoes (markups stay 1:1 with gloss_blocks). gloss_blocks (cursor stops)
is unchanged — echoes are still not cursor stops.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A4: measure attachments in `gloss_block_height`

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `gloss_block_height` (find it: `rg -n "fn gloss_block_height" src/ui/gloss_overlay.rs`).

**Interfaces:**
- Consumes: `GlossBlock.attached` (A1); existing `measure_text_height` / pango context already used in `gloss_block_height`.
- Produces: `gloss_block_height` returns a height that INCLUDES the block's attachments, so pagination reserves room for them and nothing clips.

No standalone unit test (it needs a live pango context, like the rest of `gloss_block_height` — there are deliberately no pure tests for the measurement path per CLAUDE.md). Verified by `cargo build` + the e2e spread.

- [ ] **Step 1: The current function (already read)**

`gloss_block_height` (src/ui/gloss_overlay.rs:2188) is exactly:

```rust
fn gloss_block_height(block: &GlossBlock, pctx: &pango::Context, family: &str, size_pt: i32, wrap_w: i32) -> i32 {
    let text_h = crate::ui::pagination::measure_text_height(pctx, &block.display, size_pt, family, wrap_w);
    block_height_overhead(block.kind == BlockKind::Source, text_h)
}
```

There is NO line-height local. Use `measure_text_height` (already imported via the `crate::ui::pagination` path) for each attachment and a conservative per-attachment line allowance of `size_pt + size_pt/2` (over-measure is the safe direction per `repaginate`'s comment).

- [ ] **Step 2: Add attachment height**

Rewrite `gloss_block_height` to add the attachments' measured height before returning:

```rust
fn gloss_block_height(block: &GlossBlock, pctx: &pango::Context, family: &str, size_pt: i32, wrap_w: i32) -> i32 {
    let text_h = crate::ui::pagination::measure_text_height(pctx, &block.display, size_pt, family, wrap_w);
    let mut h = block_height_overhead(block.kind == BlockKind::Source, text_h);
    // Reserve room for attachments so a paginated page never clips them
    // (over-measure: one extra line allowance per attachment as a gap/citation).
    let line = size_pt + size_pt / 2;
    for a in &block.attached {
        match a {
            crate::ui::gloss_block::Attachment::LeadLabel(s) => {
                h += crate::ui::pagination::measure_text_height(pctx, s, size_pt, family, wrap_w) + line;
            }
            crate::ui::gloss_block::Attachment::TrailEcho(markup) => {
                let inner = markup.trim_start_matches("<gloss>").trim_end_matches("</gloss>");
                h += crate::ui::pagination::measure_text_height(pctx, inner, size_pt, family, wrap_w) + line * 2;
            }
        }
    }
    h
}
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: Finished clean.

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
feat(overlay): measure block attachments in gloss_block_height

Pagination now reserves room for a block's lead labels / trailing echoes
so a paginated page never clips them. Over-measures (the safe direction).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A5: emit LeadLabels in `render_synopsis_page` multi-page branch

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `render_synopsis_page` multi-page branch (~1462–1476).

**Interfaces:**
- Consumes: `GlossBlock.attached` / `Attachment::LeadLabel` (A1); existing `synopsis_label_ranges`, `apply_synopsis_label_bold`, `rebuild_block_ranges_from`.
- Produces: the multi-page synopsis page body includes each block's `LeadLabel`(s) bolded above its body, and `synopsis_label_ranges` is set to those labels' char offsets (instead of cleared).

No unit test (GTK buffer render); verified by `cargo build` + the e2e screenshot of a multi-page synopsis.

- [ ] **Step 1: Rewrite the multi-page body build**

Replace the `else` (paginated) branch of `render_synopsis_page` (~1462–1476):

```rust
        } else {
            // Paginated: render this page's blocks, each preceded by its lead
            // label(s) (bolded) so labels survive the page turn. Track label
            // char-offset ranges in the page text so apply_synopsis_label_bold
            // can bold them.
            let Some(page) = page else { return };
            let all = self.all_blocks.borrow();
            let slice: Vec<GlossBlock> = all[page.start..page.end.min(all.len())].to_vec();
            drop(all);
            let mut body = String::new();
            let mut label_ranges: Vec<(usize, usize)> = Vec::new();
            let mut char_off = 0usize; // char offset into `body`
            for b in &slice {
                for a in &b.attached {
                    if let crate::ui::gloss_block::Attachment::LeadLabel(lbl) = a {
                        if !body.is_empty() {
                            body.push_str("\n\n");
                            char_off += 2;
                        }
                        let len = lbl.chars().count();
                        label_ranges.push((char_off, char_off + len));
                        body.push_str(lbl);
                        char_off += len;
                    }
                }
                if !body.is_empty() {
                    body.push_str("\n\n");
                    char_off += 2;
                }
                let len = b.display.chars().count();
                body.push_str(&b.display);
                char_off += len;
            }
            buffer.set_text(&body);
            *self.synopsis_label_ranges.borrow_mut() = label_ranges;
            self.apply_synopsis_label_bold();
            self.rebuild_block_ranges_from(slice);
        }
```

CONFIRMED (no change needed): `rebuild_block_ranges_from` (src/ui/gloss_overlay.rs:1173) finds each block by SEARCHING the buffer text for the block's first `display` line (`find_line` → `line_text.trim().starts_with(needle)`, advancing `search_from`). Inserted lead-label lines are simply skipped by the search, so block ranges stay correct with labels present. No edit to `rebuild_block_ranges_from`. (Soft edge: a label whose text `starts_with` a following block's first-line needle could match one line early — bounded and harmless, since labels are short `:`-terminated headings unlikely to prefix a body line.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: Finished clean.

- [ ] **Step 3: bins**

Run: `cargo test --bins 2>&1 | tail -3`
Expected: all pass (no pure test covers this; just ensure nothing else broke).

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
feat(synopsis): render lead labels on paginated pages

The multi-page synopsis render now emits each block's lead label(s) bolded
above its body (was: dropped), tracking label char ranges so
apply_synopsis_label_bold bolds them. Single-page path unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A6: confirm gloss echoes render on paginated pages + e2e

**Files:**
- Inspect: `src/ui/gloss_overlay.rs` — `render_gloss_page` multi-page branch (~1526–1535). It already does `markups[start..end].join("\n")` → `populate_gloss_buffer`; since A3 put the echo INTO the block markup, echoes now render with NO code change here.

**Interfaces:**
- Consumes: A3's echo-bearing markups; existing `populate_gloss_buffer` echo render path.
- Produces: paginated gloss pages render their echoes. (Verification task — no new production code expected; if `rebuild_block_ranges_from` in this arm needs the same care as A5, fix it.)

- [ ] **Step 1: Read render_gloss_page multi-page branch**

Confirm `body = markups[start..end].join("\n")` and that `populate_gloss_buffer(&markup, ...)` is called on it. Since the echo is now inside `markups[i]`, it is in `body`, so `populate_verse_buffer`'s `GlossElement::Gloss` + `split_echo` arm renders it. No change expected.

- [ ] **Step 2: Build + full bins + clippy**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c '^warning'   # expect 122 baseline
```
Expected: build clean; bins green; clippy 122 (no new).

- [ ] **Step 3: Commit (if any fix was needed; else skip)**

Only if `render_gloss_page`/`rebuild_block_ranges_from` needed a tweak:

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
fix(gloss): keep paginated block ranges correct with retained echoes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

- [ ] **Step 4: ASK THE USER to verify both A cases on screen**

Per CLAUDE.md, ask the user to run the e2e (or manual launch) and confirm on a MULTI-PAGE overlay:
- a synopsis whose label paragraph ("…parallels:") falls on a later page shows the bold label on that page (not dropped);
- a gloss with an echo bracket spanning >1 page shows the echo bracket on its page.

Command:

```bash
./scripts/e2e-env.sh cargo test -- --ignored --nocapture
```

and the manual single-work synopsis/gloss launch from CLAUDE.md "Headless Verification" + `grim` so they can eyeball the exact paginated spread. Open every PNG in `target/ui/` and report what you see (UI review protocol).

---

## Self-Review (completed)

**Spec coverage:**
- B color helpers (complement + WCAG contrast) → B1. ✓
- B contrast/distinctness guard (`ensure_gloss_color`, the 13-theme fix) → B2. ✓
- B both colors derived + per-theme override + all-themes invariant → B3. ✓
- B rose-pine-dawn explicit on-cursor value → B4. ✓
- B three states + tags + repaint flip + theme refresh → B5, B6. ✓
- A scope (labels + echoes; pron excluded) → A2 (labels), A3 (echoes); pron untouched by design. ✓
- A attach rule (lead→following, trail→preceding) → A2, A3. ✓
- A pagination measures attachments → A4. ✓
- A multi-page render emits attachments → A5 (synopsis), A6 (gloss verify). ✓

**Placeholder scan:** No "TBD/TODO". All previously-speculative spots are resolved against real source: B3 uses the existing `resolve_theme(name, &Value)` (theme.rs:90) and `load_all_themes()` (theme.rs:42); B5 flips the existing `reader-gloss-line` tag from `theme.focus_color` to `theme.reader_gloss` (app/mod.rs:916) and `settings.rs:285`; A4 uses the real `gloss_block_height`/`block_height_overhead` shape (gloss_overlay.rs:2188); A5's `rebuild_block_ranges_from` is confirmed text-search-based (gloss_overlay.rs:1173) so it needs no edit.

**Type consistency:** `Attachment::{LeadLabel, TrailEcho}` used identically across A1/A2/A3/A4/A5. `reader_gloss` + `reader_gloss_cursor` (Theme fields) / `reader-gloss-line` + `reader-gloss-cursor-line` (tag names) / `reader_gloss_cursor_tag` (state field) / `apply_reader_gloss_cursor_tag_to_line` consistent across B3/B5/B6. `complement_hex`, `contrast_ratio`, `ensure_gloss_color` consistent B1/B2/B3. `resolve_theme` is the real per-value builder (not `from_value`).

**Known soft spot flagged inline (not a gap):** the echo count invariant in `gloss_block_markups` (A3 step 3 warning + the explicit `all_echo_gloss_markups_count_matches_blocks` test) — the only place the 1:1 markup/block count could drift. Covered by a test and the never-fires `else` safety net.
