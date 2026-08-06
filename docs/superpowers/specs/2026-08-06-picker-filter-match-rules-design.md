# Picker filter match rules — fuzzy for short labels, literal for long text

## Problem

Searching `simile` in the journal Q&A picker returns five rows. Only one of
them has anything to do with similes.

Verified against `lit.db` (twelve `scope='passage'` entries for BH-Barrett):
exactly one entry, id 70 at division 9.0, contains "simile" in its question or
answer. The other four rows are false positives.

The filter matches two targets (`journal_picker.rs` `populate_list`):

1. The displayed row text — fuzzy `subsequence_match`
2. The entry's full question + answer (`search_haystack`) — literal substring

The body was already restricted to literal substrings, and the code comment
there states why: "over a multi-thousand-character answer, a scattered
subsequence matches almost any short filter, so every row would survive and the
filter would stop filtering."

That reasoning was never applied to the display target, which still runs fuzzy
over an 80-character passage label. `simile` is six common letters in order, so
the subsequence scavenges them across the whole label:

- 3.0 — **s**(passage) … earl**i**est … re**m**embrance … l**i**ke … so**m**e
- 4.0 — **s**(passage) … **i**t … M**i**ss … Je**l**lyby … sh**i**v**e**ring
- 8.0 — **s**(passage) … lad**i**es … **m**ost … d**i**stinguished … benevo**l**enc**e**
- 8.0 — **s**(passage) … sa**i**d … **M**rs … Pard**i**gg**l**e

Every false positive borrows its leading `s` from the word "passage" in the
type column, then collects the rest from the prose. The one true hit (9.0) does
NOT match fuzzily — it survives only via the literal body check.

Two aggravating factors, both by design and both staying:

- `scope='passage'` rows label themselves with the first line of the SOURCE
  PASSAGE, not the question, so even the true hit displays "We were going on in
  this way…" rather than anything about similes.
- The label is truncated to ~80 characters, so a body hit is routinely invisible
  in the row text.

With twelve rows the noise is merely annoying. On a longer list, a short
common-letter query matches nearly everything and the filter stops filtering —
exactly the failure the body rule was written to prevent.

## Goal

Match rule follows field length, not field identity:

- **Short label fields** (author, work, division, type) keep fuzzy matching.
  Typing `dickens` or `ch. 2` should still narrow — that is the natural gesture
  on a cross-work list.
- **Long text** (the 80-char row label, the full Q&A body) requires a
  contiguous substring.

## Design

### Shared predicate

Extract the decision into a pure function in `picker_filter.rs` so both
affected pickers share one rule and it is directly testable without GTK:

```rust
/// Match rule scaled to field length: fuzzy subsequence over the SHORT label
/// fields, contiguous substring over long text.
///
/// A subsequence over long prose degenerates — six common letters match almost
/// any passage — so only the short fields may be fuzzy. All arguments are
/// already lowercased by the caller.
pub(crate) fn row_matches(
    filter: &str,
    short_target: &str,
    long_target: &str,
    haystack: &str,
) -> bool {
    subsequence_match(filter, short_target)
        || long_target.contains(filter)
        || haystack.contains(filter)
}
```

Callers pass `""` for a target they do not have.

**Precondition: `filter` is non-empty.** Both call sites already sit inside an
`if !filter.is_empty()` guard (an empty filter shows every row without
consulting the predicate). This matters because `"".contains("")` is true, so an
empty filter would otherwise be accepted by every target including absent ones.
The guard stays where it is; `row_matches` documents the precondition rather
than re-checking it.

### Change 1 — `journal_picker.rs` `populate_list`

Replace the single concatenated `display_target` with two targets:

```rust
let short_target = format!(
    "{} {} {} {}",
    item.author_label.as_deref().unwrap_or(""),
    item.synopsis_division_label,
    item.div_label,
    item.type_label,
).to_lowercase();
let long_target = primary.to_lowercase();

let hit = crate::ui::picker_filter::row_matches(
    &filter_lower, &short_target, &long_target, &item.search_haystack,
);
```

The load-bearing detail: `type_label` ("passage") leaves the fuzzy target's
adjacency with the passage prose. That word supplied the leading `s` in all four
false positives.

`primary` already embeds `work_label` (`"BH · I was brought up…"`), so the work
title still matches literally.

### Change 2 — `recent_qa_picker.rs`

The identical defect: it concatenates `work_label` + `question_prefix` and
fuzzy-matches the pair. Split the same way — `work_label` fuzzy,
`question_prefix` literal.

### Accepted trade-offs

- **Fuzzy work-title matching is lost inside these two pickers.** `bh-b` no
  longer finds "BH-Barrett" here; `bh` and `barrett` still do. Accepted: the
  library picker is where works are fuzzy-found. (Confirmed with the user.)
