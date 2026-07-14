# Independent Theme + Theme-Cycle Keybinds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** linux-lit holds its own theme (default `kindle-sepia`), fully independent of the system-wide theme, with `Alt+t`/`Alt+Shift+T` reader keybinds cycling a curated reading-theme list.

**Architecture:** Persist the theme in the existing `Config` (`~/.config/linux-lit/config.json`, atomic save). Startup and the SIGUSR1 handler resolve the theme from config instead of the shared `~/utono/themes/.config/themes/.current_theme`. All retheming keeps flowing through the single chokepoint `apply_theme_to_state`, which stops writing the shared state file and persists config instead. Two new `Action` variants cycle a config-defined theme list. `set-theme.sh` (themes repo) stops signaling linux-lit.

**Tech Stack:** Rust, GTK4/sourceview5, serde. Build with `cargo build`; test with `cargo test`.

**Design doc:** `docs/superpowers/specs/2026-07-08-independent-theme-keybinds-design.md`

## Global Constraints

- Default theme name is exactly `kindle-sepia`; compiled default cycle list is exactly `["kindle-sepia", "kindle-green", "zenbones-light", "zenwritten-light"]`.
- New keybinds: `Alt+t` → next theme, `Alt+Shift+T` → previous theme. Any keybind change must land in BOTH `src/input/keymap_config.rs` defaults AND the stowed `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (the JSON overrides compiled defaults).
- linux-lit must not read OR write `~/utono/themes/.config/themes/.current_theme` after this plan.
- Do not touch the three uncommitted gutter-work files if still present (`git stash` nothing; just leave them out of commits — stage explicit paths only).
- Repo convention: never use `keybinds-search`/keybinds.db for this repo; source is authoritative.

---

### Task 1: Config fields and accessors

**Files:**
- Modify: `src/config.rs` (struct `Config` ~line 38, and the `#[cfg(test)]` module at the bottom)

**Interfaces:**
- Produces: `crate::config::DEFAULT_THEME: &str`, `Config::theme_name(&self) -> &str`, `Config::theme_cycle(&self) -> Vec<String>`, fields `Config.theme: Option<String>`, `Config.theme_cycle: Option<Vec<String>>`. Later tasks call these exact names.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)]` module at the bottom of `src/config.rs`:

```rust
#[test]
fn theme_name_defaults_to_kindle_sepia() {
    let config = Config::default();
    assert_eq!(config.theme_name(), "kindle-sepia");
}

#[test]
fn theme_name_uses_configured_value() {
    let mut config = Config::default();
    config.theme = Some("zenbones-light".to_string());
    assert_eq!(config.theme_name(), "zenbones-light");
}

#[test]
fn theme_cycle_defaults_to_reading_themes() {
    let config = Config::default();
    assert_eq!(
        config.theme_cycle(),
        vec!["kindle-sepia", "kindle-green", "zenbones-light", "zenwritten-light"]
    );
}

