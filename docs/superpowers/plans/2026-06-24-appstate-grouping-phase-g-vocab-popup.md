# AppState Grouping Phase G — vocab_popup cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the seven flat `vocab_popup*` fields of `AppState` into one `VocabPopupState` sub-struct — the final, hardest contained cluster (holds a widget; has a widget/state name-collision; render-tier).

**Architecture:** Define `pub struct VocabPopupState` in `src/app/vocab_popup.rs`, replace the seven flat `AppState` fields with one `vocab_popup: VocabPopupState`, init via an explicit nested literal capturing the `vocab_popup` widget local, and perform a **TWO-WAY access rewrite**: state fields `s.vocab_popup_<x>` → `s.vocab_popup.<x>` (~31 sites) AND bare-widget calls `s.vocab_popup.<method>()` → `s.vocab_popup.popup.<method>()` (14 sites). Behavior-CHANGING (access shape only). Render-tier → user gate before merge.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`, `scripts/e2e-env.sh` nav-fuzz + a manual vocab-popup eyeball.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** Purely access-shape. NO value/logic/control-flow change.
- **RENDER-TIER:** agent runs build + `cargo test --bins` (**413**) + clippy (**115**) — necessary, NOT sufficient. The widget-render path is proven only by a **user-run gate** (Task 2): nav-fuzz + a manual vocab-popup open/update/hide eyeball. Agent cannot launch cage.
- **TWO-WAY REWRITE (the crux):**
  - **State fields:** `vocab_popup_data`→`vocab_popup.data`, `vocab_popup_index`→`vocab_popup.index`, `vocab_popup_view`→`vocab_popup.view`, `vocab_popup_auto`→`vocab_popup.auto`, `vocab_popup_line`→`vocab_popup.line`, `vocab_popup_fade_gen`→`vocab_popup.fade_gen`.
  - **Bare widget:** `s.vocab_popup.<method>()` / `state.vocab_popup.<method>()` → `…vocab_popup.popup.<method>()` (the widget moved into the `popup` sub-field).
- **Explicit nested literal init** capturing the widget local (NOT `::default()`; the widget field isn't Default-constructible, and `vocab_popup_view: VocabView::Definition` is non-default).
- **BOUNDARY — stay FLAT, do NOT group/touch:** `vocab_words`, `vocab_matches`, `vocab_match_idx`, `vocab_tag`, `vocab_highlight_visible` (the separate vocab-HIGHLIGHT subsystem). In particular `s.vocab_matches` (read by the popup module) stays `s.vocab_matches`.
- **No facade.**
- 45 sites across 5 files: `src/app/vocab_popup.rs` (29), `src/input/keymap.rs` (8), `src/app/scene_synopsis.rs` (4), `src/input/highlight.rs` (3), `src/input/actions/pickers.rs` (1).
- Branch off `master`. Branch name: `refactor/appstate-grouping-vocab-popup`.

---

### Task 0: Branch + baseline

- [ ] **Step 1: Branch + baselines**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/appstate-grouping-vocab-popup
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record. (No commit.)

---

### Task 1: Group `vocab_popup*` into `VocabPopupState`

**Files:**
- Modify: `src/app/vocab_popup.rs` (define `VocabPopupState`; rewrite 29 sites — state + widget)
- Modify: `src/app/mod.rs` (7 fields → 1; init shorthand → nested literal)
- Modify: `src/input/keymap.rs` (8 sites — state + widget)
- Modify: `src/app/scene_synopsis.rs` (4 sites — widget)
- Modify: `src/input/highlight.rs` (3 sites — state + widget)
- Modify: `src/input/actions/pickers.rs` (1 site — widget)

**Interfaces:**
- Produces: `pub struct VocabPopupState { popup: VocabPopup, data: Vec<VocabWordData>, index: usize, view: VocabView, auto: bool, line: Option<usize>, fade_gen: Rc<Cell<u64>> }` in `crate::app::vocab_popup`; `AppState.vocab_popup: VocabPopupState`.

- [ ] **Step 1: Define `VocabPopupState` in `src/app/vocab_popup.rs`**

Add near the top (after the `use` lines, before the first fn):

```rust
/// Grouped state for the vocab popup (the Popover widget itself plus its
/// per-open data list, navigation index, view mode, auto-show flag, anchor
/// line, and fade generation counter). Was seven flat `vocab_popup*` fields on
/// AppState; grouped per the AppState god-struct decomposition (render-tier).
/// NOTE: the separate vocab-HIGHLIGHT fields (vocab_words, vocab_matches,
/// vocab_match_idx, vocab_tag, vocab_highlight_visible) are a different
/// subsystem and stay flat on AppState.
pub struct VocabPopupState {
    pub popup: crate::ui::vocab_popup::VocabPopup,
    pub data: Vec<crate::ui::vocab_popup::VocabWordData>,
    pub index: usize,
    pub view: crate::ui::vocab_popup::VocabView,
    pub auto: bool,
    pub line: Option<usize>,
    pub fade_gen: std::rc::Rc<std::cell::Cell<u64>>,
}
```

No `#[derive(Default)]`. (If `vocab_popup.rs` already imports `Rc`/`Cell`/the ui types by bare name, the bare forms are fine; `cargo build` confirms.)

