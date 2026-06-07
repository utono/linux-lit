use std::sync::OnceLock;

use regex::Regex;

const PROSE_TYPES: &[&str] = &["novel", "essay_collection", "prose_book", "prose"];

fn speaker_simple_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z\s.\-'\u{2018}\u{2019}/]+\.?$").unwrap())
}

fn speaker_with_direction_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z\s\-'\u{2018}\u{2019}]*,?\s*\[.*\]\.?$").unwrap())
}

fn stage_direction_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[.*\]$").unwrap())
}

pub fn is_prose_work(work_type: &str) -> bool {
    PROSE_TYPES.contains(&work_type)
}

pub fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

pub fn is_speaker(text: &str) -> bool {
    let trimmed = text.trim();
    let stripped = trimmed.trim_end_matches('.');
    if stripped.len() < 2 {
        return false;
    }
    speaker_simple_re().is_match(trimmed) || speaker_with_direction_re().is_match(trimmed)
}

pub fn is_stage_direction(text: &str) -> bool {
    let trimmed = text.trim();
    if stage_direction_re().is_match(trimmed) {
        return true;
    }
    // Multi-line stage direction: opening line starts with [ but has no closing ]
    if trimmed.starts_with('[') && !trimmed.ends_with(']') {
        return true;
    }
    // Multi-line stage direction: closing line ends with ] but doesn't start with [
    if trimmed.ends_with(']') && !trimmed.starts_with('[') {
        return true;
    }
    false
}

fn is_standalone_keyword(upper: &str, keyword: &str) -> bool {
    upper == keyword
        || upper.starts_with(&format!("{},", keyword))
        || upper.starts_with(&format!("{}.", keyword))
}

pub fn is_act_scene_marker(text: &str) -> bool {
    let trimmed = text.trim();
    let stripped = trimmed.strip_prefix("## ").unwrap_or(trimmed);
    let upper = stripped.to_uppercase();
    upper.starts_with("ACT ")
        || upper.starts_with("SCENE ")
        || upper.starts_with("CHAPTER ")
        || is_standalone_keyword(&upper, "PROLOGUE")
        || is_standalone_keyword(&upper, "EPILOGUE")
        || is_standalone_keyword(&upper, "INDUCTION")
        || is_standalone_keyword(&upper, "CHORUS")
}

pub fn is_separator(text: &str) -> bool {
    text.trim().starts_with('=')
}

pub fn is_stanza_number(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit())
}