- **Typo tolerance is lost on the row label.** `jelby` no longer finds
  "Jellyby"; `jellyby` does. Accepted as the cost of a predictable rule.

### Change 3 — `gloss_picker.rs` and `bookmark_picker.rs` (added mid-branch)

**This section corrects an error in the original spec.** The first draft
exempted these two as "genuinely short labels". That claim was never measured,
and it is false. A whole-branch review checked both against `lit.db`:

- `gloss_picker` matches `speaker + source_text`, where `source_text` is the
  FULL passage body — 7007 glossed passages, mean 524 chars, max 7123. On
  BH-Barrett, `metaphor` fuzzy-matches 438 of 464 rows against 1 real literal
  hit; on Henry IV, all 154 rows match `romeo` with ZERO literal hits.
- `bookmark_picker` matches `speaker + line_text`, where `line_text` is
  `line_mapping.canonical_text` — NOT one short line. Across 66 live bookmarks
  it means 343 chars, max 2156, because prose works store a whole paragraph per
  row. `simile` fuzzy-matches 36 of 66 with zero literal hits.

Both DISPLAY only the first line while MATCHING the whole text, so the
degeneration is invisible in the row label — the same shape as the journal
picker bug, at a higher false-positive rate. Both get the same split: `speaker`
fuzzy, the long text contiguous-only, empty haystack.

The lesson: "this field is short" is a claim about DATA and must be measured
against the database, not inferred from the field's name or its role in the UI.

## Not changing

**`media_picker` — measured and deliberately left fuzzy (2026-08-06).** The
whole-branch review reported "126/235 on `simile`" and flagged it as borderline.
That figure does not reproduce and appears to have been computed over full
filesystem paths rather than the labels the picker actually builds. Re-measured
against all 235 real `media_files` rows, simulating `format_media_label`:

- Label length: mean **46** chars, max **114** (`parent-dir/filename`), or a
  curated `display_name` — 8 exist, max **49** chars.
- `simile` fuzzy-matches **6/235 (2%)**, `bleak` 14 (5%), `romeo` 44 (18%),
  `ration` 43 (18%).

Nothing near filter collapse (gloss_picker was 96%). Two further reasons to
leave it: this picker MATCHES EXACTLY WHAT IT DISPLAYS — the same `display`
string feeds both the label and the filter — so it has none of the
invisible-matched-text problem that made the other four bugs hard to see. And
filenames are precisely where fuzzy earns its keep: scattered matching is what
lets `bhep6` find `BleakHouse_ep6_Sean_Barrett.m4b`. Converting would trade a
working affordance for a non-existent problem.

`journal_move_picker` matches a division label, `library_picker` uses its own
scorer over title/author/abbrev, and `journal_term_input` matches single terms.
All genuinely short; left alone.

The `scope='passage'` source-line labelling and the 80-char truncation are
deliberate existing behavior and are out of scope.

## Testing

Unit tests against `row_matches` — pure, no GTK, runs under `cargo test --bins`:

- Each of the four "simile" false-positive labels is REJECTED when passed as
  `long_target` with an empty haystack. These fail before the change.
- Division 9.0 is ACCEPTED via the haystack (body hit), with a label that does
  not contain the term — proving body search still reaches invisible hits.
- `jellyby` accepted against the 4.0 label (literal substring survives).
- A division-style query (`3.0`, `passage`) still fuzzy-hits the short target.
- An absent (`""`) long target with a matching short target still hits, so a
  caller omitting a field loses nothing.

`picker_filter.rs` already has a unit-test module, so the tests land beside
`subsequence_match_preserves_boolean`.

## Acceptance

Searching `simile` in the journal Q&A picker returns exactly one row —
division 9.0, the Jarndyce "no simile for his lungs" entry.

**Verified 2026-08-06** headlessly (`land-on.sh BH-Barrett 9.0`, Ctrl+j,
`simile`): ONE row returned, down from five. The four false positives at 3.0,
4.0, 8.0 and 8.0 are gone; the true hit survives via the body haystack despite
a visible label about breakfast.

Body search confirmed intact in the same run: `jellyby` returns four rows —
one matching the visible label (4.0) plus three whose ANSWER bodies discuss
Jellyby (0.0, 8.0, 8.0). Only the letter-scavenging was removed, not the
ability to find terms that never appear in the truncated row label.

Short-target immunity was also checked against real data rather than inferred:
across all 14 BH `journal_entries` rows and all three live scope values
(`passage`, `division`, `unassigned-after-reimport`), with and without the
author name, `short_target` never fuzzy-matches `simile`. The fix does not
merely relocate the false positives into the short field.
