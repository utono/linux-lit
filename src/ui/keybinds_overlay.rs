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
    key("+", "1", "show chapter", "1: copy work info", &[]),
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
const BACKSPACE: KeyDef = bare("\u{232b}", "", "ts tap");

const UPPER_ROW: &[KeyDef] = &[
    key(";", ":", "prev bkmk", ":: cycle speed", &[]),
    key(",", "<", "prev speaker", "<: prev dlg", &[("C-,", "settings")]),
    key(".", ">", "bkmk tap", "", &[("C-.", "bookmarks")]),
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("M-p", "phrase hl"), ("C-p", "Q&A page \u{25b2}")]),
    key("y", "Y", "pg back", "", &[("C-y", "copy id")]),
    key("f", "F", "next font", "F: prev font", &[("M-f", "font info")]),
    key("g", "G", "", "G: go to end", &[("C-g", "gloss tog"), ("S-C-g", "last gloss"), ("M-g", "gloss pick")]),
    key("c", "C", "toggle ch start", "C: show chapter", &[("C-c", "set track mark")]),
    key("r", "R", "vocab tap", "", &[("C-r", "vocab Q&A")]),
    key("l", "L", "toggle signs", "", &[("C-l", "chat side"), ("S-C-l", "save+quit")]),
    key("/", "?", "search", "?: search back", &[("C-/", "keybinds")]),
    ub("@", "^"),
    key("\\", "#", "cycle overlays", "", &[("C-\\", "lib picker"), ("M-\\", "vocab hi")]),
];
const TAB_KEY: KeyDef = key("Tab", "", "chat layout", "", &[("C-Tab", "last overlay")]);

const HOME_ROW: &[KeyDef] = &[
    key("a", "A", "play/pause", "A: authorship", &[("C-a", "ask passage"), ("S-C-a", "attr set")]),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[("C-e", "BCP echoes"), ("S-C-e", "reopen BCP echoes"), ("M-e", "BCP echo turns")]),
    key("u", "U", "2-col translation", "U: undo ts", &[("M-u", "scansion")]),
    key("i", "I", "start time", "", &[("M-i", "set end time"), ("C-M-i", "inline translation"), ("C-i", "page image"), ("S-C-i", "calibrate pages")]),
    key("d", "D", "", "", &[("C-d", "debug log"), ("M-d", "dim tog")]),
    key("h", "H", "synopsis", "H: auto vocab", &[]),
    key("t", "T", "", "", &[("C-t", "root variant"), ("S-C-T", "root variant prev"), ("C-M-t", "nav test"), ("M-t", "theme next"), ("M-S-T", "theme prev")]),
    key("n", "N", "next match", "N: prev match", &[("C-n", "Q&A page \u{25bc}")]),
    bare("s", "S", "sync tap"),
    key("-", "_", "", "", &[("C--", "vocab drill"), ("S-C--", "drill back")]),
];
const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

