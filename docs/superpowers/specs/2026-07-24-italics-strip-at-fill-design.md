# Strip-at-fill for inline italics — LoJ load-time fix design

**Status:** finished design — ready for `superpowers:writing-plans`.

**Date:** 2026-07-24

**Supersedes:** the STRIPPING mechanism of Phase B's `apply_inline_italics`
(`docs/superpowers/specs/2026-07-24-inline-italics-phase-b-design.md`,
implemented at master `d316321d`). Phase B `set_text`s raw `_word_` text then
post-hoc deletes each `_` from the buffer; this design moves the strip to
buffer-fill time and leaves `apply_inline_italics` as a pure tag-application
pass. The FEATURE (rendered italics) is unchanged; only HOW the delimiters are
removed changes. Phase B's parser, offset-map, karaoke translation, multi-span
per-span-tag fix, and net-zero leak fix are all preserved.

## Why (measured, not assumed)

Phase B regressed LoJ load by **~7.3s**. Profiled (fixed 1920x1200, cached
page-table path): `rebuild_buffer_text` = 7293ms = 91% of `display_work`, and the
`ITALIC_UNPAIRED` log streaming 589→7708ms proves the cost is
`apply_inline_italics`. Root cause: it deletes each of LoJ's **45,113
underscores** individually via `buffer.delete` + iterator re-fetch — a
GtkTextBuffer operation that degrades **super-linearly** (10µs/delete at 2.5k
lines → 47µs/delete at full 9,945 italic lines).

**Benchmark (real LoJ input, byte-identical output both ways):**
- Approach A (current, per-`_` `buffer.delete`): **~2112ms** (strip alone, debug)
- Approach B (strip `_` in Rust, ONE `set_text`): **~228ms** — 9.3x faster
- Removing the deletes removes the super-linear pathology; the real-world win is
  larger than the isolated 1.9s (the live 7.3s stacks tag work on the deletes).

**Decision: Approach B.** Strip at fill; `apply_inline_italics` only tags.

## The mechanism

Split `apply_inline_italics`'s two responsibilities:

### A. Strip → buffer-fill (both paths)

New shared helper (in `src/app/italics.rs` or `text_prep.rs`):

```rust
pub struct ItalicStripResult {
    pub stripped_lines: Vec<String>,                      // `_`-free text to set_text
    pub line_spans:   std::collections::HashMap<usize, Vec<(usize, usize)>>, // display-coord italic spans per buf line
    pub line_removed: std::collections::HashMap<usize, Vec<usize>>,          // removed `_` source positions per buf line
}
/// Per line: parse_italic_spans; on Some, use stripped_text + spans + removed;
/// on None (no `_` OR odd count), keep the line verbatim (odd count logged),
/// no span/removed entry.
pub fn strip_italics_for_fill(lines: &[String]) -> ItalicStripResult;
```

`rebuild_buffer_text` has TWO fill paths that today `set_text` raw text then call
`apply_inline_italics` (master `d316321d`):
- **block-aware branch (~mod.rs:4468-4482, LoJ):** `prepare_block_buffer` →
  `set_text(&bb.buf_lines.join("\n"))`.
- **default DB-join branch (~mod.rs:4490-4499, BH/PP):** `work.lines.map(text)` →
  `set_text(&text.join("\n"))`.

Both change identically, GATED on `work_type ∈ {prose,prose_book,epic_translation}`
(plays/BCP untouched — `_` stays literal there exactly as today):
1. run `strip_italics_for_fill` on the branch's line strings,
2. `set_text(&result.stripped_lines.join("\n"))` (clean text — zero deletes),
3. `state.italic_offset_map = result.line_removed` (was populated in the tag pass;
   now here — SAME data, same karaoke consumer),
4. `state.italic_line_spans = result.line_spans` (NEW short-lived field, for the
   tag pass).

For non-gated works the strip is skipped entirely (raw `set_text` as today).

