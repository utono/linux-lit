# Ctrl+t Background-Variant Cycling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ctrl+t cycles every theme between three backgrounds (designed + two
same-family variants), with the karaoke and cursor-line tints following the
background; the chosen variant persists per theme.

**Architecture:** The variant is applied inside theme resolution
(`src/theme.rs`): substitute `text_bg` (and optionally the two alpha tints)
before the existing derivation pipeline runs, so dim/sign/panel/scrim/gloss
colors re-derive automatically. Hand-authored variants come from
`<theme>."linux-lit".bg_variants` in themes-unified.json; the fallback is a
computed blend toward white. A per-theme index persists in
`config.bg_variants`.

**Tech Stack:** Rust, GTK4 (untouched — all changes are pure color math +
existing apply funnel), serde_json.

**Spec:** `docs/superpowers/specs/2026-07-10-bg-variant-cycling-design.md`

## Global Constraints

- Variant 0 must resolve byte-identical to today's theme resolution.
- Computed blends toward `#ffffff`: 65% (variant 1), 90% (variant 2).
- Computed tint alpha scaling: ×0.7 (variant 1), ×0.5 (variant 2), applied
  to `cursor_line_bg` and `phrase_highlight_bg`.
- `root_color`, `focus_color`, `text_fg`, `cursor_bg/fg`, `vocab_fg` are
  derived from the BASE (designed) background, never the variant.
- Keybind changes go in BOTH `src/input/keymap_config.rs` and the stow
  source `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.
- Never run the app against the live session; verify with `cargo test`,
  `cargo clippy`, and the headless cage flow (CLAUDE.md).
- Bypass shell aliases in scripts: `\cp -f`, `command rm -f`.

---

### Task 1: Variant math and resolution in theme.rs

**Files:**
- Modify: `src/theme.rs` (struct at :14, `load_theme_with_fallback` at :107,
  `resolve_theme` at :126, `default_theme` at ~:262, tests module at ~:940)

**Interfaces:**
- Produces: `pub const BG_VARIANT_COUNT: u8 = 3`,
  `pub fn load_theme_with_fallback(name: &str, variant: u8) -> Theme`
  (signature change — 3 call sites are fixed in Task 3),
  `Theme.bg_variant: u8`.
- Consumes: existing private helpers `blend_colors`, `str_field`,
  `hex_to_rgb` (unchanged).

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `src/theme.rs` (it already
has `resolve_theme(name, &json)`-style tests around line 950 — follow that
pattern):

```rust
const SEPIA_JSON: &str = r##"{ "meta": {"display": "S", "type": "light"},
    "dwl": {"rootcolor": "#08526b", "focuscolor": "#8a6a45"},
    "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"},
    "linux-lit": {"cursor_line_bg": "rgba(93, 66, 50, 0.14)",
                  "phrase_highlight_bg": "rgba(93, 66, 50, 0.28)"} }"##;

const AUTHORED_JSON: &str = r##"{ "meta": {"display": "S", "type": "light"},
    "kitty": {"background": "#fdfbf2", "active_tab_foreground": "#5d4232"},
    "linux-lit": {"cursor_line_bg": "rgba(93, 66, 50, 0.10)",
      "bg_variants": [
        {"bg": "#f0f0f0",
         "phrase_highlight_bg": "rgba(69, 89, 100, 0.14)",
         "cursor_line_bg": "rgba(69, 89, 100, 0.12)"}
      ]} }"##;

#[test]
fn variant_zero_is_identical_to_base_resolution() {
    let json: serde_json::Value = serde_json::from_str(SEPIA_JSON).unwrap();
    let base = resolve_theme("s", &json);
    let v0 = resolve_theme_variant("s", &json, 0);
    assert_eq!(base.text_bg, v0.text_bg);
    assert_eq!(base.cursor_line_bg, v0.cursor_line_bg);
    assert_eq!(base.phrase_highlight_bg, v0.phrase_highlight_bg);
    assert_eq!(base.dim_fg, v0.dim_fg);
    assert_eq!(base.root_color, v0.root_color);
    assert_eq!(v0.bg_variant, 0);
}