const BOTTOM_ROW: &[KeyDef] = &[
    bare("'", "\"", "next bkmk"),
    key("q", "Q", "next speaker", "Q: next dlg", &[]),
    key("j", "J", "cursor \u{2193}", "J: next speaker", &[("C-j", "journal tog"), ("M-j", "jrnl Q&A picker")]),
    key("k", "K", "cursor \u{2191}", "K: prev speaker", &[]),
    bare("x", "X", "pg fwd"),
    bare("b", "B", ""),
    key("m", "M", "bookmark", "", &[("C-m", "media picker")]),
    key("w", "W", "copy word", "W: collect", &[("M-w", "Shx echoes"), ("C-w", "Shx echo turns"), ("S-C-w", "reopen Shx echoes")]),
    key("v", "V", "vim copy", "V: visual mode", &[]),
    key("z", "Z", "conc picker", "", &[("C-z", "conc word"), ("M-z", "conc works"), ("S-C-z", "conc list")]),
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

/// The Rust-idiom description for a binding, keyed by its short action label
/// (the same strings used in the row definitions above). This is the ONLY
/// text the two-column detail panel shows next to the key glyph. The format
/// is minimal: `Action::<Variant> — src/<handler file>` (or the pre-dispatch
/// mechanism, e.g. `gg chord (Action::PendingG)`, for keys with no Action).
/// Add an `-> InputMode::X` or a short parenthetical ONLY when the variant
/// name alone would mislead (e.g. the modal vocab drill). Never document a
/// sibling chord that has its own row. When a keybind's handler moves,
/// update the file path here too. A `None` row falls back to its expanded
/// action label in the panel.
fn describe(label: &str) -> Option<&'static str> {
    // Shift-action labels carry a "X: " prefix (e.g. "O: −60", "R: drill back")
    // so the keycap can show which physical key + Shift triggers them. Strip a
    // leading single-char prefix of the form "<c>: " before matching so the
    // shift variant shares its base description where the meaning is identical.
    let key = strip_shift_prefix(label);
    let d = match key {
        // ── Page / cursor navigation ──
        "pg fwd" => "Action::PageForward — src/input/navigation.rs",
        "pg back" => "Action::PageBackward — src/input/navigation.rs",
        "cursor ↓" | "cursor down" => "Action::CursorNextDialogue — src/input/navigation.rs",
        "cursor ↑" | "cursor up" => "Action::CursorPrevLine — src/input/navigation.rs",
        "prev dlg" => "Action::JumpToPrevDialogue — src/input/navigation.rs",
        "next dlg" => "Action::JumpToNextDialogue — src/input/navigation.rs",
        "next speaker" => "Action::JumpToNextSpeaker — src/input/navigation.rs",
        "prev speaker" => "Action::JumpToPrevSpeaker — src/input/navigation.rs",
        "go to start" => "gg chord (Action::PendingG) — src/input/navigation.rs",
        "go to end" => "Action::JumpToEnd — src/input/navigation.rs",
        "pg back btm" => "Action::PageBackwardBottom — src/input/navigation.rs",

        // ── Chapters / scenes ──
        "prev ch" => "Action::JumpToPrevChapter — src/input/navigation.rs",
        "next ch" => "Action::JumpToNextChapter — src/input/navigation.rs",
        "prev scene" => "Action::JumpToPrevScene — src/input/navigation.rs",
        "next scene" => "Action::JumpToNextScene — src/input/navigation.rs",

        // ── Bookmarks ──
        "bookmark" => "Action::ToggleBookmark — src/input/actions/bookmarks.rs",
        "bkmk tap" => "Action::BookmarkTap (toggle; .. reverts and opens the \
picker via ChordState::PendingPeriod) — src/input/keymap.rs",
        "prev bkmk" => "Action::PrevBookmark — src/input/navigation.rs",
        "next bkmk" => "Action::NextBookmark — src/input/navigation.rs",
        "latest bookmark" => "g; chord (Action::PendingG) — src/input/actions/bookmarks.rs",
        "bookmarks" => "Action::OpenBookmarkPicker — src/input/actions/pickers.rs",

        // ── Pickers / overlays ──
        "lib picker" => "Action::OpenLibraryPicker — src/input/actions/pickers.rs",
        "media picker" => "Action::OpenMediaPicker — src/input/actions/pickers.rs",
        "conc picker" => "Action::OpenConcordancePicker — src/input/actions/concordance.rs",
        "conc word" => "Action::OpenConcordanceWordPicker — src/input/actions/pickers.rs",
        "phrase hl" => "Action::TogglePhraseHighlight — src/input/phrase_highlight.rs",
        "conc list" => "Action::OpenConcordanceListPicker — src/input/actions/pickers.rs",
        "conc works" => "Action::OpenConcordanceWorksPicker — src/input/actions/pickers.rs",
        "settings" => "Action::OpenSettingsOverlay — src/input/actions/settings.rs",
        "keybinds" => "Action::OpenKeybindsOverlay — src/input/actions/pickers.rs",
        "search" => "Action::OpenSearch — src/input/search.rs",
        "search back" => "Action::OpenSearchBackward — src/input/search.rs",
        "next match" => "Action::SearchNextMatch — src/input/search.rs",
        "prev match" => "Action::SearchPrevMatch — src/input/search.rs",

        // ── Gloss / echo system ──
        "gloss tog" => "Action::ToggleGlossOverlay — src/input/actions/gloss.rs",
        "gloss pick" => "Action::OpenGlossPicker — src/input/actions/pickers.rs",
        "journal tog" => "Action::ToggleJournalOverlay — src/input/actions/journal.rs",
        "last overlay" => "Action::ToggleLastOverlay (reader only: reopens \
the last-closed gloss/journal overlay; overlays close via Escape) \
— src/input/actions/gloss.rs",
        "cycle overlays" => "Action::CycleSegmentOverlays (journal Q&A → gloss \
→ synopsis, wraps; segment fixed at lap entry) — src/input/actions/overlay_cycle.rs",
        "chat layout" => "Action::ToggleChatLayout (Tab opens/cycles focus; \
Ctrl+Tab closes) — src/input/actions/chat.rs",
        "jrnl Q&A picker" => "Action::OpenJournalPicker — src/input/actions/journal.rs",
        "last gloss" => "Action::OpenLastGloss — src/input/actions/gloss.rs",
        "BCP echo turns" => "Action::ShowEchoTurnsBcp — src/input/actions/echoes.rs",
        "BCP echoes" => "Action::ShowEchoesBcp — src/input/actions/echoes.rs",
        "reopen BCP echoes" => "Action::ReopenEchoesBcp — src/input/actions/echoes.rs",
        "Shx echo turns" => "Action::ShowEchoTurnsShx — src/input/actions/echoes.rs",
        "Shx echoes" => "Action::ShowEchoesShx — src/input/actions/echoes.rs",
        "reopen Shx echoes" => "Action::ReopenEchoesShx — src/input/actions/echoes.rs",

        // ── Vocab ──
        "vocab drill" => "Action::JumpToNextVocab -> InputMode::VocabLoop \
(n/p step, a/Space pause, Esc/Ctrl+- exit; unavailable -> toast) \
— src/input/vocab_loop.rs",
        "drill back" => "Action::JumpToPrevVocab -> InputMode::VocabLoop \
— src/input/actions/concordance.rs",
        "vocab hi" => "Action::ToggleVocabHighlight — src/app.rs",
        "auto vocab" => "Action::ToggleVocabPopup — src/app/vocab_popup.rs",
        "vocab tap" => "Action::VocabPopupTap (visible: next word; rr: \
show/hide via ChordState::PendingR) — src/input/keymap.rs",
        "vocab Q&A" => "Action::VocabJournalAsk (popup visible + vocab word \
on cursor line: ask/show stored) — src/input/actions/vocab_journal.rs",
        "Q&A page ▼" => "Action::VocabJournalPageNext — src/input/actions/vocab_journal.rs",
        "Q&A page ▲" => "Action::VocabJournalPagePrev — src/input/actions/vocab_journal.rs",

        // ── Word copy / visual ──
        "copy word" => "Action::WordCycleCopy — src/input/actions/word_copy.rs",
        "copy id" => "Action::CopyLineMappingId — src/input/keymap.rs",
        "collect" => "Action::WordCollectCopy — src/input/actions/word_copy.rs",
        "visual mode" => "Action::EnterVisualMode -> InputMode::Visual \
— src/input/visual.rs",

        // ── MPV / audio ──
        "play/pause" => "Action::TogglePause — src/input/keymap.rs",
        "vim copy" => "Action::OpenSegmentVim -> InputMode::SegmentVim \
— src/input/actions/segment_vim.rs",
        "cycle speed" => "Action::TogglePlaybackSpeed (1.0 -> 1.3 -> 0.9) \
— src/input/keymap.rs",
        "seek −3.5" => "Action::SeekShortBackward — src/input/phrase_highlight.rs, \
src/input/keymap.rs",
        "seek +3.5" => "Action::SeekShortForward — src/input/phrase_highlight.rs, \
src/input/keymap.rs",
        "−60" => "Action::SeekLongBackward — src/input/keymap.rs",
        "+60" => "Action::SeekLongForward — src/input/keymap.rs",
        "volume +" => "Action::VolumeUp — src/input/keymap.rs",
        "volume −" => "Action::VolumeDown — src/input/keymap.rs",
        "sync tap" => "Action::PlaybackSyncTap (toast only; ss toggles sync \
via ChordState::PendingS) — src/input/keymap.rs",

        // ── Timestamps ──
        "start time" | "set start time" => "Action::SetStartTime — src/input/timestamps.rs",
        "set end time" => "Action::SetEndTime — src/input/timestamps.rs",
        "set track mark" => "Action::SetChapter — src/input/timestamps.rs",
        "toggle ch start" => "Action::ToggleChapterStart — src/input/actions/chapters.rs",
        "show chapter" => "Action::ShowCurrentChapter — src/input/navigation.rs",
        "copy work info" => "Action::CopyWorkInfo — src/input/keymap.rs",
        "ts tap" => "Action::DeleteTimestampTap (toast only; 2x deletes via \
ChordState::PendingBackspace) — src/input/keymap.rs",
        "undo ts" => "Action::UndoTimestamp — src/input/timestamps.rs",
        "nudge −0.2" => "Action::NudgeStartBackward — src/input/timestamps.rs",
        "+0.2" => "Action::NudgeStartForward — src/input/timestamps.rs",
        "play from ts" => "Space intercept -> timestamps::play_current_line \
— src/input/timestamps.rs",
        "clear AB" => "escape::escape_reader_mode — src/input/actions/escape.rs",

        // ── Fonts ──
        "next font" => "Action::CycleFontForward — src/app.rs",
        "prev font" => "Action::CycleFontBackward — src/app.rs",
        "font +" => "Action::AdjustFontSizeUp — src/app.rs",
        "font −" => "Action::AdjustFontSizeDown — src/app.rs",
        "reset font" => "Action::ResetFontSize — src/app.rs",
        "font info" => "Action::ShowFontInfo — src/app.rs",

        // ── Display toggles ──
        "toggle signs" => "Action::ToggleSignColumn — src/app.rs",
        "chat side" => "Action::ChatPanelFlipSide — src/input/actions/chat.rs",
        "synopsis" => "Action::ShowSynopsisOverlay — src/app.rs",
        "col layout" => "Action::ToggleColumnLayout — src/input/navigation.rs",
        "ask passage" => "Action::AskPassage -> InputMode::Visual (Ctrl+a/Return \
opens the ask card, j/k extend, Esc cancels) — src/input/visual.rs",
        "authorship" => "Action::ToggleAuthorship — src/app.rs",
        "attr set" => "Action::PickAttributionSet — src/input/keymap.rs",
        "nav test" => "Action::ToggleNavTest — src/input/keymap.rs",
        "root variant prev" => "Action::RootVariantPrev — src/input/actions/settings.rs",
        "root variant" => "Action::RootVariantNext — src/input/actions/settings.rs",
        "theme next" => "Action::ThemeNext — src/input/actions/settings.rs",
        "theme prev" => "Action::ThemePrev — src/input/actions/settings.rs",
        "scansion" => "Action::CycleScansion — src/input/keymap.rs",
        "2-col translation" => "Action::ShowTranslationOverlay — src/app.rs",
        "inline translation" => "Action::ToggleTranslations — src/app.rs",
        "page image" => "Action::ToggleImageView — src/app.rs",
        "calibrate pages" => "Action::EnterPageCalibration — src/app.rs",
        "dim tog" => "Action::ToggleDim — src/input/keymap.rs",
        "save+quit" => "Action::SaveAndQuit — src/input/keymap.rs",
        "debug log" => "Action::ToggleDebugLogging — src/input/keymap.rs",

        _ => return None,
    };
    Some(d)
}

