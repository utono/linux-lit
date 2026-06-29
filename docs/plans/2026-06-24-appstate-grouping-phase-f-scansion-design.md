# AppState grouping Phase F — scansion cluster

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING field grouping (access-shape only).
**RENDER-TIER** cluster of the AppState god-struct grouping project
(`docs/superpowers/specs/2026-06-23-appstate-grouping-design.md`). The grouped
fields feed the *displayed* scansion overlay, so — unlike the five pure-tier
clusters (nav_test, journal, word_cycle, echo_overlay, page_image) — the unit
suite alone does NOT prove correctness; a **user-run render check is required
before merge**.

## The cluster

Three flat `AppState` fields driving the scansion-marks feature.

| flat field | type | → sub-struct field |
|---|---|---|
| `scansion_label_starts` | `std::collections::HashMap<usize, usize>` | `label_starts` |
| `scansion_level` | `crate::scansion::ScanLevel` | `level` |
| `scansion_data` | `std::collections::HashMap<i64, crate::scansion::LineScansion>` | `data` |

Access: **21 sites across 3 files** — `src/app/mod.rs` (13, in `display_work`'s
buffer-build + the sign-gutter scan-text path), `src/input/keymap.rs` (7, the `s`
scansion-toggle handler), `src/input/navigation.rs` (1, an `!= Off` guard).

**Boundary — do NOT group:** `scansion_label_tag` (`gtk4::TextTag`, a separate
tag field) is NOT part of this cluster. Its name contains `scansion` but it is a
widget handle, not scansion state. Same boundary class as `word_bold_tag` in
Phase C.

## Non-default init → explicit nested literal (the journal variant)

`scansion_level`'s init is `crate::scansion::ScanLevel::Off` (the meaningful
"scansion disabled" initial value), and `ScanLevel`
(`#[derive(Debug, Clone, Copy, PartialEq, Eq)]`, src/scansion.rs:16) has **no
`Default`**. So `ScansionState` uses an **explicit nested literal**, not
`::default()`:

```rust
scansion: ScansionState {
    label_starts: std::collections::HashMap::new(),
    level: crate::scansion::ScanLevel::Off,
    data: std::collections::HashMap::new(),
},
```

No new `Default` impl; `ScansionState` does not derive `Default`. (This is the
same variant journal established.)

## The sub-struct

Define in `src/app/mod.rs`, co-located near the other small structs (the
scansion fields' heaviest consumer is `display_work`, which lives in mod.rs;
keymap.rs/navigation.rs reference the fields via `state.scansion.*`):

```rust
/// Grouped state for the scansion-marks feature (the per-line scansion data,
/// the current display level, and the buffer-line→label-start map). Was three
/// flat `scansion_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (render-tier cluster — see verification).
pub struct ScansionState {
    pub label_starts: std::collections::HashMap<usize, usize>,
    pub level: crate::scansion::ScanLevel,
    pub data: std::collections::HashMap<i64, crate::scansion::LineScansion>,
}
```

## AppState change

Replace the three flat fields with one:

```rust
pub scansion: ScansionState,
```

## Access-site rewrites

Every `s.scansion_label_starts` → `s.scansion.label_starts`, `s.scansion_level`
→ `s.scansion.level`, `s.scansion_data` → `s.scansion.data` (and the
`state.scansion_*` forms), across all 21 sites in mod.rs / keymap.rs /
navigation.rs. Compound forms carry over identically: `s.scansion.data.is_empty()`,
`s.scansion.data.clear()`, `s.scansion.level.next()`, `s.scansion.level.as_str()`,
`s.scansion.level != crate::scansion::ScanLevel::Off`,
`s.scansion.label_starts.get(&line_idx)`, `s.scansion.label_starts.clone()`.

`crate::scansion::ScanLevel` is referenced fully-qualified at every site (no bare
import in keymap.rs/navigation.rs) — leave those qualifications exactly as they
are; only the receiver `s.scansion_x` → `s.scansion.x` changes.

Do **not** touch `scansion_label_tag` or `config.scansion_level` (the latter is a
`Config` field, not an AppState field — `s.config.scansion_level` stays as-is).

## Verification — RENDER-TIER (this differs from the pure-tier clusters)

**Agent-runnable gates (necessary, not sufficient):**
- `cargo build` — clean (the compiler flags every missed site)
- `cargo test --bins` — **413** (proves the rewrite compiles + the suite,
  including `load_scansion_for_work` DB tests, passes)
- `cargo clippy` — **115**

**Why the unit suite is not sufficient here:** `scansion_level`/`data`/
`label_starts` feed `display_work`'s buffer build (the `apply_scansion_marks`
call) and the sign-gutter scan text — i.e. they affect what is *rendered*. A
field-access rewrite bug could mis-render the scansion marks in a way the unit
suite (which has no GTK/Pango measurement) cannot catch.

**User-run gate (REQUIRED before merge) — TWO parts, because the nav-fuzz does
NOT toggle scansion** (scansion is off by default; only the `s` key turns it on,
which the fuzz script does not press):

1. **Standard nav-fuzz** on a verse work — proves no regression in the
   scansion-*off* navigation paths (the `!= Off` guards that every nav touches):
   ```bash
   ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Son
   ```
2. **Manual scansion-ON render check** — the nav-fuzz never enables scansion, so
   this is the part that actually exercises the grouped fields' render path.
   Launch a verse work that HAS scansion data (the scansion DB tests use `TN` =
   Twelfth Night; any Folger verse work with `syllable_scan` rows works), press
   `s` to cycle `scansion_level` on, and confirm the scansion marks render over
   the verse exactly as before the change. The agent will state this is blocked
   for it (cannot launch cage) and ask the user to do it.

If either surfaces a regression, treat as a render-tier defect → systematic
debugging, do NOT merge.

## Risks & mitigations

- **Render regression the unit suite misses.** This is the render-tier risk;
  mitigated by the mandatory two-part user gate above (nav-fuzz + scansion-on
  eyeball).
- **Behavioral drift in the rewrite.** Mitigated: purely `s.scansion_x` →
  `s.scansion.x` (compiler rejects typos), no value/logic edits; drift check
  gates the review.
- **Grouping `scansion_label_tag` or `config.scansion_level` by mistake.**
  Mitigated by the explicit boundary above — only the three named AppState fields
  move; the tag and the Config field stay.
- **Wrong init mechanism.** Explicit nested literal (NOT `::default()`) because
  `ScanLevel::Off` is non-default and `ScanLevel` has no `Default`.

## Out of scope

Same as the project spec: core fields stay flat; `vocab_popup` (the remaining
render-tier cluster) is its own sub-project — and the hardest (8 access files,
holds a real widget, not `Default`-derivable), so it goes last.
