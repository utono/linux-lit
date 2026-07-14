# Shared ListBox picker-nav helper — design

## Goal

Remove the duplicated "select the row at the target index" tail that every
ListBox-based picker repeats in its `move_selection`, via one free helper — with
**zero behavior change**, preserving each picker's distinct clamp / empty-start
rules. This is audit opportunity #6, deliberately scoped to a behavior-preserving
tail extraction (NOT a full Picker trait).

## Why not a single shared `move_selection`

The 17 `move_selection` copies are NOT uniform. They fall into families that
genuinely differ in behavior:

- **ListBox-index family (13 sites — in scope):** all end with the identical tail
  `if let Some(row) = list_box.row_at_index(target) { list_box.select_row(Some(&row)); }`,
  but compute `target` differently:
  - **Variant A — guard + clamp** (`if let Some(current) = selected_row() { (current.index()+delta).max(0) }`, skips entirely when nothing selected): `gloss_picker`, `bookmark_picker`, `concordance_picker`, `media_picker`, `journal_picker`.
  - **Variant B — `unwrap_or(-1)` + clamp** (`(current+delta).max(0)`): `echo_picker`, `echo_turns_picker`.
  - **Variant C — `unwrap_or(-1)` + no clamp** (`current+delta`, relies on `row_at_index` returning None out of range): `echo_line_picker`, `concordance_word_picker`, `concordance_list_picker`, `concordance_works_picker`, `voice_picker`.
  - **Variant D — `unwrap_or(0)` + clamp**: `authorship_picker`.

  A and B differ in the "rows exist but nothing selected" case (A skips; B selects
  index 0 on a forward move). C has no lower clamp. These are real differences —
  a single unified `move_selection` would change behavior at some sites.

- **EXCLUDED — fundamentally different functions:**
  - `action_popup.rs`, `keybinds_overlay.rs`, `settings_overlay.rs` — `rem_euclid`
    wraparound over a `rows: Vec`, not `row_at_index` indexing (settings also skips
    a disabled row; settings/action take `&mut self`). Not the same shape.
  - `library_picker.rs` — extra scroll-into-view (`compute_bounds`/`adjustment`) and
    a `row_at_index` count loop; its tail is not the shared 3-line shape.

## Decision: helper owns ONLY the common tail

The helper owns the one part that is byte-identical across all 13 sites: select
the row at a given index if it exists. Each picker keeps its own index
computation (the part that legitimately differs), so every clamp / empty-start
rule is preserved verbatim at its call site.

Rejected alternative: a richer `move_listbox_selection(&list_box, delta, clamp:
bool)`. It would still NOT unify the empty-start variants (`-1` vs `0` vs
guard-skip), so it would be only partially shared AND would risk encoding the
wrong clamp at a site. Tail-only is the clean, fully behavior-preserving cut.

## Component

A `pub mod picker_nav;` module at `src/ui/picker_nav.rs` (registered in
`src/ui/mod.rs` alongside the other `pub mod <name>;` lines). Pure GTK, no
`AppState`.

```rust
use gtk4::prelude::*;
use gtk4::ListBox;

/// Select the row at `index` in `list_box` if it exists; no-op otherwise.
/// The shared tail of every ListBox picker's `move_selection`: callers compute
/// their own target index (preserving each picker's empty-start and clamp rules)
/// and pass it here. `index < 0` or past the end selects nothing (GTK's
/// `row_at_index` returns None) — the existing behavior at every call site.
pub(crate) fn select_row_at(list_box: &ListBox, index: i32) {
    if let Some(row) = list_box.row_at_index(index) {
        list_box.select_row(Some(&row));
    }
}
```

## Call-site changes (13 sites, all `&self`, behavior-identical)

Each picker's `move_selection` keeps its exact index computation and replaces
ONLY the trailing `if let Some(row) = list_box.row_at_index(<target>) {
list_box.select_row(Some(&row)); }` with
`crate::ui::picker_nav::select_row_at(&self.list_box, <target>);`.

### Variant A — guard + clamp (5 sites)
`gloss_picker.rs` (~146), `bookmark_picker.rs` (~143), `concordance_picker.rs`
(~153), `media_picker.rs` (~161), `journal_picker.rs` (~141):

