# AppState grouping Phase D — echo_overlay cluster

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING field grouping (access-shape only). A contained
cluster of the AppState god-struct grouping project
(`docs/superpowers/specs/2026-06-23-appstate-grouping-design.md`). Follows the
pattern proven by Phase A (`nav_test`, merge ddf20c2) and Phase B (`journal`,
merge 78a2aab). All-`Default` variant (uses `::default()`).

## The cluster

Six flat `AppState` fields holding the echo-overlay display state (the stored
echo links + the navigation index + the work-title map + the source label + the
current turn id/key). Access is in **two files**:
`src/input/actions/echoes.rs` (88 sites) + `src/input/keymap.rs` (3 sites, the
echo-overlay close/reset at lines ~1724–1726). `mod.rs` holds only the struct
def + init. Pure-tier (overlay navigation state; this grouping changes how the
fields are addressed, not what renders).

| flat field | type | → sub-struct field |
|---|---|---|
| `echo_overlay_links` | `Vec<crate::db::queries::StoredEchoLink>` | `links` |
| `echo_overlay_index` | `usize` | `index` |
| `echo_overlay_titles` | `std::collections::HashMap<String, String>` | `titles` |
| `echo_overlay_source` | `String` | `source` |
| `echo_overlay_turn_id` | `Option<i64>` | `turn_id` |
| `echo_overlay_turn_key` | `Option<crate::db::queries::EchoTurnKey>` | `turn_key` |

The `AppState` field is `echo_overlay`, so accesses read `s.echo_overlay.links`
etc. (the field name matching the prefix is correct and fine).

## Init — all-`Default` (`::default()`)

Every flat init value is the type's `Default`:

```
echo_overlay_links: Vec::new()                       // Vec::default()
echo_overlay_index: 0                                 // usize::default()
echo_overlay_titles: std::collections::HashMap::new() // HashMap::default()
echo_overlay_source: String::new()                    // String::default()
echo_overlay_turn_id: None                            // Option::default()
echo_overlay_turn_key: None                           // Option::default()
```

So `EchoOverlayState` derives `Default` and `build_window` inits it with
`echo_overlay: …::EchoOverlayState::default(),` — the Phase A simple form, not
the explicit literal.

## The sub-struct

Define in `src/input/actions/echoes.rs` (its primary consumer). echoes.rs already
imports the element types at line 12
(`use crate::db::queries::{EchoTurnKey, StoredEchoLink};`), so use the bare names:

```rust
/// Grouped state for the echo overlay (the stored echo links for the current
/// turn, the navigation index into them, the work-id→title map, the source
/// label, and the current turn id/key). Was six flat `echo_overlay_*` fields on
/// AppState; grouped per the AppState god-struct decomposition (pure-tier
/// cluster).
#[derive(Default)]
pub struct EchoOverlayState {
    pub links: Vec<StoredEchoLink>,
    pub index: usize,
    pub titles: std::collections::HashMap<String, String>,
    pub source: String,
    pub turn_id: Option<i64>,
    pub turn_key: Option<EchoTurnKey>,
}
```

## AppState change

Replace the six flat fields with one:

```rust
pub echo_overlay: crate::input::actions::echoes::EchoOverlayState,
```

## build_window init change

Replace the six inline inits with one:

```rust
echo_overlay: crate::input::actions::echoes::EchoOverlayState::default(),
```

## Access-site rewrites

Every access, prefix-stripped per the mapping, across **two files**:

- `s.echo_overlay_links` → `s.echo_overlay.links`
- `s.echo_overlay_index` → `s.echo_overlay.index`
- `s.echo_overlay_titles` → `s.echo_overlay.titles`
- `s.echo_overlay_source` → `s.echo_overlay.source`
- `s.echo_overlay_turn_id` → `s.echo_overlay.turn_id`
- `s.echo_overlay_turn_key` → `s.echo_overlay.turn_key`

`src/input/actions/echoes.rs` — 88 sites. `src/input/keymap.rs` — 3 sites
(~1724 `s.echo_overlay_links.clear()`, ~1725 `s.echo_overlay_turn_id = None`,
~1726 `s.echo_overlay_turn_key = None`). Compound forms carry over identically:
`.clear()`, `.push()`, `.len()`, `.is_empty()`, `[index]`, `.get(...)`,
`.insert(...)`, `= None`, `= Some(...)`, `.take()`.

**Do NOT touch** the other echo fields — they are NOT this cluster:
`echo_session`, `echo_add_turn_id`, `echo_picker`, `echo_turns_picker`,
`echo_line_picker`, `echo_keybinds_overlay`, `pending_echo_context`,
`pending_echo_scene_lines`. The rewrite targets the six exact `echo_overlay_*`
field names only (a per-full-name rewrite leaves `echo_session` etc. untouched).

## Verification (pure tier)

- `cargo build` — clean (compiler flags every missed/mistyped site)
- `cargo test --bins` — **413**
- `cargo clippy` — **115**, no new warnings
- **No user nav-fuzz** — pure-state cluster; grouping these fields cannot change
  what renders or what any test asserts.

## Risks & mitigations

- **Drift in the rewrite.** Mitigated: purely `s.echo_overlay_x` →
  `s.echo_overlay.x`, no value/logic edits; drift check gates the review.
- **Two-file rewrite (echoes.rs + keymap.rs).** Mitigated: the keymap.rs sites
  are exactly three (named above); `cargo build` flags any missed site in either
  file. Scope the token rewrite to those two files only — NOT mod.rs (its
  echo_overlay field/init are the 6→1 + `::default()` edits).
- **Touching a non-cluster echo field.** Mitigated by the explicit exclusion list
  and per-full-name rewrite.

## Out of scope

Core fields stay flat; the other contained clusters (`word_cycle`, `page_image`,
`scansion`, `vocab_popup`) are their own sub-projects.
