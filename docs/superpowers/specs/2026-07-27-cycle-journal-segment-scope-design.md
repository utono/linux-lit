# `\` cycle — the journal stop is scoped to the cursor's segment

## Purpose

The `\` overlay cycle must show, at every stop, material about **the segment
the cursor is on**. The gloss and syntax stops already do. The journal stop
does not: when no passage Q&A covers the cursor, it falls back to the whole
scene band and opens whichever chapter Q&A happens to sort first.

Remove that fallback from the `\` lap. When the cursor's segment has no passage
Q&A of its own, the journal stop reports no content and the lap skips it —
exactly the shape `gloss_covers_cursor` already has.

## The reported failure

BH-Barrett, chapter 10, cursor in the "It is quite dark now, and the gas-lamps
have acquired their full effect…" paragraph.

- `\` → gloss overlay, showing **that paragraph**. Correct.
- `\` → journal overlay, showing a Q&A about "a little out at elbows", quoting
  "Here, beneath the painted ceiling, with foreshortened Allegory…" — a
  different passage entirely. Footer: `Q&A 1 of 3`.

Confirmed in `linux-lit-dev.log` (16:23): `JOURNAL-PAGINATE: … heights=[332,
52, 52, 80, 80, 164, 248, 136, 164, 136, 192, 164]` — twelve blocks, a whole
band — and the timing line names the query, `band_query=3ms`.

## Root cause

`journal_has_content_at_cursor` (`src/input/actions/journal.rs:1240`) and
`open_journal_scene` (`:1289`) both run a two-tier lookup:

1. **Cursor hit** — `find_journal_page_for_line` (`src/db/journal.rs:596`),
   a `scope='passage'` entry whose parsed citation span contains the exact
   `(div1, div2, line_in_div)`. Properly segment-scoped.
2. **Fallback** — `find_scene_band_pages` (`src/db/journal.rs:245`):

   ```sql
   WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3
     AND scope IN ('scene', 'passage')
   ORDER BY timestamp ASC, id ASC
   ```

Tier 2 filters on `(div1, div2)` equality only. The cursor's line appears
nowhere in it. `ORDER BY timestamp ASC` then lands page 0 on the chapter's
oldest Q&A — the "out at elbows" entry.

The gloss side has no tier 2. `gloss_covers_cursor` (`src/input/actions/
gloss.rs:3358`) tests inclusive span overlap against the displayed passage and
returns false when nothing overlaps. That asymmetry is the bug.

### Second defect, same area

The two functions disagree about which line they are asking about:

- `journal_has_content_at_cursor:1253` resolves from the **lap anchor** —
  `gloss_return_pos.or(journal.return_pos).unwrap_or(current_line)`.
- `open_journal_scene:1306` uses raw `s.current_line`.

Opening the gloss stop moves the cursor to the end of the glossed passage, so
arriving at the journal stop via `\` probes one line and opens on another. This
is the same anchor bug the gloss side fixed on 2026-07-27; the journal side
never got the fix.

## Design

### 1. The probe becomes span-only

Delete the scene-band fallback at `journal.rs:1276-1280`.
`journal_has_content_at_cursor` returns true only on a tier-1 hit.

### 2. `open_journal_scene` takes a scope

The scene-band path cannot simply be deleted — `Ctrl+j` (`toggle_overlay`'s
open half) is the other caller and must keep browsing the chapter band. The
function grows a scope parameter:

- **Segment-only** (the `\` cycle): tier 1 only. A miss returns `false` with
  **no toast and no state mutation**, so `advance()` skips the stop. The
  existing "No journal entry for this segment" toast belongs to the band path
  and must not fire here — `advance()` owns the all-empty message.
- **Segment, else band** (`Ctrl+j`): today's behavior, unchanged.

### 3. Both paths resolve from the lap anchor

`open_journal_scene`'s tier-1 probe switches from `s.current_line` to the same
anchor expression the probe uses. One helper, called by both, so they cannot
drift again.

## Consequences accepted

- **On a segment with a gloss but no passage Q&A, `\` skips the journal.** The
  lap runs gloss → syntax, or wraps straight back to gloss.
- **`scope='scene'` entries leave the `\` lap entirely.** They carry no
  citation span, so they can never satisfy a segment-scoped probe. They stay
  reachable through `Ctrl+j` and the journal picker, which is where deliberate
  chapter-level browsing belongs.
- In the reported case, the chapter-10 "out at elbows" Q&A no longer appears
  from the "It is quite dark now…" paragraph. That is the fix, not a
  regression.

## Out of scope

- `Ctrl+j`, the journal picker, and `Ctrl+n/p` band traversal are untouched.
- The `find_scene_band_pages` query itself is unchanged — only which caller
  reaches it.
- No keybind moves, so no `keymap.json` change and no keycap-strip change.

### One legend correction rides along

`src/ui/keybinds_overlay.rs:319` describes `\` as "gloss → journal Q&A → back
to reader, no wrap; segment fixed at lap entry". That went stale on
2026-07-27, when the lap gained the syntax stop and began wrapping
indefinitely with Escape as the exit. Corrected in this change to match the
shipped rotation, since this change touches the cycle's semantics.

The two overlay legends that mention `\` — `gloss_keybinds_overlay.rs:21` and
the journal equivalent — say "skips empty", which stays accurate: this change
widens what counts as empty without changing the skip rule.

## Testing

**Unit** (`overlay_cycle.rs` already holds pure rotation tests):

- A journal stop with no covering passage Q&A is skipped, not opened.
- The anchor helper returns `gloss_return_pos` when an overlay is open and
  `current_line` when none is.

**On-screen** — the acceptance criterion, per the visible-surface rule:

Land in reader mode on BH-Barrett ch. 10 in the "It is quite dark now…"
paragraph and press `\` twice. The second press must **not** open a Q&A about
another passage. Verified headlessly via cage, then confirmed on the real
renderer.

## Files

- `src/input/actions/journal.rs` — `journal_has_content_at_cursor`,
  `open_journal_scene`, the shared anchor helper
- `src/input/actions/overlay_cycle.rs` — `Stop::open` passes the segment-only
  scope
