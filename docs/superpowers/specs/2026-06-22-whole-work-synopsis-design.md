# Whole-Work Synopsis — Synopsis Overlay + Generation Script

**Date:** 2026-06-22
**Status:** Approved
**Branch:** `feat/qa-journal-overlay`

Add a **whole-work synopsis** to each Shakespeare work: a synopsis of the entire
play (or poem) that appears as the first position in the synopsis overlay, before
Act 1 Scene 1. Two pieces:

- **A. App** — surface a `(0,0)` synopsis row as the overlay's first position,
  reachable by the existing `p`/`n` cycling and new `Ctrl+p`/`Ctrl+n` aliases.
- **B. Data** — a batch script that generates one whole-work synopsis per target
  work and writes it to `scene_synopses` as `(0,0)`.

Both pieces reuse the existing scene-synopsis machinery. **Editing comes for
free** (see below); no new edit code.

---

## Background: how the synopsis overlay works today

- Scene synopses live in `scene_synopses (work_abbrev, div1, div2, synopsis,
  claude_model)`, `UNIQUE(work_abbrev, div1, div2)`.
- `load_synopses(conn, base_abbrev)` (called in `display_work`) loads every row
  for the work into `AppState.synopsis_cache: HashMap<(i64,i64), String>`, keyed
  on `base_work_abbrev` (so `-Amb` editions share synopses).
- `ordered_synopsis_scenes(&s)` returns the `(div1,div2)` keys that have a
  cached synopsis, in reading order.
- `cycle_synopsis(state, delta)` (bound to `p`/`n` in the synopsis overlay) steps
  through that ordered list with wraparound, setting
  `AppState.synopsis_overlay_scene` and rendering via `show_synopsis`.
- Amend (`A`) / edit (`E`) / undo (`U`) all operate on
  `s.synopsis_overlay_scene`; `save_synopsis` is an **upsert** on
  `(work_abbrev, div1, div2)` (`queries.rs:421` `ON CONFLICT ... DO UPDATE`).

## Why `(-2, 0)` is the whole-work key

**Correction (2026-06-22, after final review):** an earlier draft used `(0, 0)`,
on the assumption it was unused. It is NOT: `(0, 0)` is the **Prologue/Chorus**
slot — real `line_mapping` lines AND existing Prologue scene synopses — for H5,
H8, Luc, Rom, TNK, Tro. `(-1, *)` is likewise taken (the Induction for 2H4 and
Shr). Using `(0,0)` would overwrite those Prologue synopses on generation and
mislabel "Prologue" as "Whole work".

The minimum `div1` anywhere in `line_mapping` is `-1`, and no `scene_synopses`
row has `div1 < -1`. So **`(-2, 0)`** is below all real data and unused for every
work. The whole-work synopsis is stored at `(-2, 0)`, sorts before all real
scenes (including Prologues at `(0,0)` and Inductions at `(-1,*)`), and collides
with nothing. **Every `(0, 0)` / `(0,0)` reference elsewhere in this spec should
be read as `(-2, 0)`.**

---

## A. App changes

### A1. `ordered_synopsis_scenes` prepends `(0,0)`

If `synopsis_cache` contains `(0,0)`, it must be the **first** element of the
ordered list (before the chapter loop and before the scene loop). Add, at the top
of `ordered_synopsis_scenes` (`src/app.rs:6236`), before the `is_chapter_work`
branch:

```rust
fn ordered_synopsis_scenes(s: &AppState) -> Vec<(i64, i64)> {
    let mut keys = Vec::new();
    if s.synopsis_cache.contains_key(&(0, 0)) {
        keys.push((0, 0)); // whole-work synopsis sorts first
    }
    // ... existing chapter-work branch and scene loop, pushing onto `keys`
    //     instead of a fresh Vec ...
}
```

The existing two code paths (chapter-work and the per-line scene loop) must push
onto this same `keys` vec (the chapter branch currently `return`s a fresh `keys`;
fold its `(0,0)` prepend in too). Net effect: `(0,0)` first, then the existing
order unchanged.

### A2. `synopsis_label(s, 0, 0)` → "Whole work"

`synopsis_label` (`src/app.rs:5597`) must return `"Whole work"` for `(0,0)`. Add a
guard at the top:

```rust
pub fn synopsis_label(state: &AppState, div1: i64, div2: i64) -> String {
    if (div1, div2) == (0, 0) {
        return "Whole work".to_string();
    }
    // ... existing logic ...
}
```

This labels the overlay header for the whole-work position. (The work title is
already shown in the overlay context, so the label need not repeat it.)

### A3. `Ctrl+p` / `Ctrl+n` aliases in the synopsis overlay

