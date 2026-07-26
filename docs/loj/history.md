# LoJ — What Has Been Done (linux-lit + litdb)

_Written 2026-07-26 (US Central). Covers roughly 2026-07-22 → 2026-07-26._

**LoJ** = Boswell, _The Life of Samuel Johnson_ — `work_type='prose_book'`,
six volumes (`div1=1..6`) from Project Gutenberg, narrated by a single ~51-hour
audiobook (`media_files.id=233`,
`/home/mlj/Music/boswell-james/TheLifeofSamuelJohnson_ep6.m4b`).

LoJ is the work that forced richer prose rendering. It is prose that
**embeds verse and headings** — eclogues, epitaphs, Horace odes, letters —
which the reader previously had no mechanism to render as anything but flat
paragraphs.

Companion document: [`fidelity-eval.md`](fidelity-eval.md) — how the current
rendering compares against the Gutenberg reference.

---

## 1. The source: why Gutenberg HTML, not plain text

LoJ first entered lit.db on 2026-03-04 (`f26afe1`, custom parser) via
`paragraphize_text.py` over the plain Gutenberg **`.txt`**. That format
**destroys** exactly the structure LoJ needs. The cost, measured against the
DB at the time and quoted in the litdb spec: the `.txt` import *"collapsed
every verse block into a single space-joined `line_mapping` row and flattened
all headings into ordinary paragraphs… **0 of LoJ's 18,765 rows carry leading
whitespace** — indentation stripped. Verse line breaks are gone from the
data."*

`<br>` becomes a newline indistinguishable from a paragraph break, and
`&nbsp;` indentation becomes leading spaces indistinguishable from stray
whitespace.

A survey on 2026-07-23 (recorded in
`docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md`) checked
whether Gutenberg's richer formats carry a semantic verse layer to import.
**They do not:**

- **Italics = `<i>`** — 4,333 in vol1, mapping 1:1 to `_…_` in the DB.
- **Verse = no semantic markup.** No `.verse`/`.poem`/`.linegroup` class in
  either HTML or EPUB3. Verse is a `<p>` of `<br>`-delimited lines with
  `&nbsp;&nbsp;` indentation; speakers are `<h5>`; titles are `<p><i>…</i></p>`.
- **Plain `.txt` flattens** both signals away.

So the transition was **from importing flattened `.txt` to parsing the
Gutenberg HTML directly** — not to adopt a verse class (there is none), but to
recover the *typographic* signals (`<br>`, `&nbsp;`, `<i>`, `<h1>`–`<h5>`)
before they are flattened, and record them as explicit per-row metadata.

That metadata is the column **`line_mapping.block_type`**, added by litdb:
`'prose' | 'verse' | 'heading'` (plus `'blockquote'`, added later for Bleak
House — **LoJ has no blockquote rows**). Every non-LoJ row defaults to
`'prose'`, so the change is corpus-safe.

The governing principle, stated in the spec: *"emulate the EPUB formatting"
means reproducing the visual result — line-broken, indented block, centered
speaker/title — NOT importing a class that doesn't exist.*

The six volume HTMLs are Gutenberg #8918 / 9072 / 9180 / 10357 / 10451 /
11729 — the Birkbeck Hill unabridged edition, chosen because it matches the
51-hour audiobook. The explicit rule from the spec: **"Do not source from the
`.txt`."**

### A sub-transition: regex → LLM block classification

The first HTML parser classified blocks by regex and **over-detected verse**:
294 blocks classed verse in vol1 against only ~86 real poems. The rest were
line-broken bibliography entries, lists, letters, and quoted prose — all of
which *look* like verse structurally.

The conclusion (litdb `21a47e8`, 2026-07-24) was that this is not a fixable
regex: *"A regex cannot reliably separate 'genuine verse' from 'line-broken
prose list': it is a **semantic** judgment."* It was replaced by dual-mode LLM
classification (`6cae1fe`) that is **verdict-only** — the model returns a
label keyed by block index and never rewrites the canonical text, so
misclassification can never corrupt the source. `blockquote` is exempt from
the audit because it comes straight from the `<blockquote>` tag rather than a
heuristic.

### When Gutenberg boilerplate is removed

Boilerplate is dropped in **two separate places**, and the split matters
because only one of them is automatic.

