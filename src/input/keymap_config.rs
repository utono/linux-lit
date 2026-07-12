//! Keymap configuration: KeyCombo struct + Keymap loader.
//!
//! Loaded from ~/.config/linux-lit/keymap.json with compiled-in defaults.
//! Falls back to defaults on missing or malformed JSON. Mirrors lue's
//! load_keyboard_shortcuts pattern (lue/lue/input_handler.py:48-64).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::input::actions::Action;

/// One key combination. `key` is the GDK key name as logged by handle_key
/// (e.g., "x", "Return", "BackSpace", "comma"). Modifiers default to false
/// when omitted from JSON.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct KeyCombo {
    pub key: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl KeyCombo {
    pub fn plain(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: false, alt: false }
    }
    pub fn ctrl(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: false, alt: false }
    }
    pub fn shift(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: true, alt: false }
    }
    pub fn alt(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: false, alt: true }
    }
    pub fn ctrl_shift(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: true, alt: false }
    }
    pub fn ctrl_alt(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: false, alt: true }
    }
    pub fn alt_shift(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: true, alt: true }
    }
}

/// Reader-mode keybinds. Per-overlay keymaps are deferred to F1.
pub struct Keymap {
    pub reader: HashMap<KeyCombo, Action>,
}

#[derive(Deserialize)]
struct KeymapJson {
    #[serde(default)]
    reader: Vec<BindingJson>,
}

#[derive(Deserialize)]
struct BindingJson {
    key: String,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    action: String,
}

impl Keymap {
    /// Load keymap from `~/.config/linux-lit/keymap.json` if present, else
    /// return defaults. Malformed JSON logs a warning and falls back.
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(text) = std::fs::read_to_string(&path) {
            Self::from_json_str(&text)
        } else {
            crate::logging::log(&format!(
                "keymap.json not found at {}; using compiled-in defaults",
                path.display()
            ));
            Self::default()
        }
    }

    /// Parse keymap from a JSON string. Used by tests and load(). Malformed
    /// JSON returns defaults entirely; unknown action names are skipped with
    /// a logged warning.
    pub fn from_json_str(json: &str) -> Self {
        let parsed: KeymapJson = match serde_json::from_str(json) {
            Ok(p) => p,
            Err(e) => {
                crate::logging::log(&format!("keymap.json parse error: {}; using defaults", e));
                return Self::default();
            }
        };
        let mut km = Self::default();
        for b in parsed.reader {
            let action = match parse_action(&b.action) {
                Some(a) => a,
                None => {
                    crate::logging::log(&format!(
                        "keymap.json: unknown action '{}', skipping",
                        b.action
                    ));
                    continue;
                }
            };
            let combo = KeyCombo {
                key: b.key,
                ctrl: b.ctrl,
                shift: b.shift,
                alt: b.alt,
            };
            km.reader.insert(combo, action);
        }
        km
    }

    pub fn lookup(&self, key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
        // GTK delivers shifted ASCII letters with key_name already
        // capitalized AND is_shift=true (e.g., Shift+g → "G", shift=true).
        // The shift modifier is then redundant — strip it before lookup so
        // bindings can be defined as KeyCombo::plain("G") rather than
        // requiring KeyCombo::shift("G"). Only applies to single ASCII
        // uppercase letters; symbols are layout-dependent (e.g., on RPD
        // Shift+comma may emit ("comma", shift=true) rather than ("less",
        // shift=false)) and treated as significant.
        // Ctrl+Shift+X stays distinct from Ctrl+X — only strip when ctrl
        // and alt are both off.
        let effective_shift = if !ctrl && !alt && is_uppercase_letter(key) {
            false
        } else {
            shift
        };
        let combo = KeyCombo {
            key: key.to_string(),
            ctrl,
            shift: effective_shift,
            alt,
        };
        self.reader.get(&combo).copied()
    }
}

