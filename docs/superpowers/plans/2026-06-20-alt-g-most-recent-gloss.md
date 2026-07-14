# Alt+g — Reopen the Most Recently Viewed Gloss Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reader-mode `Alt+g` keybind that reopens the gloss overlay on the gloss the user last viewed or created in the current work, restored to the exact gloss type, persisted per-work across restarts.

**Architecture:** Persist a per-work `(start_citation, gloss_type)` reference in `Config.last_gloss`. Record it via one `AppState` helper called at every gloss-display site (so a brand-new gloss is recorded the instant it shows). A new `Action::OpenLastGloss` handler reconstructs the overlay args from the stored reference (exactly like the gloss-picker confirm path) and reuses `open_gloss_overlay`, which is extended to land on a requested gloss type.

**Tech Stack:** Rust, GTK4, rusqlite, serde, GNU Stow (keymap.json).

**Reference spec:** `docs/superpowers/specs/2026-06-20-alt-g-most-recent-gloss-design.md`

---

## Build & verify conventions

- Build with `cargo build` (per CLAUDE.md — **do not run the app**; the user runs `cargo run`).
- Pure-logic tests run with `cargo test --bins`.
- Commit messages end with the Co-Authored-By trailer (see each commit step).
- Work on `master` is the project default; this plan assumes a feature branch
  `feat/alt-g-last-gloss` already exists (create it before Task 1 if not).

---

## File structure

- `src/config.rs` — `LastGloss` struct + `last_gloss` field. Owns the persisted shape.
- `src/app.rs` — `AppState::record_last_gloss` helper. Owns "stamp the most-recent reference from the current gloss_context".
- `src/input/actions/gloss.rs` — extend `open_gloss_overlay` (land on a type + record); add `open_last_gloss` handler. Owns the open/reopen flow.
- `src/input/visual.rs` — call `record_last_gloss` at the six create/cached-open display sites.
- `src/input/actions/mod.rs` — `Action::OpenLastGloss` variant.
- `src/input/keymap_config.rs` — compiled-in reader binding.
- `src/input/keymap.rs` — dispatch arm.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — JSON binding override.
- `src/ui/keybinds_overlay.rs` — Ctrl+/ overlay cap chip + `describe()` arms.

---

### Task 1: Add the persisted `LastGloss` config shape

**Files:**
- Modify: `src/config.rs` (struct `Config` around `src/config.rs:53`; add struct near `VisualModeCommand` at `src/config.rs:24-28`)
- Test: `src/config.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

Add at the bottom of `src/config.rs`:

```rust
#[cfg(test)]
mod last_gloss_tests {
    use super::*;

    #[test]
    fn last_gloss_round_trips_through_json() {
        let mut cfg = Config::default();
        cfg.last_gloss.insert(
            "Ham".to_string(),
            LastGloss { start_citation: "Ham.1.2.93".to_string(),
                        gloss_type: "reader-gloss".to_string() },
        );
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        let lg = back.last_gloss.get("Ham").unwrap();
        assert_eq!(lg.start_citation, "Ham.1.2.93");
        assert_eq!(lg.gloss_type, "reader-gloss");
    }

