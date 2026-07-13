//! Pure parsing of the journal term-extraction model response. The extractor
//! returns `{"terms":[...]}`; this normalizes it (lowercase, trim, dedupe,
//! cap 8). Mirrors litdb tag_journal.py::parse_terms_result. No DB, no GTK.

/// Fallback extraction system prompt, used when the active `journal.extract-terms`
/// row is absent from lit.db `api_prompts`. Byte-identical to litdb
/// `tag_journal.py::SYSTEM_PROMPT` so reader-auto and batch tags match even when
/// the DB row is unseeded (the real lit.db has never held this row — litdb's
/// batch tagger runs off its own hardcoded fallback too).
pub const FALLBACK_EXTRACT_PROMPT: &str = "\
You extract the substantive terms of art that a literary journal Q&A entry\n\
explains — legal, rhetorical, historical, prosodic, or theological terms a\n\
reader might later want to look up (e.g. \"fee simple\", \"anaphora\", \"recusant\").\n\
\n\
Do NOT include ordinary vocabulary, character names, or the work's title.\n\
Prefer the canonical phrasing of each term. Return AT MOST 8 terms.\n\
\n\
Return ONLY a JSON object (no markdown fences, no commentary) with exactly one\n\
key:\n\
\n\
{\"terms\": [\"term one\", \"term two\"]}\n\
\n\
If the entry explains no such term, return {\"terms\": []}.";

/// Strip a leading/trailing markdown code fence, if present. Models (esp. Haiku)
/// wrap JSON in ```` ```json ... ``` ```` despite instructions not to. Mirrors
/// litdb tag_journal.py::parse_terms_result's fence handling.
fn strip_code_fence(raw: &str) -> &str {
    let text = raw.trim();
    let Some(after_open) = text.strip_prefix("```") else {
        return text;
    };
    // Drop the rest of the opening-fence line (e.g. "json\n" or just "\n").
    let body = match after_open.find('\n') {
        Some(nl) => &after_open[nl + 1..],
        None => "",
    };
    // Drop the closing fence and anything after it.
    match body.rfind("```") {
        Some(close) => body[..close].trim(),
        None => body.trim(),
    }
}

/// Parse the extractor's `{"terms":[...]}` reply into a clean term list:
/// lowercase, trim, dedupe (order-preserving), cap at 8. Tolerant — returns an
/// empty Vec when the JSON is unparseable, lacks a `"terms"` key, or `"terms"`
/// is not a list. A leading/trailing markdown code fence is stripped first.
/// Non-string / blank-after-trim elements are skipped.
pub fn parse_terms(raw: &str) -> Vec<String> {
    let cleaned = strip_code_fence(raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(cleaned) else {
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
    fn strips_markdown_code_fence() {
        // Haiku wraps JSON in a ```json fence despite the prompt; must still parse.
        let fenced = "```json\n{\"terms\": [\"fee simple\", \"attainder\"]}\n```";
        assert_eq!(
            parse_terms(fenced),
            vec!["fee simple".to_string(), "attainder".to_string()]
        );
        // Bare ``` fence (no language tag) also stripped.
        let bare = "```\n{\"terms\":[\"anaphora\"]}\n```";
        assert_eq!(parse_terms(bare), vec!["anaphora".to_string()]);
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
