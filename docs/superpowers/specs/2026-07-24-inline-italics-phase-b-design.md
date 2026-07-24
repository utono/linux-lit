# Inline italics for prose (`_word_` → italic span) — Phase B design

**Status:** finished design — ready for `superpowers:writing-plans`.

**Date:** 2026-07-24

**Supersedes:** the **Facet-1 (inline italics)** material in
`docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md` and the
**Phase B** stub in `docs/superpowers/specs/2026-07-24-per-line-verse-reader-finish-design.md`.
This is the finished, code-grounded design for that work. Phase A (per-line
verse) is merged (`5f3040d7`); this is the next reader phase. Pure linux-lit,
unblocked.

## Goal

Render prose `_..._` runs as **inline italic spans**: the paired underscore
delimiters are hidden, the enclosed run is styled `pango::Style::Italic`. Today
`_word_` renders with literal underscores (e.g. a work title *London* shows as
`_London_`). LoJ's italics are overwhelmingly meaningful (~10.4k work/book
titles, ~5.2k phrases, ~5.2k single-word emphasis, ~544 Latin/foreign), so LoJ
RENDERS (not strips — unlike TT, which was stripped in lit.db).

## Scope

- **Included:** `prose` / `prose_book` + `epic_translation` works carrying `_`
  markup — LoJ 9,945 rows, plus PP, MobyDick, TTC, TWWLN, VF, BH (+ variants),
  DC, Il, etc.
- **Excluded this cycle: plays (Ibsen)** — WildDuck 638, DollsHouse 296,
  HeddaGabler 25, MasterBuilder 23, Ghosts 14, Eyolf 5, DeadAwaken 4. Their `_`
  is stage-direction emphasis inside `[...]` lines that `apply_dialogue_formatting`
  ALREADY italicizes WHOLE-LINE — inline italic there would be redundant /
  italic-on-italic conflict. Gate on `work_type`. Follow-up if ever wanted.
- **Not re-italicizing** already-stripped works (TT). **Not** converting
  `_x_`→`*x*` in lit.db (the buffer parses no markdown). Only italic `_..._` —
  no bold/small-caps.

## The core insight (why this feature is small)

A code survey of every buffer consumer (2026-07-24) found that hiding `_`
delimiters — which shifts every char offset after them — breaks **exactly one**
consumer. The rest re-derive from the live buffer and are immune:

| Consumer | Verdict | Why |
|---|---|---|
| `phrase_highlight.rs` (karaoke) | **AT-RISK** | `PhraseSpan.start_char/end_char` come from the DB `phrase_timestamps` table (source-relative) and are applied to buffer offsets in `apply_char_range_tag` with NO re-derivation |
| Search (`search.rs`) | SAFE | matches live `buffer.text()`, not `canonical_text` |
| Vocab (`build_vocab_matches`, `vocab_scan.rs`) | SAFE | tokenizes/tags the same live-buffer snapshot |
| word_copy (`extract_buffer_line_words`) | SAFE | scans live buffer text; already strips `_` via `trim_matches` today |
| Pagination (`viewport.rs`, `page_table.rs`) | SAFE | line-index + pixel geometry only; hiding `_` changes line WIDTH not COUNT |
| `LineMap` (`text_file_map.rs`) | SAFE | line-indexed; no per-line char-count field |

**Two in-tree precedents settle the approach — no global offset map needed:**
1. `apply_bcp_formatting`'s `^...^` handler (`formatting.rs:460-495`) already
   **deletes delimiters from the live buffer and re-tags from what's left**
   (highest-offset-first, re-fetching iters after each delete to avoid GTK
   criticals). This is the exact idiom a `_word_` italic tagger follows.
2. The `translations_visible` gate is the codebase's precedent for
   "buffer diverges from source" — but we do NOT need to disable karaoke; a
   narrow per-line offset translation keeps it correct (§3).

## Mechanism (parse, strip, tag)

A new pass in `src/app/formatting.rs` (sibling to the BCP `^...^` handler),
invoked for prose/epic_translation works. Per line:

### Parse — a pure, unit-tested helper

`parse_italic_spans(line: &str) -> Option<ItalicParse>` where `ItalicParse`
carries `{ stripped_text: String, spans: Vec<(start,end)> (display-relative),
removed_positions: Vec<usize> (source offsets of removed `_`) }`.

- Non-greedy pair rule `_([^_]+)_`. Even `_` count → each pair is an italic span.
- **Odd `_` count (unpaired): return `None`** — the line is left VERBATIM (no
  strip, no italic) and LOGGED (the 61 LoJ odd-`_` defect rows stay visible for
  upstream fixing; never italicize-to-end-of-line on a stray `_`).
- Word-internal welds (`John_son_`) fall out of the pair rule naturally (the
  enclosed run italicizes). Currency (`120_l_.`) → italic `l`, normal span.
- A line with no `_` → `None`-equivalent / no-op (fast path).

### Strip + tag

