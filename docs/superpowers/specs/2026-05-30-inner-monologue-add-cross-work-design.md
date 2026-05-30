# Inner Monologue Add: Cross-Work Passages

**Date:** 2026-05-30
**Status:** Approved

## Purpose

When viewing an inner-monologue gloss in the overlay, pressing `a` should let the user paste lines from elsewhere in Shakespeare's corpus and generate a new inner-monologue gloss that treats those lines as the unspoken inner voice beneath the original passage.

Currently, `a` in the gloss overlay always creates a teacher-generic gloss regardless of which gloss type is displayed. This change makes `a` context-sensitive: it adapts its dialog labels, Claude prompt, and save behavior based on the `gloss_type` of the gloss being viewed.

## Example

The user is viewing an inner-monologue gloss for Hamlet's death speech ("the rest is silence"). They press `a` and paste Claudio's lines from Much Ado: "Silence is the perfectest herald of joy. I were but little happy if I could say how much." Claude generates an analysis of what Hamlet and Horatio might be thinking if Claudio's lines were their inner voice — exploring the echoes between "the rest is silence" and "silence is the perfectest herald of joy."

## Design Decisions

- **Context-sensitive `a` keybind** rather than a new keybind or sub-menu. The behavior adapts to the current `gloss_type`.
- **Claude infers the source work** from the pasted text. No metadata fields or database lookup needed.
- **Dialog title changes** to "INNER MONOLOGUE PASSAGE" with hint "Paste lines from another work" when in inner-monologue context.
- **New prompt** (`INNER_MONOLOGUE_ADD_PROMPT`) focused on treating pasted lines as inner voice, not answering a question.
- **Saved as `inner-monologue`** gloss type, not teacher-generic. The pasted lines are prepended to the stored gloss text for identification when cycling.

## GlossContext Change

`GlossContext` in `src/gloss.rs` gains a `pub gloss_type: String` field. Construction sites:

- `build_context()` sets `gloss_type: "teacher-generic".to_string()`
- `build_context_for_type()` sets `gloss_type: gloss_type.to_string()`
- Manual construction in `navigate_gloss_passage` sets `gloss_type: "teacher-generic".to_string()`

## Dialog Labels

`show_amend_dialog` reads `gloss_context.gloss_type`:

- `"inner-monologue"` → title: "INNER MONOLOGUE PASSAGE", hint: "Paste lines from another work · Ctrl+Enter submit · Esc cancel"
- anything else → title: "GLOSS PROMPT", hint: "Ctrl+Enter submit · Esc cancel" (existing behavior)

## add_gloss Branching

`add_gloss` branches on `ctx.gloss_type`:

**When `"inner-monologue"`:**

- User input treated as pasted passage text
- Calls Claude with `INNER_MONOLOGUE_ADD_PROMPT` and `build_inner_monologue_add_message`
- Formats stored gloss as: `<gloss>Inner voice from:</gloss>\n\n{pasted lines as verse}\n\n{Claude analysis}`
- Saves with `gloss_type = "inner-monologue"`
- Reloads all inner-monologue glosses for citation

**When anything else:**

- Existing behavior unchanged (question → `USER_QUESTION_PROMPT` → `"teacher-generic"`)

## New Prompt: INNER_MONOLOGUE_ADD_PROMPT

```
You are a director helping actors discover the inner monologue beneath
a passage from a dramatic text.

The reader has selected a passage and provided lines from elsewhere in
Shakespeare's corpus that share thematic or verbal echoes. Treat the
provided lines as the unspoken inner voice — what the characters in the
original passage might be thinking or hearing beneath their spoken words.

For each character in the original passage:
- How do the cross-work lines illuminate what this character is really
  thinking or feeling?
- What verbal echoes connect the two passages (shared words, inverted
  meanings, parallel structures)?
- What actable inner cues can an actor draw from the cross-work lines —
  short thoughts that sit beneath each spoken line?

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

## New Message Builder: build_inner_monologue_add_message

```
Play: {work_title}
Act: {act}, Scene: {scene}
Speaker: {speaker}

--- ORIGINAL PASSAGE ---
{ctx.source_text}

--- CROSS-WORK LINES (inner voice) ---
{user_pasted_text}
```

## Saved Gloss Format

The full gloss text prepends the pasted lines as context (similar to how teacher-generic add prepends `Q: {prompt}`):

```
<gloss>Inner voice from:</gloss>

{pasted lines wrapped in <verse> tags}

{Claude's analysis}
```

## Files Changed

- **`src/gloss.rs`** — Add `gloss_type` field to `GlossContext`. Add `INNER_MONOLOGUE_ADD_PROMPT` constant. Add `build_inner_monologue_add_message` function. Update `build_context()` and `build_context_for_type()` to set `gloss_type`.

- **`src/input/actions/gloss.rs`** — Update `show_amend_dialog` to read `gloss_type` and set dialog title/hint. Update `add_gloss` to branch on `gloss_type`. Update `navigate_gloss_passage` to set `gloss_type` on manually-constructed `GlossContext`.

## Not Changed

- No DB migration or new tables
- No new keybinds
- No new UI widgets
- No changes to overlay rendering
- No changes to teacher-generic gloss behavior
- `src/input/visual.rs` unchanged (already uses correct builders)
- `src/input/keymap.rs` unchanged (`a` already calls `show_amend_dialog`)
- `src/db/queries.rs` unchanged (already parameterized for gloss_type)