#[test]
fn computed_variants_blend_toward_white_and_scale_alphas() {
    let json: serde_json::Value = serde_json::from_str(SEPIA_JSON).unwrap();
    let v1 = resolve_theme_variant("s", &json, 1);
    let v2 = resolve_theme_variant("s", &json, 2);
    assert_eq!(v1.text_bg, blend_colors("#ffffff", "#e7dec7", 0.65));
    assert_eq!(v2.text_bg, blend_colors("#ffffff", "#e7dec7", 0.90));
    // alpha 0.14 ×0.7 ≈ 0.10; 0.28 ×0.5 = 0.14
    assert_eq!(v1.cursor_line_bg, "rgba(93, 66, 50, 0.10)");
    assert_eq!(v2.phrase_highlight_bg, "rgba(93, 66, 50, 0.14)");
    // root color pinned to the designed bg's derivation, not the variant's
    assert_eq!(v1.root_color, "#08526b");
    // derivation ran against the NEW bg
    assert_ne!(v1.dim_fg, resolve_theme("s", &json).dim_fg);
    assert_eq!(v2.bg_variant, 2);
}

#[test]
fn authored_variant_overrides_bg_and_tints() {
    let json: serde_json::Value = serde_json::from_str(AUTHORED_JSON).unwrap();
    let v1 = resolve_theme_variant("s", &json, 1);
    assert_eq!(v1.text_bg, "#f0f0f0");
    assert_eq!(v1.phrase_highlight_bg, "rgba(69, 89, 100, 0.14)");
    assert_eq!(v1.cursor_line_bg, "rgba(69, 89, 100, 0.12)");
    // only 1 authored entry → variant 2 falls back to computed
    let v2 = resolve_theme_variant("s", &json, 2);
    assert_eq!(v2.text_bg, blend_colors("#ffffff", "#fdfbf2", 0.90));
}

#[test]
fn authored_entry_without_tints_scales_the_theme_tints() {
    let json: serde_json::Value = serde_json::from_str(
        r##"{ "meta": {"type": "light"},
              "kitty": {"background": "#e7dec7"},
              "linux-lit": {"cursor_line_bg": "rgba(93, 66, 50, 0.20)",
                            "bg_variants": [{"bg": "#f0f0f0"}]} }"##).unwrap();
    let v1 = resolve_theme_variant("s", &json, 1);
    assert_eq!(v1.text_bg, "#f0f0f0");
    assert_eq!(v1.cursor_line_bg, "rgba(93, 66, 50, 0.14)"); // 0.20 × 0.7
}

#[test]
fn malformed_authored_entry_falls_back_to_computed() {
    let json: serde_json::Value = serde_json::from_str(
        r##"{ "meta": {"type": "light"},
              "kitty": {"background": "#e7dec7"},
              "linux-lit": {"bg_variants": [{"note": "no bg key"}]} }"##).unwrap();
    let v1 = resolve_theme_variant("s", &json, 1);
    assert_eq!(v1.text_bg, blend_colors("#ffffff", "#e7dec7", 0.65));
}

#[test]
fn scale_rgba_alpha_scales_only_the_alpha() {
    assert_eq!(scale_rgba_alpha("rgba(93, 66, 50, 0.28)", 0.5),
               "rgba(93, 66, 50, 0.14)");
    assert_eq!(scale_rgba_alpha("rgba(1, 2, 3, 0.10)", 1.0),
               "rgba(1, 2, 3, 0.10)");           // factor 1.0 = unchanged
    assert_eq!(scale_rgba_alpha("#not-rgba", 0.5), "#not-rgba"); // passthrough
}

