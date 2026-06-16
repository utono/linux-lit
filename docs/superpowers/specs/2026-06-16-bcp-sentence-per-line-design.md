# BCP sentence-per-line layout

## Goal

In Book of Common Prayer (`BCP*`) works, render each **sentence** on its own
buffer line with a ~12px gap between lines, so prayers read as airy, separated
blocks rather than dense paragraphs — while preserving all navigation, MPV sync,
`u`/`.` timestamp binds, and concordance highlighting.

## Background

BCP prayers are stored one prayer per `line_mapping` row → one buffer line that
wraps. The existing block-spacing (`pixels_above_lines(10)`) separates prayers
but reads tight. The user wants each sentence visually broken out.

`build_line_map` matches buffer lines to DB rows by **exact normalized-text
equality**. Splitting a prayer into sentence lines naively would leave each
sub-line unmatched → broken timestamps/sync/`u`-`.`/concordance. The split must
teach the line_map that N sentence lines map to one DB row.

## Approach: split + sub-line map

### 1. Sentence splitting (BCP-only, in the clean step)

New helper in `src/db/line_types.rs`:

```rust
pub fn split_bcp_sentences(line: &str) -> Vec<String>
```

Rules (period-only):

- Break after `.` when followed by whitespace + an uppercase letter.
- Do NOT break on `:` or `;` (internal list separators stay inline).
- Do NOT break inside abbreviations/numerals: `&c.`, single-letter + `.`,
  dotted roman-numeral groups (`.vi.`, `.vii.`, `iiii.`), or a `.` adjacent to
  `*` (footnote marker, e.g. `Elizabeth*`).
- `Amen.` stays attached to the sentence before it (no dangling one-word line).
- Returns the line unchanged (single-element vec) when no split point is found.

Only **body** lines are split: not rubrics (`[...]`), not `## ` headings, not
already-short litany lines (a line with no internal split point returns as-is).

### 2. clean_file_lines: emit sub-lines + record provenance

`clean_file_lines` (src/app.rs) gains a BCP-only branch: for a body line, push
each `split_bcp_sentences` result as its own cleaned line. It additionally
returns a `Vec<usize>` mapping **cleaned-line index → source-line index** (the
prayer's original index) so the line_map can group sub-lines.

`clean_file_lines` is called from `prepare_text_only` and
`prepare_text_for_display`; both thread the new provenance vec through.

Whether a work is BCP is decided by `is_bcp_work(&work.abbrev)`, passed into
`clean_file_lines`.

### 3. build_line_map: N sentence lines → 1 DB row

`build_line_map` gains a BCP path keyed off the provenance vec:

- For each group of cleaned lines sharing a source-line index, the **joined**
  normalized text of the group equals the DB row's normalized text, so the
  existing equality match still finds the row.
- All sub-lines of the group get the **same** `work_lines` index in
  `buffer_to_work`. The **first** sub-line is canonical: `work_to_buffer[wi]`
  points at it (timestamps/sync/`u`/`.` read & write there).
- `section_starts` / `chapter_breaks` attribute to the first sub-line.

Non-BCP works keep the current 1:1 path untouched.

### 4. Styling & spacing (apply_bcp_formatting, src/app.rs)

- `bcp-body` / `bcp-body-indent` `pixels_above_lines` → **12** (per sentence
  line now). Continuation block left-indent applies to all of a prayer's lines.
- Opening-word small-caps applies only to a prayer's **first** sentence line
  (detected: source-line changed from the previous line).
- Rubrics/litany keep current italic/centered styling + the 12px gap.

### 5. Snapshot version

Bump `SNAPSHOT_VERSION` (src/snapshot.rs, currently 5 → 6): cleaned-lines and
`LineMap` serialized shape change for BCP works; old snapshots must be
invalidated.

## Testing

Unit (`cargo test --bins`):

- `split_bcp_sentences`: the three real prayers (id 903526 → 2 sentences,
  903534 → 1, 903536 → 1 with the colon-list inline); abbreviation non-splits
  (`&c.`, `.vi.`, `.vii.`, `iiii.`, `Elizabeth*`); `:`/`;` non-splits; `Amen.`
  attachment; no-split-point returns single element.
- `build_line_map` on a split BCP prayer: every sub-line maps to the same work
  index; first sub-line is canonical in `work_to_buffer`.

Visual (user-run, headless cage is unreliable on the live session):

- Litany / Queens-Majesty / Ten-Commandments spreads: each sentence on its own
  line with a clear gap; timestamps/`u`/`.`/concordance still land correctly.

## Out of scope

- Full (Fill) justification — GtkTextView does not render it (prior finding).
- First-line `TextTag:indent` — GtkTextView does not render it; continuation
  indent uses `left_margin`.
- DB-level reflow (would affect Android/iOS/web readers and concordance).
