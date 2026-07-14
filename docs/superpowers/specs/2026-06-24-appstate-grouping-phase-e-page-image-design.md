# AppState grouping Phase E — page_image cluster

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING field grouping (access-shape only). A contained
cluster of the AppState god-struct grouping project
(`docs/superpowers/specs/2026-06-23-appstate-grouping-design.md`). Follows the
pattern proven by Phase A (`nav_test` → `NavTestState`, merge ddf20c2) and Phase B
(`journal` → `JournalState`). All-`Default` variant (uses `::default()`).

## The cluster — UNUSUAL: access is mod.rs-internal

Five flat `AppState` fields for the page-scan image view and the calibration
mode. Unlike journal (whose accesses live in a separate `input/` file), **all 43
real access sites are inside `src/app/mod.rs` itself** — in the image-view /
calibration free functions that stayed in `mod.rs` (`enter_page_calibration`,
`refresh_page_image`, `calibration_show_page`, `calibration_jump_page`,
`toggle_image_view`, etc.). There is **no access in any `input/` file**. So the
entire change — the new sub-struct, the `AppState` field, the `build_window`
init, and the access rewrites — is confined to one file: `src/app/mod.rs`.
Pure-tier (image/calibration state is data the grouping cannot affect; the render
of the image view is driven by these reads, but grouping the fields changes only
how they're addressed, not the values — and there is no rendered-spread invariant
the unit suite can't cover here, see Verification).

| flat field | type | → sub-struct field |
|---|---|---|
| `page_images` | `Vec<crate::db::models::PageImage>` | `images` |
| `image_dir` | `Option<String>` | `dir` |
| `image_mode` | `bool` | `mode` |
| `current_page_order` | `Option<i64>` | `page_order` |
| `calibration_index` | `usize` | `calibration_index` |

Note the `AppState` field is the **singular `page_image`** (not `page_images`),
so the first sub-field reads `state.page_image.images` — no collision, and it
also stays clear of the unrelated `page_image_overlay` field (see Boundaries).

## All-`Default` init variant

Every original init value is the `Default` for its type:

```
page_images: Vec::new()        // Vec::default()
image_dir: None                // Option::default()
image_mode: false              // bool::default()
current_page_order: None       // Option::default()
calibration_index: 0           // usize::default()
```

So `PageImageState` derives `Default` and `build_window` inits it with
`page_image: PageImageState::default(),` — the same simple form as `nav_test`,
not the explicit-literal form journal needed.

## The sub-struct

Because the only consumer is `mod.rs`, define `PageImageState` co-located **in
`src/app/mod.rs`** (near the other small state structs `SearchMatch` /
`VocabMatch`, or just above `AppState`):

```rust
/// Grouped state for the page-scan image view + calibration mode. Was five flat
/// fields on AppState (`page_images`/`image_dir`/`image_mode`/`current_page_order`/
/// `calibration_index`); grouped per the AppState god-struct decomposition
/// (pure-tier cluster). All accesses are mod.rs-internal (the image/calibration
/// free functions).
#[derive(Default)]
pub struct PageImageState {
    pub images: Vec<crate::db::models::PageImage>,
    pub dir: Option<String>,
    pub mode: bool,
    pub page_order: Option<i64>,
    pub calibration_index: usize,
}
```

Since this is defined in `mod.rs` the same file uses the bare `PageImageState`
for the field type and init; no cross-module path is needed.

## AppState change

Replace the five flat fields (`page_images`, `image_dir`, `image_mode`,
`current_page_order`, `calibration_index`) with one:

```rust
pub page_image: PageImageState,
```

## Access-site rewrites (mod.rs only)

Rewrite all 43 sites in `src/app/mod.rs`:

- `state.page_images` → `state.page_image.images`
- `state.image_dir` → `state.page_image.dir`
- `state.image_mode` → `state.page_image.mode`
- `state.current_page_order` → `state.page_image.page_order`
- `state.calibration_index` → `state.page_image.calibration_index`

(and the same for any `s.` receiver form). Compound forms carry over identically:
`state.page_image.images.is_empty()`, `state.page_image.images.len()`,
`state.page_image.page_order = Some(...)`, `state.page_image.calibration_index += 1`,
indexing `state.page_image.images[i]`, etc.

## Boundaries — substrings that must NOT be touched

Three things in `mod.rs` contain `page_image` / `image` as a substring but are
**not** cluster fields. They have 14 occurrences total and must be left exactly
as-is:

- **`page_image_overlay`** — a separate overlay-widget field on `AppState` (the
  PageImageOverlay). Not part of this cluster.
- **`page_image_for_line_id`** — an `impl AppState` method. Its body may *read*
  `self.page_images` → that read becomes `self.page_image.images`, but the
  **method name does not change**.
- **`refresh_page_image`** — a free function. Its **name does not change**; only
  its body's `state.X` accesses to the five fields get rewritten.

The two `db/models.rs` + `db/queries.rs` hits for `image_dir` are **doc-comment
mentions of the `works.image_dir` DB column** — false positives, not AppState
fields. Do not touch them.

## Verification (pure tier)

- `cargo build` — clean (the compiler flags every missed/mistyped site)
- `cargo test --bins` — **413** (proves the rewrite compiles + the suite passes)
- `cargo clippy` — **115**, no new warnings
- **No user nav-fuzz.** The grouping is an access-shape change to data fields read
  by the image/calibration functions; it cannot change the values or the control
  flow, and there is no pagination/spread invariant in scope (the page-image view
  is a separate display path, and this change does not touch its render logic —
  only how the same data is addressed). The compiler-checked rewrite + the 413
  suite are the proof, consistent with the other pure-tier clusters.

## Risks & mitigations

- **Behavioral drift in the rewrite.** Mitigated: the change is purely
  `state.<field>` → `state.page_image.<sub>` (compiler rejects typos), no
  value/logic edits. A drift check (every changed line in `mod.rs`'s
  image/calibration fns is exclusively the token rewrite, except the new struct
  def + the field/init replacement) gates the review.
- **Touching a boundary substring** (`page_image_overlay` /
  `page_image_for_line_id` name / `refresh_page_image` name). Mitigated by the
  explicit boundary list; a blind sed over `mod.rs` is forbidden — rewrite the
  five exact field-access patterns only.
- **mod.rs-internal blast.** All edits are in one file, so there is no
  cross-module repath; `cargo build` fully validates.

## Out of scope

Same as the project spec: the core fields stay flat; the other contained
clusters (`word_cycle`, `echo_overlay`, `scansion`, `vocab_popup`) are their own
sub-projects.