/// True when the key name is a single ASCII uppercase letter (the shifted
/// form is encoded in the key name itself, so the shift modifier flag is
/// redundant).
fn is_uppercase_letter(key: &str) -> bool {
    key.len() == 1 && key.chars().next().map_or(false, |c| c.is_ascii_uppercase())
}

impl Default for Keymap {
    fn default() -> Self {
        Self { reader: default_reader_bindings() }
    }
}

fn config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join(".config/linux-lit/keymap.json")
}

fn parse_action(name: &str) -> Option<Action> {
    // serde_json round-trip via a single-element JSON value.
    let json = format!("\"{}\"", name);
    serde_json::from_str(&json).ok()
}

/// Compiled-in default reader bindings, assembled from per-category
/// sub-functions. Each sub-function groups bindings by the Action's
/// Category for organizational clarity; the runtime Keymap is a flat
/// HashMap.
pub fn default_reader_bindings() -> HashMap<KeyCombo, Action> {
    let mut m = HashMap::new();
    for (combo, action) in nav_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in media_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in vocab_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in display_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in selection_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in timestamp_bindings() {
        m.insert(combo, action);
    }
    for (combo, action) in app_bindings() {
        m.insert(combo, action);
    }
    m
}

fn nav_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        // Page navigation
        (KeyCombo::plain("x"), Action::PageForward),
        (KeyCombo::plain("y"), Action::PageBackward),
        // Shift+, emits ("less", shift=true) on this layout (verified in the
        // debug log), NOT ("comma", shift=true) — bind the shifted glyph.
        (KeyCombo::shift("less"), Action::JumpToPrevDialogue),
        // space / Shift+space were PageForward/PageBackward; space is now a
        // global play/pause toggle handled directly in handle_key.
        // Cursor / dialogue
        (KeyCombo::plain("j"), Action::CursorNextDialogue),
        (KeyCombo::plain("k"), Action::CursorPrevLine),
        (KeyCombo::plain("Q"), Action::JumpToNextDialogue),
        (KeyCombo::plain("Up"), Action::CursorPrevLine),
        (KeyCombo::shift("Up"), Action::PageBackwardBottom),
        (KeyCombo::plain("Down"), Action::CursorNextDialogue),
        (KeyCombo::plain("comma"), Action::JumpToPrevSpeaker),
        (KeyCombo::plain("q"), Action::JumpToNextSpeaker),
        (KeyCombo::plain("J"), Action::JumpToNextSpeaker),
        (KeyCombo::plain("K"), Action::JumpToPrevSpeaker),
        // Multi-key chord entry (gg → JumpToStart)
        (KeyCombo::plain("g"), Action::PendingG),
        (KeyCombo::plain("G"), Action::JumpToEnd),
        // Chapter / scene
        // RPD: the 2/3 number-row keys emit bracketleft/braceleft unshifted and
        // 2/3 shifted. Scene jumps sit on BOTH glyphs of each key: `[`/Shift+`[`
        // jump to the current scene's first line (thereafter the previous
        // scene); `{`/Shift+`{` jump to the next scene's first line.
        // RPD: `(`/`4` share the AE04 key (unshifted symbol, shifted digit) and
        // `&`/`5` share AE05. The unshifted symbol steps BOOKMARKS; the shifted
        // digit takes the chapter jump the symbol used to have.
        (KeyCombo::plain("parenleft"), Action::PrevBookmark),
        (KeyCombo::plain("ampersand"), Action::NextBookmark),
        (KeyCombo::plain("4"), Action::JumpToPrevChapter),
        (KeyCombo::shift("4"), Action::JumpToPrevChapter),
        (KeyCombo::plain("5"), Action::JumpToNextChapter),
        (KeyCombo::shift("5"), Action::JumpToNextChapter),
        // `}` (braceright on RPD) duplicates `4` — jump to the previous chapter.
        (KeyCombo::plain("braceright"), Action::JumpToPrevChapter),
        // `]` (bracketright on RPD) duplicates `5` — jump to the next chapter.
        (KeyCombo::plain("bracketright"), Action::JumpToNextChapter),
        (KeyCombo::plain("bracketleft"), Action::JumpToPrevScene),
        (KeyCombo::plain("braceleft"), Action::JumpToNextScene),
        (KeyCombo::plain("C"), Action::ShowCurrentChapter),
        // Shift+; emits ("colon", shift=true) on this layout (same class as
        // Shift+, → "less") — toggle playback speed. The bare-name form is
        // bound too in case the compositor reports ("semicolon", shift=true)
        // instead.
        (KeyCombo::shift("colon"), Action::TogglePlaybackSpeed),
        (KeyCombo::shift("semicolon"), Action::TogglePlaybackSpeed),
        // Bookmarks (`(`/`&` on the AE04/AE05 keys; `;`/`'` are home-region
        // aliases — `;` displaced ShowCurrentChapter, which keeps `C` and
        // gains Shift+`;`)
        (KeyCombo::plain("semicolon"), Action::PrevBookmark),
        (KeyCombo::plain("apostrophe"), Action::NextBookmark),
        (KeyCombo::plain("m"), Action::ToggleBookmark),
        // `.` is overloaded (Action::BookmarkTap): single tap toggles the
        // bookmark; .. reverts the toggle and opens the picker.
        (KeyCombo::plain("period"), Action::BookmarkTap),
        (KeyCombo::ctrl("c"), Action::SetChapter),
        (KeyCombo::ctrl("e"), Action::ShowEchoesBcp),
        // `2` duplicates `}` (braceright) / `4` — jump to the previous chapter.
        (KeyCombo::plain("2"), Action::JumpToPrevChapter),
        (KeyCombo::shift("2"), Action::JumpToPrevChapter),
        // `3` duplicates `]` (bracketright) / `5` — jump to the next chapter.
        (KeyCombo::plain("3"), Action::JumpToNextChapter),
        (KeyCombo::shift("3"), Action::JumpToNextChapter),
        (KeyCombo::ctrl("period"), Action::OpenBookmarkPicker),
    ]
}