- [ ] **Step 2: Replace the seven flat fields in `AppState`**

In `src/app/mod.rs`, find the seven field decls (`rg -n 'pub vocab_popup' src/app/mod.rs` — lines for `vocab_popup`, `vocab_popup_data`, `vocab_popup_index`, `vocab_popup_view`, `vocab_popup_auto`, `vocab_popup_line`, `vocab_popup_fade_gen`):

```rust
// remove these seven lines:
pub vocab_popup: crate::ui::vocab_popup::VocabPopup,
pub vocab_popup_data: Vec<crate::ui::vocab_popup::VocabWordData>,
pub vocab_popup_index: usize,
pub vocab_popup_view: crate::ui::vocab_popup::VocabView,
pub vocab_popup_auto: bool,
pub vocab_popup_line: Option<usize>,
pub vocab_popup_fade_gen: Rc<Cell<u64>>,
```

Replace with one line:

```rust
pub vocab_popup: crate::app::vocab_popup::VocabPopupState,
```

Do NOT touch the flat highlight fields (`vocab_words`, `vocab_matches`, `vocab_match_idx`, `vocab_tag`, `vocab_highlight_visible`).

- [ ] **Step 3: Replace the init (shorthand → explicit nested literal capturing the widget local)**

In `src/app/mod.rs` build_window, the widget local is `let vocab_popup = crate::ui::vocab_popup::VocabPopup::new();` (~line 1096 — leave it). In the `AppState { … }` literal, find the shorthand `vocab_popup,` (~1527) and the six `vocab_popup_*: …` init lines (~1528–1533):

```rust
// remove the shorthand `vocab_popup,` and these six lines:
vocab_popup_data: Vec::new(),
vocab_popup_index: 0,
vocab_popup_view: crate::ui::vocab_popup::VocabView::Definition,
vocab_popup_auto: false,
vocab_popup_line: None,
vocab_popup_fade_gen: Rc::new(Cell::new(0)),
```

Replace all seven with one nested literal (the `popup` field takes the captured local):

```rust
vocab_popup: crate::app::vocab_popup::VocabPopupState {
    popup: vocab_popup,
    data: Vec::new(),
    index: 0,
    view: crate::ui::vocab_popup::VocabView::Definition,
    auto: false,
    line: None,
    fade_gen: Rc::new(Cell::new(0)),
},
```

- [ ] **Step 4: First rewrite pass — STATE fields (~31 sites)**

Across `vocab_popup.rs`, `keymap.rs`, `highlight.rs` (the files that touch state fields), rewrite `vocab_popup_<suffix>` → `vocab_popup.<suffix>`:
`vocab_popup_data`→`vocab_popup.data`, `vocab_popup_index`→`vocab_popup.index`, `vocab_popup_view`→`vocab_popup.view`, `vocab_popup_auto`→`vocab_popup.auto`, `vocab_popup_line`→`vocab_popup.line`, `vocab_popup_fade_gen`→`vocab_popup.fade_gen`. Compound forms (`.clone()`, `.set(...)`, `.get(...)`, indexing, `+= 1`) carry over identically.

Do NOT do this in `mod.rs` (handled by Steps 2-3). Do NOT touch `vocab_matches`/`vocab_words`/etc.

- [ ] **Step 5: Build to surface the WIDGET sites**

```bash
cargo build
```
Expected: errors of the form `no method named <m> found for struct VocabPopupState` / `no field <x> on VocabPopupState`. **This error list is the widget-rewrite checklist** for Step 6. (Plus possibly state sites missed in Step 4 — fix those as `vocab_popup.<suffix>` too.)

- [ ] **Step 6: Second rewrite pass — BARE WIDGET (14 sites)**

Rewrite every `s.vocab_popup.<method>` / `state.vocab_popup.<method>` → `…vocab_popup.popup.<method>` at these exact sites (the widget moved into `popup`):
- `src/app/vocab_popup.rs`: 79 (`set_margin_start`), 85 (`hide`), 91 (`hide`), 99 (`update`), 106 (`show`), 113 (`is_visible`), 143 (`hide`)
- `src/app/scene_synopsis.rs`: 231 (`update_synopsis`), 232 (`show`)
- `src/input/keymap.rs`: 2351 (`is_visible`), 2354 (`widget()`), 2363 (`widget()`)
- `src/input/highlight.rs`: 165 (`is_visible`)
- `src/input/actions/pickers.rs`: 791 (`hide`)

(e.g. `state.vocab_popup.hide()` → `state.vocab_popup.popup.hide()`; `s.vocab_popup.widget()` → `s.vocab_popup.popup.widget()`.)

