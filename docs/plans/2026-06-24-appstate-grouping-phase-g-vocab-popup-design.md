# AppState grouping Phase G — vocab_popup cluster (final contained cluster)

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING field grouping (access-shape only).
**RENDER-TIER**, and the **hardest contained cluster** — it holds a real widget
and has a name-collision between the cluster field and the widget. Final cluster
of the AppState god-struct grouping project
(`docs/superpowers/specs/2026-06-23-appstate-grouping-design.md`).

## The cluster — and the boundary that is the main risk

Seven flat `AppState` fields form the **vocab-popup's own state**:

| flat field | type | → sub-struct field |
|---|---|---|
| `vocab_popup` (the **widget**) | `crate::ui::vocab_popup::VocabPopup` | `popup` |
| `vocab_popup_data` | `Vec<crate::ui::vocab_popup::VocabWordData>` | `data` |
| `vocab_popup_index` | `usize` | `index` |
| `vocab_popup_view` | `crate::ui::vocab_popup::VocabView` | `view` |
| `vocab_popup_auto` | `bool` | `auto` |
| `vocab_popup_line` | `Option<usize>` | `line` |
| `vocab_popup_fade_gen` | `Rc<Cell<u64>>` | `fade_gen` |

**BOUNDARY — these stay FLAT (a SEPARATE vocab-highlight subsystem, NOT this
cluster):** `vocab_words`, `vocab_matches`, `vocab_match_idx`, `vocab_tag`,
`vocab_highlight_visible`. They drive buffer word-highlighting (`apply_vocab_highlighting`
/ `build_vocab_matches`), a different concern. The popup module *reads*
`s.vocab_matches` (vocab_popup.rs:27/134) — that access stays `s.vocab_matches`
unchanged; do NOT group it. Grouping the wrong vocab field is the primary risk.

## The name-collision (why this is the hardest cluster)

The field `vocab_popup` is BOTH the cluster name AND the widget handle. After
grouping, `state.vocab_popup` is the sub-struct, and the widget lives at
`state.vocab_popup.popup`. So there are **two kinds of access to rewrite**, and a
naive `vocab_popup_` → `vocab_popup.` token replace is WRONG because it misses
the bare-widget accesses (which have no `_suffix`):

1. **Widget method calls** (~11 sites) — `state.vocab_popup.<method>()` →
   `state.vocab_popup.popup.<method>()`. The methods seen:
   `.hide()`, `.show()`, `.update(...)`, `.update_synopsis(...)`, `.is_visible()`,
   `.set_margin_start(...)`, `.widget` (the VocabPopup's own `.widget()` /
   `.widget` accessor — becomes `s.vocab_popup.popup.widget`). Locations:
   vocab_popup.rs (79/85/91/99/106/113/143), scene_synopsis.rs (231/232),
   highlight.rs (165), and any in keymap.rs/pickers.rs.
2. **State field accesses** (~37 sites) — `state.vocab_popup_<suffix>` →
   `state.vocab_popup.<suffix>` (prefix-stripped per the table):
   `vocab_popup_data`→`data` (10), `vocab_popup_index`→`index` (8),
   `vocab_popup_view`→`view` (5), `vocab_popup_auto`→`auto` (5),
   `vocab_popup_line`→`line` (4), `vocab_popup_fade_gen`→`fade_gen` (5).

**The implementer must rewrite BOTH kinds.** A `cargo build` after only the
state rewrite will surface the widget sites as `no method <m> on VocabPopupState`
— that error list IS the widget-rewrite checklist.

## Access spread

45 sites across **5 files**: `src/app/vocab_popup.rs` (29 — the popup module
itself), `src/input/keymap.rs` (8), `src/app/scene_synopsis.rs` (4),
`src/input/highlight.rs` (3), `src/input/actions/pickers.rs` (1).

## Non-default init → explicit nested literal (widget local captured)

The widget is constructed BEFORE the struct literal (`let vocab_popup =
crate::ui::vocab_popup::VocabPopup::new();` at mod.rs:1096) and inited via field
shorthand (`vocab_popup,`). Two inits are non-default: the widget itself and
`vocab_popup_view: VocabView::Definition` (VocabView has variants
`Definition`/`Gloss`, no `Default`). So the init is an **explicit nested literal**
referencing the captured local:

