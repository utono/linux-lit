# Claude API Glossing for linux-lit

Date: 2026-04-29

## Overview

Add passage glossing to linux-lit using the Anthropic Claude API. Users visually select dialogue lines (including speaker names), trigger a gloss from the action popup, and see a performance-focused literary analysis in an overlay. Glosses are persisted to lit.db so the same passage shows its cached gloss on subsequent requests. Users can amend a gloss with a custom prompt or regenerate it from scratch.

## Architecture

Two new Rust modules, replacing the existing `ollama.rs`:

- `src/claude.rs` — thin Anthropic HTTP API client. Single public function `send_message(system, user_message, model)` that POSTs to `api.anthropic.com/v1/messages`. Reads `ANTHROPIC_API_KEY` from environment. 60-second timeout, non-streaming. Error types: `MissingApiKey`, `Timeout`, `RateLimited`, `ApiError(String)`.

- `src/gloss.rs` — gloss domain logic. Owns prompt construction, citation matching, DB persistence, and the amend/regenerate workflows. Public functions:
  - `generate_gloss(work, lines, amend_prompt, existing_gloss)` — orchestrates the full flow; `existing_gloss` is `Option<SavedGloss>` containing the gloss ID and text when amending
  - `find_existing_gloss(conn, work_abbrev, start_citation, end_citation)` — checks DB for cached gloss
  - `save_gloss(conn, passage_info, gloss_text)` — inserts passage (if new) and gloss
  - `update_gloss(conn, gloss_id, gloss_text)` — updates existing gloss for amend
  - `delete_gloss(conn, gloss_id)` — deletes gloss for regenerate
  - `build_prompt(lines, work, amend_prompt, existing_gloss)` — returns (system, user_message) tuple

### Files modified

- `src/main.rs` — add `mod claude; mod gloss;`
- `src/input/visual.rs` — replace `action_gloss_with_llm` Ollama call with `gloss.rs` entry point; rename builtin action from "Gloss with ollama" to "Gloss with Claude"
- `src/input/keymap.rs` — add `a` keybind in `handle_gloss_key` for amend dialog
- `src/db/queries.rs` — add passage/gloss query functions
- `src/config.rs` — remove `ollama_model` and `ollama_endpoint` fields; add `claude_model` field (default: `claude-sonnet-4-6-20250514`)
- `Cargo.toml` — no new dependencies (reqwest and serde already present)

### Files removed

- `src/ollama.rs` — replaced by `claude.rs` + `gloss.rs`

## Data Flow

1. User enters visual mode, selects dialogue lines (including speaker names for context).
2. User presses Enter to open the action popup, selects "Gloss with Claude".
3. `gloss.rs` builds citation range from selected lines' `div1`/`div2`/`line_in_div` fields.
4. `gloss.rs` checks DB for an existing gloss matching the citation range and `gloss_type = "teacher-generic"`.
5. **If found:** show cached gloss immediately in the overlay. User can amend (`a`), regenerate (`r`), or close (`Esc`).
6. **If not found:** show loading state in overlay, call Claude API, save passage + gloss to DB, show result in overlay.

### Amend flow

1. User presses `a` in gloss overlay.
2. GTK text input dialog appears with placeholder "Enhancement prompt...".
3. User types prompt (e.g. "focus more on the legal terminology"), presses Enter.
4. Dialog closes, overlay shows loading state.
5. `gloss.rs` builds prompt with original text + existing gloss + user's enhancement request.
6. Claude API returns amended gloss.
7. `gloss.rs` updates the existing gloss row in DB.
8. Overlay shows the amended gloss.

### Regenerate flow

1. User presses `r` in gloss overlay.
2. Overlay shows loading state.
3. `gloss.rs` deletes the existing gloss row.
4. Fresh API call (same as "not found" path).
5. New gloss saved to DB and shown in overlay.

## Citation Format

Each `Line` has `div1` (act), `div2` (scene), `line_in_div`. Citations follow the format `{work_abbrev}.{div1}.{div2}.{line_in_div}`.

Example: selecting lines 1-15 of Act 1, Scene 1 of Comedy of Errors produces `start_citation = "Err.1.1.1"`, `end_citation = "Err.1.1.15"`.

The passage hash is `md5("{work_abbrev}:{start_citation}:{end_citation}:teacher-generic")` — deterministic from citation range and gloss type, matching the litdb convention.

### Companion work normalization

Some works have companion variants (e.g. `Err-Amb` is an ambiguity-annotated version of `Err`). These share the same underlying text but may have different line numbering. To share glosses between a base work and its companions, the `work_abbrev` is normalized by stripping the `-Amb` suffix before building citations, hashes, and DB queries. This means `Err` and `Err-Amb` both query and store against `Err` passages.

The normalization function: if `work_abbrev` ends with `-Amb`, strip that suffix. Otherwise use it as-is.

## Database Operations

### find_existing_gloss

