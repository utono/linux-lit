# Overlay `\` cycle — failure modes

Frequency-ordered ledger for the plain-`\` segment-overlay rotation
(`src/input/actions/overlay_cycle.rs`). Read this BEFORE debugging a
"`\` doesn't go to X" report.

## 1. A stop opens nothing and the lap silently ends (fixed 2026-07-27)

**Tell.** `\` in an overlay puts you back in the READER instead of the next
overlay. The log shows `KEY: name=backslash … mode=GlossOverlay` immediately
followed by `KEY: name=backslash … mode=Reader`, with a
`CHAPTER_TOAST: … "No journal entry for this segment"` in between.

**Root cause.** `open_journal_scene` returned `()`. `cycle_from_gloss` called
it as its last statement, having ALREADY hidden the gloss overlay and restored
the reader position, so when the journal stop turned out to be empty it had no
way to know and no way to go back. Any scene without a journal entry made the
syntax stop unreachable by cycling — even when a syntax gloss existed.

**Fix.** `open_journal_scene` returns `bool`. `advance()` PROBES the candidate
stops (`gloss_covers_cursor` / `journal_has_content_at_cursor`) BEFORE any
teardown, and opens the first with content. When none has content the current
overlay is left untouched and a toast says
"Nothing else to cycle to for this passage".

## 2. Probing the live cursor instead of the lap anchor

**Tell.** The probe reports the wrong line — e.g. `cur=(5, 2, 437)` when the
lap started at 424 — so a narrow stop (a syntax gloss over one sentence) never
matches while a wide one (a reader-gloss over a whole speech) does.

**Root cause.** Opening the gloss stop moves `current_line` to the END of the
glossed passage. Probing `s.current_line` therefore asks about the wrong line
entirely.

**Fix.** Resolve from the lap anchor —
`gloss_return_pos.or(journal.return_pos)` — falling back to `current_line`
only when no overlay is open.

## 2b. Probing the anchor LINE instead of the displayed PASSAGE (fixed 2026-07-27)

**Tell.** `\` in an open gloss overlay toasts "Nothing else to cycle to for
this passage" even though the passage on screen visibly HAS another stop. The
probe log reports a `cur=` triple equal to the END of the displayed passage,
and a `spans=` list whose only entry does not contain it — e.g.
`cur=(5, 2, 437) passages=1 hit=false spans=[("Ant.5.2.424", "Ant.5.2.425")]`.

**Root cause.** Fixing #2 (anchor vs. live cursor) was not enough, because the
anchor itself is a single LINE. The stops sit on passages of different widths
(see #3), so the reader-gloss anchor — the cursor line where the lap started,
often the end of a long speech — falls outside a narrow syntax passage. The
question the cycle actually needs to ask is *"does another stop cover the
passage I am LOOKING AT"*, not *"…this one line"*.

**Fix.** When an overlay is open, `gloss_covers_cursor` tests inclusive SPAN
OVERLAP between `gloss_context`'s citation range and each candidate passage,
falling back to the line rule only when no overlay is open.
`try_open_syntax_gloss_at_cursor` gained the same fallback — **probe and open
must agree**, or the cycle advances into a stop that then opens nothing and
dumps the reader out (exactly the symptom of #1). Regression tests:
`overlay_cycle::tests::wider_displayed_passage_overlaps_a_narrower_syntax_gloss`
and `non_overlapping_passages_are_not_matched`.

**If you touch either function, change BOTH.** They encode one rule in two
places; a mismatch is silent and reproduces #1.

## 2c. The journal stop opens a Q&A from ANOTHER passage (fixed 2026-07-27)

**Tell.** `\` from the gloss stop opens a journal Q&A whose quoted passage is
plainly not the one on screen, often tens of lines away. The overlay footer
reads `Q&A 1 of N` with N > 1, and the log shows a `JOURNAL-PAGINATE:` line
with a long `heights=[…]` list plus `JOURNAL-TIMING: band_query=…`.

**Root cause.** `open_journal_scene` and `journal_has_content_at_cursor` each
had TWO tiers: a `scope='passage'` citation-span lookup, then a fallback to
`find_scene_band_pages`, which filters on `(div1, div2)` equality only —
the cursor's line appears nowhere in it. `ORDER BY timestamp ASC` then landed
page 0 on the chapter's OLDEST entry. Reported case: cursor at BH-Barrett line
979, entry anchored at 944, i.e. the band reached 35 lines away. The gloss stop
has no such tier, which is why only the journal stop misbehaved.

**Fix.** `open_journal_scene` takes a `JournalOpenScope`. The `\` cycle passes
`SegmentOnly` — tier 1 only, and a miss returns `false` SILENTLY (no toast, no
state mutation) so `advance()` skips the stop. `Ctrl+j` passes
`SegmentElseBand` and keeps the band fallback, which is its whole purpose.
The probe drops its fallback entirely. Both resolve the line through the shared
`lap_anchor_for` helper — see the warning under #2b: probe and open must agree.

**Consequence, deliberate.** `scope='scene'` entries are unreachable by `\`.
Reach them with `Ctrl+j` or the picker.

**`scope` is assigned by the BAND YOU ASKED FROM, not by content**
(`journal.rs` ~2841, the `save_journal_page`/`save_passage_page` match on
`JournalBand`, reading `s.journal_band` via `ask_claude`). The three producers:

- **Ctrl+j → ask** (scene band) → `JournalBand::Scene` → `scope='scene'`
- **Ctrl+a in the GLOSS overlay** → `open_passage_qa_float` (`gloss.rs:3301`)
  builds `JournalBand::Passage` from the glossed passage's own span → `'passage'`
- **Ctrl+a block-select in the reader** → `'passage'`

So two of the three paths already produce exactly what `\` needs; only the
Ctrl+j route stores `'scene'`, and it does so even when the citation names a
single line. If `\` skips the journal stop on a work whose entries look
segment-anchored, check `SELECT scope FROM journal_entries` before suspecting
the cycle code — the entries are probably `'scene'` from the Ctrl+j route, and
the fix is a lit.db re-scope (upstream), not a reader change. New entries made
via either Ctrl+a path need no migration.

## 3. The reader-gloss and syntax-gloss sit on DIFFERENT passages

Not a bug — expected, and the reason #2 and #2b bite. A syntax gloss is created from
an explicit narrower selection (visual-mode "Syntax", or `-`/`_` + `Return`),
so it gets its OWN `passages` row with its own span. Example (Ant 5.2):

- passage 17588 `Ant.5.2.424`–`437` → reader-gloss (whole speech)
- passage 17589 `Ant.5.2.424`–`425` → syntax-gloss (one sentence)

A cursor at 437 is inside the first and outside the second. `\` correctly
reports no syntax stop there; move to 424 and it appears. **Confirm the
cursor's `line_in_div` before concluding the cycle is broken** — the reader's
glossed-passage TINT covers the whole 424–437 span, so a screenshot showing a
highlighted "Most probable" does NOT mean the cursor is on line 424.

## 4. Verifying headlessly

The whole rotation drives under cage. Land on the work, step the cursor onto a
line covered by BOTH gloss types, then send `\` and watch the mode transitions
in the log. Screenshot byte-size alone distinguishes the stops (the syntax card
renders 1 page, the reader gloss 2). See
`docs/troubleshooting/headless-testing.md` for the harness rules — in
particular, use a FRESH `XDG_RUNTIME_DIR` so `grim` cannot capture the user's
own desktop.
