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
    bare("+", "1", "show chapter"),
    key("[", "2", "prev scene", "2: prev ch", &[("M-[", "col layout")]),
    key("{", "3", "next scene", "3: next ch", &[]),
    key("(", "4", "prev bkmk", "4: prev ch", &[]),
    key("&", "5", "next bkmk", "5: next ch", &[]),
    ub("=", "6"),
    ub(")", "7"),
    bare("}", "8", "prev ch"),
    bare("]", "9", "next ch"),
    key("*", "0", "", "reset font", &[]),
    key("!", "%", "", "", &[("C-!", "font \u{2212}")]),
    key("|", "`", "", "", &[("C-|", "font +")]),
];
const BACKSPACE: KeyDef = bare("\u{232b}", "", "delete ts");

const UPPER_ROW: &[KeyDef] = &[
    key(";", ":", "prev bkmk", ":: toggle speed", &[]),
    key(",", "<", "prev speaker", "<: prev dlg", &[("C-,", "settings")]),
    key(".", ">", "", "", &[("C-.", "bookmarks")]),
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("S-C-p", "conc word"), ("M-p", "phrase hl")]),
    key("y", "Y", "pg back", "", &[("C-y", "copy id")]),
    key("f", "F", "next font", "F: prev font", &[("M-f", "font info")]),
    key("g", "G", "", "", &[("C-g", "gloss tog"), ("S-C-g", "last gloss"), ("M-g", "gloss pick"), ("M-g", "gloss from jrnl"), ("C-g", "view gloss")]),
    key("c", "C", "toggle ch start", "C: show chapter", &[("C-c", "set track mark"), ("C-M-c", "conc list")]),
    key("r", "R", "next conc", "R: prev conc", &[("C-r", "next vocab"), ("S-C-r", "prev vocab"), ("M-r", "conc works")]),
    key("l", "L", "toggle signs", "", &[("S-C-l", "save+quit"), ("l", "verse audio: play/stop"), ("L", "verse audio: pick voice")]),
    key("/", "?", "search", "?: search back", &[("C-/", "keybinds")]),
    ub("@", "^"),
    key("\\", "#", "conc picker", "◀ vocab", &[("C-\\", "lib picker"), ("M-\\", "vocab hi")]),
];
const TAB_KEY: KeyDef = key("Tab", "", "play/pause", "", &[("C-Tab", "last overlay")]);

const HOME_ROW: &[KeyDef] = &[
    key("a", "A", "play/pause", "", &[("C-a", "authorship"), ("S-C-a", "attr set")]),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[("C-e", "BCP echoes"), ("S-C-e", "reopen BCP echoes"), ("M-e", "BCP echo turns"), ("e", "synopsis edit (vim)")]),
    key("u", "U", "start time", "U: undo ts", &[("M-u", "set end time")]),
    key("i", "I", "2-col translation", "", &[("M-i", "scansion"), ("C-M-i", "inline translation"), ("C-i", "page image"), ("S-C-i", "calibrate pages")]),
    key("d", "D", "", "", &[("C-d", "debug log"), ("M-d", "dim tog")]),
    key("h", "H", "synopsis", "H: auto vocab", &[("C-h", "synopsis side")]),
    key("t", "T", "", "", &[("S-C-t", "nav test"), ("M-t", "theme next"), ("M-S-T", "theme prev")]),
    key("n", "N", "next match", "N: prev match", &[]),
    bare("s", "S", "sync tog"),
    key("-", "_", "prev work", "", &[("C--", "recent")]),
];
const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

const BOTTOM_ROW: &[KeyDef] = &[
    bare("'", "\"", "next bkmk"),
    key("q", "Q", "next speaker", "Q: next dlg", &[]),
    key("j", "J", "cursor \u{2193}", "J: next speaker", &[("C-j", "journal tog"), ("M-j", "jrnl Q&A picker"), ("C-j", "view jrnl"), ("C-S-j", "move jrnl band")]),
    key("k", "K", "cursor \u{2191}", "K: prev speaker", &[]),
    bare("x", "X", "pg fwd"),
    bare("b", "B", ""),
    key("m", "M", "bookmark", "", &[("C-m", "media picker")]),
    key("w", "W", "copy word", "W: collect", &[("M-w", "Shx echoes"), ("C-w", "Shx echo turns"), ("S-C-w", "reopen Shx echoes")]),
    key("v", "V", "vim copy", "V: visual mode", &[("v", "voice: add/remove"), ("C-v", "voice: cycle")]),
    bare("z", "Z", "vocab ▶"),
];

const SHIFT_KEY: KeyDef = ub("Shift", "");
/// The spacebar sits below the bottom row physically, so it is appended to the
/// BOTTOM ROW screen — and repeated on the MODIFIERS & SEQUENCES screen.
const SPACE_KEY: KeyDef = bare("Space", "", "play from ts");

