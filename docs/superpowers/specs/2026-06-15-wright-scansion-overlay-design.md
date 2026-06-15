# Wright scansion overlay — design

A reader-facing display overlay in linux-lit that shows George T. Wright's
metrical scansion (stress marks, line-type, caesura) on the current work's verse
lines, cycled by a keybind. Read-only over data the `wright-taxonomy` project
already wrote into lit.db.

- **Owning repo:** linux-lit (the GTK4 Rust e-reader).
- **Data source:** lit.db tables `line_meter` + `syllable_scan`, written by
  `~/utono/wright-taxonomy`. Keyed on `line_mapping.id`.
- **Handoff seed:** `~/utono/wright-taxonomy/docs/linux-lit-render-handoff.md`
  (the prior design intent that anticipated this feature).

## Goal

Press a key to cycle the current verse work's scansion overlay through three
states; the marks/labels appear inline on the verse lines without disturbing the
base text, cursor, audio sync, or audio highlight.

## Keybind & states

- **Key:** `Ctrl+Alt+i` (free — `i` = translation overlay, `Alt+i` = toggle
  translations; `Ctrl+Alt+i` unbound). Added in `display_bindings()` in
  `src/input/keymap_config.rs`, alongside `Alt+d` (ToggleDim). `KeyCombo::ctrl_alt`
  already exists (keymap_config.rs:42).
- **Cycle (one key, three states):** `Off → StressOnly → Full → Off`.
  - **Off** — plain text, exactly as today.
  - **StressOnly** — combining acute (U+0301) over stressed syllables only
    (`ictus = 1`); no breve. Plus line-type label and caesura marker. The clean
    default reading view.
  - **Full** — adds combining breve (U+0306) over unstressed syllables
    (`ictus = 0`). The close metrical-study view.
- Line-type label and caesura marker show in both `StressOnly` and `Full`.

## Critical invariant (load-bearing)

`syllable_scan.start_char/end_char` index the DB's `canonical_text`. The displayed
buffer line is read from `work.text_file` and cleaned (`clean_file_lines`,
`src/app.rs:3170`). **Marks are placed by re-finding each syllable's vowel in the
displayed line — never by trusting a stored char offset.** Stripping the combining
marks from a rendered line must reproduce the displayed line exactly.

Two facts were verified empirically (2026-06-14, against real lit.db + files), and
both are recorded so they are not re-derived:

1. **Buffer == canonical_text, but treat as coincidence.** For verse works the
   displayed line equals `canonical_text` byte-for-byte (TN 1.1 = 43/43, PL Book 1
   = 834/834). `clean_file_lines` drops whole lines but does not mutate matched
   verse content. This is an empirical coincidence of file prep, NOT a contract —
   so we still re-find (robust to a future re-import / prose-with-inline-verse).
2. **Audio highlight is strictly line-level.** `phrase_timestamps` appears nowhere
   in linux-lit. All timestamp/highlight ops key on `line.id` (= line_mapping.id)
   and track by buffer-line index (`src/input/timestamps.rs`). Therefore inserting
   zero-width intra-line combining marks CANNOT drift the audio highlight. This is
   what makes Approach A (below) safe.

## Architecture — units

Five small units, each one responsibility:

### 1. DB query — `src/db/queries.rs`

New `load_scansion_for_work(conn, abbrev) -> HashMap<i64, LineScansion>`. Joins
`line_meter` + `syllable_scan` on `line_id`, filtered to the work's lines, keyed by
`line_mapping.id`. Follows the existing `load_work` query idiom (queries.rs:38).
Lines with no `line_meter` row are simply absent from the map.

Real schema (confirmed against lit.db — the handoff doc's column list is correct;
an earlier exploration guessed a single `line_scansion` table with combined columns,
which is WRONG):

- `line_meter(line_id, syllable_count, nominal_feet, line_type, caesura_after,
  is_rhymed, confidence, source_note)`
- `syllable_scan(line_id, position, foot_index, ictus, foot_type, surface,
  start_char, end_char, phenomenon, is_extrametrical)` — ordered by `position`.

### 2. Scansion model — `src/db/models.rs`

```rust
pub struct LineScansion {
    pub line_type: String,
    pub caesura_after: Option<i32>,   // syllable position, or None
    pub syllables: Vec<ScanSyllable>,
}
pub struct ScanSyllable {
    pub surface: String,       // the syllable text as scanned
    pub ictus: i8,             // 1 strong, 0 weak — single source of truth
    pub is_extrametrical: bool,
}
```

No char offsets stored in the model — placement re-finds vowels on the displayed
line. `position` order is preserved as `Vec` order.

### 3. Mark renderer — new `src/scansion.rs` (pure, no DB, no GTK)

```rust
pub enum ScanLevel { Off, StressOnly, Full }
pub fn mark_line(displayed_line: &str, scan: &LineScansion, level: ScanLevel)
    -> MarkedLine;   // { text: String, label: String }
```

Port of wright-taxonomy's terminal-preview `mark_line` vowel-scan: walk the
syllables in order, locate each syllable's vowel within the displayed line (advance
a cursor across the line, matching `surface` where possible; fall back to the first
unconsumed vowel), insert the combining mark AFTER that vowel. At `StressOnly`, only
`ictus == 1` gets a mark; at `Full`, weak syllables get the breve. Insert the
caesura marker after the vowel of syllable `caesura_after`. The line-type string is
returned separately as `label` (styled via a tag, not inserted as word chars — see
render integration). This unit owns the strip-marks-reproduces-line invariant and is
unit-tested in isolation.