**Ordering invariant preserved by construction:** stripping is now PART OF the
fill, which is before `display_work`'s `build_vocab_matches` — so vocab still
tokenizes stripped text (Phase B's guarantee holds automatically).

### B. Tag-application → slimmed `apply_inline_italics`

No parsing, no deleting. It:
1. keeps the **net-zero `foreach`-remove** of `inline-italic-*` tags at the top
   (the Phase B leak fix — VERBATIM),
2. iterates `state.italic_line_spans`, and for each line's spans applies one
   fresh **named** `inline-italic-{i}-{k}` italic tag per span (the Phase B
   multi-span disjoint-range fix — VERBATIM), reading spans from the map instead
   of re-parsing.

No `buffer.delete`, no iterator-refetch discipline → the GTK-iterator-safety
risk (Task-4's whole concern) is ELIMINATED. The fix is safer, not just faster.

## Correctness preserved (the whole risk)

- **Offset-map / karaoke:** `italic_offset_map` now set at fill (from
  `line_removed`) — identical data, consumed unchanged by `translate_offset` in
  `apply_char_range_tag`. No karaoke change.
- **Multi-span per-span named tags:** preserved verbatim (tag loop reads spans
  from the map; same `inline-italic-{i}-{k}` tags, same render).
- **Leak fix (net-zero removal):** preserved verbatim.
- **`ItalicParse.stripped_text`** — the field Phase B `#[allow(dead_code)]`'d as a
  test-only cross-check — becomes LOAD-BEARING (it is what gets `set_text`).
  Remove the `#[allow]`.
- **Excluded/non-italic:** work_type gate + no-`_` skip → plays, BCP, and every
  non-italic line are byte-identical (raw `set_text`, no span/removed entry,
  `translate_offset` identity on empty).

## Lifecycle

`italic_line_spans` mirrors `italic_offset_map`: declared on `AppState`, init
empty, cleared on every `rebuild_buffer_text` return path, set only on the
gated fill paths. (Phase B already clears `italic_offset_map` on all 4 branches;
add `italic_line_spans.clear()` alongside each.) Never leaks across works.

## Testing & acceptance

- **TDD `strip_italics_for_fill` (pure):** mixed prose lines →
  (stripped_lines, line_spans, line_removed) agree with per-line
  `parse_italic_spans`; `_London_` → stripped + span + removed; multi-`_` →
  multiple spans; ODD `_` → line verbatim + logged, no entry; no-`_` → unchanged,
  no entry. Byte-identical stripped output to Phase B's delete path.
- **Phase B parser/offset tests** (`parse_italic_spans`, `translate_offset`)
  UNCHANGED and green — this is a relocation, not a logic change.
- **Headless on-screen gate (non-optional) — confirm BOTH:**
  1. **Italic renders correctly** — multi-span line (`Life of Johnson` +
     `Pre-Crokerian`) both italic, single-span (`Rasselas`) italic, underscores
     hidden; BH/play regression unchanged. (Same visual as Phase B post-fix.)
  2. **Load time dropped** — at fixed 1920x1200 (cached page-table), measure
     `rebuild_buffer_text` / `display_work total`; the ~7.3s `apply_inline_italics`
     strip cost must be GONE (rebuild back to sub-second-ish). This is the point
     of the fix — prove it with the number.
- **Leak still fixed** (tag count flat across reloads — reuse Phase B's
  `ITALIC_TAGCOUNT` probe) and **0 GTK-iterator criticals** (trivially — no
  buffer deletes at all now).

## Non-goals

- Any change to WHICH text is italic, or to the parser / offset logic / karaoke.
- Plays / BCP inline italics (still excluded).
- Vocab/gutter/page-table load costs (separate; vocab is ~700ms, not the target).
- The cage-vs-GL page-table regen (a headless artifact; the user's real launch
  hits the cache).

## References

- Root-cause profile + benchmark: `/tmp/loj-load-rootcause.md` (this session).
- Phase B (the mechanism this supersedes):
  `docs/superpowers/specs/2026-07-24-inline-italics-phase-b-design.md`,
  `docs/superpowers/plans/2026-07-24-inline-italics-phase-b.md`.
- Touch points: `src/app/mod.rs` `rebuild_buffer_text` (two fill branches ~4468,
  ~4490), `src/app/formatting.rs` `apply_inline_italics`, `src/app/italics.rs`
  (`parse_italic_spans`, `translate_offset`, new `strip_italics_for_fill`),
  `src/input/phrase_highlight.rs` `apply_char_range_tag` (unchanged consumer).