/// Row 5: modifiers, sequences, and arrows gathered into one screen.
const MOD_SEQ_ROW: &[KeyDef] = &[
    SPACE_KEY,
    bare("gg", "", "go to start"),
    key("G", "", "", "go to end", &[]),
    bare("g;", "", "latest bookmark"),
    key("\u{2191}", "", "cursor up", "pg back btm", &[("C-\u{2191}", "volume +")]),
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
        3 => std::iter::once(&SHIFT_KEY)
            .chain(BOTTOM_ROW.iter())
            .chain(std::iter::once(&SPACE_KEY))
            .collect(),
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

/// Map a GTK keyval name for a symbol key to the cap glyph used in the row
/// tables (`unshifted` field). Single-character letter/digit names are NOT in
/// this table — `find_cap` matches those by identity. Returns `None` for names
/// with no symbol cap.
fn key_name_to_glyph(key_name: &str) -> Option<&'static str> {
    Some(match key_name {
        "slash" => "/",
        "comma" => ",",
        "period" => ".",
        "parenleft" => "(",
        "parenright" => ")",
        "ampersand" => "&",
        "bracketleft" => "[",
        "bracketright" => "]",
        "braceleft" => "{",
        "braceright" => "}",
        "backslash" => "\\",
        "minus" => "-",
        "apostrophe" => "'",
        "semicolon" => ";",
        "plus" => "+",
        "asterisk" => "*",
        "exclam" => "!",
        "bar" => "|",
        "at" => "@",
        "dollar" => "$",
        "equal" => "=",
        _ => return None,
    })
}

/// Resolve an incoming GTK key name to the `(row_idx, cap_idx)` of the first cap
/// whose `unshifted` glyph matches. Symbol names go through
/// `key_name_to_glyph`; everything else (letters/digits) is matched by identity.
/// Returns `None` when no cap matches (the caller consumes the key as a no-op).
///
/// Assumes each unshifted glyph is unique across rows — if a glyph were ever
/// duplicated, this would jump to the first occurrence. Digit keys never match:
/// the number-row caps store digits in the `shifted` slot, not `unshifted`, so
/// `1`..`0` are jump no-ops by design (reach those caps with the arrow keys).
fn find_cap(key_name: &str) -> Option<(usize, usize)> {
    let glyph = key_name_to_glyph(key_name).unwrap_or(key_name);
    for row in 0..ROW_COUNT {
        let keys = row_keys(row);
        if let Some(idx) = keys.iter().position(|d| d.unshifted == glyph) {
            return Some((row, idx));
        }
    }
    None
}

