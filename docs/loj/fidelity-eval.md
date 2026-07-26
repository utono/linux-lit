# LoJ Rendering Fidelity — Reader vs Gutenberg Reference

_Evaluated 2026-07-26 (US Central). Read-only assessment; no code or data changed._

Companion document: [`history.md`](history.md) — what has been built for LoJ
across linux-lit and litdb, the HTML-import transition, and the skills chain.

## Scope and method

Compares linux-lit's rendering of **LoJ** (Boswell, _The Life of Samuel
Johnson_, `work_type='prose_book'`) against the Project Gutenberg reference
edition — the six volumes at
`~/utono/literature/boswell-james/gutenberg/life-of-johnson-vol{1..6}.html`,
which map to LoJ `div1=1..6`.

Note on the reference: `ac` item 4 named `~/Downloads/pg8918-images.html`. That
file was trashed, but it is byte-identical (md5 `2abe0c90…`) to the filed
`life-of-johnson-vol1.html`, so nothing was lost — the filed volumes are the
canonical copy and cover all six divs rather than just vol 1.

Reference structure was parsed from the HTML (`<p>` blocks split on `<br>`,
`<i>` spans, `h1`–`h5`), normalized (NFKC, `_` delimiters stripped, `[N]`
footnote markers stripped, whitespace collapsed) and matched against
`line_mapping` rows. Vol1 matched 1379/1383 verse lines (99.7%).

