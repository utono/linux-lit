# Character gender in `lit.db` — design

**Status:** design (not implemented). Prerequisite for **voice selection** in
[gloss-driven OP IPA tagging](./2026-06-08-gloss-ipa-tagging-design.md) §5 and the
[custom OP voices guide](../../guides/elevenlabs-v3-custom-voices.md) ("Female
speakers"). It is *not* a prerequisite for the IPA markup itself — only for
choosing the male (A-OP) vs female (A-OP-F) voice that reads an already-tagged
line.

## Problem

The OP voices come in gendered pairs — A-OP / B (male) and A-OP-F / B-F (female)
— selected per gloss by the gender of the character who speaks the source
passage. linux-lit can already resolve a gloss to its **speaker name**
(`passages.character`, or `line_mapping.speaker` over the citation span — e.g.
`HAMLET`, `OPHELIA`), but **nothing in `lit.db` stores a character's gender**.
This spec adds that mapping.

It is a small, separable piece of work (one table + a batch population tool),
deliberately kept out of the IPA-tagging spec so each stays focused and
independently implementable.

## Decisions

- **Gender is a fixed per-character property — the character's *true* gender.**
  Cross-dressed characters (Viola→Cesario, Rosalind→Ganymede, Portia→Balthazar,
  Imogen→Fidele) use their true gender throughout: Viola is `female` even while
  disguised as Cesario. The voice reflects who the character *is*, not who they
  present as. (Per-scene disguise tracking is explicitly out of scope; revisit
  only if a disguised read sounds wrong.)
- **Four gender values:** `male` | `female` | `neutral` | `unknown`.
  - `neutral` — deliberately non-gendered: groups (`ALL`, `LORDS`), choruses,
    most spirits/abstractions, mixed-gender combined speakers.
  - `unknown` — not (yet) resolved.
  - Both fall back to the default (male) voice, but mean different things: one is
    a decision, the other is a gap.
- **Population is LLM-assigned, NO human-review gate.** The tool asks Claude for
  each speaker's gender and loads the result directly — no intermediate
  reviewable list. (See *Trust model* below for why this is acceptable.)
- **Combined speakers resolve to a single gender if unambiguous.** `A / B` where
  both are the same gender → that gender; mixed or unclear → `neutral`. The
  combined string is stored as its own row with the resolved value.

## 1. Schema

```sql
CREATE TABLE IF NOT EXISTS characters (
  work_abbrev TEXT NOT NULL,   -- e.g. 'Ham', 'Mac' — scopes names per work
  speaker     TEXT NOT NULL,   -- speaker string EXACTLY as in line_mapping.speaker
  gender      TEXT NOT NULL,   -- 'male' | 'female' | 'neutral' | 'unknown'
  PRIMARY KEY (work_abbrev, speaker)
);
```

- **Keyed by `(work_abbrev, speaker)`**, not a global name — the same name can
  differ across works, and speaker strings are per-edition. The TTS-time lookup
  joins `passages.character` / `line_mapping.speaker` on this exact pair.
- `speaker` stores the **verbatim** string, including combined forms
  (`CORNELIUS / VOLTEMAND`) and role-prefixed / edition-qualified names
  (`PLAYER KING`, `ANTIPHOLUS OF SYRACUSE`). The join is therefore exact —
  **no runtime normalization**; all normalization happens once, at curation time.
- linux-lit creates the table with `CREATE TABLE IF NOT EXISTS` (the existing
  pattern for `gloss_audio` / `bookmarks`), but the **rows are loaded by the
  curation tool**, not the app.

## 2. Population — LLM curation tool (`scripts/`)

An out-of-band batch tool (like the phonetic-STT utility), not linked into the
GTK app:

1. **Enumerate** every distinct `(work_abbrev, speaker)` from `line_mapping`
   where `speaker IS NOT NULL`. (~260k speaker-bearing rows total, but distinct
   speakers per work are far fewer.)
2. **Assign gender** per distinct speaker via Claude (it knows the canon), with
   these rules baked into the prompt:
   - **True gender** of the character (ignore disguises — Viola = female).
   - **Combined `A / B`** → resolve if both the same gender, else `neutral`.
   - **Groups / collective** (`ALL`, `LORDS`, `BOTH`) → `neutral`.
   - **Spirits / non-human** → the canonically clear gender where one exists
     (Hamlet's `GHOST` = male; the `WITCHES` → `neutral`), else `neutral`.
   - **Genuinely unresolvable** → `unknown` (rather than a guess).
3. **Load directly** into `characters` — no review step.
4. **Re-runnable.** New or edited works re-enumerate; only newly-distinct
   speakers are sent to the LLM. A wrong assignment can be corrected by
   re-running that speaker or a one-off `UPDATE`.

### Trust model (why no review gate)

Skipping human review trades a small, bounded error rate for a far simpler
pipeline. The risks are contained:

- **Uncertain cases self-protect.** Anything the LLM can't resolve becomes
  `unknown`/`neutral` → the safe male-voice fallback, never a confident wrong
  guess.
- **Errors are visible and cheap to fix.** A wrong-gender voice is audible; the
  tool is re-runnable and the table is a single `UPDATE` away from a correction.
  Review is thus *opt-in / after-the-fact*, not a mandatory gate.
- **Worst case is a wrong voice on a minor speaker** — not data corruption, not a
  crash. Acceptable for a first version over a large corpus where full manual
  curation would be the dominant cost.

## 3. Consumption (the join the IPA spec needs)

At TTS voice-selection time (IPA spec §5), for a source-verse block:

1. Resolve the gloss → speaker: `passages.character`, or derive from
   `line_mapping.speaker` over the citation span.
2. `SELECT gender FROM characters WHERE work_abbrev = ? AND speaker = ?`.
3. Map to a voice:
   - `male` → **A-OP** (verse) / **B** (explication)
   - `female` → **A-OP-F** / **B-F**
   - `neutral`, `unknown`, **or no matching row** → **A-OP / B (male) fallback**
     — never guess (per the guide's rule).

The `gloss_audio` cache keys only on `(gloss_id, kind, paragraph_index)` —
`voice_id`/`model_id` are stored columns, NOT part of the key. So a later gender
correction does **not** auto-invalidate: the next play is a cache hit on the old
audio and replays the old voice until that gloss's cached rows are deleted
(`delete_gloss_audio`) and re-synthesized. A gender re-curation that should change
existing audio must therefore clear the affected glosses' cache explicitly; for a
fresh gloss (no cached audio yet) the gendered voice is picked correctly on first
synthesis with no extra step.

## Key files (when implemented)

- New table: created via `CREATE TABLE IF NOT EXISTS` alongside the existing
  `gloss_audio` / `bookmarks` DDL (`src/db/queries.rs`).
- Curation tool: a new `scripts/` utility (Python or a one-off Rust bin) that
  enumerates speakers, calls Claude, and loads `characters`.
- Consumption: the voice-selection step in the IPA tagging implementation
  (`src/input/actions/gloss.rs` TTS path), joining on `characters`.

## Open questions / future work

- **Per-scene disguise voicing** (Cesario read in a male voice while Viola is
  disguised) — deferred; needs disguise span tracking. Revisit only if the
  true-gender read sounds wrong on stage-heavy disguise plots.
- **Doubling / role-prefixed generics** (`FIRST PLAYER`, `MESSENGER`, `SERVANT`)
  — these get `neutral`/`unknown` → default voice; a finer per-role assignment is
  possible later but low value.
- **Non-Shakespeare works** in the corpus — the same table/tool applies; the LLM
  prompt is the only Shakespeare-specific part and can be generalized.