/// A longer, sentence-length explanation for a binding, keyed by its short
/// action label (the same strings used in the row definitions above). Returns
/// `None` for self-explanatory bindings (cursor moves, seeks, font), whose
/// short label already says everything; those rows render without a blurb.
///
/// Each blurb explains what the binding does, then ends with a code reference
/// (`-> module::function — file.rs`) pointing at the handler that implements it.
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
        "pg fwd" => "Turn one page forward (aliased on x). \
-> navigation::page_forward — src/input/navigation.rs",
        "pg back" => "Turn one page backward (aliased on y). \
-> navigation::page_backward — src/input/navigation.rs",
        "page ↓" => "Page forward one screen (Space). \
-> navigation::page_forward — src/input/navigation.rs",
        "page ↑" => "Page backward one screen (Shift+Space). \
-> navigation::page_backward — src/input/navigation.rs",
        "cursor ↓" | "cursor down" => "Cursor down one dialogue line (turns the \
page at the bottom); seeks audio to the line. \
-> navigation::cursor_next_dialogue — src/input/navigation.rs",
        "cursor ↑" | "cursor up" => "Cursor up one dialogue line; seeks audio to \
the line. \
-> navigation::cursor_prev_line — src/input/navigation.rs",
        "prev dlg" => "Jump to the previous dialogue line. \
-> navigation::jump_to_prev_dialogue — src/input/navigation.rs",
        "next dlg" => "Jump to the next dialogue line. \
-> navigation::jump_to_next_dialogue — src/input/navigation.rs",
        "next speaker" => "Jump to the next speaker turn and seek audio to it. \
-> navigation::jump_to_next_speaker — src/input/navigation.rs",
        "prev speaker" => "Jump to the top of the current speaker turn; from there, \
the previous turn. Seeks audio. \
-> navigation::jump_to_prev_speaker — src/input/navigation.rs",
        "go to start" => "Jump to the first line (gg). \
-> navigation::jump_to_start — src/input/navigation.rs",
        "go to end" => "Jump to the last line (G). \
-> navigation::jump_to_end — src/input/navigation.rs",
        "pg back btm" => "Page backward, landing the cursor on the new page's \
bottom line (Shift+Up). \
-> navigation::page_backward_bottom — src/input/navigation.rs",

        // ── Chapters / scenes ──
        "prev ch" => "Jump to the previous chapter (prose) or act (play). \
-> navigation::jump_to_prev_chapter — src/input/navigation.rs",
        "next ch" => "Jump to the next chapter (prose) or act (play). \
-> navigation::jump_to_next_chapter — src/input/navigation.rs",
        "prev scene" => "Jump to the current scene/chapter's first line; pressed \
there, the previous one. \
-> navigation::jump_to_prev_section — src/input/navigation.rs",
        "next scene" => "Jump to the next scene (plays) or chapter (prose). \
-> navigation::jump_to_next_section — src/input/navigation.rs",

        // ── Bookmarks ──
        "bookmark" => "Toggle a bookmark on the current line. \
-> bookmarks::toggle_bookmark — src/input/actions/bookmarks.rs",
        "prev bkmk" => "Jump to the previous bookmark. \
-> navigation::prev_bookmark — src/input/navigation.rs",
        "next bkmk" => "Jump to the next bookmark. \
-> navigation::next_bookmark — src/input/navigation.rs",
        "latest bookmark" => "Jump to the most recently added bookmark (g;). \
-> bookmarks::jump_to_recent_bookmark — src/input/actions/bookmarks.rs",
        "bookmarks" => "Open the bookmark picker. \
-> pickers::open_bookmark_picker — src/input/actions/pickers.rs",

        // ── Pickers / overlays ──
        "lib picker" => "Open the library picker to switch works. \
-> pickers::open_library_picker_from_reader — src/input/actions/pickers.rs \
(opens via app::display_work_at_with_prepared — src/app.rs)",
        "media picker" => "Open the media picker to choose the synced audio file. \
-> pickers::open_media_picker — src/input/actions/pickers.rs",
        "conc picker" => "Open the concordance picker (author-wide word list; \
step hits with r / R). \
-> concordance::open_picker — src/input/actions/concordance.rs",
        "conc word" => "Open the concordance word picker. \
-> pickers::open_concordance_word_picker — src/input/actions/pickers.rs",
        "phrase hl" => "Cycle the karaoke narration highlight for this work's \
class: OFF -> PHRASE (spoken phrase; the default) -> LINE (whole verse line / \
prose sentence). Saved to config. \
-> TogglePhraseHighlight arm — src/input/keymap.rs \
(driver: src/input/phrase_highlight.rs)",
        "conc list" => "Open the concordance occurrence-list picker. \
-> pickers::open_concordance_list_picker — src/input/actions/pickers.rs",
        "conc works" => "Open the concordance works picker. \
-> pickers::open_concordance_works_picker — src/input/actions/pickers.rs",
        "recent" => "Open the recent-works picker. \
-> pickers::open_recent_picker — src/input/actions/pickers.rs",
        "prev work" => "Swap back to the previously open work (like vim's Ctrl-^). \
-> pickers::toggle_previous_work — src/input/actions/pickers.rs",
        "settings" => "Open the settings overlay. \
-> settings::open_settings — src/input/actions/settings.rs",
        "keybinds" => "Open this keyboard-shortcut overlay. \
-> pickers::open_keybinds_overlay / open_keybinds_from_mode — \
src/input/actions/pickers.rs (drawing: src/ui/keybinds_overlay.rs)",
        "search" => "Open in-text search, forward. -> OpenSearch arm \
(inline) -> search::clear_search — src/input/keymap.rs, src/input/search.rs",
        "search back" => "Open in-text search, backward (?). -> OpenSearchBackward \
arm (inline) — src/input/keymap.rs, src/input/search.rs",
        "next match" => "Next search match — or next concordance hit when a \
concordance is active. \
-> SearchNextMatch arm -> search::reactivate_and_step / concordance::concordance_next_in_work \
— src/input/keymap.rs",
        "prev match" => "Previous search match — or previous concordance hit when \
a concordance is active. \
-> SearchPrevMatch arm -> search::reactivate_and_step / concordance::concordance_prev_in_work \
— src/input/keymap.rs",

        // ── Gloss / echo system ──
        "gloss tog" => "Open the gloss overlay for the current passage; its binds \
are on its Ctrl+/ legend. \
-> gloss::toggle_overlay — src/input/actions/gloss.rs; \
gloss::synth_all_prose_blocks — src/input/actions/gloss.rs",
        "gloss pick" => "Open the gloss picker (Alt+t cycles the type filter). \
-> pickers::open_gloss_picker — src/input/actions/pickers.rs (confirm: \
handle_gloss_picker_key in src/input/keymap.rs)",
        "journal tog" => "Open or close the Q&A journal for the current scene; \
its binds are on its Ctrl+/ legend. \
-> journal::toggle_overlay — src/input/actions/journal.rs (overlay keys: \
handle_journal_key in src/input/keymap.rs)",
        "last overlay" => "Reopen the last-used gloss/journal overlay; from inside \
it, close back to the reader. \
-> gloss::toggle_last_overlay — src/input/actions/gloss.rs",
        "jrnl Q&A picker" => "Open Journal Q&A picker. \
-> journal::open_picker_from_reader — src/input/actions/journal.rs",
        "last gloss" => "Reopen the most recently viewed gloss in this work. \
-> gloss::open_last_gloss — src/input/actions/gloss.rs",
        // ── Gloss ↔ journal cross-view (Task 7) ──
        "gloss from jrnl" => "On a journal passage page (Alt+g): open (or create) \
the reader-gloss for the cited passage. \
-> journal::action_gloss_from_journal_passage \
\u{2014} src/input/actions/journal.rs",
        "view jrnl" => "From the gloss overlay (Ctrl+j): open the journal pages \
for the gloss's passage. \
-> journal::view_journal_from_gloss \
\u{2014} src/input/actions/journal.rs",
        "move jrnl band" => "From the journal overlay (Ctrl+Shift+J): move the \
current Q&A page to another band. \
-> journal::open_move_picker / confirm_move_picker \u{2014} src/input/actions/journal.rs",
        "view gloss" => "On a journal passage page (Ctrl+g / Ctrl+j): open the \
gloss for the cited passage. \
-> journal::view_gloss_from_journal \
\u{2014} src/input/actions/journal.rs",
        "BCP echo turns" => "Pick a speaker turn with cached BCP echoes and reopen \
them. -> echoes::open_echo_turns_picker(Bcp) \
— src/input/actions/echoes.rs (confirm: echoes::confirm_echo_turns_pick)",
        "BCP echoes" => "Show cached BCP echoes for the current speaker turn. \
-> echoes::show_echoes_for_cursor_line(Bcp) — src/input/actions/echoes.rs",
        "reopen BCP echoes" => "Reopen the last BCP echo results. \
-> echoes::reopen_echoes(Bcp) — src/input/actions/echoes.rs",
        "Shx echo turns" => "Pick a speaker turn with cached Shakespeare echoes \
and reopen them. \
-> echoes::open_echo_turns_picker(Shakespeare) — src/input/actions/echoes.rs",
        "Shx echoes" => "Run a cross-work echo search on the current speaker turn. \
-> echoes::show_echoes_for_cursor_line(Shakespeare) — src/input/actions/echoes.rs",
        "reopen Shx echoes" => "Reopen the last Shakespeare echo results. \
-> echoes::reopen_echoes(Shakespeare) — src/input/actions/echoes.rs",
        "voice: add/remove" => "In the gloss overlay: add or remove a voice for \
this gloss. -> open_voice_picker(GlossOverlay) — src/input/actions/settings.rs",
        "voice: cycle" => "In the gloss overlay: cycle the gloss's active voice. \
-> cycle_active_voice — src/input/actions/gloss.rs",
        "verse audio: play/stop" => "In the gloss overlay: play/stop the \
synthesized reading of the source verse (pauses MPV first). \
-> toggle_source_tts — src/input/actions/gloss.rs",
        "verse audio: pick voice" => "In the gloss overlay: pick the voice for \
the synthesized verse reading and play it. \
-> pick_source_voice — src/input/actions/gloss.rs",

        // ── Vocab ──
        "next conc" => "Next concordance hit for the active word (cross-work). \
-> concordance::concordance_next — src/input/actions/concordance.rs",
        "prev conc" => "Previous concordance hit for the active word (cross-work). \
-> concordance::concordance_prev — src/input/actions/concordance.rs",
        "next vocab" => "Jump to the next vocabulary word. \
-> concordance::jump_to_next_vocab — src/input/actions/concordance.rs",
        "prev vocab" => "Jump to the previous vocabulary word. \
-> concordance::jump_to_prev_vocab — src/input/actions/concordance.rs",
        "vocab hi" => "Toggle vocabulary-word highlighting (saved per work). \
-> ToggleVocabHighlight arm -> app::apply_vocab_highlighting / \
app::remove_vocab_highlighting — src/input/keymap.rs, src/app.rs",
        "auto vocab" => "Toggle the auto vocabulary popup. \
-> ToggleVocabPopup arm -> app::open_vocab_popup / close_vocab_popup — \
src/input/keymap.rs, src/app.rs",
        "vocab ▶" => "Next word in the vocabulary popup. \
-> handle_vocab_popup_key(.., true) — src/input/keymap.rs",
        "◀ vocab" => "Previous word in the vocabulary popup. \
-> handle_vocab_popup_key(.., false) — src/input/keymap.rs",

        // ── Word copy / visual ──
        "copy word" => "Copy the word under the cursor; repeated presses cycle \
adjacent words. \
-> word_copy::word_cycle_copy — src/input/actions/word_copy.rs",
        "copy id" => "Copy the current line's line-mapping id (and media id) to \
the clipboard. \
-> CopyLineMappingId arm (inline) — src/input/keymap.rs",
        "collect" => "Collect the word under the cursor into the vocabulary list \
and copy it. -> word_copy::word_collect_copy — src/input/actions/word_copy.rs",
        "visual mode" => "Enter visual selection mode: y yanks, i shows echoes, \
Return opens the action popup, Esc/V exits. \
-> visual::enter_visual_mode — src/input/visual.rs; \
handle_block_visual_key / gloss_overlay::enter_visual \
— src/input/keymap.rs, src/ui/gloss_overlay.rs",

        // ── MPV / audio ──
        "play/pause" => "Play/pause without seeking (unlike Space; a and Tab \
both bind this). -> TogglePause arm -> MpvCommand::TogglePause — src/input/keymap.rs",
        "vim copy" => "Open the cursor's paragraph/line in a copy-only vim \
editor, seeded in visual mode: extend with motions, y copies the selection to \
the system clipboard, :q or double-Esc exits. Nothing is saved. \
-> segment_vim::open — src/input/actions/segment_vim.rs",
        "toggle speed" => "Toggle playback speed between 1.0x and 1.3x. \
-> TogglePlaybackSpeed arm (inline) -> MpvCommand::SetSpeed — \
src/input/keymap.rs",
        "seek −3.5" => "Seek MPV back 3.5 seconds. \
-> do_mpv_seek(state, -3.5) — src/input/keymap.rs",
        "seek +3.5" => "Seek MPV forward 3.5 seconds. \
-> do_mpv_seek(state, 3.5) — src/input/keymap.rs",
        "−60" => "Seek MPV back 60 seconds (Shift+o). \
-> do_mpv_seek(state, -60.0) — src/input/keymap.rs",
        "+60" => "Seek MPV forward 60 seconds (Shift+e). \
-> do_mpv_seek(state, 60.0) — src/input/keymap.rs",
        "volume +" => "Raise MPV volume by 5. \
-> VolumeUp arm -> MpvCommand::VolumeAdjust(5.0) — src/input/keymap.rs",
        "volume −" => "Lower MPV volume by 5. \
-> VolumeDown arm -> MpvCommand::VolumeAdjust(-5.0) — src/input/keymap.rs",
        "sync tog" => "Toggle playback sync (cursor and page follow the audio). \
-> TogglePlaybackSync arm (inline) — src/input/keymap.rs",

        // ── Timestamps ──
        "start time" | "set start time" => "Set the current line's start timestamp \
from MPV's position. -> timestamps::set_start_time — src/input/timestamps.rs",
        "set end time" => "Set the current line's end timestamp from MPV's \
position. \
-> timestamps::set_end_time — src/input/timestamps.rs",
        "set track mark" => "Set an audio track mark on the current line (ffmpeg \
chapter export only; distinct from the structural chapter 'c' toggles). \
-> timestamps::set_chapter — src/input/timestamps.rs",
        "toggle ch start" => "Prose only: toggle whether the cursor's paragraph \
begins a structural chapter (distinct from Ctrl+c's audio track mark). \
-> chapters::toggle_chapter_start — src/input/actions/chapters.rs",
        "show chapter" => "Toast the current act/scene or chapter. \
-> navigation::show_current_chapter — src/input/navigation.rs",
        "delete ts" => "Delete the current line's timestamp (undoable). \
-> timestamps::delete_timestamp — src/input/timestamps.rs",
        "undo ts" => "Undo the last timestamp edit. \
-> timestamps::undo_timestamp — src/input/timestamps.rs",
        "nudge −0.2" => "Nudge the current line's start timestamp 0.2s earlier. \
-> timestamps::nudge_start_backward — src/input/timestamps.rs",
        "+0.2" => "Nudge the current line's start timestamp 0.2s later (Shift+p). \
-> timestamps::nudge_start_forward — src/input/timestamps.rs",
        "play from ts" => "Seek to the current line's start timestamp and play \
(for pause/resume without a seek, use Tab or a). \
-> timestamps::play_current_line — src/input/timestamps.rs",
        "clear AB" => "Dismiss a toast, else clear the A–B range / exit sub-modes. \
-> escape::escape_reader_mode — src/input/actions/escape.rs",

        // ── Fonts ──
        "next font" => "Next font in the cycling list. \
-> app::cycle_font(.., true) — src/app.rs",
        "prev font" => "Previous font in the cycling list (Shift+f). \
-> app::cycle_font(.., false) — src/app.rs",
        "font +" => "Increase the font size. \
-> app::adjust_font_size(.., 1) — src/app.rs",
        "font −" => "Decrease the font size. \
-> app::adjust_font_size(.., -1) — src/app.rs",
        "reset font" => "Reset the font size to the default. \
-> app::reset_font_size — src/app.rs",
        "font info" => "Toast the current font name and size. \
-> app::show_font_info — src/app.rs",

        // ── Display toggles ──
        "toggle signs" => "Toggle the sign column (left gutter markers). \
-> app::toggle_sign_column — src/app.rs",
        "synopsis" => "Show the synopsis overlay for the current scene; its binds \
are on its Ctrl+/ legend. \
-> app::show_synopsis_overlay — src/app.rs; \
gloss::read_current_synopsis_block, gloss::synth_all_synopsis_blocks \
— src/input/actions/gloss.rs (Ctrl+h toggles \
the side panel via app::toggle_synopsis).",
        "synopsis side" => "Toggle the persistent synopsis side panel. \
-> app::toggle_synopsis — src/app.rs",
        "synopsis edit (vim)" => "In the synopsis overlay: e edits the synopsis \
in a modal vim editor; R opens the ask-Claude rewrite card. \
-> synopsis::begin_edit (R -> show_edit_prompt) — \
src/input/actions/synopsis.rs",
        "col layout" => "Toggle one-column / two-column (spread) layout. \
-> navigation::toggle_column_layout — src/input/navigation.rs",
        "authorship" => "Toggle authorship formatting (marks lines by attributed \
author). -> ToggleAuthorship arm -> \
app::apply_authorship_formatting — src/input/keymap.rs, src/app.rs",
        "attr set" => "Pick which attribution set to apply. \
-> PickAttributionSet arm — \
src/input/keymap.rs",
        "nav test" => "Toggle the in-app navigation test harness (dev only). \
-> ToggleNavTest arm — src/input/keymap.rs",
        "theme next" => "Cycle the reader theme forward (Alt+t). \
-> settings::cycle_theme — src/input/actions/settings.rs",
        "theme prev" => "Cycle the reader theme backward (Alt+Shift+T). \
-> settings::cycle_theme — src/input/actions/settings.rs",
        "scansion" => "Cycle the metrical scansion overlay: off -> stress-only -> \
full. -> input::keymap CycleScansion",
        "2-col translation" => "Open the two-column translation overlay (Alt+i; \
distinct from Ctrl+Alt+i's inline column). \
-> app::show_translation_overlay — src/app.rs",
        "inline translation" => "Toggle the inline translation column \
(Ctrl+Alt+i). \
-> app::toggle_translations — src/app.rs",
        "page image" => "Toggle between rendered text and the page-scan image \
(Ctrl+i). -> app::toggle_image_view — src/app.rs",
        "calibrate pages" => "Enter page-image calibration (Ctrl+Shift+I): Enter \
records the line that begins each page scan, Esc saves. \
-> app::enter_page_calibration — src/app.rs",
        "dim tog" => "Toggle dimming of lines outside the A–B range. \
-> ToggleDim arm (inline) — src/input/keymap.rs",
        "save+quit" => "Save the reading position, quit MPV, close the window. \
-> SaveAndQuit arm -> app::save_position — src/input/keymap.rs, \
src/app.rs",
        "debug log" => "Toggle debug logging. \
-> ToggleDebugLogging arm (inline) — src/input/keymap.rs, src/logging.rs",

        _ => return None,
    };
    Some(d)
}

