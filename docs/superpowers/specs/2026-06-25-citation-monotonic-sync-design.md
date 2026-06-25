# Citation-monotonic playback sync + 2H6 timestamp repair

**Date:** 2026-06-25
**Status:** Design approved

## Problem

In `2H6-Amb` Act 1, Scene 4 the Spirit speaks a prophecy (lines 1.4.34–39,
audio ~2468–2493 s). York then re-reads the same prophecy paper aloud
(lines 1.4.67–73). In `lit.db` the **re-read lines were assigned the first
occurrence's timestamps** (2467–2493) instead of their true later audio
(~2620–2663 s, the gap between line 64 @ 2619.74 and line 74 @ 2663.9). Two
distinct lines therefore claim the same audio instant for both media rows
(`media_id` 88 and 275).

`find_line_for_time` (`src/mpv/client.rs`) maps an audio position to a line
purely by `start_time` using `partition_point`. When two lines share a
bracketing `start_time`, it can resolve to the **later** occurrence (the
re-read, ~50 work lines ahead), yanking the cursor forward while the Spirit is
still speaking. This is the premature navigation observed in the screenshots
(`Tell me what fate awaits the Duke of Suffolk?` and `Let him shun castles;`
highlighted out of sequence).

The only existing defense is the `ABERRANT dist>50` guard in
`src/main.rs`'s `CursorSync` handler — a coarse magic number that *skips* the
event rather than choosing the correct line.

## Goals

1. **Robust sync:** the cursor must move through the work's line ordering
   (= citation order; `LineMap.work_to_buffer` is monotonic in
   `(div1, div2, line_in_div)`) the way `j`/`k`/`q`/`g`/`G` do — never jumping
   to a distant duplicate-timestamp line. Robust even if bad timestamps exist.
2. **Repair the data:** remove the impossible timestamps on the 2H6-Amb
   re-read lines so they stop acting as false sync anchors.

Additional goals (folded in after a codebase-wide citation audit — see
"Related citation-fragility findings" below):

3. **Gloss audio seek** must seek to the correct occurrence of a re-read verse
   line, not the first textual match (Part D).
4. **Jump-to-gloss-source** cursor must land on the in-passage occurrence, not a
   far duplicate (Part E).
5. **Resume/startup position** must survive a `lit.db` re-import without silently
   landing on the wrong speech (Part F).

Non-goals: re-measuring the re-read's true audio times (a later litdb pass);
`[`/`{` bookmark navigation (already monotonic, unaffected — out of scope);
`lookup_citation`/`verify_echo_citations` (finding #4 — input is free LLM text
with no authoritative id, structurally hard, deferred); the scene-jump
text-classifier fallback and vocab `line_index` (in-load-only / never persisted,
low risk, deferred).

## Part A — Citation-monotonic line selection (robust fix)

### Where

`find_line_for_time` in `src/mpv/client.rs`. Selection happens **inside the
mpv-client task** (option A1), which already owns the timestamp table. No new
channel message is needed.

### State

The mpv-client task gains a field `last_synced_work_idx: Option<usize>`,
updated every time it emits a `CursorSync(idx)` (the `idx` is already a work
index via `line_id_to_index`). This is the "where the cursor is, in citation
order" anchor.

### Selection rule (forward-only, nearest)

`find_line_for_time` currently picks a single `active` timestamp index by
`partition_point` + the gap-aware early-jump promotion, then maps it through
`line_id_to_index`. The change: when more than one timestamp entry brackets the
effective time (i.e. shares the resolved `start_time`, OR the gap-promotion is
ambiguous), build the **candidate set** of work indices and choose:

- the candidate with **minimum `|work_idx − last_synced_work_idx|`**,
- breaking ties **toward the forward candidate** (the larger index), so normal
  line-by-line progress always advances.

When `last_synced_work_idx` is `None` (first sync after load/seek), fall back to
the candidate nearest the timestamp's own position (current behavior — the
`partition_point` winner).

This selection:

- **Rejects the duplicate glitch:** the far re-read line is never "nearest" to
  the Spirit's line, so the cursor stays on the first occurrence.
- **Honors real backward seeks:** when the user seeks MPV back (`o`/`O`/Left or
  scrubbing), those keybinds already set 86400 s sync suppression and re-seek;
  on resume `last_synced_work_idx` may be stale, but "nearest in absolute
  work-index distance" lands the cursor where the audio actually is, because the
  near candidate beats the far one.

### Retire the magic guard

The `ABERRANT dist>50` check in `src/main.rs` is no longer load-bearing once
selection is citation-nearest. Keep a wide sanity clamp (e.g. reject a single
event that would move the cursor more than the whole work — defensive only),
but remove the `dist>50` early-`continue`. The principled choice replaces it.

### Tie/edge cases

- **No duplicates (normal case):** candidate set has one element; behavior is
  identical to today. Zero risk to existing works.