#[test]
fn variant_index_wraps_modulo_count() {
    let json: serde_json::Value = serde_json::from_str(SEPIA_JSON).unwrap();
    let v3 = resolve_theme_variant("s", &json, 3);
    assert_eq!(v3.text_bg, resolve_theme("s", &json).text_bg); // 3 % 3 = 0
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib theme:: 2>&1 | tail -20
```

Expected: compile error — `resolve_theme_variant`, `scale_rgba_alpha`,
`bg_variant` not defined. (If `--lib` doesn't match this crate layout, use
`cargo test theme::` — the binary crate compiles tests via `cargo test`.)

- [ ] **Step 3: Implement**

In `src/theme.rs`:

3a. Add to the `Theme` struct (after `scrim_bg`):

```rust
    pub bg_variant: u8,           // active background variant (0 = designed)
```

Add `bg_variant: 0,` to the struct literal in `default_theme()` and any
other `Theme { ... }` literal the compiler flags.

3b. Add constants and helpers near the top (after `READER_GLOSS_MIN_CONTRAST`):

```rust
/// Number of background variants every theme has (index 0 = designed bg).
/// Cycled by Ctrl+t; see docs/superpowers/specs/2026-07-10-bg-variant-cycling-design.md.
pub const BG_VARIANT_COUNT: u8 = 3;

/// Blend fraction toward #ffffff for computed variants 1 and 2.
const BG_VARIANT_BLEND: [f64; 2] = [0.65, 0.90];

/// Alpha scale for cursor-line / karaoke tints at computed variants 1 and 2.
const BG_VARIANT_ALPHA: [f64; 2] = [0.7, 0.5];

/// Hand-authored variant entry from `<theme>."linux-lit".bg_variants`.
struct AuthoredVariant {
    bg: String,
    cursor_line_bg: Option<String>,
    phrase_highlight_bg: Option<String>,
}

/// Authored entry for `variant` (1 or 2), if present and well-formed
/// (`bg` is required; a malformed entry falls back to computed).
fn authored_variant(val: &Value, variant: u8) -> Option<AuthoredVariant> {
    let arr = val.get("linux-lit")?.get("bg_variants")?.as_array()?;
    let entry = arr.get((variant - 1) as usize)?;
    let bg = str_field(entry, "bg")?;
    Some(AuthoredVariant {
        bg,
        cursor_line_bg: str_field(entry, "cursor_line_bg"),
        phrase_highlight_bg: str_field(entry, "phrase_highlight_bg"),
    })
}

/// Scale the alpha of an `rgba(r, g, b, a)` string; non-rgba strings and
/// factor 1.0 pass through unchanged.
fn scale_rgba_alpha(s: &str, factor: f64) -> String {
    if (factor - 1.0).abs() < f64::EPSILON {
        return s.to_string();
    }
    let inner = s
        .trim()
        .strip_prefix("rgba(")
        .and_then(|r| r.strip_suffix(')'));
    let Some(inner) = inner else { return s.to_string() };
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return s.to_string();
    }
    let Ok(a) = parts[3].parse::<f64>() else { return s.to_string() };
    format!("rgba({}, {}, {}, {:.2})", parts[0], parts[1], parts[2], a * factor)
}
```

3c. Rename `resolve_theme` to `resolve_theme_variant(name: &str, val:
&Value, variant: u8) -> Theme` and re-add the old name as a thin wrapper
(existing tests and `load_all_themes` keep calling it):

```rust
fn resolve_theme(name: &str, val: &Value) -> Theme {
    resolve_theme_variant(name, val, 0)
}
```

3d. Inside `resolve_theme_variant`, immediately after `root_color` and
`focus_color` are computed (they must read the DESIGNED bg — today's
`text_bg` binding at :145 — so leave everything above unchanged), insert the
substitution. Rename today's `let text_bg = ...` binding to `base_bg`, fix
`root_color`'s fallback to use `&base_bg`, then:

```rust
    let variant = variant % BG_VARIANT_COUNT;
    let authored = if variant == 0 { None } else { authored_variant(val, variant) };
    let alpha_factor = if variant == 0 { 1.0 } else { BG_VARIANT_ALPHA[(variant - 1) as usize] };
    let text_bg = match (&authored, variant) {
        (Some(a), _) => a.bg.clone(),
        (_, 0) => base_bg.clone(),
        (_, v) => blend_colors("#ffffff", &base_bg, BG_VARIANT_BLEND[(v - 1) as usize]),
    };