/// Strip a leading shift-key prefix of the form `"<char>: "` from a shift-action
/// label so the variant matches its base description (e.g. `"O: −60"` -> `"−60"`,
/// `"R: prev vocab"` -> `"prev vocab"`). Returns the label unchanged if it has no
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

/// Expand an abbreviated action label into a full, spelled-out form for the
/// breakout panel (the keycaps keep the short labels, which must stay narrow).
/// Returns the label unchanged when there is no expansion. A leading shift
/// prefix (`"R: prev vocab"`) is preserved and the remainder expanded.
fn expand_action(label: &str) -> String {
    let prefix_len = {
        let mut chars = label.char_indices();
        match (chars.next(), chars.next(), chars.next()) {
            (Some((_, _)), Some((i1, ':')), Some((_, ' '))) => i1 + 2, // "X: "
            _ => 0,
        }
    };
    let (prefix, base) = label.split_at(prefix_len);
    let full = match base {
        "prev ch" => "previous chapter",
        "next ch" => "next chapter",
        "prev scene" => "previous scene",
        "next scene" => "next scene",
        "prev bkmk" => "previous bookmark",
        "next bkmk" => "next bookmark",
        "prev dlg" => "previous dialogue",
        "next dlg" => "next dialogue",
        "prev vocab" => "previous vocab word",
        "next vocab" => "next vocab word",
        "prev match" => "previous match",
        "next match" => "next match",
        "pg fwd" => "page forward",
        "pg back" => "page backward",
        "lib picker" => "library picker",
        "conc picker" => "concordance picker",
        "media picker" => "media picker",
        "gloss tog" => "toggle gloss overlay",
        "gloss pick" => "gloss picker",
        "last gloss" => "reopen last gloss",
        "BCP echo turns" => "BCP echo turns picker",
        "Shx echo turns" => "Shakespeare echo turns picker",
        "vocab hi" => "toggle vocab highlight",
        "auto vocab" => "toggle auto-vocab popup",
        "toggle signs" => "toggle sign column",
        "sync tog" => "toggle playback sync",
        "dim tog" => "toggle dim",
        "debug log" => "toggle debug log",
        "set track mark" => "set audio track mark",
        "toggle ch start" => "toggle structural chapter",
        "show chapter" => "show current chapter",
        "bookmarks" => "bookmark picker",
        "start time" => "set start time",
        "set end time" => "set end time",
        "play from ts" => "play from timestamp",
        "delete ts" => "delete timestamp",
        "copy id" => "copy line id",
        "save+quit" => "save and quit",
        "clear AB" => "clear A-B / exit mode",
        "font info" => "show font info",
        "font +" => "increase font size",
        "font −" => "decrease font size",
        "reset font" => "reset font size",
        "vocab ▶" => "next vocab word",
        "◀ vocab" => "previous vocab word",
        _ => return label.to_string(),
    };
    format!("{prefix}{full}")
}