**Automatic — at parse time.** `parse_loj_html.py` drops the whole
`<header id="pg-header">` and `<footer id="pg-footer">`: the machine-header
`Title:` / `Author:` / `Language:` / `Credits:` metadata and the
`*** START/END OF THE PROJECT GUTENBERG EBOOK ***` separators. A `_PG_META`
text guard catches any stray `<p><strong>Label</strong>: value` that escaped
the header. Per `wizard-gutenberg-html-blocks`: *"You should NOT see those as
rows — if you do, the edition's markup differs; fix `parse_loj_html.py`, don't
hand-trim."* Two regression tests lock this
(`test_modern_pg_header_footer_stripped`,
`test_pg_metadata_label_paragraph_dropped`).

**Manual — a slice in Phase 4.** What the parser deliberately does *not* trim
is front-cover matter: the `<h1>` title, `by`, author, `CONTENTS`/TOC
headings, and any `Produced by … / Distributed Proofreaders` **DP-credit**
lines. The DP credit sits *after* `</header>`, so it is outside the PG machine
block and the parser leaves it in — by design, not a bug. The operator
identifies `START_IDX` (the first real block, typically `PREFACE` /
`INTRODUCTION` / `CHAPTER I`) in Phase 2.1 and slices the serialized files to
it in Phase 4, keeping txt+meta 1:1.

**LoJ's actual state — verified:** zero rows match `PROJECT GUTENBERG` /
`Title:` / `Credits:`, and zero match an anchored DP-credit pattern
(`Produced by…`, `Distributed Proofreading Team`, `pgdp.net`). Both stages
came out clean. What *is* retained is the title page itself — rows `1.1`–`1.8`
are `_BOSWELL'S_`, `_LIFE OF JOHNSON_`, `EDITED BY`, `GEORGE BIRKBECK HILL,
D.C.L.`, … as `heading` rows. That is a deliberate keep, not leaked
boilerplate: it is the edition's own cover matter and it renders (see the
title-page underscore case in §4).

One caution when auditing this yourself: an unanchored
`LIKE '%Produced by%'` returns 21 LoJ rows, **all false positives** — body
prose containing the phrase. Anchor the pattern (`LIKE 'Produced by%'`) or you
will diagnose a boilerplate leak that does not exist.

There is a separate consumer-side note: the *reference* HTML keeps its footer
boilerplate, and it begins after the `*** END OF THE PROJECT GUTENBERG EBOOK
***` marker (vol1 line 29313). Any analysis that greps the reference file must
exclude everything past that line, or heading counts are inflated and five
`class="secthead"` divs show up that are boilerplate, not content. This bit
the first pass of the fidelity eval.

---

## 2. The data model reversed mid-effort: block → per-line

This is the single most important thing to understand about LoJ's history,
because shipped reader code straddles both models.

**First model (block granularity), 2026-07-24.** One `line_mapping` row per
verse *block*. A multi-line verse block was a SINGLE row whose
`canonical_text` carried **embedded `\n`** between lines, leading spaces
preserved as indent. Chosen deliberately: it aligned 1:1 with the whisperX
transcript's stanza-level segments. linux-lit shipped a full reader for this
shape (`e1d5c762`…`ec2dc056`, "Facet 2").

**Why it was reversed.** User testing showed the fatal consequence: with one
timestamp per block, every line of a stanza shared the *first* line's
timestamp. There was no per-line seek, and karaoke could only tint a whole
stanza at once. For a 51-hour audiobook that is not usable sync.

**Second model (per-line), 2026-07-24.** litdb reimported verse as **one row
per verse LINE**, each with its own timestamp. A mid-poem stanza break became
**one empty `verse` row**. `block_type` values unchanged.

The reader spec notes this was a **net simplification**: a verse row became
1:1 with a display line — the case the reader already handled for prose — and
per-line seek and per-line karaoke then fell out naturally.

A related decision was tried and **reverted** in the same window: expanding
the *cursor* tint to a whole verse block (`1b12eaa8` → reverted `fc936aa1`).
It floods a long eclogue and never appears to move. Conclusion recorded in the
spec: whole-block is a **karaoke** idea; the **cursor** tint stays line-by-line.

---

## 3. linux-lit — what shipped

Ordered; all merged to `master`.

**Facet 2 — verse-block rendering (`ec2dc056`, 2026-07-24).** Built for the
block model. Read `line_mapping.block_type` into `Line` (`e1d5c762`), added
block predicates + a `work_has_blocks` activation gate (`6e56f1cc`), a
verse-aware `LineMap` builder (`f753a27b`), `prepare_block_buffer` splitting
verse rows and capturing indent tiers (`340fac73`), and `apply_block_typography`
— per-tier verse indent (48/80/112px) + centered small-caps headings.