    #[test]
    fn config_without_last_gloss_key_loads_empty() {
        // An older config file with no `last_gloss` key must still deserialize.
        let json = r#"{}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(cfg.last_gloss.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins last_gloss_tests`
Expected: FAIL — `cannot find type LastGloss` / `no field last_gloss`.

> Note: `config_without_last_gloss_key_loads_empty` requires every other
> `Config` field to be `#[serde(default)]`. Inspection of `src/config.rs:31-87`
> confirms every field already has `#[serde(default ...)]`, so `{}` deserializes.

- [ ] **Step 3: Add the struct and field**

Add this struct after `VisualModeCommand` (after `src/config.rs:28`):

```rust
/// The most-recently-viewed gloss for one work: which passage (by its
/// start citation) and which gloss type was on screen. Reopened by Alt+g.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastGloss {
    pub start_citation: String,
    pub gloss_type: String,
}
```

Add this field to `struct Config`, immediately after the `work_positions`
field (after `src/config.rs:53`):

```rust
    /// Per-work most-recently-viewed gloss, keyed by work_abbrev. Mirrors
    /// `work_positions`. Written at every gloss-display site; read by Alt+g.
    #[serde(default)]
    pub last_gloss: HashMap<String, LastGloss>,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins last_gloss_tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: compiles. (`Config` derives `Default`? It does NOT — it has a manual
`Default` impl or `..Default::default()` is unavailable. Check: `Config::default()`
is used in the test. If `Config` has no `Default`, the test uses whatever
constructor exists.)

> IMPORTANT pre-check before Step 1: confirm how a default `Config` is built.
> `src/config.rs:31` shows `#[derive(Debug, Clone, Serialize, Deserialize)]` —
> NO `Default`. Search for `impl Default for Config` or a `Config::default`
> fn. If none exists, build the test config by deserializing `"{}"`:
> `let mut cfg: Config = serde_json::from_str("{}").unwrap();` in
> `last_gloss_round_trips_through_json` instead of `Config::default()`.
> Adjust the test accordingly. The new field needs no manual default work
> because `#[serde(default)]` + `HashMap` default `{}` covers it.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat(gloss): add per-work last_gloss config field

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Add `AppState::record_last_gloss` helper

**Files:**
- Modify: `src/app.rs` (add a method to the `impl AppState` block; `gloss_context` field is at `src/app.rs:291`)

This helper has no direct unit test (it mutates `AppState`, which needs GTK).
Its behavior is exercised at runtime in Task 7's manual checks. Keep it tiny so
inspection suffices.

- [ ] **Step 1: Add the method**

Find the `impl AppState { ... }` block in `src/app.rs`. Add:

```rust
    /// Record the currently-open gloss (from `self.gloss_context`) as the
    /// most-recently-viewed gloss for its work, and persist config. Called at
    /// every site that displays a gloss, so a freshly created gloss becomes
    /// "most recent" the instant it is shown. No-op if no gloss_context is set.
    pub fn record_last_gloss(&mut self, gloss_type: &str) {
        if let Some(ctx) = &self.gloss_context {
            let work = ctx.work_abbrev.clone();
            let entry = crate::config::LastGloss {
                start_citation: ctx.start_citation.clone(),
                gloss_type: gloss_type.to_string(),
            };
            self.config.last_gloss.insert(work, entry);
            crate::config::save(&self.config);
        }
    }
```

> `crate::config::save(&Config)` is the persistence fn (`src/config.rs:247`),
> the same one `ToggleVocabHighlight` uses (`src/input/keymap.rs:1958`).
> `self.config` is the live `Config` on `AppState` (confirmed used at
> keymap.rs:1957). `self.gloss_context` is `Option<GlossContext>` at
> `src/app.rs:291`; `GlossContext` has `work_abbrev` and `start_citation`
> (`src/gloss.rs:481,483`).

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles (helper is unused for now — allow the dead-code warning; it
is wired up in Tasks 3 and 4).

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(gloss): add AppState::record_last_gloss helper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Extend `open_gloss_overlay` to land on a requested gloss type and record it

**Files:**
- Modify: `src/input/actions/gloss.rs:1852-1903` (`open_gloss_overlay`)
- Modify: `src/input/actions/gloss.rs:2010` (cursor-open caller)
- Modify: `src/input/keymap.rs:424` (picker-confirm caller)
- Test: `src/input/actions/gloss.rs` (inline `#[cfg(test)]` for the pure index helper)

The index-selection logic is a pure function — unit-test that. The signature
change is verified by `cargo build` (both call sites updated).

- [ ] **Step 1: Write the failing test for the pure selector**

Add to `src/input/actions/gloss.rs` (bottom, inline test module):

```rust
#[cfg(test)]
mod start_gloss_idx_tests {
    use super::start_gloss_idx;

    #[test]
    fn matches_requested_type() {
        let types = ["teacher-generic", "reader-gloss", "inner-monologue"];
        assert_eq!(start_gloss_idx(&types, Some("reader-gloss")), 1);
        assert_eq!(start_gloss_idx(&types, Some("inner-monologue")), 2);
    }

    #[test]
    fn falls_back_to_zero_when_type_absent() {
        let types = ["teacher-generic"];
        assert_eq!(start_gloss_idx(&types, Some("reader-gloss")), 0);
    }

    #[test]
    fn falls_back_to_zero_when_none_requested() {
        let types = ["teacher-generic", "reader-gloss"];
        assert_eq!(start_gloss_idx(&types, None), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins start_gloss_idx_tests`
Expected: FAIL — `cannot find function start_gloss_idx`.

- [ ] **Step 3: Add the pure helper**

Add near `open_gloss_overlay` in `src/input/actions/gloss.rs` (above it):

```rust
/// Pick the starting index into a gloss list for a desired gloss type.
/// Returns the index of the first gloss whose type matches `desired`, or 0
/// when `desired` is None or no gloss of that type is present.
fn start_gloss_idx(types: &[impl AsRef<str>], desired: Option<&str>) -> usize {
    desired
        .and_then(|d| types.iter().position(|t| t.as_ref() == d))
        .unwrap_or(0)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins start_gloss_idx_tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Change `open_gloss_overlay` to use the desired type + record**

Replace the body of `open_gloss_overlay` (`src/input/actions/gloss.rs:1852-1903`)
with this. Changes: new `desired_type: Option<&str>` param; compute `idx` via
`start_gloss_idx`; display/position/context use `idx`; call `record_last_gloss`
at the end.

```rust
pub(crate) fn open_gloss_overlay(
    s: &mut AppState,
    passages: Vec<crate::db::queries::GlossedPassage>,
    passage_index: usize,
    passage: crate::db::queries::GlossedPassage,
    all_glosses: Vec<crate::db::queries::SavedGloss>,
    from_picker: bool,
    desired_type: Option<&str>,
) {
    let types: Vec<&str> = all_glosses.iter().map(|g| g.gloss_type.as_str()).collect();
    let idx = start_gloss_idx(&types, desired_type);

    let work_title = s
        .current_work
        .as_ref()
        .map(|w| w.title.clone())
        .unwrap_or_default();
    let ctx = crate::gloss::GlossContext {
        work_abbrev: passage.work_abbrev,
        work_title,
        start_citation: passage.start_citation,
        end_citation: passage.end_citation,
        act: passage.act,
        scene: passage.scene,
        speaker: passage.speaker,
        source_text: passage.source_text,
        source_line_numbers: Vec::new(),
        hash: String::new(),
        gloss_type: all_glosses[idx].gloss_type.clone(),
    };

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let source_lines: Vec<(String, i64)> = Vec::new();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text,
        &all_glosses[idx].gloss_text,
        cw,
        h,
        Some(&s.theme.root_color),
        &source_lines,
    );
    s.gloss_overlay.set_position(idx, all_glosses.len());

    let shown_type = all_glosses[idx].gloss_type.clone();
    s.gloss_passages = passages;
    s.gloss_passage_index = passage_index;
    s.gloss_list = all_glosses;
    s.gloss_index = idx;
    s.gloss_active_voice = 0;
    s.gloss_context = Some(ctx);
    s.gloss_opened_from_picker = from_picker;
    // input_mode MUST be set before recolor: recolor_cached_blocks selects the
    // gloss vs synopsis branch off it and no-ops otherwise.
    s.input_mode = crate::app::InputMode::GlossOverlay;
    recolor_cached_blocks(s);

    // Stamp the most-recent reference from the gloss now displayed.
    s.record_last_gloss(&shown_type);
}
```

- [ ] **Step 6: Update the cursor-open caller**

At `src/input/actions/gloss.rs:2010`, add the trailing `None`:

```rust
    open_gloss_overlay(&mut s, passages, passage_index, passage, all_glosses, false, None);
```

- [ ] **Step 7: Update the picker-confirm caller**

At `src/input/keymap.rs:424`, the call currently passes 6 args ending `true`.
Add the trailing `None`:

```rust
                        crate::input::actions::gloss::open_gloss_overlay(
                            &mut s, passages, idx, passage, all_glosses, true, None,
                        );
```

> Verify the exact argument names at that site before editing — match the
> existing local variable names (`passages`, `idx`, `passage`, `all_glosses`).
> Only the new trailing `None` is added.

- [ ] **Step 8: Build and test**

Run: `cargo build`
Expected: compiles, no "wrong number of arguments" errors.

Run: `cargo test --bins`
Expected: PASS (config + selector tests green).

- [ ] **Step 9: Commit**

```bash
git add src/input/actions/gloss.rs src/input/keymap.rs
git commit -m "feat(gloss): open_gloss_overlay lands on requested type and records last gloss

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Record on freshly-created and cached-open visual-mode glosses

**Files:**
- Modify: `src/input/visual.rs` — six display sites (per spec §2).

Each site already sets `s.gloss_context = Some(ctx)` right after showing the
gloss. Insert `s.record_last_gloss("<type>")` immediately AFTER that assignment,
using the literal type for that action. No tests here (GTK + async); verified at
runtime in Task 7.

- [ ] **Step 1: Locate the six `s.gloss_context = Some(ctx)` sites**

Run: `rg -n "gloss_context = Some" src/input/visual.rs`
Expected: six matches, near these anchors (cached-open / fresh per action):
- `action_reader_gloss` cached ≈ line 456; fresh ≈ line 526
- `action_gloss_with_claude` cached ≈ line 597; fresh ≈ line 667
- `action_inner_monologue` cached ≈ line 738; `run_pending_inner_monologue_blocking` fresh ≈ line 932

> Line numbers drift; the `rg` output is authoritative. Match each site to its
> enclosing function to pick the right type literal (below).

- [ ] **Step 2: Insert the recording call at each site**

After each `s.gloss_context = Some(ctx);` (the `s` is the mutable `AppState`
borrow in scope — synchronous sites borrow directly; async sites use
`state_for_result.borrow_mut()` already bound to `s`), add:

- In `action_reader_gloss` (both its sites):

```rust
        s.record_last_gloss("reader-gloss");
```

- In `action_gloss_with_claude` (both its sites):

```rust
        s.record_last_gloss("teacher-generic");
```

- In `action_inner_monologue` cached site AND `run_pending_inner_monologue_blocking` fresh site:

```rust
        s.record_last_gloss("inner-monologue");
```

> If any site stores the context as a different variable name than `ctx`, or the
> mutable state borrow is not named `s`, adapt the call to the in-scope state
> borrow — the requirement is only that it runs after `gloss_context` is set,
> under a mutable `AppState`. Do not clone or re-borrow; reuse the existing one.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles. If a borrow-checker error appears (e.g. `s` already moved
or a shared borrow active), move the `record_last_gloss` call to the last line
that holds the mutable borrow, still after the `gloss_context` assignment.

- [ ] **Step 4: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat(gloss): record last gloss on visual-mode create and cached-open

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Add `Action::OpenLastGloss` and its handler

**Files:**
- Modify: `src/input/actions/mod.rs` (add enum variant; gloss actions live near `mod.rs:98-99`)
- Modify: `src/input/actions/gloss.rs` (add `open_last_gloss` handler)
- Modify: `src/input/keymap.rs:1961-1962` (add dispatch arm)

- [ ] **Step 1: Add the Action variant**

In `src/input/actions/mod.rs`, next to `OpenGlossPicker` (≈ `mod.rs:99`), add:

```rust
    OpenLastGloss,
```

> If `Action` is parsed from `keymap.json` by `strum`/`serde` or a manual
> `from_str`, the variant name `OpenLastGloss` must match the JSON `action`
> string exactly. Confirm by checking how other variants map (the JSON uses the
> exact variant name, e.g. `"OpenGlossPicker"`). No extra wiring if it is a
> derive-based string match.

- [ ] **Step 2: Add the handler**

Add to `src/input/actions/gloss.rs` (after `toggle_overlay`, ≈ line 2011). This
mirrors `toggle_overlay`'s resolve-then-open structure but resolves the passage
from `config.last_gloss` instead of the cursor, and uses the stored
`gloss_type` as `desired_type`. All failure paths show the same toast.

```rust
/// Reopen the gloss overlay on the most-recently-viewed gloss for the current
/// work (persisted in `config.last_gloss`), restored to the gloss type that was
/// last shown. Toasts "no recent gloss" when there is no usable reference
/// (none recorded, passage gone, or no glosses remain).
pub(crate) fn open_last_gloss(state: &Rc<RefCell<AppState>>) {
    const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];

    // Resolve current work + the stored reference, all under a shared borrow.
    let (work_abbrev, start_citation, desired_type) = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => {
                drop(s);
                show_tts_toast(state, "No recent gloss");
                return;
            }
        };
        let abbrev = crate::app::base_work_abbrev(&work.abbrev).to_string();
        match s.config.last_gloss.get(&abbrev) {
            Some(lg) => (abbrev, lg.start_citation.clone(), lg.gloss_type.clone()),
            None => {
                drop(s);
                show_tts_toast(state, "No recent gloss");
                return;
            }
        }
    };

    // Read-only DB work before any mutation (same pattern as toggle_overlay).
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => {
            show_tts_toast(state, "No recent gloss");
            return;
        }
    };
    let passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev, GLOSS_TYPES)
        .unwrap_or_default();

    let found = passages
        .iter()
        .enumerate()
        .find(|(_, p)| p.start_citation == start_citation);
    let (passage_index, passage) = match found {
        Some((i, p)) => (i, p.clone()),
        None => {
            // Stale reference: passage deleted or work re-imported.
            show_tts_toast(state, "No recent gloss");
            return;
        }
    };

    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();
    if all_glosses.is_empty() {
        show_tts_toast(state, "No recent gloss");
        return;
    }

    let mut s = state.borrow_mut();
    // Remember the reader page so Escape returns here (from_picker = false).
    s.gloss_return_pos = Some((s.current_line, s.page_top_line));
    open_gloss_overlay(
        &mut s,
        passages,
        passage_index,
        passage,
        all_glosses,
        false,
        Some(&desired_type),
    );
}
```

> Confirm `show_tts_toast` is in scope in `gloss.rs` — it is the toast used by
> `toggle_overlay` (`src/input/actions/gloss.rs:1930`). `base_work_abbrev`
> (`src/app.rs`, used at gloss.rs:1951) and `open_db`/`find_glossed_passages`/
> `find_glosses_by_start` are all already imported/used in this file.

- [ ] **Step 3: Add the dispatch arm**

In `src/input/keymap.rs`, next to the `OpenGlossPicker` arm (`keymap.rs:1962`),
add:

```rust
        OpenLastGloss => crate::input::actions::gloss::open_last_gloss(state),
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles. A non-exhaustive-match error on `Action` means another
`match` needs the arm — add `OpenLastGloss` there (or a wildcard if the match
already has one).

- [ ] **Step 5: Run full bin tests (regression)**

Run: `cargo test --bins`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/mod.rs src/input/actions/gloss.rs src/input/keymap.rs
git commit -m "feat(gloss): add OpenLastGloss action and Alt+g handler

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Bind Alt+g (compiled default + keymap.json)

**Files:**
- Modify: `src/input/keymap_config.rs` (reader bindings, near `keymap_config.rs:275`)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (near line 34)

Both files must change or the JSON silently overrides the compiled default
(CLAUDE.md). Reader-mode Alt+g is currently free (overlay-mode Alt+g at
`keymap.rs:711` is a different input mode — no conflict).

- [ ] **Step 1: Add the compiled-in binding**

In `src/input/keymap_config.rs`, immediately after the `OpenGlossPicker` line
(`keymap_config.rs:275`):

```rust
        (KeyCombo::alt("g"), Action::OpenLastGloss),
```

> `KeyCombo::alt(&str)` exists (`keymap_config.rs:36`). On RPD, `g` is a plain
> letter key — `"g"` is the correct key name (confirmed: `Ctrl+g` uses `"g"` at
> keymap_config.rs:275).

- [ ] **Step 2: Add the JSON override**

In `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, after the
`OpenGlossPicker` entry (line 34), add:

```json
    {"key": "g", "alt": true, "action": "OpenLastGloss"},
```

> Match the modifier-flag spelling used elsewhere in the file. Inspection shows
> `"ctrl": true` and `"shift": true` are used (lines 33-34); confirm the loader
> reads `"alt"` (vs `"meta"`). Check `keymap_config.rs`'s JSON parser for the
> alt flag name and use exactly that key. If the parser uses `"alt"`, the line
> above is correct.

- [ ] **Step 3: Deploy the stow package**

```bash
cd ~/tty-dotfiles && stow linux-lit
```

Expected: no conflict output (file already symlinked; edit is in place).

- [ ] **Step 4: Build**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: compiles.

- [ ] **Step 5: Commit (linux-lit repo)**

```bash
cd ~/utono/linux-lit
git add src/input/keymap_config.rs
git commit -m "feat(gloss): bind reader Alt+g to OpenLastGloss

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 6: Commit (tty-dotfiles repo)**

```bash
cd ~/tty-dotfiles
git add linux-lit/.config/linux-lit/keymap.json
git commit -m "feat(linux-lit): bind Alt+g to OpenLastGloss in keymap.json

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Update the Ctrl+/ keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the `g` key row at `keybinds_overlay.rs:58`; `describe()` arms at ≈ line 348 and ≈ line 621)

