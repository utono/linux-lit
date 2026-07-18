//! Cross-work semantic echo persistence: the `echo_turns` / `echo_links`
//! tables and the embedding-similarity search that ranks candidate echoes.
//! Extracted verbatim from `queries.rs` (audit #84) — it shared none of that
//! file's imports, taking only `Connection` + the `db::affect` / `db::echo_channel`
//! siblings, so it lives here as its own module along the echo subsystem seam.

use rusqlite::{Connection, OptionalExtension};


/// A candidate cross-work echo found by semantic search.
#[derive(Debug, Clone)]
pub struct EchoCandidate {
    pub work_abbrev: String,
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64,
    pub speaker: String,
    pub passage_type: String,
    pub passage_text: String,
    pub similarity: f32,
}

/// Decode a stored embedding blob (little-endian f32 values) into a vector.
fn decode_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return -1.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return -1.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Find the top-N passages most similar to the query, excluding the source
/// work. Ranks by a blend of semantic cosine and an optional affect (NRC-VAD)
/// axis: `score = (1 - w) * semantic + w * affect`, where `w` is
/// `affect_weight` in [0, 1].
///
/// `query_text` is the raw highlighted passage text (NOT the enriched
/// "SPEAKER to ADDRESSEE: ..." string) — its VAD is computed locally so the
/// speaker labels don't skew the affect score, matching the document side.
///
/// At `affect_weight == 0.0` (the default), the affect axis is skipped
/// entirely and the ranking is byte-for-byte the pure semantic ranking. The
/// affect axis is also skipped if the lexicon is unavailable or a candidate
/// has no stored `sentiment` blob.
pub fn find_similar_passages(
    conn: &Connection,
    query_embedding: &[f32],
    query_text: &str,
    exclude_work: &str,
    top_n: usize,
    affect_weight: f32,
) -> Result<Vec<EchoCandidate>, rusqlite::Error> {
    // `passage_embeddings` is keyed by base-work abbrevs, so exclude the
    // canonical base — a variant edition (`Cym-BBC`) must not surface its own
    // base work (`Cym`) as an "echo" of itself.
    let base_exclude = crate::db::queries::canonical_work_abbrev(conn, exclude_work);

    // Only engage the affect axis when it's both requested and possible.
    let affect_on = affect_weight > 0.0 && crate::db::affect::lexicon_available();
    let query_vad = if affect_on {
        crate::db::affect::compute_vad(query_text)
    } else {
        None
    };

    let mut stmt = conn.prepare(
        "SELECT work_abbrev, div1, div2, start_line, speaker, passage_type, passage_text, embedding, sentiment \
         FROM passage_embeddings \
         WHERE work_abbrev != ?1",
    )?;

    let rows = stmt.query_map([base_exclude], |row| {
        let blob: Vec<u8> = row.get(7)?;
        let sentiment: Option<Vec<u8>> = row.get(8)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            blob,
            sentiment,
        ))
    })?;

    let mut candidates: Vec<EchoCandidate> = Vec::new();
    for row in rows {
        let (work_abbrev, div1, div2, start_line, speaker, passage_type, passage_text, blob, sentiment) =
            row?;
        let emb = decode_embedding(&blob);
        let sim = cosine_similarity(query_embedding, &emb);

        // Blend in the affect cosine when active and both sides have a vector.
        // If anything is missing for this candidate, fall back to pure semantic
        // similarity for it rather than penalizing it.
        let score = match (query_vad, sentiment.as_deref().and_then(crate::db::affect::decode_sentiment)) {
            (Some(qv), Some(cv)) => {
                let affect = crate::db::affect::affect_cosine(&qv, &cv);
                (1.0 - affect_weight) * sim + affect_weight * affect
            }
            _ => sim,
        };

        candidates.push(EchoCandidate {
            work_abbrev,
            div1,
            div2,
            start_line,
            speaker,
            passage_type,
            passage_text,
            similarity: score,
        });
    }

    candidates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(top_n);
    Ok(candidates)
}

// ─── Echo links persistence ─────────────────────────────────────────────────

/// Identifies a turn (the cache key for its echoes).
#[derive(Debug, Clone)]
pub struct EchoTurnKey {
    pub work_abbrev: String,
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64,
    pub end_line: i64,
    pub speaker: String,
    pub turn_text: String,
}