/// Strip a leading shift-key prefix of the form `"<char>: "` from a shift-action
/// label so the variant matches its base description (e.g. `"O: −60"` -> `"−60"`,
/// `"R: drill back"` -> `"drill back"`). Returns the label unchanged if it has no
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
        "vocab drill" => "vocab-sentence drill loop",
        "drill back" => "vocab-sentence drill, backward",
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
        "chat side" => "flip chat panel column",
        "sync tap" => "sync state (ss toggles)",
        "bkmk tap" => "bookmark (.. opens picker)",
        "dim tog" => "toggle dim",
        "debug log" => "toggle debug log",
        "set track mark" => "set audio track mark",
        "toggle ch start" => "toggle structural chapter",
        "show chapter" => "show current chapter",
        "copy work info" => "copy abbrev + media + whisperX",
        "bookmarks" => "bookmark picker",
        "start time" => "set start time",
        "set end time" => "set end time",
        "play from ts" => "play from timestamp",
        "ts tap" => "timestamp (2x deletes)",
        "copy id" => "copy line id",
        "save+quit" => "save and quit",
        "clear AB" => "clear A-B / exit mode",
        "font info" => "show font info",
        "font +" => "increase font size",
        "font −" => "decrease font size",
        "reset font" => "reset font size",
        "vocab tap" => "vocab popup (rr toggles)",
        "vocab Q&A" => "vocab word journal Q&A",
        "Q&A page ▼" => "vocab Q&A next page",
        "Q&A page ▲" => "vocab Q&A previous page",
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

    // Layout constants for the detail panel. Two columns only: the key glyph
    // and the Rust-idiom description (Action::/InputMode::/handler path) —
    // there is no separate action-label column.
    let pad: f64 = 40.0; // inner padding (left/right/top/bottom breathing room)
    let glyph_x = panel_x + pad; // key-glyph column
    let desc_x = panel_x + pad + 220.0; // Rust-idiom column (clears "Ctrl+Shift+X")
    let desc_max_w = panel_x + panel_w - pad - desc_x; // free width for blurbs
    let row_pad: f64 = 12.0; // vertical breathing room per binding row
    let desc_line_h: f64 = 30.0; // line height inside a wrapped blurb
    let base_row_h: f64 = 36.0; // height of a row with no blurb
    let desc_font: f64 = 22.0; // blurb font size
    let shift_gap: f64 = desc_line_h; // one blank line above the shifted key

    // Pre-pass: wrap each description (the FULL describe() text — the code
    // reference is part of the Rust idiom, not hidden) and record per-row
    // height so the panel grows to fit. A bound row with no describe() arm
    // falls back to its expanded action label. Wrapping must use the same
    // font the blurb is drawn with (monospace — the description is code).
    cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(desc_font);
    let wrapped: Vec<Vec<String>> = rows
        .iter()
        .map(|(_, act, _, blurb, _)| match blurb {
            Some(text) => wrap_to_width(cr, text, desc_max_w),
            None if !act.is_empty() => wrap_to_width(cr, &expand_action(act), desc_max_w),
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

    // Binding rows: <key glyph>  <Rust-idiom description>, all at the body
    // size. The glyph takes the binding-type color (pine bare / iris Shift /
    // gold Ctrl / green Ctrl+Shift / rose Alt) — the color coding the old
    // action-label column carried. First row's baseline sits one body-line
    // below the top padding.
    let mut ry = panel_y + pad + desc_font;
    for (i, (glyph, _, col, _, is_shift)) in rows.iter().enumerate() {
        // Two blank lines above the shifted key.
        if *is_shift {
            ry += shift_gap;
        }
        let top = ry; // baseline of the key line
        // Key glyph (the physical key / combo that triggers this binding).
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        cr.set_font_size(desc_font);
        cr.set_source_rgb(col.0, col.1, col.2);
        let _ = cr.move_to(glyph_x, top);
        let _ = cr.show_text(glyph);

        // Wrapped Rust-idiom description to the right.
        let lines = &wrapped[i];
        if !lines.is_empty() {
            cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(desc_font);
            cr.set_source_rgb(0.376, 0.357, 0.439);
            for (li, line) in lines.iter().enumerate() {
                let _ = cr.move_to(desc_x, top + li as f64 * desc_line_h);
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
