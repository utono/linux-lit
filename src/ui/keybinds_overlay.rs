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
    key("[", "2", "prev ch", "2: prev ch", &[("M-[", "col layout")]),
    key("{", "3", "next ch", "3: next ch", &[]),
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
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("C-p", "lib picker"), ("S-C-p", "conc word"), ("C-M-p", "conc list")]),
    key("y", "Y", "pg back", "", &[("C-y", "copy id")]),
    key("f", "F", "next font", "F: prev font", &[("M-f", "font info")]),
    key("g", "G", "", "", &[("C-g", "gloss tog"), ("S-C-g", "last gloss"), ("M-g", "gloss pick"), ("M-g", "gloss from jrnl"), ("C-g", "view gloss")]),
    key("c", "C", "toggle ch start", "C: show chapter", &[("C-c", "set track mark")]),
    key("r", "R", "next conc", "R: prev conc", &[("C-r", "next vocab"), ("S-C-r", "prev vocab"), ("M-r", "conc works")]),
    key("l", "L", "toggle signs", "", &[("S-C-l", "save+quit"), ("l", "verse audio: play/stop"), ("L", "verse audio: pick voice")]),
    key("/", "?", "search", "?: search back", &[("C-/", "keybinds")]),
    ub("@", "^"),
    key("\\", "#", "gloss tog", "◀ vocab", &[("C-\\", "conc picker"), ("M-\\", "vocab hi")]),
];
const TAB_KEY: KeyDef = key("Tab", "", "play/pause", "", &[("C-Tab", "last overlay")]);

const HOME_ROW: &[KeyDef] = &[
    key("a", "A", "play from ts", "", &[("C-a", "authorship"), ("S-C-a", "attr set")]),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[("C-e", "BCP echoes"), ("S-C-e", "reopen BCP echoes"), ("M-e", "BCP echo turns"), ("e", "synopsis edit (vim)")]),
    key("u", "U", "start time", "U: undo ts", &[("M-u", "set end time")]),
    key("i", "I", "2-col translation", "", &[("M-i", "scansion"), ("C-M-i", "inline translation"), ("C-i", "page image"), ("S-C-i", "calibrate pages")]),
    key("d", "D", "", "", &[("C-d", "debug log"), ("M-d", "dim tog")]),
    key("h", "H", "synopsis", "H: auto vocab", &[("C-h", "synopsis side")]),
    key("t", "T", "", "", &[("S-C-t", "nav test")]),
    key("n", "N", "next match", "N: prev match", &[]),
    bare("s", "S", "sync tog"),
    key("-", "_", "prev work", "", &[("C--", "recent")]),
];
const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

const BOTTOM_ROW: &[KeyDef] = &[
    bare("'", "\"", "next bkmk"),
    key("q", "Q", "next speaker", "Q: next dlg", &[]),
    key("j", "J", "cursor \u{2193}", "J: next speaker", &[("C-j", "journal tog"), ("M-j", "jrnl Q&A picker"), ("r", "jrnl from gloss"), ("C-j", "view jrnl"), ("C-S-j", "move jrnl band")]),
    key("k", "K", "cursor \u{2191}", "K: prev speaker", &[]),
    bare("x", "X", "pg fwd"),
    bare("b", "B", ""),
    key("m", "M", "bookmark", "", &[("C-m", "media picker")]),
    key("w", "W", "copy word", "W: collect", &[("M-w", "Shx echoes"), ("C-w", "Shx echo turns"), ("S-C-w", "reopen Shx echoes")]),
    key("v", "V", "", "V: visual mode", &[("v", "voice: add/remove"), ("C-v", "voice: cycle")]),
    bare("z", "Z", "vocab ▶"),
];

const SHIFT_KEY: KeyDef = ub("Shift", "");

