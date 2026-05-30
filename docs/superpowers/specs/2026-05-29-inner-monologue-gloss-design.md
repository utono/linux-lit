# Inner Monologue Gloss

**Date:** 2026-05-29
**Status:** Approved

## Purpose

Add an "Inner Monologue" action to the visual mode action picker (CR in visual mode). This new gloss type asks Claude to explore the subtext beneath a selected passage: what each character is really hearing, thinking, and feeling, and what actable inner cues an actor can use as inner monologue.

Unlike the existing "Gloss with Claude" (teacher-generic), which paraphrases and explains archaic language, the inner monologue gloss is director-focused. It draws on the full scene context to surface connections between the selected lines and surrounding dialogue. For example, when Claudio says "Silence is the perfectest herald of joy," the inner monologue gloss would connect this to the Prince's later line "Your silence most offends me" and explore what Claudio really hears when Beatrice says "Speak, count, 'tis your cue."

## Design Decisions

- **New top-level picker action** rather than a sub-menu of the existing gloss action
- **Full scene context** sent to Claude (all lines sharing the same act/scene division), with the selected passage marked separately
- **All characters analyzed**, not just the speaker of the selected line
- **Cached in DB** using `gloss_type = 'inner-monologue'` in the existing `glosses` table
- **Same overlay** used for display (existing gloss overlay with XML tag rendering)
- **Minimal approach**: new prompt, new gloss type, reuse existing infrastructure

## Action Picker

`BUILTIN_ACTIONS` in `src/input/visual.rs:129` becomes:

```
["Gloss with Claude", "Inner Monologue", "Copy", "Copy with metadata"]
```

"Inner Monologue" is at index 1. The `execute_action` match adds index 1 calling `action_inner_monologue(state_rc)`. Existing indices shift: Copy becomes 2, Copy with metadata becomes 3.

## Scene Context

The inner monologue action differs from the teacher-generic gloss in one key way: it sends the **full scene** as context, not just the selected lines.

**Building the user message:**

1. From the selected lines, extract `div1` and `div2` (act/scene)
2. Filter `work.lines` to get all lines with matching `div1`/`div2`
3. Format the user message with two clearly separated sections:
   - Full scene (all lines, with speaker names)
   - Highlighted passage (the selected lines, marked for focus)

The work already has all lines loaded with `div1`/`div2` fields, so this is an in-memory filter — no extra DB query.

## Caching

The hash for caching uses `'inner-monologue'` as the gloss type suffix:

```
format!("{}:{}:{}:inner-monologue", abbrev, start_citation, end_citation)
```

The same passage can have both a teacher-generic gloss and an inner-monologue gloss cached independently.

## DB Query Changes

The existing `find_all_glosses`, `find_existing_gloss`, `find_glossed_passages`, and `save_gloss` functions in `src/db/queries.rs` hardcode `gloss_type = 'teacher-generic'`. These are parameterized to accept a `gloss_type: &str` parameter. All existing call sites pass `"teacher-generic"` explicitly.

No DB migration is needed — the `glosses` table already has a `gloss_type` column.

## System Prompt

```
You are a director helping actors discover the inner monologue beneath
a passage from a dramatic text.

Given a scene and a highlighted passage within it, explore what each
character present is thinking, hearing, and feeling — the subtext
beneath the spoken words.

For each character in the highlighted passage:
- What do they actually hear when the other character speaks?
  (e.g., Claudio hears "Speak, count, 'tis your cue" but what he
  really hears is "speak now or your silence will offend")
- What inner monologue drives their response? What are they telling
  themselves before they open their mouth?
- What words or phrases could an actor use as inner cues — short,
  actable thoughts that sit beneath each line?
- How does the surrounding scene (lines before AND after the passage)
  illuminate what the character is really saying?

Draw on the full scene provided for evidence. Reference specific lines
that echo, foreshadow, or reframe the passage's meaning.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each character's analysis section (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers
```

## User Message Format

```
Play: {work_title}
Act: {act}, Scene: {scene}
Speaker: {speaker(s) in selection}

--- FULL SCENE ---
{all lines in act/scene, formatted with speaker names}

--- HIGHLIGHTED PASSAGE ---
{selected lines only}
```

## Files Changed

- **`src/input/visual.rs`** — Add "Inner Monologue" to `BUILTIN_ACTIONS`. Add match arm in `execute_action`. Add `action_inner_monologue` function (same pattern as `action_gloss_with_claude` but with full-scene context and inner-monologue prompt/type).

- **`src/gloss.rs`** — Add `INNER_MONOLOGUE_PROMPT` constant. Add `build_inner_monologue_message` function that formats the full-scene + highlighted-passage user message. Update hash generation to support the new gloss type.

- **`src/db/queries.rs`** — Parameterize `find_all_glosses`, `find_existing_gloss`, `find_glossed_passages`, and `save_gloss` to accept `gloss_type: &str` instead of hardcoding `'teacher-generic'`. Update all existing call sites.

## Not Changed

- No DB migration (table already supports arbitrary gloss_type values)
- No keymap changes (triggered from existing visual mode CR picker)
- No new UI widgets (reuses existing gloss overlay)
- No changes to the existing "Gloss with Claude" behavior
