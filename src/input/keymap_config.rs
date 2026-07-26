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
    #[allow(dead_code)] // completes the modifier-combo constructor family
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
        // space / Shift+space were PageForward/PageBackward; space is now a
        // global play/pause toggle handled directly in handle_key.
        // Bookmark steps live on j / k (swapped with `'`/`;`, which carry
        // the seeking cursor steps below). The speaker JUMPS j / k once
        // duplicated stay on q / comma (and the shifted J / K forms below).
        // h / t are the cursor-only NO-SEEK dialogue twins.
        (KeyCombo::plain("j"), Action::NextBookmark),
        (KeyCombo::plain("k"), Action::PrevBookmark),
        (KeyCombo::plain("Q"), Action::JumpToNextDialogue),
        (KeyCombo::plain("h"), Action::CursorNextDialogueNoSeek),
        (KeyCombo::plain("t"), Action::CursorPrevDialogueNoSeek),
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
        // `}`/`]` (braceright/bracketright) chapter jumps were dropped. The
        // former number-row duplicates (`4`/`5`, `2`/`3`, shifted forms) and
        // the AE04/AE05 symbol binds (`(`/`&`) were dropped as redundant; bookmarks
        // moved fully to the `;`/`'` home-region pair below.
        (KeyCombo::plain("bracketleft"), Action::JumpToPrevScene),
        // Ctrl+[ sets an audio track/chapter mark (moved off Ctrl+c, which now
        // toggles the previous work).
        (KeyCombo::ctrl("bracketleft"), Action::SetChapter),
        (KeyCombo::plain("braceleft"), Action::JumpToNextScene),
        (KeyCombo::plain("C"), Action::ShowCurrentChapter),
        // Shift+; emits ("colon", shift=true) on this layout (same class as
        // Shift+, → "less") — toggle playback speed. The bare-name form is
        // bound too in case the compositor reports ("semicolon", shift=true)
        // instead.
        (KeyCombo::shift("colon"), Action::TogglePlaybackSpeed),
        (KeyCombo::shift("semicolon"), Action::TogglePlaybackSpeed),
        // `;`/`'` carry the seeking cursor steps (prev line / next dialogue),
        // swapped with `k`/`j` (which took the bookmark steps above).
        // `}`/`]` (braceright/bracketright) stay unbound.
        (KeyCombo::plain("semicolon"), Action::CursorPrevLine),
        (KeyCombo::plain("apostrophe"), Action::CursorNextDialogue),
        (KeyCombo::plain("m"), Action::ToggleBookmark),
        // `.` is overloaded (Action::BookmarkTap): single tap toggles the
        // bookmark; .. reverts the toggle and opens the picker.
        (KeyCombo::plain("period"), Action::BookmarkTap),
        // Ctrl+c toggles between the current work and the previous one, restoring
        // each work's exact cursor line + its MPV media (A<->B). SetChapter moved
        // to Ctrl+[ to free this cap.
        (KeyCombo::ctrl("c"), Action::TogglePreviousWork),
        (KeyCombo::ctrl("e"), Action::ShowEchoesBcp),
        (KeyCombo::ctrl("period"), Action::OpenBookmarkPicker),
    ]
}

