# Per-gloss voice set + age-aware default voice — design

**Status:** design (not implemented). Builds on the merged
[character-gender → gendered TTS voice](./2026-06-08-character-gender-design.md)
work (kept, not retired) and the gloss-IPA TTS pipeline.

## Problem

Gloss audio is currently synthesized in a single voice chosen by the speaking
character's gender (male/female × verse/prose, the four custom `eleven_v3`
voices). Two limitations:

1. **No way to hear a gloss in a different/specific voice.** The voice is forced
   by the character's gender — you can't audition the same gloss in another voice,
   and a wrong-gender curation produces a wrong voice with no in-app override.
2. **The default ignores age.** A 14-year-old (Juliet) and an 80-year-old (Lear)
   of the same gender get the identical voice. The default should match the
   character's *age* as well as gender.

This spec adds: a **per-gloss voice set** (zero, one, or more voices a user
associates with a gloss, cycled at playback), and an **age-aware default**
((gender, age) → voice from a voice catalog) that replaces the gender-only
default when the gloss has no associated voices.

## Decisions

- **Granularity: whole gloss.** A gloss has one voice set; every block (verse +
  explication) plays in any of the gloss's voices. (`gloss_voices(gloss_id, …)`.)
- **Precedence at playback:** the gloss's *associated voices* (if any) override
  the *character-based default*. The default is never removed — it is what plays
  when the set is empty (the common case).
- **Active voice is session-only.** Which associated voice plays next is an
  in-memory index, reset when the gloss changes; not persisted.
- **Per-voice audio cache.** Each voice's synthesized audio for a block is cached
  separately, so switching back to a heard voice is instant.
- **Default = (gender, age) → voice.** Character age is an LLM-curated integer;
  voices carry an inclusive age range; resolution is containment, then nearest
  same-gender band.
- **Nothing is retired.** The gender machinery stays; `characters` gains an `age`
  column; `voice_for` is subsumed by the age-aware `resolve_default_voice` in
  Phase 2.

## Build phases

- **Phase 1 — per-gloss voice set (standalone, useful immediately).** Association
  + cycling + per-voice cache. The default branch stays the merged gender-only
  `voice_for`. Ships and works on today's four voices.
- **Phase 2 — age-aware default.** `voice_catalog` + `characters.age` +
  `resolve_default_voice` (containment → nearest band), seeded by the four
  existing voices so it degrades to current behavior until narrower bands are
  added. Swaps the default branch from `voice_for` to `resolve_default_voice`.
  The curation tool (`curate_genders.py` → `curate_characters.py`) gains age.

---

## Phase 1

### 1.1 Schema

**New `gloss_voices` table** — the gloss↔voices association:

```sql
CREATE TABLE IF NOT EXISTS gloss_voices (
  gloss_id  INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
  voice_id  TEXT NOT NULL,
  model_id  TEXT NOT NULL,
  position  INTEGER NOT NULL,   -- cycle order; 0,1,2…
  PRIMARY KEY (gloss_id, voice_id)
);
```

- `position` gives a stable cycle order; the active voice is an index into the
  position-ordered list.
- `model_id` is stored because synthesis needs it (most voices use a default
  model; the four custom narration voices use `eleven_v3`).
- Created via `ensure_gloss_voices_table` alongside the existing `ensure_*` DDL
  in `src/db/queries.rs`, wired into the startup `BOOKMARKS_INIT.call_once`.

**`gloss_audio` key migration — add `voice_id`:**

The current cache keys on `(gloss_id, kind, paragraph_index)` (UNIQUE), so a
second voice overwrites the first. Extend the key to include `voice_id`:

- `UNIQUE(gloss_id, kind, paragraph_index)` → `UNIQUE(gloss_id, kind,
  paragraph_index, voice_id)`.
- Migrate via the existing `GLOSS_AUDIO_COLUMNS`-shared-const + legacy-rebuild
  pattern the table already uses (bump the migration; copy old rows — each keeps
  its existing `voice_id` as the new key component, so no data loss).
- `find_gloss_audio(conn, gloss_id, kind, index, voice_id)` gains a `voice_id`
  arg and filters on it.
- The cached **filename** gains the voice id so two voices' files for one block
  don't collide: stem `source-<index>-<voice_id>.mp3` / `<index>-<voice_id>.mp3`
  (in `play_block_tts` / `gloss_audio_dir`).

### 1.2 Association/removal helpers (`src/db/queries.rs`)

- `get_gloss_voices(conn, gloss_id) -> Vec<(voice_id, model_id)>` — ordered by
  `position`.