Plain `p`/`n` already call `cycle_synopsis(-1)`/`cycle_synopsis(+1)` and will
reach `(0,0)` by wraparound (from Act 1 Scene 1, `p` lands on the whole-work
synopsis; `n` from it goes to Act 1 Scene 1). Per the user's request, also bind
`Ctrl+p`/`Ctrl+n` as aliases. In `handle_synopsis_overlay_key`
(`src/input/keymap.rs`), the existing arms are:

```rust
        "n" => { crate::app::cycle_synopsis(state, 1); true }
        "p" => { crate::app::cycle_synopsis(state, -1); true }
```

Add Ctrl variants (place the guarded arms before the plain ones so the modifier
matches):

```rust
        "n" if is_ctrl => { crate::app::cycle_synopsis(state, 1); true }
        "p" if is_ctrl => { crate::app::cycle_synopsis(state, -1); true }
        "n" => { crate::app::cycle_synopsis(state, 1); true }
        "p" => { crate::app::cycle_synopsis(state, -1); true }
```

`gg` is **unchanged** (the user retracted the gg request): it still jumps to the
first paragraph of the *current* synopsis text.

### A4. Editing the whole-work synopsis — no new code

Navigating to `(0,0)` sets `s.synopsis_overlay_scene = (0,0)` (via
`cycle_synopsis`). The amend/edit/undo path keys entirely on
`synopsis_overlay_scene`, and `save_synopsis` upserts on
`(work_abbrev, 0, 0)`. So `A` (amend), `E` (edit), and `U` (undo) operate on the
whole-work row exactly as they do on any scene synopsis — the same way glosses
and journal Q&A pages are editable. **No edit code is added; this is a
verification requirement, not an implementation one.**

### A5. Keybinds overlay

Update the Ctrl+/ keybinds overlay's synopsis-overlay key descriptions to mention
that `p`/`n` (and now `Ctrl+p`/`Ctrl+n`) cycle synopses including the whole-work
synopsis at the start. Run the `update-cairo-keybinds-overlay` cross-reference.

---

## B. Generation script

New script `~/utono/litdb/scripts/whole_work_synopses.py`, modeled on the
existing `chapter_synopses.py` (same `common.claude_api` batch helpers, same
`scene_synopses` upsert pattern).

### Targets

Generate one whole-work synopsis per **base** work (so `-Amb`/`-BBC`/`-KPR`/`-DC`
editions are NOT generated separately — they share the base row via
`base_work_abbrev`):

- **37 plays** (the canonical Shakespeare plays) **+ TNK** (Two Noble Kinsmen, 26
  scenes).
- **Narrative poems:** Ven (Venus and Adonis), Luc (The Rape of Lucrece), LC (A
  Lover's Complaint), PhT (The Phoenix and the Turtle).
- **Excluded:** Son (the sonnet collection — a sequence, not a single narrative),
  BenCrystalOP (an OP anthology, not one work).

The script takes a `--work ABBREV` argument (like `chapter_synopses.py`) and/or an
`--all` flag iterating the target list. The target list is hard-coded in the
script (base abbrevs).

### Generation method

- Ask Claude (model **opus**, i.e. `claude-opus-4-8`, via the `CLAUDE_MODEL` env
  or `--model`) for a whole-work synopsis using its **training knowledge** of the
  work. Send only the work's **title and author** — NOT the full text.
- System prompt: a concise instruction to write a synopsis of the entire work
  (the through-line of the plot / the poem's argument), prose, no spoiler gating
  (consistent with the journal's whole-play stance — the synopsis may discuss the
  ending), suitable as the "beginning" overview a reader sees before Act 1.
  Mirror the tone/length of existing scene synopses (a few paragraphs).
- For poems with no scene structure (Ven, LC, PhT), `(0,0)` becomes the work's
  sole synopsis — expected and useful.

### Persistence

Upsert each result into `scene_synopses` as `(base_abbrev, 0, 0, synopsis,
'claude-opus-4-8')`, exactly like `chapter_synopses.py`'s `apply_results`
(`INSERT ... ON CONFLICT(work_abbrev, div1, div2) DO UPDATE`). Back up
`scene_synopses` first (the script reuses `chapter_synopses.py`'s `backup_synopses`
pattern). Support `--dry-run` (print the prompts/targets, no API calls, no
writes) and `--resume BATCH_ID`.

### Provenance

The generated rows carry `claude_model = 'claude-opus-4-8'`, consistent with
`[[project_gloss_synopsis_model_provenance]]`. Once generated, the rows are
editable in-app (A4) like any synopsis.

---

## Out of scope

- No `SNAPSHOT_VERSION` bump (that governs `LineMap` serialization;
  `synopsis_cache` is loaded fresh from the DB each session).
- No changes to scene-synopsis generation or the scene navigation order beyond
  prepending `(0,0)`.
- Sonnet collection (Son) and BenCrystalOP anthology synopses.
- Audio/TTS for the whole-work synopsis is inherited from the existing synopsis
  TTS path (no special handling); not separately specced.
