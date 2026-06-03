use gtk4::prelude::*;
use gtk4::{DrawingArea, Overlay};
use std::rc::Rc;

/// Definition of a single key on the keyboard overlay.
struct KeyDef {
    unshifted: &'static str,
    shifted: &'static str,
    action: &'static str,
    shift_action: &'static str,
    modifiers: &'static [(&'static str, &'static str)],
}

const fn key(
    unshifted: &'static str,
    shifted: &'static str,
    action: &'static str,
    shift_action: &'static str,
    modifiers: &'static [(&'static str, &'static str)],
) -> KeyDef {
    KeyDef { unshifted, shifted, action, shift_action, modifiers }
}

const fn ub(unshifted: &'static str, shifted: &'static str) -> KeyDef {
    key(unshifted, shifted, "", "", &[])
}

const fn bare(unshifted: &'static str, shifted: &'static str, action: &'static str) -> KeyDef {
    key(unshifted, shifted, action, "", &[])
}

// ── Row definitions ──────────────────────────────────────────────────

const NUMBER_ROW: &[KeyDef] = &[
    ub("$", "~"),
    bare("+", "1", "toggle speed"),
    key("[", "2", "prev ch", "2: prev scene", &[]),
    key("{", "3", "next ch", "3: next scene", &[]),
    bare("(", "4", "prev bkmk"),
    bare("&", "5", "next bkmk"),
    ub("=", "6"),
    ub(")", "7"),
    ub("}", "8"),
    ub("]", "9"),
    key("*", "0", "", "reset font", &[]),
    bare("!", "%", "font \u{2212}"),
    bare("|", "`", "font +"),
];
const BACKSPACE: KeyDef = bare("\u{232b}", "", "delete ts");

const UPPER_ROW: &[KeyDef] = &[
    bare(";", ":", "reopen echoes"),
    key(",", "<", "prev dlg", "", &[("C-,", "settings")]),
    bare(".", ">", "set chapter"),
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("C-p", "lib picker")]),
    key("y", "Y", "pg back", "", &[("C-y", "copy id")]),
    key("f", "F", "font \u{2192}", "F: \u{2190}", &[("C-f", "pg fwd"), ("M-f", "font info")]),
    key("g", "G", "", "", &[("C-g", "gloss tog"), ("M-g", "gloss pick"), ("S-C-g", "echo turns")]),
    ub("c", "C"),
    key("r", "R", "next vocab", "R: prev vocab", &[]),
    key("l", "L", "toggle signs", "", &[("S-C-l", "save+quit")]),
    key("/", "?", "search", "", &[("C-/", "keybinds")]),
    ub("@", "^"),
    key("\\", "#", "vocab ▶", "◀ vocab", &[("C-\\", "conc picker"), ("M-\\", "vocab hi")]),
];
const TAB_KEY: KeyDef = bare("Tab", "", "play/pause");

const HOME_ROW: &[KeyDef] = &[
    bare("a", "A", "play from ts"),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[]),
    key("u", "U", "start time", "", &[("C-u", "pg fwd"), ("M-u", "set end time")]),
    key("i", "I", "echoes", "I: reopen echoes", &[("M-i", "translations")]),
    key("d", "D", "", "", &[("C-d", "debug log"), ("M-d", "dim tog")]),
    key("h", "H", "synopsis", "H: auto vocab", &[]),
    key("t", "T", "", "", &[("M-t", "title tog")]),
    key("n", "N", "next match", "N: prev match", &[]),
    bare("s", "S", "sync tog"),
    key("-", "_", "", "", &[("C--", "recent")]),
];
const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

const BOTTOM_ROW: &[KeyDef] = &[
    bare("'", "\"", "reopen echoes"),
    bare("q", "Q", "next dlg"),
    bare("j", "J", "cursor \u{2193}"),
    bare("k", "K", "cursor \u{2191}"),
    bare("x", "X", "pg fwd"),
    key("b", "B", "", "", &[("C-b", "pg back")]),
    key("m", "M", "bookmark", "", &[("C-m", "bookmarks"), ("C-S-m", "media picker")]),
    key("w", "W", "copy word", "W: collect", &[]),
    key("v", "V", "", "V: visual mode", &[]),
    bare("z", "Z", "zt…"),
];

const SHIFT_KEY: KeyDef = ub("Shift", "");

