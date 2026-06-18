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

/// A Book of Common Prayer work, identified by its abbrev prefix. Mirrors the
/// `LIKE 'BCP%'` echo-channel rule (src/db/echo_channel.rs) and the inline
/// `abbrev.starts_with("BCP")` test in src/input/actions/echoes.rs.
pub fn is_bcp_work(abbrev: &str) -> bool {
    abbrev.starts_with("BCP")
}

/// A BCP heading line, carrying the `## ` marker from extract_blocks. Kept
/// distinct from `is_act_scene_marker` so BCP headings get centered liturgical
/// styling rather than the play act/scene treatment.
pub fn is_bcp_heading(text: &str) -> bool {
    text.trim().starts_with("## ")
}

/// A top-level BCP rite title: a `## ` heading whose text is all-caps (e.g.
/// "## THE SUPPER"). Distinguished from mixed-case sub-headings so only rite
/// titles get ornamental flourishes.
pub fn is_bcp_rite_title(text: &str) -> bool {
    let Some(rest) = text.trim().strip_prefix("## ") else { return false };
    let rest = rest.trim();
    !rest.is_empty()
        && rest.chars().any(|c| c.is_alphabetic())
        && rest.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
}

/// A BCP rubric (stage direction / instruction), wrapped in `[ ]` by
/// extract_blocks. Distinct from `is_stage_direction` (which also matches
/// multi-line bracket fragments) because BCP rubrics are whole-line `[...]`.
pub fn is_rubric(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('[') && t.ends_with(']') && t.len() >= 2
}

/// Max words for a rubric to be treated as a short centered cue rather than a
/// hanging-indent instructional paragraph. Tunable; 8 fits the Oxford text.
const RUBRIC_CENTER_MAX_WORDS: usize = 8;

/// Decide a rubric's layout. Pass the rubric's INNER text (no surrounding
/// brackets). Short cues with no sentence-internal period ("The Priest.",
/// "Then likewise he shall say.") center; longer instructional prose hangs.
/// Display heuristic only — a wrong call misplaces alignment, never text.
pub fn rubric_is_centered(inner: &str) -> bool {
    let t = inner.trim().trim_start_matches('¶').trim();
    let words = t.split_whitespace().count();
    if words == 0 || words > RUBRIC_CENTER_MAX_WORDS {
        return false;
    }
    // A period anywhere but the very end signals multi-sentence instruction.
    let trimmed_end = t.trim_end_matches('.');
    !trimmed_end.contains('.')
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

/// True when `text` is a plain title line sitting directly above a `=====`
/// separator — the anthology header form (`Sonnet 116` / `==========`), where
/// `next_text` is the immediately following line. Such a title heads its
/// section but matches none of the speaker/act-scene/separator/stanza-number
/// predicates, so without this check the cursor-landing paths treat it as
/// spoken dialogue and `gg`/`x` highlight the title instead of the first verse
/// line. Mirrors `text_file_map::is_title_above_separator` (which works on the
/// raw file lines for `section_starts`) but on already-prepared text.
pub fn is_title_above_separator(text: &str, next_text: &str) -> bool {
    let t = text.trim();
    if t.is_empty()
        || is_act_scene_marker(t)
        || is_separator(t)
        || is_stanza_number(t)
        || is_speaker(t)
        || is_stage_direction(t)
    {
        return false;
    }
    is_separator(next_text.trim())
}

fn divine_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Whole-word all-caps GOD or LORD. \b word boundaries reject GODLY/LORDES.
    RE.get_or_init(|| Regex::new(r"\b(GOD|LORD)\b").unwrap())
}

/// Byte ranges (start, end) of whole-word all-caps divine names (GOD, LORD) in
/// `line`, for word-level small-caps tagging. Title-case ("Lord") and partials
/// ("GODLY", "LORDES") are not matched — only the source's emphatic all-caps.
pub fn divine_name_spans(line: &str) -> Vec<(usize, usize)> {
    divine_name_re()
        .find_iter(line)
        .map(|m| (m.start(), m.end()))
        .collect()
}

/// A BCP body paragraph: a non-blank line that is neither a `## ` heading nor a
/// `[...]` rubric. These are the prayers and litany petitions that get
/// justified, block-spaced typography (and first-line indent / opening
/// small-caps). Display classification only.
pub fn is_bcp_body(text: &str) -> bool {
    !is_blank(text) && !is_bcp_heading(text) && !is_rubric(text) && !is_bcp_speaker(text)
}

