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
        (KeyCombo::plain("less"), Action::PageBackward),
        // space / Shift+space were PageForward/PageBackward; space is now a
        // global play/pause toggle handled directly in handle_key.
        (KeyCombo::ctrl("f"), Action::PageForward),
        (KeyCombo::ctrl("u"), Action::PageForward),
        (KeyCombo::ctrl("b"), Action::PageBackward),
        // Cursor / dialogue
        (KeyCombo::plain("j"), Action::CursorNextDialogue),
        (KeyCombo::plain("k"), Action::CursorPrevLine),
        (KeyCombo::plain("Q"), Action::CursorToPageBottom),
        (KeyCombo::plain("Up"), Action::CursorPrevLine),
        (KeyCombo::shift("Up"), Action::PageBackwardBottom),
        (KeyCombo::plain("Down"), Action::CursorNextDialogue),
        (KeyCombo::plain("comma"), Action::JumpToPrevDialogue),
        (KeyCombo::shift("comma"), Action::PageBackwardBottom),
        (KeyCombo::plain("q"), Action::JumpToNextDialogue),
        // Multi-key chord entry (gg → JumpToStart, zt → ScrollCursorTop)
        (KeyCombo::plain("g"), Action::PendingG),
        (KeyCombo::plain("G"), Action::JumpToEnd),
        (KeyCombo::plain("z"), Action::PendingZ),
        // Chapter / scene
        (KeyCombo::plain("parenleft"), Action::JumpToPrevChapter),
        (KeyCombo::plain("ampersand"), Action::JumpToNextChapter),
        (KeyCombo::plain("2"), Action::JumpToPrevScene),
        (KeyCombo::shift("2"), Action::JumpToPrevScene),
        (KeyCombo::plain("3"), Action::JumpToNextScene),
        (KeyCombo::shift("3"), Action::JumpToNextScene),
        (KeyCombo::plain("C"), Action::ShowCurrentChapter),
        (KeyCombo::plain("semicolon"), Action::ShowCurrentChapter),
        // Bookmarks
        (KeyCombo::plain("m"), Action::ToggleBookmark),
        (KeyCombo::ctrl("e"), Action::ReopenEchoes),
        (KeyCombo::plain("bracketleft"), Action::PrevBookmark),
        (KeyCombo::plain("braceleft"), Action::NextBookmark),
        (KeyCombo::ctrl("period"), Action::OpenBookmarkPicker),
    ]
}

fn media_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("s"), Action::TogglePlaybackSync),
        (KeyCombo::plain("Tab"), Action::TogglePlayback),
        (KeyCombo::plain("o"), Action::SeekShortBackward),
        (KeyCombo::plain("e"), Action::SeekShortForward),
        (KeyCombo::plain("O"), Action::SeekLongBackward),
        (KeyCombo::plain("E"), Action::SeekLongForward),
        (KeyCombo::plain("Left"), Action::SeekShortBackward),
        (KeyCombo::ctrl("Up"), Action::VolumeUp),
        (KeyCombo::ctrl("Down"), Action::VolumeDown),
        (KeyCombo::plain("plus"), Action::TogglePlaybackSpeed),
    ]
}

fn vocab_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("h"), Action::ShowSynopsisOverlay),
        (KeyCombo::plain("backslash"), Action::VocabPopupNext),
        (KeyCombo::plain("numbersign"), Action::VocabPopupPrev),
        (KeyCombo::plain("r"), Action::ConcordanceNext),
        (KeyCombo::plain("R"), Action::ConcordancePrev),
        (KeyCombo::ctrl("r"), Action::JumpToNextVocab),
        (KeyCombo::ctrl_shift("R"), Action::JumpToPrevVocab),
        (KeyCombo::alt("backslash"), Action::ToggleVocabHighlight),
        (KeyCombo::ctrl("g"), Action::ToggleGlossOverlay),
        (KeyCombo::plain("i"), Action::ShowTranslationOverlay),
        (KeyCombo::plain("apostrophe"), Action::ReopenEchoes),
        (KeyCombo::ctrl("backslash"), Action::OpenConcordancePicker),
        (KeyCombo::ctrl_shift("P"), Action::OpenConcordanceWordPicker),
        (KeyCombo::ctrl_alt("p"), Action::OpenConcordanceListPicker),
        (KeyCombo::alt("r"), Action::OpenConcordanceWorksPicker),
        (KeyCombo::alt("g"), Action::OpenGlossPicker),
        (KeyCombo::plain("H"), Action::ToggleVocabPopup),
    ]
}

