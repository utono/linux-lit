# Gloss overlay pagination — Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development. One subagent per task, review between. Steps use `- [ ]`.

**Goal:** Paginate the gloss overlay's two block-bearing modes (synopsis, gloss result) like the journal overlay (src/ui/pagination.rs) so block nav never clips a partial block at either edge. Echoes + glossing-loading unchanged.

**Architecture:** Mirror the journal's model: `all_blocks` (full list) + `pages` + `page_idx` + `cursor_full` (global) with `cursor_block` page-local. Each page renders only its block slice via the EXISTING populate/render functions, then `rebuild_block_ranges_from` re-derives page-local line spans; vadjustment pinned at 0 (so the accent bar + line-number gutter are automatically correct). j/k step the global cursor and turn the page at boundaries.

**Spec:** docs/superpowers/specs/2026-06-28-gloss-overlay-pagination-design.md (read it first).
**Pipeline map:** see the spec's "Modes" + "risk register".

## Global Constraints

- No `cargo run`; build with `cargo build`, user verifies GUI.
- Behavior-preserving for ECHOES and GLOSSING-LOADING modes — do NOT touch their render or scroll paths.
- The accent bar draw func, line-number draw func, `populate_verse_buffer`, `populate_gloss_buffer`, `rebuild_block_ranges_from`, `size_scroll`, `AskCardHost` stay UNCHANGED.
- Over-estimate Source (verse) block height — NEVER under (a clipped speaker label is the failure). A block too tall for a page gets its own page (paginate already does this).
- Do NOT re-paginate when the ask card opens (matches the journal).
- Visual mode stays within the current page (cross-page selection out of scope).
- `cargo test --bins` stays green; clippy no new warnings (baseline 122).

---

### Task 1: `gloss_block_height` — verse-aware block measurement

**Files:** Modify `src/ui/gloss_overlay.rs` (add a private fn + consts; add a unit test).

**Interfaces:**
- Consumes: `crate::ui::pagination::measure_text_height`, the existing `GlossBlock` type (from `gloss_block::gloss_blocks` / `synopsis_blocks`) — inspect its fields (`kind: BlockKind`, `text`, etc.).
- Produces: `fn gloss_block_height(block: &GlossBlock, pctx: &pango::Context, family: &str, size_pt: i32, wrap_w: i32) -> i32`.

- [ ] **Step 1: Inspect** `gloss_block.rs` for the `GlossBlock`/block struct returned by `gloss_blocks` and `synopsis_blocks` (fields + how to tell Source verse from Explication/synopsis prose). Inspect `gloss_render.rs` for the speaker tag geometry: `gloss-speaker` `pixels_above_lines(36)` + `scale(0.75)`, verse per-line gaps.

- [ ] **Step 2: Write the failing test** in gloss_overlay.rs tests:

```rust
#[test]
fn source_block_height_exceeds_prose_for_equal_text() {
    // A Source (verse, has a speaker heading + per-line gaps) block must measure
    // TALLER than an Explication block of the same text — the conservative
    // over-estimate that prevents clipping the speaker label.
    // (Pure-arithmetic check on the overhead constants; no GTK pango here —
    // factor the overhead into a pure helper `block_height_overhead(is_source,
    // text_h)` that gloss_block_height calls, and test THAT.)
    assert!(block_height_overhead(true, 100) > block_height_overhead(false, 100));
    // Prose overhead is the journal's per-paragraph pad.
    assert_eq!(block_height_overhead(false, 100), 100 + PROSE_PAD);
}
```

- [ ] **Step 3: Implement** `block_height_overhead(is_source, text_h) -> i32` (pure) + `gloss_block_height(...)` (calls `measure_text_height` then adds the overhead). Consts, tied by comment to the tag geometry:

```rust
/// Per-paragraph spacing the rendered view adds that a plain pango::Layout omits
/// (mirrors the journal's pad_per_para at 19pt-ish). Prose/synopsis blocks.
const PROSE_PAD: i32 = 16;
/// Conservative overhead for a Source (verse) block: the speaker heading's 36px
/// `pixels_above_lines` + its ~0.75-scaled line, plus slack. OVER-estimate so a
/// multi-line speech never clips its speaker label at a page top. Tied to
/// gloss_render.rs `gloss-speaker` (pixels_above_lines 36, scale 0.75).
const SPEAKER_BLOCK_OVERHEAD: i32 = 56;

