//! Pure parsing of the journal term-extraction model response. The extractor
//! returns `{"terms":[...]}`; this normalizes it (lowercase, trim, dedupe,
//! cap 8). Mirrors litdb tag_journal.py::parse_terms_result. No DB, no GTK.

/// Parse the extractor's `{"terms":[...]}` reply into a clean term list:
/// lowercase, trim, dedupe (order-preserving), cap at 8. Tolerant — returns an
/// empty Vec when the JSON is unparseable, lacks a `"terms"` key, or `"terms"`
/// is not a list. Non-string / blank-after-trim elements are skipped.
pub fn parse_terms(raw: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = value.get("terms").and_then(|t| t.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in arr {
        let Some(s) = item.as_str() else { continue };
        let norm = s.trim().to_lowercase();
        if norm.is_empty() || out.contains(&norm) {
            continue;
        }
        out.push(norm);
        if out.len() == 8 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        let out = parse_terms(r#"{"terms":["Fee Simple","  freehold ","FEE SIMPLE"]}"#);
        // lowercased, trimmed, deduped (order-preserving), "fee simple" once.
        assert_eq!(out, vec!["fee simple".to_string(), "freehold".to_string()]);
    }

    #[test]
    fn empty_terms_list_ok() {
        assert!(parse_terms(r#"{"terms":[]}"#).is_empty());
    }

    #[test]
    fn missing_key_or_bad_shape_is_empty_not_panic() {
        assert!(parse_terms(r#"{"nope":[1]}"#).is_empty());
        assert!(parse_terms(r#"{"terms":"notalist"}"#).is_empty());
        assert!(parse_terms("total garbage not json").is_empty());
    }

    #[test]
    fn caps_at_eight_and_skips_blanks() {
        let raw = r#"{"terms":["a","b","c","d","e","f","g","h","i","  "]}"#;
        let out = parse_terms(raw);
        assert_eq!(out.len(), 8);
        assert_eq!(out, vec!["a","b","c","d","e","f","g","h"]);
    }
}