- [ ] **Step 7: Build (clean)**

```bash
cargo build
```
Expected: clean. Iterate Steps 4/6 on any remaining `no field`/`no method` error.

- [ ] **Step 8: Clippy + tests + zero-flat-form check**

```bash
cargo clippy 2>&1 | rg -c 'warning:'
cargo test --bins 2>&1 | rg 'test result'
rg -n 's\.vocab_popup_\w+|state\.vocab_popup_\w+' src/
```
Expected: clippy `115`; `413 passed`; the `rg` returns ZERO hits (all state fields rewritten; the bare widget form `s.vocab_popup.popup.X` is fine; the highlight fields `vocab_matches` etc. are not matched by `vocab_popup_`).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(app): group vocab_popup* fields into VocabPopupState

Final contained cluster of the AppState god-struct grouping (render-tier,
hardest). The seven flat vocab_popup* fields (the VocabPopup widget +
data/index/view/auto/line/fade_gen) become one VocabPopupState sub-struct in
src/app/vocab_popup.rs, held as AppState.vocab_popup. Explicit nested literal
init captures the vocab_popup widget local and preserves view:
VocabView::Definition (non-default). Two-way access rewrite: state fields
s.vocab_popup_x -> s.vocab_popup.x, and bare-widget calls s.vocab_popup.m() ->
s.vocab_popup.popup.m() (the widget moved into the popup sub-field), 45 sites
across 5 files. The separate vocab-HIGHLIGHT fields (vocab_words/vocab_matches/
vocab_match_idx/vocab_tag/vocab_highlight_visible) stay flat. Behavior-
preserving: access shape only. 413 tests + clippy 115 unchanged.

Render-tier: user gate (nav-fuzz + manual vocab-popup eyeball) before merge.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 10: Report runtime verification blocked**

Note in the report: unit gates pass + prove the rewrite compiles, BUT vocab_popup IS the popup widget — the open/update/hide render path needs the user gate (Task 2). Agent cannot launch cage.

**STOP after commit. Do NOT merge/push — the user render gate is Task 2.**

---

### Task 2: User render verification + finish the branch

- [ ] **Step 1: Final unit gates**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: clean, `413 passed`, clippy `115`. `git status` clean.

- [ ] **Step 2: ASK THE USER to run the render gate (REQUIRED)**

The agent cannot run these. Give both, wait for confirmation:

Part 1 — nav-fuzz on a work with vocab data (no regression in popup auto-show / fade nav paths):
```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Son
```

Part 2 — manual vocab-popup eyeball: launch a work with vocab words, open the vocab popup (the keybind that calls `open_vocab_popup`), confirm it opens, shows word data, toggles Definition/Gloss view, and hides — exactly as before the change.

**Do NOT merge until the user confirms BOTH clean.** Regression → systematic debugging, do NOT merge.

- [ ] **Step 3: Merge (only after user confirms)**

```bash
git checkout master
git merge --no-ff refactor/appstate-grouping-vocab-popup
```

- [ ] **Step 4: Re-verify merged**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean, `413 passed`.

- [ ] **Step 5: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping-vocab-popup
```

- [ ] **Step 6: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, mark Phase G (`vocab_popup` → `VocabPopupState`) DONE and note **ALL seven contained clusters are now grouped** — the AppState grouping project's committed scope is complete (core fields stay flat; medium-spread clusters deferred). Commit + push:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping Phase G (vocab_popup) DONE — all contained clusters complete

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- VocabPopupState (7 fields incl widget, no Default) in vocab_popup.rs (spec "The sub-struct") → Task 1 Step 1 ✓
- 7 flat fields → 1 (spec "AppState change") → Task 1 Step 2 ✓
- explicit nested literal capturing the widget local, view: VocabView::Definition (spec "Non-default init") → Task 1 Step 3 ✓
- TWO-WAY rewrite: state fields (Step 4) + bare widget → .popup. (Step 6), with the build-surfaces-widget-sites trick (Step 5) (spec "The name-collision") ✓
- boundary: highlight fields stay flat, vocab_matches untouched (spec "Boundary") → Global Constraints + Task 1 Steps 2/4 ✓
- render-tier user gate: nav-fuzz + manual popup eyeball (spec "Verification") → Global Constraints + Task 1 Step 10 + Task 2 Step 2 ✓
- no facade (spec) → Global Constraints ✓

**Placeholder scan:** No TBD/TODO. The 14 widget sites are enumerated by file:line (Step 6); state-field sites located by suffix. `<ABBR>` in Task 2 Part 1 is `Son` (any vocab-bearing verse work).

**Type consistency:** `VocabPopupState` field names (popup/data/index/view/auto/line/fade_gen) consistent across struct def (Step 1), init literal (Step 3), and both rewrite passes (Steps 4/6). The widget sub-field `popup` is used consistently in every bare-widget rewrite. `AppState.vocab_popup` matches init and all `s.vocab_popup.*` accesses.