fn media_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        // `s` is overloaded (Action::PlaybackSyncTap): single tap toasts the
        // sync state; ss toggles — an accidental press can't kill sync.
        (KeyCombo::plain("s"), Action::PlaybackSyncTap),
        // Space plays from the cursor line's start timestamp (PlayCurrentLine;
        // it is intercepted before dispatch in keymap.rs). `a` is a PURE
        // pause/resume toggle — no seek (TogglePause).
        (KeyCombo::plain("a"), Action::TogglePause),
        // '-' is unbound (vocab popup cycling moved to `r`; Ctrl+- enters
        // the vocab-sentence loop, InputMode::VocabLoop — no jump fallback).
        (KeyCombo::plain("o"), Action::SeekShortBackward),
        (KeyCombo::plain("e"), Action::SeekShortForward),
        (KeyCombo::plain("O"), Action::SeekLongBackward),
        (KeyCombo::plain("E"), Action::SeekLongForward),
        (KeyCombo::plain("Left"), Action::SeekShortBackward),
        (KeyCombo::ctrl("Up"), Action::VolumeUp),
        (KeyCombo::ctrl("Down"), Action::VolumeDown),
        (KeyCombo::plain("plus"), Action::ShowCurrentChapter),
        (KeyCombo::alt("p"), Action::TogglePhraseHighlight),
    ]
}