**Phase A — per-line verse finish (`5f3040d7`, 2026-07-24).** Adapted the
above to the reimported per-line data: retired the `\n`-split (`fc45aeb1`),
locked a 1:1 buffer↔work map for per-line seek (`d69b0ed9`), made an empty
verse row render as a stanza gap (`e15504bf`), moved verse karaoke onto the
line-by-line path (`2071562e`), and taught the prose cursor to skip empty
verse rows (`dfefeb4f`).

**Phase B — inline italics (`d316321d`, 2026-07-24).** `_word_` → real italic
spans with the delimiters hidden. Pipeline: `parse_italic_spans` +
`translate_offset` (`src/app/italics.rs`) → `apply_inline_italics` → an
`italic_offset_map` that karaoke's `apply_char_range_tag` consults so
source-derived char offsets still land correctly on lines whose displayed text
is shorter than its source (`e7cf280c`).

**The load-time regression and its fix (`11ba9c57`, 2026-07-24).** Phase B
initially `set_text`-ed the raw `_word_` text and then deleted LoJ's **45,113**
underscores one at a time — super-linear in GTK. `rebuild_buffer_text` took
**7,293 ms**. Fixed by stripping `_` in Rust *before* `set_text`
(`strip_italics_for_fill`) and applying italics through a **K-pool** of
reusable named tags (`inline-italic-pool-{k}`) instead of ~22,500 fresh
per-span tags. Result: **~1,080 ms**. Note the residual second is buffer-fill
and vocab (~700 ms), not italics.

**Karaoke instrumentation (2026-07-25).** Env-gated sweep tracing plus a
headless clause-width test (`cca96e38`), a whole-work accelerated sweep
(`8925e2f2`), and a fix so trace slices report *display* offsets on italic
lines (`7508cec2`).

**Shared block-type work.** `is_blockquote_line` (`be96a51e`) and inset
blockquote rendering (`93ffb999`) — infrastructure from the same effort, but
exercised by **Bleak House**, not LoJ.

---

## 4. Underscores: how orphans arise and what the reader does

Italics reach the DB as `_…_` in `canonical_text` — the direct
transliteration of the reference's `<i>…</i>`. LoJ carries **45,113**
underscores.

**Orphans are now repaired at both layers (2026-07-26).** Previously
`parse_italic_spans` bailed on an odd count and the row rendered **verbatim**,
putting a literal `_` on screen with no italics at all. That was deliberate —
the alternative of guessing where a span ends risked italicising the wrong
range — but it meant 131 LoJ rows, including the title page, shipped a visible
stray delimiter.

Both layers now give the orphan the sibling it lacks:

- **litdb, at import** (`pair_orphan_underscores` in `parse_loj_html.py`,
  commit `e4b0691`) — so no new row is imported with an odd count.
- **linux-lit, at render** (`repair_orphan` in `src/app/italics.rs`, commit
  `13fe03a2`) — so rows already in lit.db display correctly without a
  reimport.

The repair is deliberately **local**: it italicises only the word run adjacent
to the orphan and never spans a space, so a stray delimiter can never
italicise the rest of a line. That constraint comes straight from the data —
see the 1,535-char case below.

**Logging is retained by design.** `ITALIC_UNPAIRED` still fires per repaired
line, now reading `orphan repaired` rather than `rendered literal`:

```
ITALIC_UNPAIRED: line {i} odd `_` count, orphan repaired: "…"
```

That log is how you find works still carrying the defect upstream. A LoJ load
emits **131** of them, matching the 131 odd-count rows in lit.db exactly.

**131 LoJ rows are affected**, spread across volumes:

| div1 | rows | | by block_type | rows |
|---|---|---|---|---|
| 1 | 42 | | prose | 115 |
| 2 | 16 | | verse | 14 |
| 3 | 17 | | heading | 2 |
| 4 | 1 | | | |
| 6 | 55 | | **total** | **131** |

They have **two distinct causes**, which need different fixes:

### Cause A — a genuine span crossing a row boundary (import-shape artifact)

An italic span that wraps a source line break is split by the row-per-line
import: one row gets the opener, the next gets the closer. Each row alone is
odd, so **both** render verbatim with a stray `_`. Reference vol1 lines
4209–4210:

```html
Mulberry Tree, by Mr. Lovibond, the ingenious authour of <i>The Tears of<br>
```

becomes DB rows `1.805` (`…authour of _The Te…`) and `1.806`
(`Old-May-day_[301].`). The reference handles this natively because `<i>` can
span a `<br>`; a per-row model cannot express it.

The most visible instance is the **title page**, verified on screen: rows
`1.3` and `1.4` render as literal
`_INCLUDING BOSWELL'S JOURNAL OF A TOUR TO THE HEBRIDES` and
`AND JOHNSON'S DIARY OF A JOURNEY INTO NORTH WALES_`, neither italic. The
source is faithful — reference `<h3>`/`<h5>` at vol1 lines 91/93 — only the
per-line parser cannot span rows.

### Cause B — the Gutenberg HTML is itself corrupt

Some underscores were never converted to `<i>` by Gutenberg's own pipeline and
sit raw in the reference. Reference vol1 line 1568:

```html
<p id="id00241">In the same Magazine his Reviews_ are of the following Books:</p>
```

There is no opener anywhere — the corruption is upstream of litdb entirely,
and the import carries it faithfully. Vol1 alone has ~48 raw underscores in
its content HTML, including cases like `9-1/2_d_. a line` (line 21237) where
`_d_` is a *correct* currency italic the reference failed to convert.

That last point cuts the other way and is worth stating plainly: for the `_d_`
/ `_s_` currency italics, **litdb is more faithful than the reference HTML** —
the DB has the italic, the reference lost it.

### How each cause is handled

- **Cause A is rejoined at import.** litdb's pass 1 detects an unclosed opener
  on row *i* and a dangling closer on row *i+1* and reunites them, restoring
  the author's intent exactly. This only applies WITHIN a block — the title
  page's rows are separate `<h3>`/`<h5>` elements, not one `<p>` split by
  `<br>`, so they fall through to the local repair (`_INCLUDING_ …`,
  `… _WALES_`) rather than the parser guessing across element boundaries.
- **Cause B cannot be reconstructed, only contained.** Nothing can infer the
  intended span from corrupt source, so the orphan is paired against its own
  word run — one word wrongly italicised at worst, versus a whole line.
- **Under-italicising is the accepted trade.** A cross-row span that the
  reader repairs locally (rather than litdb rejoining at import) italicises
  only the adjacent word. That is deliberate: the opposite error —
  over-italicising to end-of-line — is what makes LoJ 1.1316 catastrophic.
- **The tell is unchanged:** grep the debug log for `ITALIC_UNPAIRED`. It now
  reports repairs rather than literal renders, so a non-zero count means the
  DB still holds odd rows and the work is a reimport candidate.

---

## 5. litdb — the `line_syntax` layer and phrase timestamps

Recent litdb work added a parse layer, **`line_syntax`**, sitting between
aligned line timestamps and karaoke phrase data, with skills
`build-line-syntax`, `backfill-phrase-timestamps`, `tune-phrase-grouping`, and
`fix-karaoke-gaps`.

`line_syntax` is a **spaCy dependency parse stored per token per line**, keyed
on `line_mapping_id` (`tok_i, start_char, end_char, pos, tag, dep, head_i,
lemma`). Char offsets are source offsets into `canonical_text` — the same space
`phrase_timestamps` uses, so consumers join without translation.

The motivation was an empirical dead end. Karaoke phrase grouping had been
tuned all day 2026-07-25 on punctuation and pause heuristics and kept failing,
because the constructions are lexically identical and differ only in
grammatical role. The measured wall, from the litdb spec: the gap before
`irresolute` in "…as he stands in the dark room, **irresolute**, makes him
start and say" (want a split) and the gap before `I` in "and I being wholly
free…" (must NOT split) are **both exactly 0.220 s**. No threshold separates
them — so the decision needs grammar, not timing.

Two properties worth stating precisely:

- **There is no separate "apply" step.** The break rule runs *inside*
  `build_phrase_timestamps.py`. To write new phrasing you rebuild the work.
- **The rule is purely additive** — it only ever ADDS a break at a set-off
  modifier, never removes or moves one, so approved phrasing cannot regress.
  Seven conditions must all hold (POS, dep, non-hedge lemma, comma-bracketed,
  ≥0.18 s pauses both sides, and governing nothing to either side); conditions
  6 and 7 were found by running the validator against real corpus data.