```

3e. Change the two tint derivations (currently :171 and :176) to honor
authored overrides and the alpha factor:

```rust
    let lit = val.get("linux-lit").unwrap_or(&Value::Null);
    let cursor_line_bg = authored
        .as_ref()
        .and_then(|a| a.cursor_line_bg.clone())
        .unwrap_or_else(|| {
            let base = str_field(lit, "cursor_line_bg")
                .unwrap_or_else(|| "rgba(86, 148, 100, 0.25)".to_string());
            scale_rgba_alpha(&base, alpha_factor)
        });

    let phrase_highlight_bg = authored
        .as_ref()
        .and_then(|a| a.phrase_highlight_bg.clone())
        .unwrap_or_else(|| match str_field(lit, "phrase_highlight_bg") {
            Some(explicit) => scale_rgba_alpha(&explicit, alpha_factor),
            None => {
                let (r, g, b) = rgba_str_to_rgb(&cursor_line_bg);
                format!(
                    "rgba({}, {}, {}, {:.2})",
                    (r * 255.0) as u8,
                    (g * 255.0) as u8,
                    (b * 255.0) as u8,
                    0.28 * alpha_factor
                )
            }
        });
```

Note: the derived-phrase branch previously hardcoded `0.28` with no
format precision; `{:.2}` keeps variant 0 emitting `0.28` exactly, so the
`variant_zero_is_identical` test pins this.

3f. Set `bg_variant: variant,` in the `Theme { ... }` literal at the bottom
of `resolve_theme_variant`.

3g. Change `load_theme_with_fallback` (:107) to take the variant and thread
it through both branches:

```rust
pub fn load_theme_with_fallback(name: &str, variant: u8) -> Theme {
    let path = themes_path();
    let data: Value = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
    {
        Some(d) => d,
        None => return default_theme(),
    };
    if let Some(val) = data.get(name) {
        return resolve_theme_variant(name, val, variant);
    }
    let fallback = crate::config::DEFAULT_THEME;
    match data.get(fallback) {
        Some(val) => resolve_theme_variant(fallback, val, variant),
        None => default_theme(),
    }
}
```

The 3 external call sites now fail to compile — that is expected and fixed
in Task 3. To keep THIS task's tests runnable first, temporarily pass `0` at
those call sites in this task (mechanical, no behavior change):
`src/app/mod.rs:991`, `src/main.rs:629`, `src/input/actions/settings.rs:509`
each get `, 0` appended to the call. Task 3 replaces the `0`s with the real
config lookup.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test theme:: 2>&1 | tail -10
```

Expected: all new tests PASS, all pre-existing theme tests (variant-0
identity guarantees them) PASS.

- [ ] **Step 5: Full build + clippy, then commit**

```bash
cargo build 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3
git add src/theme.rs src/app/mod.rs src/main.rs src/input/actions/settings.rs
git commit -m "feat(theme): background-variant resolution (authored + computed toward-white)"
```

---

### Task 2: Per-theme variant persistence in config.rs

**Files:**
- Modify: `src/config.rs` (Config struct :121-212, tests at file bottom)

**Interfaces:**
- Consumes: `crate::theme::BG_VARIANT_COUNT` (Task 1).
- Produces: `Config.bg_variants: HashMap<String, u8>`,
  `pub fn bg_variant_for(&self, theme_name: &str) -> u8`.

- [ ] **Step 1: Write the failing tests**

Add to an existing `#[cfg(test)]` module in `src/config.rs`:

```rust
#[test]
fn bg_variant_for_defaults_to_zero_and_wraps() {
    let mut c: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(c.bg_variant_for("kindle-sepia"), 0);
    c.bg_variants.insert("kindle-sepia".into(), 2);
    assert_eq!(c.bg_variant_for("kindle-sepia"), 2);
    c.bg_variants.insert("kindle-sepia".into(), 7); // malformed config
    assert_eq!(c.bg_variant_for("kindle-sepia"), 1); // 7 % 3
}

#[test]
fn bg_variants_roundtrip_serde() {
    let mut c: Config = serde_json::from_str("{}").unwrap();
    c.bg_variants.insert("sepia-lightest".into(), 1);
    let json = serde_json::to_string(&c).unwrap();
    let back: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bg_variant_for("sepia-lightest"), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test config:: 2>&1 | tail -5
```

Expected: compile error — no field `bg_variants`.

- [ ] **Step 3: Implement**

Add to the `Config` struct after `theme_cycle` (:211):

```rust
    /// Per-theme background-variant index (0-2) chosen with Ctrl+t. Keyed
    /// by theme name; absent = 0 (the designed background). Merged
    /// ours-wins on save, like `theme` itself (see merge_configs).
    #[serde(default)]
    pub bg_variants: HashMap<String, u8>,
```