- **Gap-aware promotion** (`SYNC_GAP_PREROLL`) still applies; it operates on the
  chosen candidate, not before it.
- **Untimestamped next line:** unchanged — `pending_advance` in `main.rs` still
  carries the cursor across gaps.

## Part B — Repair 2H6-Amb data (B1: null the bad timestamps)

NULL out the impossible timestamps on the re-read lines for **both** media rows.
Affected `line_mapping_id`s (verified): 1175128, 1175130, 1175131, 1175132,
1175133, 1175134 (line_in_div 67, 69, 70, 71, 72, 73). Line 68 ("Suffolk?") and
65–66 (Latin) already have no timestamp.

Done as a small SQL migration in `~/utono/litdb`, **not** in linux-lit code:

```sql
DELETE FROM line_timestamps
WHERE line_mapping_id IN (1175128,1175130,1175131,1175132,1175133,1175134);
```

(Or set start/end NULL if a row must persist for another column — but these rows
exist only to hold the timestamp, so DELETE is clean.)

After repair, sync has no false anchor through the re-read; the lines display
normally and `pending_advance` carries the cursor across them. Part A makes sync
correct even if such data reappears, so B is belt-and-suspenders.

This must be applied to the live `~/utono/litdb/data/lit.db`. The snapshot cache
is `.txt`-mtime + db-fingerprint guarded, so it invalidates automatically on the
next launch.

## Part D — Gloss audio seek (`first_source_start_time`)

### The bug

`source_block_seek_time` → `first_source_start_time`
(`src/input/actions/gloss.rs:2144`) resolves a gloss source verse to an audio
time by scanning all work lines for `text.trim() == needle` and returning the
**first** match's `start_time`. A re-read/refrain line returns the wrong
occurrence's timestamp, so `a`/`space` on a glossed passage seeks audio to the
wrong place — the same bug class as Part A.

### The `-Amb` exception is now obsolete (verified 2026-06-25)

The existing text-first code was written because `-Amb` editions used to render
an aberrant, renumbered `.txt` whose `(div1,div2,line_in_div)` did not match the
base-numbered citation. **That is no longer true.** `-Amb` editions now render
the canonical folger-cleaned `.txt` — verified: `2H6` and `2H6-Amb` share the
same `works.text_file` AND identical `(div1,div2,line_in_div)` tuples for every
line (base_lid == amb_lid), and every `-Amb`/`-BBC`/`-DC` work has a
`text_file`. So a gloss's citation resolves **directly** in `-Amb` `work.lines`.

### The fix — resolve by citation/id, drop text matching

`source_block_seek_time` has the `gloss` in scope, which carries
`start_citation`. Resolve it to `(div1, div2, line_in_div)` and find the work
line by `position(|l| (l.div1,l.div2,l.line_in_div) == start)` (or by line
`id`). Read that line's `timestamp.start`. No text comparison — unambiguous, no
duplicate-occurrence hazard.

Keep a single, clearly-labelled text fallback ONLY for the genuinely citationless
case (a parsed `.txt`-only work with no `line_map`/citation, or a malformed
citation), so the function never silently returns nothing where it used to work.
Remove the `-Amb` rationale from the comment; cite the verification above.

Implementation: `source_block_seek_time` resolves the line by citation and reads
its start time directly; `first_source_start_time`'s text scan is retained only
as the citationless fallback (or deleted if no such path remains). Keep pure
helpers unit-testable.

## Part E — Jump-to-gloss-source cursor (`jump_to_gloss_source_start`)

### The bug

`jump_to_gloss_source_start` (`src/input/actions/gloss.rs:47`) finds the first
work line with `l.text.trim() == first_src`, falling back to the citation tuple
only if the text lookup fails entirely. A repeated first source line lands the
cursor on the wrong occurrence.

### The fix — resolve by citation first (the `-Amb` caveat is gone)

The function already receives `target` (the citation tuple). Since `-Amb`
editions now render the canonical `.txt` with matching citations (see Part D
verification), **invert the lookup order: resolve by the citation tuple first**
(`position(|l| (l.div1,l.div2,l.line_in_div) == t)`), using the text match only
as the citationless fallback. This removes the duplicate-occurrence hazard
entirely — the citation is unique.

Rewrite the comment at lines 36–42: the `-Amb` renumbering rationale no longer
holds; citation is now authoritative and primary, text is the
`.txt`-only/citationless fallback.

## Part F — Resume / startup position keyed on `line_mapping_id`

### The bug

`save_position` (`src/app/mod.rs:3470`) and the work-switch save
(`src/app/mod.rs:2408`) persist `state.current_line` — a **raw buffer-line
index** — into `config.work_positions[abbrev]`. On restore
(`src/app/mod.rs:2430` → applied at `:2581`) the raw index is used directly. If a
`lit.db` re-import or snapshot rebuild shifts buffer lines, the stored integer
now points at a **different citation line**, and the nearest-dialogue snap
(`:2960`) hides the drift by landing on the wrong speech.