Defaults (tunable, flagged open in the handoff doc):
- **Caesura glyph:** ` ‖ ` (thin double bar) inserted at `caesura_after`.
- **Extrametrical syllables:** same breve as any weak syllable (no special case in
  v1). May get distinct styling later.

### 4. State + toggle — `src/app.rs` + `src/input/`

- `AppState.scansion_level: ScanLevel` (default `Off`).
- `AppState.scansion_data: HashMap<i64, LineScansion>` — loaded once per work
  (lazily on first toggle-on, or at work-load). Empty map for works with no
  scansion → overlay is a no-op (toast "No scansion for this work").
- `Action::CycleScansion` (`src/input/actions/mod.rs`).
- Dispatch arm (`src/input/keymap.rs`): advance `scansion_level` (Off→StressOnly→
  Full→Off), populate `scansion_data` if empty, then rebuild the buffer. Mirrors the
  `ToggleDim` arm's shape (borrow_mut, mutate flag, re-render, persist).
- Persist last level in `Config` (`src/config.rs`) so it survives restart
  (optional; default Off if absent). On work-load, if the persisted level is
  non-Off but the work has no scansion rows, the overlay silently stays visually
  Off (no toast — that's reserved for an explicit keypress); the persisted level
  is retained so a scanned work opened next still honors it.

### 5. Render integration — `src/app.rs::rebuild_buffer_text` (extends app.rs:3305)

**Approach A — rebuild the whole buffer with marks baked in.** After building
`filtered_contents` as today, if `scansion_level != Off`: for each buffer line that
maps to a work line (`line_map.buffer_to_work`) whose `line.id ∈ scansion_data`, run
`scansion::mark_line(displayed_line, &scan, level)` and substitute the marked text;
append nothing destructive — unmapped / un-scanned lines pass through unchanged. Then
`buffer.set_text(marked_contents)`. The `line_map` is unchanged (no lines added or
removed — only intra-line combining chars), so all line-level consumers are
untouched.

- **Whole-buffer** (not page-only): one `set_text` covers every page; verse works
  are small enough. Re-render triggers = `CycleScansion` + the existing
  work-load/rebuild path. (Page-turn does not rebuild text today; the marks are
  already in the buffer from the toggle, so page-turn needs no change.)
- **Line-type label styling:** the label is applied as a GTK TextTag over an
  appended label span AFTER `set_text` (a tag colors an existing substring without
  changing characters), OR the word-highlight passes are told to skip the label
  span. Chosen: append the label text + apply a dim `scansion-label` tag, and
  exclude the label column from the vocab word-scan range. This keeps lowercase
  line-type names (e.g. `regular`) from being mistaken for vocab words.

## Data flow

1. Work loads → `scansion_data` empty, `scansion_level` = persisted (default Off).
2. User presses `Ctrl+Alt+i` → dispatch advances level; if `scansion_data` empty,
   `load_scansion_for_work` populates it (toast + revert to Off if the work has no
   scansion rows).
3. `rebuild_buffer_text` runs with the new level → marked (or plain) buffer text →
   `set_text` → label tags applied → highlight refreshed via the existing
   `update_highlight` path.
4. Page-turns and audio sync proceed unchanged (line-level; marks invisible to
   them).

## Error handling

- **No scansion for work:** `scansion_data` empty → toast "No scansion for this
  work", force level back to Off, no buffer change.
- **DB open/query failure:** log via `crate::logging::log`, toast a soft error,
  leave level Off. Never panic; never block the reader.
- **Syllable surface not locatable in the displayed line:** that syllable gets no
  mark (renderer skips it) rather than mis-placing one. Logged at debug. The line
  still renders with whatever marks did resolve.
- **Un-mapped buffer line / no work line:** rendered plain (handoff rule: un-scanned
  lines render plain, no marks, no label).

## Testing

Pure-logic tests (`cargo test --bins`, no GUI):

- **`scansion::mark_line` unit tests** (the core):
  - StressOnly marks only `ictus==1`; Full marks both; Off is identity.
  - Strip-combining-marks reproduces the input displayed line exactly (the
    invariant) — for a known TN line.
  - Caesura marker placed at `caesura_after`; absent when `None`.
  - Surface-not-found → syllable skipped, no panic, other marks intact.
  - Vowel-less or punctuation-heavy syllable → falls back gracefully.
- **`load_scansion_for_work`** against a fixture DB (or a small in-memory table):
  returns the expected map; missing-line absent from map.
- **Vocab-highlight regression:** the word-highlight pass (`src/app.rs:~4638`)
  re-scans the marked buffer correctly — combining marks (non-word chars) don't
  split words; the line-type label isn't gold-highlighted.

Headless e2e (existing harness, `./scripts/e2e-env.sh`): toggle cycles the visible
overlay on a known verse work; a screenshot diff confirms marks appear/disappear.

## Out of scope (v1, YAGNI)

- Distinct styling for `is_extrametrical` (same breve for now).
- Surfacing `confidence` / `source_note` (e.g. dimming low-confidence lines).
- Prose / `unmetered` works (no marks; they render plain by construction).
- Per-syllable IPA (`ˈ`/`ˌ`) — the handoff notes it derives from `ictus`, but it
  is not part of this overlay.

## References

- Handoff seed: `~/utono/wright-taxonomy/docs/linux-lit-render-handoff.md`
- wright-taxonomy design: `~/utono/wright-taxonomy/docs/superpowers/specs/2026-06-14-wright-taxonomy-design.md`
- Taxonomy + notation reference:
  `~/utono/eleven-lit/docs/guides/wright_metrical_taxonomy.md`
- Verified facts (project memory): buffer==canonical_text (coincidence; re-find
  anyway); audio highlight is line-level (marks can't drift it).