Add the accessor near `theme_name()` (:365):

```rust
    /// Saved background-variant index for `theme_name`, wrapped into range.
    pub fn bg_variant_for(&self, theme_name: &str) -> u8 {
        self.bg_variants.get(theme_name).copied().unwrap_or(0)
            % crate::theme::BG_VARIANT_COUNT
    }
```

No `merge_configs` change: `merged = ours.clone()` already gives the field
last-writer-wins semantics, matching `theme` (theme-keyed prefs are MRU,
not per-work state — the documented slot-drift tradeoff applies).

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test config:: 2>&1 | tail -5
```

Expected: PASS (including pre-existing merge tests).

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): per-theme bg_variants persistence"
```

---

### Task 3: Action, Ctrl+t keybind, handler, and call-site wiring

**Files:**
- Modify: `src/input/actions/mod.rs` (enum ~:165, Category ~:275, name() ~:412)
- Modify: `src/input/keymap_config.rs` (`display_bindings()` ~:331, tests ~:491)
- Modify: `src/input/keymap.rs` (dispatch ~:3100)
- Modify: `src/input/actions/settings.rs` (`SettingsChange::Theme` :46,
  `cycle_theme` :509, new handler)
- Modify: `src/app/mod.rs:991`, `src/main.rs:625-630` (replace Task 1's `0`s)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (~:106)

**Interfaces:**
- Consumes: `load_theme_with_fallback(name, variant)` (Task 1),
  `Config::bg_variant_for` / `Config.bg_variants` (Task 2),
  existing `apply_theme_to_state` (saves config at :306).
- Produces: `Action::BgVariantNext` (serde name `"BgVariantNext"` — Action
  derives Deserialize, so `parse_action` picks it up with no extra code),
  `pub(crate) fn cycle_bg_variant(state: &Rc<RefCell<AppState>>)`.

- [ ] **Step 1: Write the failing keymap test**

In `src/input/keymap_config.rs` tests (next to the ThemeNext assertion :491;
lookup argument order is `(key, ctrl, shift, alt)`):

```rust
assert_eq!(km.lookup("t", true, false, false), Some(Action::BgVariantNext));
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test keymap_config 2>&1 | tail -5
```

Expected: compile error — `BgVariantNext` not found.

- [ ] **Step 3: Implement**

3a. `src/input/actions/mod.rs` — three edits, each next to `ThemeNext`:

```rust
    ThemeNext,
    ThemePrev,
    BgVariantNext,
```

```rust
            | Action::ThemeNext
            | Action::ThemePrev
            | Action::BgVariantNext
```

```rust
            Action::ThemeNext => "ThemeNext",
            Action::BgVariantNext => "BgVariantNext",
```

(If `name()` is an exhaustive match the compiler will point at any arm list
missed — follow it.)

3b. `src/input/keymap_config.rs` `display_bindings()` after the ThemePrev
line:

```rust
        (KeyCombo::ctrl("t"), Action::BgVariantNext),
```

3c. `src/input/keymap.rs` dispatch, next to the ThemeNext arm (:3100):

```rust
        BgVariantNext => crate::input::actions::settings::cycle_bg_variant(state),
```

3d. `src/input/actions/settings.rs` — new handler after `cycle_theme`:

```rust
/// Ctrl+t: cycle the current theme's background variant (0 → 1 → 2 → 0).
/// The index persists per theme (config.bg_variants); the theme is
/// re-resolved so every bg-derived color (karaoke, panels, guards)
/// follows the new background. See
/// docs/superpowers/specs/2026-07-10-bg-variant-cycling-design.md.
pub(crate) fn cycle_bg_variant(state: &Rc<RefCell<crate::app::AppState>>) {
    let mut s = state.borrow_mut();
    let name = s.theme.name.clone();
    let next = (s.theme.bg_variant + 1) % crate::theme::BG_VARIANT_COUNT;
    // Insert BEFORE apply_theme_to_state — it saves the config snapshot.
    s.config.bg_variants.insert(name.clone(), next);
    let theme = crate::theme::load_theme_with_fallback(&name, next);
    apply_theme_to_state(&mut s, &theme);
    let _ = std::process::Command::new("notify-send")
        .args(["-t", "1500", "-h",
               "string:x-canonical-private-synchronous:linux-lit-theme",
               &format!("Background [{}/{}]", next + 1, crate::theme::BG_VARIANT_COUNT),
               &s.theme.text_bg])
        .spawn();
}
```

3e. `cycle_theme` (:509) — the target theme restores ITS saved variant:

```rust
    let variant = s.config.bg_variant_for(&cycle[next]);
    let theme = crate::theme::load_theme_with_fallback(&cycle[next], variant);
```

3f. `SettingsChange::Theme` (:46) — the settings overlay hands over a
variant-0 `Theme` (it resolves via `load_all_themes`); re-load with the
saved variant when one is set:

```rust
        SettingsChange::Theme(theme) => {
            let v = s.config.bg_variant_for(&theme.name);
            let theme = if v == 0 {
                theme
            } else {
                crate::theme::load_theme_with_fallback(&theme.name, v)
            };
            apply_theme_to_state(&mut s, &theme);
        }
```

(Adjust the binding names to the actual surrounding code — the arm
currently reads `apply_theme_to_state(&mut s, &theme);`.)

3g. Replace Task 1's placeholder `0`s:

- `src/app/mod.rs:991`:

```rust
    let theme = crate::theme::load_theme_with_fallback(
        config.theme_name(),
        config.bg_variant_for(config.theme_name()),
    );
```

- `src/main.rs` SIGUSR1 arm (~:625): adopt `bg_variants` from disk alongside
  `theme_cycle` (external control: edit config.json, then `kill -USR1`):

```rust
                        let disk = crate::config::load();
                        s.config.theme_cycle = disk.theme_cycle.clone();
                        s.config.bg_variants = disk.bg_variants.clone();
                        let name = disk.theme_name().to_string();
                        let variant = s.config.bg_variant_for(&name);
                        let theme = crate::theme::load_theme_with_fallback(&name, variant);
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &theme);
```

3h. Stow source `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` —
add next to the Alt+t line (:106):

```json
    {"key": "t", "ctrl": true, "action": "BgVariantNext"},
```

(The deployed `~/.config/linux-lit/keymap.json` is a stow symlink to this
file — no restow needed. Verify with `ls -la ~/.config/linux-lit/keymap.json`;
if it is NOT a symlink, apply the same edit there too.)

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test 2>&1 | tail -5 && cargo clippy 2>&1 | tail -3
```

Expected: all PASS, no new clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs \
        src/input/actions/settings.rs src/app/mod.rs src/main.rs
git commit -m "feat: Ctrl+t cycles per-theme background variants"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json \
  && git commit -m "linux-lit: bind Ctrl+t BgVariantNext" && cd ~/utono/linux-lit
```

---

### Task 4: Ctrl+/ keybinds-overlay mirror

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (`key("t", ...)` at :76, `describe()`
  at :217+)

**Interfaces:**
- Consumes: nothing new — the overlay is a hand-maintained mirror of label
  strings.

- [ ] **Step 1: Add the detail entry and describe arm**

At :76, add `("C-t", "bg variant")` to the front of the detail slice:

```rust
    key("t", "T", "", "", &[("C-t", "bg variant"), ("S-C-t", "nav test"),
        ("M-t", "theme next"), ("M-S-T", "theme prev")]),
```

In `describe()`, next to the `"theme next"` arm (:511), following the same
string style:

```rust
        "bg variant" => "Cycle the current theme's background between its \
            three variants (Ctrl+t): the designed color plus two lighter \
            same-family alternates — hand-authored per theme in \
            themes-unified.json bg_variants, else computed toward white. \
            The cursor-line and karaoke tints follow the background; the \
            chosen variant persists per theme. \
            -> cycle_bg_variant — src/input/actions/settings.rs",
```

- [ ] **Step 2: Run the update-cairo-keybinds-overlay cross-reference**

Invoke the `update-cairo-keybinds-overlay` skill's three-pass check (no
blank slot hides a real binding; no label names the wrong action; every
label has a describe() arm) for the `t` key and the new label.