Contrast: the `target_line_id` branch (`src/app/mod.rs:3027`) already does the
correct `id → position(|l| l.id == id) → work_to_buffer` remap. Resume is the
one path not using it.

### The fix — store and restore by id

- **Config schema:** change `work_positions` from `HashMap<String, usize>` to
  store a `line_mapping_id` (i64) instead of a buffer index. To stay
  backward-compatible with existing config files (which hold raw indices),
  either (a) add a new field `work_position_ids: HashMap<String, i64>` and read
  it preferentially, falling back to the legacy `work_positions` index when the
  id map has no entry; or (b) migrate on load. **Recommended: (a)** — additive,
  no destructive migration, legacy entries degrade to today's behavior.
- **Save:** at both save sites, resolve `current_line → buffer_to_work →
  work.lines[wi].id` and store that id under `work_position_ids[abbrev]`.
  (Keep writing the legacy index too during a transition window so a downgrade
  doesn't lose place.)
- **Restore:** in `display_work_at_with_prepared`, after `line_map` is built
  (i.e. alongside / reusing the `target_line_id` remap at `:3027`), if no
  explicit `target_line_id` was given, look up `work_position_ids[abbrev]` and
  remap it through the SAME `position(|l| l.id == saved_id) → work_to_buffer`
  path. Only fall back to the legacy raw `work_positions` index when the id is
  absent. `LIT_START_POS` continues to override (raw line, test-only).

This makes resume survive re-imports/repagination exactly as concordance jumps
already do.

### Note on ordering

The current code sets `state.current_line = saved_line` at `:2581` *before*
`line_map` exists. The id remap must run *after* the line map is built. Move the
resume application to the post-line-map point (near `:3027`) so both the
explicit-target and resume paths share one id→buffer resolution. Preserve the
existing `page_top_line`/canonical-spread behavior.

## Testing

### Unit (pure logic, `cargo test --bins`)

In `src/mpv/client.rs` tests for `find_line_for_time`:

- **Duplicate-timestamp, cursor near first:** two line ids share `start=2484`,
  `last_synced_work_idx` near the first occurrence → asserts the **first**
  (nearer) work index is returned, not the far duplicate.
- **Backward seek:** `last_synced_work_idx` far ahead, audio time brackets an
  earlier line → asserts the near earlier candidate is chosen.
- **No-duplicate regression:** existing `test_find_line_for_time*` cases must
  still pass unchanged (single-candidate path).

For Part D (`first_source_start_time` / citation-range filter): a case where the
verse text appears twice, with a citation range covering the SECOND occurrence,
asserts the second occurrence's `start_time` is returned (not the first).

For Part E (`jump_to_gloss_source_start`): unit-test the pure citation-ranked
selection helper (duplicate first-source line, range covering the later one →
later work index chosen).

For Part F: the config id↔index round-trip and the "legacy index when id absent"
fallback are pure and unit-testable in `src/config.rs` / the save/restore
helper; the full restore-after-reimport path is integration-level (user-run).

### Visual (user-run, per CLAUDE.md "do not run the app")

Reproduction for the user to eyeball:

1. Open `2H6-Amb`, navigate to Act 1, Scene 4 (the conjuring scene).
2. Start playback (Tab) from before the Spirit's prophecy
   ("By the eternal God…", ~2427 s).
3. Watch the highlight cross the Spirit's prophecy (lines 34–39).
4. **Expected:** the cursor advances one line at a time through the Spirit's
   lines and does **not** jump forward to the re-read prophecy
   (`Tell me what fate awaits…` / `Let him shun castles;`).

## Files

- `src/mpv/client.rs` — `find_line_for_time` selection + `last_synced_work_idx`
  state + unit tests.
- `src/main.rs` — remove the `ABERRANT dist>50` early-continue; keep a wide
  sanity clamp.
- `~/utono/litdb` — SQL migration nulling the 6 re-read timestamps (separate
  repo, separate commit).
- `src/input/actions/gloss.rs` — Part D (`first_source_start_time` /
  `source_block_seek_time` citation-range filter) + Part E
  (`jump_to_gloss_source_start` citation-ranked selection).
- `src/app/mod.rs` + `src/config.rs` — Part F (id-keyed resume:
  `work_position_ids`, save both sites, restore via the post-line-map id remap).

## Related citation-fragility findings (audit, 2026-06-25)

A codebase-wide audit (sync/seek, text-matching, keybind nav) found the app is
mostly citation-correct. The fragile surface folded into this spec: Part A
(sync, finding #1), Part D (gloss audio seek, finding #3 HIGH), Part E
(jump-to-gloss-source, finding #2 MEDIUM), Part F (resume restore, the one
persisted raw-buffer-line signal). Deferred (documented, not in this spec):
`lookup_citation`/`verify_echo_citations` (free LLM text, no id — finding #4),
scene-jump text-classifier fallback (in-load only), vocab `line_index` (rebuilt
each load, never persisted).