**Counting caveat worth repeating:** heading counts must exclude Project
Gutenberg's footer boilerplate, which begins after the `*** END OF THE PROJECT
GUTENBERG EBOOK ***` marker (vol1 line 29313). A naive `rg -c` over the whole
file inflates `h2`/`h3` counts and picks up 5 `class="secthead"` divs that are
boilerplate, not content. Content-only vol1 census: **h1=2, h2=14, h3=5,
h4=18, h5=252 — 291 headings across five levels.**

## Verdict

Everything the reader itself controls is faithful: verse/prose
classification, indent tiers, footnote markers, and inline italics all match
the reference at or near 100%. Every real gap found is either **structural
data the schema cannot express** or an **upstream ingest defect** — none is a
rendering bug in linux-lit.

---

## REAL GAPS

### 1. Stanza breaks are absent corpus-wide (upstream litdb + a dead reader conditional)

The reference encodes stanzas as separate adjacent `<p>` blocks. The Horace
ode (`Translation of HORACE. Book I. Ode xxii.`, vol1 HTML 2651–2696) is six
four-line `<p>` blocks, `id00427`…`id00432`. The DB stores it as 24
consecutive verse rows, `div1=1` `line_in_div` 458–481, with nothing marking
the boundaries at 461/462, 465/466, …

Verified directly:

```
458 verse [The man, my friend, whose conscious heart]
459 verse [  With virtue's sacred ardour glows,]
460 verse [Nor taints with death the envenom'd dart,]
461 verse [  Nor needs the guard of Moorish bows:]     <- stanza ends here
462 verse [Though Scythia's icy cliffs he treads,]     <- new stanza, no break
```

`SELECT COUNT(*) … block_type='verse' AND TRIM(canonical_text)=''` returns
**0 for every div1**. Missing empty rows by volume: vol1 **29** (8
multi-stanza poems), vol2 5, vol3 3, vols 4–6 none — **37 total**.

This **contradicts a stated premise** in
`docs/superpowers/specs/2026-07-24-per-line-verse-reader-finish-design.md`
(line 34), which asserts "Vol1 has zero of these — its verse has no internal
blank-line breaks," and the derived claim in `ac` that the Phase-A empty-row
limitations are "inert." Vol1 in fact has the most stanza breaks of any
volume. The Phase-A gap is not inert; it is unobservable only because the
data never arrived.

**Second, independent cause — the gap tag is now a no-op.** In
`src/app/formatting.rs:737`:

```rust
if prev_src != Some(wi) {
    state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
}
```

`prev_src` tracks the *source row* index. Under the old block-granularity
model (one row → N buffer lines) this correctly fired only on a block's first
line. Under per-line verse, `prepare_block_buffer` emits one buffer line per
row with strictly increasing source index, so `prev_src != Some(wi)` is true
on **every** verse line and all of them receive `pixels_above_lines(12)`.

So the visual result is not a tightly-packed wall — it is verse that is
*uniformly* loosely leaded, carrying no stanza information at all. Restoring
the 37 empty rows upstream would **not** fix it on its own; the conditional
must be re-derived from something that still distinguishes a stanza start.

Per the repo's upstream-root-cause rule, the data half belongs in litdb; the
dead conditional is reader-side.

### 2. Five heading levels flatten into one (schema-level)

The reference distinguishes `h1`–`h5` and uses them meaningfully: `<h2>APPENDIX
A` / `PREFACE.` for volume-level divisions down to `<h5>'SIR,` for a letter
salutation. `line_mapping.block_type` is a flat four-value vocabulary
(`prose|verse|heading|blockquote`), so all 291 vol1 headings become one
`heading`, and `apply_block_typography` (`formatting.rs:741`) renders each
identically through a single `block-heading-center` tag (centered +
`SmallCaps`).

A volume title and a letter salutation are therefore typographically
indistinguishable. Nothing downstream can recover the level — it needs a
litdb column.

**The reference disagrees about centering, too.** Its stylesheet explicitly
comments centering *out* for chapter and topic heads:

```css
h2 {
    /* text-align:center;  left-aligned by default. */
```

So the reader centers headings the reference deliberately left-aligns. Worth
a deliberate decision rather than an accidental divergence.

### 3. vol1 TOC / List-of-Illustrations entries mis-tagged as prose (upstream, vol1-only)

Reference vol1 line 342 marks each contents entry `<h5>`; the DB has them as
`prose`. The container headings are correct — only the entries flipped:

```
46 heading  CONTENTS OF VOL. I.        <- correct
47 prose    PAGE
48 prose    DEDICATION TO SIR JOSHUA REYNOLDS . . . 1   <- ref <h5>
```

Roughly 15 contiguous rows. **Volume-specific**: vols 2 and 5 capture headings
at 100% (181/181, 102/102), so this is a vol1 front-matter ingest defect, not
systemic. Upstream (litdb).

### 4. Cross-row italic spans render a literal `_`

When an italic span wraps a source newline, it splits across two rows — one
carrying the opener, one the closer. Each row alone has an odd underscore
count, so `parse_italic_spans` (`italics.rs:35`) bails and the row renders
verbatim:

```
1.0.1274  the notes into text, and inserted it amongst his _Lives of the English
1.0.1275  Poets_.
```

Both lines show a stray `_` and neither is italic. Affects ~42 vol1 rows and
~55 vol6 rows. The per-line bail is correct defensive behavior (and is logged
as `ITALIC_UNPAIRED`, `italics.rs:100`), but the visible result is wrong.

**Confirmed on screen**, LoJ title page: rows 3 and 4 —
`_INCLUDING BOSWELL'S JOURNAL OF A TOUR TO THE HEBRIDES` /
`AND JOHNSON'S DIARY OF A JOURNEY INTO NORTH WALES_` — render with literal
underscores and no italics. The reference has the same span split across
`<h3>`/`<h5>` at vol1 lines 91/93, so the source is faithful; only the
per-line parser can't span rows.

---

## ACHIEVED

- **Verse/prose classification** — 1095/1102 vol1 verse rows match a
  reference `<br>` line. No prose mis-tagged as verse: the 271 `<br>`-lines
  stored as prose are Gutenberg's hard-wrapped prose (bibliography catalogue,
  letters, ledgers), correctly rejoined. Vols 2/3/6 matched with zero
  unmatched lines. In 7 cases the DB classifies verse the markup did not
  (single-`<p>` quotations, the Blaney epitaph) — **more** accurate than the
  reference.
- **Indent tiers** — essentially perfect. Leading whitespace survives into
  `canonical_text`, is bucketed by `leading_space_tier` (0 / 1–2 / 3+) and
  rendered at 48/80/112px. vol1 1095/1095, vol2 432/432, vol3 506/506,
  vol6 20/20. The alternating flush/indent quatrain pattern of the Horace
  odes survives exactly.
- **Footnote markers in verse** — identical treatment to prose, no
  special-casing. 47/1102 vol1 verse rows carry one, all verbatim.
- **Inline italics in body prose** — vol3 3705/3707. Multi-span rows are
  common (875 in vol1) and fully carried. Italics survive inside heading and
  verse rows (`apply_inline_italics` is block-type agnostic), including
  roman text between two spans and footnote markers inside a span.
- **Letter structure** — addressee, salutation, and signature all become
  `heading` rows with the dateline as prose, mirroring the reference's
  `h4`/`h5`/`p` shape. (What is lost is only the *level* distinction, §2.)

## Not a gap

- **Residual italic mismatches are mostly upstream Gutenberg corruption.**
  The reference HTML itself carries 73 raw unconverted underscores in vol1
  (e.g. line 21237 `9-1/2_d_. a line`), which never became `<i>`. The DB's
  `_d_` / `_s_` currency italics are therefore *correct* and the reference is
  wrong — **litdb is more faithful than the reference here.**
- **Vocab highlighting** (blue tinted words on the title page) is a
  deliberate reader feature gated on `works.vocab_highlight=1`, with no
  reference counterpart. Out of scope.

## Unverified — flagged, not concluded

- **vol6 (the index volume) is a scale risk that was not tested.** Its rows
  are enormous concatenated index entries — one row is ~96,625 chars with 332
  italic spans. `k_max` (`formatting.rs:798`) is uncapped, so loading vol6
  allocates 332 `TextTag`s and applies 332 ranges inside a single ~96KB
  paragraph — well outside the envelope the Phase-B K-pool was profiled
  against. Whether vol6 renders correctly, or fast, is **unknown**; it needs
  a headless load of `LoJ` div1=6.
- The stanza-gap no-op (§1) was established by code reading and confirmed
  against the data; its visual consequence (uniformly loose verse leading)
  has **not** been pixel-measured.

## Suggested order of work

1. **litdb** — emit the 37 empty verse rows (stanza breaks); fix vol1
   front-matter TOC tagging. Both upstream, both mechanical.
2. **linux-lit** — re-derive the stanza-gap condition so it survives the
   per-line model. Pointless before (1) lands, since there is nothing to
   detect.
3. **Decide** whether heading levels are worth a litdb schema column, and
   whether to keep centering headings the reference left-aligns. Both are
   product calls, not defects.
4. **Verify vol6** loads acceptably before treating the K-pool as settled.