fn block_height_overhead(is_source: bool, text_h: i32) -> i32 {
    if is_source {
        // verse lines carry per-line gaps too -> 1.15 slack on the text height.
        (text_h as f32 * 1.15) as i32 + SPEAKER_BLOCK_OVERHEAD
    } else {
        text_h + PROSE_PAD
    }
}
```

- [ ] **Step 4: Run** `cargo test --bins block_height -- --nocapture && cargo build`. Expected: pass, clean.

- [ ] **Step 5: Commit** `feat(gloss): verse-aware block height for pagination`.

---

### Task 2: Paginate synopsis mode (low risk)

**Files:** Modify `src/ui/gloss_overlay.rs` (struct state + `show_synopsis` + the cursor-nav methods for synopsis). `src/ui/pagination.rs` (reuse paginate/page_containing_block — already there).

**Interfaces:**
- Consumes: Task 1's `gloss_block_height`; `pagination::{paginate, page_containing_block, Page}`.
- Produces: new fields `all_blocks`, `pages`, `page_idx`, `cursor_full`; a private `render_page()` (synopsis arm) + `repaginate()`.

- [ ] **Step 1:** Add state fields (mirror the journal): `all_blocks: RefCell<Vec<GlossBlock>>`, `pages: RefCell<Vec<pagination::Page>>`, `page_idx: Cell<usize>`, `cursor_full: Cell<usize>`. Init in `new()`.

- [ ] **Step 2:** In `show_synopsis`, after building the synopsis block list (`synopsis_blocks(synopsis)`), store it in `all_blocks`, set `cursor_full=0`, `repaginate()` (measure each block via `gloss_block_height` at the synopsis font/width, `paginate` by `size_scroll`'s scroll_h), `page_idx=0`, then render ONLY page 0's block slice (build the page's synopsis text from the slice, `set_text`, `rebuild_block_ranges_from(page_slice)`, pin vadjustment 0, `mark_cursor_block`). Keep label-bold + prose-card margins as today.

- [ ] **Step 3:** Make `cursor_next_block`/`prev`/`first`/`last` page-aware when in synopsis mode (step `cursor_full`, turn page via `page_containing_block` + `render_page` when it leaves the page, else re-mark). Reuse the journal's `step_full_cursor`/`sync_cursor_page` logic shape.

- [ ] **Step 4:** Build + `cargo test --bins`. Verify echoes/gloss-result modes still compile + behave (don't share the new render_page path yet).

- [ ] **Step 5: Commit** `feat(gloss): paginate synopsis mode`.

- [ ] **Step 6: USER visual check** (no-cargo-run): open a synopsis (h), page with j/k/q/, — no partial paragraph at either edge, bar tracks cursor, gg/G to ends, Space reads cursor block.

---

### Task 3: Paginate gloss-result (verse) mode (medium risk)

**Files:** `src/ui/gloss_overlay.rs` (`show_gloss_with_color` + nav for gloss mode + per-page cached-coloring/TTS).

- [ ] **Step 1:** In `show_gloss_with_color`, after `gloss_blocks(gloss)`, store full list in `all_blocks`, `repaginate()` (using `gloss_block_height` — Source blocks over-measured), render page 0's slice via `populate_gloss_buffer` over just those blocks, `rebuild_block_ranges_from(page_slice)`, pin vadjustment 0, `mark_cursor_block`. Line numbers come from `populate_verse_buffer` per page (automatic).

- [ ] **Step 2:** Make gloss-mode nav page-aware (same as synopsis). Ensure `read_current_block` (TTS) + `color_audio_blocks` operate on the current page's blocks; re-apply cached coloring on each page turn (per-page, like the journal's recolor).

- [ ] **Step 3:** Build + `cargo test --bins`.

- [ ] **Step 4: USER visual check:** a multi-speaker verse gloss — speaker labels NEVER clipped at a page top; line-number gutter aligned; bar on cursor block; page turns at j/k boundaries; Space reads.

- [ ] **Step 5: Commit** `feat(gloss): paginate gloss-result verse mode`.

---

### Task 4: Confirm echoes + glossing-loading untouched; cleanup

- [ ] **Step 1:** `rg` confirm `show_echoes` + `show_glossing` + `scroll_echo_into_view` paths are unchanged. Confirm `scroll_gloss`/`scroll_cursor_into_view` are still used ONLY by echoes (or removed if now dead — check before deleting).
- [ ] **Step 2:** `cargo test --bins`, `cargo clippy` (no new warnings vs 122). Remove any now-dead scroll helpers.
- [ ] **Step 3: Commit** `refactor(gloss): drop scroll helpers unused after pagination` (only if anything is dead).

## Notes
- The journal's `step_full_cursor`/`full_cursor_to_end`/`sync_cursor_page`/`render_page` in src/ui/journal_overlay.rs are the reference implementation — read them.
- Per-page cached coloring: see journal's `recolor_journal_cached_blocks` + `color_cached_blocks`.
