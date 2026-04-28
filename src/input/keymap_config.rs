//! Keymap configuration: KeyCombo struct + Keymap loader.
//!
//! Loaded from ~/.config/linux-lit/keymap.json with compiled-in defaults.
//! Falls back to defaults on missing or malformed JSON. Mirrors lue's
//! load_keyboard_shortcuts pattern (lue/lue/input_handler.py:48-64).

use serde::{Deserialize, Serialize};

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