/// Marker prefix on a BCP speaker cue row emitted on its own line by the TEI
/// pipeline (`@ Aunswere.`, `@ Priest.`). Mirrors the `## ` heading and `[...]`
/// rubric markers: a text-only signal the reader keys on, so a cue is never
/// confused with a one-word liturgical response (`Alleluya.`, `Amen.`) — which a
/// pure text heuristic cannot distinguish. Must match the BCP TEI pipeline's
/// `tei_to_rows.SPEAKER_MARKER` / `tei_to_text` `@ `. The `@ ` normalizes away,
/// so the cue row still text-matches its lit.db row.
pub const BCP_SPEAKER_MARKER: &str = "@ ";

/// True when `text` is a BCP speaker cue on its own line (Kindle layout): the
/// `Aunswere.` / `Priest.` cue rendered centered + italic above its response.
/// Keyed on the `@ ` marker (`BCP_SPEAKER_MARKER`), not a heuristic — BCP cues
/// are title-case so `is_speaker`'s all-caps regex never fires, and a one-word
/// response is textually identical to a one-word cue.
pub fn is_bcp_speaker(text: &str) -> bool {
    text.trim_start().starts_with(BCP_SPEAKER_MARKER)
}

/// Strip the `@ ` speaker marker for display (`@ Priest.` → `Priest.`). Returns
/// the input unchanged when unmarked.
pub fn strip_bcp_speaker_marker(text: &str) -> &str {
    let t = text.trim_start();
    t.strip_prefix(BCP_SPEAKER_MARKER).unwrap_or(text)
}

/// Split a BCP body prayer into one string per sentence, for sentence-per-line
/// display. Breaks at sentence-ending periods only; colons and semicolons (used
/// in BCP as internal list separators) stay inline. Returns the input as a
/// single-element vec when no break point is found (short litany lines, etc.).
///
/// A `.` is a break point when it is followed by whitespace and then an
/// uppercase letter, UNLESS:
/// - the token ending at the `.` is a known abbreviation (`&c`), or contains a
///   digit / is a dotted-numeral group (`.vi.`, `iiii.`), or
/// - the period is adjacent to a `*` footnote marker, or
/// - the next sentence is just `Amen.` — which attaches to the prior sentence so
///   it never dangles on its own line.
///
/// Caller is responsible for only passing body lines (not rubrics/headings).
pub fn split_bcp_sentences(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut sentences: Vec<String> = Vec::new();
    let mut start = 0usize; // start char index of the current sentence

    let mut i = 0usize;
    while i < n {
        if chars[i] == '.' && is_sentence_break_at(&chars, i) {
            // Tentative sentence: chars[start..=i]. Decide whether the NEXT
            // sentence is just "Amen." (then don't break here — let it attach).
            let after = next_word_after(&chars, i + 1);
            if after.eq_ignore_ascii_case("amen") {
                // Don't break; the Amen sentence will fold into this one. Advance
                // past this period and continue (the Amen's own terminal period
                // is the real line end).
                i += 1;
                continue;
            }
            let sentence: String = chars[start..=i].iter().collect();
            sentences.push(sentence.trim().to_string());
            // Skip the whitespace after the period to begin the next sentence.
            let mut j = i + 1;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            start = j;
            i = j;
            continue;
        }
        i += 1;
    }
    // Trailing remainder (the last sentence, or the whole line if no break).
    if start < n {
        let rest: String = chars[start..].iter().collect();
        let rest = rest.trim();
        if !rest.is_empty() {
            sentences.push(rest.to_string());
        }
    }
    if sentences.is_empty() {
        sentences.push(line.trim().to_string());
    }
    sentences
}