- `toggle_gloss_voice(conn, gloss_id, voice_id, model_id) -> bool` — if present,
  delete (returns `false` = removed); else insert at `MAX(position)+1` (returns
  `true` = added).

### 1.3 Active voice state

`AppState.gloss_active_voice: usize` (default 0). Reset to 0 whenever the gloss
changes — the same points that already rebuild gloss context: gloss open, gloss
nav (`Ctrl+n`/`p`), passage nav (`Alt+n`/`p`).

### 1.4 Playback resolution (`play_block_tts`)

Replace the current gender-only block with this precedence:

1. `voices = get_gloss_voices(conn, gloss_id)`.
2. **If `voices` non-empty:** active = `voices[gloss_active_voice.min(len-1)]` →
   `(voice_id, model_id)`.
3. **Else (empty):** the default — Phase 1: `voice_for(gender, is_verse)` (the
   merged gender path: `get_character_gender` + `voice_for`, unchanged). Phase 2
   swaps this single call to `resolve_default_voice` (§2.3).
4. Cache lookup/synth/402-fallback/cache-write downstream are unchanged except
   that `find_gloss_audio`/`save_gloss_audio` now thread `voice_id` (the
   *actually-used* voice — Alice on a 402 fallback) and the filename includes it.

The global `config.elevenlabs_voice_id` is **not** used in gloss playback (the
default always resolves via gender). Alice remains the synth-layer 402 fallback.

### 1.5 UI — voice picker reuse + keys

Reuse the existing `VoicePicker` (`src/ui/voice_picker.rs`) + `list_voices()`
fetch. The picker is generic; `confirm_voice_picker` is currently hardwired to
write `config.elevenlabs_voice_id` and return to `Settings`. Add a context flag:

- `AppState.voice_picker_origin: enum { Settings, GlossOverlay }` (mirrors the
  existing `gloss_picker_from_overlay` pattern).
- `confirm_voice_picker`: `Settings` → existing behavior; `GlossOverlay` →
  `toggle_gloss_voice(current gloss, selected)` + return to `InputMode::GlossOverlay`.
- The picker shows a "✓ associated" badge on voices already in the current
  gloss's set (so `v` is a clear add/remove toggle).

**Keys in `handle_gloss_key` (`src/input/keymap.rs`)** — both `v`/`V` are free:

- **`v`** → open the voice picker with `voice_picker_origin = GlossOverlay`,
  `InputMode::VoicePicker`. Confirm toggles membership; toast "Associated: <name>"
  / "Removed: <name>".
- **`V`** → `cycle_active_voice`: advance `gloss_active_voice` (wrap) over the
  associated set; toast the now-active voice name ("Voice: <name>"). Empty set →
  toast "no voices associated — default in use", no change. Does **not** auto-play.

**Ctrl+/ keybinds overlay** (`src/ui/keybinds_overlay.rs`) must be updated for
`v`/`V` (per CLAUDE.md and the `update-cairo-keybinds-overlay` skill).

### 1.6 Phase 1 testability

Pure/DB-testable: `get_gloss_voices`, `toggle_gloss_voice` (add→remove round
trip, position ordering), the migrated `find_gloss_audio`/`save_gloss_audio`
voice-keyed round trip (two voices coexist for one block). GTK/live-TTS parts
(picker open, cycle toast, actual audio in the chosen voice) are user-verified.

---

## Phase 2 — age-aware default

### 2.1 Voice catalog

```sql
CREATE TABLE IF NOT EXISTS voice_catalog (
  voice_id  TEXT NOT NULL,
  model_id  TEXT NOT NULL,
  gender    TEXT NOT NULL,        -- 'male' | 'female'
  age_min   INTEGER NOT NULL,     -- inclusive
  age_max   INTEGER NOT NULL,     -- inclusive
  role      TEXT NOT NULL,        -- 'verse' | 'prose'
  label     TEXT,                 -- e.g. 'Willa OP — young female verse'
  PRIMARY KEY (voice_id, role)
);
```

- Populated with the user's gender+age-band voice sets (each set = a `verse` row
  + a `prose` row sharing gender + age range).
- **Seed rows** for the existing four voices with a broad range (`age_min=0`,
  `age_max=120`) so Phase 2 has a working catalog from day one and degrades to
  today's behavior until narrower bands are added. Seeded by
  `ensure_voice_catalog_table` (idempotent `INSERT OR IGNORE` of the four).

### 2.2 `characters.age`

