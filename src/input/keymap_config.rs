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

/// Compiled-in default reader bindings. Mirrors the inline match arms
/// currently in keymap.rs:1338-1741 (base-keys block) and the Ctrl+ /
/// Shift+ / Alt+ combo blocks.
pub fn default_reader_bindings() -> HashMap<KeyCombo, Action> {
    let mut m = HashMap::new();

    // Page navigation
    m.insert(KeyCombo::plain("x"), Action::PageForward);
    m.insert(KeyCombo::plain("y"), Action::PageBackward);
    m.insert(KeyCombo::plain("less"), Action::PageBackward);
    m.insert(KeyCombo::plain("space"), Action::PageForward);
    m.insert(KeyCombo::shift("space"), Action::PageBackward);
    m.insert(KeyCombo::ctrl("d"), Action::ToggleDebugLogging);
    m.insert(KeyCombo::ctrl("f"), Action::PageForward);
    m.insert(KeyCombo::ctrl("u"), Action::PageForward);
    m.insert(KeyCombo::ctrl("b"), Action::PageBackward);

    // Cursor / dialogue
    m.insert(KeyCombo::plain("j"), Action::CursorNextDialogue);
    m.insert(KeyCombo::plain("k"), Action::CursorPrevLine);
    m.insert(KeyCombo::plain("Q"), Action::CursorToPageBottom);
    m.insert(KeyCombo::plain("Up"), Action::JumpToPrevDialogue);
    m.insert(KeyCombo::shift("Up"), Action::PageBackwardBottom);
    m.insert(KeyCombo::plain("Down"), Action::JumpToNextDialogue);
    m.insert(KeyCombo::plain("comma"), Action::JumpToPrevDialogue);
    m.insert(KeyCombo::shift("comma"), Action::PageBackwardBottom);
    m.insert(KeyCombo::plain("q"), Action::JumpToNextDialogue);

    // Multi-key chord entry
    m.insert(KeyCombo::plain("g"), Action::PendingG);
    m.insert(KeyCombo::plain("G"), Action::JumpToEnd);

    // Chapter / scene
    m.insert(KeyCombo::plain("bracketleft"), Action::JumpToPrevChapter);
    m.insert(KeyCombo::plain("braceleft"), Action::JumpToNextChapter);
    m.insert(KeyCombo::plain("2"), Action::JumpToPrevScene);
    m.insert(KeyCombo::plain("3"), Action::JumpToNextScene);

    // Bookmarks
    m.insert(KeyCombo::plain("m"), Action::ToggleBookmark);
    m.insert(KeyCombo::plain("semicolon"), Action::NextBookmark);
    m.insert(KeyCombo::shift("semicolon"), Action::PrevBookmark);
    m.insert(KeyCombo::plain("colon"), Action::PrevBookmark);
    m.insert(KeyCombo::ctrl("m"), Action::OpenBookmarkPicker);

    // Pickers
    m.insert(KeyCombo::ctrl("p"), Action::OpenLibraryPicker);
    m.insert(KeyCombo::ctrl_shift("M"), Action::OpenMediaPicker);
    m.insert(KeyCombo::ctrl("backslash"), Action::OpenConcordancePicker);
    m.insert(KeyCombo::ctrl_shift("P"), Action::OpenConcordanceWordPicker);
    m.insert(KeyCombo::ctrl_alt("p"), Action::OpenConcordanceListPicker);
    m.insert(KeyCombo::ctrl("comma"), Action::OpenSettingsOverlay);
    m.insert(KeyCombo::ctrl("slash"), Action::OpenKeybindsOverlay);
    m.insert(KeyCombo::plain("slash"), Action::OpenSearch);

    // MPV / media
    m.insert(KeyCombo::plain("s"), Action::TogglePlaybackSync);
    m.insert(KeyCombo::plain("Tab"), Action::TogglePlayback);
    m.insert(KeyCombo::plain("o"), Action::SeekShortBackward);
    m.insert(KeyCombo::plain("e"), Action::SeekShortForward);
    m.insert(KeyCombo::plain("O"), Action::SeekLongBackward);
    m.insert(KeyCombo::plain("E"), Action::SeekLongForward);
    m.insert(KeyCombo::plain("Left"), Action::SeekBackward30);
    m.insert(KeyCombo::ctrl("Up"), Action::VolumeUp);
    m.insert(KeyCombo::ctrl("Down"), Action::VolumeDown);
    m.insert(KeyCombo::plain("plus"), Action::TogglePlaybackSpeed);

    // Vocab / glossing
    m.insert(KeyCombo::plain("h"), Action::ToggleVocabPopup);
    m.insert(KeyCombo::plain("backslash"), Action::VocabPopupNext);
    m.insert(KeyCombo::plain("numbersign"), Action::VocabPopupPrev);
    m.insert(KeyCombo::plain("r"), Action::JumpToNextVocab);
    m.insert(KeyCombo::plain("R"), Action::JumpToPrevVocab);
    m.insert(KeyCombo::alt("backslash"), Action::ToggleVocabHighlight);

    // Visual / selection
    m.insert(KeyCombo::plain("V"), Action::EnterVisualMode);
    m.insert(KeyCombo::plain("w"), Action::WordCycleCopy);
    m.insert(KeyCombo::plain("W"), Action::WordCollectCopy);

    // Translations
    m.insert(KeyCombo::plain("i"), Action::ToggleTranslations);

    // Settings (in reader)
    m.insert(KeyCombo::plain("exclam"), Action::AdjustFontSizeDown);
    m.insert(KeyCombo::plain("bar"), Action::AdjustFontSizeUp);
    m.insert(KeyCombo::plain("0"), Action::ResetFontSize);
    m.insert(KeyCombo::plain("f"), Action::CycleFontForward);
    m.insert(KeyCombo::plain("F"), Action::CycleFontBackward);
    m.insert(KeyCombo::plain("l"), Action::ToggleSignColumn);
    m.insert(KeyCombo::plain("minus"), Action::ToggleCursorLine);
    m.insert(KeyCombo::alt("d"), Action::ToggleDim);
    m.insert(KeyCombo::alt("f"), Action::ShowFontInfo);

    // Timestamps
    m.insert(KeyCombo::plain("u"), Action::SetStartTime);
    m.insert(KeyCombo::plain("Right"), Action::SetStartTime);
    m.insert(KeyCombo::alt("i"), Action::SetEndTime);
    m.insert(KeyCombo::plain("period"), Action::SetChapter);
    m.insert(KeyCombo::plain("BackSpace"), Action::DeleteTimestamp);
    m.insert(KeyCombo::plain("p"), Action::NudgeStartBackward);
    m.insert(KeyCombo::plain("P"), Action::NudgeStartForward);
    m.insert(KeyCombo::plain("U"), Action::UndoTimestamp);
    m.insert(KeyCombo::plain("a"), Action::PlayCurrentLine);

    // App
    m.insert(KeyCombo::ctrl_alt("l"), Action::SaveAndQuit);
    m.insert(KeyCombo::ctrl("y"), Action::CopyLineMappingId);

    // Search (in reader)
    m.insert(KeyCombo::plain("n"), Action::SearchNextMatch);
    m.insert(KeyCombo::plain("N"), Action::SearchPrevMatch);

    m
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
        assert_eq!(m.get(&KeyCombo::ctrl_alt("l")), Some(&Action::SaveAndQuit));
    }

    #[test]
    fn keymap_lookup_returns_action_for_bound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("f", true, false, false), Some(Action::PageForward));
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
                {"key": "z", "action": "ThisActionDoesNotExist"}
            ]
        }"#;
        let km = Keymap::from_json_str(json);
        // Override succeeded for known action:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageBackward));
        // Unknown action skipped silently:
        assert_eq!(km.lookup("z", false, false, false), None);
    }
}