For a line with `Some(parse)`: delete the paired `_` from the DISPLAYED buffer
text (highest-offset-first so earlier offsets stay valid), re-fetching iterators
after each delete (per the `^...^` precedent, GTK-critical avoidance), then apply
a `pango::Style::Italic` TextTag to each `spans` range on the now-shifted line.

### Record the offset map

`state.italic_offset_map: HashMap<buffer_line, Vec<usize>>` — for each stripped
line, its `removed_positions` (sorted source offsets of removed `_`). Only italic
lines carry an entry; absent = empty = identity. Cleared/rebuilt on every
`rebuild_buffer_text` (like `block_indent_tiers`), never leaks across works.

## Karaoke offset reconciliation (§3 — the one at-risk consumer)

`phrase_highlight::apply_char_range_tag` receives a SOURCE-relative
`start_char`/`end_char` from `phrase_timestamps` and applies it to the stripped
buffer line — too large by the number of `_` removed before the offset. Fix: a
pure translation consulted ONLY here:

```
translate_offset(removed_positions: &[usize], source_offset: usize) -> usize
    = source_offset - (count of removed_positions <= source_offset)
```

- Monotonic; **identity when `removed_positions` is empty** → every non-italic
  line and every excluded/non-LoJ work is byte-identical (zero cost).
- `apply_char_range_tag` looks up the line's `removed_positions` from
  `italic_offset_map` (absent → empty → identity) and translates `sc`/`ec`
  before `set_line_offset`. This is the ONLY consumer that consults the map.

**Exercisable NOW on PP — not LoJ-blocked.** DB survey (2026-07-24) of italic
works vs `phrase_timestamps`:
- **PP (Pickwick Papers): 244 italic rows AND 87,857 phrase_timestamps** — the
  ideal test work. MobyDick (180 italic rows, 32k phrase ts) and TTC (88 / 18k)
  also qualify.
- LoJ: 9,945 italic rows but **0 phrase_timestamps** (its karaoke shows nothing,
  `spans=0`, until the litdb `backfill-phrase-timestamps` run — Phase C).

So the karaoke reconciliation IS on-screen-provable this cycle on **PP** (an
italic word on a phrase-timed line, karaoke sweep lands on the right chars after
`_` removal). LoJ's karaoke-over-italics proof still waits for its Phase-C
backfill, but the reconciliation logic is NOT LoJ-gated — PP verifies it now.

## Testing & acceptance

- **TDD the pure logic (mandatory — offset-sensitive core):**
  - `parse_italic_spans`: even → stripped + spans + removed positions; odd →
    `None` (verbatim, logged); non-greedy (`_A_, _B_` → two spans); weld
    (`John_son_` → italic `son`); currency (`120_l_.` → italic `l`); no-`_` →
    no-op.
  - `translate_offset`: subtracts removed-≤-offset count; **identity on empty**;
    offset at a removed position; multiple removals before a span.
- **Pure-helper tests** (`cargo test --bins`) — no GTK for the logic.
- **Headless on-screen proof (non-optional gate):** cage/grim to a LoJ
  italic-rich passage (work titles, Latin). Confirm by eye + pixel: `_word_`
  renders as ITALIC `word` (slanted glyphs, NO underscores); a work title shows
  italic; surrounding roman text unaffected; no clipping. Regression: a prose
  work with no `_` screenshots identical to pre-change; search/vocab/word-copy on
  an italic line still work (re-derive from the stripped buffer — search "London"
  hits the italic-displayed word).
- **Karaoke reconciliation on-screen proof — THIS cycle, on PP** (244 italic
  rows + 87,857 phrase_timestamps). Drive to a PP line where an italic word sits
  on a phrase-timed line, play/seek, and confirm the karaoke sweep lands on the
  correct chars AFTER `_` removal (not shifted by the removed delimiters). LoJ's
  own karaoke-over-italics proof waits for its Phase-C backfill, but the logic is
  verified on PP now — not deferred.

## Non-goals

- Plays / Ibsen inline italics (excluded — whole-line stage italic already).
- Bold, small-caps, other markup — only italic `_..._`.
- Karaoke-over-italics on-screen proof ON LoJ specifically (Phase C, data-gated —
  LoJ has 0 phrase_timestamps). The reconciliation itself IS proven this cycle on
  PP; only LoJ's own karaoke proof waits.
- Re-italicizing stripped works (TT); `_x_`→`*x*` DB conversion.

## References

- Consumer risk map (2026-07-24 code survey) — summarized in "The core insight".
- Precedent idiom: `src/app/formatting.rs` `apply_bcp_formatting` `^...^` handler
  (delete-delimiters-in-buffer + retag) ~lines 460-495; divine-name small-caps
  ~539-567.
- At-risk consumer: `src/input/phrase_highlight.rs` `apply_char_range_tag`
  (~775-803), `PhraseSpan.start_char/end_char` from `src/db/queries.rs` (~735-748).
- Superseded Facet-1 detail: `docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md`.
- Phase A (per-line verse, merged): `docs/superpowers/specs/2026-07-24-per-line-verse-reader-finish-design.md`.
