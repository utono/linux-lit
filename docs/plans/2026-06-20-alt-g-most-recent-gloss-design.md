# Alt+g — Reopen the Most Recently Viewed Gloss

## Summary

Add a reader-mode keybind, `Alt+g`, that reopens the gloss overlay on the
gloss the user last viewed *or created* in the current work, restored to the
exact gloss type that was on screen. The reference is persisted per work across
restarts. A brand-new gloss (created via the visual-mode gloss actions) becomes
the "most recent" the instant it is shown. When the current work has no usable
recorded gloss — never viewed, or the remembered gloss was deleted / the work
re-imported so the citation no longer resolves — `Alt+g` shows a brief
"no recent gloss" toast and does nothing else.

## Requirements

- **Scope:** per-work, persisted across sessions.
- **Precision:** restore the passage AND the specific gloss type that was last
  shown (a passage may hold up to three gloss types:
  `reader-gloss`, `teacher-generic`, `inner-monologue`).
- **Freshness:** a freshly created gloss must immediately become the most
  recent — not only after it is reopened as a saved passage.
- **Empty / stale case:** toast "no recent gloss"; never error, never fall back
  to the picker.
- **Binding:** `Alt+g` in reader mode (currently unbound there).

## Background — relevant existing code

(Confirmed by source inspection; line numbers are anchors, not contracts.)

### The shared overlay open path

`open_gloss_overlay` (`src/input/actions/gloss.rs:1852`) is the canonical
display entry point used by cursor-open (`toggle_overlay`, `gloss.rs:1905`) and
the gloss-picker confirm handler (`src/input/keymap.rs:424`):

```rust
pub(crate) fn open_gloss_overlay(
    s: &mut AppState,
    passages: Vec<GlossedPassage>,   // full passage list for n/p nav
    passage_index: usize,            // index into `passages`
    passage: GlossedPassage,         // the passage being shown
    all_glosses: Vec<SavedGloss>,    // glosses for this passage (>= 1)
    from_picker: bool,               // controls Escape return path
)
```

It currently always displays `all_glosses[0]`. The picker-confirm handler
(`keymap.rs:389-428`) rebuilds its args with `find_glossed_passages`
(`src/db/queries.rs:1643`) + `find_glosses_by_start` (`queries.rs:1588`) — the
same reconstruction Alt+g will use.

### The visual-mode create/cached-open actions

`action_reader_gloss` (`visual.rs:400`, type `reader-gloss`),
`action_gloss_with_claude` (`visual.rs:541`, type `teacher-generic`),
`action_inner_monologue` (`visual.rs:682` + `run_pending_inner_monologue_blocking`
`visual.rs:845`, type `inner-monologue`). These do NOT route through
`open_gloss_overlay`; they display via `gloss_overlay.show_gloss_with_color(...)`
directly. Each has a cached-open branch (synchronous) and a freshly-generated
branch (async `glib::spawn_future_local` continuation after Claude returns).

### The unifying invariant

Every display branch — `open_gloss_overlay` and all six visual-mode sites
(three cached-open, three freshly-generated) — sets `s.gloss_context = Some(ctx)`
immediately after showing the gloss. `GlossContext` (`src/gloss.rs:480`) carries
`work_abbrev` and `start_citation`. This is the single fact the recording helper
relies on, so recording logic lives in one place.

### Data model

- `SavedGloss { gloss_id, passage_id, gloss_text, timestamp, gloss_type }`
  (`queries.rs:1497`). `gloss_type` ∈ {`reader-gloss`, `teacher-generic`,
  `inner-monologue`}.
- `GlossedPassage { passage_id, work_abbrev, start_citation, end_citation, act,
  scene, speaker, source_text }` (`queries.rs:1632`).
- A passage is looked up in practice by `(work_abbrev, start_citation)`.

### Config persistence

`Config` (`src/config.rs`) persists per-work `work_positions:
HashMap<String, usize>` (`config.rs:53`). No gloss value is persisted today.

## Design

### 1. Config: the persisted reference

Add to `Config`:

```rust
/// Per-work most-recently-viewed gloss, keyed by work_abbrev.
#[serde(default)]
pub last_gloss: HashMap<String, LastGloss>,
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LastGloss {
    pub start_citation: String,
    pub gloss_type: String,
}
```

`#[serde(default)]` so existing config files without the key load cleanly. This
mirrors `work_positions`.

### 2. Recording: one helper, called at each display site

Add a method on `AppState` (`src/app.rs`):

```rust
fn record_last_gloss(&mut self, gloss_type: &str) {
    if let Some(ctx) = &self.gloss_context {
        self.config.last_gloss.insert(
            ctx.work_abbrev.clone(),
            LastGloss { start_citation: ctx.start_citation.clone(),
                        gloss_type: gloss_type.to_string() },
        );
        self.config.save(); // same persistence call used elsewhere
    }
}
```

It reads the just-set `gloss_context`, so it must be called *after*
`s.gloss_context = Some(ctx)` at each site. Call it at the seven display sites:

- `open_gloss_overlay` (`gloss.rs:1852`) — after it sets `gloss_context`, with
  the gloss_type of the gloss it actually displayed (see §4).
- `visual.rs` cached-open branches: `~450` (reader-gloss), `~591`
  (teacher-generic), `~732` (inner-monologue).
- `visual.rs` freshly-generated branches: `~526` (reader-gloss), `~667`
  (teacher-generic), `~932` (inner-monologue, in
  `run_pending_inner_monologue_blocking`).

At every site the gloss_type is a known string literal matching the branch, and
`gloss_context` is set, so the call is one line. The freshly-generated branches
run inside the async continuation, so a just-created gloss is recorded the
instant it is shown — satisfying the freshness requirement.

