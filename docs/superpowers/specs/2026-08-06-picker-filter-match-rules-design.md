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

## Not changing

`bookmark_picker`, `gloss_picker`, `media_picker`, `journal_move_picker`, and
`library_picker` all fuzzy-match over genuinely short labels (work titles,
bookmark names), where fuzzy is the feature and degeneration cannot occur.
`journal_term_input` matches single terms. Left alone.

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
division 9.0, the Jarndyce "no simile for his lungs" entry. Verified on screen
in the real picker, not from tests alone.