/// Is the `.` at char index `dot` a sentence break (followed by whitespace +
/// uppercase, and not part of an abbreviation/numeral/footnote)?
fn is_sentence_break_at(chars: &[char], dot: usize) -> bool {
    let n = chars.len();
    // Must be followed by whitespace then an uppercase alphabetic char.
    let mut j = dot + 1;
    if j >= n || !chars[j].is_whitespace() {
        return false;
    }
    while j < n && chars[j].is_whitespace() {
        j += 1;
    }
    if j >= n || !chars[j].is_uppercase() {
        return false;
    }
    // Footnote marker adjacent to the period (e.g. "name*." or "name.*").
    if dot > 0 && chars[dot - 1] == '*' {
        return false;
    }
    // Inspect the token immediately preceding the period.
    let mut k = dot; // dot is exclusive start; walk back over the token chars
    while k > 0 {
        let c = chars[k - 1];
        if c.is_whitespace() {
            break;
        }
        k -= 1;
    }
    let token: String = chars[k..dot].iter().collect();
    let token_lower = token.to_ascii_lowercase();
    // Known abbreviations.
    if token_lower == "&c" || token_lower.ends_with("&c") {
        return false;
    }
    // Tokens containing digits (e.g. ".vi", "iiii", part of a numeral group).
    if token.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    // Dotted-numeral group: the token began with a `.` (e.g. ".vi" in ".vi."),
    // meaning this period closes a `.roman.` abbreviation, not a sentence.
    if k > 0 && chars[k] == '.' {
        return false;
    }
    // Single-letter token + period: an initial/abbreviation (e.g. "S." "x.").
    if token.chars().filter(|c| c.is_alphabetic()).count() == 1 && token.len() <= 2 {
        return false;
    }
    true
}

/// The next whitespace-delimited word starting at or after char index `from`,
/// stripped of a trailing period — used to detect a following "Amen." sentence.
fn next_word_after(chars: &[char], from: usize) -> String {
    let n = chars.len();
    let mut j = from;
    while j < n && chars[j].is_whitespace() {
        j += 1;
    }
    let mut word = String::new();
    while j < n && !chars[j].is_whitespace() {
        word.push(chars[j]);
        j += 1;
    }
    word.trim_end_matches(['.', ',', '!', '?']).to_string()
}