fn vocab_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("h"), Action::ShowSynopsisOverlay),
        // `r` is overloaded (Action::VocabPopupTap): a single tap cycles the
        // visible popup's words; rr in quick succession toggles visibility
        // (ChordState::PendingR). HideVocabPopup is unbound — rr covers it.
        // VocabPopupNext is unbound (tap covers the cycle). ConcordanceNext
        // AND ConcordancePrev are deliberately unbound (step in-work hits
        // with n/N while a concordance is active).
        (KeyCombo::plain("r"), Action::VocabPopupTap),
        // Ctrl+r: vocab journal Q&A — ask about the popup's current word
        // (gated on popup visible + a vocab word on the cursor line; was R).
        // Ctrl+n/p page the popup's Journal answer; the pickers/overlays
        // keep their own modal Ctrl+n/p (handled before reader dispatch).
        (KeyCombo::ctrl("r"), Action::VocabJournalAsk),
        (KeyCombo::ctrl("n"), Action::VocabJournalPageNext),
        (KeyCombo::ctrl("p"), Action::VocabJournalPagePrev),
        (KeyCombo::alt("backslash"), Action::ToggleVocabHighlight),
        (KeyCombo::ctrl_shift("G"), Action::OpenLastGloss),
        // Alt+u/Alt+i are swapped with the plain u/i swap: Alt+u cycles
        // scansion, Alt+i sets the end timestamp (beside plain i's start).
        (KeyCombo::alt("u"), Action::CycleScansion),
        // ALL concordance pickers cluster on z: plain opens the main picker
        // (was `\`), Ctrl+z the word picker (was Ctrl+Shift+P), Alt+z the
        // works picker (was Alt+r), Ctrl+Shift+Z the occurrence-list picker
        // (was Ctrl+Alt+c).
        (KeyCombo::plain("z"), Action::OpenConcordancePicker),
        (KeyCombo::ctrl("z"), Action::OpenConcordanceWordPicker),
        (KeyCombo::alt("z"), Action::OpenConcordanceWorksPicker),
        (KeyCombo::ctrl_shift("Z"), Action::OpenConcordanceListPicker),
        (KeyCombo::ctrl("g"), Action::ToggleGlossOverlay),
        (KeyCombo::alt("g"), Action::OpenGlossPicker),
        (KeyCombo::ctrl("j"), Action::ToggleJournalOverlay),
        (KeyCombo::alt("j"), Action::OpenJournalPicker),
        (KeyCombo::ctrl("Tab"), Action::ToggleLastOverlay),
        // `\` cycles the segment overlays: journal Q&A → gloss → synopsis
        // (wraps; advance arms live in the overlay modal handlers).
        (KeyCombo::plain("backslash"), Action::CycleSegmentOverlays),
        (KeyCombo::plain("H"), Action::ToggleVocabPopup),
    ]
}

fn display_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::ctrl("exclam"), Action::AdjustFontSizeDown),
        (KeyCombo::ctrl("bar"), Action::AdjustFontSizeUp),
        (KeyCombo::plain("0"), Action::ResetFontSize),
        (KeyCombo::plain("f"), Action::CycleFontForward),
        (KeyCombo::plain("F"), Action::CycleFontBackward),
        (KeyCombo::plain("l"), Action::ToggleSignColumn),
        // Tab toggles the chat layout; TogglePreviousWork is unbound (Ctrl+minus recent picker covers it).
        (KeyCombo::plain("Tab"), Action::ToggleChatLayout),
        (KeyCombo::alt("d"), Action::ToggleDim),
        (KeyCombo::alt("t"), Action::ThemeNext),
        (KeyCombo::alt_shift("T"), Action::ThemePrev),
        (KeyCombo::ctrl("t"), Action::RootVariantNext),
        (KeyCombo::ctrl_shift("T"), Action::RootVariantPrev),
        // u/i plain binds are swapped (2026-07-11): i sets the start time,
        // u opens the two-column translation. Their modifier families stay
        // put (Shift+U undo ts, Alt+u end ts; Ctrl+i image, Alt+i scansion).
        (KeyCombo::plain("u"), Action::ShowTranslationOverlay),
        (KeyCombo::alt("bracketleft"), Action::ToggleColumnLayout),
        // Authorship moved off Ctrl+a (now AskPassage). plain("A") is the
        // shifted `a` (cf. plain("G") normalization above).
        (KeyCombo::plain("A"), Action::ToggleAuthorship),
        (KeyCombo::ctrl_shift("A"), Action::PickAttributionSet),
        (KeyCombo::alt("f"), Action::ShowFontInfo),
        (KeyCombo::ctrl_alt("i"), Action::ToggleTranslations),
        (KeyCombo::ctrl("i"), Action::ToggleImageView),
        (KeyCombo::ctrl_shift("I"), Action::EnterPageCalibration),
        (KeyCombo::alt("e"), Action::ShowEchoTurnsBcp),
        (KeyCombo::alt("w"), Action::ShowEchoesShx),
        (KeyCombo::ctrl("w"), Action::ShowEchoTurnsShx),
        (KeyCombo::ctrl_shift("W"), Action::ReopenEchoesShx),
        // Ctrl+h unbound; ToggleSynopsis (the persistent side panel) remains
        // available for user keymaps.
        // Ctrl+l: flip a floating chat panel to the other reading column.
        (KeyCombo::ctrl("l"), Action::ChatPanelFlipSide),
        (KeyCombo::ctrl("comma"), Action::OpenSettingsOverlay),
    ]
}