/// Row 5: modifiers, sequences, and arrows gathered into one screen.
const MOD_SEQ_ROW: &[KeyDef] = &[
    bare("Space", "", "play from ts"),
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
        "pg fwd" => "Turn one page forward in the e-reader pagination (the cursor \
follows to the new page top). Aliased on x. \
-> navigation::page_forward — src/input/navigation.rs",
        "pg back" => "Turn one page backward in the e-reader pagination. Aliased \
on y. (In the synopsis overlay's Shift+V visual mode, y instead yanks the \
selected paragraphs to the clipboard.) \
-> navigation::page_backward — src/input/navigation.rs",
        "page ↓" => "Page forward by one screen (Space). \
-> navigation::page_forward — src/input/navigation.rs",
        "page ↑" => "Page backward by one screen (Shift+Space). \
-> navigation::page_backward — src/input/navigation.rs",
        "cursor ↓" | "cursor down" => "Move the cursor down one dialogue line, \
turning the page when it reaches the bottom, and seek audio to that line's \
start timestamp. \
-> navigation::cursor_next_dialogue — src/input/navigation.rs",
        "cursor ↑" | "cursor up" => "Move the cursor up one dialogue line, \
turning the page when it reaches the top, and seek audio to that line's \
start timestamp. \
-> navigation::cursor_prev_line — src/input/navigation.rs",
        "prev dlg" => "Jump the cursor to the previous line of dialogue, skipping \
speaker names, stage directions, and blank lines. \
-> navigation::jump_to_prev_dialogue — src/input/navigation.rs",
        "next dlg" => "Jump the cursor to the next line of dialogue, skipping \
speaker names, stage directions, and blank lines. \
-> navigation::jump_to_next_dialogue — src/input/navigation.rs",
        "next speaker" => "Jump the cursor to the first dialogue line of the next \
speaker turn — the next time the speaker changes — and seek audio to it. \
-> navigation::jump_to_next_speaker — src/input/navigation.rs",
        "prev speaker" => "If mid-speech, jump the cursor to the first dialogue line \
of the CURRENT speaker's block; if already at the block top, step to the first \
line of the previous speaker turn. Seeks audio to the landing line. \
-> navigation::jump_to_prev_speaker — src/input/navigation.rs",
        "go to start" => "Jump to the very first line of the work (gg). \
-> navigation::jump_to_start — src/input/navigation.rs",
        "go to end" => "Jump to the very last line of the work (G). \
-> navigation::jump_to_end — src/input/navigation.rs",
        "pg back btm" => "Turn one page backward and land the cursor on the bottom \
line of the new page (Shift+Up). \
-> navigation::page_backward_bottom — src/input/navigation.rs",

        // ── Chapters / scenes ──
        "prev ch" => "Jump to the previous chapter (prose) or act (play) — a div1 \
boundary sourced from (div1,div2) divisions, independent of any loaded audio. In \
a play the cursor lands on the act's FIRST DIALOGUE line, never the entrance \
stage direction. -> navigation::jump_to_prev_chapter — src/input/navigation.rs",
        "next ch" => "Jump to the next chapter (prose) or act (play) — a div1 \
boundary sourced from (div1,div2) divisions, independent of any loaded audio. In \
a play the cursor lands on the act's FIRST DIALOGUE line, never the entrance \
stage direction. -> navigation::jump_to_next_chapter — src/input/navigation.rs",
        "prev scene" => "Jump to the first line of the CURRENT scene (plays) or \
chapter (prose); pressed again from that first line, jump to the previous \
scene/chapter. Toasts the landing act/scene or chapter. \
-> navigation::jump_to_prev_section — src/input/navigation.rs",
        "next scene" => "Jump to the first line of the next scene (plays) or \
chapter (prose). Toasts the landing act/scene or chapter. \
-> navigation::jump_to_next_section — src/input/navigation.rs",

        // ── Bookmarks ──
        "bookmark" => "Toggle a bookmark on the current line (writes/removes a \
bookmark row in lit.db and updates the sign column). \
-> bookmarks::toggle_bookmark — src/input/actions/bookmarks.rs",
        "prev bkmk" => "Jump the cursor to the previous bookmarked line in this \
work. -> navigation::prev_bookmark — src/input/navigation.rs",
        "next bkmk" => "Jump the cursor to the next bookmarked line in this work. \
-> navigation::next_bookmark — src/input/navigation.rs",
        "latest bookmark" => "Jump to the most recently added bookmark (g; \
sequence). -> bookmarks::jump_to_recent_bookmark — src/input/actions/bookmarks.rs",
        "bookmarks" => "Open the bookmark picker: a list of this work's bookmarks \
to jump to. -> pickers::open_bookmark_picker — src/input/actions/pickers.rs",

        // ── Pickers / overlays ──
        "lib picker" => "Open the library picker to switch works: browse by \
author, then by work, with fuzzy filtering. The chosen work opens at your \
saved position, or at the first dialogue line on first open. \
-> pickers::open_library_picker_from_reader — src/input/actions/pickers.rs \
(opens via app::display_work_at_with_prepared — src/app.rs)",
        "media picker" => "Open the media picker to choose which audio file syncs \
with this work; the MPV socket path is derived from the file path. \
-> pickers::open_media_picker — src/input/actions/pickers.rs",
        "conc picker" => "Open the concordance picker: a stopword-filtered list \
of words used across this author's works. Pick a word to start cross-work \
concordance navigation, then step through occurrences with r / R. \
-> concordance::open_picker — src/input/actions/concordance.rs",
        "conc word" => "Open the concordance WORD picker — choose the word whose \
cross-work occurrences r / R will step through. \
-> pickers::open_concordance_word_picker — src/input/actions/pickers.rs",
        "conc list" => "Open the concordance LIST picker (the list of occurrences \
for the active concordance word). \
-> pickers::open_concordance_list_picker — src/input/actions/pickers.rs",
        "conc works" => "Open the concordance WORKS picker — choose which work to \
jump into for the active concordance word. \
-> pickers::open_concordance_works_picker — src/input/actions/pickers.rs",
        "recent" => "Open the recent-works picker (most-recently-used works). \
-> pickers::open_recent_picker — src/input/actions/pickers.rs",
        "prev work" => "Toggle back to the previously open work (a quick A/B swap \
between the last two works, like vim's Ctrl-^). \
-> pickers::toggle_previous_work — src/input/actions/pickers.rs",
        "settings" => "Open the settings overlay (margins, offsets, and other \
reader options). -> settings::open_settings — src/input/actions/settings.rs",
        "keybinds" => "Open this keyboard-shortcut overlay. Also works inside the \
gloss and synopsis overlays (Ctrl+/), returning to that overlay when closed. \
-> pickers::open_keybinds_overlay / open_keybinds_from_mode — \
src/input/actions/pickers.rs (drawing: src/ui/keybinds_overlay.rs)",
        "search" => "Open the in-text search bar (forward: first match at or after \
the cursor); Escape restores the pre-search reader position. -> OpenSearch arm \
(inline) -> search::clear_search — src/input/keymap.rs, src/input/search.rs",
        "search back" => "Open the in-text search bar searching BACKWARD (?: last \
match at or before the cursor); otherwise identical to /. -> OpenSearchBackward \
arm (inline) — src/input/keymap.rs, src/input/search.rs",
        "next match" => "Move to the next search match (reactivates the last \
search pattern if search was cancelled; stops at the last match with a toast, \
no wrap) — or, when a concordance is active, the next concordance hit within \
this work. \
-> SearchNextMatch arm -> search::reactivate_and_step / concordance::concordance_next_in_work \
— src/input/keymap.rs",
        "prev match" => "Move to the previous search match (reactivates the last \
search pattern if search was cancelled; stops at the first match with a toast, \
no wrap) — or the previous concordance hit within this work when a concordance \
is active. \
-> SearchPrevMatch arm -> search::reactivate_and_step / concordance::concordance_prev_in_work \
— src/input/keymap.rs",

        // ── Gloss / echo system ──
        "gloss tog" => "Open the gloss overlay for the current passage (close with \
Escape or n). \
A gloss is a saved AI commentary on a highlighted passage — a \
teacher-style Q&A note, an inner-monologue cross-reference, or a terse \
reader-focused gloss. If a gloss is \
already loaded it reopens that one without re-querying the database. Inside the \
overlay, j/k (or q/comma) move the cursor block; Space (or Tab) reads the cursor \
block aloud; Ctrl+j goes to the passage's journal while Ctrl+g returns to the \
reader; Shift+Space \
batch-synthesizes every prose (explication) block to \
cached ElevenLabs MP3s (cache only, no playback), showing a \
\u{201c}Synthesizing\u{2026}\u{201d} toast. \
-> gloss::toggle_overlay — src/input/actions/gloss.rs; \
gloss::synth_all_prose_blocks — src/input/actions/gloss.rs",
        "gloss pick" => "Open a fuzzy-filterable list of every passage in this \
work that has a saved gloss (teacher-generic, inner-monologue, or reader-gloss). \
Alt+t cycles the type filter teacher-generic -> inner-monologue -> reader-gloss. \
Each row shows the speaker, the first source line, and the citation; confirming \
loads that passage's glosses into the overlay and jumps the reader to it. \
-> pickers::open_gloss_picker — src/input/actions/pickers.rs (confirm: \
handle_gloss_picker_key in src/input/keymap.rs)",
        "journal tog" => "Open or close the Q&A journal for the current scene. \
The journal is a per-work notebook: each scene holds zero or more \u{201c}pages,\u{201d} \
where a page is one question you asked and the answer Claude gave. It opens on the \
scene under the reading cursor; if that scene has no pages yet it opens the author \
corpus band instead (when the author has corpus notes), else shows an empty \
card prompting you to press r to ask. Inside the overlay: r asks a new question \
(Claude answers, drawing on its knowledge of the whole work, in the work\u{2019}s own \
genre \u{2014} novel/chapter, play/scene, epic/book, etc.), e opens an edit \
card (Question + Answer pre-filled, plus an optional rewrite-instruction field): \
Ctrl+Enter submits \u{2014} with an instruction it sends the Q&A + instruction to \
Claude and saves the revised answer, with the field empty it saves your hand-edits \
straight to lit.db (no Claude); u undoes the last edit after a y/Esc confirm. \
D deletes the current page, j/k (or q/comma, \
matching the reading card) move the paragraph cursor (the left accent bar) between \
blocks and gg/G jump to the first/last block; Space \
(or Tab) plays/stops the cursor paragraph\u{2019}s TTS (synthesizing a plain-prose ElevenLabs \
MP3 on a cache miss, cached per page) and `a` restarts it from the beginning, \
Ctrl+n / Ctrl+p step through every Q&A in the work across bands (at a band\u{2019}s \
last page Ctrl+n rolls into the next chapter/scene\u{2019}s first Q&A, in the same \
order as the Ctrl+\\ picker), Alt+n / Alt+p jump to the \
next/prev scene that has pages, Alt+w switches to the Work band \u{2014} \
whole-work pages about the work as a whole (Claude is sent only the title and \
author, not a scene) \u{2014} and Ctrl+\\ opens a picker of every Q&A page in the \
work (whole-work pages first, then scene pages in scene order) to jump straight \
to one. Escape (or Ctrl+j) closes and returns the cursor to where you were \
reading; Ctrl+g on a passage page cross-jumps to that passage's gloss. \
-> journal::toggle_overlay — src/input/actions/journal.rs (overlay keys: \
handle_journal_key in src/input/keymap.rs)",
        "last overlay" => "Flip between the reading card and whichever of the \
gloss / journal overlays you last had open. From the reader it reopens the last \
one (the gloss for the cursor line, or the journal on the cursor\u{2019}s scene) \u{2014} \
fresh from the cursor, exactly like pressing Ctrl+g / Ctrl+j; from inside that \
overlay it closes it back to the reader. Which overlay is remembered is recorded \
whenever either closes (by toggle, Escape, or undo-return), so opening with \
Ctrl+j and closing with Escape still lets Ctrl+Tab reopen the journal. With \
nothing opened yet this session it toasts \u{201c}No overlay to reopen.\u{201d} \
-> gloss::toggle_last_overlay — src/input/actions/gloss.rs",
        "jrnl Q&A picker" => "Open the Q&A picker directly from the reading card \
(Alt+j), without first opening the journal overlay. Lists every Q&A page in the \
work \u{2014} whole-work pages first, then scene pages in scene order \u{2014} the \
same picker the journal overlay\u{2019}s Ctrl+\\ opens. Choosing a page reveals the \
journal overlay landed on that Q&A; Escape returns you to where you were reading. \
Toasts \u{201c}No journal pages yet\u{201d} when the work has none. \
-> journal::open_picker_from_reader — src/input/actions/journal.rs",
        "last gloss" => "Reopen the gloss overlay on the most recently viewed \
gloss in this work, restored to the gloss type that was on screen. The reference \
is remembered per work and persists across restarts; it is updated whenever a \
gloss is viewed or freshly created. Toasts \u{201c}No recent gloss\u{201d} when \
there is none recorded, or the remembered passage was deleted or no longer \
resolves. \
-> gloss::open_last_gloss — src/input/actions/gloss.rs",
        // ── Gloss ↔ journal cross-view (Task 7) ──
        "jrnl from gloss" => "While the gloss overlay is open (r): create a \
journal Q&A page for the gloss\u{2019}s current source passage. Closes the gloss overlay, \
opens the journal overlay in the Passage band, and presents the ask card so you can \
pose a question. Uses the same passage markup (speaker/verse/stage) the journal \
passage renderer displays. \
-> journal::begin_passage_ask — src/input/actions/journal.rs \
(handler: handle_gloss_key \u{2018}r\u{2019} arm \u{2014} src/input/keymap.rs)",
        "gloss from jrnl" => "While the journal overlay is open on a passage page (Alt+g): \
create (or show cached) a reader-gloss for the passage cited by the current journal page. \
If the passage already has a reader-gloss it opens instantly; otherwise calls Claude and \
saves. Toasts \u{201c}Not a passage page\u{201d} when the current band is Work or Scene. \
-> journal::action_gloss_from_journal_passage \
\u{2014} src/input/actions/journal.rs",
        "view jrnl" => "While the gloss overlay is open (Ctrl+j only \u{2014} Ctrl+g now returns to \
the reader): view the journal passage pages for the gloss\u{2019}s current source passage. Looks \
up journal entries whose start/end citations match the open gloss; if found, closes the gloss \
overlay and opens the journal overlay in the Passage band on the first page. When the passage \
has no journal page, falls back to the author corpus band (if the author has corpus notes); \
toasts \u{201c}No journal page for this passage\u{201d} only when neither exists. \
-> journal::view_journal_from_gloss \
\u{2014} src/input/actions/journal.rs",
        "move jrnl band" => "While the journal overlay is open (Ctrl+Shift+J): move the \
current Q&A page to a different band. Opens a picker listing every scene/chapter in \
the work plus a \u{201c}whole work\u{201d} row (the current band omitted); Enter re-targets \
the entry\u{2019}s scope + (div1,div2) in lit.db and follows it to its new band. Passage \
pages can\u{2019}t be moved. \
-> journal::open_move_picker / confirm_move_picker \u{2014} src/input/actions/journal.rs",
        "view gloss" => "While the journal overlay is open on a passage page (Ctrl+g or \
Ctrl+j): view the gloss for the passage cited by the current journal page. Looks up \
glosses by the page\u{2019}s start_citation; if found, closes the journal overlay and opens \
the gloss overlay on that passage (reader-gloss preferred). Toasts \u{201c}Not a passage \
page\u{201d} or \u{201c}No gloss for this passage\u{201d} as appropriate. \
-> journal::view_gloss_from_journal \
\u{2014} src/input/actions/journal.rs",
        "BCP echo turns" => "List every speaker turn in this work that has cached \
Book of Common Prayer inner-monologue echoes (echo_work_abbrev LIKE 'BCP%'). \
Selecting a turn jumps the cursor to its first line and reopens its stored BCP \
echoes instantly. BCP echoes are precomputed (ws-book-of-common-prayer-references) \
— this channel never runs a live search. -> echoes::open_echo_turns_picker(Bcp) \
— src/input/actions/echoes.rs (confirm: echoes::confirm_echo_turns_pick)",
        "BCP echoes" => "Show cached Book of Common Prayer inner-monologue echoes \
for the current speaker turn — liturgical passages that resonate with the speech, \
to animate performance. Cache-only: no live Voyage search. \
-> echoes::show_echoes_for_cursor_line(Bcp) — src/input/actions/echoes.rs",
        "reopen BCP echoes" => "Reopen the BCP echoes overlay with the most recent \
results, without running a new search. \
-> echoes::reopen_echoes(Bcp) — src/input/actions/echoes.rs",
        "Shx echo turns" => "List every speaker turn in this work that has cached \
Shakespeare-to-Shakespeare cross-work echoes (echo_work_abbrev NOT LIKE 'BCP%') — \
thematically similar passages found by a prior echo search. Selecting a turn \
jumps the cursor to its first line and reopens its stored echoes instantly. \
-> echoes::open_echo_turns_picker(Shakespeare) — src/input/actions/echoes.rs",
        "Shx echoes" => "Run a cross-work echo search on the current speaker turn: \
embed the turn, find thematically similar passages elsewhere in the author's \
works, and show them in the echoes overlay. \
-> echoes::show_echoes_for_cursor_line(Shakespeare) — src/input/actions/echoes.rs",
        "reopen Shx echoes" => "Reopen the Shakespeare echoes overlay with the most \
recent echo results, without running a new search. \
-> echoes::reopen_echoes(Shakespeare) — src/input/actions/echoes.rs",
        "voice: add/remove" => "While the gloss overlay is open, open the voice \
picker to add or remove a voice associated with this gloss (toggles voice-set \
membership). -> open_voice_picker(GlossOverlay) — src/input/actions/settings.rs",
        "voice: cycle" => "While the gloss overlay is open, cycle which associated \
voice plays for this gloss. \
-> cycle_active_voice — src/input/actions/gloss.rs",
        "verse audio: play/stop" => "While the gloss overlay is open, on a source \
verse: play or stop the synthesized (ElevenLabs) reading in the gloss's active \
voice — the AI voice, separate from the recorded media that space/a play. \
Starting it pauses the MPV recording so they don't overlap; press again to stop \
(recording stays paused, resume with space). Cache miss synthesizes on first \
play. -> toggle_source_tts — src/input/actions/gloss.rs",
        "verse audio: pick voice" => "While the gloss overlay is open, on a source \
verse: open the voice picker for the synthesized reading; confirming sets that \
voice as the gloss's active voice and plays the verse (pausing MPV first). If \
the synthesized audio is already playing, this stops it instead. R is the only \
key that opens the picker; r replays in the current active voice. \
-> pick_source_voice — src/input/actions/gloss.rs",

        // ── Vocab ──
        "next conc" => "Step to the next concordance hit for the active word — \
cross-work, loading the new work in place and seeking MPV to the hit line. \
Shows a \"no concordance active\" toast if no word is selected. \
-> concordance::concordance_next — src/input/actions/concordance.rs",
        "prev conc" => "Step to the previous concordance hit for the active word \
(cross-work, loads the work in place). \
-> concordance::concordance_prev — src/input/actions/concordance.rs",
        "next vocab" => "Jump the cursor to the next vocabulary word in the work \
(words you have collected for study), independent of any active concordance. \
-> concordance::jump_to_next_vocab — src/input/actions/concordance.rs",
        "prev vocab" => "Jump the cursor to the previous vocabulary word in the \
work. -> concordance::jump_to_prev_vocab — src/input/actions/concordance.rs",
        "vocab hi" => "Toggle highlighting of vocabulary words in the text (state \
saved per-work in lit.db). -> ToggleVocabHighlight arm -> app::apply_vocab_highlighting / \
app::remove_vocab_highlighting — src/input/keymap.rs, src/app.rs",
        "auto vocab" => "Toggle the auto vocabulary popup, which shows definitions \
for the current line's vocab words as the cursor moves. \
-> ToggleVocabPopup arm -> app::open_vocab_popup / close_vocab_popup — \
src/input/keymap.rs, src/app.rs",
        "vocab ▶" => "Step forward through the vocabulary popup's words for the \
current line. -> handle_vocab_popup_key(.., true) — src/input/keymap.rs",
        "◀ vocab" => "Step backward through the vocabulary popup's words for the \
current line. -> handle_vocab_popup_key(.., false) — src/input/keymap.rs",

        // ── Word copy / visual ──
        "copy word" => "Copy the word under the cursor to the clipboard; repeated \
presses cycle outward through adjacent words. \
-> word_copy::word_cycle_copy — src/input/actions/word_copy.rs",
        "copy id" => "Copy the current line's line-mapping id (and media id, when \
present) to the clipboard via wl-copy — useful for debugging and lit.db edits. \
-> CopyLineMappingId arm (inline) — src/input/keymap.rs",
        "collect" => "Collect the word under the cursor into the vocabulary list \
and copy it. -> word_copy::word_collect_copy — src/input/actions/word_copy.rs",
        "visual mode" => "Enter visual selection mode (vim-style); then y yanks, \
i shows echoes for the selection, Return opens the action popup, Esc/V exits. \
The synopsis overlay (h) has its own Shift+V visual mode: j/k (and gg/G) extend \
a paragraph-block selection, y yanks the selected paragraphs to the clipboard \
(blank-line joined) and exits, Esc/Shift+V cancels. The gloss overlay (Ctrl+g) \
likewise has Shift+V visual mode: j/k (and gg/G) extend a block selection, y \
yanks the selected blocks' full text (source verse + gloss, as displayed) and \
exits, Esc/Shift+V cancels; there Ctrl+V cycles the active voice. \
-> visual::enter_visual_mode — src/input/visual.rs; \
handle_block_visual_key / gloss_overlay::enter_visual \
— src/input/keymap.rs, src/ui/gloss_overlay.rs",

        // ── MPV / audio ──
        "play/pause" => "Pure play/pause toggle of the synced audio. Does NOT \
seek — playback resumes wherever it was paused (unlike `a`/Space, which seek to \
the cursor line's start timestamp). \
-> TogglePause arm -> MpvCommand::TogglePause — src/input/keymap.rs",
        "toggle speed" => "Toggle MPV playback speed between 1.0x and 1.3x (shows \
a toast). -> TogglePlaybackSpeed arm (inline) -> MpvCommand::SetSpeed — \
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
        "sync tog" => "Toggle playback sync: when on, the cursor and page follow \
MPV's audio position automatically; a toast shows the new state. \
-> TogglePlaybackSync arm (inline) — src/input/keymap.rs",

        // ── Timestamps ──
        "start time" | "set start time" => "Set the audio start timestamp for the \
current line from MPV's current playback position and write it to lit.db \
(updates the sign column). -> timestamps::set_start_time — src/input/timestamps.rs",
        "set end time" => "Set the audio end timestamp for the current line from \
MPV's current playback position. \
-> timestamps::set_end_time — src/input/timestamps.rs",
        "set track mark" => "Set an audio track mark on the current line at MPV's \
current playback position (export metadata for ffmpeg chapter embedding), written \
to line_timestamps.is_track_mark in lit.db. Export-only — it does NOT affect \
chapter nav or the gutter chapter sign, which follow structural divisions. \
Distinct from the structural chapter that plain 'c' toggles. \
-> timestamps::set_chapter — src/input/timestamps.rs",
        "toggle ch start" => "Prose only: toggle whether the cursor's paragraph \
begins a structural chapter — a (div1,div2) division boundary. Flips \
line_mapping.chapter_start in lit.db, re-derives the work's chapter divisions, and \
reloads in place with the cursor preserved. In prose this is what '(' / '&' jump \
between and what the ▸ gutter chapter sign marks; plays carry no chapter marks (▸ \
never shows) and '(' / '&' jump acts from (div1) metadata instead. Distinct from \
the audio track mark that Ctrl+c sets. \
-> chapters::toggle_chapter_start — src/input/actions/chapters.rs",
        "show chapter" => "Show the current act/scene (plays) or chapter (prose) \
for the line the cursor is on, as a transient toast. \
-> navigation::show_current_chapter — src/input/navigation.rs",
        "delete ts" => "Delete the current line's saved timestamp from lit.db \
(undoable). -> timestamps::delete_timestamp — src/input/timestamps.rs",
        "undo ts" => "Undo the last timestamp edit (the most recent u start-time \
set, Ctrl+c track mark, or timestamp delete), restoring the previous value in \
lit.db. -> timestamps::undo_timestamp — src/input/timestamps.rs",
        "nudge −0.2" => "Nudge the current line's start timestamp 0.2s earlier. \
-> timestamps::nudge_start_backward — src/input/timestamps.rs",
        "+0.2" => "Nudge the current line's start timestamp 0.2s later (Shift+p). \
-> timestamps::nudge_start_forward — src/input/timestamps.rs",
        "play from ts" => "Seek MPV to the current line's saved start timestamp \
and play from there (`a` and Space). For a pure pause/resume with no seek, use \
Tab (play/pause). -> timestamps::play_current_line — src/input/timestamps.rs",
        "clear AB" => "Dismiss any visible toast, else clear the A–B repeat range \
/ exit reader sub-modes (Esc). \
-> escape::escape_reader_mode — src/input/actions/escape.rs",

        // ── Fonts ──
        "next font" => "Cycle to the next font in the font-cycling list. \
-> app::cycle_font(.., true) — src/app.rs",
        "prev font" => "Cycle to the previous font in the font-cycling list (Shift+f). \
-> app::cycle_font(.., false) — src/app.rs",
        "font +" => "Increase the reader font size by one step (saved to config). \
-> app::adjust_font_size(.., 1) — src/app.rs",
        "font −" => "Decrease the reader font size by one step (saved to config). \
-> app::adjust_font_size(.., -1) — src/app.rs",
        "reset font" => "Reset the reader font size to the default. \
-> app::reset_font_size — src/app.rs",
        "font info" => "Show a toast with the current font name and size. \
-> app::show_font_info — src/app.rs",

        // ── Display toggles ──
        "toggle signs" => "Toggle the sign column — the left gutter dots/markers \
that flag timestamps, chapters, bookmarks, and A/B points. \
-> app::toggle_sign_column — src/app.rs",
        "synopsis" => "Show the synopsis overlay for the current scene. Inside \
it, j/k (or q/comma) move the cursor block (left accent bar) and gg/G jump \
first/last, like the gloss overlay; Space (or Tab) plays/stops the cursor paragraph's TTS \
(synthesizing on a cache miss); Shift+Space batch-synthesizes every synopsis paragraph to \
cached ElevenLabs MP3s (cache only, no playback), showing a \u{201c}Synthesizing\u{2026}\u{201d} \
toast. Ctrl+p / Ctrl+n step to the previous / next scene's synopsis, clamping at \
the ends (no wraparound); a whole-work synopsis sorts first (before Act 1), so \
from Act 1 Scene 1, Ctrl+p shows the whole-work overview. e edits the \
current synopsis in place (no new row); u undoes the last edit after a y/Esc \
confirm; r opens a journal Q&A for the displayed scene/chapter. \
-> app::show_synopsis_overlay — src/app.rs; \
gloss::read_current_synopsis_block, gloss::synth_all_synopsis_blocks \
— src/input/actions/gloss.rs (Ctrl+h toggles \
the side panel via app::toggle_synopsis).",
        "synopsis side" => "Toggle the persistent synopsis side panel (distinct \
from h's transient synopsis overlay). -> app::toggle_synopsis — src/app.rs",
        "synopsis edit (vim)" => "While the synopsis overlay is open (h), press e to \
edit the scene synopsis IN PLACE in a modal vim editor (raw text, monospace font): \
motions/edits, :w/:wq save, :q/:q! quit, double-Esc exits. R opens the ask-Claude \
rewrite card instead. -> synopsis::begin_edit (R -> show_edit_prompt) — \
src/input/actions/synopsis.rs",
        "col layout" => "Toggle between one-column and two-column (spread) page \
layout. -> navigation::toggle_column_layout — src/input/navigation.rs",
        "authorship" => "Toggle authorship formatting on/off — visually marks each \
line by its attributed author (for collaboratively-written works). Toasts \"No \
authorship data\" when the work has none. -> ToggleAuthorship arm -> \
app::apply_authorship_formatting — src/input/keymap.rs, src/app.rs",
        "attr set" => "Pick which attribution set to apply (when a work has more \
than one scholarly authorship attribution). -> PickAttributionSet arm — \
src/input/keymap.rs",
        "nav test" => "Toggle the in-app navigation test harness (Ctrl+Shift+T) — \
drives nav actions and asserts on-page landing; for development only. \
-> ToggleNavTest arm — src/input/keymap.rs",
        "scansion" => "Cycle the Wright metrical scansion overlay on verse lines: \
off -> stress-only -> full (stress + unstressed marks), with line-type label and \
caesura. -> input::keymap CycleScansion",
        "2-col translation" => "Open the scrolling two-column translation overlay \
(Alt+i) for the current scene (source text beside its translation), with its own \
nav, playback, and sync binds. Distinct from Ctrl+Alt+i's inline translation \
column. -> app::show_translation_overlay — src/app.rs",
        "inline translation" => "Toggle the parallel inline translation column \
(Ctrl+Alt+i) alongside the text (pauses MPV first). \
-> app::toggle_translations — src/app.rs",
        "page image" => "Toggle the main card between rendered text and the \
page-scan image (Ctrl+i) for the cursor's current page (BCP1549 etc.). The shown \
leaf follows the cursor, so it tracks media playback. No-op for works without \
page images. -> app::toggle_image_view — src/app.rs",
        "calibrate pages" => "Enter page-image calibration (Ctrl+Shift+I): the \
card shows each page scan with a caption; move the cursor (j/k) to the line that \
begins the page and press Enter to record it and advance. n/p step pages, gg/G \
jump to the first/last page, Esc saves. Fills page_images start/end ranges so the \
leaf tracks the cursor. -> app::enter_page_calibration — src/app.rs",
        "dim tog" => "Toggle dimming of lines outside the current A–B sync range \
and refresh the highlight. -> ToggleDim arm (inline) — src/input/keymap.rs",
        "save+quit" => "Save the current reading position, tell MPV to quit, and \
close the window. -> SaveAndQuit arm -> app::save_position — src/input/keymap.rs, \
src/app.rs",
        "debug log" => "Toggle debug logging on/off (briefly shows a gear/blocked \
icon). The log file is linux-lit-dev.log / linux-lit-release.log. \
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
    let row_pad: f64 = 24.0; // vertical breathing room per binding row
    let desc_line_h: f64 = 30.0; // line height inside a wrapped blurb
    let base_row_h: f64 = 36.0; // height of a row with no blurb
    let desc_font: f64 = 22.0; // blurb font size
    let shift_gap: f64 = 2.0 * desc_line_h; // two blank lines above the shifted key

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
