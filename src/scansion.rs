//! Pure Wright-scansion mark renderer. No DB, no GTK. Given a DISPLAYED line and
//! its scansion, returns the line with combining stress marks inserted on each
//! syllable's vowel, plus the line-type label. Marks are placed by re-finding the
//! vowel IN the displayed line — never by trusting a stored char offset — so the
//! invariant "strip the combining marks -> the displayed line" always holds.

/// Combining acute U+0301 over a stressed syllable's vowel.
pub const ACUTE: char = '\u{0301}';
/// Combining breve U+0306 over an unstressed syllable's vowel.
pub const BREVE: char = '\u{0306}';
/// Thin double bar marking a caesura (metrical pause), inserted after the
/// caesura syllable's vowel.
pub const CAESURA: &str = "\u{2016}"; // ‖

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanLevel {
    Off,
    StressOnly,
    Full,
}

impl ScanLevel {
    /// Advance Off -> StressOnly -> Full -> Off.
    pub fn next(self) -> ScanLevel {
        match self {
            ScanLevel::Off => ScanLevel::StressOnly,
            ScanLevel::StressOnly => ScanLevel::Full,
            ScanLevel::Full => ScanLevel::Off,
        }
    }

    /// The persisted/config string form.
    pub fn as_str(self) -> &'static str {
        match self {
            ScanLevel::Off => "off",
            ScanLevel::StressOnly => "stress",
            ScanLevel::Full => "full",
        }
    }

    /// Parse the persisted/config string form; unknown -> Off.
    pub fn from_config_str(s: &str) -> ScanLevel {
        match s {
            "stress" => ScanLevel::StressOnly,
            "full" => ScanLevel::Full,
            _ => ScanLevel::Off,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanSyllable {
    /// The syllable text as scanned (from syllable_scan.surface).
    pub surface: String,
    /// 1 = strong (stressed), 0 = weak (unstressed). Single source of truth.
    pub ictus: i8,
    pub is_extrametrical: bool,
}

#[derive(Debug, Clone)]
pub struct LineScansion {
    pub line_type: String,
    /// 1-based syllable position after which a caesura falls, or None.
    pub caesura_after: Option<i32>,
    pub syllables: Vec<ScanSyllable>,
}

/// A rendered line: the marked text plus the separate line-type label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedLine {
    pub text: String,
    pub label: String,
}

/// Vowels that can carry a combining mark.
fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// Render `displayed_line` with stress marks for `scan` at `level`.
///
/// Walks the syllables in order, advancing a cursor across the displayed line.
/// For each syllable it locates the syllable's first vowel at or after the cursor
/// (anchored on the syllable's `surface` when that substring is found ahead, so
/// repeated letters don't misalign) and records a combining mark for that char
/// index. A syllable whose surface can't be located (or which has no vowel ahead)
/// is skipped (no mark) rather than mis-placed. Marks are inserted AFTER the vowel char so stripping the
/// combining chars reproduces `displayed_line` exactly.
pub fn mark_line(displayed_line: &str, scan: &LineScansion, level: ScanLevel) -> MarkedLine {
    if level == ScanLevel::Off {
        return MarkedLine { text: displayed_line.to_string(), label: scan.line_type.clone() };
    }

    let chars: Vec<char> = displayed_line.chars().collect();
    // char index -> combining mark to insert after it
    let mut marks: std::collections::BTreeMap<usize, char> = std::collections::BTreeMap::new();
    // char indices after which to insert a caesura glyph
    let mut caesura_at: Option<usize> = None;

    let mut cursor = 0usize; // char index into `chars`
    for (pos, syl) in scan.syllables.iter().enumerate() {
        // Anchor on the syllable's surface. If the surface is non-empty but cannot
        // be located at/after the cursor, skip this syllable (no mark) rather than
        // mis-placing a mark on an unrelated vowel ahead.
        let has_surface = syl.surface.chars().any(|c| c.is_alphanumeric());
        let search_from = match find_surface(&chars, cursor, &syl.surface) {
            Some(i) => i,
            None if has_surface => continue, // surface present but not found -> skip
            None => cursor,                  // empty/punctuation-only surface -> best effort from cursor
        };
        let vowel_idx = match (search_from..chars.len()).find(|&i| is_vowel(chars[i])) {
            Some(i) => i,
            None => continue, // no vowel locatable -> skip
        };
        // Place the stress mark per level.
        let want_mark = match level {
            ScanLevel::StressOnly => syl.ictus == 1,
            ScanLevel::Full => true,
            ScanLevel::Off => false,
        };
        if want_mark {
            marks.insert(vowel_idx, if syl.ictus == 1 { ACUTE } else { BREVE });
        }
        // Caesura falls after the 1-based `caesura_after` syllable's vowel.
        if scan.caesura_after == Some(pos as i32 + 1) {
            caesura_at = Some(vowel_idx);
        }
        cursor = vowel_idx + 1;
    }

    // Rebuild the string, inserting marks/caesura after the relevant chars.
    let mut out = String::with_capacity(displayed_line.len() + marks.len() * 2 + 3);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        if let Some(&mk) = marks.get(&i) {
            out.push(mk);
        }
        if caesura_at == Some(i) {
            out.push(' ');
            out.push_str(CAESURA);
            out.push(' ');
        }
    }
    MarkedLine { text: out, label: scan.line_type.clone() }
}