### 3. The Alt+g handler — `Action::OpenLastGloss`

Add `OpenLastGloss` to the `Action` enum (`src/input/actions/mod.rs`) and a
reader-mode handler. The handler:

1. Resolve the current `work_abbrev` from app state.
2. `config.last_gloss.get(work_abbrev)` → clone `(start_citation, gloss_type)`.
   `None` → `toast("no recent gloss")`, return.
3. Load `passages = find_glossed_passages(conn, work_abbrev, ALL_THREE_TYPES)`.
   Find `passage_index` = position of the passage whose `start_citation`
   matches. Not found → `toast("no recent gloss")`, return. **(stale guard)**
4. `all_glosses = find_glosses_by_start(conn, work_abbrev, start_citation,
   ALL_THREE_TYPES)`. Empty → `toast("no recent gloss")`, return.
   **(stale guard)**
5. Set `gloss_return_pos` to the current reader position (as the picker-confirm
   handler does, `keymap.rs:421`).
6. Call `open_gloss_overlay(s, passages, passage_index, passage, all_glosses,
   /*from_picker=*/ false)` requesting the stored `gloss_type` (see §4).

Escape from the overlay returns to the reader (the `from_picker=false` path),
consistent with cursor-open.

### 4. `open_gloss_overlay` lands on a requested gloss type

`open_gloss_overlay` currently hardcodes `all_glosses[0]`. Change it to accept a
desired starting gloss type and select the matching gloss:

- Add a parameter, e.g. `desired_type: Option<&str>`.
- Compute `start_idx = desired_type
    .and_then(|t| all_glosses.iter().position(|g| g.gloss_type == t))
    .unwrap_or(0);`
  (the same `.position()` pattern commit `87ec294` introduced for cached-open).
- Display `all_glosses[start_idx]`, set the overlay position to `start_idx`, and
  record `all_glosses[start_idx].gloss_type` via `record_last_gloss`.

Existing callers (cursor-open, picker-confirm) pass `None` → unchanged behavior
(index 0). The Alt+g handler passes `Some(&gloss_type)`.

### 5. Keybind wiring (the mandatory trio + action + overlay)

Per `CLAUDE.md`, any keybind change touches all of:

- `src/input/actions/mod.rs` — add `Action::OpenLastGloss`.
- `src/input/keymap_config.rs` — add
  `(KeyCombo::alt("g"), Action::OpenLastGloss)` to the reader bindings.
  Reader-mode `Alt+g` is currently free. (Overlay-mode `Alt+g` at
  `keymap.rs:711` opens the picker over the overlay — a different input mode,
  no conflict. The two echo each other intentionally: `Alt+g` = "gloss jump".)
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — add
  `{"key": "g", "alt": true, "action": "OpenLastGloss"}` (else the JSON
  silently overrides the compiled default).
- `src/ui/keybinds_overlay.rs` — add the Alt variant on the `g` key cap and a
  `describe()` arm for `OpenLastGloss` (with its `-> handler — src/path`
  reference). Use the `update-cairo-keybinds-overlay` skill so the exhaustive
  cross-reference catches any blank/wrong detail slot.
- Dispatch `OpenLastGloss` to the handler in the reader-mode action dispatch
  (`keymap.rs` / `gloss.rs`).

### 6. Error / edge handling

- No recorded gloss for the work → toast.
- Recorded passage no longer exists (deleted, or work re-imported and the
  citation changed) → toast (caught at step 3).
- Recorded passage exists but has no glosses → toast (step 4).
- Recorded `gloss_type` no longer present on the passage (e.g. that one type
  was deleted) → fall back to index 0 and show whatever remains (step 4 already
  guarantees ≥1 gloss). This is a soft fallback, not a toast, because a gloss
  *does* exist to show.

## Files touched

- `src/config.rs` — `LastGloss` struct, `last_gloss` field + serde default.
- `src/app.rs` — `record_last_gloss` helper on `AppState`.
- `src/input/visual.rs` — six `record_last_gloss(...)` calls.
- `src/input/actions/gloss.rs` — `desired_type` param + selection in
  `open_gloss_overlay`; the `OpenLastGloss` handler; its `record_last_gloss`
  call.
- `src/input/actions/mod.rs` — `Action::OpenLastGloss`.
- `src/input/keymap_config.rs` — reader-mode `Alt+g` binding.
- `src/input/keymap.rs` — dispatch for `OpenLastGloss` (if reader dispatch
  lives here).
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — JSON binding.
- `src/ui/keybinds_overlay.rs` — cap + `describe()` arm.

## Verification

- `cargo build` — compiles.
- `cargo test --bins` — config serde round-trip for `last_gloss`; the
  gloss-type selection (`position(...).unwrap_or(0)`) extracted as a pure helper
  and unit-tested for: type present (matched index), type absent (index 0),
  empty list guarded upstream.
- **Runtime / visual acceptance is a render check** and must be run by the user
  per the project's headless-verification rule. Ask the user to:
  1. `cargo run`, open a work, create a gloss in visual mode, Escape, press
     `Alt+g` → overlay reopens on that gloss/type.
  2. View a different existing gloss of a different type, Escape, `Alt+g` →
     reopens on the most recent one, correct type.
  3. Restart the app on the same work, `Alt+g` → reopens the remembered gloss
     (persistence).
  4. On a work with no gloss ever viewed, `Alt+g` → "no recent gloss" toast.
  5. Delete the remembered gloss, `Alt+g` → toast (stale guard).

## Out of scope

- Global (cross-work) most-recent gloss. This is per-work by decision.
- A most-recent *list* / history beyond the single last gloss.
- Recording from the gloss popup or any path that does not set
  `gloss_context`.
