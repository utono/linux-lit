# AppState grouping Phase B — journal cluster

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING field grouping (access-shape only). Second
cluster of the AppState god-struct grouping project
(`docs/superpowers/specs/2026-06-23-appstate-grouping-design.md`). Follows the
pattern proven by Phase A (`nav_test` → `NavTestState`, merge ddf20c2), with one
documented variant: a non-`Default` init.

## The cluster

Four flat `AppState` fields, all real access sites in **one file**
(`src/input/actions/journal.rs`, 33 sites); `mod.rs` holds only the struct def +
init. Pure-tier (journal pages are data + a return-position; no render path this
grouping can change).

| flat field | type | → sub-struct field |
|---|---|---|
| `journal_pages` | `Vec<crate::db::journal::JournalPage>` | `pages` |
| `journal_page_index` | `usize` | `page_index` |
| `journal_return_pos` | `Option<(usize, usize)>` | `return_pos` |
| `journal_prompt_mode` | `JournalPromptMode` | `prompt_mode` |

## The one variant from Phase A: non-`Default` init

Phase A's `nav_test` init was all-`Default` (`false`/`0`/`None`), so it used
`NavTestState::default()`. Journal's init is **not** all-default:
`journal_prompt_mode: JournalPromptMode::Ask` — and `JournalPromptMode`
(`#[derive(Clone, Copy, Debug, PartialEq, Eq)]`) has **no `Default`**. Per the
project spec's stated fallback, `JournalState` is therefore initialized with an
**explicit nested literal**, not `::default()`:

```rust
journal: crate::input::actions::journal::JournalState {
    pages: Vec::new(),
    page_index: 0,
    return_pos: None,
    prompt_mode: JournalPromptMode::Ask,
},
```

This adds **no new `Default` impl** anywhere (no crate-wide
`JournalPromptMode::default()` is introduced) — the meaningful `Ask` value stays
visible at the construction site. This is the precedent for every later cluster
whose init isn't all-default.

`JournalState` itself does **not** derive `Default` (it would be unused, and one
of its fields can't supply a sensible default). No derive.

## The sub-struct

Define in `src/input/actions/journal.rs` (beside its only consumer):

`journal.rs` already imports `JournalPromptMode` at line 1
(`use crate::app::{AppState, InputMode, JournalBand, JournalPromptMode};`), so the
sub-struct uses the bare `JournalPromptMode` (already in scope):

```rust
/// Grouped state for the journal feature (band pages + viewer index + the
/// return-to-reader position + the add/edit prompt mode). Was four flat
/// `journal_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (pure-tier cluster).
pub struct JournalState {
    pub pages: Vec<crate::db::journal::JournalPage>,
    pub page_index: usize,
    pub return_pos: Option<(usize, usize)>,
    pub prompt_mode: JournalPromptMode,
}
```

In `mod.rs`'s `AppState` field and `build_window` init, the type is the
fully-qualified `crate::input::actions::journal::JournalState` (mod.rs does not
import the journal module's items by bare name).

## AppState change

Replace the four flat fields (`journal_pages`, `journal_page_index`,
`journal_return_pos`, `journal_prompt_mode`) with one:

```rust
pub journal: crate::input::actions::journal::JournalState,
```

## Access-site rewrites

Every `s.journal_pages` → `s.journal.pages`, `s.journal_page_index` →
`s.journal.page_index`, `s.journal_return_pos` → `s.journal.return_pos`,
`s.journal_prompt_mode` → `s.journal.prompt_mode`, across the 33 sites in
`src/input/actions/journal.rs` ONLY. Compound forms carry over identically:
`s.journal.return_pos.take()`, `s.journal.page_index -= 1`,
`s.journal.pages.is_empty()`, `s.journal.pages[s.journal.page_index].id`, etc.

Do **not** touch `journal_overlay`, `journal_picker`, `journal_band` — those are
separate fields (overlay widget / picker / band selector), NOT part of this
cluster. The grouping is exactly the four `journal_*` fields above.

## Verification (pure tier)

- `cargo build` — clean (the compiler flags every missed/mistyped site)
- `cargo test --bins` — **413** (no journal unit tests change; this proves the
  rewrite compiles + the suite passes)
- `cargo clippy` — **115**, no new warnings
- **No user nav-fuzz** — journal is a pure-state cluster; grouping its fields
  cannot change what renders or what any test asserts.

## Risks & mitigations

- **Behavioral drift in the rewrite.** Mitigated: the change is purely
  `s.journal_x` → `s.journal.x` (compiler rejects typos), no value/logic edits;
  and the explicit nested literal preserves the exact `JournalPromptMode::Ask`
  init. A drift check (every changed line in journal.rs is exclusively the token
  rewrite, except the new struct def) gates the review.
- **Wrong type path on `prompt_mode`.** Mitigated by reusing journal.rs's
  existing `JournalPromptMode` path; `cargo build` confirms.
- **Touching the wrong `journal_*` field** (overlay/picker/band). Mitigated by
  the explicit four-field membership above.

## Out of scope

Same as the project spec: the core fields stay flat; the other contained
clusters (`page_image`, `word_cycle`, `echo_overlay`, `scansion`, `vocab_popup`)
are their own sub-projects.