```rust
pub fn move_selection(&self, delta: i32) {
    if let Some(current) = self.list_box.selected_row() {
        crate::ui::picker_nav::select_row_at(&self.list_box, (current.index() + delta).max(0));
    }
}
```
(The original computed `let idx = current.index(); let new_idx = (idx + delta).max(0);`
then the tail — the inlined `(current.index() + delta).max(0)` is the same value.
If a picker's local style is clearer keeping the `let idx`/`let new_idx` bindings,
that is fine — only the tail must become `select_row_at`.)

### Variant B — `unwrap_or(-1)` + clamp (2 sites)
`echo_picker.rs` (~153), `echo_turns_picker.rs` (~150):

```rust
pub fn move_selection(&self, delta: i32) {
    let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
    crate::ui::picker_nav::select_row_at(&self.list_box, (current + delta).max(0));
}
```

### Variant C — `unwrap_or(-1)` + no clamp (5 sites)
`echo_line_picker.rs` (~84), `concordance_word_picker.rs` (~117),
`concordance_list_picker.rs` (~112), `concordance_works_picker.rs` (~124),
`voice_picker.rs` (~178):

```rust
pub fn move_selection(&self, delta: i32) {
    let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
    crate::ui::picker_nav::select_row_at(&self.list_box, current + delta);
}
```
(`voice_picker` writes the `unwrap_or(-1)` across multiple lines; preserve its
formatting, only the tail changes.)

### Variant D — `unwrap_or(0)` + clamp (1 site)
`authorship_picker.rs` (~77):

```rust
pub fn move_selection(&self, delta: i32) {
    let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(0);
    crate::ui::picker_nav::select_row_at(&self.list_box, (current + delta).max(0));
}
```

## Behavior preservation

The target-index expression at every site is unchanged — only the
select-if-exists tail is delegated to `select_row_at`, which does exactly what
the inlined tail did (`row_at_index(index)` → `select_row` if Some). No clamp is
added or removed; no empty-start rule changes. The reviewer verifies, per site,
that the index expression passed to `select_row_at` equals the originally
computed target.

## Global Constraints

- **No behavior change.** Per-site index computation preserved verbatim; only the
  3-line tail delegates. Reviewer checks each of the 13 sites individually.
- **Do NOT touch** `action_popup.rs`, `keybinds_overlay.rs`, `settings_overlay.rs`,
  `library_picker.rs` — out of scope (different functions).
- **No keybind change** → do NOT touch `keybinds_overlay.rs`, `keymap_config.rs`,
  `keymap.json`.
- New module registered as `pub mod picker_nav;` in `src/ui/mod.rs`.
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): `rg`/`fd` not `grep`/`find`; `\mv -f`/`\cp -f`/
  `command rm -f` for non-interactive overwrite/delete.

## Testing

Selecting a ListBox row needs a realized GTK widget; not exercisable in
`cargo test --bins`, and the helper has no branchable logic beyond the GTK call —
so **no new unit test** (consistent with `ask_card`/`footer`; a fake test would
assert nothing). Verification = build + clippy + `cargo test --bins` green +
reviewer per-site equivalence + the user's cage pass:

- Open several pickers (e.g. Ctrl+p library is excluded; use Ctrl+\\ concordance,
  the media picker, the gloss picker, an echo picker) and navigate with j/k —
  confirm selection moves as before, including at the top edge (clamp behavior)
  and when the list first opens (empty-start behavior).

Per the headless-verification protocol, the agent cannot reliably drive cage on
the live dwl session, so this is handed to the user.

## Out of scope

- A full `Picker` trait unifying the divergent method names (`entry`/`search_entry`,
  `set_items`/`set_words`/`set_voices`, `populate_list`/`filter_changed`) — would
  force unlike pickers under one interface and churn every call site for cosmetic
  naming; the audit itself flagged this as risky. Not done.
- The 4 excluded pickers (action_popup, keybinds_overlay, settings_overlay,
  library_picker).
- This is the last of the 5 audit refactors; no further refactor spec follows.