/// Row 5: modifiers, sequences, and arrows gathered into one screen.
const MOD_SEQ_ROW: &[KeyDef] = &[
    key("Space", "", "page \u{2193}", "page \u{2191}", &[]),
    bare("gg", "", "go to start"),
    key("G", "", "", "go to end", &[]),
    bare("g;", "", "latest bookmark"),
    bare("zt", "", "scroll cursor top"),
    key("\u{2191}", "", "cursor up", "", &[("C-\u{2191}", "volume +")]),
    key("\u{2193}", "", "cursor down", "", &[("C-\u{2193}", "volume \u{2212}")]),
    bare("\u{2190}", "", "seek \u{2212}3.5"),
    bare("\u{2192}", "", "set start time"),
];

// ── Per-row screens ──────────────────────────────────────────────────

/// Title shown for each keyboard-row screen, plus the gamepad screen.
const ROW_TITLES: &[&str] = &[
    "NUMBER / SYMBOL ROW",
    "UPPER ROW",
    "HOME ROW",
    "BOTTOM ROW",
    "MODIFIERS & SEQUENCES",
];

/// Number of keyboard-row screens (the gamepad is a 6th screen handled by the
/// gamepad overlay, reached by cycling past the last row).
pub const ROW_COUNT: usize = 5;

/// The keys shown on row screen `idx` (0..ROW_COUNT). The row-leader key
/// (Backspace/Tab/Esc/Shift) is appended so every key in the physical row is
/// represented.
fn row_keys(idx: usize) -> Vec<&'static KeyDef> {
    match idx {
        0 => NUMBER_ROW.iter().chain(std::iter::once(&BACKSPACE)).collect(),
        1 => std::iter::once(&TAB_KEY).chain(UPPER_ROW.iter()).collect(),
        2 => std::iter::once(&ESC_KEY).chain(HOME_ROW.iter()).collect(),
        3 => std::iter::once(&SHIFT_KEY).chain(BOTTOM_ROW.iter()).collect(),
        _ => MOD_SEQ_ROW.iter().collect(),
    }
}

/// Index of the first bound key in a row (so the highlight starts on something
/// useful), else 0.
fn first_bound(keys: &[&KeyDef]) -> usize {
    keys.iter()
        .position(|d| !d.action.is_empty() || !d.shift_action.is_empty() || !d.modifiers.is_empty())
        .unwrap_or(0)
}


