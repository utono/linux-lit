# AppState grouping Phase C — word_cycle cluster

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING field grouping (access-shape only). Third
cluster of the AppState god-struct grouping project
(`docs/superpowers/specs/2026-06-23-appstate-grouping-design.md`). Follows the
pattern proven by Phase A (`nav_test` → `NavTestState`, merge ddf20c2) and
Phase B (`journal` → `JournalState`, merge 78a2aab). All-`Default` variant —
uses `::default()` (like Phase A, unlike Phase B's explicit literal).

## The cluster

Five flat `AppState` fields for the word-copy / word-cycle feature (cursor-word
cycling + multi-word phrase collection + the bold-highlight generation counter).
All real access sites in **one file** (`src/input/actions/word_copy.rs`, 20
sites); `mod.rs` holds only the struct def + init. Pure-tier (word selection +
collection state + a generation counter; no render path this grouping changes).

| flat field | type | → sub-struct field |
|---|---|---|
| `word_cycle_line` | `Option<usize>` | `cycle_line` |
| `word_cycle_index` | `usize` | `cycle_index` |
| `word_bold_gen` | `Rc<Cell<u64>>` | `bold_gen` |
| `word_collect_words` | `Vec<String>` | `collect_words` |
| `word_collect_ranges` | `Vec<(usize, usize)>` | `collect_ranges` |

Sub-struct field names strip the leading `word_` from each full flat name (the
cluster mixes `word_cycle_*`, `word_collect_*`, and `word_bold_gen`, so a single
prefix strip is by full name, not by a `word_cycle_` prefix).

## Init variant: all-`Default` → `::default()`

Every flat init value is the type's `Default`:

- `word_cycle_line: None` (`Option` default)
- `word_cycle_index: 0` (`usize` default)
- `word_bold_gen: Rc::new(Cell::new(0))` — this **is** `Rc<Cell<u64>>::default()`
  (`Rc::default()` == `Rc::new(T::default())`, and `Cell<u64>::default()` ==
  `Cell::new(0)`)
- `word_collect_words: Vec::new()` (`Vec` default)
- `word_collect_ranges: Vec::new()` (`Vec` default)

So `WordCycleState` derives `Default` and `build_window` inits it with
`WordCycleState::default()` (the Phase A variant). No explicit literal needed.

## The sub-struct

Define in `src/input/actions/word_copy.rs` (beside its only consumer):

```rust
/// Grouped state for the word-copy / word-cycle feature (cursor-word cycling,
/// multi-word phrase collection, and the bold-highlight generation counter).
/// Was five flat `word_cycle_*` / `word_collect_*` / `word_bold_gen` fields on
/// AppState; grouped per the AppState god-struct decomposition (pure-tier
/// cluster).
#[derive(Default)]
pub struct WordCycleState {
    pub cycle_line: Option<usize>,
    pub cycle_index: usize,
    pub bold_gen: std::rc::Rc<std::cell::Cell<u64>>,
    pub collect_words: Vec<String>,
    pub collect_ranges: Vec<(usize, usize)>,
}
```

(Use whatever `Rc`/`Cell` path `word_copy.rs` already has in scope; if it does
not import them, the fully-qualified `std::rc::Rc<std::cell::Cell<u64>>` above is
correct as written. `cargo build` confirms.)

## AppState change

Replace the five flat fields (`word_cycle_line`, `word_cycle_index`,
`word_bold_gen`, `word_collect_words`, `word_collect_ranges`) with one:

```rust
pub word_cycle: crate::input::actions::word_copy::WordCycleState,
```

## build_window init change

Replace the five inline inits with one line:

```rust
word_cycle: crate::input::actions::word_copy::WordCycleState::default(),
```

(All five initial values are the `Default`, so `::default()` is exact — same as
Phase A's `nav_test`.)

## Access-site rewrites

Every access in `src/input/actions/word_copy.rs` (20 sites), prefix-stripped per
the mapping:

- `state.word_cycle_line` → `state.word_cycle.cycle_line`
- `state.word_cycle_index` → `state.word_cycle.cycle_index`
- `state.word_bold_gen` → `state.word_cycle.bold_gen`
- `state.word_collect_words` → `state.word_cycle.collect_words`
- `state.word_collect_ranges` → `state.word_cycle.collect_ranges`

Compound forms carry over identically: `state.word_cycle.collect_words.clear()`,
`state.word_cycle.collect_words.push(...)`, `state.word_cycle.collect_words.join(" ")`,
`state.word_cycle.collect_ranges.clone()`, `state.word_cycle.bold_gen.get()`,
`state.word_cycle.bold_gen.set(gen)`, `state.word_cycle.bold_gen.clone()`,
`state.word_cycle.cycle_line == Some(state.current_line)`,
`state.word_cycle.cycle_index % words.len()`.

Do **not** touch `word_status_timer`, `word_status_label`, or `word_bold_tag` —
those are separate fields (a toast timer, a status label widget, and a text tag),
NOT part of this cluster. Note in particular `word_bold_gen` (in cluster) is
distinct from `word_bold_tag` (NOT in cluster) — a per-full-name token rewrite of
`word_bold_gen` leaves `word_bold_tag` untouched.

## Verification (pure tier)

- `cargo build` — clean (the compiler flags every missed/mistyped site)
- `cargo test --bins` — **413** (no word_copy unit tests change; this proves the
  rewrite compiles + the suite passes)
- `cargo clippy` — **115**, no new warnings
- **No user nav-fuzz** — word_cycle is a pure-state cluster; grouping its fields
  cannot change what renders or what any test asserts.

## Risks & mitigations

- **Behavioral drift in the rewrite.** Mitigated: the change is purely
  `state.word_*` → `state.word_cycle.*` (compiler rejects typos), no value/logic
  edits. A drift check (every changed line in word_copy.rs is exclusively the
  token rewrite, except the new struct def) gates the review.
- **`word_bold_tag` corruption.** Mitigated: the rewrite targets full field names
  (`word_bold_gen`, not `word_bold`), so `word_bold_tag` is never matched.
- **`::default()` not exact for `Rc<Cell<u64>>`.** Mitigated by the explicit
  derivation note above (`Rc<Cell<u64>>::default()` == `Rc::new(Cell::new(0))`);
  if any doubt, `cargo build` + a one-line check confirms behavior parity.

## Out of scope

Same as the project spec: the core fields stay flat; the other contained
clusters (`page_image`, `echo_overlay`, `scansion`, `vocab_popup`) are their own
sub-projects.
