//! Read-only access to lit.db's `line_syntax` table — a spaCy dependency parse
//! per token per line, built upstream in litdb (see that repo's
//! docs/superpowers/specs/2026-07-25-line-syntax-layer-design.md).
//!
//! Coverage is PARTIAL by design: 5 of 306 works are parsed. An unparsed work
//! returns an empty vec, which callers treat as "no enrichment available", not
//! as an error. Never gate a feature on this table being populated.

use rusqlite::Connection;

/// One parsed token, in the shape the diagram prompt needs.
#[derive(Debug, Clone)]
pub struct SyntaxToken {
    pub text: String,
    pub pos: String,
    pub dep: String,
    /// `tok_i` of this token's head within its line; self-pointing = ROOT.
    pub head_i: i64,
}

/// Tokens for `line_ids`, ordered by line then token index.
///
/// Returns empty when the lines are unparsed, the table is absent (an older
/// lit.db), or the query fails — all "no enrichment", never fatal.
pub fn load_line_syntax(conn: &Connection, line_ids: &[i64]) -> Vec<SyntaxToken> {
    if line_ids.is_empty() {
        return Vec::new();
    }
    // rusqlite has no array binding; line_ids are i64 from our own DB rows, so
    // formatting them into the IN list is not an injection vector.
    let ids = line_ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT ls.start_char, ls.end_char, ls.pos, ls.dep, ls.head_i, \
                lm.canonical_text \
         FROM line_syntax ls \
         JOIN line_mapping lm ON lm.id = ls.line_mapping_id \
         WHERE ls.line_mapping_id IN ({ids}) \
         ORDER BY ls.line_mapping_id, ls.tok_i"
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            crate::logging::log(&format!("SYNTAX: line_syntax unavailable: {e}"));
            return Vec::new();
        }
    };
    let rows = stmt.query_map([], |row| {
        let start: i64 = row.get(0)?;
        let end: i64 = row.get(1)?;
        let pos: String = row.get(2)?;
        let dep: String = row.get(3)?;
        let head_i: i64 = row.get(4)?;
        let canonical: String = row.get(5)?;
        // Offsets index canonical_text exactly (guaranteed by the upstream
        // backfill), but clamp anyway: a stale parse must not panic the reader.
        let (s, e) = (start.max(0) as usize, end.max(0) as usize);
        let text = if s < e && e <= canonical.len()
            && canonical.is_char_boundary(s) && canonical.is_char_boundary(e)
        {
            canonical[s..e].to_string()
        } else {
            String::new()
        };
        Ok(SyntaxToken { text, pos, dep, head_i })
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).filter(|t| !t.text.is_empty()).collect(),
        Err(e) => {
            crate::logging::log(&format!("SYNTAX: line_syntax query failed: {e}"));
            Vec::new()
        }
    }
}

/// Render tokens as the compact table the diagram prompt embeds. One token per
/// line, tab-separated: `word<TAB>POS<TAB>dep<TAB>head_index`. Compact because
/// a long prose selection can run to hundreds of tokens and every one costs
/// prompt budget.
pub fn tokens_as_table(tokens: &[SyntaxToken]) -> String {
    let mut out = String::from("word\tPOS\tdep\thead\n");
    for (i, t) in tokens.iter().enumerate() {
        out.push_str(&format!("{}\t{}\t{}\t{}\n", t.text, t.pos, t.dep, t.head_i));
        if i >= 600 {
            out.push_str("… (truncated)\n");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_mapping (
               id INTEGER PRIMARY KEY,
               canonical_text TEXT);
             INSERT INTO line_mapping VALUES
               (7, 'a touch'),
               (9, 'make');
             CREATE TABLE line_syntax (
               line_mapping_id INTEGER NOT NULL,
               tok_i INTEGER NOT NULL,
               start_char INTEGER NOT NULL,
               end_char INTEGER NOT NULL,
               pos TEXT NOT NULL,
               tag TEXT,
               dep TEXT NOT NULL,
               head_i INTEGER NOT NULL,
               lemma TEXT,
               PRIMARY KEY (line_mapping_id, tok_i));
             INSERT INTO line_syntax VALUES
               (7, 0, 0, 1, 'DET',  'DT', 'det',   1, 'a'),
               (7, 1, 2, 7, 'NOUN', 'NN', 'nsubj', 2, 'touch'),
               (9, 0, 0, 4, 'VERB', 'VB', 'ROOT',  0, 'make');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn loads_tokens_in_line_and_token_order() {
        let toks = load_line_syntax(&db(), &[9, 7]);
        assert_eq!(toks.len(), 3);
        // Ordered by line id then tok_i, regardless of argument order.
        assert_eq!(toks[0].pos, "DET");
        assert_eq!(toks[1].dep, "nsubj");
        assert_eq!(toks[2].pos, "VERB");
    }

    #[test]
    fn unparsed_lines_yield_no_tokens() {
        assert!(load_line_syntax(&db(), &[404]).is_empty());
    }

    #[test]
    fn empty_input_does_not_query() {
        assert!(load_line_syntax(&db(), &[]).is_empty());
    }

    #[test]
    fn missing_table_is_empty_not_an_error() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(load_line_syntax(&conn, &[7]).is_empty());
    }
}