pub fn is_dialogue(text: &str, is_prose: bool) -> bool {
    if is_blank(text) {
        return false;
    }
    if is_separator(text) || is_act_scene_marker(text) {
        return false;
    }
    if is_prose {
        return true;
    }
    // A bare stanza/sonnet number ("1", "138") is a section heading, not spoken
    // verse — exclude it so the cursor and playback sync skip it and land on the
    // first line of verse (sonnet_sequence and other numbered verse).
    if is_stanza_number(text) {
        return false;
    }
    !is_speaker(text) && !is_stage_direction(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prose_types() {
        assert!(is_prose_work("novel"));
        assert!(is_prose_work("essay_collection"));
        assert!(is_prose_work("prose_book"));
        assert!(is_prose_work("prose"));
        assert!(!is_prose_work("play"));
        assert!(!is_prose_work("poem"));
    }

    #[test]
    fn test_blank() {
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(is_blank("\t\n"));
        assert!(!is_blank("text"));
    }

    #[test]
    fn test_speaker_simple() {
        assert!(is_speaker("HAMLET"));
        assert!(is_speaker("HAMLET."));
        assert!(is_speaker("FIRST GENTLEMAN"));
        assert!(is_speaker("FIRST GENTLEMAN."));
        assert!(is_speaker("KING HENRY"));
        // Folger texts use a typographic apostrophe (U+2019) in possessive
        // speaker names like SHEPHERD'S SON.
        assert!(is_speaker("SHEPHERD\u{2019}S SON"));
        assert!(is_speaker("SHEPHERD'S SON"));
        // Folger texts join two characters speaking in unison with a slash,
        // e.g. CORNELIUS/VOLTEMAND, TITINIUS/MESSALA.
        assert!(is_speaker("CORNELIUS/VOLTEMAND"));
        assert!(is_speaker("TITINIUS/MESSALA"));
        assert!(!is_speaker("A"));
        assert!(!is_speaker("A."));
        assert!(!is_speaker("hamlet"));
        assert!(!is_speaker("Hamlet"));
        // Dialogue lines that happen to be uppercase must not be treated as
        // speaker labels (e.g. Titus's cry in Titus Andronicus).
        assert!(!is_speaker("O, O, O!"));
    }

    #[test]
    fn test_speaker_with_direction() {
        assert!(is_speaker("LUCIANA, [to Adriana]"));
        assert!(is_speaker("PRINCE HENRY [aside]"));
    }

    #[test]
    fn test_stage_direction() {
        assert!(is_stage_direction("[Exit]"));
        assert!(is_stage_direction("[Exeunt all but HAMLET]"));
        assert!(!is_stage_direction("Not a direction"));
        // Multi-line: opening line starts with [ without closing ]
        assert!(is_stage_direction("[Enter the King of England, Humphrey Duke of"));
        // Multi-line: closing line ends with ] without opening [
        assert!(is_stage_direction("and Exeter, with other Attendants.]"));
    }

    #[test]
    fn test_act_scene_marker() {
        assert!(is_act_scene_marker("ACT 1"));
        assert!(is_act_scene_marker("SCENE 2"));
        assert!(is_act_scene_marker("Scene 3"));
        assert!(is_act_scene_marker("Act 1"));
        assert!(is_act_scene_marker("PROLOGUE"));
        assert!(is_act_scene_marker("Prologue"));
        assert!(is_act_scene_marker("EPILOGUE"));
        assert!(is_act_scene_marker("Epilogue"));
        // Chorus headings (Pericles' Gower, R&J Act 2) are act/scene markers,
        // not dialogue — so the cursor never lands on the heading word.
        assert!(is_act_scene_marker("Chorus"));
        assert!(is_act_scene_marker("CHORUS"));
        assert!(!is_act_scene_marker("Action"));
        // Dialogue containing marker keywords must not match
        assert!(!is_act_scene_marker(
            "Epilogue or to hear a Bergomask dance between"
        ));
        assert!(!is_act_scene_marker("Chorus of the spheres did sing"));
        assert!(!is_act_scene_marker("Prologue to the story begins here"));
        assert!(!is_act_scene_marker("Induction of the current was strong"));
        // New: ## headers from cleaned format
        assert!(is_act_scene_marker("## Act 1, Scene 1"));
        assert!(is_act_scene_marker("## Prologue"));
        assert!(is_act_scene_marker("## Epilogue"));
        assert!(is_act_scene_marker("## Induction"));
    }

    #[test]
    fn test_separator() {
        assert!(is_separator("===="));
        assert!(is_separator("= Chapter"));
        assert!(!is_separator("not a separator"));
    }

    #[test]
    fn test_dialogue_play() {
        assert!(is_dialogue("Who's there?", false));
        assert!(is_dialogue(
            "Nay, answer me. Stand and unfold yourself.",
            false
        ));
        assert!(!is_dialogue("HAMLET.", false));
        assert!(!is_dialogue("[Exit]", false));
        assert!(!is_dialogue("ACT 1", false));
        assert!(!is_dialogue("", false));
    }

    #[test]
    fn test_dialogue_prose() {
        assert!(is_dialogue("Any text at all.", true));
        assert!(is_dialogue("HAMLET.", true));
        assert!(is_dialogue("[Exit]", true));
        assert!(!is_dialogue("", true));
    }

    #[test]
    fn test_prose_separator_is_not_dialogue() {
        assert!(!is_dialogue("= Chapter One", true));
        assert!(!is_dialogue("========", true));
    }

    #[test]
    fn test_prose_act_scene_marker_is_not_dialogue() {
        assert!(!is_dialogue("ACT 1", true));
        assert!(!is_dialogue("## Act 3, Scene 2", true));
        assert!(!is_dialogue("PROLOGUE", true));
    }

    #[test]
    fn test_prose_normal_line_still_dialogue() {
        assert!(is_dialogue("It was the best of times.", true));
        assert!(is_dialogue("Mr. Jarndyce looked at us.", true));
    }

    #[test]
    fn test_prose_blank_still_not_dialogue() {
        assert!(!is_dialogue("", true));
        assert!(!is_dialogue("   ", true));
    }

    #[test]
    fn test_verse_stanza_number_is_not_dialogue() {
        // A bare sonnet/stanza number is a section heading, not spoken verse —
        // the cursor and sync skip it (verse mode only).
        assert!(!is_dialogue("1", false));
        assert!(!is_dialogue("138", false));
        assert!(!is_dialogue("  144  ", false));
        // Real verse is still dialogue.
        assert!(is_dialogue("From fairest creatures we desire increase,", false));
        // In prose a number line is still treated as content (prose returns
        // early before the stanza-number check).
        assert!(is_dialogue("1", true));
    }
}