/// Turn a modifier-combo string (`"C-g"`, `"M-f"`, `"S-C-g"`, `"C-,"`,
/// `"C-\u{2191}"`) into a readable key label like `"Ctrl+g"`, `"Alt+f"`,
/// `"Ctrl+Shift+g"`. The trailing token after the last `-` is the key itself;
/// the `_fallback` (the cap's unshifted glyph) is used only if the combo has no
/// key token.
fn combo_glyph(combo: &str, _fallback: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut key = combo;
    // Strip leading modifier tokens ("C-", "M-", "S-") in any order.
    loop {
        if let Some(rest) = key.strip_prefix("C-") {
            parts.push("Ctrl");
            key = rest;
        } else if let Some(rest) = key.strip_prefix("M-") {
            parts.push("Alt");
            key = rest;
        } else if let Some(rest) = key.strip_prefix("S-") {
            parts.push("Shift");
            key = rest;
        } else {
            break;
        }
    }
    if key.is_empty() {
        key = _fallback;
    }
    parts.push(key);
    parts.join("+")
}

/// Split a stored blurb into its prose and its code reference. Blurbs end with
/// a maintenance reference (`... -> handler — file.rs`) starting at the first
/// ` -> ` marker. Returns `(prose, Some(reference))`, or `(whole, None)` when
/// there is no reference.
fn split_blurb(text: &str) -> (&str, Option<&str>) {
    match text.find(" -> ") {
        Some(i) => {
            let prose = text[..i].trim_end();
            // Drop the leading "-> " marker from the reference; inner "->"
            // (e.g. "arm (inline) -> MpvCommand") are left intact.
            let reference = text[i + 1..].trim_start().trim_start_matches("-> ");
            (prose, Some(reference))
        }
        None => (text, None),
    }
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
    jump_mode: bool,
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
    let mode = if jump_mode { "JUMP" } else { "NAV" };
    let header = format!("Row {} of {}  —  {}  —  {}", row_idx + 1, ROW_COUNT + 1, title, mode);
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
    // Cap the panel to a readable measure (long descriptions wrap to a column
    // instead of stretching across the whole screen) and center it horizontally.
    // Sits well below the keycap strip so there is clear separation.
    let panel_y = strip_y + cap_h + 64.0;
    let panel_w = (widget_w - 2.0 * margin).min(1240.0);
    let panel_x = (widget_w - panel_w) / 2.0;
    let def = keys[sel];

    // Each binding row: (key glyph, action label, color, optional blurb, is_shift).
    // The left column is the actual key (the unshifted char, the shifted char,
    // or a Ctrl/Alt combo) rather than a "Shift"/"Ctrl" word — there is no
    // separate title row. The shifted key always gets a row when the key has a
    // distinct shifted glyph, even if no action is bound to it: the glyph shows
    // on its own line with a blank action and no blurb.
    let mut rows: Vec<(String, String, (f64, f64, f64), Option<&'static str>, bool)> = Vec::new();
    if !def.action.is_empty() {
        rows.push((def.unshifted.to_string(), def.action.to_string(), (0.157, 0.412, 0.514), describe(def.action), false)); // pine
    }
    if !def.shifted.is_empty() {
        // Show the shifted key whether or not it has an action; a bound action
        // gets its label + blurb, an unbound one shows the key alone. Label it
        // "Shift+<unshifted>" (e.g. "Shift+,") rather than the bare shifted
        // glyph so the chord is explicit.
        rows.push((format!("Shift+{}", def.unshifted), def.shift_action.to_string(), (0.565, 0.478, 0.663), describe(def.shift_action), true)); // iris
    }
    for &(combo, act) in def.modifiers {
        let col = if combo.starts_with("M-") && !combo.contains("C-") {
            (0.706, 0.388, 0.478) // rose (Alt)
        } else if combo.contains("S-") {
            (0.204, 0.506, 0.341) // green (Ctrl+Shift)
        } else {
            (0.557, 0.420, 0.208) // gold (Ctrl)
        };
        rows.push((combo_glyph(combo, def.unshifted), act.to_string(), col, describe(act), false));
    }
    if rows.is_empty() {
        rows.push((String::new(), "(unbound)".to_string(), (0.596, 0.576, 0.647), None, false));
    }

    // Layout constants for the detail panel. Fonts here are deliberately large
    // (the panel is the "breakout" the user reads), so the columns and line
    // height are widened to match.
    let pad: f64 = 40.0; // inner padding (left/right/top/bottom breathing room)
    let glyph_x = panel_x + pad; // key-glyph column
    let act_x = panel_x + pad + 170.0; // action label column
    // Wrapped blurb column. The gap after the action column must clear the
    // WIDEST expanded action label (see expand_action) so the two columns never
    // overlap — e.g. "H: toggle auto-vocab popup" is ~26 monospace chars at
    // desc_font, ~345px, which overran the old 270px action column.
    let desc_x = panel_x + pad + 540.0; // wrapped blurb column (gap after action)
    let desc_max_w = panel_x + panel_w - pad - desc_x; // free width for blurbs
    let row_pad: f64 = 12.0; // vertical breathing room per binding row
    let desc_line_h: f64 = 30.0; // line height inside a wrapped blurb
    let base_row_h: f64 = 36.0; // height of a row with no blurb
    let desc_font: f64 = 22.0; // blurb font size
    let shift_gap: f64 = desc_line_h; // one blank line above the shifted key

    // Pre-pass: wrap each blurb and record per-row height so the panel grows to
    // fit. Each blurb renders as wrapped prose, then a blank line, then the code
    // reference (indented) on its own wrapped line(s). Each wrapped line carries
    // an is_reference flag so the draw pass can indent the reference. Wrapping
    // must use the same font the blurb is drawn with.
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(desc_font);
    let wrapped: Vec<Vec<(String, bool)>> = rows
        .iter()
        .map(|(_, _, _, blurb, _)| match blurb {
            Some(text) => {
                // Show only the prose; the code reference (after " -> ") is kept
                // in source for maintenance but not displayed in the panel.
                let (prose, _reference) = split_blurb(text);
                wrap_to_width(cr, prose, desc_max_w).into_iter().map(|l| (l, false)).collect()
            }
            None => Vec::new(),
        })
        .collect();
    let row_heights: Vec<f64> = wrapped
        .iter()
        .zip(rows.iter())
        .map(|(lines, (_, _, _, _, is_shift))| {
            let blurb_h: f64 = if lines.is_empty() { 0.0 } else { lines.len() as f64 * desc_line_h };
            let gap = if *is_shift { shift_gap } else { 0.0 };
            base_row_h.max(blurb_h) + row_pad + gap
        })
        .collect();
    let rows_total_h: f64 = row_heights.iter().sum();
    // No title row anymore — the panel is top padding, the rows (whose first
    // baseline drops one body-line below the top padding), then bottom padding.
    let panel_h = pad + desc_font + rows_total_h + pad;

    // Panel background
    cr.set_source_rgb(0.965, 0.949, 0.925);
    rounded_rect(cr, panel_x, panel_y, panel_w, panel_h, 10.0);
    let _ = cr.fill();
    cr.set_source_rgb(0.886, 0.847, 0.784);
    rounded_rect(cr, panel_x + 0.5, panel_y + 0.5, panel_w - 1.0, panel_h - 1.0, 10.0);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Binding rows: <key glyph>  <action>  <description>, all at the body size.
    // First row's baseline sits one body-line below the top padding.
    let mut ry = panel_y + pad + desc_font;
    for (i, (glyph, act, col, _, is_shift)) in rows.iter().enumerate() {
        // Two blank lines above the shifted key.
        if *is_shift {
            ry += shift_gap;
        }
        let top = ry; // baseline of the key/action line
        // Key glyph (the physical key / combo that triggers this binding).
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        cr.set_font_size(desc_font);
        cr.set_source_rgb(0.149, 0.251, 0.478);
        let _ = cr.move_to(glyph_x, top);
        let _ = cr.show_text(glyph);
        // Action label.
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
        cr.set_source_rgb(col.0, col.1, col.2);
        cr.set_font_size(desc_font);
        let _ = cr.move_to(act_x, top);
        let _ = cr.show_text(&expand_action(act));

        // Wrapped blurb to the right; code-reference lines are indented and
        // drawn in a dimmer color so they read as belonging to this binding.
        let lines = &wrapped[i];
        if !lines.is_empty() {
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(desc_font);
            for (li, (line, is_ref)) in lines.iter().enumerate() {
                let x = desc_x;
                if *is_ref {
                    cr.set_source_rgb(0.553, 0.533, 0.612); // dimmer for references
                } else {
                    cr.set_source_rgb(0.376, 0.357, 0.439);
                }
                let _ = cr.move_to(x, top + li as f64 * desc_line_h);
                let _ = cr.show_text(line);
            }
        }

        ry += row_heights[i] - if *is_shift { shift_gap } else { 0.0 };
    }

    // ── Footer hint ──
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(14.0);
    cr.set_source_rgb(0.78, 0.76, 0.82);
    let foot = if jump_mode {
        "Esc close  \u{00b7}  Tab jump/nav  \u{00b7}  press a key to jump to its cap  \u{00b7}  \u{2190}/\u{2192} move  \u{00b7}  \u{2191}/\u{2193} rows"
    } else {
        "Esc close  \u{00b7}  Tab jump/nav  \u{00b7}  n/p or \u{2191}/\u{2193} rows  \u{00b7}  j/k or \u{2190}/\u{2192} move"
    };
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
    jump_mode: Rc<std::cell::Cell<bool>>,
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
        let jump_mode = Rc::new(std::cell::Cell::new(true));

        let row_draw = row_index.clone();
        let sel_draw = selected.clone();
        let jump_draw = jump_mode.clone();
        drawing_area.set_draw_func(move |_area, cr, w, h| {
            draw_row_screen(cr, row_draw.get(), sel_draw.get(), jump_draw.get(), w as f64, h as f64);
        });

        KeybindsOverlay { overlay, drawing_area, row_index, selected, jump_mode }
    }

    pub fn show(&self) {
        self.jump_mode.set(true);
        // Reopen on the previously viewed row (row_index/selected persist across
        // hide/show). Clamp the row in case ROW_COUNT changed.
        let row = self.row_index.get().min(ROW_COUNT - 1);
        self.row_index.set(row);
        let len = row_keys(row).len();
        if self.selected.get() >= len {
            self.selected.set(first_bound(&row_keys(row)));
        }
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
        self.jump_mode.set(true);
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

    /// Jump the highlight to the cap for `key_name`, switching rows if the cap
    /// is on another row. Returns true if a cap matched, false otherwise.
    pub fn jump_to_key(&self, key_name: &str) -> bool {
        match find_cap(key_name) {
            Some((row, idx)) => {
                self.row_index.set(row);
                self.selected.set(idx);
                self.drawing_area.queue_draw();
                true
            }
            None => false,
        }
    }

    /// Flip between jump mode and nav mode and redraw.
    pub fn toggle_mode(&self) {
        self.jump_mode.set(!self.jump_mode.get());
        self.drawing_area.queue_draw();
    }

    /// Whether the overlay is currently in jump mode (vs nav mode).
    pub fn is_jump_mode(&self) -> bool {
        self.jump_mode.get()
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.drawing_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_for_symbol_names() {
        assert_eq!(key_name_to_glyph("slash"), Some("/"));
        assert_eq!(key_name_to_glyph("comma"), Some(","));
        assert_eq!(key_name_to_glyph("period"), Some("."));
        assert_eq!(key_name_to_glyph("parenleft"), Some("("));
        assert_eq!(key_name_to_glyph("plus"), Some("+"));
        assert_eq!(key_name_to_glyph("backslash"), Some("\\"));
        assert_eq!(key_name_to_glyph("apostrophe"), Some("'"));
    }

    #[test]
    fn glyph_returns_none_for_letters() {
        // Letters are matched by identity in find_cap, not via this table.
        assert_eq!(key_name_to_glyph("h"), None);
        assert_eq!(key_name_to_glyph("g"), None);
    }

    #[test]
    fn find_cap_resolves_representative_keys() {
        // 'h' is on the home row (index 2).
        let (row, idx) = find_cap("h").expect("h has a cap");
        assert_eq!(row, 2);
        assert_eq!(row_keys(row)[idx].unshifted, "h");

        // '/' (slash) is on the upper row (index 1).
        let (row, idx) = find_cap("slash").expect("slash has a cap");
        assert_eq!(row, 1);
        assert_eq!(row_keys(row)[idx].unshifted, "/");

        // '+' (plus) is on the number row (index 0).
        let (row, idx) = find_cap("plus").expect("plus has a cap");
        assert_eq!(row, 0);
        assert_eq!(row_keys(row)[idx].unshifted, "+");
    }

    #[test]
    fn find_cap_none_for_unmapped() {
        assert_eq!(find_cap("F5"), None);
        assert_eq!(find_cap("Return"), None);
    }

    #[test]
    fn every_lettered_cap_is_findable() {
        // Every cap with a single-char ASCII-letter glyph must resolve to
        // itself via identity matching.
        for row in 0..ROW_COUNT {
            for def in row_keys(row) {
                let g = def.unshifted;
                if g.len() == 1 && g.chars().all(|c| c.is_ascii_alphabetic()) {
                    let (r, i) = find_cap(g).unwrap_or_else(|| panic!("no cap for {g}"));
                    assert_eq!(row_keys(r)[i].unshifted, g);
                }
            }
        }
    }

    #[test]
    fn bound_symbol_keys_resolve_by_gtk_name() {
        // Every symbol key that has a bound cap must be jump-reachable by the
        // GTK keyval name it is delivered under. Guards against a row cap whose
        // GTK name is missing from key_name_to_glyph (the semicolon class of bug).
        let cases = [
            ("slash", "/"),
            ("comma", ","),
            ("period", "."),
            ("parenleft", "("),
            ("ampersand", "&"),
            ("bracketleft", "["),
            ("braceleft", "{"),
            ("backslash", "\\"),
            ("minus", "-"),
            ("apostrophe", "'"),
            ("plus", "+"),
            ("semicolon", ";"),
        ];
        for (name, glyph) in cases {
            let (row, idx) = find_cap(name)
                .unwrap_or_else(|| panic!("no cap for GTK name {name} (glyph {glyph})"));
            assert_eq!(row_keys(row)[idx].unshifted, glyph,
                "find_cap({name}) landed on the wrong cap");
        }
    }
}
