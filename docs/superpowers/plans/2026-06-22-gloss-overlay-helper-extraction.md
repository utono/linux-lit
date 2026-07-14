# Gloss Overlay Helper Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move ~1,125 lines of pure helper functions (block model, OP-IPA processing, geometry/citation) plus ~750 lines of their tests out of the 3,606-line `src/ui/gloss_overlay.rs` into three focused sibling modules, leaving the file at ~1,730 lines of widget + rendering code.

**Architecture:** Pure code motion — no logic changes. Three new flat modules of pure functions, each carrying its own `#[cfg(test)]` tests. The GTK-touching buffer-population functions stay in `gloss_overlay.rs`. The only cross-module helper edge is `gloss_block::gloss_blocks → gloss_ipa::strip_ipa`. External call sites in `input/actions/` are updated to the new module paths (no re-export facade).

**Tech Stack:** Rust, GTK4 (`gtk4` crate), `cargo` for build/test/clippy.

## Global Constraints

- **Behavior-preserving.** No function body changes. Move verbatim; only adjust `pub`/`pub(crate)`/`pub(super)`/private visibility and add `use` imports.
- **Test count is the invariant.** Baseline is **413 passing tests** (`cargo test --bins`). After every task the count must stay **413** (no test dropped or duplicated by the moves).
- **The GTK buffer-population code stays in `gloss_overlay.rs`:** `populate_gloss_buffer`, `populate_gloss_buffer_ex`, `apply_bracket_styling`, `line_is_speaker`, and the private structs `BarRange`, `LineNumber`, `BlockRange`. Never move these.
- **Scope is `src/ui/` + two call-site files** (`src/input/actions/gloss.rs`, `src/input/actions/settings.rs`). No `app.rs` / `AppState` changes.
- **Module dependency graph after extraction:** `gloss_ipa` (leaf), `gloss_util` (leaf), `gloss_block → gloss_ipa::strip_ipa`, `gloss_overlay → all three`.
- **Verification per task:** `cargo build`, `cargo test --bins`, `cargo clippy` — no e2e/cage run (pure logic, rendering path untouched).
- **Commit conventions:** end commit messages with the two trailer lines used in this repo (`Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` and the `Claude-Session:` line). Branch off `master` first (see Task 0).

---

### Task 0: Create the working branch

**Files:** none (git only)

- [ ] **Step 1: Confirm clean tree on master and baseline tests**

Run:
```bash
cd ~/utono/linux-lit && git status --short && git branch --show-current
cargo test --bins 2>&1 | rg 'test result' | tail -1
```
Expected: clean tree, branch `master`, `test result: ok. 413 passed`.

- [ ] **Step 2: Create the feature branch**

Run:
```bash
git checkout -b refactor/gloss-overlay-helper-extraction
```
Expected: `Switched to a new branch 'refactor/gloss-overlay-helper-extraction'`.

---

### Task 1: Extract `gloss_util.rs` (leaf — geometry, color, citation)

Start with the leaf module that has **no external callers and no cross-module deps** — lowest risk, establishes the move-and-reimport pattern.