fn selection_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("V"), Action::EnterVisualMode),
        // Ctrl+a: paragraph/speech ask — pre-selects the block, second Ctrl+a
        // or Return opens the Journal Q&A ask card.
        (KeyCombo::ctrl("a"), Action::AskPassage),
        // Copy-only vim view of the cursor's segment: opens in VISUAL mode,
        // visual `y` copies to the system clipboard, nothing is ever saved.
        (KeyCombo::plain("v"), Action::OpenSegmentVim),
        (KeyCombo::plain("w"), Action::WordCycleCopy),
        (KeyCombo::plain("W"), Action::WordCollectCopy),
    ]
}

fn timestamp_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("i"), Action::SetStartTime),
        (KeyCombo::plain("Right"), Action::SetStartTime),
        (KeyCombo::alt("i"), Action::SetEndTime),
        (KeyCombo::plain("c"), Action::ToggleChapterStart),
        // BackSpace is overloaded (Action::DeleteTimestampTap): single tap
        // toasts the line's timestamp; a second quick tap deletes it.
        (KeyCombo::plain("BackSpace"), Action::DeleteTimestampTap),
        (KeyCombo::plain("p"), Action::NudgeStartBackward),
        (KeyCombo::plain("P"), Action::NudgeStartForward),
        (KeyCombo::plain("U"), Action::UndoTimestamp),
    ]
}

