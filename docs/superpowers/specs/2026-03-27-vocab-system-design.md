# Vocab System Design

Date: 2026-03-27

## Overview

Add vocabulary word highlighting, a definition panel, vocab navigation, and a concordance picker to linux-lit. Mirrors the vocab features in the lit Neovim plugin, adapted to GTK4.

## Data Layer

### Database Queries (src/db/queries.rs)

- `load_vocab_words(conn, work_abbrev)` — all vocab words + variants appearing in this work's lines. Joins `vocab_words` + `vocab_word_variants` to build a lowercase word set. Called once on `display_work`.
- `load_vocab_definition(conn, word)` — definition and sources from `vocab_words` + `vocab_word_sources`. Returns `(definition: String, sources: Vec<String>)`.
- `load_vocab_etymology(conn, word)` — prefix/root/suffix breakdown from `vocab_rhetoric`. Returns `Option<VocabEtymology>`.
- `load_vocab_gloss(conn, word, work_abbrev, line_index)` — finds the passage containing this line (via `passages` table's `start_citation`/`end_citation` range for the work), then returns gloss text from `glosses` where `gloss_type = 'vocab-word'` and `word_id` matches. Returns `Option<String>`.
- `load_vocab_word_list(conn, work_abbrev)` — for concordance picker: each vocab word found in the work with occurrence count. Returns `Vec<(String, usize)>` sorted alphabetically.

### Word Matching

Lowercase exact match against the word set (base words from `vocab_words` + variants from `vocab_word_variants`). No stemming.

### Precomputed Index

```rust
struct VocabMatch {
    word: String,       // base word (lowercased)
    line_index: usize,  // buffer line
    byte_start: usize,  // byte offset in line
    byte_end: usize,    // byte offset end
}
```

Built once on `display_work` by tokenizing each line and checking against the word set. Stored as `vocab_matches: Vec<VocabMatch>` on `AppState`, sorted by `line_index` then `byte_start`.

## Vocab Highlighting

### Theme Integration (src/theme.rs)

New field on `Theme`: `vocab_fg: String`. Resolved from `themes-unified.json` → `nvim.highlights.VocabWord.guifg`. Fallbacks: `#8a6534` (light themes), `#d8a657` (dark themes).

### TextTag

New `"vocab-word"` tag registered in `build_window` with `foreground` set to `theme.vocab_fg`. No bold, no underline — color only.

### Application

New function `apply_vocab_highlighting(state)` runs after `apply_dialogue_formatting` during `display_work`. Iterates `vocab_matches` and applies the `vocab-word` tag to each match's byte range.

### Dim Interaction

Vocab words on non-cursor lines have both `dim` and `vocab-word` tags. The `dim` tag's foreground takes priority so dimmed lines look uniform. On the cursor line, `dim` is removed and the vocab color shows through. This matches existing behavior.

### Toggle

`AppState` field: `vocab_highlight_visible: bool` (default `true`). `Alt+\` toggles — applies or removes all `vocab-word` tags. Persisted in `config.json` as `vocab_highlight_visible`.

## Definition Panel (src/ui/definition_panel.rs)

### Widget Structure

- Outer container: `gtk4::Box` (vertical), CSS class `definition-panel`, `width_request: 320`
- Three sections, each with a header label (small, dim, letterspaced) and content label:
  - DEFINITION — word definition text
  - ETYMOLOGY — prefix/root/suffix breakdown with parts colored in `vocab_fg`
  - GLOSS — contextual gloss from `glosses` table (if available for current passage)
- Bottom hint bar label: `w next · W prev · \ hide · Alt+\ highlights`
- Wrapped in a `ScrolledWindow` for long definitions

### Placement

Current `scrolled` text card is wrapped in a horizontal `gtk4::Box`. Definition panel sits to the right. Both share `margin_top: 24`, `margin_bottom: 24`. Text card keeps `margin_start: 24`, definition panel gets `margin_end: 24`, spacing of 16px between them.

### CSS (src/theme.rs generate_css)

`.definition-panel` class: same `background-color` as text area, same `color`, `border-radius: 12px`. Section headers styled via Pango markup in labels.

### Show/Hide

`AppState` field: `definition_panel_visible: bool` (default `false`). `\` toggles `set_visible()`. Pressing `w`/`W` auto-shows the panel.

### Update Logic

When cursor moves to a line with a vocab word, or `w`/`W` jumps to one, call `update_definition_panel(state, word)` which runs the three DB queries and updates label text. If cursor line has no vocab word, panel shows the last word viewed.

## Keybindings (src/input/keymap.rs)

| Key | Action | Context |
|-----|--------|---------|
| `w` | Jump to next vocab word occurrence | Normal mode |
| `W` | Jump to previous vocab word occurrence | Normal mode |
| `\` | Toggle definition panel | Normal mode |
| `Alt+\` | Toggle vocab highlighting on/off | Normal mode |
| `Ctrl+\` | Open concordance picker | Normal mode |
| `n` | Search next match | Always (unchanged) |
| `N` | Search previous match | Always (unchanged) |

### w/W Navigation Logic (src/input/navigation.rs)

- `w`: find next entry in `vocab_matches` after current `vocab_match_idx` (or after cursor line if no current index). Wraps to beginning at end. Moves cursor via existing `move_to_line`, seeks MPV to line's start time, sets `definition_panel_visible = true`, calls `update_definition_panel`.
- `W`: same but backward, wraps to end.
- `vocab_match_idx: Option<usize>` on `AppState` tracks position in the match list.

## Concordance Picker (src/ui/concordance_picker.rs)

Settings-style popup (dark overlay, rounded corners). Shows alphabetically sorted list of all vocab words in current work, each with occurrence count displayed to the right.

- Fuzzy filter input at top
- `j`/`k` navigate, `Enter` jumps to first occurrence of selected word, `Esc` closes
- Jumping auto-shows definition panel and updates it with the selected word

Modeled on the existing `SettingsOverlay` / `LibraryPicker` widget pattern.

## New Files

- `src/ui/definition_panel.rs`
- `src/ui/concordance_picker.rs`

## Modified Files

- `src/app.rs` — new AppState fields, widget tree (hbox wrapping text card + panel), vocab-word tag registration
- `src/theme.rs` — `vocab_fg` field on Theme, `.definition-panel` CSS
- `src/input/keymap.rs` — route `w`, `W`, `\`, `Alt+\`, `Ctrl+\`
- `src/input/navigation.rs` — vocab jump logic, panel update calls
- `src/db/queries.rs` — five new query functions
- `src/config.rs` — `vocab_highlight_visible` field, persist/load
