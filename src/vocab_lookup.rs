//! Local dictionary lookup for the add-vocab-word flow. Shells to WordNet
//! (`wn`) then GNU dict/gcide (`dict -d gcide`), mirroring litdb's
//! `scripts/vocab/definitions.py`. Parsing is split from the `Command`
//! invocation so it is unit-testable without the CLI tools installed.

use std::process::Command;

/// Parse `wn <word> -over` output: the first `-- (definition…)` gloss.
pub(crate) fn parse_wn(stdout: &str) -> Option<String> {
    // wn overview lines look like: "1. (12) word -- (the definition text)"
    for line in stdout.lines() {
        if let Some(idx) = line.find("-- (") {
            let rest = &line[idx + 4..];
            if let Some(end) = rest.rfind(')') {
                let def = rest[..end].trim();
                if !def.is_empty() {
                    return Some(def.to_string());
                }
            }
        }
    }
    None
}

/// Parse `dict -d gcide <word>` output: the first sense line after the
/// headword block. gcide senses are indented and often start with a POS tag.
pub(crate) fn parse_gcide(stdout: &str) -> Option<String> {
    // Take the first non-empty line that looks like a definition body:
    // skip the "From ... [gcide]:" header, the headword line, and blanks.
    let mut saw_header = false;
    for line in stdout.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("From ") && t.contains("[gcide]") {
            saw_header = true;
            continue;
        }
        if !saw_header {
            continue;
        }
        // First substantive line after the header is the headword; the sense
        // text follows. Heuristic: a line containing "Defn:" or the first
        // sentence-like line. Prefer the text after a "Defn:" marker.
        if let Some(idx) = t.find("Defn:") {
            let def = t[idx + 5..].trim();
            if !def.is_empty() {
                return Some(def.to_string());
            }
        }
    }
    None
}

/// Try WordNet then gcide. Returns `(definition, source)` or `None` if both
/// are silent or the binaries are absent. A spawn error (tool not installed)
/// is treated as "no result", never a panic.
pub fn lookup_local(word: &str) -> Option<(String, String)> {
    if let Some(out) = run(&["wn", word, "-over"]) {
        if let Some(def) = parse_wn(&out) {
            return Some((def, "wordnet".to_string()));
        }
    }
    if let Some(out) = run_dict(word) {
        if let Some(def) = parse_gcide(&out) {
            return Some((def, "gcide".to_string()));
        }
    }
    None
}

fn run(args: &[&str]) -> Option<String> {
    let (cmd, rest) = args.split_first()?;
    let output = Command::new(cmd).args(rest).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_dict(word: &str) -> Option<String> {
    let output = Command::new("dict")
        .args(["-d", "gcide", word])
        .output()
        .ok()?;
    // dict exits non-zero (20/21) when the word is not found — that is a
    // legitimate "no definition", not an error to log.
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wn_extracts_first_gloss() {
        let out = "\nOverview of noun brave\n\nThe noun brave has 1 sense\n\n1. (2) brave, courageous -- (a North American Indian warrior)\n";
        assert_eq!(
            parse_wn(out).as_deref(),
            Some("a North American Indian warrior")
        );
    }

    #[test]
    fn parse_wn_none_when_no_gloss() {
        assert_eq!(parse_wn("No information available for word\n"), None);
    }

    #[test]
    fn parse_gcide_extracts_defn() {
        let out = "1 definition found\n\nFrom The Collaborative International Dictionary of English v.0.48 [gcide]:\n\n  Brave \\Brave\\, a.\n     Defn: Bold; courageous; daring; intrepid.\n";
        assert_eq!(
            parse_gcide(out).as_deref(),
            Some("Bold; courageous; daring; intrepid.")
        );
    }

    #[test]
    fn parse_gcide_none_when_empty() {
        assert_eq!(parse_gcide(""), None);
    }
}
