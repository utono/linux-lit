use std::sync::OnceLock;

use regex::Regex;

const PROSE_TYPES: &[&str] = &["novel", "essay_collection", "prose_book", "prose"];

fn speaker_simple_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z\s.\-']+\.?$").unwrap())
}

fn speaker_with_direction_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z\s\-']*,?\s*\[.*\]\.?$").unwrap())
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
    stage_direction_re().is_match(text.trim())
}

pub fn is_act_scene_marker(text: &str) -> bool {
    let upper = text.trim().to_uppercase();
    upper.starts_with("ACT ")
        || upper.starts_with("SCENE ")
        || upper.starts_with("PROLOGUE")
        || upper.starts_with("EPILOGUE")
}

pub fn is_separator(text: &str) -> bool {
    text.trim().starts_with('=')
}

pub fn is_dialogue(text: &str, is_prose: bool) -> bool {
    if is_blank(text) {
        return false;
    }
    if is_prose {
        return true;
    }
    !is_speaker(text)
        && !is_stage_direction(text)
        && !is_act_scene_marker(text)
        && !is_separator(text)
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
        assert!(!is_speaker("A"));
        assert!(!is_speaker("A."));
        assert!(!is_speaker("hamlet"));
        assert!(!is_speaker("Hamlet"));
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
        assert!(!is_stage_direction("[partial"));
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
        assert!(!is_act_scene_marker("Action"));
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
}