/// First char index >= `from` where `surface` begins in `chars` (case-insensitive,
/// alphanumeric-only comparison so punctuation/spacing differences don't block it).
/// Returns None if not found.
fn find_surface(chars: &[char], from: usize, surface: &str) -> Option<usize> {
    let needle: Vec<char> = surface.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if needle.is_empty() {
        return None;
    }
    for start in from..chars.len() {
        let mut ni = 0usize;
        let mut ci = start;
        while ci < chars.len() && ni < needle.len() {
            let cc = chars[ci];
            if cc.is_alphanumeric() {
                if cc.to_ascii_lowercase() != needle[ni] {
                    break;
                }
                ni += 1;
            }
            ci += 1;
        }
        if ni == needle.len() {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syl(surface: &str, ictus: i8) -> ScanSyllable {
        ScanSyllable { surface: surface.to_string(), ictus, is_extrametrical: false }
    }

    fn strip(s: &str) -> String {
        s.chars().filter(|c| *c != ACUTE && *c != BREVE).collect()
    }

    #[test]
    fn off_is_identity() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 0), syl("mu", 1)] };
        let m = mark_line("If music", &scan, ScanLevel::Off);
        assert_eq!(m.text, "If music");
        assert_eq!(m.label, "regular");
    }

    #[test]
    fn stress_only_marks_only_strong() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 0), syl("mu", 1), syl("sic", 0)] };
        let m = mark_line("If music", &scan, ScanLevel::StressOnly);
        assert!(m.text.contains(ACUTE));   // the strong "mu"
        assert!(!m.text.contains(BREVE));  // no breve in StressOnly
        assert_eq!(strip(&m.text), "If music"); // invariant: strip -> displayed line
    }

    #[test]
    fn full_marks_both() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 0), syl("mu", 1)] };
        let m = mark_line("If mu", &scan, ScanLevel::Full);
        assert!(m.text.contains(ACUTE));
        assert!(m.text.contains(BREVE));
        assert_eq!(strip(&m.text), "If mu");
    }

    #[test]
    fn caesura_inserted_after_position() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: Some(1),
            syllables: vec![syl("If", 1), syl("mu", 0)] };
        let m = mark_line("If mu", &scan, ScanLevel::StressOnly);
        assert!(m.text.contains(CAESURA));
        // The caesura is inserted chrome (` ‖ ` with padding), not a combining-mark
        // overlay. Removing the combining marks AND the full caesura chrome must
        // reproduce the displayed line exactly.
        let cleaned = strip(&m.text).replace(&format!(" {} ", CAESURA), "");
        assert_eq!(cleaned, "If mu");
    }

    #[test]
    fn surface_not_found_skips_syllable_no_panic() {
        // "xyz" isn't in the line; that syllable is skipped (no mark mis-placed),
        // and other syllables still mark normally.
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 1), syl("xyz", 1)] };
        let m = mark_line("If music", &scan, ScanLevel::StressOnly);
        assert_eq!(strip(&m.text), "If music"); // strip-invariant holds
        // Exactly one acute, and it's on "If" (after the 'I'), not on "music".
        assert_eq!(m.text.matches(ACUTE).count(), 1);
        let i_pos = m.text.find('I').unwrap();
        let acute_pos = m.text.find(ACUTE).unwrap();
        assert!(acute_pos > i_pos && acute_pos < m.text.find('f').unwrap(),
                "acute should sit on the I of 'If', not later");
    }
}