/// A stored echo link (cached search result, possibly curated).
#[derive(Debug, Clone)]
pub struct StoredEchoLink {
    pub link_id: i64,
    pub echo_work_abbrev: String,
    pub echo_div1: i64,
    pub echo_div2: i64,
    pub echo_start_line: i64,
    pub echo_text: String,
    pub similarity: f32,
    pub curated: bool,
    pub rank: i64,
}

/// A turn in a work that has at least one echo link. Used by the
/// echo-turns picker (Ctrl+Shift+G) to list all annotated turns.
#[derive(Debug, Clone)]
pub struct EchoTurnSummary {
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64, // line_in_div of the turn's first line
    pub speaker: String,
    pub turn_text: String,
}

/// List every turn in `work_abbrev` that has >= 1 echo link, in reading
/// order (div1, div2, start_line). The JOIN + GROUP BY guarantees only
/// turns with links appear.
pub fn list_echo_turns_for_work(
    conn: &Connection,
    work_abbrev: &str,
    channel: crate::db::echo_channel::EchoChannel,
) -> Result<Vec<EchoTurnSummary>, rusqlite::Error> {
    let sql = format!(
        "SELECT t.div1, t.div2, t.start_line, t.speaker, t.turn_text \
         FROM echo_turns t \
         JOIN echo_links l ON l.turn_id = t.id \
         WHERE t.work_abbrev = ?1 AND {} \
         GROUP BY t.id \
         ORDER BY t.div1, t.div2, t.start_line",
        channel.sql_predicate(),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(EchoTurnSummary {
            div1: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            div2: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            start_line: row.get(2)?,
            speaker: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            turn_text: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// Create the echo_turns and echo_links tables if absent.
pub fn ensure_echo_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS echo_turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            work_abbrev TEXT NOT NULL,
            div1 INTEGER,
            div2 INTEGER,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            speaker TEXT,
            turn_text TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            UNIQUE(work_abbrev, div1, div2, start_line, end_line)
        );
        CREATE TABLE IF NOT EXISTS echo_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            turn_id INTEGER NOT NULL REFERENCES echo_turns(id) ON DELETE CASCADE,
            echo_work_abbrev TEXT NOT NULL,
            echo_div1 INTEGER,
            echo_div2 INTEGER,
            echo_start_line INTEGER,
            echo_text TEXT NOT NULL,
            similarity REAL,
            curated INTEGER NOT NULL DEFAULT 0,
            rank INTEGER NOT NULL,
            UNIQUE(turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_text)
        );
        CREATE INDEX IF NOT EXISTS idx_echo_links_turn ON echo_links(turn_id);"
    )?;
    // Migration: add echo_start_line to pre-existing echo_links tables.
    // Ignore the "duplicate column" error if it already exists.
    let _ = conn.execute("ALTER TABLE echo_links ADD COLUMN echo_start_line INTEGER", []);
    Ok(())
}

/// Find a cached turn whose line range CONTAINS the given line, for a work.
///
/// BCP echo_turns are keyed by chunk boundaries (start_line..end_line spanning
/// several physical lines), so a reader's cursor on a single line inside a chunk
/// won't match `find_echo_turn`'s exact start/end. This range lookup resolves the
/// containing chunk. Returns (turn_id, start_line, end_line, speaker, turn_text)
/// so the caller can build a full EchoSession. Prefers the smallest matching
/// span if chunks ever overlap.
pub fn find_echo_turn_containing(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    line: i64,
) -> Result<Option<(i64, i64, i64, Option<String>, String)>, rusqlite::Error> {
    conn.query_row(
        "SELECT id, start_line, end_line, speaker, turn_text FROM echo_turns \
         WHERE work_abbrev = ?1 AND div1 = ?2 \
           AND start_line <= ?3 AND end_line >= ?3 \
         ORDER BY (end_line - start_line) ASC LIMIT 1",
        rusqlite::params![work_abbrev, div1, line],
        |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        )),
    )
    .optional()
}

/// Find a cached turn row id by its key.
pub fn find_echo_turn(conn: &Connection, key: &EchoTurnKey) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT id FROM echo_turns \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND start_line = ?4 AND end_line = ?5",
        rusqlite::params![key.work_abbrev, key.div1, key.div2, key.start_line, key.end_line],
        |row| row.get::<_, i64>(0),
    )
    .optional()
}