- [ ] **Step 3: Build and commit**

```bash
cargo build 2>&1 | tail -3
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): Ctrl+t bg-variant in keybinds overlay"
```

---

### Task 5: Seed sepia-lightest's authored cool variant

**Files:**
- Modify: `~/utono/themes/.config/themes/themes-unified.json` (outside this
  repo; no linux-lit commit)

- [ ] **Step 1: Add the authored variant with jq**

```bash
SCRATCH=$(mktemp -d)
UNIFIED=~/utono/themes/.config/themes/themes-unified.json
jq '."sepia-lightest"."linux-lit".bg_variants =
      [{"bg": "#f0f0f0",
        "phrase_highlight_bg": "rgba(69, 89, 100, 0.14)",
        "cursor_line_bg": "rgba(69, 89, 100, 0.12)"}]' \
  "$UNIFIED" > "$SCRATCH/u.json" \
&& jq -e '."sepia-lightest"."linux-lit".bg_variants[0].bg == "#f0f0f0"' \
     "$SCRATCH/u.json" >/dev/null \
&& \cp -f "$SCRATCH/u.json" "$UNIFIED" && echo OK
```

Expected: `OK`. Variant 2 stays computed (one authored entry only).

- [ ] **Step 2: Commit in the themes repo** (it also has the two new sepia
  themes uncommitted from earlier this session — include them):