```rust
vocab_popup: VocabPopupState {
    popup: vocab_popup,                                   // the captured local (line 1096)
    data: Vec::new(),
    index: 0,
    view: crate::ui::vocab_popup::VocabView::Definition,
    auto: false,
    line: None,
    fade_gen: Rc::new(Cell::new(0)),
},
```

No `#[derive(Default)]` (the widget field isn't Default-constructible anyway).
The `let vocab_popup = …VocabPopup::new();` local at 1096 stays as-is — it's moved
into the `popup` field by the literal.

## The sub-struct

Define in `src/app/vocab_popup.rs` (the popup module — its heaviest consumer):

```rust
/// Grouped state for the vocab popup (the Popover widget itself plus its
/// per-open data list, navigation index, view mode, auto-show flag, anchor
/// line, and fade generation counter). Was seven flat `vocab_popup*` fields on
/// AppState; grouped per the AppState god-struct decomposition (render-tier
/// cluster). NOTE: the separate vocab-HIGHLIGHT fields (vocab_words,
/// vocab_matches, vocab_match_idx, vocab_tag, vocab_highlight_visible) are a
/// different subsystem and stay flat on AppState.
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

(Confirm the `Rc`/`Cell` path against what vocab_popup.rs already imports — use
the bare `Rc<Cell<u64>>` if they're in scope, else the fully-qualified path
above. `cargo build` confirms.)

## AppState change

Replace the seven flat fields with one:

```rust
pub vocab_popup: crate::app::vocab_popup::VocabPopupState,
```

## Verification — RENDER-TIER (user gate required)

**Agent-runnable gates (necessary, not sufficient):**
- `cargo build` — clean (the widget-method errors after the state rewrite are the
  checklist for the second rewrite pass)
- `cargo test --bins` — **413**
- `cargo clippy` — **115**

**Why insufficient:** `vocab_popup` IS the vocab Popover widget — grouping
rewrites every `.show()`/`.hide()`/`.update()` call on it. A wrong widget rewrite
could break the popup's display while still compiling (e.g. if a method call were
mis-targeted). Only a rendered check proves the popup still opens/updates/hides.

**User-run gate (REQUIRED before merge):**
1. **Nav-fuzz** on a work with vocab data (proves no regression in the nav paths
   that touch popup auto-show / fade):
   ```bash
   ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work <ABBR>
   ```
2. **Manual vocab-popup eyeball** — the fuzz may not exercise the popup's full
   open/update/view-toggle path. Launch a work with vocab words, trigger the
   vocab popup (the keybind that calls `open_vocab_popup`), confirm it opens,
   shows the word data, toggles view (Definition/Gloss), and hides — exactly as
   before. The agent states this is blocked for it and asks the user.

If either surfaces a regression → systematic debugging, do NOT merge.

## Risks & mitigations

- **Grouping a vocab-HIGHLIGHT field by mistake** (`vocab_matches` etc.). The
  primary risk. Mitigated by the explicit 7-field membership + the explicit
  flat-stays list; `s.vocab_matches` in the popup module stays unchanged.
- **Missing the bare-widget rewrites** (`s.vocab_popup.hide()` etc.). Mitigated:
  after the state-field rewrite, `cargo build` errors every un-rewritten widget
  site as `no method on VocabPopupState` — work that list to zero.
- **Widget render regression the unit suite misses.** Mitigated by the mandatory
  user gate (nav-fuzz + manual popup eyeball).
- **Wrong init mechanism.** Explicit nested literal capturing the `vocab_popup`
  local (1096) and preserving `VocabView::Definition`; NOT `::default()` (the
  widget field isn't Default-constructible).
- **Drift.** The rewrite is purely access-shape (`vocab_popup_x`→`vocab_popup.x`
  and `vocab_popup.<method>`→`vocab_popup.popup.<method>`); no value/logic edits.
  Drift check gates the review.

## Out of scope

This is the LAST contained cluster. After it: all contained single-file/low-spread
clusters are grouped. The core fields stay flat (permanently), and the
medium-spread clusters (search, mpv/sync, translations, gloss-state, toasts,
gutter) remain deferred — re-evaluate whether any are worth grouping once this
ships, but they are not committed scope.