Per CLAUDE.md this is mandatory and must use the dedicated skill, which carries
the exhaustive three-pass cross-reference.

- [ ] **Step 1: Invoke the skill**

Use the `update-cairo-keybinds-overlay` skill to add the `M-g` → "last gloss"
binding to the `g` key. The intended edits (the skill will verify/finalize them):

  - In the `g` row (`keybinds_overlay.rs:58`), add the chip to the modifier
    list, so it reads:

```rust
    key("g", "G", "", "", &[("C-g", "gloss pick"), ("S-C-g", "gloss tog"), ("M-g", "last gloss")]),
```

  - Add a detail-panel `describe()` arm (near the existing `"gloss pick"` arm at
    ≈ line 348):

```rust
        "last gloss" => "Reopen the gloss overlay on the most recently viewed \
gloss in this work, restored to its gloss type. Persists per work across \
restarts; toasts \"No recent gloss\" if none is recorded. \
-> open_last_gloss — src/input/actions/gloss.rs",
```

  - Add the short-label arm (near the second `"gloss pick"` arm at ≈ line 621):

```rust
        "last gloss" => "open last gloss",
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(gloss): show Alt+g last-gloss in keybinds overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Final verification

- [ ] **Step 1: Full build + bin tests**

Run: `cargo build && cargo test --bins`
Expected: both green.

- [ ] **Step 2: Clippy (project convention)**

Run: `cargo clippy`
Expected: no new warnings from the changed files. Fix any the new code raised.

- [ ] **Step 3: Hand off runtime/visual verification to the user**

The acceptance criterion is visual (the right gloss renders, persistence across
restart). Per CLAUDE.md, the user runs the app. Ask the user to `cargo run` and
confirm, with this checklist (spec §Verification):

  1. Open a work, create a gloss in visual mode (`V` → action), Escape,
     press `Alt+g` → overlay reopens on that gloss, correct type.
  2. View a different existing gloss of a different type, Escape, `Alt+g` →
     reopens the most recent one, correct type.
  3. Restart on the same work, `Alt+g` → reopens the remembered gloss
     (persistence).
  4. Work with no gloss ever viewed, `Alt+g` → "No recent gloss" toast.
  5. Delete the remembered gloss, `Alt+g` → "No recent gloss" toast (stale
     guard).

  Headless equivalent (if the user prefers the harness):

```bash
./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture
```

---

## Self-review notes

- **Spec coverage:** config shape (Task 1) ✓; record helper (Task 2) ✓;
  land-on-type + record in shared path (Task 3) ✓; freshness via six visual
  sites (Task 4) ✓; handler with stale guards + toast (Task 5) ✓; binding trio
  (Tasks 6-7) ✓; verification (Task 8) ✓.
- **Type consistency:** `LastGloss { start_citation, gloss_type }`,
  `Config.last_gloss: HashMap<String, LastGloss>`, `record_last_gloss(&str)`,
  `start_gloss_idx(&[..], Option<&str>)`, `open_gloss_overlay(.., desired_type:
  Option<&str>)`, `open_last_gloss(&Rc<RefCell<AppState>>)`,
  `Action::OpenLastGloss` — names used identically across all tasks.
- **Soft fallback** (recorded type gone but passage has other glosses) is
  handled implicitly by `start_gloss_idx → 0` in Task 3, matching spec §6.
- **Pre-checks flagged inline** where line numbers/derives must be confirmed
  before editing (Config Default in Task 1; arg names in Task 3 Step 7; the six
  borrow sites in Task 4; the JSON alt-flag spelling in Task 6).
```