fn app_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("Escape"), Action::EscapeReaderMode),
        (KeyCombo::ctrl("d"), Action::ToggleDebugLogging),
        (KeyCombo::ctrl_alt("t"), Action::ToggleNavTest),
        (KeyCombo::ctrl_shift("E"), Action::ReopenEchoesBcp),
        (KeyCombo::ctrl("backslash"), Action::OpenLibraryPicker),
        // Both vocab-drill entries live on the minus cap: Ctrl+- forward,
        // Ctrl+Shift+- backward (InputMode::VocabLoop); when the mode can't
        // start the reason is toasted — no jump fallback. Shifted minus can
        // arrive as "underscore" or as "minus"+shift depending on layout
        // path, so the backward entry binds both (same defensive dual as
        // slash/question).
        (KeyCombo::ctrl("minus"), Action::JumpToNextVocab),
        (KeyCombo::ctrl_shift("underscore"), Action::JumpToPrevVocab),
        (KeyCombo::ctrl_shift("minus"), Action::JumpToPrevVocab),
        (KeyCombo::ctrl("m"), Action::OpenMediaPicker),
        (KeyCombo::ctrl("slash"), Action::OpenKeybindsOverlay),
        (KeyCombo::plain("slash"), Action::OpenSearch),
        // `?` = Shift+slash → backward search. RPD/xkb maps <AD11> level 2 to
        // `question`; GTK usually delivers ("question", shift=true). Bind both
        // the shifted-symbol form and the shift("slash") form so it resolves
        // regardless of how the layout reports it.
        (KeyCombo::plain("question"), Action::OpenSearchBackward),
        (KeyCombo::shift("question"), Action::OpenSearchBackward),
        (KeyCombo::shift("slash"), Action::OpenSearchBackward),
        (KeyCombo::ctrl_shift("L"), Action::SaveAndQuit),
        (KeyCombo::ctrl("y"), Action::CopyLineMappingId),
        // Shift+'+' — copy work abbrev + active media path + large whisperX
        // JSON. The RPD number-row plus key (<AE01>) delivers `1` at the
        // shift level; bind both delivery forms (cf. `question` above).
        (KeyCombo::shift("1"), Action::CopyWorkInfo),
        (KeyCombo::plain("1"), Action::CopyWorkInfo),
        (KeyCombo::plain("n"), Action::SearchNextMatch),
        (KeyCombo::plain("N"), Action::SearchPrevMatch),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::actions::Action;

    #[test]
    fn default_reader_bindings_returns_nonempty_map() {
        let m = default_reader_bindings();
        assert!(m.len() > 50, "expected ~70 default bindings, got {}", m.len());
    }

    #[test]
    fn default_reader_bindings_contains_known_bindings() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("x")), Some(&Action::PageForward));
        assert_eq!(m.get(&KeyCombo::plain("y")), Some(&Action::PageBackward));
        assert_eq!(m.get(&KeyCombo::plain("j")), Some(&Action::CursorNextDialogue));
        assert_eq!(m.get(&KeyCombo::plain("k")), Some(&Action::CursorPrevLine));
        assert_eq!(m.get(&KeyCombo::plain("y")), Some(&Action::PageBackward));
        assert_eq!(m.get(&KeyCombo::ctrl("m")), Some(&Action::OpenMediaPicker));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("L")), Some(&Action::SaveAndQuit));
        assert_eq!(m.get(&KeyCombo::ctrl("a")), Some(&Action::AskPassage));
        assert_eq!(m.get(&KeyCombo::plain("A")), Some(&Action::ToggleAuthorship));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("A")), Some(&Action::PickAttributionSet));
        assert_eq!(m.get(&KeyCombo::plain("u")), Some(&Action::ShowTranslationOverlay));
        assert_eq!(m.get(&KeyCombo::plain("i")), Some(&Action::SetStartTime));
        assert_eq!(m.get(&KeyCombo::alt("u")), Some(&Action::CycleScansion));
        assert_eq!(m.get(&KeyCombo::alt("i")), Some(&Action::SetEndTime));
        assert_eq!(m.get(&KeyCombo::ctrl_alt("i")), Some(&Action::ToggleTranslations));
        assert_eq!(m.get(&KeyCombo::ctrl("i")), Some(&Action::ToggleImageView));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("I")), Some(&Action::EnterPageCalibration));
    }

    #[test]
    fn r_cycles_vocab_and_ctrl_r_asks_journal() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("Tab")), Some(&Action::ToggleChatLayout));
        assert_eq!(m.get(&KeyCombo::plain("r")), Some(&Action::VocabPopupTap));
        // minus and # freed; Ctrl+r = vocab journal Q&A (was R, now unbound;
        // VocabPopupNext dropped from Ctrl+r); Ctrl+n/p page its answer.
        assert_eq!(m.get(&KeyCombo::plain("minus")), None);
        assert_eq!(m.get(&KeyCombo::plain("numbersign")), None);
        assert_eq!(m.get(&KeyCombo::ctrl("r")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::plain("R")), None);
        assert_eq!(m.get(&KeyCombo::ctrl("n")), Some(&Action::VocabJournalPageNext));
        assert_eq!(m.get(&KeyCombo::ctrl("p")), Some(&Action::VocabJournalPagePrev));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("R")), None);
        assert_eq!(m.get(&KeyCombo::ctrl("minus")), Some(&Action::JumpToNextVocab));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("underscore")), Some(&Action::JumpToPrevVocab));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("minus")), Some(&Action::JumpToPrevVocab));
        assert_eq!(m.get(&KeyCombo::plain("z")), Some(&Action::OpenConcordancePicker));
        assert_eq!(m.get(&KeyCombo::ctrl("z")), Some(&Action::OpenConcordanceWordPicker));
        assert_eq!(m.get(&KeyCombo::alt("z")), Some(&Action::OpenConcordanceWorksPicker));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("Z")), Some(&Action::OpenConcordanceListPicker));
        // `\` cycles the segment overlays (journal Q&A → gloss → synopsis).
        assert_eq!(
            m.get(&KeyCombo::plain("backslash")),
            Some(&Action::CycleSegmentOverlays)
        );
        assert_eq!(m.get(&KeyCombo::plain("a")), Some(&Action::TogglePause));
    }

    #[test]
    fn ctrl_l_flips_chat_panel_side() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::ctrl("l")), Some(&Action::ChatPanelFlipSide));
        assert_eq!(m.get(&KeyCombo::plain("l")), Some(&Action::ToggleSignColumn));
    }

    #[test]
    fn speaker_turn_keys_bound_to_capital_j_and_k() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("J")), Some(&Action::JumpToNextSpeaker));
        assert_eq!(m.get(&KeyCombo::plain("K")), Some(&Action::JumpToPrevSpeaker));
        // Lowercase j / k keep their existing cursor bindings (regression guard).
        assert_eq!(m.get(&KeyCombo::plain("j")), Some(&Action::CursorNextDialogue));
        assert_eq!(m.get(&KeyCombo::plain("k")), Some(&Action::CursorPrevLine));
    }

    #[test]
    fn shift_j_resolves_to_next_speaker_via_lookup() {
        let km = Keymap::default();
        // GTK delivers Shift+j as key "J" with shift=true; is_uppercase_letter
        // strips the redundant shift, so plain("J") matches.
        assert_eq!(km.lookup("J", false, true, false), Some(Action::JumpToNextSpeaker));
        assert_eq!(km.lookup("K", false, true, false), Some(Action::JumpToPrevSpeaker));
        // Bare , / q are the speaker jumps; the shifted forms are the
        // dialogue-line jumps. On this layout Shift+, emits ("less", shift=true)
        // — NOT ("comma", shift=true) — and Shift+q emits "Q" (uppercase,
        // shift stripped).
        assert_eq!(km.lookup("comma", false, false, false), Some(Action::JumpToPrevSpeaker));
        assert_eq!(km.lookup("q", false, false, false), Some(Action::JumpToNextSpeaker));
        assert_eq!(km.lookup("less", false, true, false), Some(Action::JumpToPrevDialogue));
        assert_eq!(km.lookup("Q", false, true, false), Some(Action::JumpToNextDialogue));
        // Ctrl+, opens the settings overlay (mirrors the per-overlay handlers).
        assert_eq!(km.lookup("comma", true, false, false), Some(Action::OpenSettingsOverlay));
    }

    #[test]
    fn keymap_lookup_returns_action_for_bound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("y", false, false, false), Some(Action::PageBackward));
    }

    #[test]
    fn alt_bracketleft_is_toggle_column_layout() {
        let km = Keymap::default();
        assert_eq!(
            km.lookup("bracketleft", false, false, true),
            Some(Action::ToggleColumnLayout),
        );
        // Both glyphs of the RPD 2-key jump scenes: plain [ and the shifted
        // 2 glyph (Shift+[) land on the current scene's start, thereafter the
        // previous scene. Same for {/3 and the next scene.
        assert_eq!(
            km.lookup("bracketleft", false, false, false),
            Some(Action::JumpToPrevScene),
        );
        assert_eq!(km.lookup("2", false, true, false), Some(Action::JumpToPrevChapter));
        assert_eq!(km.lookup("3", false, true, false), Some(Action::JumpToNextChapter));
        assert_eq!(km.lookup("braceleft", false, false, false), Some(Action::JumpToNextScene));
        // Shift+; (the shifted colon glyph) cycles playback speed; `+` toasts
        // the current act/scene or chapter (swapped in e42fd92).
        assert_eq!(km.lookup("colon", false, true, false), Some(Action::TogglePlaybackSpeed));
        assert_eq!(km.lookup("plus", false, false, false), Some(Action::ShowCurrentChapter));
        // Shift+'+' (the shifted `1` glyph on RPD <AE01>) copies the work
        // abbrev + media path + large whisperX JSON; both delivery forms.
        assert_eq!(km.lookup("1", false, true, false), Some(Action::CopyWorkInfo));
        assert_eq!(km.lookup("1", false, false, false), Some(Action::CopyWorkInfo));
    }

    #[test]
    fn alt_t_cycles_theme() {
        let km = Keymap::default();
        assert_eq!(km.lookup("t", false, false, true), Some(Action::ThemeNext));
        assert_eq!(km.lookup("T", false, true, true), Some(Action::ThemePrev));
    }

    #[test]
    fn ctrl_t_cycles_root_variant() {
        let km = Keymap::default();
        assert_eq!(km.lookup("t", true, false, false), Some(Action::RootVariantNext));
        assert_eq!(km.lookup("T", true, true, false), Some(Action::RootVariantPrev));
        assert_eq!(km.lookup("t", true, false, true), Some(Action::ToggleNavTest));
    }

    #[test]
    fn keymap_lookup_returns_none_for_unbound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("zzz", false, false, false), None);
    }

    #[test]
    fn keymap_lookup_distinguishes_modifiers() {
        let km = Keymap::default();
        // "a" plain is TogglePause; Ctrl+a is AskPassage; Ctrl+a vs Ctrl+Shift+a differ.
        let a_ctrl = km.lookup("a", true, false, false);
        let a_ctrl_shift = km.lookup("A", true, true, false);
        assert_ne!(a_ctrl, a_ctrl_shift);
        assert_eq!(km.lookup("f", false, false, false), Some(Action::CycleFontForward));
        assert_eq!(a_ctrl, Some(Action::AskPassage));
        assert_eq!(km.lookup("a", false, false, false), Some(Action::TogglePause));
        // plain v (segment vim copy) vs Shift+v (reader visual mode) differ.
        assert_eq!(km.lookup("v", false, false, false), Some(Action::OpenSegmentVim));
        assert_eq!(km.lookup("V", false, true, false), Some(Action::EnterVisualMode));
    }

    #[test]
    fn keymap_load_from_json_overrides_defaults() {
        let json = r#"{
            "reader": [
                {"key": "x", "action": "PageBackward"}
            ]
        }"#;
        let km = Keymap::from_json_str(json);
        // Override took effect:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageBackward));
        // Other defaults preserved:
        assert_eq!(km.lookup("y", false, false, false), Some(Action::PageBackward));
        assert_eq!(km.lookup("j", false, false, false), Some(Action::CursorNextDialogue));
    }

    #[test]
    fn keymap_load_from_malformed_json_returns_defaults() {
        let bad_json = "not valid json {{{ ";
        let km = Keymap::from_json_str(bad_json);
        // Falls back to defaults entirely:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("y", false, false, false), Some(Action::PageBackward));
    }

    #[test]
    fn keymap_load_skips_unknown_action() {
        let json = r#"{
            "reader": [
                {"key": "x", "action": "PageBackward"},
                {"key": "F12", "action": "ThisActionDoesNotExist"}
            ]
        }"#;
        let km = Keymap::from_json_str(json);
        // Override succeeded for known action:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageBackward));
        // Unknown action skipped silently:
        assert_eq!(km.lookup("F12", false, false, false), None);
    }
}