/// Byte range (start, end) of the leading all-caps word run that opens a BCP
/// prayer, for small-caps styling of the opening word (LYGHTEN, WHOSOEVER,
/// ALMIGHTY, WE, THE). The source sets the opening word in caps as a proxy for
/// the printed small-caps. Returns `None` when the line does not begin with an
/// all-caps alphabetic word.
///
/// A lone leading "O" (the vocative, "O GOD merciful father") is skipped so the
/// span lands on the substantive opening word — but if the line is *only* "O"
/// followed by lowercase, nothing matches. Trailing all-caps divine names mid
/// run (e.g. "O LORD our heavenly father") are picked up by `divine_name_spans`
/// instead, so this only spans the very first emphatic word.
pub fn bcp_opening_smallcaps_span(line: &str) -> Option<(usize, usize)> {
    let mut chars = line.char_indices().peekable();
    // Skip leading whitespace.
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    // Optionally skip a lone vocative "O" so the span lands on the next word.
    let mut start = match chars.peek() {
        Some(&(i, _)) => i,
        None => return None,
    };
    if let Some(&(i, 'O')) = chars.peek() {
        let mut look = chars.clone();
        look.next(); // consume the O
        if matches!(look.peek(), Some(&(_, c)) if c.is_whitespace()) {
            // It's a standalone "O " — advance start past it and the space.
            chars.next();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace() {
                    chars.next();
                } else {
                    break;
                }
            }
            start = match chars.peek() {
                Some(&(j, _)) => j,
                None => return None,
            };
            let _ = i;
        }
    }
    // Consume the leading word; it must be all-uppercase alphabetic.
    let mut end = start;
    let mut has_alpha = false;
    let mut all_upper = true;
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() {
            break;
        }
        if c.is_alphabetic() {
            has_alpha = true;
            if !c.is_uppercase() {
                all_upper = false;
            }
        }
        end = i + c.len_utf8();
        chars.next();
    }
    if has_alpha && all_upper && end > start {
        Some((start, end))
    } else {
        None
    }
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
    fn test_title_above_separator() {
        // The anthology header form: a plain title directly above a ===== rule.
        assert!(is_title_above_separator("Sonnet 116", "=========="));
        assert!(is_title_above_separator("Sonnet 18", "========="));
        // Not a heading without a separator beneath it.
        assert!(!is_title_above_separator("Sonnet 116", ""));
        assert!(!is_title_above_separator(
            "Let me not to the marriage of true minds",
            "Admit impediments. Love is not love"
        ));
        // A bare stanza number is handled by is_stanza_number, not this.
        assert!(!is_title_above_separator("116", "=========="));
        // Speaker / act markers above a rule are claimed by their own predicate.
        assert!(!is_title_above_separator("HAMLET", "=========="));
        assert!(!is_title_above_separator("ACT 1", "=========="));
        // The separator line itself is not a title.
        assert!(!is_title_above_separator("==========", "Some text"));
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

    #[test]
    fn test_is_bcp_work() {
        assert!(is_bcp_work("BCP1559"));
        assert!(is_bcp_work("BCP1559M"));
        assert!(is_bcp_work("BCP1662"));
        assert!(!is_bcp_work("Ham"));
        assert!(!is_bcp_work("bcp1559")); // case-sensitive, matches echo-channel convention
    }

    #[test]
    fn test_is_bcp_heading() {
        assert!(is_bcp_heading("## THE SUPPER"));
        assert!(is_bcp_heading("## An Order for Morning"));
        assert!(!is_bcp_heading("THE SUPPER")); // no marker
        assert!(!is_bcp_heading("[a rubric]"));
    }

    #[test]
    fn test_is_rubric() {
        assert!(is_rubric("[The Priest shall say.]"));
        assert!(is_rubric("[¶ The Morning prayer shall be used.]"));
        assert!(!is_rubric("## A heading"));
        assert!(!is_rubric("Our Father, which art in heaven."));
    }

    #[test]
    fn test_rubric_is_centered() {
        // Short transition/speaker cues -> centered.
        assert!(rubric_is_centered("The Priest."));
        assert!(rubric_is_centered("The Answer."));
        assert!(rubric_is_centered("Then likewise he shall say."));
        // A leading pilcrow does not change the decision.
        assert!(rubric_is_centered("¶ Then the Collect of the day."));
        // Long instructional prose -> hanging (not centered).
        assert!(!rubric_is_centered(
            "At the beginning both of Morning Prayer, and likewise of Evening \
             Prayer, the Minister shall read with a loud voice, some one of these \
             sentences of the Scriptures that follow."
        ));
    }

    #[test]
    fn test_is_bcp_speaker_and_strip() {
        assert!(is_bcp_speaker("@ Priest."));
        assert!(is_bcp_speaker("@ Aunswere."));
        assert!(is_bcp_speaker("   @ Answer."));
        // Unmarked lines are NOT cues — a one-word response is left as body.
        assert!(!is_bcp_speaker("Alleluya."));
        assert!(!is_bcp_speaker("Priest."));
        assert!(!is_bcp_speaker("## A heading"));
        assert!(!is_bcp_speaker("[a rubric]"));
        assert_eq!(strip_bcp_speaker_marker("@ Priest."), "Priest.");
        assert_eq!(strip_bcp_speaker_marker("Alleluya."), "Alleluya.");
        // A marked cue is not body (no body spacing / opening small-caps).
        assert!(!is_bcp_body("@ Priest."));
        assert!(is_bcp_body("Alleluya."));
    }

    #[test]
    fn test_divine_name_spans() {
        // Whole-word GOD / LORD -> byte ranges of each.
        let line = "O Lord GOD, Lamb of GOD";
        let spans = divine_name_spans(line);
        // "GOD" at byte 7..10 and 20..23; "Lord" is title-case, not all-caps -> skip.
        assert_eq!(spans, vec![(7, 10), (20, 23)]);
    }

    #[test]
    fn test_divine_name_spans_ignores_partials_and_lowercase() {
        assert_eq!(divine_name_spans("god is good"), vec![]); // lowercase
        assert_eq!(divine_name_spans("GODLY living"), vec![]); // not whole word
        assert_eq!(divine_name_spans("the LORDES table"), vec![]); // LORDES != LORD
        // Whole-word all-caps LORD is found.
        assert_eq!(divine_name_spans("the LORD reigneth"), vec![(4, 8)]);
    }

    #[test]
    fn test_is_bcp_body() {
        assert!(is_bcp_body("O GOD merciful father, that despisest not"));
        assert!(is_bcp_body("Lord have mercy upon us."));
        assert!(!is_bcp_body("## THE SUPPER")); // heading
        assert!(!is_bcp_body("[The Priest shall say.]")); // rubric
        assert!(!is_bcp_body("   ")); // blank
    }

    #[test]
    fn test_bcp_opening_smallcaps_span() {
        // Leading all-caps word -> spanned.
        assert_eq!(bcp_opening_smallcaps_span("ALMIGHTY God, which"), Some((0, 8)));
        assert_eq!(bcp_opening_smallcaps_span("WE humbly beseech the"), Some((0, 2)));
        assert_eq!(bcp_opening_smallcaps_span("THE grace of our Lord"), Some((0, 3)));
        assert_eq!(bcp_opening_smallcaps_span("LYGHTEN our darkenes"), Some((0, 7)));
        // Vocative "O " is skipped; span lands on the next emphatic word.
        assert_eq!(bcp_opening_smallcaps_span("O GOD merciful father"), Some((2, 5)));
        assert_eq!(bcp_opening_smallcaps_span("O LORD our heavenly father"), Some((2, 6)));
        // Title-case / lowercase openings -> no span.
        assert_eq!(bcp_opening_smallcaps_span("Lord have mercy upon us."), None);
        assert_eq!(bcp_opening_smallcaps_span("Our father which art"), None);
        assert_eq!(bcp_opening_smallcaps_span("And lead us not"), None);
        // Leading whitespace tolerated.
        assert_eq!(bcp_opening_smallcaps_span("  WHOSOEVER will be"), Some((2, 11)));
        // Lone "O" with lowercase after the vocative skip leaves no caps word.
        assert_eq!(bcp_opening_smallcaps_span("O father, mercifully"), None);
    }

    #[test]
    fn test_split_bcp_sentences_basic() {
        // Two sentences: break at ". A" (period + space + capital).
        let two = "O GOD merciful father, that despisest not the sighing of a \
            contrite heart, whensoever they oppress us. And graciously hear us, \
            that those evils be brought to nought, through Jesus Christ our Lord.";
        let parts = split_bcp_sentences(two);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with("oppress us."));
        assert!(parts[1].starts_with("And graciously"));
        assert!(parts[1].ends_with("our Lord."));
    }

    #[test]
    fn test_split_bcp_sentences_no_split_on_colon_semicolon() {
        // Colons and semicolons are internal list separators, not breaks.
        let one = "O LORD our heavenly father, high and mighty king of kings: \
            Grant her in health and wealth long to live; strength her that she \
            may vanquish and overcome al her enemies: through Jesus Christ.";
        let parts = split_bcp_sentences(one);
        assert_eq!(parts.len(), 1, "colon/semicolon must not split");
    }

    #[test]
    fn test_split_bcp_sentences_abbreviations() {
        // &c. is not a sentence end.
        assert_eq!(
            split_bcp_sentences("Our father which art in heaven. &c. And lead us not.").len(),
            // "heaven. &c." -> "&c." is lowercase after the period so no break there;
            // "&c. And" -> capital A, but "&c." is a known abbreviation -> no break.
            // Only "us not." ends the line. So: "...heaven." | "&c. And lead us not."?
            // "heaven. &c": "&" is not uppercase-alpha -> no break. Single sentence.
            1
        );
        // Dotted roman-numeral groups and trailing-letter numerals don't split.
        assert_eq!(
            split_bcp_sentences(
                "do all that thou haste to do, but the .vii. day is the Sabbath \
                 of the lord thy god. In it thou shalt do no manner of work."
            )
            .len(),
            2,
            ".vii. stays inline; only the real sentence end splits"
        );
        // Footnote asterisk adjacent to a name+period must not split.
        assert_eq!(
            split_bcp_sentences(
                "behold our most gracious sovereign Lady Queen Elizabeth*, and \
                 so replenish her with the grace of thy holy spirit. Amen."
            )
            .len(),
            1,
            "Elizabeth*, is mid-sentence; Amen. attaches -> one line"
        );
    }

    #[test]
    fn test_split_bcp_sentences_amen_attaches() {
        // Trailing "Amen." stays with the preceding sentence (no dangling line).
        let parts = split_bcp_sentences(
            "Grant this for thy mercy's sake. Through Jesus Christ our Lord. Amen.",
        );
        assert_eq!(parts.len(), 2);
        assert!(parts[1].ends_with("our Lord. Amen."), "Amen. attaches to prior sentence");
    }

    #[test]
    fn test_split_bcp_sentences_no_split_point() {
        // A short line with no sentence-internal break returns unchanged.
        assert_eq!(
            split_bcp_sentences("Lord have mercy upon us."),
            vec!["Lord have mercy upon us.".to_string()]
        );
        assert_eq!(
            split_bcp_sentences("Let us pray."),
            vec!["Let us pray.".to_string()]
        );
    }

    #[test]
    fn test_is_bcp_rite_title() {
        assert!(is_bcp_rite_title("## THE SUPPER"));
        assert!(is_bcp_rite_title("## AN ORDER FOR MORNING"));
        // Mixed-case heading is a sub-heading, not a rite title.
        assert!(!is_bcp_rite_title("## The third Collect: for grace."));
        assert!(!is_bcp_rite_title("Our Father")); // not a heading
    }
}
