# Local LLM Transcript Correction

**Date:** 2026-03-27
**Status:** Approved

## Problem

"Correct with Claude" spawns an interactive `claude` CLI session in a kitty terminal, polls for file sentinels, and opens a second terminal for diff review. This is slow, fragile, and tightly coupled to the Claude CLI and kitty.

## Solution

Replace with a direct HTTP call to a local Ollama instance. Show corrections in an inline before/after overlay within the app.

## Config

Two new fields in `~/.config/linux-lit/config.json` with defaults:

- `ollama_model` — string, default `"qwen2.5:7b"`
- `ollama_endpoint` — string, default `"http://localhost:11434"`

These are read from `AppConfig` at correction time. No app restart required if changed between uses.

## User Flow

1. Enter visual mode, select lines
2. Open action popup, pick "Correct with LLM"
3. App shows a brief "Correcting..." indicator
4. On response: before/after overlay appears — original text on top, corrected text below, with changed words highlighted
5. Press `y` to accept, `n` to reject
6. Accept writes to DB via `db::queries::replace_lines`, reloads the work
7. Reject dismisses the overlay, no changes

## System Prompt

Same correction instructions as today, passed as the `system` field in the Ollama API request:

> You are correcting mistranscribed audiobook text. Fix ONLY words that are obviously wrong due to speech-to-text mishearing (homophones, phonetically similar but wrong words). Do NOT rephrase, restructure, or improve the text. Preserve original line breaks exactly. Output ONLY the corrected text with no commentary.

## Components

### `src/ollama.rs` (new)

Small async module:

- `correct_text(endpoint: &str, model: &str, text: &str) -> Result<String, OllamaError>`
- POSTs to `{endpoint}/api/generate` with `model`, `system` (correction prompt), and `prompt` (the selected text)
- Sets `stream: false` for a single response
- Returns the corrected text or an error (connection refused, timeout, model not found)
- Timeout: 30 seconds

Error enum:

- `ConnectionRefused` — Ollama not running
- `Timeout` — model took too long
- `ModelNotFound` — requested model not pulled
- `Other(String)` — unexpected errors

### `src/config.rs` changes

Add to `AppConfig`:

- `ollama_model: String` with `#[serde(default = "default_ollama_model")]`
- `ollama_endpoint: String` with `#[serde(default = "default_ollama_endpoint")]`

Default functions return `"qwen2.5:7b"` and `"http://localhost:11434"`.

### `src/input/visual.rs` changes

- Rename menu item from "Correct with Claude" to "Correct with LLM"
- Replace `action_correct_with_claude` (~190 lines) with `action_correct_with_llm` (~40 lines):
  1. Collect selected lines
  2. Show "Correcting..." indicator
  3. Spawn async task calling `ollama::correct_text`
  4. On success: open the diff overlay with original and corrected text
  5. On error: show error message inline (e.g., "Ollama not running — start with: systemctl start ollama")

### Diff overlay (in `src/ui/` or `src/input/`)

A new overlay following the same pattern as the keybinds popup:

- Displays original text (top section) and corrected text (bottom section)
- Highlights words that differ between the two
- Key handling: `y` to accept, `n` to reject, `Escape` to reject
- On accept: push undo entry, call `replace_lines`, reload work, dismiss overlay
- On reject: dismiss overlay

The word-level diff highlighting uses a simple split-on-whitespace comparison — no external diff crate needed.

## Dependencies

Add to `Cargo.toml`:

- `reqwest = { version = "0.12", features = ["json"] }` — HTTP client, integrates with existing Tokio runtime

## What Gets Removed

- The entire kitty terminal spawn mechanism for Claude CLI
- The `/tmp/linux-lit-claude/` temp directory usage
- The `glib::timeout_add_local` polling loop watching for sentinel files
- The second kitty terminal for diff review
- ~150 lines net reduction in `visual.rs`

## Error Handling

- Ollama not running: show inline message with the `systemctl start ollama` hint
- Model not pulled: show message with `ollama pull <model>` hint
- Timeout (>30s): show timeout message, suggest smaller selection
- All errors dismiss cleanly, no app state corruption

## Testing

- `cargo build` — verify compilation with new `reqwest` dependency
- `cargo clippy` — no warnings
- Manual test: select lines, trigger correction, verify overlay appears
- Manual test: reject correction, verify no DB changes
- Manual test: accept correction, verify DB updated and work reloads
- Manual test: stop Ollama, verify error message appears cleanly
