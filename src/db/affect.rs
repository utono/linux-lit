//! NRC-VAD affect scoring for the echo re-rank axis.
//!
//! Computes a 3-D affect vector (Valence, Arousal, Dominance) for a query
//! passage by averaging the per-word NRC-VAD scores, mirroring the
//! `compute_vad` function in `scripts/build_embeddings.py` exactly so the
//! query side and the stored document side agree.
//!
//! The lexicon is the same gitignored file the Python build script reads
//! (`scripts/data/NRC-VAD-Lexicon.txt`). It is loaded lazily once and cached.
//! If the file is absent, scoring returns `None` and callers fall back to
//! pure semantic ranking.
//!
//! See docs/superpowers/specs/2026-05-30-semantic-echo-search-design.md
//! ("Sentiment/Affect Re-Rank Axis").

use std::collections::HashMap;
use std::sync::OnceLock;

/// Neutral midpoint used when a passage contains no in-lexicon words, so the
/// affect cosine stays well-defined rather than producing a zero vector.
/// Matches `VAD_NEUTRAL` in scripts/build_embeddings.py.
pub const VAD_NEUTRAL: [f32; 3] = [0.5, 0.5, 0.5];

/// Path to the NRC-VAD lexicon, resolved relative to the repo at compile time.
/// This is a single-machine personal app, so an absolute compiled-in path is
/// consistent with how the DB and theme paths are handled.
fn lexicon_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("data")
        .join("NRC-VAD-Lexicon.txt")
}

/// Lazily-loaded lexicon: word -> [valence, arousal, dominance].
/// `None` means the lexicon file was missing or unreadable.
static LEXICON: OnceLock<Option<HashMap<String, [f32; 3]>>> = OnceLock::new();

fn lexicon() -> &'static Option<HashMap<String, [f32; 3]>> {
    LEXICON.get_or_init(|| {
        let path = lexicon_path();
        let contents = std::fs::read_to_string(&path).ok()?;
        let mut map = HashMap::new();
        for line in contents.lines() {
            // Tab-separated: word\tvalence\tarousal\tdominance, no header.
            let mut parts = line.split('\t');
            let (Some(word), Some(v), Some(a), Some(d)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if parts.next().is_some() {
                continue; // more than 4 fields — malformed
            }
            if let (Ok(v), Ok(a), Ok(d)) = (v.parse::<f32>(), a.parse::<f32>(), d.parse::<f32>()) {
                map.insert(word.to_string(), [v, a, d]);
            }
        }
        if map.is_empty() {
            crate::logging::log(&format!(
                "AFFECT: lexicon at {} loaded 0 words — affect re-rank disabled",
                path.display()
            ));
            None
        } else {
            Some(map)
        }
    })
}

/// True if the lexicon is available (so affect scoring will work).
pub fn lexicon_available() -> bool {
    lexicon().is_some()
}

/// Tokenize like the Python `WORD_RE = [a-z']+` over lowercased text.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_ascii_alphabetic() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
}

/// Mean VAD over the in-lexicon words of `text`.
///
/// Returns `None` if the lexicon is unavailable. Returns `VAD_NEUTRAL` if the
/// lexicon is present but the passage has no in-lexicon words — matching the
/// Python side, so a query with no scorable words compares as neutral rather
/// than failing.
pub fn compute_vad(text: &str) -> Option<[f32; 3]> {
    let lex = lexicon().as_ref()?;
    let mut sum = [0.0f32; 3];
    let mut n = 0u32;
    for w in words(text) {
        if let Some(vad) = lex.get(&w) {
            sum[0] += vad[0];
            sum[1] += vad[1];
            sum[2] += vad[2];
            n += 1;
        }
    }
    if n == 0 {
        return Some(VAD_NEUTRAL);
    }
    let n = n as f32;
    Some([sum[0] / n, sum[1] / n, sum[2] / n])
}

/// Cosine similarity between two 3-D affect vectors, in [-1, 1].
/// Returns 0.0 if either vector is degenerate (zero norm).
pub fn affect_cosine(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let na = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let nb = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

/// Decode a stored `sentiment` blob (3 little-endian f32) into a VAD vector.
/// Returns `None` if the blob is absent or the wrong length.
pub fn decode_sentiment(blob: &[u8]) -> Option<[f32; 3]> {
    if blob.len() != 12 {
        return None;
    }
    Some([
        f32::from_le_bytes([blob[0], blob[1], blob[2], blob[3]]),
        f32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]),
        f32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_when_no_lexicon_words() {
        // 'zzqqxx' is not a word in any lexicon; if the lexicon is present this
        // must be neutral, and if absent, None. Either way never a zero vector.
        match compute_vad("zzqqxx") {
            Some(v) => assert_eq!(v, VAD_NEUTRAL),
            None => {} // lexicon not installed in this environment
        }
    }

    #[test]
    fn cosine_identical_is_one() {
        let v = [0.3, 0.6, 0.4];
        assert!((affect_cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_handles_zero_vector() {
        assert_eq!(affect_cosine(&[0.0, 0.0, 0.0], &[0.3, 0.6, 0.4]), 0.0);
    }

    #[test]
    fn decode_roundtrip() {
        let vad = [0.115f32, 0.794, 0.245];
        let mut blob = Vec::new();
        blob.extend_from_slice(&vad[0].to_le_bytes());
        blob.extend_from_slice(&vad[1].to_le_bytes());
        blob.extend_from_slice(&vad[2].to_le_bytes());
        let decoded = decode_sentiment(&blob).unwrap();
        assert!((decoded[0] - vad[0]).abs() < 1e-6);
        assert!((decoded[1] - vad[1]).abs() < 1e-6);
        assert!((decoded[2] - vad[2]).abs() < 1e-6);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert!(decode_sentiment(&[0, 1, 2]).is_none());
        assert!(decode_sentiment(&[]).is_none());
    }

    #[test]
    fn vad_matches_python_reference() {
        // Parity guard: this exact value comes from running compute_vad() in
        // scripts/build_embeddings.py on the same string. If the lexicon isn't
        // installed in this environment the test is a no-op; when it is, the
        // Rust and Python tokenizers/averaging MUST agree or the query-side and
        // document-side affect vectors would be computed differently.
        if let Some(v) = compute_vad("Out, out, brief candle! despair and death") {
            if v == VAD_NEUTRAL {
                return; // lexicon absent — skip
            }
            assert!((v[0] - 0.304250).abs() < 1e-4, "valence {}", v[0]);
            assert!((v[1] - 0.508750).abs() < 1e-4, "arousal {}", v[1]);
            assert!((v[2] - 0.335000).abs() < 1e-4, "dominance {}", v[2]);
        }
    }
}
