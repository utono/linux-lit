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