**Files:**
- Create: `src/ui/gloss_util.rs`
- Modify: `src/ui/gloss_overlay.rs` (remove the moved items; add a `use` import)
- Modify: `src/ui/mod.rs` (add `pub mod gloss_util;`)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces (all `pub(super)` so `gloss_overlay`'s impl can call them):
  - `struct CursorScrollGeom` (fields unchanged from the original)
  - `fn cursor_scroll_target(g: &CursorScrollGeom) -> Option<f64>`
  - `fn snap_up_to_row(target_y: f64, row_tops: &[f64], lower: f64, max_value: f64) -> f64`
  - `fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64)>`
  - `fn build_diff_markup(original: &str, corrected: &str, is_original: bool) -> String`
  - `fn split_echo(text: &str) -> Option<(String, String)>`
  - `fn parse_citation(c: &str) -> Option<(&str, &str, &str, &str)>`
  - `fn format_citation_range(start: &str, end: &str) -> Option<String>`

- [ ] **Step 1: Create `src/ui/gloss_util.rs` with the moved functions + tests**

Cut these items **verbatim** from `gloss_overlay.rs` into the new file, changing only their visibility to `pub(super)` (they are currently private `fn`/`struct`):

From `gloss_overlay.rs`, move the bodies of:
- `struct CursorScrollGeom` (currently line ~2725)
- `fn cursor_scroll_target` (~2749)
- `fn snap_up_to_row` (~2793)
- `fn parse_hex_color` (~2803)
- `fn build_diff_markup` (~2814)
- `fn split_echo` (~2699)
- `fn parse_citation` (~3518)
- `fn format_citation_range` (~3539)

And move these whole test modules (verbatim, including `#[cfg(test)]`):
- `mod snap_up_tests` (~2993–3040)
- `mod cursor_scroll_tests` (~3041–3146)
- `mod citation_range_tests` (~3568–end)

The new file's skeleton (fill the bodies from the originals — do NOT rewrite them):

```rust
//! Pure geometry, color, and citation helpers extracted from `gloss_overlay`.
//! No GTK dependencies; all functions are pure and `pub(super)` for the
//! overlay's impl to call.

pub(super) struct CursorScrollGeom {
    // ...fields copied verbatim from the original struct...
}

pub(super) fn cursor_scroll_target(g: &CursorScrollGeom) -> Option<f64> {
    // ...body copied verbatim...
}

pub(super) fn snap_up_to_row(target_y: f64, row_tops: &[f64], lower: f64, max_value: f64) -> f64 {
    // ...body copied verbatim...
}

pub(super) fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64)> {
    // ...body copied verbatim...
}

pub(super) fn build_diff_markup(original: &str, corrected: &str, is_original: bool) -> String {
    // ...body copied verbatim...
}

pub(super) fn split_echo(text: &str) -> Option<(String, String)> {
    // ...body copied verbatim...
}

pub(super) fn parse_citation(c: &str) -> Option<(&str, &str, &str, &str)> {
    // ...body copied verbatim...
}

pub(super) fn format_citation_range(start: &str, end: &str) -> Option<String> {
    // ...body copied verbatim...
}

#[cfg(test)]
mod snap_up_tests {
    use super::*;
    // ...tests copied verbatim...
}

#[cfg(test)]
mod cursor_scroll_tests {
    use super::*;
    // ...tests copied verbatim...
}

#[cfg(test)]
mod citation_range_tests {
    use super::*;
    // ...tests copied verbatim...
}
```

- [ ] **Step 2: Register the module**

In `src/ui/mod.rs`, add the line in alphabetical position near the other `gloss_*` mods:

```rust
pub mod gloss_util;
```

- [ ] **Step 3: Import the moved fns in `gloss_overlay.rs`**

At the top of `src/ui/gloss_overlay.rs`, after the existing `use` block, add:

```rust
use crate::ui::gloss_util::{
    build_diff_markup, cursor_scroll_target, format_citation_range, parse_citation,
    parse_hex_color, snap_up_to_row, split_echo, CursorScrollGeom,
};
```

(Confirm the originals are now fully removed from `gloss_overlay.rs` — no leftover definitions.)

- [ ] **Step 4: Build**

Run:
```bash
cargo build 2>&1 | tail -20
```
Expected: clean build. If a `pub(super)` item is "private" to a caller, that means a call lives outside `gloss_overlay`'s module — re-check; for this task all callers are in `gloss_overlay`, so `pub(super)` suffices.

- [ ] **Step 5: Test — count must stay 413**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result' | tail -1
```
Expected: `test result: ok. 413 passed; 0 failed; 1 ignored`. If the count dropped, a test module didn't move cleanly; if it rose, a module was duplicated.

- [ ] **Step 6: Clippy**

Run:
```bash
cargo clippy --bins 2>&1 | rg -c 'warning' || echo "no warnings"
```
Expected: no new warnings. Watch for "unused import" in `gloss_overlay.rs` (remove any import left dangling by the move) and "function is never used" (means a fn had no caller — shouldn't happen here).

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_util.rs src/ui/gloss_overlay.rs src/ui/mod.rs
git commit -m "$(cat <<'EOF'
refactor(gloss): extract geometry/color/citation helpers to gloss_util

Pure code motion of cursor_scroll_target, snap_up_to_row, parse_hex_color,
build_diff_markup, split_echo, parse_citation, format_citation_range (+ their
three test modules) out of the 3606-line gloss_overlay.rs. pub(super), no
external callers. Test count unchanged at 413.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 2: Extract `gloss_ipa.rs` (leaf — OP-IPA / bracket markup)

The second leaf. Has external callers in `gloss.rs` (so keeps `pub(crate)` items) and a **mislabeled test split** to handle: the IPA tests currently live inside `synopsis_label_tests`.

**Files:**
- Create: `src/ui/gloss_ipa.rs`
- Modify: `src/ui/gloss_overlay.rs` (remove moved items + the IPA half of `synopsis_label_tests`; add `use`)
- Modify: `src/ui/mod.rs` (add `pub mod gloss_ipa;`)
- Modify: `src/input/actions/gloss.rs` (repath the three externally-used fns)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces:
  - `pub(crate) fn ipa_for_tts(text: &str) -> String`
  - `pub(crate) fn contains_ipa_span(s: &str) -> bool`
  - `pub(crate) fn replace_word_ipa(text: &str, word: &str, new_ipa: &str) -> Option<String>`
  - `pub(crate) fn replace_word_ipa_in_source_block(...) -> Option<String>` (signature copied verbatim — it spans several lines starting ~2608)
  - `pub(crate) fn strip_ipa(text: &str) -> String` — **must be `pub(crate)`** because Task 3's `gloss_block::gloss_blocks` will call it cross-module
  - private: `fn strip_brackets`, `fn normalize_ipa_whitespace`, `fn is_ipa_span`, `fn opener_on_boundary`

- [ ] **Step 1: Create `src/ui/gloss_ipa.rs` with the moved functions**

Cut verbatim from `gloss_overlay.rs`:
- `fn strip_brackets` (~2337) → keep private
- `fn is_ipa_span` (~2380) → keep private
- `fn opener_on_boundary` (~2390) → keep private
- `fn strip_ipa` (~2397) → **change to `pub(crate)`**
- `fn normalize_ipa_whitespace` (~2424) → keep private
- `fn ipa_for_tts` (~2460) → already `pub(crate)`, keep
- `fn contains_ipa_span` (~2518) → already `pub(crate)`, keep
- `fn replace_word_ipa` (~2545) → already `pub(crate)`, keep
- `fn replace_word_ipa_in_source_block` (~2608) → already `pub(crate)`, keep

Skeleton:

```rust
//! OP-IPA / bracket markup processing extracted from `gloss_overlay`.
//! Pure string transforms. `pub(crate)` items are consumed by
//! `input::actions::gloss`; `strip_ipa` is also called by `gloss_block`.

pub(crate) fn ipa_for_tts(text: &str) -> String { /* verbatim */ }
pub(crate) fn contains_ipa_span(s: &str) -> bool { /* verbatim */ }
pub(crate) fn replace_word_ipa(text: &str, word: &str, new_ipa: &str) -> Option<String> { /* verbatim */ }
pub(crate) fn replace_word_ipa_in_source_block(/* verbatim params */) -> Option<String> { /* verbatim */ }
pub(crate) fn strip_ipa(text: &str) -> String { /* verbatim */ }

fn strip_brackets(text: &str) -> String { /* verbatim */ }
fn normalize_ipa_whitespace(text: &str) -> String { /* verbatim */ }
fn is_ipa_span(inner: &[char], opener_on_boundary: bool) -> bool { /* verbatim */ }
fn opener_on_boundary(chars: &[char], slash_idx: usize) -> bool { /* verbatim */ }
```

- [ ] **Step 2: Move the IPA tests out of the mislabeled `synopsis_label_tests`**

The module `synopsis_label_tests` (~3148–3430) contains BOTH label tests and IPA tests. Move only the **IPA tests** here — every test fn from `strip_ipa_removes_tagged_words` onward (the contiguous block ~lines 3181–3424):

`strip_ipa_removes_tagged_words`, `strip_ipa_keeps_literal_slash`, `strip_ipa_strips_all_ascii_span`, `strip_ipa_all_ascii_span_does_not_eat_following_text`, `strip_ipa_all_ascii_span_does_not_collapse_newlines`, `strip_ipa_no_tags_is_identity`, `strip_ipa_handles_stress_marks`, `strip_ipa_removes_leaked_prose_ipa`, `strip_ipa_no_space_before_punctuation`, `strip_ipa_real_verse_line_single_spaced`, `ipa_for_tts_replaces_word_with_its_ipa`, `ipa_for_tts_all_ascii_span_replaces_its_word`, `ipa_for_tts_keeps_untagged_words`, `ipa_for_tts_before_punctuation`, `ipa_for_tts_ipa_at_line_start_is_kept`, `ipa_for_tts_keeps_literal_slash_and_plain`, `ipa_for_tts_adjacent_spans_dont_eat_each_other`, `contains_ipa_span_detects_real_ipa`, `replace_word_ipa_swaps_the_words_ipa`, `replace_word_ipa_all_occurrences`, `replace_word_ipa_is_whole_word`, `replace_word_ipa_word_without_following_ipa_is_none`, `replace_word_ipa_case_insensitive_word_match`, `replace_in_source_block_rewrites_multiline_verse`, `replace_in_source_block_none_when_word_absent`, `replace_in_source_block_scopes_to_the_block`, `replace_in_source_block_distinguishes_identical_lines_across_blocks`.

Leave the first 3 tests (`bolds_standalone_label_paragraph`, `does_not_bold_running_prose`, `plain_text_synopsis_has_no_labels`) in `gloss_overlay.rs` for now — Task 3 moves them to `gloss_block.rs`. (Do NOT delete the `synopsis_label_tests` module yet; it still holds those 3.)

Append to `gloss_ipa.rs`:

```rust
#[cfg(test)]
mod ipa_tests {
    use super::*;
    // ...the 27 IPA test fns copied verbatim...
}
```

- [ ] **Step 3: Register the module**

In `src/ui/mod.rs`:

```rust
pub mod gloss_ipa;
```

- [ ] **Step 4: Import the impl-side fns in `gloss_overlay.rs`**

`gloss_overlay`'s impl calls `strip_ipa` (in `populate_gloss_buffer_ex`) and may reference `ipa_for_tts`/`contains_ipa_span`. Add to the top `use` block:

```rust
use crate::ui::gloss_ipa::strip_ipa;
```

(Add `ipa_for_tts`, `contains_ipa_span`, etc. to this `use` only if `gloss_overlay`'s own code — not `gloss.rs` — still calls them. Verify with `rg -n 'ipa_for_tts|contains_ipa_span' src/ui/gloss_overlay.rs`; import only what remains referenced.)

- [ ] **Step 5: Repath the external call sites in `gloss.rs`**

In `src/input/actions/gloss.rs`, change every `crate::ui::gloss_overlay::` reference for the IPA fns to `crate::ui::gloss_ipa::`. Affected symbols: `ipa_for_tts`, `contains_ipa_span`, `replace_word_ipa_in_source_block`. Find them with:

```bash
rg -n 'gloss_overlay::(ipa_for_tts|contains_ipa_span|replace_word_ipa)' src/input/actions/gloss.rs
```

Replace `gloss_overlay::` with `gloss_ipa::` on each of those lines (the function names are unchanged). Example: `crate::ui::gloss_overlay::ipa_for_tts(&text)` → `crate::ui::gloss_ipa::ipa_for_tts(&text)`.

- [ ] **Step 6: Build**

Run:
```bash
cargo build 2>&1 | tail -20
```
Expected: clean. A "private function `strip_ipa`" error means it wasn't bumped to `pub(crate)` — fix in `gloss_ipa.rs`.

- [ ] **Step 7: Test — count must stay 413**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result' | tail -1
```
Expected: `413 passed`. (The 27 moved IPA tests + 3 still-in-place label tests = same total.)

- [ ] **Step 8: Clippy**

Run:
```bash
cargo clippy --bins 2>&1 | rg -c 'warning' || echo "no warnings"
```
Expected: no new warnings. Remove any now-unused imports in `gloss_overlay.rs`.

- [ ] **Step 9: Commit**

```bash
git add src/ui/gloss_ipa.rs src/ui/gloss_overlay.rs src/ui/mod.rs src/input/actions/gloss.rs
git commit -m "$(cat <<'EOF'
refactor(gloss): extract OP-IPA markup helpers to gloss_ipa

Move ipa_for_tts/contains_ipa_span/replace_word_ipa(_in_source_block)/strip_ipa
and the private bracket/whitespace helpers (+ the IPA tests split out of the
mislabeled synopsis_label_tests module) into gloss_ipa.rs. strip_ipa is
pub(crate) for Task 3's cross-module use. gloss.rs call sites repathed.
Test count unchanged at 413.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 3: Extract `gloss_block.rs` (block model — depends on `gloss_ipa::strip_ipa`)

The largest cluster and widest external surface. Depends on Task 2's `gloss_ipa::strip_ipa`. Completes the `synopsis_label_tests` split by moving the remaining 3 label tests here.

**Files:**
- Create: `src/ui/gloss_block.rs`
- Modify: `src/ui/gloss_overlay.rs` (remove moved items + the now-empty `synopsis_label_tests`; add `use`)
- Modify: `src/ui/mod.rs` (add `pub mod gloss_block;`)
- Modify: `src/input/actions/gloss.rs` (repath block fns)
- Modify: `src/input/actions/settings.rs` (repath `BlockKind`)

**Interfaces:**
- Consumes: `crate::ui::gloss_ipa::strip_ipa` (Task 2).
- Produces:
  - `pub enum BlockKind` (variants verbatim)
  - `pub struct GlossBlock` (fields verbatim)
  - `pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock>`
  - `pub fn synopsis_blocks(synopsis: &str) -> Vec<GlossBlock>`
  - `pub fn visual_block_range(anchor: usize, cursor: usize) -> (usize, usize)`
  - `pub fn selected_blocks_text(synopsis: &str, start: usize, end: usize) -> String`
  - `pub fn render_synopsis_with_labels(synopsis: &str) -> (String, Vec<(usize, usize)>)`
  - private: `enum GlossElement`, `fn parse_gloss_tags`, `fn carry_forward_block_speakers`, `fn try_extract`, `fn is_label_paragraph`

- [ ] **Step 1: Create `src/ui/gloss_block.rs` with the moved items**

Cut verbatim from `gloss_overlay.rs`:
- `enum GlossElement` (~1731) → private
- `fn is_label_paragraph` (~1743) → private
- `fn render_synopsis_with_labels` (~1756) → keep `pub`
- `enum BlockKind` (~1794) → keep `pub`
- `struct GlossBlock` (~1800) → keep `pub`
- `fn visual_block_range` (~1818) → keep `pub`
- `fn selected_blocks_text` (~1827) → keep `pub`
- `fn synopsis_blocks` (~1848) → keep `pub`
- `fn gloss_blocks` (~1913) → keep `pub`
- `fn parse_gloss_tags` (~1964) → private
- `fn carry_forward_block_speakers` (~2005) → private
- `fn try_extract` (~2034) → private

**Do NOT move** `fn line_is_speaker` (~1896) — it takes `&gtk4::TextBuffer`, is called only at line 524 inside the impl, and stays in `gloss_overlay.rs`. It is physically interleaved between `synopsis_blocks` and `gloss_blocks`; skip over it when cutting.

Skeleton (`gloss_blocks` calls `strip_ipa` — import it):

```rust
//! Gloss/synopsis block model and text parsing extracted from `gloss_overlay`.
//! Pure (no GTK). Depends on `gloss_ipa::strip_ipa` for Source-block display.

use crate::ui::gloss_ipa::strip_ipa;

#[derive(/* verbatim derives */)]
pub enum BlockKind { /* verbatim */ }

#[derive(/* verbatim derives */)]
pub struct GlossBlock { /* verbatim fields */ }

enum GlossElement { /* verbatim */ }

pub fn render_synopsis_with_labels(synopsis: &str) -> (String, Vec<(usize, usize)>) { /* verbatim */ }
pub fn visual_block_range(anchor: usize, cursor: usize) -> (usize, usize) { /* verbatim */ }
pub fn selected_blocks_text(synopsis: &str, start: usize, end: usize) -> String { /* verbatim */ }
pub fn synopsis_blocks(synopsis: &str) -> Vec<GlossBlock> { /* verbatim */ }
pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock> { /* verbatim — calls strip_ipa */ }

fn is_label_paragraph(p: &str) -> bool { /* verbatim */ }
fn parse_gloss_tags(gloss: &str) -> Vec<GlossElement> { /* verbatim */ }
fn carry_forward_block_speakers(elements: Vec<GlossElement>) -> Vec<GlossElement> { /* verbatim */ }
fn try_extract<'a>(s: &'a str, tag: &str) -> Option<(&'a str, &'a str)> { /* verbatim */ }
```

- [ ] **Step 2: Move the test modules + finish the label-test split**

Move these whole test modules verbatim into `gloss_block.rs`:
- `mod block_tests` (~2856–2992)
- `mod synopsis_blocks_tests` (~3432–3465)
- `mod visual_range_tests` (~3467–3517)

And move the **3 remaining label tests** still inside `synopsis_label_tests` (left there by Task 2): `bolds_standalone_label_paragraph`, `does_not_bold_running_prose`, `plain_text_synopsis_has_no_labels`. Put them into a `mod label_tests` here:

```rust
#[cfg(test)]
mod block_tests { use super::*; /* verbatim */ }