```sql
SELECT g.id, g.gloss_text, g.timestamp, p.id as passage_id
FROM glosses g
JOIN passages p ON g.passage_id = p.id
WHERE p.work_abbrev = ?
  AND p.start_citation = ?
  AND p.end_citation = ?
  AND g.gloss_type = 'teacher-generic'
ORDER BY g.timestamp DESC
LIMIT 1
```

### save_gloss

```sql
-- Step 1: insert passage if not exists
INSERT OR IGNORE INTO passages
  (hash, work_abbrev, start_citation, end_citation, act, scene, character, source_text)
VALUES (?, ?, ?, ?, ?, ?, ?, ?)

-- Step 2: get passage_id
SELECT id FROM passages
WHERE work_abbrev = ? AND start_citation = ? AND end_citation = ?

-- Step 3: insert gloss
INSERT INTO glosses (passage_id, gloss_type, gloss_text)
VALUES (?, 'teacher-generic', ?)
```

### update_gloss (amend)

```sql
UPDATE glosses SET gloss_text = ?, timestamp = CURRENT_TIMESTAMP WHERE id = ?
```

### delete_gloss (regenerate)

```sql
DELETE FROM glosses WHERE id = ?
```

## System Prompt

Gloss type: `teacher-generic`

```
You are a performance-focused teacher helping a reader understand a passage from a literary text.

Given a passage with speaker names and dialogue, provide an actor's explication that:
- Paraphrases the passage in clear, modern English
- Explains archaic vocabulary, allusions, and complex syntax
- Notes rhetorical devices, verse structure, and breath patterns that shape delivery
- Identifies the speaker's intention, operative words, and emotional arc
- References classical pedagogy where relevant (Barton, Berry, Hall, Rodenburg, Linklater)
- Defines literary terminology on first use (enjambment, caesura, anaphora, antithesis, etc.)

Formatting rules:
- Speaker name in ALL CAPS followed by a period
- Bold the quoted passage on the next line using **text** markers, preserving original line breaks exactly as they appear in the source (one canonical line per line, no reflowing or joining)
- Quote verbatim — exact words, exact spelling, exact line breaks from the source
- Leave a blank line between the closing ** and your analysis
- Never use / to join verse lines
- Never truncate with ...
- No bullets, numbered lists, headers, or block quotes
- Write in flowing prose. Prefer 3-4 sentence paragraphs, never exceed 6 sentences per paragraph.
- For long speeches (over 8 lines), break into 4-8 line chunks with analysis between each.
```

## User Message Construction

### New gloss

```
Play: {work.title}
Act: {div1}, Scene: {div2}
Speaker: {unique speakers in order, comma-separated, or "UNKNOWN"}

{selected text verbatim}
```

### Amend (existing gloss + user prompt)

```
Play: {work.title}
Act: {div1}, Scene: {div2}
Speaker: {unique speakers in order, comma-separated, or "UNKNOWN"}

{selected text verbatim}

---
Previous gloss:
{existing gloss text}

---
Enhancement request: {user's amend prompt}
```

### Regenerate

Same as "New gloss" — existing gloss is deleted before the API call.

## Overlay UI

The existing `CorrectionOverlay` widget is reused. It already supports:
- `show(original, gloss)` — side-by-side display
- `show_loading(message)` — centered loading message
- `hide()` — dismiss

### Keybindings in GlossOverlay mode

- `Escape` — close overlay, return to Reader mode
- `a` — open amend dialog (GTK text input popup)
- `r` — regenerate (delete existing, fresh API call)

### Amend dialog

A GTK `Window` with a multi-line `TextView` widget, appearing centered over the overlay. The text area should be large enough for a few sentences (approximately 4-6 lines tall). Keybindings:
- `Ctrl+Enter` — submit prompt, close dialog, trigger amend flow
- `Escape` — cancel, return to GlossOverlay

### Error states

- **ANTHROPIC_API_KEY not set** — overlay shows: "Set ANTHROPIC_API_KEY environment variable"
- **API timeout (60s)** — overlay shows: "Request timed out — try selecting fewer lines" (r to retry)
- **Rate limited** — overlay shows: "Rate limited — try again in a moment" (r to retry)
- **Other API error** — overlay shows error message (r to retry)

All error states support `Esc` to close and `r` to retry.

## API Configuration

- API key: `ANTHROPIC_API_KEY` environment variable (required, no config file fallback)
- Model: `claude_model` field in `~/.config/linux-lit/config.json` (default: `claude-sonnet-4-6-20250514`)
- Endpoint: hardcoded `https://api.anthropic.com/v1/messages` (no config needed)

## Action Popup Change

The builtin actions list in `visual.rs` changes from:

```
"Copy", "Copy with metadata", "Gloss with ollama"
```

to:

```
"Copy", "Copy with metadata", "Gloss with Claude"
```

## Follow-up Work

- Create a companion litdb skill at `~/utono/litdb/.claude/skills/analysis-teacher-generic/SKILL.md` following the `analysis-acting-generic` pattern, so the litdb CLI workflow can also generate `teacher-generic` glosses.
- Add more personas (teacher-barton, teacher-hall, etc.) as additional gloss types with persona-specific system prompts in `gloss.rs`.
