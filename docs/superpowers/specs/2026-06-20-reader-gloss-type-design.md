# Reader Gloss — a new reader-focused gloss type

**Date:** 2026-06-20
**Status:** Approved (pending spec review)
**Repos touched:** linux-lit (Rust + spec), `~/utono/claude-api-prompts` (prompt masters), shared `lit.db`

## Purpose

Add a new gloss type, **Reader Gloss**, created from the visual-mode Action
menu. Unlike `teacher-generic` (an acting coach's direction — operative words,
breath, verse delivery, Barton/Berry/Rodenburg pedagogy), Reader Gloss is
**terse** and aimed at a *reader's comprehension*: it explicates the
character's motives within the scene and any Elizabethan vocabulary, allusions,
metaphors, idioms, or social/political concepts a modern reader would miss.

It is a **distinct `gloss_type`** that coexists with `teacher-generic` and
`inner-monologue`. A passage may carry all three side by side; the Action you
choose decides which you create and view.

## Identifiers

- Action menu label: **`Reader Gloss`** — the new **first** built-in action.
- Stored `glosses.gloss_type`: **`reader-gloss`**.
- `api_prompts.prompt_key`: **`gloss.reader-gloss`** (plus dedicated edit / Q&A
  / add variants — see below).

## The prompts

All Reader Gloss prompts are **plain text — no OP-IPA plumbing**. They have no
`{ipa_rules}`/`{}` slot, so they are NOT added to the
`all_templated_gloss_prompts_fill_their_placeholder` test (that test is for
IPA-templated prompts only).

They **must keep the exact `<speaker>` / `<verse>` / `<gloss>` XML output
format** — the gloss overlay renderer parses those tags; deviating breaks
display. The parser (`parse_gloss_tags`, `gloss_overlay.rs:2071`) is
**order-agnostic**, so a leading `<gloss>` lede placed *before* the first
`<speaker>`/`<verse>` block renders correctly as the first Explication block —
**no overlay change is required** for the lede. What differs from
teacher-generic is the *content* of `<gloss>`:

- **The first `<gloss>` paragraph is a one-sentence motivation lede.** Exactly
  one sentence, focused on motivation — what the speaker wants in this moment.
  - If the selected verse contains **more than one speaker**, this single lede
    sentence uses **semicolons** to describe each character's motivation in
    turn (one independent clause per character, in order of appearance), e.g.
    "Suffolk flatters the Protector's pride to provoke him; Gloucester deflects
    with feigned humility to mask his contempt." It stays one sentence — clauses
    joined by semicolons, not multiple sentences.
- After the lede, the remaining `<gloss>` paragraphs are terse (1–3 sentences
  each) and explicate (a) any further motive shifts and (b) Elizabethan words,
  allusions, metaphors, idioms, and social/political concepts a reader would miss.
- **Drop** the acting-pedagogy material entirely: operative words, breath,
  verse-delivery notes, Barton/Berry/Hall/Rodenburg/Linklater references.
- No IPA anywhere.
- **Always keep the lede.** The one-sentence motivation lede is mandatory in
  every Reader Gloss and must survive refinement:
  - `READER_GLOSS_EDIT_PROMPT` regenerates the whole gloss, so it must
    **preserve (or rewrite, but never drop) the leading one-sentence motivation
    lede** as the first `<gloss>` paragraph — same one-sentence / semicolon
    rules as a fresh gloss.
  - Q&A (`-question`) and Add (`-add`) only *append* a new `<gloss>Q: …</gloss>`
    block after the existing gloss, so the original lede is preserved by
    construction; these prompts must not emit their own lede or restate it.