fn media_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        // Space plays from the cursor line's start timestamp (PlayCurrentLine;
        // it is intercepted before dispatch in keymap.rs). `a` is a PURE
        // pause/resume toggle — no seek (TogglePause). This holds for ALL work
        // types; poetry/plays no longer swap the two (they match prose). The
        // journal/gloss/translation overlays mirror this `a` (see keymap.rs).
        (KeyCombo::plain("a"), Action::TogglePause),
        // `s` toggles playback sync directly (was `@`/`at`; `s` was TogglePause).
        (KeyCombo::plain("s"), Action::TogglePlaybackSync),
        // '-' is unbound (vocab popup cycling moved to `r`; Ctrl+- enters
        // the vocab-sentence loop, InputMode::VocabLoop — no jump fallback).
        (KeyCombo::plain("o"), Action::SeekShortBackward),
        (KeyCombo::plain("e"), Action::SeekShortForward),
        (KeyCombo::plain("O"), Action::SeekLongBackward),
        (KeyCombo::plain("E"), Action::SeekLongForward),
        (KeyCombo::plain("Left"), Action::SeekShortBackward),
        (KeyCombo::ctrl("Up"), Action::VolumeUp),
        (KeyCombo::ctrl("Down"), Action::VolumeDown),
        // `+` copies "abbrev div1.div2" to the clipboard (CopyWorkDivision,
        // 2026-07-22 — was ShowCurrentChapter, which stays on `C`).
        (KeyCombo::plain("plus"), Action::CopyWorkDivision),
        (KeyCombo::alt("p"), Action::TogglePhraseHighlight),
        // Alt+, is the unshifted comma cap (<AD02>) + alt → ("comma", alt=true),
        // NOT ("less", ...) which is the shifted glyph. Jumps to prev dialogue.
        (KeyCombo::alt("comma"), Action::JumpToPrevDialogue),
    ]
}

fn vocab_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::ctrl("h"), Action::ShowSynopsisOverlay),
        // `r` is overloaded (Action::VocabPopupTap): a single tap cycles the
        // visible popup's words; rr in quick succession toggles visibility
        // (ChordState::PendingR). HideVocabPopup is unbound — rr covers it.
        // VocabPopupNext is unbound (tap covers the cycle). ConcordanceNext
        // AND ConcordancePrev are deliberately unbound (step in-work hits
        // with n/N while a concordance is active).
        (KeyCombo::plain("r"), Action::VocabPopupTap),
        // The `r` key is the vocab hub: plain = tap the popup word, Ctrl =
        // add a vocab word, Ctrl+Shift = vocab journal Q&A, Alt = toggle the
        // per-work vocab highlight. AddVocabWord moved here off Ctrl+Alt+\ and
        // ToggleVocabHighlight off Alt+\ (both freed) so every vocab function
        // lives on one cap (2026-07-23 consolidation).
        (KeyCombo::ctrl("r"), Action::AddVocabWord),
        // Ctrl+Shift+r: vocab journal Q&A — ask about the popup's current word
        // (gated on popup visible + a vocab word on the cursor line). Stored
        // answer → journal overlay; fresh ask → held toast, then the overlay
        // on the saved entry. RPD emits this as lowercase "r"+shift, so bind
        // both cases (mirrors OpenLastGloss below).
        (KeyCombo::ctrl_shift("r"), Action::VocabJournalAsk),
        (KeyCombo::ctrl_shift("R"), Action::VocabJournalAsk),
        (KeyCombo::alt("r"), Action::ToggleVocabHighlight),
        // Word-copy family lives on the `-` (minus) cap — see app_bindings.
        // Shift+r / Ctrl+Alt+r are unbound here (word-copy moved off them
        // 2026-07-23 when the chat panel binds were disabled and `-` freed).
        // Ctrl+Shift+g: on RPD this physical key emits key_name "g" (lowercase)
        // with shift=true under Ctrl+Shift — NOT "G" — so the "G" bind alone
        // never matched and the reader scrolled instead (confirmed from the
        // KEY: log: `name=g ctrl=true shift=true`). Bind the emitted lowercase
        // combo; keep "G" for layouts that DO capitalize (e.g. Ctrl+Shift+L
        // logs as "L"). Both compiled here + in the stowed keymap.json.
        (KeyCombo::ctrl_shift("g"), Action::OpenLastGloss),
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
        // Ctrl+Alt+g: toggle the reader-gloss/journal segment tint on the
        // main card (rounds out the g gloss family).
        (KeyCombo::ctrl_alt("g"), Action::ToggleAnnotationTint),
        // Ctrl+f: cross-corpus journal/gloss regex search popup. Also wired
        // directly (bypassing this table) from the journal/gloss overlay key
        // handlers, which short-circuit before reaching keymap.lookup.
        (KeyCombo::ctrl("f"), Action::OpenCorpusSearch),
        // `j` = journal (2026-07-23 reshuffle): both journal pickers live on the
        // j cap. Ctrl+j = work-wide journal Q&A picker (was Alt+j); Alt+j =
        // cross-work recent-Q&A jump-back (was Ctrl+a). ToggleJournalOverlay
        // (formerly Ctrl+j) is dropped — the `\` overlay cycle opens the journal.
        (KeyCombo::ctrl("j"), Action::OpenJournalPicker),
        (KeyCombo::alt("j"), Action::OpenRecentQaPicker),
        // Ctrl+o (ToggleLastOverlay: reopen the last-closed gloss/journal
        // overlay) was dropped 2026-07-22 — the action remains reachable
        // only via a keymap.json override.
        // Chat panel disabled 2026-07-23 — the `-` cap now carries word-copy
        // (see app_bindings), not ReaderGlossChatAtCursor. Its own in-panel
        // handlers (`-`/Ctrl+Tab/Escape) remain compiled but unreachable.
        // `\` cycles the segment overlays: journal Q&A → gloss → synopsis
        // (wraps; advance arms live in the overlay modal handlers).
        (KeyCombo::plain("backslash"), Action::CycleSegmentOverlays),
    ]
}