/// A longer, sentence-length explanation for a binding, keyed by its short
/// action label (the same strings used in the row definitions above). Returns
/// `None` for self-explanatory bindings (cursor moves, seeks, font), whose
/// short label already says everything; those rows render without a blurb.
///
/// Each blurb explains what the binding does, then ends with a code reference
/// (`→ module::function — file.rs`) pointing at the handler that implements it.
/// All handlers are reached through `dispatch_action` in
/// `src/input/keymap.rs`; the reference names the leaf function that does the
/// work, or `dispatch_action (keymap.rs)` when the logic is inline in the
/// match arm itself. Keep these to a few sentences — they are word-wrapped
/// into the detail panel's free width, so there is room, but the panel still
/// shares the screen with the keycap strip. When a keybind's handler moves,
/// update the reference here too.
fn describe(label: &str) -> Option<&'static str> {
    // Shift-action labels carry a "X: " prefix (e.g. "O: −60", "R: prev vocab")
    // so the keycap can show which physical key + Shift triggers them. Strip a
    // leading single-char prefix of the form "<c>: " before matching so the
    // shift variant shares its base description where the meaning is identical.
    let key = strip_shift_prefix(label);
    let d = match key {
        // ── Page / cursor navigation ──
        "pg fwd" => "Turn one page forward in the e-reader pagination (the cursor \
follows to the new page top). Aliased on x and Ctrl+f / Ctrl+u. \
→ navigation::page_forward — src/input/navigation.rs",
        "pg back" => "Turn one page backward in the e-reader pagination. Aliased \
on y and Ctrl+b. → navigation::page_backward — src/input/navigation.rs",
        "page ↓" => "Page forward by one screen (Space). \
→ navigation::page_forward — src/input/navigation.rs",
        "page ↑" => "Page backward by one screen (Shift+Space). \
→ navigation::page_backward — src/input/navigation.rs",
        "cursor ↓" | "cursor down" => "Move the cursor down one dialogue line, \
turning the page when it reaches the bottom. \
→ navigation::cursor_next_dialogue — src/input/navigation.rs",
        "cursor ↑" | "cursor up" => "Move the cursor up one dialogue line, \
turning the page when it reaches the top. \
→ navigation::cursor_prev_line — src/input/navigation.rs",
        "prev dlg" => "Jump the cursor to the previous line of dialogue, skipping \
speaker names, stage directions, and blank lines. \
→ navigation::jump_to_prev_dialogue — src/input/navigation.rs",
        "next dlg" => "Jump the cursor to the next line of dialogue, skipping \
speaker names, stage directions, and blank lines. \
→ navigation::jump_to_next_dialogue — src/input/navigation.rs",
        "go to start" => "Jump to the very first line of the work (gg). \
→ navigation::jump_to_start — src/input/navigation.rs",
        "go to end" => "Jump to the very last line of the work (G). \
→ navigation::jump_to_end — src/input/navigation.rs",
        "scroll cursor top" | "zt…" => "Scroll the viewport so the cursor line \
sits at the top of the page (vim zt). \
→ navigation::scroll_cursor_top — src/input/navigation.rs",

        // ── Chapters / scenes ──
        "prev ch" => "Jump to the previous chapter boundary (a line marked \
is_chapter in lit.db). → navigation::jump_to_prev_chapter — src/input/navigation.rs",
        "next ch" => "Jump to the next chapter boundary (a line marked \
is_chapter in lit.db). → navigation::jump_to_next_chapter — src/input/navigation.rs",
        "prev scene" => "Jump to the previous scene/act section heading. \
→ navigation::jump_to_prev_section — src/input/navigation.rs",
        "next scene" => "Jump to the next scene/act section heading. \
→ navigation::jump_to_next_section — src/input/navigation.rs",

        // ── Bookmarks ──
        "bookmark" => "Toggle a bookmark on the current line (writes/removes a \
bookmark row in lit.db and updates the sign column). \
→ bookmarks::toggle_bookmark — src/input/actions/bookmarks.rs",
        "prev bkmk" => "Jump the cursor to the previous bookmarked line in this \
work. → navigation::prev_bookmark — src/input/navigation.rs",
        "next bkmk" => "Jump the cursor to the next bookmarked line in this work. \
→ navigation::next_bookmark — src/input/navigation.rs",
        "latest bookmark" => "Jump to the most recently added bookmark (g; \
sequence). → bookmarks::jump_to_recent_bookmark — src/input/actions/bookmarks.rs",
        "bookmarks" => "Open the bookmark picker: a list of this work's bookmarks \
to jump to. → pickers::open_bookmark_picker — src/input/actions/pickers.rs",

        // ── Pickers / overlays ──
        "lib picker" => "Open the library picker to switch works: browse by \
author, then by work, with fuzzy filtering. The chosen work opens at your \
saved position, or at the first dialogue line on first open. \
→ pickers::open_library_picker_from_reader — src/input/actions/pickers.rs \
(opens via app::display_work_at_with_prepared — src/app.rs)",
        "media picker" => "Open the media picker to choose which audio file syncs \
with this work; the MPV socket path is derived from the file path. \
→ pickers::open_media_picker — src/input/actions/pickers.rs",
        "conc picker" => "Open the concordance picker: a stopword-filtered list \
of words used across this author's works. Pick a word to start cross-work \
concordance navigation, then step through occurrences with r / R. \
→ concordance::open_picker — src/input/actions/concordance.rs",
        "recent" => "Open the recent-works picker (most-recently-used works). \
→ pickers::open_recent_picker — src/input/actions/pickers.rs",
        "settings" => "Open the settings overlay (margins, offsets, and other \
reader options). → settings::open_settings — src/input/actions/settings.rs",
        "keybinds" => "Open this keyboard-shortcut overlay. \
→ pickers::open_keybinds_overlay — src/input/actions/pickers.rs (drawing: \
src/ui/keybinds_overlay.rs)",
        "search" => "Open the in-text search bar; Escape restores the pre-search \
reader position. → OpenSearch arm (inline) → search::clear_search — \
src/input/keymap.rs, src/input/search.rs",
        "next match" => "Move to the next search match — or, when a concordance \
is active, the next concordance hit within this work. \
→ SearchNextMatch arm → search::next_match / concordance::concordance_next_in_work \
— src/input/keymap.rs",
        "prev match" => "Move to the previous search match — or the previous \
concordance hit within this work when a concordance is active. \
→ SearchPrevMatch arm → search::prev_match / concordance::concordance_prev_in_work \
— src/input/keymap.rs",

        // ── Gloss / echo system ──
        "gloss tog" => "Show or hide the gloss overlay for the current passage. \
A gloss is a saved AI commentary on a highlighted passage — either a \
teacher-style Q&A note or an inner-monologue cross-reference. If a gloss is \
already loaded it reopens that one without re-querying the database. \
→ gloss::toggle_overlay — src/input/actions/gloss.rs",
        "gloss pick" => "Open a fuzzy-filterable list of every passage in this \
work that has a saved gloss (teacher-generic or inner-monologue). Each row \
shows the speaker, the first source line, and the citation; confirming loads \
that passage's glosses into the overlay and jumps the reader to it. \
→ pickers::open_gloss_picker — src/input/actions/pickers.rs (confirm: \
handle_gloss_picker_key in src/input/keymap.rs)",
        "echo turns" => "List every speaker turn in this work that already has \
cached cross-work echoes — thematically similar passages found by a prior echo \
search and stored in lit.db. Selecting a turn jumps the cursor to its first \
line and reopens its stored echoes instantly, with no new Voyage embedding \
call. → echoes::open_echo_turns_picker — src/input/actions/echoes.rs (confirm: \
echoes::confirm_echo_turns_pick)",
        "echoes" => "Run a cross-work echo search on the current speaker turn: \
embed the turn, find thematically similar passages elsewhere in the author's \
works, and show them in the echoes overlay. \
→ echoes::show_echoes_for_cursor_line — src/input/actions/echoes.rs",
        "reopen echoes" => "Reopen the echoes overlay with the most recent echo \
results, without running a new search. \
→ echoes::reopen_echoes — src/input/actions/echoes.rs",

        // ── Vocab ──
        "next vocab" => "Jump the cursor to the next vocabulary word in the work \
(words you have collected for study), independent of any active concordance. \
→ concordance::jump_to_next_vocab — src/input/actions/concordance.rs",
        "prev vocab" => "Jump the cursor to the previous vocabulary word in the \
work. → concordance::jump_to_prev_vocab — src/input/actions/concordance.rs",
        "vocab hi" => "Toggle highlighting of vocabulary words in the text (state \
saved to config). → ToggleVocabHighlight arm → app::apply_vocab_highlighting / \
app::remove_vocab_highlighting — src/input/keymap.rs, src/app.rs",
        "auto vocab" => "Toggle the auto vocabulary popup, which shows definitions \
for the current line's vocab words as the cursor moves. \
→ ToggleVocabPopup arm → app::open_vocab_popup / close_vocab_popup — \
src/input/keymap.rs, src/app.rs",
        "vocab ▶" => "Step forward through the vocabulary popup's words for the \
current line. → handle_vocab_popup_key(.., true) — src/input/keymap.rs",
        "◀ vocab" => "Step backward through the vocabulary popup's words for the \
current line. → handle_vocab_popup_key(.., false) — src/input/keymap.rs",

        // ── Word copy / visual ──
        "copy word" => "Copy the word under the cursor to the clipboard; repeated \
presses cycle outward through adjacent words. \
→ word_copy::word_cycle_copy — src/input/actions/word_copy.rs",
        "copy id" => "Copy the current line's line-mapping id (and media id, when \
present) to the clipboard via wl-copy — useful for debugging and lit.db edits. \
→ CopyLineMappingId arm (inline) — src/input/keymap.rs",
        "collect" => "Collect the word under the cursor into the vocabulary list \
and copy it. → word_copy::word_collect_copy — src/input/actions/word_copy.rs",
        "visual mode" => "Enter visual selection mode (vim-style); then y yanks, \
i shows echoes for the selection, Return opens the action popup, Esc/V exits. \
→ visual::enter_visual_mode — src/input/visual.rs",

        // ── MPV / audio ──
        "play/pause" => "Toggle MPV playback (play or pause the synced audio). \
→ TogglePlayback arm → search::toggle_playback — src/input/keymap.rs, \
src/input/search.rs",
        "toggle speed" => "Toggle MPV playback speed between 1.0× and 1.3× (shows \
a toast). → TogglePlaybackSpeed arm (inline) → MpvCommand::SetSpeed — \
src/input/keymap.rs",
        "seek −3.5" => "Seek MPV back 3.5 seconds. \
→ do_mpv_seek(state, -3.5) — src/input/keymap.rs",
        "seek +3.5" => "Seek MPV forward 3.5 seconds. \
→ do_mpv_seek(state, 3.5) — src/input/keymap.rs",
        "−60" => "Seek MPV back 60 seconds (Shift+o). \
→ do_mpv_seek(state, -60.0) — src/input/keymap.rs",
        "+60" => "Seek MPV forward 60 seconds (Shift+e). \
→ do_mpv_seek(state, 60.0) — src/input/keymap.rs",
        "volume +" => "Raise MPV volume by 5. \
→ VolumeUp arm → MpvCommand::VolumeAdjust(5.0) — src/input/keymap.rs",
        "volume −" => "Lower MPV volume by 5. \
→ VolumeDown arm → MpvCommand::VolumeAdjust(-5.0) — src/input/keymap.rs",
        "sync tog" => "Toggle playback sync: when on, the cursor and page follow \
MPV's audio position automatically; a toast shows the new state. \
→ TogglePlaybackSync arm (inline) — src/input/keymap.rs",

        // ── Timestamps ──
        "start time" | "set start time" => "Set the audio start timestamp for the \
current line from MPV's current playback position and write it to lit.db \
(updates the sign column). → timestamps::set_start_time — src/input/timestamps.rs",
        "set end time" => "Set the audio end timestamp for the current line from \
MPV's current playback position. \
→ timestamps::set_end_time — src/input/timestamps.rs",
        "set chapter" => "Mark the current line as a chapter/scene boundary at \
MPV's current playback position. \
→ timestamps::set_chapter — src/input/timestamps.rs",
        "delete ts" => "Delete the current line's saved timestamp from lit.db \
(undoable). → timestamps::delete_timestamp — src/input/timestamps.rs",
        "nudge −0.2" => "Nudge the current line's start timestamp 0.2s earlier. \
→ timestamps::nudge_start_backward — src/input/timestamps.rs",
        "+0.2" => "Nudge the current line's start timestamp 0.2s later (Shift+p). \
→ timestamps::nudge_start_forward — src/input/timestamps.rs",
        "play from ts" => "Seek MPV to the current line's saved start timestamp \
and play from there. → timestamps::play_current_line — src/input/timestamps.rs",
        "clear AB" => "Clear the A–B repeat range / exit reader sub-modes (Esc). \
→ escape::escape_reader_mode — src/input/actions/escape.rs",

        // ── Fonts ──
        "font →" => "Cycle to the next font in the font-cycling list. \
→ app::cycle_font(.., true) — src/app.rs",
        "←" => "Cycle to the previous font in the font-cycling list (Shift+f). \
→ app::cycle_font(.., false) — src/app.rs",
        "font +" => "Increase the reader font size by one step (saved to config). \
→ app::adjust_font_size(.., 1) — src/app.rs",
        "font −" => "Decrease the reader font size by one step (saved to config). \
→ app::adjust_font_size(.., -1) — src/app.rs",
        "reset font" => "Reset the reader font size to the default. \
→ app::reset_font_size — src/app.rs",
        "font info" => "Show a toast with the current font name and size. \
→ app::show_font_info — src/app.rs",

        // ── Display toggles ──
        "toggle signs" => "Toggle the sign column — the left gutter dots/markers \
that flag timestamps, chapters, bookmarks, and A/B points. \
→ app::toggle_sign_column — src/app.rs",
        "synopsis" => "Show the synopsis overlay for the current scene. \
→ app::show_synopsis_overlay — src/app.rs (Ctrl+h toggles the side panel via \
app::toggle_synopsis).",
        "translations" => "Toggle the parallel translation column alongside the \
text (pauses MPV first). → app::toggle_translations — src/app.rs",
        "dim tog" => "Toggle dimming of lines outside the current A–B sync range \
and refresh the highlight. → ToggleDim arm (inline) — src/input/keymap.rs",
        "title tog" => "Toggle the title bar (work author/title and current \
scene). → ToggleTitleBar arm (inline) — src/input/keymap.rs",
        "save+quit" => "Save the current reading position, tell MPV to quit, and \
close the window. → SaveAndQuit arm → app::save_position — src/input/keymap.rs, \
src/app.rs",
        "debug log" => "Toggle debug logging on/off (briefly shows a gear/blocked \
icon). The log file is linux-lit-dev.log / linux-lit-release.log. \
→ ToggleDebugLogging arm (inline) — src/input/keymap.rs, src/logging.rs",

        _ => return None,
    };
    Some(d)
}