```sql
ALTER TABLE characters ADD COLUMN age INTEGER;   -- nullable; LLM-curated
```

`curate_genders.py` → renamed `curate_characters.py`: asks Claude for each
speaker's gender AND an approximate integer age. JSON value becomes
`{"gender": "...", "age": N}` per key; the script writes both columns. Missing
age → NULL.

### 2.3 `resolve_default_voice`

`resolve_default_voice(conn, work_abbrev, speaker, is_verse) -> (voice_id, model_id)`
(in `src/db/queries.rs`, replacing the `voice_for` call in `play_block_tts`'s
default branch):

1. Look up `(gender, age)` from `characters` (the existing `get_character_gender`
   generalizes to also read `age`). Missing/multi-speaker/unknown gender → `male`;
   missing age → a default constant `DEFAULT_AGE = 40`.
2. `role = if is_verse { "verse" } else { "prose" }`.
3. **Containment:** `SELECT voice_id, model_id FROM voice_catalog WHERE
   gender=?1 AND role=?2 AND ?3 BETWEEN age_min AND age_max ORDER BY
   (age_max-age_min) ASC LIMIT 1` (narrowest containing band wins).
4. **Else nearest:** same-gender/role voice minimizing distance from `age` to the
   band `[age_min, age_max]` (distance 0 if contained; else `age - age_max` or
   `age_min - age`).
5. **Else** (no same-gender voice in catalog — impossible given the seed rows):
   fall to `voice_for(gender, is_verse)` (the legacy constants) as a last resort.

### 2.4 Swap the default branch

`play_block_tts`'s empty-set branch (§1.4 step 3) changes its single call from
`voice_for(gender, is_verse)` to `resolve_default_voice(conn, work_abbrev,
speaker, is_verse)`. The associated-voices override path (§1.4 step 2) is
unchanged. So Phase 2 touches only the default branch.

### 2.5 Phase 2 testability

Pure/DB-testable: `resolve_default_voice` against a seeded `voice_catalog` +
`characters` — containment (age inside a band), nearest (age outside all bands →
closest), gender fallback (unknown → male), age fallback (NULL → DEFAULT_AGE),
verse vs prose role. The curation script's age field is user-run (live API).

---

## Retirement / cleanup

Nothing from the gender feature is deleted:

- `Gender`, `get_character_gender`, the four voice constants, `OP_MODEL_ID`,
  `voice_for` all stay. `voice_for` remains the Phase-1 default and the Phase-2
  last-resort fallback (§2.3.5).
- `characters` gains `age`; the curation tool is renamed and extended, not
  removed.
- The global `config.elevenlabs_voice_id` / `elevenlabs_model_id` are no longer
  read in gloss playback but stay for the settings voice-picker (which still
  writes them) — they are not part of this feature's playback path.

## Key files

- DDL + helpers: `src/db/queries.rs` (`ensure_gloss_voices_table`,
  `ensure_voice_catalog_table`, the `gloss_audio` migration, `get_gloss_voices`,
  `toggle_gloss_voice`, `resolve_default_voice`; `get_character_gender` extended
  for age).
- Playback: `src/input/actions/gloss.rs` (`play_block_tts`, `cycle_active_voice`,
  the per-voice cache threading + filename).
- Picker reuse: `src/ui/voice_picker.rs` (associated-badge), `src/input/actions/
  settings.rs` (`confirm_voice_picker` origin branch), `src/input/keymap.rs`
  (`handle_gloss_key` `v`/`V`, `InputMode::VoicePicker` routing).
- State: `src/app.rs` (`gloss_active_voice`, `voice_picker_origin`; startup DDL).
- Overlay docs: `src/ui/keybinds_overlay.rs` (`v`/`V`).
- Curation: `scripts/curate_genders.py` → `scripts/curate_characters.py`.

## Open questions / future work

- **Multi-select picker.** Phase 1 toggles one voice per picker-open. A true
  multi-select add could come later; toggle is the minimal viable add/remove.
- **Persisting the active voice per gloss.** Currently session-only; a
  `gloss_voices.active` flag or a per-gloss column could persist the last choice
  if desired.
- **Age bands as a shared vocabulary.** Voices use ranges and characters use a
  number; a future UI could surface named bands. Not needed for resolution.
- **Removing a voice that has cached audio.** Toggling a voice off doesn't delete
  its `gloss_audio` rows; the audio is simply unreachable until re-associated.
  Acceptable (cheap to re-cache); a cleanup pass could prune orphaned per-voice
  audio.
