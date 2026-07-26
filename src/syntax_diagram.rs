//! Data model for the syntax diagram overlay: the validated band/POS spans a
//! Cairo surface draws. Pure — no GTK — so the whole correctness surface is
//! unit-testable without a display.
//!
//! Spans are CHAR OFFSETS into the selection text, matching the `line_syntax`
//! convention (offsets into `canonical_text`), so parse-derived and
//! Claude-derived spans share one coordinate space.

use serde::Deserialize;

/// A labelled span marking what a stretch of the selection grammatically IS.
#[derive(Debug, Clone, Deserialize)]
pub struct Band {
    pub start_char: usize,
    pub end_char: usize,
    pub label: String,
    /// 0 = outermost. Nesting depth, used to stack rows.
    pub depth: u8,
}

/// Part of speech for one word.
#[derive(Debug, Clone, Deserialize)]
pub struct PosTag {
    pub start_char: usize,
    pub end_char: usize,
    pub pos: String,
}

/// What Claude returns, before validation.
#[derive(Debug, Deserialize)]
struct RawAnalysis {
    bands: Vec<Band>,
    #[serde(default)]
    pos: Vec<PosTag>,
    #[serde(default)]
    note: Option<String>,
}

/// A validated analysis, safe to draw.
#[derive(Debug)]
pub struct SyntaxAnalysis {
    /// The selection, exactly as sent.
    pub text: String,
    pub bands: Vec<Band>,
    pub pos: Vec<PosTag>,
    pub note: Option<String>,
}

/// True when `span` is a usable slice of `text`: ordered, in bounds, and on
/// char boundaries (a mid-UTF-8 offset would panic any later slicing).
fn span_ok(text: &str, start: usize, end: usize) -> bool {
    start < end
        && end <= text.len()
        && text.is_char_boundary(start)
        && text.is_char_boundary(end)
}

/// True when two spans nest (one contains the other) or are disjoint.
/// Partial overlap is not drawable as a stack, so it is rejected.
fn compatible(a: &Band, b: &Band) -> bool {
    let disjoint = a.end_char <= b.start_char || b.end_char <= a.start_char;
    let a_in_b = b.start_char <= a.start_char && a.end_char <= b.end_char;
    let b_in_a = a.start_char <= b.start_char && b.end_char <= a.end_char;
    disjoint || a_in_b || b_in_a
}

/// Parse Claude's JSON reply into a validated analysis.
///
/// Malformed JSON is an Err (the caller toasts and does not open the overlay).
/// Individual bad SPANS are dropped, not fatal: a hallucinated offset loses one
/// band, but a bad one would draw garbage. Bands are checked against every band
/// already accepted, so the survivors are mutually nestable.
pub fn parse_analysis(json: &str, text: &str) -> Result<SyntaxAnalysis, String> {
    // Claude may wrap JSON in prose or a fence; take the outermost object.
    let slice = match (json.find('{'), json.rfind('}')) {
        (Some(a), Some(b)) if b > a => &json[a..=b],
        _ => return Err("no JSON object in reply".to_string()),
    };
    let raw: RawAnalysis =
        serde_json::from_str(slice).map_err(|e| format!("bad JSON: {e}"))?;

    let mut bands: Vec<Band> = Vec::new();
    for b in raw.bands {
        if !span_ok(text, b.start_char, b.end_char) {
            crate::logging::log(&format!(
                "SYNTAX: dropped band '{}' [{}..{}] — bad span",
                b.label, b.start_char, b.end_char
            ));
            continue;
        }
        if !bands.iter().all(|k| compatible(&b, k)) {
            crate::logging::log(&format!(
                "SYNTAX: dropped band '{}' [{}..{}] — partial overlap",
                b.label, b.start_char, b.end_char
            ));
            continue;
        }
        bands.push(b);
    }

    let pos = raw
        .pos
        .into_iter()
        .filter(|p| span_ok(text, p.start_char, p.end_char))
        .collect();

    Ok(SyntaxAnalysis { text: text.to_string(), bands, pos, note: raw.note })
}

/// Display row per band, by depth: row 0 sits directly under the POS strip,
/// deeper bands stack above it, so the outermost band is the bottom rule.
pub fn assign_rows(bands: &[Band]) -> Vec<usize> {
    bands.iter().map(|b| b.depth as usize).collect()
}

/// Highest row index any band occupies (0 when there are none) — the drawing
/// code sizes the band stack from this.
pub fn max_row(bands: &[Band]) -> usize {
    assign_rows(bands).into_iter().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "A touch on the hand, irresolute, makes him start";

    fn json_with(bands: &str) -> String {
        format!(r#"{{"bands":{bands},"pos":[],"note":null}}"#)
    }

    #[test]
    fn parses_well_formed_bands() {
        let j = json_with(
            r#"[{"start_char":0,"end_char":19,"label":"subject","depth":1},
                {"start_char":0,"end_char":47,"label":"main clause","depth":0}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert_eq!(a.bands.len(), 2);
        assert_eq!(a.text, TEXT);
    }

    #[test]
    fn drops_band_past_end_of_text() {
        let j = json_with(
            r#"[{"start_char":0,"end_char":9999,"label":"bogus","depth":0}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert!(a.bands.is_empty(), "out-of-range band must be dropped");
    }

    #[test]
    fn drops_inverted_band() {
        let j = json_with(
            r#"[{"start_char":20,"end_char":5,"label":"inverted","depth":0}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert!(a.bands.is_empty(), "end before start must be dropped");
    }

    #[test]
    fn drops_partially_overlapping_band() {
        // Nesting requires containment or disjointness. 0..20 and 10..30
        // partially overlap: the second must go.
        let j = json_with(
            r#"[{"start_char":0,"end_char":20,"label":"a","depth":0},
                {"start_char":10,"end_char":30,"label":"b","depth":1}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert_eq!(a.bands.len(), 1);
        assert_eq!(a.bands[0].label, "a");
    }

    #[test]
    fn keeps_disjoint_and_contained_bands() {
        let j = json_with(
            r#"[{"start_char":0,"end_char":47,"label":"outer","depth":0},
                {"start_char":0,"end_char":19,"label":"inner","depth":1},
                {"start_char":21,"end_char":31,"label":"sibling","depth":1}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert_eq!(a.bands.len(), 3);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_analysis("not json at all", TEXT).is_err());
    }

    #[test]
    fn rejects_offsets_splitting_a_utf8_char() {
        // "café" — byte 4 is inside the é. A band boundary there would panic
        // any later slicing, so it must be dropped.
        let text = "café au lait";
        let j = json_with(r#"[{"start_char":0,"end_char":4,"label":"x","depth":0}]"#);
        let a = parse_analysis(&j, text).unwrap();
        assert!(a.bands.is_empty(), "non-char-boundary offset must be dropped");
    }

    #[test]
    fn assigns_deeper_bands_to_higher_rows() {
        let bands = vec![
            Band { start_char: 0, end_char: 47, label: "outer".into(), depth: 0 },
            Band { start_char: 0, end_char: 19, label: "inner".into(), depth: 1 },
        ];
        let rows = assign_rows(&bands);
        assert_eq!(rows, vec![0, 1]);
    }
}