The model is `en_core_web_trf` in a separate venv — the `sm` model mis-tags
the motivating case. Commits in the arc include `df7d87f` (split at set-off
modifiers, wired into `main()`), `729ff59` (keep terminal punctuation with a
sentence-final modifier), `7cfc8ea` (skip the separating space so the next
phrase has no leading gap), and `d94e7ed` (apply syntax breaks inside repaired
clips).

**LoJ has zero `line_syntax` rows today.** The table currently holds
BH-Barrett / BH-Margolyes / BH-Vance (~434k tokens each), Ham-Arkangel, and TT.

Pipeline shape:

```
Gutenberg HTML
  → import (block_type + `_…_` italics preserved)  → line_mapping
  → alignment vs the whisperX transcript           → line_timestamps
  → line_syntax parse (clause/phrase boundaries)
  → phrase grouping                                → phrase_timestamps
  → linux-lit karaoke (per-phrase tint)
```

Details of the litdb side — exact scripts, the skills' order, and how
`line_syntax` is validated — are litdb's to document; this section records
only the shape and why linux-lit depends on it.

---

## 6. Building LoJ from scratch — the full skills chain

Every skill that would be involved in recreating LoJ end to end, in order,
across `~/utono/{runpod,whisper-transcript,litdb,linux-lit}`. Skills are
invoked as `/<name>`; each lives at `<repo>/.claude/skills/<name>/SKILL.md`.

### Stage 1 — Transcribe the audiobook (runpod / whisper-transcript)

Only needed if no WhisperX cache exists. **LoJ already has one** —
`…/whisperx-cache/TheLifeofSamuelJohnson_ep6.whisperX-transcript-large-v3.en.json`
(`large-v3`, the preferred model) — so a rebuild today would skip this stage.

| Skill | Repo | Role |
|---|---|---|
| `pod` | runpod | Pod lifecycle — create/start/stop/terminate, ensure packages, SSH |
| `pod-end-to-end` | runpod | Upload audio → whisperX/stable-ts → download results |
| `pod-clean` | runpod | Delete uploaded media from the pod to free workspace |
| `transcribe-audio` | whisper-transcript | Local transcription + forced alignment + status/deps |
| `download-models` | whisper-transcript | Fetch whisper model weights |
| `compare-transcription-models` | whisper-transcript | Pick a model tier before a long run |

(`transcribe-ambrose-all` is the Shakespeare batch driver — not part of a LoJ
build.)

### Stage 2 — Import the text (litdb)

| Skill | Role |
|---|---|
| **`wizard-loj-block-reimport`** | **The LoJ-specific driver.** Resumable multi-session pipeline: classifies blocks per volume (regex baseline + verse-audit subagent), imports all six volumes as ONE work, aligns the whole work against the audiobook at once |
| `wizard-gutenberg-html-blocks` | The single-work generalisation of the above — Gutenberg URL or local `.html` → typed blocks. Use for a one-volume work; LoJ's six-volume case uses the driver above |
| `wizard-gutenberg` | The **older, flat** path — `.txt` import with no verse/heading structure. This is what LoJ used *before* the HTML transition; kept for works with no verse to recover |
| `wizard-transcript` | For prose with **no** authoritative reference text (transcript-derived). Not LoJ's path — LoJ has Gutenberg |
| `wizard-shared` | Shared wizard internals the above call into |
| `create-parser` | Author a new per-work parser when an edition's markup doesn't fit |
| `associate-media` | Bind `media_files` to the work (LoJ → id 233) |
| `strip-italic-underscores` | Remove `_word_` markup for works that should NOT keep it. **Deliberately NOT run on LoJ** — LoJ renders italics, so its underscores are load-bearing (see §4) |

### Stage 3 — Align and time (litdb)

| Skill | Role |
|---|---|
| `troubleshoot-alignment` | Watch `/tmp/align-*.log` for failures, drift, bad timestamps, stable-ts blowups |
| `reset-timestamp` | Clear/repair a bad timestamp |
| `mark-chapters` | Set `is_track_mark=1` in `line_timestamps` for a prose work |

### Stage 4 — Karaoke phrase data (litdb)

Order matters here; see the hazards below.

