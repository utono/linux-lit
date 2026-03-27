# Visual Mode Design

Vim-style linewise visual selection for linux-lit. Select lines, then act on them: copy to clipboard, merge into one line, or pipe to an external command for correction.

## State Machine

- **V** in normal mode enters visual mode, anchoring at current line
- **j/k** extend selection (anchor fixed, cursor moves). All visible lines including translations are selectable
- **G/gg** extend selection to end/start of buffer
- **Enter** opens action popup
- **Esc or V** cancels selection, returns to normal mode
- **u** in normal mode undoes the last destructive action

Action popup:
- **Ctrl+n / Ctrl+p** navigate menu items
- **Enter** executes selected action
- **Esc** closes popup, returns to visual mode (selection preserved)

Visual mode coexists with other overlays. Opening Ctrl+p (library picker) while in visual mode gives the picker priority; when it closes, visual mode and selection remain active.

## Data Structures

```rust
pub struct SelectionState {
    pub anchor_line: usize,
    pub cursor_line: usize,
}

pub struct UndoEntry {
    pub db_lines: Vec<Line>,                   // original DB rows to restore
    pub file_backup: Option<(String, String)>,  // (path, content) if text_file exists
    pub cursor_line: usize,
}

pub struct ActionPopupState {
    pub selected_index: usize,
}
```

Added to AppState:
- `visual_selection: Option<SelectionState>` — None means normal mode
- `action_popup: Option<ActionPopupState>` — None means popup not showing
- `undo_stack: Vec<UndoEntry>` — cleared on work switch
- `selection_tag: gtk4::TextTag` — theme-aware blue tint background

Selection range is always `min(anchor, cursor)..=max(anchor, cursor)`.

## Actions

### Copy

Extracts text of selected lines joined with `\n`. Pipes to `wl-copy`. No undo entry. Clears selection, returns to normal mode.

### Copy with metadata

Each line formatted as `[line_num] SPEAKER (start-end): text`. Speaker and timestamp included only when present. Pipes to `wl-copy`. No undo entry.

### Merge lines

Joins selected lines into a single line, separated by single spaces, trimming whitespace.

DB operations:
- Insert merged text as replacement for the first selected line
- Delete the remaining original lines

If `text_file` exists: update the file to match.

Pushes undo entry with original DB lines and file content. Rebuilds buffer.

### External commands

Configured in `~/.config/linux-lit/config.json`:

```json
{
  "visual_mode_commands": [
    { "name": "Correct transcript", "command": "claude-correct" },
    { "name": "Fix punctuation", "command": "fix-punct.sh" }
  ]
}
```

Selected lines (joined with `\n`) go to the command's stdin. Stdout replaces the selected lines.

DB operations:
- Replace selected line rows with output lines
- Delete excess original lines if output has fewer lines
- Insert new rows if output has more lines

If `text_file` exists: update the file to match.

On non-zero exit: show error, no modification, selection stays intact.

Pushes undo entry. Rebuilds buffer.

### Availability

- Copy, copy with metadata: always available (all works)
- Merge, external commands: always available — DB-only works update lit.db; works with text_file update both DB and file

## Undo

**u** in normal mode pops the last undo entry:
- Restores original DB lines (re-insert deleted rows, remove merged/replaced row)
- If file_backup present: writes saved content back to text file
- Rebuilds buffer, restores cursor position

Undo stack is unbounded for the session (text files are small). Cleared on work switch.

## Action Popup

The popup is a lightweight GTK overlay. Shows built-in actions first, then a separator, then user-configured external commands:

```
  Copy
  Copy with metadata
  Merge lines
  ───────────────
  Correct transcript
  Fix punctuation
```

Ctrl+n/Ctrl+p navigate, Enter executes, Esc dismisses (back to visual mode).

If no external commands are configured, the separator and external section are omitted.

## Visual Highlighting

Selected lines receive:
- Undimmed text (same as current line in normal mode)
- Blue background tint via `selection_tag` (theme-aware from themes-unified.json)
- Cursor line within selection is bold

The dim tag is removed from all lines in the selection range, and the selection_tag is applied on top. On selection clear, dim is reapplied and selection_tag removed.

## Key Routing Priority

```
1. Library picker        (highest)
2. Media picker
3. Settings overlay
4. Search bar
5. Action popup          (new)
6. Visual mode           (new)
7. Normal mode           (lowest)
```

## Config Changes

Add to Config struct and `config.json`:

```rust
pub visual_mode_commands: Vec<VisualModeCommand>,
```

```rust
pub struct VisualModeCommand {
    pub name: String,
    pub command: String,
}
```

Defaults to empty vec. Serde skip_deserializing with default for backwards compatibility.

## File Structure

New files:
- `src/input/visual.rs` — SelectionState, selection movement, highlight updates
- `src/input/action_popup.rs` — ActionPopupState, popup widget, action dispatch
- `src/input/undo.rs` — UndoEntry, undo stack, DB/file restore logic

Modified files:
- `src/input/keymap.rs` — add visual mode and action popup to routing chain
- `src/app.rs` — add visual_selection, action_popup, undo_stack, selection_tag to AppState
- `src/config.rs` — add visual_mode_commands to Config
- `src/db/queries.rs` — add write queries (merge, replace, delete, restore lines)