#[test]
fn theme_cycle_uses_configured_list() {
    let mut config = Config::default();
    config.theme_cycle = Some(vec!["melange-light".to_string()]);
    assert_eq!(config.theme_cycle(), vec!["melange-light"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config:: 2>&1 | tail -20`
Expected: compile error — `theme` field and `theme_name` method do not exist.

- [ ] **Step 3: Implement**

In `src/config.rs`, add near the top (module level, next to `FONT_CYCLE`):

```rust
/// linux-lit's theme is independent of the system-wide theme system.
/// This is the effective theme when config.json has no `theme` field.
pub const DEFAULT_THEME: &str = "kindle-sepia";

fn default_theme_cycle() -> Vec<String> {
    ["kindle-sepia", "kindle-green", "zenbones-light", "zenwritten-light"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
```

Add two fields to `struct Config` (after `mpv_volume`):

```rust
    /// Per-app theme (independent of the system-wide theme). None means
    /// DEFAULT_THEME. Set by the settings overlay and Alt+t / Alt+Shift+T.
    #[serde(default)]
    pub theme: Option<String>,
    /// Ordered list the theme-cycle keybinds walk through. None means the
    /// compiled default reading list. Edit config.json to customize.
    #[serde(default)]
    pub theme_cycle: Option<Vec<String>>,
```

Add accessors in the existing `impl Config` block:

```rust
    /// Effective theme name: configured, else DEFAULT_THEME.
    pub fn theme_name(&self) -> &str {
        self.theme.as_deref().unwrap_or(DEFAULT_THEME)
    }

    /// Effective theme-cycle list: configured, else the compiled default.
    pub fn theme_cycle(&self) -> Vec<String> {
        match &self.theme_cycle {
            Some(list) if !list.is_empty() => list.clone(),
            _ => default_theme_cycle(),
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config:: 2>&1 | tail -5`
Expected: the 4 new tests pass, no failures.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "config: per-app theme + theme_cycle fields (default kindle-sepia)"
```

---

### Task 2: Resolve theme from config; stop touching .current_theme

**Files:**
- Modify: `src/theme.rs` (`current_theme_path`/`current_theme_name` ~lines 40-51; `load_theme` ~line 100)
- Modify: `src/app/mod.rs` (~lines 961-967, the "Load theme" block)
- Modify: `src/main.rs` (~lines 578-587, `MpvEvent::ThemeChanged` arm)
- Modify: `src/input/actions/settings.rs` (`apply_theme_to_state`, the "Write .current_theme file" block ~lines 297-302)

**Interfaces:**
- Consumes: `Config::theme_name()` from Task 1.
- Produces: `crate::theme::load_theme_with_fallback(name: &str) -> Theme` (used by Task 3). `current_theme_name()`/`current_theme_path()` are DELETED — nothing may call them after this task.

- [ ] **Step 1: Add `load_theme_with_fallback` to `src/theme.rs`**

Place directly under the existing `load_theme`:

```rust
/// Load `name` from themes-unified.json; if absent fall back to the app
/// default theme name, then to the hardcoded default_theme(). linux-lit's
/// theme is independent of the system-wide .current_theme (see
/// docs/superpowers/specs/2026-07-08-independent-theme-keybinds-design.md).
pub fn load_theme_with_fallback(name: &str) -> Theme {
    let path = themes_path();
    let data: Value = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
    {
        Some(d) => d,
        None => return default_theme(),
    };
    if let Some(val) = data.get(name) {
        return resolve_theme(name, val);
    }
    let fallback = crate::config::DEFAULT_THEME;
    match data.get(fallback) {
        Some(val) => resolve_theme(fallback, val),
        None => default_theme(),
    }
}
```

- [ ] **Step 2: Delete `current_theme_path()` and `current_theme_name()` from `src/theme.rs`**

Remove both functions (~lines 40-51) and their doc comments.

- [ ] **Step 3: Switch startup to config**

In `src/app/mod.rs`, the build function already has the loaded `Config` in scope (it stores it in `AppState`; find the local binding — `rg -n 'config::load' src/app/mod.rs`). Replace:

```rust
    // Load theme
    let theme_name = crate::theme::current_theme_name();
    let theme = if theme_name.is_empty() {
        crate::theme::load_theme("gruvbox-material")
    } else {
        crate::theme::load_theme(&theme_name)
    };
```

with:

```rust
    // Load theme from the app's own config (independent of the system-wide
    // theme; default kindle-sepia).
    let theme = crate::theme::load_theme_with_fallback(config.theme_name());
```

If the local variable holding the config at that point has a different name (e.g. `cfg`), use that name. If the config is loaded AFTER this block, move the `crate::config::load()` call above it.

- [ ] **Step 4: Switch the SIGUSR1 handler to config**

In `src/main.rs`, replace the `MpvEvent::ThemeChanged` arm:

```rust
                    MpvEvent::ThemeChanged => {
                        let mut s = state_for_events.borrow_mut();
                        let theme_name = crate::theme::current_theme_name();
                        let theme = if theme_name.is_empty() {
                            crate::theme::load_theme("gruvbox-material")
                        } else {
                            crate::theme::load_theme(&theme_name)
                        };
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &theme);
                    }
```

with:

```rust
                    MpvEvent::ThemeChanged => {
                        // SIGUSR1 = "re-read MY config and re-apply". External
                        // control: edit config.json's theme, then kill -USR1.
                        let mut s = state_for_events.borrow_mut();
                        let name = s.config.theme_name().to_string();
                        let theme = crate::theme::load_theme_with_fallback(&name);
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &theme);
                    }
```

- [ ] **Step 5: Persist config instead of writing .current_theme**

In `src/input/actions/settings.rs`, inside `apply_theme_to_state`, replace:

```rust
    // Write .current_theme file
    let home = std::env::var("HOME").unwrap_or_default();
    let theme_path = std::path::PathBuf::from(&home)
        .join("utono/themes/.config/themes/.current_theme");
    let _ = std::fs::write(&theme_path, &theme.name);
```

with:

```rust
    // Persist the per-app theme. linux-lit no longer reads or writes the
    // system-wide .current_theme — its theme is independent (default
    // kindle-sepia). config::save is atomic and a no-op under
    // LIT_HEADLESS_TEST.
    state.config.theme = Some(theme.name.clone());
    crate::config::save(&state.config);
```

Also update the function's doc comment (`/// Apply a theme to AppState: load CSS, update tag colors, write .current_theme.`) to say `persist config.theme` instead of `write .current_theme`.

- [ ] **Step 6: Verify no caller remains and build**

Run: `rg -n 'current_theme_name|current_theme_path|\.current_theme' src/`
Expected: no matches (comments mentioning the old behavior are fine only if updated to past tense; code matches are a failure).

Run: `cargo build 2>&1 | tail -5`
Expected: compiles with no errors.

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/theme.rs src/app/mod.rs src/main.rs src/input/actions/settings.rs
git commit -m "theme: resolve from app config, not shared .current_theme

Startup and SIGUSR1 read config.theme (default kindle-sepia).
apply_theme_to_state persists config instead of clobbering the
system-wide .current_theme."
```

---

### Task 3: ThemeNext/ThemePrev actions, cycle logic, keybinds

**Files:**
- Modify: `src/input/actions/mod.rs` (enum `Action` ~line 148 block; `category()` Display arm ~line 294; `name()` ~line 350)
- Modify: `src/input/actions/settings.rs` (new `cycle_theme` + `next_cycle_index` and tests)
- Modify: `src/input/keymap.rs` (dispatch match, near the `ToggleDim =>` arm ~line 3012)
- Modify: `src/input/keymap_config.rs` (`KeyCombo` impl ~line 40; `display_bindings()` ~line 318; tests ~line 455)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (stowed override — REQUIRED, it shadows compiled defaults)

**Interfaces:**
- Consumes: `Config::theme_cycle()` (Task 1), `load_theme_with_fallback` (Task 2), `apply_theme_to_state` (existing).
- Produces: `Action::ThemeNext`, `Action::ThemePrev`; `settings::cycle_theme(state: &Rc<RefCell<AppState>>, forward: bool)`; `settings::next_cycle_index(len: usize, current: Option<usize>, forward: bool) -> usize`.

- [ ] **Step 1: Write the failing tests**

In `src/input/actions/settings.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::next_cycle_index;

    #[test]
    fn cycle_forward_wraps() {
        assert_eq!(next_cycle_index(4, Some(0), true), 1);
        assert_eq!(next_cycle_index(4, Some(3), true), 0);
    }

    #[test]
    fn cycle_backward_wraps() {
        assert_eq!(next_cycle_index(4, Some(0), false), 3);
        assert_eq!(next_cycle_index(4, Some(2), false), 1);
    }

    #[test]
    fn current_not_in_list_jumps_to_first() {
        assert_eq!(next_cycle_index(4, None, true), 0);
        assert_eq!(next_cycle_index(4, None, false), 0);
    }
}
```

In `src/input/keymap_config.rs` tests (next to `alt_bracketleft_is_toggle_column_layout`):

```rust
    #[test]
    fn alt_t_cycles_theme() {
        let km = Keymap::default();
        assert_eq!(km.lookup("t", false, false, true), Some(Action::ThemeNext));
        assert_eq!(km.lookup("T", false, true, true), Some(Action::ThemePrev));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib 2>&1 | tail -10`
Expected: compile error — `ThemeNext` and `next_cycle_index` do not exist.

- [ ] **Step 3: Add the Action variants and metadata**

In `src/input/actions/mod.rs`:

1. In the `enum Action` "Settings (in reader)" block (after `ToggleDim`):

```rust
    ThemeNext,
    ThemePrev,
```

2. In `category()`, add to the Display arm's `|` chain (before `| Action::OpenSettingsOverlay`):

```rust
            | Action::ThemeNext
            | Action::ThemePrev
```

3. In `name()`, following the existing verbatim-variant-name convention:

```rust
            Action::ThemeNext => "ThemeNext",
            Action::ThemePrev => "ThemePrev",
```

Do NOT add them to `flashes_prose_cursor()` (that list is navigation-only).

- [ ] **Step 4: Implement cycle logic**

In `src/input/actions/settings.rs`:

```rust
/// Index of the next theme in a cycle list of length `len`.
/// `current` = position of the active theme in the list, if it is in the
/// list at all; when it is not (e.g. set via the settings overlay), both
/// directions jump to the first entry.
pub(crate) fn next_cycle_index(len: usize, current: Option<usize>, forward: bool) -> usize {
    match current {
        Some(i) if forward => (i + 1) % len,
        Some(i) => (i + len - 1) % len,
        None => 0,
    }
}

/// Alt+t / Alt+Shift+T: cycle through the curated theme list in config.
pub(crate) fn cycle_theme(state: &Rc<RefCell<crate::app::AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    let cycle = s.config.theme_cycle();
    if cycle.is_empty() {
        return;
    }
    let current = cycle.iter().position(|t| *t == s.theme.name);
    let next = next_cycle_index(cycle.len(), current, forward);
    let theme = crate::theme::load_theme_with_fallback(&cycle[next]);
    apply_theme_to_state(&mut s, &theme);
}
```

(`use std::rc::Rc; use std::cell::RefCell;` are already imported in this file — verify with `rg -n 'use std::rc::Rc' src/input/actions/settings.rs` and add if missing.)

- [ ] **Step 5: Dispatch the actions**

In `src/input/keymap.rs`, in the big `match` (near the `ToggleDim =>` arm), add:

```rust
        ThemeNext => crate::input::actions::settings::cycle_theme(state, true),
        ThemePrev => crate::input::actions::settings::cycle_theme(state, false),
```

- [ ] **Step 6: Bind the keys (BOTH places)**

In `src/input/keymap_config.rs`, add to the `impl KeyCombo` block:

```rust
    pub fn alt_shift(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: true, alt: true }
    }
```

Add to `display_bindings()` (after the `ToggleDim` line):

```rust
        (KeyCombo::alt("t"), Action::ThemeNext),
        (KeyCombo::alt_shift("T"), Action::ThemePrev),
```

In `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, add two entries in the `reader` array near the other `"t"` keys (keep the file's alphabetical-ish ordering):

```json
    {"key": "t", "alt": true, "action": "ThemeNext"},
    {"key": "T", "alt": true, "shift": true, "action": "ThemePrev"},
```

First confirm no collision: `rg '"key": "t"' ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — there must be no existing entry with `"key": "t"` (or `"T"`) and `"alt": true`.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib 2>&1 | tail -10`
Expected: new tests pass, including `every_action_has_a_category` (it forces the category entries) and `alt_t_cycles_theme`.

- [ ] **Step 8: Commit (linux-lit repo, then tty-dotfiles repo)**

```bash
git add src/input/actions/mod.rs src/input/actions/settings.rs src/input/keymap.rs src/input/keymap_config.rs
git commit -m "keybinds: Alt+t / Alt+Shift+T cycle the per-app theme list"
git -C ~/tty-dotfiles add linux-lit/.config/linux-lit/keymap.json
git -C ~/tty-dotfiles commit -m "linux-lit keymap: Alt+t / Alt+Shift+T theme cycle"
```

---

### Task 4: Keybinds overlay and docs

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the Ctrl+/ overlay)
- Modify: `CLAUDE.md` (keymap section)
- Modify: `docs/guides/theming-and-external-theme-changes.md`

**Interfaces:**
- Consumes: the binds from Task 3. No new interfaces produced.

- [ ] **Step 1: Update the Ctrl+/ keybinds overlay**

Invoke the repo's `update-cairo-keybinds-overlay` skill (this is the documented required step after any keybind change; it knows the overlay's layout rules). If running without skill access, manually add a row for `Alt+t / Alt+Shift+T — cycle reader theme` to the Display section of `src/ui/keybinds_overlay.rs`, following the exact format of the adjacent `Alt+d` (ToggleDim) row.

- [ ] **Step 2: Update docs**

In `CLAUDE.md`, in the theming/keymap sections, state:

```markdown
- linux-lit's theme is INDEPENDENT of the system-wide theme system. It is
  stored in `config.json` (`theme`, default `kindle-sepia`); `Alt+t` /
  `Alt+Shift+T` cycle `theme_cycle` (default: kindle-sepia, kindle-green,
  zenbones-light, zenwritten-light). SIGUSR1 re-reads the app's OWN config
  (external control: edit config.json, then `kill -USR1`). linux-lit never
  reads or writes `~/utono/themes/.config/themes/.current_theme`.
```

In `docs/guides/theming-and-external-theme-changes.md`, update the SIGUSR1 chain description: the signal now re-applies `config.theme` (not `.current_theme`), and `set-theme.sh` no longer signals linux-lit.

- [ ] **Step 3: Build and commit**

Run: `cargo build 2>&1 | tail -3` — expected: clean.

```bash
git add src/ui/keybinds_overlay.rs CLAUDE.md docs/guides/theming-and-external-theme-changes.md
git commit -m "docs+overlay: independent theme, Alt+t/Alt+Shift+T cycle binds"
```

---

### Task 5: Decouple the themes repo and verify end-to-end

**Files:**
- Modify: `~/utono/themes/.config/themes/set-theme.sh` (the trailing `pkill -USR1 linux-lit` block)
- Modify: `~/utono/themes/CLAUDE.md` (pipeline description)

**Interfaces:**
- Consumes: everything above. No new interfaces.

- [ ] **Step 1: Remove the signal from set-theme.sh**

In `~/utono/themes/.config/themes/set-theme.sh`, delete these final lines:

```bash
# Signal linux-lit to reload theme
pkill -USR1 linux-lit 2>/dev/null || true
```

In `~/utono/themes/CLAUDE.md`, remove/adjust any sentence saying the pipeline signals linux-lit (search: `rg -n 'linux-lit' ~/utono/themes/CLAUDE.md`) — note instead that linux-lit themes itself independently.

- [ ] **Step 2: End-to-end verification (real app)**

1. Ensure config has no theme yet: `jq 'del(.theme)' ~/.config/linux-lit/config.json` → write back with `command cp` (or edit the dev config if launching in dev mode). Launch linux-lit; the startup log must show `Theme: Kindle Sepia (kindle-sepia)`.
2. Press `Alt+t` — theme visibly changes to kindle-green; `jq -r '.theme' ~/.config/linux-lit/config.json` prints `kindle-green`. Press `Alt+Shift+T` — back to kindle-sepia.
3. Run `~/.config/themes/set-theme.sh zenbones-dark` (or cycle the dwl bind) — linux-lit must NOT change. Restore the system theme afterward with `set-theme.sh <previous>`.
4. External control still works: `jq '.theme = "zenwritten-light"' ...` write-back, then `kill -USR1 $(pgrep -x linux-lit)` — reader rethemes.
5. Settings overlay (`Ctrl+comma`, row 0) still cycles themes, and `cat ~/utono/themes/.config/themes/.current_theme` is UNCHANGED by any of the above.

- [ ] **Step 3: Commit and push (themes repo)**

```bash
git -C ~/utono/themes add .config/themes/set-theme.sh CLAUDE.md
git -C ~/utono/themes commit -m "set-theme: stop signaling linux-lit (reader theme is now independent)"
```

Push linux-lit, tty-dotfiles, and themes only after the user confirms the end-to-end verification looks right.