/// Strip a leading shift-key prefix of the form `"<char>: "` from a shift-action
/// label so the variant matches its base description (e.g. `"O: −60"` → `"−60"`,
/// `"R: prev vocab"` → `"prev vocab"`). Returns the label unchanged if it has no
/// such prefix.
fn strip_shift_prefix(label: &str) -> &str {
    let mut chars = label.char_indices();
    if let (Some((_, _c0)), Some((i1, c1))) = (chars.next(), chars.next()) {
        if c1 == ':' {
            // skip ": " (colon at i1, then a space)
            let rest = &label[i1 + 1..];
            return rest.strip_prefix(' ').unwrap_or(rest);
        }
    }
    label
}

/// Break `text` into lines that each fit within `max_w` px at the current Cairo
/// font, splitting on spaces. Long single words are left intact (rare here).
fn wrap_to_width(cr: &gtk4::cairo::Context, text: &str, max_w: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        let w = cr.text_extents(&trial).map(|e| e.width()).unwrap_or(0.0);
        if w > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// ── Drawing (per-row screen) ─────────────────────────────────────────

/// Draw one row-screen: a keycap strip across the top (one key highlighted)
/// and a detail panel below listing the highlighted key's full bindings.
fn draw_row_screen(
    cr: &gtk4::cairo::Context,
    row_idx: usize,
    selected: usize,
    widget_w: f64,
    widget_h: f64,
) {
    // Full-screen scrim
    cr.set_source_rgba(0.341, 0.322, 0.475, 0.95);
    cr.rectangle(0.0, 0.0, widget_w, widget_h);
    let _ = cr.fill();

    let keys = row_keys(row_idx);
    let sel = selected.min(keys.len().saturating_sub(1));

    // ── Header ──
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
    cr.set_font_size(20.0);
    cr.set_source_rgb(0.96, 0.94, 0.90);
    let title = ROW_TITLES.get(row_idx).copied().unwrap_or("");
    let header = format!("Row {} of {}  —  {}", row_idx + 1, ROW_COUNT + 1, title);
    let he = cr.text_extents(&header).unwrap();
    let _ = cr.move_to((widget_w - he.width()) / 2.0, 48.0);
    let _ = cr.show_text(&header);

    // ── Keycap strip ──
    // Fit `n` caps across the available width.
    let margin = 40.0;
    let avail_w = widget_w - 2.0 * margin;
    let n = keys.len().max(1) as f64;
    let cap_gap = 8.0;
    let cap_w = ((avail_w - (n - 1.0) * cap_gap) / n).min(110.0).max(36.0);
    let cap_h = 60.0;
    let strip_w = n * cap_w + (n - 1.0) * cap_gap;
    let strip_x = (widget_w - strip_w) / 2.0;
    let strip_y = 78.0;

    for (i, def) in keys.iter().enumerate() {
        let x = strip_x + i as f64 * (cap_w + cap_gap);
        let bound = !def.action.is_empty() || !def.shift_action.is_empty() || !def.modifiers.is_empty();
        let is_sel = i == sel;

        // Selected glow
        if is_sel {
            cr.set_source_rgba(0.227, 0.353, 0.616, 0.30);
            rounded_rect(cr, x - 3.0, strip_y - 3.0, cap_w + 6.0, cap_h + 6.0, 9.0);
            let _ = cr.fill();
        }

        // Cap background
        if is_sel {
            cr.set_source_rgb(0.875, 0.910, 0.957);
        } else if bound {
            cr.set_source_rgb(0.949, 0.914, 0.882);
        } else {
            cr.set_source_rgb(1.0, 0.98, 0.953);
        }
        rounded_rect(cr, x, strip_y, cap_w, cap_h, 7.0);
        let _ = cr.fill();

        // Cap border
        if is_sel {
            cr.set_source_rgb(0.227, 0.353, 0.616);
            cr.set_line_width(2.0);
        } else {
            cr.set_source_rgb(0.72, 0.66, 0.66);
            cr.set_line_width(1.0);
        }
        rounded_rect(cr, x + 0.5, strip_y + 0.5, cap_w - 1.0, cap_h - 1.0, 7.0);
        let _ = cr.stroke();

        // Unshifted glyph (centered-ish)
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        cr.set_font_size(if def.unshifted.chars().count() > 1 { 18.0 } else { 22.0 });
        if is_sel {
            cr.set_source_rgb(0.149, 0.251, 0.478);
        } else if bound {
            cr.set_source_rgb(0.341, 0.322, 0.475);
        } else {
            cr.set_source_rgb(0.596, 0.576, 0.647);
        }
        let ge = cr.text_extents(def.unshifted).unwrap();
        let _ = cr.move_to(x + (cap_w - ge.width()) / 2.0 - ge.x_bearing(), strip_y + 28.0);
        let _ = cr.show_text(def.unshifted);

        // Shifted glyph (small, top-right)
        if !def.shifted.is_empty() {
            cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(12.0);
            cr.set_source_rgb(0.565, 0.478, 0.663);
            let se = cr.text_extents(def.shifted).unwrap();
            let _ = cr.move_to(x + cap_w - se.width() - 6.0, strip_y + 16.0);
            let _ = cr.show_text(def.shifted);
        }

        // Tiny bare-action hint under the glyph (truncated; full text is in panel)
        if !def.action.is_empty() {
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(10.0);
            cr.set_source_rgb(0.157, 0.412, 0.514);
            let hint = truncate_to_width(cr, def.action, cap_w - 10.0);
            let _ = cr.move_to(x + 5.0, strip_y + cap_h - 8.0);
            let _ = cr.show_text(&hint);
        }
    }

    // ── Detail panel ──
    let panel_x = margin;
    let panel_y = strip_y + cap_h + 28.0;
    let panel_w = widget_w - 2.0 * margin;
    let def = keys[sel];

    // Each binding row: (modifier label, action label, color, optional blurb).
    let mut rows: Vec<(&str, String, (f64, f64, f64), Option<&'static str>)> = Vec::new();
    if !def.action.is_empty() {
        rows.push((def.unshifted, def.action.to_string(), (0.157, 0.412, 0.514), describe(def.action))); // pine
    }
    if !def.shift_action.is_empty() {
        rows.push(("Shift", def.shift_action.to_string(), (0.565, 0.478, 0.663), describe(def.shift_action))); // iris
    }
    for &(combo, act) in def.modifiers {
        let (label, col) = if combo.starts_with("M-") && !combo.contains("C-") {
            ("Alt", (0.706, 0.388, 0.478)) // rose
        } else if combo.contains("S-") {
            ("Ctrl+Shift", (0.204, 0.506, 0.341)) // green
        } else {
            ("Ctrl", (0.557, 0.420, 0.208)) // gold
        };
        rows.push((label, act.to_string(), col, describe(act)));
    }
    if rows.is_empty() {
        rows.push(("", "(unbound)".to_string(), (0.596, 0.576, 0.647), None));
    }

    // Layout constants for the detail panel.
    let act_x = panel_x + 180.0; // action label column
    let desc_x = panel_x + 340.0; // wrapped blurb column
    let desc_max_w = panel_x + panel_w - 24.0 - desc_x; // free width for blurbs
    let row_pad: f64 = 12.0; // vertical breathing room per binding row
    let desc_line_h: f64 = 21.0; // line height inside a wrapped blurb
    let base_row_h: f64 = 30.0; // height of a row with no blurb

    // Pre-pass: wrap each blurb and record per-row height so the panel grows to
    // fit. Wrapping must use the same font the blurb is drawn with.
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(14.0);
    let wrapped: Vec<Vec<String>> = rows
        .iter()
        .map(|(_, _, _, blurb)| match blurb {
            Some(text) => wrap_to_width(cr, text, desc_max_w),
            None => Vec::new(),
        })
        .collect();
    let row_heights: Vec<f64> = wrapped
        .iter()
        .map(|lines| {
            let blurb_h: f64 = if lines.is_empty() { 0.0 } else { lines.len() as f64 * desc_line_h };
            base_row_h.max(blurb_h) + row_pad
        })
        .collect();
    let rows_total_h: f64 = row_heights.iter().sum();
    let panel_h = 60.0 + rows_total_h + 8.0;

    // Panel background
    cr.set_source_rgb(0.965, 0.949, 0.925);
    rounded_rect(cr, panel_x, panel_y, panel_w, panel_h, 10.0);
    let _ = cr.fill();
    cr.set_source_rgb(0.886, 0.847, 0.784);
    rounded_rect(cr, panel_x + 0.5, panel_y + 0.5, panel_w - 1.0, panel_h - 1.0, 10.0);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Panel title: the key glyph
    cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
    cr.set_font_size(24.0);
    cr.set_source_rgb(0.149, 0.251, 0.478);
    let _ = cr.move_to(panel_x + 22.0, panel_y + 36.0);
    let mut ttl = def.unshifted.to_string();
    if !def.shifted.is_empty() {
        ttl.push_str(&format!("   ({} = shift)", def.shifted));
    }
    let _ = cr.show_text(&ttl);

    // Binding rows
    let mut ry = panel_y + 60.0;
    for (i, (label, act, col, _)) in rows.iter().enumerate() {
        let top = ry; // baseline of the modifier/action line
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
        cr.set_font_size(15.0);
        cr.set_source_rgb(0.4, 0.38, 0.45);
        let _ = cr.move_to(panel_x + 24.0, top);
        let _ = cr.show_text(label);
        cr.set_source_rgb(col.0, col.1, col.2);
        cr.set_font_size(16.0);
        let _ = cr.move_to(act_x, top);
        let _ = cr.show_text(act);

        // Wrapped blurb to the right, filling the panel's free width.
        let lines = &wrapped[i];
        if !lines.is_empty() {
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(14.0);
            cr.set_source_rgb(0.376, 0.357, 0.439);
            for (li, line) in lines.iter().enumerate() {
                let _ = cr.move_to(desc_x, top + li as f64 * desc_line_h);
                let _ = cr.show_text(line);
            }
        }

        ry += row_heights[i];
    }

    // ── Footer hint ──
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(14.0);
    cr.set_source_rgb(0.78, 0.76, 0.82);
    let foot = "Esc close  \u{00b7}  n/p cycle rows  \u{00b7}  j/k or \u{2190}/\u{2192} move highlight";
    let fe = cr.text_extents(foot).unwrap();
    let _ = cr.move_to((widget_w - fe.width()) / 2.0, widget_h - 28.0);
    let _ = cr.show_text(foot);
}

/// Truncate `text` so it fits within `max_w` px, appending "…" if cut.
fn truncate_to_width(cr: &gtk4::cairo::Context, text: &str, max_w: f64) -> String {
    if cr.text_extents(text).map(|e| e.width()).unwrap_or(0.0) <= max_w {
        return text.to_string();
    }
    let mut s = String::new();
    for ch in text.chars() {
        let mut trial = s.clone();
        trial.push(ch);
        trial.push('\u{2026}');
        if cr.text_extents(&trial).map(|e| e.width()).unwrap_or(0.0) > max_w {
            s.push('\u{2026}');
            return s;
        }
        s.push(ch);
    }
    s
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 3.0 * pi / 2.0);
    cr.close_path();
}


// ── Public API ───────────────────────────────────────────────────────

pub struct KeybindsOverlay {
    pub overlay: Overlay,
    drawing_area: DrawingArea,
    row_index: Rc<std::cell::Cell<usize>>,
    selected: Rc<std::cell::Cell<usize>>,
}

impl KeybindsOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let drawing_area = DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::Fill)
            .visible(false)
            .build();
        drawing_area.add_css_class("keybinds-overlay-canvas");

        let row_index = Rc::new(std::cell::Cell::new(0usize));
        let selected = Rc::new(std::cell::Cell::new(first_bound(&row_keys(0))));

        let row_draw = row_index.clone();
        let sel_draw = selected.clone();
        drawing_area.set_draw_func(move |_area, cr, w, h| {
            draw_row_screen(cr, row_draw.get(), sel_draw.get(), w as f64, h as f64);
        });

        KeybindsOverlay { overlay, drawing_area, row_index, selected }
    }

    pub fn show(&self) {
        // Always open at the first row.
        self.row_index.set(0);
        self.selected.set(first_bound(&row_keys(0)));
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    pub fn hide(&self) {
        self.drawing_area.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.drawing_area.is_visible()
    }

    /// Cycle to the next row. Returns false when cycling past the last keyboard
    /// row (caller switches to the gamepad screen).
    pub fn next_row(&self) -> bool {
        let cur = self.row_index.get();
        if cur + 1 >= ROW_COUNT {
            return false; // caller should advance to the gamepad screen
        }
        let next = cur + 1;
        self.row_index.set(next);
        self.selected.set(first_bound(&row_keys(next)));
        self.drawing_area.queue_draw();
        true
    }

    /// Cycle to the previous row. Returns false when at the first row (caller
    /// wraps to the gamepad screen).
    pub fn prev_row(&self) -> bool {
        let cur = self.row_index.get();
        if cur == 0 {
            return false; // caller should wrap to the gamepad screen
        }
        let prev = cur - 1;
        self.row_index.set(prev);
        self.selected.set(first_bound(&row_keys(prev)));
        self.drawing_area.queue_draw();
        true
    }

    /// Jump directly to the last keyboard row (used when entering from the
    /// gamepad screen via `p`).
    pub fn show_last_row(&self) {
        let last = ROW_COUNT - 1;
        self.row_index.set(last);
        self.selected.set(first_bound(&row_keys(last)));
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    /// Move the key highlight within the current row (wraps).
    pub fn move_selection(&self, delta: i32) {
        let len = row_keys(self.row_index.get()).len();
        if len == 0 {
            return;
        }
        let cur = self.selected.get() as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.selected.set(next);
        self.drawing_area.queue_draw();
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.drawing_area);
    }
}
