# Gloss overlay opens but the body is blank

Frequency-ordered failure modes for "the gloss overlay maps, the footer
counter is right, and the card is empty" — plus the sibling failure, "the
bind toasts *no gloss* on a line that demonstrably has one."

Both classes share one tell: **the build is green, the log says success,
and only the pixels disagree.** `TEST_OVERLAY_VIEWPORT_RECT` is logged when
the overlay MAPS, not when it renders ink. Never accept it as acceptance.

---

## 1. Untagged (Markdown) gloss text rendered through the tagged path

**Tell.** Overlay opens, header and footer correct ("Vocab-word 1 of 1"),
body completely empty. Screenshot is suspiciously small (~10 KB at 1280x720
vs ~170 KB once text renders) and BYTE-IDENTICAL across runs.

**Root cause.** `gloss_type='vocab-word'` glosses are stored as plain
Markdown. Every other type is stored with `<speaker>`/`<segment>`/`<gloss>`
tags. `gloss_blocks` only emits blocks for TAGGED elements, so untagged text
yields zero blocks -> `repaginate` clears `pages` -> `render_gloss_page`'s
page slice hits `let Some(page) = page else { return }` and leaves the
buffer untouched. The footer is painted by a different path, which is why it
looks half-working.

**Fix.** `render_gloss_page` detects a gloss with no tagged blocks and
routes it to `render_markdown_gloss_page`, which renders via
`plan_markdown_blocks` + `render_markdown_blocks` — the journal overlay's
pipeline. `MarkdownTags::register` is idempotent and already runs in `new`.

**Detect by CONTENT, not by `gloss_type`.** The first attempt keyed on
`footer_gloss_type == Some("vocab-word")` and silently never fired:
`record_last_gloss` sets that field AFTER the first render. Measured with a
temporary log line: `footer_type=None gloss_len=2200`. Keying on "no tagged
blocks" also makes any future untagged type work for free.

---

## 2. Prose citations silently unparseable (`parse_citation`)

**Tell.** A gloss bind toasts "No … gloss on this line" on a PROSE work
where the DB plainly has a covering passage. Plays work fine. Any
citation-driven feature can hit this — gloss overlay, tint, picker landing.

**Root cause.** `db::models::parse_citation` took the trailing THREE
numbers. Prose elides the always-zero div2, so citations arrive 3-part
(`LoJ.1.2207`, `PL.1.417`) rather than 4-part (`Err.1.1.1`). The parser fed
the abbrev to `"LoJ".parse::<i64>()` for div1 and returned `None`. That
makes a passage INVISIBLE to `passage_covers` rather than non-matching — the
caller cannot tell "no gloss here" from "cannot parse this", so the failure
presents as a clean miss.

**Fix.** `parse_citation` accepts both shapes: when the third-from-last part
is non-numeric it is the abbrev, so the apparent div2 is really div1 and
div2 is the elided 0. Confirmed against `line_mapping`, where every LoJ row
stores `div2 = 0`. Regression test:
`parse_citation_accepts_three_part_prose_citations`.

**Generalization.** A parse failure that returns `None` into a
`.find(...)`/filter chain is indistinguishable from "no match". When a
lookup reports nothing found on data you can see in the DB, verify the
KEY PARSES before auditing the query.

---

## Diagnostic order for this class

1. Confirm the row exists and note its citation SHAPE:
   `sqlite3 lit.db "SELECT start_citation FROM passages WHERE …"` — count
   the dots. 2 dots = prose, 3 = play.
2. Confirm the bind dispatches: `rg "KEY: name=…" $LOG`. A missing line is a
   keymap problem (check `keymap.json` shadowing), not a lookup problem.
3. Confirm the overlay mapped: `TEST_OVERLAY_VIEWPORT_RECT`. Present + blank
   card = item 1 above. Absent + toast = item 2, or a genuine miss.
4. PIXEL-CHECK the capture. `stat -c%s` on the PNG is the fastest signal: an
   unchanged byte count across a rebuild means your branch never ran.
5. If a branch "should" be firing, add one `log_fmt!` of the state it keys
   on before changing more code. That single line is what identified the
   `footer_gloss_type=None` ordering bug after two wrong fixes.