fn display_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("exclam"), Action::AdjustFontSizeDown),
        (KeyCombo::plain("bar"), Action::AdjustFontSizeUp),
        (KeyCombo::plain("0"), Action::ResetFontSize),
        (KeyCombo::plain("f"), Action::CycleFontForward),
        (KeyCombo::plain("F"), Action::CycleFontBackward),
        (KeyCombo::plain("l"), Action::ToggleSignColumn),
        (KeyCombo::plain("minus"), Action::TogglePreviousWork),
        (KeyCombo::alt("d"), Action::ToggleDim),
        (KeyCombo::alt("bracketleft"), Action::ToggleColumnLayout),
        (KeyCombo::alt("t"), Action::ToggleTitleBar),
        (KeyCombo::ctrl("a"), Action::ToggleAuthorship),
        (KeyCombo::ctrl_shift("A"), Action::PickAttributionSet),
        (KeyCombo::alt("f"), Action::ShowFontInfo),
        (KeyCombo::alt("i"), Action::ToggleTranslations),
        (KeyCombo::alt("e"), Action::ShowEchoes),
        (KeyCombo::ctrl("h"), Action::ToggleSynopsis),
        (KeyCombo::ctrl("comma"), Action::OpenSettingsOverlay),
    ]
}

fn selection_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("V"), Action::EnterVisualMode),
        (KeyCombo::plain("w"), Action::WordCycleCopy),
        (KeyCombo::plain("W"), Action::WordCollectCopy),
    ]
}

fn timestamp_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("u"), Action::SetStartTime),
        (KeyCombo::plain("Right"), Action::SetStartTime),
        (KeyCombo::alt("u"), Action::SetEndTime),
        (KeyCombo::plain("c"), Action::SetChapter),
        (KeyCombo::plain("BackSpace"), Action::DeleteTimestamp),
        (KeyCombo::plain("p"), Action::NudgeStartBackward),
        (KeyCombo::plain("P"), Action::NudgeStartForward),
        (KeyCombo::plain("U"), Action::UndoTimestamp),
        (KeyCombo::plain("a"), Action::PlayCurrentLine),
    ]
}

fn app_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("Escape"), Action::EscapeReaderMode),
        (KeyCombo::ctrl("d"), Action::ToggleDebugLogging),
        (KeyCombo::ctrl_shift("T"), Action::ToggleNavTest),
        (KeyCombo::ctrl_shift("E"), Action::ShowEchoTurns),
        (KeyCombo::ctrl("p"), Action::OpenLibraryPicker),
        (KeyCombo::ctrl("minus"), Action::OpenRecentPicker),
        (KeyCombo::ctrl_shift("M"), Action::OpenMediaPicker),
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
        assert_eq!(m.get(&KeyCombo::ctrl("f")), Some(&Action::PageForward));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("M")), Some(&Action::OpenMediaPicker));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("L")), Some(&Action::SaveAndQuit));
        assert_eq!(m.get(&KeyCombo::ctrl("a")), Some(&Action::ToggleAuthorship));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("A")), Some(&Action::PickAttributionSet));
    }

    #[test]
    fn keymap_lookup_returns_action_for_bound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("f", true, false, false), Some(Action::PageForward));
    }

    #[test]
    fn alt_bracketleft_is_toggle_column_layout() {
        let km = Keymap::default();
        assert_eq!(
            km.lookup("bracketleft", false, false, true),
            Some(Action::ToggleColumnLayout),
        );
        assert_eq!(
            km.lookup("bracketleft", false, false, false),
            Some(Action::PrevBookmark),
        );
    }

    #[test]
    fn keymap_lookup_returns_none_for_unbound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("zzz", false, false, false), None);
    }

    #[test]
    fn keymap_lookup_distinguishes_modifiers() {
        let km = Keymap::default();
        // "f" is bound to CycleFontForward; Ctrl+f to PageForward.
        let f_plain = km.lookup("f", false, false, false);
        let f_ctrl = km.lookup("f", true, false, false);
        assert_ne!(f_plain, f_ctrl);
        assert_eq!(f_plain, Some(Action::CycleFontForward));
        assert_eq!(f_ctrl, Some(Action::PageForward));
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