/// Insert (or fetch existing) the turn row, returning its id.
pub fn save_echo_turn(conn: &Connection, key: &EchoTurnKey) -> Result<i64, rusqlite::Error> {
    if let Some(id) = find_echo_turn(conn, key)? {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO echo_turns (work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            key.work_abbrev, key.div1, key.div2, key.start_line, key.end_line,
            key.speaker, key.turn_text
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Load all echo links for a turn, curated first then by rank.
pub fn load_echo_links(conn: &Connection, turn_id: i64, channel: crate::db::echo_channel::EchoChannel) -> Result<Vec<StoredEchoLink>, rusqlite::Error> {
    // JOIN echo_turns so the channel predicate can see the turn's work_abbrev
    // (the BCP channel is "either side is BCP", not just the link side).
    let sql = format!(
        "SELECT l.id, l.echo_work_abbrev, COALESCE(l.echo_div1, 0), COALESCE(l.echo_div2, 0), \
                COALESCE(l.echo_start_line, 0), l.echo_text, \
                COALESCE(l.similarity, 0.0), l.curated, l.rank \
         FROM echo_links l JOIN echo_turns t ON t.id = l.turn_id \
         WHERE l.turn_id = ?1 AND {} \
         ORDER BY l.curated DESC, l.rank ASC",
        channel.sql_predicate(),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([turn_id], |row| {
        Ok(StoredEchoLink {
            link_id: row.get(0)?,
            echo_work_abbrev: row.get(1)?,
            echo_div1: row.get(2)?,
            echo_div2: row.get(3)?,
            echo_start_line: row.get(4)?,
            echo_text: row.get(5)?,
            similarity: row.get::<_, f64>(6)? as f32,
            curated: row.get::<_, i64>(7)? != 0,
            rank: row.get(8)?,
        })
    })?;
    rows.collect()
}

/// Insert echo links for a turn. Ignores duplicates (UNIQUE constraint).
/// Tuple: (work, div1, div2, start_line, text, similarity, rank).
pub fn insert_echo_links(
    conn: &Connection,
    turn_id: i64,
    links: &[(String, i64, i64, i64, String, f32, i64)],
) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(
        "INSERT OR IGNORE INTO echo_links \
         (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
    )?;
    for (work, d1, d2, sl, text, sim, rank) in links {
        stmt.execute(rusqlite::params![turn_id, work, d1, d2, sl, text, *sim as f64, rank])?;
    }
    Ok(())
}

/// Toggle the curated flag on a link, returning the new state.
pub fn toggle_echo_curated(conn: &Connection, link_id: i64) -> Result<bool, rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET curated = 1 - curated WHERE id = ?1",
        [link_id],
    )?;
    conn.query_row(
        "SELECT curated FROM echo_links WHERE id = ?1",
        [link_id],
        |row| row.get::<_, i64>(0).map(|v| v != 0),
    )
}

/// Insert a manual curated echo link at the top of the curated group (rank 0),
/// shifting existing curated ranks down. Returns the new link's id.
pub fn add_curated_echo_link(
    conn: &Connection,
    turn_id: i64,
    work: &str,
    div1: i64,
    div2: i64,
    line_in_div: i64,
    text: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = rank + 1 WHERE turn_id = ?1 AND curated = 1",
        [turn_id],
    )?;
    conn.execute(
        "INSERT INTO echo_links \
         (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0.0, 1, 0)",
        rusqlite::params![turn_id, work, div1, div2, line_in_div, text],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Set a link's rank and curated flag.
pub fn set_echo_link_rank(conn: &Connection, link_id: i64, rank: i64, curated: bool) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = ?2, curated = ?3 WHERE id = ?1",
        rusqlite::params![link_id, rank, curated as i64],
    )?;
    Ok(())
}

/// Delete all non-curated links for a turn (used by refresh).
pub fn delete_noncurated_echo_links(conn: &Connection, turn_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM echo_links WHERE turn_id = ?1 AND curated = 0",
        [turn_id],
    )?;
    Ok(())
}

/// Delete every link (curated and non-curated) for a turn.
pub fn delete_all_echo_links(conn: &Connection, turn_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM echo_links WHERE turn_id = ?1", [turn_id])?;
    Ok(())
}