| Skill | Role |
|---|---|
| `build-line-syntax` | spaCy dependency parse per line → `line_syntax`; plus a read-only validator and the gates that must pass before phrase rows are rewritten |
| `backfill-phrase-timestamps` | Build `phrase_timestamps` from the whisperX JSON, applying the `line_syntax` break rule |
| `tune-phrase-grouping` | Adjust grouping knobs (`PHRASE_MAX_SECONDS`, `SILENCE_GAP`, …) |
| `fix-karaoke-gaps` | Repair a line whose phrase coverage stops short (whisperX dropout) |

**Two hazards, both documented in the skills and worth restating:**

1. **Order is backfill → repair, never the reverse.** `fix-karaoke-gaps`
   states it as *"run `backfill-phrase-timestamps --force <WORK>` BEFORE
   this"*; `build-line-syntax` states the same rule from the other side — the
   backfill re-aligns from the whisperX JSON, which still contains the
   dropout, so running it *after* a repair recreates the smear and discards
   the fix. Measured on TT (2026-07-25): a rerun took the worst phrase from
   8.96 s to 29.65 s. `build_line_syntax.py` itself is safe — it writes only
   `line_syntax` and never touches `phrase_timestamps`.
2. **Verse gets no phrase timestamps.** `build_phrase_timestamps.py` skips
   verse wholesale — keyed on `works.work_type` *and* on `block_type='verse'`
   for verse quoted inside a prose work, **naming LoJ explicitly**
   (`build_phrase_timestamps.py:734-736`). So LoJ's 2,067 verse rows will
   have no phrase rows even after a clean run, and verse karaoke falls back to
   line granularity. This is by design, not a gap to fix — but it means
   "verse karaoke" in LoJ means per-*line* tint, not per-phrase.

### Stage 5 — Enrichment (litdb, optional)

`vocab-coverage`, `vocab-word`, `vocab-gloss`, `vocab-difficulty`,
`vocab-elided`, `vocab-rhetoric` (vocabulary layer — LoJ has
`works.vocab_highlight=1`); `synopses`, `paragraph-synopses`,
`gloss-division`, `gloss-rewrite`, `gloss-literary-device`, `tag-journal`
(study layer); `backup-db` / `backup-db-remote` before any destructive
reimport.

### Stage 6 — Verify in the reader (linux-lit)

| Skill | Role |
|---|---|
| `test-headless-navigation` | Screenshot the reader / inject keys / check for clipping without a monitor |
| `test-prose-navigation` | Page-turn correctness for prose works |
| `test-karaoke-highlight` | Headless real-MPV sweep with `LIT_DEBUG_KARAOKE`; reports the clause-width profile |
| `test-playback-sync` | Confirms sync turns the page at the right moment |
| `verify-overlay-ui` | On-screen invariants for overlay/journal/gloss surfaces |
| `debug-playback-sync`, `debug-navigation-sync`, `debug-sync-page-turn` | Diagnose a sync failure against a screenshot |
| `colorscheme`, `set-text-margins`, `set-cursor-offset` | Reader presentation tuning |

### Shortest path for LoJ *today*

Given the transcript, text, and media already exist, and only timing data is
missing, **stages 1 and 2 are already done** — start at alignment. The exact
command lives in `~/utono/litdb/data/loj-reimport/PICKUP-loj-align.md`:

1. Re-align: `map_gutenberg_timestamps.py LoJ 233 <large-v3 cache> --verify`,
   detached, **~70 min**, run **alone** (a concurrent lit.db writer is what
   killed the last attempt). Align PLAIN — **not** `--spoken-only`. Expect
   ~14,445 timestamps and gate 6.7 PASS. `troubleshoot-alignment` watches
   `/tmp/align-*.log`.
2. `/build-line-syntax LoJ` — before the phrase build, or breaks are SKIPPED.
3. `/backfill-phrase-timestamps LoJ 233` — likely with `--min-align-pct`.
4. `/fix-karaoke-gaps LoJ 233` — **last**, never before step 3.
5. `/test-karaoke-highlight` + `/test-prose-navigation` in linux-lit to verify
   on screen.

Note the ~70-minute figure supersedes any estimate scaled from the 51-hour
runtime — alignment reads the cached transcript, it does not re-transcribe.

---

## 7. Current state and what is blocked

**linux-lit:** `master`, clean. Verse rendering, headings, indent tiers, and
inline italics are all shipped and match the reference at or near 100%
(see [`fidelity-eval.md`](fidelity-eval.md)).

**LoJ timing data: gone.** LoJ currently has **zero `line_timestamps` and
zero `phrase_timestamps`**.