```bash
cd ~/utono/themes && git add .config/themes/themes-unified.json \
  && git commit -m "sepia-light/lightest themes + sepia-lightest cool bg variant" \
  && cd ~/utono/linux-lit
```

---

### Task 6: End-to-end verification (headless)

**Files:** none (verification only)

- [ ] **Step 1: Full test + clippy sweep**

```bash
cargo test 2>&1 | tail -5 && cargo clippy 2>&1 | tail -3
```

Expected: PASS / no new warnings.

- [ ] **Step 2: Headless visual acceptance**

Per CLAUDE.md *Headless Verification* (cage on its own socket; never touch
the live session; `GSK_RENDERER=cairo` mandatory):

```bash
cd ~/utono/linux-lit && cargo build
LIT_DEV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  dbus-run-session cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 6
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
grim /tmp/bgv0.png && stat -c%s /tmp/bgv0.png   # expect tens of KB
wtype -M ctrl -k t -m ctrl && sleep 2 && grim /tmp/bgv1.png
wtype -M ctrl -k t -m ctrl && sleep 2 && grim /tmp/bgv2.png
wtype -M ctrl -k t -m ctrl && sleep 2 && grim /tmp/bgv3.png
pkill -f "cage -- ./target/debug/linux-lit"
```

Read all four PNGs. Acceptance (active theme is sepia-lightest):
- bgv0: warm cream `#fdfbf2` page.
- bgv1: cool grey `#f0f0f0` page (the authored variant — visibly cooler).
- bgv2: near-white computed page.
- bgv3: back to warm cream (cycle wrapped).
Note: a headless run may write `config-dev` variant state; that is the
saved-per-theme behavior working (and LIT_HEADLESS_TEST is NOT set here so
the persistence path is exercised — if this run must not touch the dev
config, snapshot `~/.config/linux-lit/config-dev.json` first and restore
after).

- [ ] **Step 3: Verify persistence + karaoke tint from config/theme data**

```bash
jq '.bg_variants' ~/.config/linux-lit/config-dev.json
```

Expected: `{"sepia-lightest": 0}` (after the 3-press wrap) — or the index
where the run ended.

- [ ] **Step 4: Report**

Report screenshots inline (UI review protocol), state test results plainly,
and hand the user the manual eyeball command for the real GL renderer:

```bash
crll   # then press Ctrl+t in the live reader
```

---

## Self-review notes

- Spec coverage: variant model (Task 1), persistence (Task 2), keybind +
  Alt+t/startup/SIGUSR1/settings-overlay integration (Task 3), overlay
  mirror (Task 4), seed data (Task 5), tests + visual acceptance (Tasks
  1/2/6). Out-of-scope items from the spec are untouched.
- Variant-0 byte-identity is pinned by `variant_zero_is_identical...` plus
  the untouched pre-existing theme tests running through the wrapper.
- Type consistency: `load_theme_with_fallback(name: &str, variant: u8)`
  used identically in Tasks 1 and 3; `bg_variant_for` returns `u8`
  everywhere; `BG_VARIANT_COUNT` is the single cycle modulus.