Four dedicated prompts (full parity with teacher-generic's set), each shipped
both as a compiled `FALLBACK` in `src/gloss.rs` AND seeded into `api_prompts`
(`is_active=1`, descriptive `note`), matching the existing two-source pattern:

- `gloss.reader-gloss` — fresh Reader Gloss (`READER_GLOSS_PROMPT`).
- `gloss.reader-gloss-question` — follow-up Q&A (`READER_GLOSS_QUESTION_PROMPT`).
- `gloss.reader-gloss-edit` — edit existing (`READER_GLOSS_EDIT_PROMPT`).
- `gloss.reader-gloss-add` — add cross-work / user lines (`READER_GLOSS_ADD_PROMPT`).

(The add variant mirrors teacher-generic's behavior, which currently reuses
`USER_QUESTION_PROMPT` for "add" on non-monologue glosses; for Reader Gloss the
"add" path gets its own terse prompt so refinements stay in voice.)

## Action menu order and dispatch

`BUILTIN_ACTIONS` becomes (Reader Gloss first, existing four unchanged after):

```
["Reader Gloss", "Gloss with Claude", "Inner Monologue", "Copy", "Copy with metadata"]
```

`execute_action` index map (`src/input/visual.rs`) shifts accordingly:

- `0` → `action_reader_gloss` (new)
- `1` → `action_gloss_with_claude` (was 0)
- `2` → `action_inner_monologue` (was 1)
- `3` → `action_copy(false)` (was 2)
- `4` → `action_copy(true)` (was 3)

`handle_action_popup_key` in `keymap.rs` is index-agnostic (it forwards
`selected_index` straight to `execute_action`), so only `execute_action`'s match
arms change — no index constants live in keymap.

## New code: `action_reader_gloss`

A near-clone of `action_gloss_with_claude` (`src/input/visual.rs:396`),
differing only in:

- `build_context_for_type(work, &selected_lines, "reader-gloss")` instead of
  `build_context(...)` (so `ctx.gloss_type == "reader-gloss"` and the cache
  hash keys on `reader-gloss`).
- `find_all_glosses(.., &["reader-gloss"])` for the cache lookup.
- system prompt = `READER_GLOSS_PROMPT` (via `call_claude_with_prompt`).
- `save_gloss(.., "reader-gloss", ..)` on result.

The loading card, `<speaker>`/`<verse>` source header, overlay show calls, and
DB stamp are otherwise identical.

## Full-parity touchpoints (add `"reader-gloss"`)

The two refinement handlers branch on `ctx.gloss_type`. Their current two-way
branch (`is_inner_monologue` vs. else→teacher) becomes three-way
(inner-monologue / reader-gloss / teacher-generic), each writing its own
`gloss_type` back:

- `src/input/actions/gloss.rs::add_gloss` (~673) — add a reader-gloss arm using
  `READER_GLOSS_ADD_PROMPT`, `gloss_type_str = "reader-gloss"`. The "Q:"/"Inner
  voice" prefix wrapper: use a `<gloss>Q: …</gloss>` style prefix as
  teacher-generic does.
- `src/input/actions/gloss.rs::edit_gloss` (~773) — add a reader-gloss arm using
  `READER_GLOSS_EDIT_PROMPT`, `gloss_type_str = "reader-gloss"`.

Discovery/picker arrays that currently list `&["teacher-generic",
"inner-monologue"]` must include `"reader-gloss"` so `Ctrl+g` discovery, the
gloss picker, and synopsis batch see Reader Glosses:

- `src/input/keymap.rs:406`
- `src/input/actions/gloss.rs:85`, `:115`, `:1891` (`GLOSS_TYPES`)
- `src/input/actions/synopsis.rs:305`, `:333`

(`pickers.rs:843` and `visual.rs:493,503` are teacher-generic-specific code
paths for the *existing* action and are left unchanged; the new action has its
own clone.)

## Ctrl+/ keybinds overlay

The Action menu is not a keycap, so the overlay's keycap strip is unaffected.
Verify no `describe()` arm enumerates the Action menu items; if one does, add
"Reader Gloss". (Expected: no change needed.)

## Database seeding (prompts repo: `~/utono/claude-api-prompts`)

Prompts are NOT seeded with hand-written SQL. The canonical masters live as
plaintext in `~/utono/claude-api-prompts/prompts/<key>.md`, and
`scripts/sync-to-db.py` (or the `sync-prompts` skill) inserts/activates them in
`lit.db`'s `api_prompts`. So seeding is:

1. Add four master files in that repo:
   - `prompts/gloss.reader-gloss.md`
   - `prompts/gloss.reader-gloss-question.md`
   - `prompts/gloss.reader-gloss-edit.md`
   - `prompts/gloss.reader-gloss-add.md`
2. Run the sync (`python scripts/sync-to-db.py`, or `sync-prompts` skill) to
   write them as version 1, `is_active=1`, with a `note`.
3. Commit the masters in `claude-api-prompts` (its own repo).

The app prefers DB prompts over compiled fallbacks via `active_prompt`, and the
compiled `FALLBACK` strings in `src/gloss.rs` guarantee the feature works even
before the rows are synced — so linux-lit can build and run while the prompts
repo work happens in parallel. **The master `.md` content and the compiled
`FALLBACK` must be kept identical** (the existing prompts follow this rule).

## Testing

- `cargo build` and `cargo test --bins` (prompt-registry test, `find_all_glosses`
  query). Pure-logic only.
- **Renders-correctly criterion → user-run verification.** Because the output
  appears in the gloss overlay, an agent cannot self-verify reliably. After
  build passes, the user runs the app (or the headless e2e) to confirm:
  1. "Reader Gloss" is the top Action menu item.
  2. Selecting it generates a gloss and the overlay renders the
     `<speaker>`/`<verse>`/`<gloss>` XML correctly.
  3. A passage can hold both a Reader Gloss and a Gloss-with-Claude gloss, each
     reachable via its own action / the gloss picker.
  4. Edit / Q&A / add on a Reader Gloss stays terse and saves back as
     `reader-gloss` (not teacher-generic).

## Out of scope

- No change to vocab-word / vocab-elided glosses.
- No retirement of teacher-generic.
- No new keybind (reached through the existing visual-mode Action popup only).