#[cfg(test)]
mod synopsis_blocks_tests { use super::*; /* verbatim */ }

#[cfg(test)]
mod visual_range_tests { use super::*; /* verbatim */ }

#[cfg(test)]
mod label_tests {
    use super::*;
    // the 3 render_synopsis_with_labels tests, verbatim
}
```

After this, the `synopsis_label_tests` module in `gloss_overlay.rs` is **empty** — delete the now-empty `#[cfg(test)] mod synopsis_label_tests { ... }` shell entirely.

- [ ] **Step 3: Register the module**

In `src/ui/mod.rs`:

```rust
pub mod gloss_block;
```

- [ ] **Step 4: Import the block fns/types in `gloss_overlay.rs`**

The impl calls `gloss_blocks`, `synopsis_blocks`, `visual_block_range`, `selected_blocks_text`, `render_synopsis_with_labels` and references `BlockKind`/`GlossBlock`. Add to the top `use` block:

```rust
use crate::ui::gloss_block::{
    gloss_blocks, render_synopsis_with_labels, selected_blocks_text, synopsis_blocks,
    visual_block_range, BlockKind, GlossBlock,
};
```

(Import only the names actually still referenced in `gloss_overlay.rs` — verify each with `rg`; drop any unused to avoid a clippy warning. In particular check whether `GlossBlock`/`BlockKind` are named in the staying code.)

