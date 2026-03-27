use gtk4::prelude::*;

use crate::app::AppState;
use crate::db::models::Line;

/// Tracks the visual selection range (anchor..cursor).
pub struct SelectionState {
    pub anchor_line: usize,
    pub cursor_line: usize,
}

impl SelectionState {
    pub fn new(line: usize) -> Self {
        Self {
            anchor_line: line,
            cursor_line: line,
        }
    }

    /// Returns (start, end) as an inclusive range, regardless of direction.
    pub fn range(&self) -> (usize, usize) {
        let start = self.anchor_line.min(self.cursor_line);
        let end = self.anchor_line.max(self.cursor_line);
        (start, end)
    }
}

/// A snapshot of state before a destructive action, for undo.
pub struct UndoEntry {
    pub db_lines: Vec<Line>,
    pub file_backup: Option<(String, String)>,
    pub cursor_line: usize,
}

/// Apply the selection_tag to all lines in the visual selection range.
/// Also removes dim_tag from those lines so they appear at full brightness.
pub fn apply_selection_highlight(state: &AppState) {
    let selection = match &state.visual_selection {
        Some(s) => s,
        None => return,
    };
    let (start, end) = selection.range();
    let buffer = &state.buffer;

    for line_idx in start..=end {
        if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            buffer.remove_tag(&state.dim_tag, &line_start, &line_end);
            buffer.apply_tag(&state.selection_tag, &line_start, &line_end);
        }
    }
}

/// Remove the selection_tag from the entire buffer.
pub fn clear_selection_highlight(state: &AppState) {
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.selection_tag, &buf_start, &buf_end);
}

/// Move the visual selection cursor by delta lines.
/// Does NOT skip translation lines (visual mode selects all visible lines).
pub fn move_selection_cursor(state: &mut AppState, delta: i32) {
    if state.visual_selection.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    let current_cursor = state.visual_selection.as_ref().unwrap().cursor_line;
    let new_cursor = (current_cursor as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;
    state.visual_selection.as_mut().unwrap().cursor_line = new_cursor;
    state.current_line = new_cursor;

    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Enter visual mode: set anchor at current line.
pub fn enter_visual_mode(state: &mut AppState) {
    state.visual_selection = Some(SelectionState::new(state.current_line));
    crate::input::navigation::update_highlight_and_ensure_visible(state);
    crate::logging::log(&format!("VISUAL: entered at line {}", state.current_line));
}

/// Exit visual mode: clear selection and highlighting.
pub fn exit_visual_mode(state: &mut AppState) {
    if state.visual_selection.is_some() {
        state.visual_selection = None;
        clear_selection_highlight(state);
        crate::input::navigation::update_highlight_and_ensure_visible(state);
        crate::logging::log("VISUAL: exited");
    }
}

/// Extend visual selection to the first line (gg equivalent).
pub fn extend_to_start(state: &mut AppState) {
    if let Some(ref mut sel) = state.visual_selection {
        sel.cursor_line = 0;
        state.current_line = 0;
        crate::input::navigation::update_highlight_and_ensure_visible(state);
    }
}

/// Extend visual selection to the last line (G equivalent).
pub fn extend_to_end(state: &mut AppState) {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    if let Some(ref mut sel) = state.visual_selection {
        sel.cursor_line = line_count - 1;
        state.current_line = line_count - 1;
        crate::input::navigation::update_highlight_and_ensure_visible(state);
    }
}

/// Tracks which action is highlighted in the popup menu.
pub struct ActionPopupState {
    pub selected_index: usize,
}

/// Built-in action names, in display order.
pub const BUILTIN_ACTIONS: &[&str] = &["Copy", "Copy with metadata", "Merge lines"];

/// Determine which built-in actions are available for the current work.
pub fn available_builtin_actions(_state: &AppState) -> Vec<&'static str> {
    BUILTIN_ACTIONS.to_vec()
}

/// Open the action popup menu.
pub fn open_action_popup(state: &mut AppState) {
    let builtins = available_builtin_actions(state);
    let externals: Vec<(String, String)> = state
        .config
        .visual_mode_commands
        .iter()
        .map(|c| (c.name.clone(), c.command.clone()))
        .collect();
    state.action_popup_widget.show_actions(
        &builtins.iter().map(|s| *s).collect::<Vec<_>>(),
        &externals,
    );
    state.action_popup = Some(ActionPopupState { selected_index: 0 });
    crate::logging::log("VISUAL: action popup opened");
}

/// Close the action popup without executing.
pub fn close_action_popup(state: &mut AppState) {
    state.action_popup = None;
    state.action_popup_widget.hide();
    crate::logging::log("VISUAL: action popup closed");
}

/// Execute the action at the given index.
/// Indices 0..N are built-in actions, N.. are external commands.
pub fn execute_action(state: &mut AppState, index: usize, _tokio_handle: &tokio::runtime::Handle) {
    let builtin_count = available_builtin_actions(state).len();

    if index < builtin_count {
        match index {
            0 => action_copy(state, false),
            1 => action_copy(state, true),
            2 => action_merge(state),
            _ => {}
        }
    } else {
        let ext_index = index - builtin_count;
        let command = state.config.visual_mode_commands.get(ext_index).map(|c| c.command.clone());
        if let Some(cmd) = command {
            action_external_command(state, &cmd);
        }
    }

    // Exit visual mode after action
    exit_visual_mode(state);
}

fn action_copy(_state: &mut AppState, _with_metadata: bool) {
    // Implemented in Task 7
}

fn action_merge(_state: &mut AppState) {
    // Implemented in Task 9
}

fn action_external_command(_state: &mut AppState, _command: &str) {
    // Implemented in Task 10
}