/// Delete a single echo link by id.
pub fn delete_echo_link(conn: &Connection, link_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM echo_links WHERE id = ?1", [link_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_echo_link_rank_updates_rank_and_curated() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY, turn_id INTEGER, echo_work_abbrev TEXT,
                echo_div1 INTEGER, echo_div2 INTEGER, echo_start_line INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER
             );
             INSERT INTO echo_links (id, turn_id, echo_work_abbrev, echo_div1, echo_div2,
                echo_start_line, echo_text, similarity, curated, rank)
                VALUES (1, 7, 'Ham', 1, 1, 1, 'x', 0.0, 0, 5);",
        ).unwrap();
        set_echo_link_rank(&conn, 1, 2, true).unwrap();
        let (rank, curated): (i64, i64) = conn.query_row(
            "SELECT rank, curated FROM echo_links WHERE id = 1", [],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(rank, 2);
        assert_eq!(curated, 1);
    }

    #[test]
    fn add_curated_echo_link_inserts_at_top_shifting_curated() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT, turn_id INTEGER, echo_work_abbrev TEXT,
                echo_div1 INTEGER, echo_div2 INTEGER, echo_start_line INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER
             );
             INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2,
                echo_start_line, echo_text, similarity, curated, rank) VALUES
                (7, 'Mac', 5, 5, 19, 'old curated', 0.0, 1, 0),
                (7, 'Lr', 1, 1, 92, 'noncurated', 0.0, 0, 0);",
        ).unwrap();
        let new_id = add_curated_echo_link(&conn, 7, "Ham", 3, 1, 56, "To be").unwrap();
        let (curated, rank): (i64, i64) = conn.query_row(
            "SELECT curated, rank FROM echo_links WHERE id = ?1", [new_id],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((curated, rank), (1, 0));
        let old_rank: i64 = conn.query_row(
            "SELECT rank FROM echo_links WHERE echo_text = 'old curated'", [],
            |r| r.get(0)).unwrap();
        assert_eq!(old_rank, 1);
        let nc: (i64, i64) = conn.query_row(
            "SELECT curated, rank FROM echo_links WHERE echo_text = 'noncurated'", [],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(nc, (0, 0));
    }

    #[test]
    fn list_echo_turns_for_work_returns_only_linked_turns_in_reading_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT, work_abbrev TEXT NOT NULL,
                div1 INTEGER, div2 INTEGER, start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL, speaker TEXT, turn_text TEXT NOT NULL
             );
             CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT, turn_id INTEGER NOT NULL,
                echo_work_abbrev TEXT, echo_div1 INTEGER, echo_div2 INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER,
                echo_start_line INTEGER
             );
             -- Two Hamlet turns with links, one without; one turn in another work.
             INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text)
                VALUES
                (1, 'Ham', 3, 1, 56, 60, 'HAMLET', 'To be or not to be'),
                (2, 'Ham', 1, 2, 10, 12, 'HAMLET', 'O that this too too'),
                (3, 'Ham', 5, 1, 1, 2, 'GHOST', 'no links here'),
                (4, 'Mac', 1, 1, 1, 2, 'MACBETH', 'is this a dagger');
             INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_text, curated, rank)
                VALUES
                (1, 'Mac', 'echo a', 0, 0),
                (1, 'Lr', 'echo b', 1, 1),
                (2, 'Mac', 'echo c', 0, 0),
                (4, 'Ham', 'echo d', 0, 0);",
        ).unwrap();

        let rows = list_echo_turns_for_work(&conn, "Ham", crate::db::echo_channel::EchoChannel::Shakespeare).unwrap();
        // Turn 3 (no links) and turn 4 (other work) excluded -> only 2 rows.
        assert_eq!(rows.len(), 2);
        // Reading order: (1,2,10) before (3,1,56) -> turn 2 first, then turn 1.
        assert_eq!(rows[0].speaker, "HAMLET");
        assert_eq!(rows[0].div1, 1);
        assert_eq!(rows[0].div2, 2);
        assert_eq!(rows[0].start_line, 10);
        assert_eq!(rows[1].div1, 3);
        assert_eq!(rows[1].start_line, 56);
        assert_eq!(rows[1].turn_text, "To be or not to be");
    }

    #[test]
    fn load_echo_links_filters_by_channel() {
        use crate::db::echo_channel::EchoChannel;
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'Ham',5,1,1,4,'Clown','a')", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'BCP1559',11,NULL,1,'I am the resurrection',0.9,1,0)", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'Mac',1,2,5,'Tomorrow',0.8,0,0)", []).unwrap();
        let bcp = load_echo_links(&conn, 1, EchoChannel::Bcp).unwrap();
        assert_eq!(bcp.len(), 1);
        assert_eq!(bcp[0].echo_work_abbrev, "BCP1559");
        let shx = load_echo_links(&conn, 1, EchoChannel::Shakespeare).unwrap();
        assert_eq!(shx.len(), 1);
        assert_eq!(shx[0].echo_work_abbrev, "Mac");
    }

    #[test]
    fn list_echo_turns_for_work_filters_by_channel() {
        use crate::db::echo_channel::EchoChannel;
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'Ham',5,1,1,4,'Clown','a')", []).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (2,'Ham',1,2,10,12,'Hamlet','b')", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'BCP1559',11,NULL,1,'x',0.9,1,0)", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (2,'Mac',1,2,5,'y',0.8,0,0)", []).unwrap();
        let bcp = list_echo_turns_for_work(&conn, "Ham", EchoChannel::Bcp).unwrap();
        assert_eq!(bcp.len(), 1);
        assert_eq!(bcp[0].start_line, 1);
        let shx = list_echo_turns_for_work(&conn, "Ham", EchoChannel::Shakespeare).unwrap();
        assert_eq!(shx.len(), 1);
        assert_eq!(shx[0].start_line, 10);
    }

    #[test]
    fn find_echo_turn_containing_matches_by_range() {
        // BCP echo_turns span a chunk; a cursor on any line inside resolves it.
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'BCP1559',11,NULL,13,20,NULL,'I AM the resurrection')", []).unwrap();
        // A line inside the chunk resolves.
        let hit = find_echo_turn_containing(&conn, "BCP1559", 11, 15).unwrap();
        assert!(hit.is_some());
        let (id, start, end, speaker, _text) = hit.unwrap();
        assert_eq!((id, start, end), (1, 13, 20));
        assert!(speaker.is_none());
        // A line outside the chunk does not.
        assert!(find_echo_turn_containing(&conn, "BCP1559", 11, 99).unwrap().is_none());
        // Boundaries are inclusive.
        assert!(find_echo_turn_containing(&conn, "BCP1559", 11, 13).unwrap().is_some());
        assert!(find_echo_turn_containing(&conn, "BCP1559", 11, 20).unwrap().is_some());
        // Wrong rite (div1) does not match.
        assert!(find_echo_turn_containing(&conn, "BCP1559", 5, 15).unwrap().is_none());
    }

    #[test]
    fn bcp_channel_includes_bcp_turn_with_shakespeare_echo() {
        // The inverse direction (BCP -> Shakespeare): turn is a BCP work, echo
        // is a Shakespeare work. The two-sided filter must put this in the BCP
        // channel even though echo_work_abbrev is NOT 'BCP%'.
        use crate::db::echo_channel::EchoChannel;
        let conn = Connection::open_in_memory().unwrap();
        ensure_echo_tables(&conn).unwrap();
        conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'BCP1559',11,NULL,1,3,NULL,'I am the resurrection')", []).unwrap();
        conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'Ham',5,1,1,'the grave',0.9,1,0)", []).unwrap();

        // load_echo_links: the Shakespeare echo of a BCP turn is BCP-channel.
        let bcp = load_echo_links(&conn, 1, EchoChannel::Bcp).unwrap();
        assert_eq!(bcp.len(), 1);
        assert_eq!(bcp[0].echo_work_abbrev, "Ham");
        // ...and NOT in the Shakespeare channel.
        assert_eq!(load_echo_links(&conn, 1, EchoChannel::Shakespeare).unwrap().len(), 0);

        // list_echo_turns_for_work: the BCP work's turn shows in the BCP channel.
        let turns = list_echo_turns_for_work(&conn, "BCP1559", EchoChannel::Bcp).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].start_line, 1);
        assert_eq!(list_echo_turns_for_work(&conn, "BCP1559", EchoChannel::Shakespeare).unwrap().len(), 0);
    }
}