- [ ] **Step 5: Repath external call sites**

In `src/input/actions/gloss.rs`, change the block-fn references from `gloss_overlay::` to `gloss_block::`:

```bash
rg -n 'gloss_overlay::(gloss_blocks|synopsis_blocks|visual_block_range|selected_blocks_text|render_synopsis_with_labels|GlossBlock|BlockKind)' src/input/actions/gloss.rs
```

Replace `gloss_overlay::` with `gloss_block::` on each. Also update the `use crate::ui::gloss_overlay::BlockKind;` at the top of `gloss.rs` (line ~7) to `use crate::ui::gloss_block::BlockKind;`.

In `src/input/actions/settings.rs`, change line ~195's `crate::ui::gloss_overlay::BlockKind::Source` to `crate::ui::gloss_block::BlockKind::Source`:

```bash
rg -n 'gloss_overlay::BlockKind' src/input/actions/settings.rs
```

- [ ] **Step 6: Build**

Run:
```bash
cargo build 2>&1 | tail -20
```
Expected: clean. Errors to expect-and-fix: a missed `gloss_overlay::` repath (private/unresolved path), or `strip_ipa` not imported in `gloss_block.rs`.

- [ ] **Step 7: Test — count must stay 413**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result' | tail -1
```
Expected: `413 passed`. This is the proof the `synopsis_label_tests` split lost nothing: 3 label tests + 27 IPA tests (moved in Task 2) + the three whole block-test modules all still run.

- [ ] **Step 8: Clippy + confirm no stray `gloss_overlay::` helper paths remain**

Run:
```bash
cargo clippy --bins 2>&1 | rg -c 'warning' || echo "no warnings"
rg -n 'gloss_overlay::(gloss_blocks|synopsis_blocks|visual_block_range|selected_blocks_text|render_synopsis_with_labels|ipa_for_tts|contains_ipa_span|replace_word_ipa|GlossBlock|BlockKind)' src || echo "no stale external paths"
```
Expected: no new warnings; `no stale external paths`.

- [ ] **Step 9: Commit**

```bash
git add src/ui/gloss_block.rs src/ui/gloss_overlay.rs src/ui/mod.rs src/input/actions/gloss.rs src/input/actions/settings.rs
git commit -m "$(cat <<'EOF'
refactor(gloss): extract block model + parsing to gloss_block