**It is important to state the cause correctly: alignment was not "never run"
— it succeeded, twice, and the results were then destroyed.** Tracing litdb's
backup chain shows LoJ reaching a fully-timed state and losing it:

| backup | rows | line_ts | phrase_ts |
|---|---|---|---|
| loj-trial 07-24 08:42 (old `.txt` import) | 18,765 | 10,832 | 109,201 |
| loj-phrasebackfill 07-24 22:27 | 21,543 | 14,449 | 0 |
| TT-realign 07-25 06:26 | 21,543 | 14,449 | **84,913** |
| loj-boilerplate 07-25 06:51 | 21,543 | 14,449 | 84,913 |
| loj-spoken 07-25 08:09 | 21,520 | 14,445 | **0** |
| **current** | **21,520** | **0** | **0** |

Two separate losses. First, the boilerplate reimport (`5c3e859`) rebuilt
`line_mapping` (21,543 → 21,520 rows), which **by design** deletes the FK'd
`phrase_timestamps` — the wizard states they "are deleted here and NOT
auto-rebuilt." Second, the 14,445 line timestamps from the post-boilerplate
re-align were lost separately.

litdb records the immediate cause in
`data/loj-reimport/PICKUP-loj-align.md` (2026-07-25):

> LoJ is fully reimported (21,520 rows, 6 volumes, 0 Gutenberg boilerplate)
> but has **0 timestamps** — the re-align was locked out by a concurrent DB
> writer and must be re-run.

**So nothing is blocking alignment.** It is a ~70-minute job that must run
alone with no other lit.db writer. The PICKUP note carries the exact command;
its key constraints are: align **PLAIN** (not `--spoken-only`), expect ~14,445
timestamps and gate 6.7 PASS. Corroborating evidence that the earlier run was
sound: `loj-verify-report.txt` (07-25 07:49) records 14,445 timestamps, gate
6.7 PASS, and clean non-overlapping per-volume windows (vol1 0–10.2 h, vol2
10.2–25.7 h, vol3 25.7–38.7 h, vol4 38.7–50.9 h).

The practical order:

1. **litdb — re-align.** `map_gutenberg_timestamps.py LoJ 233 <large-v3 cache>
   --verify`, detached, ~70 min, run alone. The WhisperX cache is already on
   disk, so nothing needs re-transcribing.
2. **litdb — `build-line-syntax LoJ`** *before* the phrase build, or the
   backfill reports `Syntax breaks: SKIPPED`.
3. **litdb — `backfill-phrase-timestamps LoJ 233`.** Now permitted for
   `prose_book` (`6f71a46`); likely needs `--min-align-pct` (`6676293`) since
   LoJ is ~62% unnarrated apparatus.
4. **litdb — `fix-karaoke-gaps LoJ 233`** last (see the ordering hazard in §6).
5. **linux-lit — Phase C.** Per an investigation on 2026-07-25 this needs *no*
   mechanism change: verse karaoke already works (the paint path is
   class-agnostic and italic-aware, and LoJ is `prose_book` so `is_prose()`
   already defaults it to karaoke). Phase C reduces to deleting the now-dead
   `block_buffer_range`, on-screen-verifying verse karaoke, and closing the
   Phase-A carry-forward holes.

**Known-expected, do not chase:** gate 6.7b fails for LoJ with ~5,955
script-distant collisions. This is pre-existing (the pre-reimport backup fails
it identically) and intrinsic to SequenceMatcher on a ~62%-apparatus text.
Only gate 6.7 must pass. Likewise, vols 5–6 being near-untimed is correct —
the audio narrates roughly print vols 1–4 plus the tail of vol5, not the
scholarly back-matter.

**Two data gaps found by the fidelity eval** should be folded into step 1
while the importer is already being touched — both upstream:

- **37 missing empty verse rows** (stanza breaks) corpus-wide, 29 in vol1
  alone. The per-line spec's premise that "Vol1 has zero of these" is
  **factually wrong**; vol1 has the most of any volume.
- **vol1 TOC / List-of-Illustrations entries mis-tagged `prose`** where the
  reference marks them `<h5>` (~15 contiguous rows). Vol1-specific — vols 2
  and 5 capture headings at 100%.

And one reader-side defect the eval surfaced independently: the stanza-gap
condition in `formatting.rs:737` became a **no-op** under the per-line model,
so restoring those 37 rows would not by itself produce visible stanza breaks.