fn display_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::ctrl("exclam"), Action::AdjustFontSizeDown),
        (KeyCombo::ctrl("bar"), Action::AdjustFontSizeUp),
        (KeyCombo::plain("0"), Action::ResetFontSize),
        // `f`: open the cross-work journal term filter (tag/term Q&A search),
        // matching the journal overlay's `f`. Font cycling used to live here.
        (KeyCombo::plain("f"), Action::OpenJournalTermInput),
        (KeyCombo::plain("l"), Action::ToggleSignColumn),
        // Chat panel disabled 2026-07-23 (Ctrl+l ChatPanelFlipSide commented out
        // below; `-` freed for word-copy in app_bindings).
        (KeyCombo::alt("d"), Action::ToggleDim),
        (KeyCombo::ctrl("t"), Action::ThemeNext),
        (KeyCombo::ctrl_shift("T"), Action::ThemePrev),
        // Root-variant cycling lives on the RPD <TLDE> cap (QWERTY `/~ key),
        // which emits `dollar` unshifted and `asciitilde` shifted. Because $
        // is level-1 (no shift needed), Ctrl+$ and Ctrl+Shift+$ are distinct
        // chords — same forward/back split the t cap had.
        (KeyCombo::ctrl("dollar"), Action::RootVariantNext),
        (KeyCombo::ctrl_shift("dollar"), Action::RootVariantPrev),
        // Ctrl+Alt+$ mirrors Ctrl+Shift+$ (previous root variant) — a
        // shift-free reverse chord on the same cap.
        (KeyCombo::ctrl_alt("dollar"), Action::RootVariantPrev),
        // `b` sets the start time (plain `u`/`i` no longer do — `i` opens the
        // translation overlay). The old modifier families stay put
        // (Shift+U undo ts, Alt+u scansion; Ctrl+i image, Alt+i end ts).
        (KeyCombo::plain("b"), Action::SetStartTime),
        (KeyCombo::alt("bracketleft"), Action::ToggleColumnLayout),
        // Authorship moved off Ctrl+a (now CloseChatLayout). plain("A") is the
        // shifted `a` (cf. plain("G") normalization above).
        (KeyCombo::plain("A"), Action::ToggleAuthorship),
        (KeyCombo::ctrl_shift("A"), Action::PickAttributionSet),
        (KeyCombo::alt("f"), Action::ShowFontInfo),
        (KeyCombo::ctrl_alt("t"), Action::ShowThemeInfo),
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
        // Disabled 2026-07-23 with the rest of the chat panel binds; restore
        // this line (and plain `-` = ReaderGlossChatAtCursor in app_bindings)
        // to re-enable the chat panel.
        // (KeyCombo::ctrl("l"), Action::ChatPanelFlipSide),
        (KeyCombo::ctrl("comma"), Action::OpenSettingsOverlay),
    ]
}