Move GlossBlock/BlockKind/gloss_blocks/synopsis_blocks/visual_block_range/
selected_blocks_text/render_synopsis_with_labels and their private parsers
into gloss_block.rs (depends on gloss_ipa::strip_ipa). Completes the
synopsis_label_tests split (3 label tests -> gloss_block). gloss.rs and
settings.rs call sites repathed. gloss_overlay.rs now ~1730 lines.
Test count unchanged at 413.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4: Final verification and merge to master

**Files:** none (verification + git)

- [ ] **Step 1: Confirm the file shrank and the buffer-population code stayed**

Run:
```bash
wc -l src/ui/gloss_overlay.rs src/ui/gloss_block.rs src/ui/gloss_ipa.rs src/ui/gloss_util.rs
rg -n 'fn (populate_gloss_buffer|populate_gloss_buffer_ex|apply_bracket_styling|line_is_speaker)\b' src/ui/gloss_overlay.rs
rg -n '\b(BarRange|LineNumber|BlockRange)\b' src/ui/gloss_overlay.rs | head -3
```
Expected: `gloss_overlay.rs` ~1,730 lines; all four GTK fns and the three private structs still present in `gloss_overlay.rs`.

- [ ] **Step 2: Full test + clippy gate**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result' | tail -1
cargo clippy --bins 2>&1 | rg -c 'warning' || echo "no warnings"
```
Expected: `413 passed`; no new warnings.

- [ ] **Step 3: Merge to master and push (per CLAUDE.md "Finishing a Branch")**

Run:
```bash
git checkout master
git merge --no-ff refactor/gloss-overlay-helper-extraction
cargo test --bins 2>&1 | rg 'test result' | tail -1
git push origin master
git branch -d refactor/gloss-overlay-helper-extraction
```
Expected: merge succeeds, post-merge tests `413 passed`, push succeeds, branch deleted.

---

## Self-Review

**Spec coverage** — every spec section maps to a task:
- `gloss_block.rs` module → Task 3 ✓
- `gloss_ipa.rs` module → Task 2 ✓
- `gloss_util.rs` module → Task 1 ✓
- "What stays in gloss_overlay.rs" (buffer-population + private structs) → enforced in Task 3 Step 1 and Task 4 Step 1 ✓
- Test-module split (mislabeled `synopsis_label_tests`) → Task 2 Step 2 (IPA half) + Task 3 Step 2 (label half) ✓
- Module dependency note (`strip_ipa` `pub(crate)`, `gloss_block → gloss_ipa`) → Task 2 (visibility) + Task 3 Step 1 (import) ✓
- Update call sites, no facade → Task 2 Step 5 (`gloss.rs` IPA), Task 3 Step 5 (`gloss.rs` block + `settings.rs`) ✓
- Verification (build/test --bins/clippy, no e2e) → every task ✓
- Risk: test count invariant → baseline 413 asserted in every task ✓

**Placeholder scan** — the `/* verbatim */` markers are intentional: this is pure code motion, so the "implementation" is literally the existing function body cut from a known line range. Each is pinned to an exact source line number and symbol name, not a vague "implement later." No `TODO`/`TBD`/"handle edge cases" placeholders.

**Type consistency** — `BlockKind`, `GlossBlock`, `gloss_blocks`, `synopsis_blocks`, `strip_ipa`, `ipa_for_tts`, `CursorScrollGeom` are spelled identically across the Interfaces blocks, the skeletons, the repath greps, and the commit messages. `strip_ipa` is consistently `pub(crate)` in both Task 2 (definition) and Task 3 (consumer import).

Order rationale: leaves first (Task 1 `gloss_util`, Task 2 `gloss_ipa`), then the dependent `gloss_block` (Task 3) so `gloss_ipa::strip_ipa` already exists when Task 3 imports it.