fn selection_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("V"), Action::EnterVisualMode),
        // Action::AskPassage (paragraph/speech pre-select -> Journal Q&A ask
        // card) is deliberately UNBOUND — it used to sit on Ctrl+a. The action
        // and its code path (visual::enter_visual_block_mode, the pending_ask
        // fast-path) are intact, so re-adding a bind here (or a keymap.json
        // entry) restores it. Journal Q&A is still reachable without it: select
        // with `V`, then Ctrl+a in visual mode / the Action menu.
        // Copy-only vim view of the cursor's segment: opens in VISUAL mode,
        // visual `y` copies to the system clipboard, nothing is ever saved.
        (KeyCombo::plain("v"), Action::OpenSegmentVim),
        // Word-copy moved off the `w` cap to the `-` (minus) cap (2026-07-23):
        // plain w / Shift+W are now unbound here; WordCycleCopy is on plain `-`
        // and WordCollectCopy on `_` (Shift+-) — see app_bindings. The w cap
        // keeps only the Shx echo chords (Alt+w / Ctrl+w / Ctrl+Shift+W).
    ]
}

fn timestamp_bindings() -> Vec<(KeyCombo, Action)> {
    vec![
        (KeyCombo::plain("i"), Action::ShowTranslationOverlay),
        (KeyCombo::plain("Right"), Action::SetStartTime),
        // Alt+b sets the end time (moved off Alt+i 2026-07-22, pairing with
        // plain `b` = SetStartTime on the same cap).
        (KeyCombo::alt("b"), Action::SetEndTime),
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
        (KeyCombo::ctrl_alt("d"), Action::ToggleDebugLogging),
        // Moved off Ctrl+Alt+t (2026-07-12); Ctrl+t/Ctrl+Shift+t are theme cycling.
        (KeyCombo::ctrl_alt("n"), Action::ToggleNavTest),
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
        // Word-copy family on the `-` cap (2026-07-23; chat panel disabled).
        // Plain `-` cycles one word at a time to the clipboard; `_` (Shift+-)
        // collects the whole line. CONFIRMED FROM THE LOG: GTK delivers `_` as
        // ("underscore", shift=TRUE) — not shift=false — so shift("underscore")
        // is the bind that actually fires. `_` is a symbol, so lookup() keeps
        // the shift flag significant (effective_shift only strips shift for bare
        // uppercase letters), which is why plain("underscore") never matched.
        // shift("minus") is kept defensively for a layout path that reports the
        // shifted cap as ("minus", shift=true) instead.
        //
        // Chat panel disabled 2026-07-23: plain `-` previously opened/closed the
        // reader-gloss chat panel (ReaderGlossChatAtCursor). Restore that bind
        // (and Ctrl+l ChatPanelFlipSide in display_bindings) to re-enable it.
        (KeyCombo::plain("minus"), Action::WordCycleCopy),
        // Return diagrams the sentence containing the `-`/`_` underlined
        // words. Reader mode binds no Return today, so this is additive; the
        // dispatch arm no-ops when nothing is underlined.
        (KeyCombo::plain("Return"), Action::OpenSyntaxDiagramForUnderlined),
        (KeyCombo::shift("underscore"), Action::WordCollectCopy),
        (KeyCombo::shift("minus"), Action::WordCollectCopy),
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
        assert_eq!(m.get(&KeyCombo::plain("j")), Some(&Action::NextBookmark));
        assert_eq!(m.get(&KeyCombo::plain("k")), Some(&Action::PrevBookmark));
        assert_eq!(m.get(&KeyCombo::plain("y")), Some(&Action::PageBackward));
        assert_eq!(m.get(&KeyCombo::ctrl("m")), Some(&Action::OpenMediaPicker));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("L")), Some(&Action::SaveAndQuit));
        // The reader table binds no Tab caps (chat panel disabled 2026-07-23).
        assert_eq!(m.get(&KeyCombo::ctrl("Tab")), None);
        assert_eq!(m.get(&KeyCombo::plain("Tab")), None);
        // Ctrl+o (ToggleLastOverlay) dropped from the defaults.
        assert_eq!(m.get(&KeyCombo::ctrl("o")), None);
        // `j` = journal (2026-07-23 reshuffle): Ctrl+j = work-wide journal Q&A
        // picker, Alt+j = cross-work recent-Q&A jump-back. Ctrl+a is now unbound
        // and ToggleJournalOverlay (formerly Ctrl+j) has no reader bind.
        assert_eq!(m.get(&KeyCombo::ctrl("j")), Some(&Action::OpenJournalPicker));
        assert_eq!(m.get(&KeyCombo::alt("j")), Some(&Action::OpenRecentQaPicker));
        assert_eq!(m.get(&KeyCombo::ctrl("a")), None);
        assert_eq!(m.get(&KeyCombo::plain("A")), Some(&Action::ToggleAuthorship));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("A")), Some(&Action::PickAttributionSet));
        assert_eq!(m.get(&KeyCombo::plain("b")), Some(&Action::SetStartTime));
        assert_eq!(m.get(&KeyCombo::plain("i")), Some(&Action::ShowTranslationOverlay));
        assert_eq!(m.get(&KeyCombo::alt("u")), Some(&Action::CycleScansion));
        assert_eq!(m.get(&KeyCombo::alt("i")), None);
        assert_eq!(m.get(&KeyCombo::alt("b")), Some(&Action::SetEndTime));
        assert_eq!(m.get(&KeyCombo::ctrl_alt("i")), Some(&Action::ToggleTranslations));
        assert_eq!(m.get(&KeyCombo::ctrl("i")), Some(&Action::ToggleImageView));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("I")), Some(&Action::EnterPageCalibration));
    }

    #[test]
    fn r_is_the_vocab_hub() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("r")), Some(&Action::VocabPopupTap));
        // Chat panel disabled 2026-07-23: plain `-` no longer opens the
        // reader-gloss chat. The `-` cap now carries the word-copy family —
        // plain `-` = WordCycleCopy, `_` (Shift+-) = WordCollectCopy (bound
        // both delivery forms). The `r` key stays the vocab hub: Ctrl+r adds a
        // vocab word, Ctrl+Shift+r asks the vocab journal Q&A, Alt+r toggles the
        // per-work vocab highlight. Shift+r / Ctrl+Alt+r are now unbound.
        assert_eq!(
            m.get(&KeyCombo::plain("minus")),
            Some(&Action::WordCycleCopy)
        );
        // `_` arrives as ("underscore", shift=true) — shift("underscore") is
        // the bind that fires (confirmed from the debug log); shift("minus") is
        // a defensive alternate. plain("underscore") never matches.
        assert_eq!(
            m.get(&KeyCombo::shift("underscore")),
            Some(&Action::WordCollectCopy)
        );
        assert_eq!(
            m.get(&KeyCombo::shift("minus")),
            Some(&Action::WordCollectCopy)
        );
        assert_eq!(m.get(&KeyCombo::plain("underscore")), None);
        assert_eq!(m.get(&KeyCombo::plain("numbersign")), None);
        assert_eq!(m.get(&KeyCombo::ctrl("r")), Some(&Action::AddVocabWord));
        // Shift+r / Ctrl+Alt+r are unbound (word-copy moved to the `-` cap).
        assert_eq!(m.get(&KeyCombo::plain("R")), None);
        assert_eq!(m.get(&KeyCombo::ctrl_alt("r")), None);
        assert_eq!(m.get(&KeyCombo::ctrl_shift("r")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("R")), Some(&Action::VocabJournalAsk));
        assert_eq!(m.get(&KeyCombo::alt("r")), Some(&Action::ToggleVocabHighlight));
        // Reader Ctrl+n/p unbound since the popup Journal view was removed.
        assert_eq!(m.get(&KeyCombo::ctrl("n")), None);
        assert_eq!(m.get(&KeyCombo::ctrl("p")), None);
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
    fn ctrl_l_chat_flip_disabled() {
        let m = default_reader_bindings();
        // Ctrl+l (ChatPanelFlipSide) disabled 2026-07-23 with the chat panel.
        assert_eq!(m.get(&KeyCombo::ctrl("l")), None);
        assert_eq!(m.get(&KeyCombo::plain("l")), Some(&Action::ToggleSignColumn));
    }

    #[test]
    fn speaker_turn_keys_bound_to_capital_j_and_k() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("J")), Some(&Action::JumpToNextSpeaker));
        assert_eq!(m.get(&KeyCombo::plain("K")), Some(&Action::JumpToPrevSpeaker));
        // Lowercase j / k carry the bookmark steps (swapped with `'`/`;`);
        // the speaker jumps stay on q / comma and the capitals above.
        assert_eq!(m.get(&KeyCombo::plain("j")), Some(&Action::NextBookmark));
        assert_eq!(m.get(&KeyCombo::plain("k")), Some(&Action::PrevBookmark));
        assert_eq!(m.get(&KeyCombo::plain("h")), Some(&Action::CursorNextDialogueNoSeek));
        assert_eq!(m.get(&KeyCombo::plain("t")), Some(&Action::CursorPrevDialogueNoSeek));
    }

    #[test]
    fn shift_j_resolves_to_next_speaker_via_lookup() {
        let km = Keymap::default();
        // GTK delivers Shift+j as key "J" with shift=true; is_uppercase_letter
        // strips the redundant shift, so plain("J") matches.
        assert_eq!(km.lookup("J", false, true, false), Some(Action::JumpToNextSpeaker));
        assert_eq!(km.lookup("K", false, true, false), Some(Action::JumpToPrevSpeaker));
        // Bare , / q are the speaker jumps; Shift+q emits "Q" (uppercase,
        // shift stripped) for the next-dialogue jump.
        assert_eq!(km.lookup("comma", false, false, false), Some(Action::JumpToPrevSpeaker));
        assert_eq!(km.lookup("q", false, false, false), Some(Action::JumpToNextSpeaker));
        assert_eq!(km.lookup("Q", false, true, false), Some(Action::JumpToNextDialogue));
        // Ctrl+, opens the settings overlay (mirrors the per-overlay handlers).
        assert_eq!(km.lookup("comma", true, false, false), Some(Action::OpenSettingsOverlay));
        // Alt+, (unshifted comma cap + alt) jumps to the previous dialogue.
        assert_eq!(km.lookup("comma", false, false, true), Some(Action::JumpToPrevDialogue));
    }

    #[test]
    fn keymap_lookup_returns_action_for_bound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("y", false, false, false), Some(Action::PageBackward));
    }

    #[test]
    fn ctrl_shift_g_resolves_open_last_gloss_both_cases() {
        // RPD emits Ctrl+Shift+g as key_name "g" (lowercase) + shift=true (NOT
        // "G") — confirmed from the KEY: log. The lookup must resolve the
        // emitted lowercase combo; the "G" case is kept for layouts that
        // capitalize. (ctrl is set, so lookup does NOT strip shift.)
        let km = Keymap::default();
        assert_eq!(km.lookup("g", true, true, false), Some(Action::OpenLastGloss));
        assert_eq!(km.lookup("G", true, true, false), Some(Action::OpenLastGloss));
        // Ctrl+g (no shift) stays the overlay toggle, unaffected.
        assert_eq!(km.lookup("g", true, false, false), Some(Action::ToggleGlossOverlay));
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
        // The number-row (`2`/`3`/`4`/`5`) and `(`/`&` duplicates were dropped.
        assert_eq!(km.lookup("2", false, true, false), None);
        assert_eq!(km.lookup("3", false, true, false), None);
        assert_eq!(km.lookup("4", false, false, false), None);
        assert_eq!(km.lookup("5", false, false, false), None);
        assert_eq!(km.lookup("parenleft", false, false, false), None);
        assert_eq!(km.lookup("ampersand", false, false, false), None);
        // `;`/`'` carry the seeking cursor steps; `}`/`]` are unbound.
        assert_eq!(km.lookup("braceright", false, false, false), None);
        assert_eq!(km.lookup("bracketright", false, false, false), None);
        assert_eq!(km.lookup("semicolon", false, false, false), Some(Action::CursorPrevLine));
        assert_eq!(km.lookup("apostrophe", false, false, false), Some(Action::CursorNextDialogue));
        assert_eq!(km.lookup("braceleft", false, false, false), Some(Action::JumpToNextScene));
        // Shift+; (the shifted colon glyph) cycles playback speed; `+` copies
        // the work + division to the clipboard (was ShowCurrentChapter, which
        // stays on `C`).
        assert_eq!(km.lookup("colon", false, true, false), Some(Action::TogglePlaybackSpeed));
        assert_eq!(km.lookup("plus", false, false, false), Some(Action::CopyWorkDivision));
        // Shift+'+' (the shifted `1` glyph on RPD <AE01>) copies the work
        // abbrev + media path + large whisperX JSON; both delivery forms.
        assert_eq!(km.lookup("1", false, true, false), Some(Action::CopyWorkInfo));
        assert_eq!(km.lookup("1", false, false, false), Some(Action::CopyWorkInfo));
    }

    #[test]
    fn ctrl_t_cycles_theme() {
        let km = Keymap::default();
        // Ctrl+t = next theme, Ctrl+Shift+t = prev theme (moved off Alt 2026-07-12).
        assert_eq!(km.lookup("t", true, false, false), Some(Action::ThemeNext));
        assert_eq!(km.lookup("T", true, true, false), Some(Action::ThemePrev));
    }

    #[test]
    fn ctrl_dollar_cycles_root_variant() {
        let km = Keymap::default();
        assert_eq!(km.lookup("dollar", true, false, false), Some(Action::RootVariantNext));
        assert_eq!(km.lookup("dollar", true, true, false), Some(Action::RootVariantPrev));
        assert_eq!(km.lookup("dollar", true, false, true), Some(Action::RootVariantPrev));
        assert_eq!(km.lookup("n", true, false, true), Some(Action::ToggleNavTest));
    }

    #[test]
    fn keymap_lookup_returns_none_for_unbound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("zzz", false, false, false), None);
    }

    #[test]
    fn keymap_lookup_distinguishes_modifiers() {
        let km = Keymap::default();
        // The `a` cap: plain = TogglePause; Ctrl+a is now UNBOUND (its recent-Q&A
        // picker moved to Alt+j in the 2026-07-23 reshuffle), Ctrl+Shift+a is the
        // attribution set. (Both Tab caps are unbound — the chat panel uses `-`
        // and its own handlers.)
        assert_eq!(km.lookup("a", true, false, false), None);
        assert_eq!(km.lookup("A", true, true, false), Some(Action::PickAttributionSet));
        // The `j` cap resolves its two modifiers to the two journal pickers:
        // Ctrl+j = work-wide journal Q&A picker, Alt+j = recent-Q&A jump-back.
        let j_ctrl = km.lookup("j", true, false, false);
        let j_alt = km.lookup("j", false, false, true);
        assert_ne!(j_ctrl, j_alt);
        assert_eq!(j_ctrl, Some(Action::OpenJournalPicker));
        assert_eq!(j_alt, Some(Action::OpenRecentQaPicker));
        assert_eq!(km.lookup("Tab", false, false, false), None);
        assert_eq!(km.lookup("Tab", true, false, false), None);
        // Plain f opens the journal term filter (matching the journal overlay);
        // Shift+F is unbound (font cycling removed); Ctrl+f is the corpus search.
        assert_eq!(km.lookup("f", false, false, false), Some(Action::OpenJournalTermInput));
        assert_eq!(km.lookup("F", false, false, false), None);
        assert_eq!(km.lookup("f", true, false, false), Some(Action::OpenCorpusSearch));
        assert_eq!(km.lookup("o", true, false, false), None);
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
        assert_eq!(km.lookup("j", false, false, false), Some(Action::NextBookmark));
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
