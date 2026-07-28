use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct JournalPage {
    pub id: i64,
    pub div1: i64,
    pub div2: i64,
    pub question: String,
    pub answer: String,
    pub claude_model: String,
    pub timestamp: String,
    pub start_citation: Option<String>,
    pub end_citation: Option<String>,
    pub source_text: Option<String>,
    pub kind: String,
}

/// A journal page plus the work it belongs to — used by cross-work term browse
/// (the term query spans all works, so the abbrev is not a fixed parameter).
#[derive(Debug, Clone)]
pub struct TermMatch {
    pub page: JournalPage,
    pub work_abbrev: String,
}

/// The SELECT column list every `find_*` query uses, in the order
/// `map_journal_page_row` reads. Kept as one const so the column list and the
/// row mapper cannot drift apart.
const JOURNAL_PAGE_COLUMNS: &str =
    "id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp, \
     start_citation, end_citation, source_text, COALESCE(kind, 'qa')";

/// Build a `JournalPage` from a row selected with `JOURNAL_PAGE_COLUMNS`.
fn map_journal_page_row(row: &rusqlite::Row<'_>) -> Result<JournalPage, rusqlite::Error> {
    Ok(JournalPage {
        id: row.get(0)?,
        div1: row.get(1)?,
        div2: row.get(2)?,
        question: row.get(3)?,
        answer: row.get(4)?,
        claude_model: row.get(5)?,
        timestamp: row.get(6)?,
        start_citation: row.get(7)?,
        end_citation: row.get(8)?,
        source_text: row.get(9)?,
        kind: row.get(10)?,
    })
}

pub fn ensure_journal_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal_entries (
            id          INTEGER PRIMARY KEY,
            work_abbrev TEXT    NOT NULL,
            div1        INTEGER NOT NULL,
            div2        INTEGER NOT NULL,
            question    TEXT    NOT NULL,
            answer      TEXT    NOT NULL,
            claude_model TEXT,
            scope       TEXT    NOT NULL DEFAULT 'scene',
            start_citation TEXT,
            end_citation   TEXT,
            source_text    TEXT,
            kind        TEXT    NOT NULL DEFAULT 'qa',
            timestamp   TEXT    NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journal_work_scene
            ON journal_entries(work_abbrev, div1, div2, timestamp);",
    )?;
    // Idempotent migration for any DB whose table predates the scope column.
    // All column-existence probes share the `column_exists` helper (audit #67).
    use crate::db::queries::column_exists;
    if !column_exists(conn, "journal_entries", "scope")? {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN scope TEXT NOT NULL DEFAULT 'scene';",
        )?;
    }
    for col in ["start_citation", "end_citation", "source_text"] {
        if !column_exists(conn, "journal_entries", col)? {
            conn.execute_batch(&format!(
                "ALTER TABLE journal_entries ADD COLUMN {col} TEXT;"
            ))?;
        }
    }
    if !column_exists(conn, "journal_entries", "kind")? {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN kind TEXT NOT NULL DEFAULT 'qa';",
        )?;
    }
    if !column_exists(conn, "journal_entries", "word")? {
        conn.execute_batch("ALTER TABLE journal_entries ADD COLUMN word TEXT;")?;
    }
    ensure_journal_tags(conn)?;
    Ok(())
}

/// Idempotent create of the durable rewrite-revision history table. lit.db's
/// schema is owned by litdb (see add_rewrite_revisions.sql); this mirrors it so
/// the app works before litdb re-runs, exactly like `ensure_journal_table`.
pub fn ensure_rewrite_revisions_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS rewrite_revisions (
            id           INTEGER PRIMARY KEY,
            kind         TEXT    NOT NULL,
            entry_id     INTEGER NOT NULL,
            question     TEXT,
            body         TEXT    NOT NULL,
            claude_model TEXT,
            prompt       TEXT,
            timestamp    TEXT    NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_rewrite_revisions_entry
            ON rewrite_revisions(kind, entry_id, timestamp);",
    )?;
    Ok(())
}

/// One stored prior version of a journal Q&A or gloss.
#[derive(Debug, Clone)]
pub struct Revision {
    pub id: i64,
    pub question: Option<String>,
    pub body: String,
    pub claude_model: String,
    pub prompt: Option<String>,
    pub timestamp: String,
}

/// Append a pre-rewrite version to the durable history. `kind` is 'journal' or
/// 'gloss'; `question` is Some only for journal. `prompt` is the custom
/// instruction that produced the version REPLACING this one.
pub fn append_revision(
    conn: &Connection,
    kind: &str,
    entry_id: i64,
    question: Option<&str>,
    body: &str,
    claude_model: &str,
    prompt: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO rewrite_revisions (kind, entry_id, question, body, claude_model, prompt)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![kind, entry_id, question, body, claude_model, prompt],
    )?;
    Ok(())
}

/// All stored revisions for an entry, oldest → newest.
pub fn list_revisions(
    conn: &Connection,
    kind: &str,
    entry_id: i64,
) -> Result<Vec<Revision>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, question, body, claude_model, prompt, timestamp
         FROM rewrite_revisions
         WHERE kind = ?1 AND entry_id = ?2
         ORDER BY timestamp ASC, id ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![kind, entry_id], |r| {
        Ok(Revision {
            id: r.get(0)?,
            question: r.get(1)?,
            body: r.get(2)?,
            claude_model: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            prompt: r.get(4)?,
            timestamp: r.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn ensure_journal_tags(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal_tags (
            entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE CASCADE,
            term   TEXT NOT NULL,
            source TEXT,
            PRIMARY KEY (entry_id, term)
        );
        CREATE INDEX IF NOT EXISTS idx_journal_tags_term ON journal_tags(term);
        CREATE VIRTUAL TABLE IF NOT EXISTS journal_fts USING fts5(
            question, answer, content='journal_entries', content_rowid='id'
        );
        CREATE TRIGGER IF NOT EXISTS journal_entries_ai AFTER INSERT ON journal_entries BEGIN
            INSERT INTO journal_fts(rowid, question, answer)
            VALUES (new.id, new.question, new.answer);
        END;
        CREATE TRIGGER IF NOT EXISTS journal_entries_ad AFTER DELETE ON journal_entries BEGIN
            INSERT INTO journal_fts(journal_fts, rowid, question, answer)
            VALUES ('delete', old.id, old.question, old.answer);
        END;
        CREATE TRIGGER IF NOT EXISTS journal_entries_au AFTER UPDATE ON journal_entries BEGIN
            INSERT INTO journal_fts(journal_fts, rowid, question, answer)
            VALUES ('delete', old.id, old.question, old.answer);
            INSERT INTO journal_fts(rowid, question, answer)
            VALUES (new.id, new.question, new.answer);
        END;",
    )
}

pub fn save_journal_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
    scope: &str,
    kind: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope, kind, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
        rusqlite::params![work_abbrev, div1, div2, question, answer, claude_model, scope, kind],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_journal_pages(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND scope = 'scene'
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, div1, div2], map_journal_page_row)?;
    rows.collect()
}

/// All pages that belong to a scene/chapter BAND: both scene Q&As and the
/// passage Q&As anchored in the same `(div1, div2)`, ordered by creation time.
/// A passage Q&A belongs to its scene band (the band the reader was in when the
/// passage was selected), so the journal overlay pages through scene + passage
/// Q&As together via `Ctrl+n/p`. `find_journal_pages` (scene only) is still used
/// by the ask-save reload path; this is the band-render path.
pub fn find_scene_band_pages(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND scope IN ('scene', 'passage')
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, div1, div2], map_journal_page_row)?;
    rows.collect()
}

/// The band's most recently CREATED scene-scoped entry — the synopsis `\`
/// target. Scene-scoped ONLY: passage entries belong to a span inside the
/// band, and the reader's `\` already reaches those from the passage they
/// cover (see the 2026-07-27 segment-scoping change).
///
/// "Newest" is newest CREATED. `timestamp` is a creation stamp; journal
/// entries have no last-viewed tracking, and this deliberately does not add
/// any. `id DESC` breaks ties for rows written in the same second.
pub fn find_newest_scene_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Option<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND scope = 'scene'
         ORDER BY timestamp DESC, id DESC
         LIMIT 1",
    ))?;
    let mut rows = stmt.query_map(
        rusqlite::params![work_abbrev, div1, div2],
        map_journal_page_row,
    )?;
    rows.next().transpose()
}

/// The (div1, div2) sentinel that marks an author/corpus-scope journal row.
/// Distinct from JOURNAL_WORK_DIV (-1,-1) so author rows never collide with
/// whole-work rows. `work_abbrev` holds the AUTHOR string for these rows.
pub const AUTHOR_DIV: (i64, i64) = (-2, -2);

pub fn save_author_page(
    conn: &Connection,
    author: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
    kind: &str,
) -> Result<i64, rusqlite::Error> {
    save_journal_page(
        conn, author, AUTHOR_DIV.0, AUTHOR_DIV.1, question, answer, claude_model, "author", kind,
    )
}

pub fn find_author_pages(
    conn: &Connection,
    author: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'author' \
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![author], map_journal_page_row)?;
    rows.collect()
}

pub fn find_work_pages(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'work'
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map([work_abbrev], map_journal_page_row)?;
    rows.collect()
}

/// All pages for a work, ordered for the picker: whole-work pages first (by
/// creation time), then scene pages grouped by scene (div1, div2), each scene's
/// pages by creation time. `(scope = 'work')` sorts true(1) before false(0) via
/// DESC so work rows lead.
pub fn find_all_pages_ordered(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries
         WHERE work_abbrev = ?1
         ORDER BY (scope = 'work') DESC, div1 ASC, div2 ASC, timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map([work_abbrev], map_journal_page_row)?;
    rows.collect()
}

pub fn find_pages_by_term(
    conn: &Connection,
    term: &str,
) -> Result<Vec<TermMatch>, rusqlite::Error> {
    let term_norm = term.trim().to_lowercase();
    if term_norm.is_empty() {
        return Ok(Vec::new());
    }
    let order = "ORDER BY (scope = 'work') DESC, div1 ASC, div2 ASC, timestamp ASC, id ASC";

    // 1) tags-first (case-insensitive exact term)
    let tag_sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, work_abbrev \
         FROM journal_entries \
         WHERE id IN (SELECT entry_id FROM journal_tags WHERE LOWER(term) = ?1) \
         {order}"
    );
    let tags: Vec<TermMatch> = {
        let mut stmt = conn.prepare(&tag_sql)?;
        let rows = stmt.query_map([&term_norm], map_term_match_row)?;
        rows.collect::<Result<_, _>>()?
    };
    if !tags.is_empty() {
        return Ok(tags);
    }

    // 2) FTS5 fallback (phrase-quoted so multi-word terms match as a phrase).
    // Strip embedded quotes so the term can't break out of the phrase; an
    // all-punctuation term collapses to an empty phrase that simply matches
    // nothing (returns Ok([]), the "no matches" toast — never an FTS error).
    let fts_sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, work_abbrev \
         FROM journal_entries \
         WHERE id IN (SELECT rowid FROM journal_fts WHERE journal_fts MATCH ?1) \
         {order}"
    );
    let phrase = format!("\"{}\"", term_norm.replace('"', ""));
    let mut stmt = conn.prepare(&fts_sql)?;
    let rows = stmt.query_map([&phrase], map_term_match_row)?;
    rows.collect()
}

/// Fetch a single journal entry (plus its work_abbrev) by id, for the Ctrl+f
/// cross-corpus search jump. Returns the same `TermMatch` shape the term-browse
/// filter renders, so the caller can drop it straight into `JournalFilter` and
/// reuse `render_filtered_match` (no new rendering path). `Ok(None)` if the id
/// no longer exists (entry deleted between popup-load and Enter).
pub fn find_page_by_id(
    conn: &Connection,
    id: i64,
) -> Result<Option<TermMatch>, rusqlite::Error> {
    let sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, work_abbrev \
         FROM journal_entries WHERE id = ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([id], map_term_match_row)?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

/// The most-recently-created (or -edited) journal entries across ALL works,
/// newest-first, capped at `limit`. Carries `work_abbrev` like `find_page_by_id`
/// so the cross-work open path can resolve each entry's edition. `id DESC` is the
/// monotonic tiebreaker when two rows share a `timestamp`. Feeds the recent-Q&A
/// jump-back picker (Alt+j). No `timestamp`-only index today; a small `LIMIT`
/// scan is cheap at current table sizes — revisit only if the table grows large.
pub fn find_recent_pages(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<TermMatch>, rusqlite::Error> {
    let sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, work_abbrev \
         FROM journal_entries \
         ORDER BY timestamp DESC, id DESC \
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([limit], map_term_match_row)?;
    rows.collect()
}

fn map_term_match_row(row: &rusqlite::Row<'_>) -> Result<TermMatch, rusqlite::Error> {
    let page = map_journal_page_row(row)?;
    // work_abbrev is the column AFTER the JOURNAL_PAGE_COLUMNS list (index 11).
    let work_abbrev: String = row.get(11)?;
    Ok(TermMatch { page, work_abbrev })
}

/// Distinct journal-tag terms, sorted ascending. Feeds the term-browse input's
/// suggestion list (the `f` key inside the journal overlay).
pub fn find_distinct_terms(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT DISTINCT term FROM journal_tags ORDER BY term ASC")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// The distinct terms tagged on a single journal entry, sorted ascending.
/// Complements `find_pages_by_term` (term→entries): this is entry→terms, used
/// to ground the improve-question rewrite on what the entry actually explains.
pub fn terms_for_entry(conn: &Connection, entry_id: i64) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT term FROM journal_tags WHERE entry_id = ?1 ORDER BY term ASC")?;
    let rows = stmt.query_map([entry_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Replace this entry's auto-generated tags with `terms` in one transaction:
/// delete rows whose source is 'backfill' or 'reader-auto', then insert `terms`
/// with source 'reader-auto'. Tags with any other source (e.g. 'manual') are
/// preserved. An empty `terms` slice just clears the auto rows.
pub fn replace_auto_tags(
    conn: &Connection,
    entry_id: i64,
    terms: &[String],
) -> Result<(), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM journal_tags \
         WHERE entry_id = ?1 AND source IN ('backfill', 'reader-auto')",
        rusqlite::params![entry_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO journal_tags (entry_id, term, source) \
             VALUES (?1, ?2, 'reader-auto')",
        )?;
        for term in terms {
            stmt.execute(rusqlite::params![entry_id, term])?;
        }
    }
    tx.commit()
}

pub fn save_passage_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_citation: &str,
    end_citation: &str,
    source_text: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope,
             start_citation, end_citation, source_text, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'passage', ?7, ?8, ?9, datetime('now'))",
        rusqlite::params![
            work_abbrev, div1, div2, question, answer, claude_model,
            start_citation, end_citation, source_text
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_passage_pages(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'passage' \
           AND start_citation = ?2 AND end_citation = ?3 \
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(
        rusqlite::params![work_abbrev, start_citation, end_citation],
        map_journal_page_row,
    )?;
    rows.collect()
}

/// Insert a vocab-word journal Q&A: passage scope anchored to the cursor
/// segment, `kind='vocab'`, with the word stored for exact reuse lookup.
pub fn save_vocab_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_citation: &str,
    end_citation: &str,
    source_text: &str,
    word: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope,
             start_citation, end_citation, source_text, kind, word, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'passage', ?7, ?8, ?9, 'vocab', ?10,
                 datetime('now'))",
        rusqlite::params![
            work_abbrev, div1, div2, question, answer, claude_model,
            start_citation, end_citation, source_text, word
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The most recent vocab Q&A for `word` in the segment's `(div1, div2)`, or
/// None. Pressing R with a hit renders the stored answer — no duplicate ask.
pub fn find_vocab_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    word: &str,
) -> Result<Option<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
           AND kind = 'vocab' AND word = ?4 \
         ORDER BY timestamp DESC, id DESC LIMIT 1",
    ))?;
    let mut rows = stmt.query_map(
        rusqlite::params![work_abbrev, div1, div2, word],
        map_journal_page_row,
    )?;
    rows.next().transpose()
}

/// Distinct `(start_citation, end_citation)` ranges of every passage-scope
/// Q&A entry for a work. Feeds the main-card line tint: a line covered by a
/// journal passage Q&A is colored exactly like a reader-glossed line
/// (`apply_reader_gloss_highlighting`). Callers pass `Work.canonical_abbrev`,
/// like every other journal path.
pub fn find_passage_citation_ranges(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT start_citation, end_citation FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'passage'
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

/// The passage-scope Q&A entry whose `[start_citation, end_citation]` line
/// range contains the cursor line `(div1, div2, line_in_div)`, or `None`. Used
/// by reader Ctrl+j so a cursor sitting inside a passage Q&A's span opens the
/// overlay LANDED on that entry (via `land_on_page`), instead of falling to the
/// scene-band top / the picker. Callers pass `Work.canonical_abbrev`, like every
/// other journal path.
///
/// A passage never crosses a scene (start and end share `(div1, div2)`), so we
/// filter on the stored `div1`/`div2` columns and compare only the trailing line
/// number of each citation — extracted with the shared trailing-line-number CAST
/// idiom (`rtrim` strips the digits, `replace` deletes that prefix, leaving the
/// number). The range test is inclusive on both ends.
///
/// Priority when several passages contain the line (e.g. a narrow passage nested
/// in a wider one): NEAREST START — the largest `start_line <= line_in_div` wins
/// (`ORDER BY start_line DESC`), so the most specific containing entry is chosen.
/// Ties (same start line) break to the NEWEST entry (`id DESC`). Returns the
/// entry's own `(div1, div2, id)`; the caller builds `JournalBand::Scene(div1,
/// div2)` and lands on `id`.
pub fn find_journal_page_for_line(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    line_in_div: i64,
) -> Result<Option<(i64, i64, i64)>, rusqlite::Error> {
    // Match on the CITATION's own parsed address, not the entry's stored band
    // `(div1, div2)`: a litdb re-import can renumber an edition's chapters
    // (front-matter offset), leaving old entries banded under one numbering
    // while their citations — written from the reading cursor — address the
    // other. The band columns still say where the entry is FILED, so return
    // them for `land_on_page`; the citation says where the passage LIVES.
    let mut stmt = conn.prepare(
        "SELECT div1, div2, id, start_citation, end_citation FROM journal_entries \
         WHERE work_abbrev = ?1 \
           AND start_citation IS NOT NULL AND end_citation IS NOT NULL",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    // Priority when several passages contain the line: nearest start (largest
    // start_line <= line wins, so a narrow passage nested in a wider one is
    // preferred), tie-break newest id.
    let mut best: Option<(i64, i64, i64, i64)> = None; // (start_line, id, band_d1, band_d2)
    for row in rows {
        let (bd1, bd2, id, start, end) = row?;
        let (Some((sd1, sd2, sl)), Some((ed1, ed2, el))) = (
            crate::db::models::parse_citation(&start),
            crate::db::models::parse_citation(&end),
        ) else {
            continue;
        };
        if (sd1, sd2) != (div1, div2) || (ed1, ed2) != (div1, div2) {
            continue;
        }
        if !(sl <= line_in_div && line_in_div <= el) {
            continue;
        }
        if best.map_or(true, |(bs, bid, _, _)| (sl, id) > (bs, bid)) {
            best = Some((sl, id, bd1, bd2));
        }
    }
    Ok(best.map(|(_, id, bd1, bd2)| (bd1, bd2, id)))
}

pub fn find_journal_scenes(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(i64, i64)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT div1, div2 FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'scene'
         ORDER BY div1 ASC, div2 ASC",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn update_journal_page(
    conn: &Connection,
    id: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE journal_entries
         SET question = ?1, answer = ?2, claude_model = ?3, timestamp = datetime('now')
         WHERE id = ?4",
        rusqlite::params![question, answer, claude_model, id],
    )?;
    Ok(())
}

/// Re-target an existing journal entry to a different band by updating its
/// `scope` + `(div1, div2)` in place. Used by the journal overlay's
/// "move to band" action (Ctrl+Shift+J). Does NOT touch question/answer.
/// For the whole-work band pass `scope = "work"` and `div1 = div2 = -1`;
/// for a scene/chapter pass `scope = "scene"` and the scene's `(div1, div2)`.
pub fn move_journal_page(
    conn: &Connection,
    id: i64,
    scope: &str,
    div1: i64,
    div2: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE journal_entries
         SET scope = ?1, div1 = ?2, div2 = ?3
         WHERE id = ?4",
        rusqlite::params![scope, div1, div2, id],
    )?;
    Ok(())
}

pub fn delete_journal_page(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    // Enable FK enforcement on THIS connection only, so `journal_tags`'
    // `ON DELETE CASCADE` fires and the entry's tags are removed with it.
    // Scoped here (not globally in `open_db_rw`) to avoid changing FK behavior
    // for the ~50 other write paths that share that connection (bookmarks,
    // glosses, echo — some of which reference synthetic line_mapping rows).
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.execute("DELETE FROM journal_entries WHERE id = ?1", [id])?;
    Ok(())
}

/// Every journal entry across all works, with body text, for the Ctrl+f
/// cross-corpus search popup. Ordered by work then band for stable display.
pub fn list_all_journal_rows(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Vec<crate::input::corpus_search::JournalRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, work_abbrev, div1, div2, question, answer
         FROM journal_entries
         ORDER BY work_abbrev, div1, div2, id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(crate::input::corpus_search::JournalRow {
                id: r.get(0)?,
                work_abbrev: r.get(1)?,
                div1: r.get(2)?,
                div2: r.get(3)?,
                question: r.get(4)?,
                answer: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_journal_table(&conn).unwrap();
        conn
    }

    #[test]
    fn scene_pages_roundtrip_and_exclude_work() {
        let conn = mem();
        save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "claude-opus-4-8", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "claude-opus-4-8", "scene", "qa").unwrap();
        // A work page in the same work must NOT appear in scene queries.
        save_journal_page(&conn, "Ham", -1, -1, "WQ?", "WA.", "claude-opus-4-8", "work", "qa").unwrap();

        let scene_pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(scene_pages.len(), 2);
        assert_eq!(scene_pages[0].question, "Q1?");
        assert_eq!(scene_pages[1].question, "Q2?");

        // find_journal_scenes lists only scene-scoped rows.
        let scenes = find_journal_scenes(&conn, "Ham").unwrap();
        assert_eq!(scenes, vec![(1, 2)]);
    }

    #[test]
    fn work_pages_roundtrip_and_exclude_scene() {
        let conn = mem();
        save_journal_page(&conn, "Ham", -1, -1, "WQ1?", "WA1.", "claude-opus-4-8", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "WQ2?", "WA2.", "claude-opus-4-8", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", 3, 1, "SQ?", "SA.", "claude-opus-4-8", "scene", "qa").unwrap();

        let work_pages = find_work_pages(&conn, "Ham").unwrap();
        assert_eq!(work_pages.len(), 2);
        assert_eq!(work_pages[0].question, "WQ1?");
        assert_eq!(work_pages[1].question, "WQ2?");

        // A scene query must NOT return work pages.
        assert!(find_journal_pages(&conn, "Ham", -1, -1).unwrap().is_empty());
    }

    #[test]
    fn passage_citation_ranges_distinct_and_scoped() {
        let conn = mem();
        // Two Q&As on the SAME passage → one distinct range.
        save_passage_page(&conn, "Rom", 2, 2, "Rom.2.2.25", "Rom.2.2.25", "Ay me.", "Q1?", "A1.", "m").unwrap();
        save_passage_page(&conn, "Rom", 2, 2, "Rom.2.2.25", "Rom.2.2.25", "Ay me.", "Q2?", "A2.", "m").unwrap();
        save_passage_page(&conn, "Rom", 2, 2, "Rom.2.2.33", "Rom.2.2.36", "O Romeo…", "Q3?", "A3.", "m").unwrap();
        // Scene-scope entry (no citations) and another work must not appear.
        save_journal_page(&conn, "Rom", 2, 2, "SQ?", "SA.", "m", "scene", "qa").unwrap();
        save_passage_page(&conn, "Ham", 1, 2, "Ham.1.2.1", "Ham.1.2.3", "…", "HQ?", "HA.", "m").unwrap();

        let mut ranges = find_passage_citation_ranges(&conn, "Rom").unwrap();
        ranges.sort();
        assert_eq!(
            ranges,
            vec![
                ("Rom.2.2.25".to_string(), "Rom.2.2.25".to_string()),
                ("Rom.2.2.33".to_string(), "Rom.2.2.36".to_string()),
            ]
        );
    }

    #[test]
    fn update_and_delete_still_work() {
        let conn = mem();
        let id1 = save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "m", "scene", "qa").unwrap();

        update_journal_page(&conn, id1, "Q1b?", "A1b.", "m").unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages[0].question, "Q1b?");
        assert_eq!(pages[0].answer, "A1b.");

        delete_journal_page(&conn, id1).unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q2?");
    }

    #[test]
    fn shared_base_abbrev_contract() {
        // 2H6 and 2H6-Amb share a journal because callers always pass the
        // canonical base abbrev (`Work.canonical_abbrev` == "2H6"). This test
        // documents that contract at the DB layer: a page saved under "2H6" is
        // found when querying "2H6".
        let conn = mem();
        save_journal_page(&conn, "2H6", 4, 8, "Q?", "A.", "m", "scene", "qa").unwrap();
        let pages = find_journal_pages(&conn, "2H6", 4, 8).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q?");
    }

    #[test]
    fn all_pages_ordered_work_first_then_scenes() {
        let conn = mem();
        // Insert out of order; expect: work pages (by time), then scene pages
        // grouped by (div1,div2) then by time.
        save_journal_page(&conn, "Ham", 3, 1, "S31a?", "a", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "W1?", "a", "m", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "S12a?", "a", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "W2?", "a", "m", "work", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "S12b?", "a", "m", "scene", "qa").unwrap();

        let ordered = find_all_pages_ordered(&conn, "Ham").unwrap();
        let qs: Vec<&str> = ordered.iter().map(|p| p.question.as_str()).collect();
        assert_eq!(qs, vec!["W1?", "W2?", "S12a?", "S12b?", "S31a?"]);
    }

    #[test]
    fn recent_pages_newest_first_across_works_with_limit_and_id_tiebreak() {
        let conn = mem();
        // Insert across two works, then stamp explicit timestamps (the DEFAULT
        // datetime('now') would collide within the same second). id order is
        // insertion order (id1 < id2 < ...).
        let id_a1 =
            save_journal_page(&conn, "Ham", 1, 2, "Ham-old?", "a", "m", "scene", "qa").unwrap();
        let id_r1 =
            save_journal_page(&conn, "Rom", 2, 2, "Rom-mid?", "a", "m", "scene", "qa").unwrap();
        let id_a2 =
            save_journal_page(&conn, "Ham", 1, 3, "Ham-new-a?", "a", "m", "scene", "qa").unwrap();
        let id_r2 =
            save_journal_page(&conn, "Rom", 3, 1, "Rom-new-b?", "a", "m", "scene", "qa").unwrap();

        let stamp = |id: i64, ts: &str| {
            conn.execute(
                "UPDATE journal_entries SET timestamp = ?2 WHERE id = ?1",
                rusqlite::params![id, ts],
            )
            .unwrap();
        };
        stamp(id_a1, "2026-07-20T10:00:00");
        stamp(id_r1, "2026-07-21T10:00:00");
        // Two rows share the newest timestamp → id DESC breaks the tie
        // (id_r2 > id_a2, so Rom-new-b? leads Ham-new-a?).
        stamp(id_a2, "2026-07-22T10:00:00");
        stamp(id_r2, "2026-07-22T10:00:00");

        // Full ordering: newest-first across both works, id-tiebroken.
        let all = find_recent_pages(&conn, 10).unwrap();
        let got: Vec<(&str, &str)> = all
            .iter()
            .map(|m| (m.work_abbrev.as_str(), m.page.question.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Rom", "Rom-new-b?"),
                ("Ham", "Ham-new-a?"),
                ("Rom", "Rom-mid?"),
                ("Ham", "Ham-old?"),
            ]
        );

        // LIMIT is honored and returns the newest N.
        let top2 = find_recent_pages(&conn, 2).unwrap();
        let q2: Vec<&str> = top2.iter().map(|m| m.page.question.as_str()).collect();
        assert_eq!(q2, vec!["Rom-new-b?", "Ham-new-a?"]);
    }

    #[test]
    fn ensure_table_is_idempotent_and_adds_scope() {
        let conn = mem();
        // Calling again must not error (idempotent ALTER guard).
        ensure_journal_table(&conn).unwrap();
        let has_scope: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='scope'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has_scope);
    }

    #[test]
    fn ensure_rewrite_revisions_table_is_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_rewrite_revisions_table(&conn).unwrap();
        ensure_rewrite_revisions_table(&conn).unwrap(); // second call must not error
        // insert + read back one row
        conn.execute(
            "INSERT INTO rewrite_revisions (kind, entry_id, question, body, claude_model, prompt)
             VALUES ('journal', 34, 'Q?', 'old answer', 'm', 'make it shorter')",
            [],
        )
        .unwrap();
        let body: String = conn
            .query_row(
                "SELECT body FROM rewrite_revisions WHERE entry_id = 34 AND kind = 'journal'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(body, "old answer");
    }

    #[test]
    fn append_and_list_revisions_in_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_rewrite_revisions_table(&conn).unwrap();
        append_revision(&conn, "gloss", 7, None, "v1 markup", "m1", Some("shorten")).unwrap();
        append_revision(&conn, "gloss", 7, None, "v2 markup", "m2", Some("expand")).unwrap();
        // a different entry must not leak in
        append_revision(&conn, "gloss", 8, None, "other", "m", None).unwrap();
        let revs = list_revisions(&conn, "gloss", 7).unwrap();
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].body, "v1 markup");
        assert_eq!(revs[1].body, "v2 markup");
        assert_eq!(revs[0].prompt.as_deref(), Some("shorten"));
    }

    #[test]
    fn passage_pages_roundtrip_and_isolate_from_scene_work() {
        let conn = mem();
        let id = save_passage_page(
            &conn, "2H6", 1, 4, "2H6.1.4.43", "2H6.1.4.50",
            "<speaker>YORK</speaker>\n<segment>Lay hands…</segment>\n<stage>[To Jourdain.]</stage>",
            "What is York doing?", "He arrests the conjurers.", "claude-opus-4-8",
        ).unwrap();
        assert!(id > 0);

        // A scene page and a work page in the same scene must NOT come back as passage pages.
        save_journal_page(&conn, "2H6", 1, 4, "SceneQ?", "SceneA.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "2H6", -1, -1, "WorkQ?", "WorkA.", "m", "work", "qa").unwrap();

        let pages = find_passage_pages(&conn, "2H6", "2H6.1.4.43", "2H6.1.4.50").unwrap();
        assert_eq!(pages.len(), 1, "exactly the one passage page");
        let p = &pages[0];
        assert_eq!(p.question, "What is York doing?");
        assert_eq!(p.start_citation.as_deref(), Some("2H6.1.4.43"));
        assert_eq!(p.end_citation.as_deref(), Some("2H6.1.4.50"));
        assert!(p.source_text.as_deref().unwrap().contains("<stage>[To Jourdain.]</stage>"));

        // The passage page must NOT leak into scene/work queries.
        assert!(find_journal_pages(&conn, "2H6", 1, 4).unwrap().iter().all(|p| p.question != "What is York doing?"));
        assert!(find_work_pages(&conn, "2H6").unwrap().iter().all(|p| p.question != "What is York doing?"));

        // A different citation pair returns nothing.
        assert!(find_passage_pages(&conn, "2H6", "2H6.1.4.99", "2H6.1.4.99").unwrap().is_empty());
    }

    #[test]
    fn journal_page_for_line_finds_containing_passage_and_prefers_nearest_start() {
        let conn = mem();
        // A passage spanning lines 43..=50 in (1,4). A cursor line inside it must
        // resolve to this entry, regardless of which scene band the picker groups
        // it under.
        let id_wide = save_passage_page(
            &conn, "2H6", 1, 4, "2H6.1.4.43", "2H6.1.4.50",
            "<segment>wide…</segment>", "Wide Q?", "Wide A.", "m",
        ).unwrap();
        // A NARROWER, later passage 46..=48 nested inside the wide one. For a line
        // in [46,48], both contain it; the nearest-start rule (largest start_line
        // <= line) prefers the narrow entry.
        let id_narrow = save_passage_page(
            &conn, "2H6", 1, 4, "2H6.1.4.46", "2H6.1.4.48",
            "<segment>narrow…</segment>", "Narrow Q?", "Narrow A.", "m",
        ).unwrap();
        // A single-line passage in a DIFFERENT scene band (2,1) — never a match
        // for scene (1,4).
        save_passage_page(
            &conn, "2H6", 2, 1, "2H6.2.1.5", "2H6.2.1.5",
            "<segment>other…</segment>", "Other Q?", "Other A.", "m",
        ).unwrap();
        // A scene-scope (no-citation) entry in the same band must be ignored.
        save_journal_page(&conn, "2H6", 1, 4, "SceneQ?", "SceneA.", "m", "scene", "qa").unwrap();

        // Line 44: only the wide passage contains it. The query returns the
        // entry's own band coordinates (div1, div2) plus its id.
        let (d1, d2, id) = find_journal_page_for_line(&conn, "2H6", 1, 4, 44).unwrap().unwrap();
        assert_eq!(id, id_wide);
        assert_eq!((d1, d2), (1, 4));

        // Line 47: both contain it; nearest start (46 > 43) wins → narrow.
        let (_d1, _d2, id) = find_journal_page_for_line(&conn, "2H6", 1, 4, 47).unwrap().unwrap();
        assert_eq!(id, id_narrow);

        // Boundary lines are inclusive: 43 (wide start) and 50 (wide end).
        assert_eq!(find_journal_page_for_line(&conn, "2H6", 1, 4, 43).unwrap().unwrap().2, id_wide);
        assert_eq!(find_journal_page_for_line(&conn, "2H6", 1, 4, 50).unwrap().unwrap().2, id_wide);

        // A line outside every passage in the band → None.
        assert!(find_journal_page_for_line(&conn, "2H6", 1, 4, 60).unwrap().is_none());
        // Right scene, but the line belongs to a different band's passage → None
        // (the (2,1) passage must not leak into a (1,4) lookup).
        assert!(find_journal_page_for_line(&conn, "2H6", 1, 4, 5).unwrap().is_none());
    }

    #[test]
    fn journal_page_for_line_ties_break_newest_id() {
        let conn = mem();
        // Two passages with the SAME start line 10, same span. The nearest-start
        // rule can't decide; the tie-break prefers the newest id.
        let _older = save_passage_page(
            &conn, "Ham", 1, 2, "Ham.1.2.10", "Ham.1.2.12",
            "<segment>a</segment>", "Older Q?", "Older A.", "m",
        ).unwrap();
        let newer = save_passage_page(
            &conn, "Ham", 1, 2, "Ham.1.2.10", "Ham.1.2.12",
            "<segment>b</segment>", "Newer Q?", "Newer A.", "m",
        ).unwrap();
        let (_d1, _d2, id) = find_journal_page_for_line(&conn, "Ham", 1, 2, 11).unwrap().unwrap();
        assert_eq!(id, newer);
    }

    #[test]
    fn journal_page_for_line_matches_citation_divs_not_band_divs() {
        let conn = mem();
        // An edition re-import shifted the chapter numbering: this entry is
        // BANDED under div1=11 but its citation addresses chapter 10
        // (BH.10.0.948). The cursor reads the current edition, where the
        // paragraph is (10, 0, 948). The lookup must match on the CITATION's
        // parsed divs — and still return the stored band (11, 0), which is
        // where land_on_page has to look for the entry.
        let id = save_passage_page(
            &conn, "BH", 11, 0, "BH.10.0.948", "BH.10.0.948",
            "<p>Here, beneath the painted ceiling…</p>", "Q?", "A.", "m",
        ).unwrap();
        let (bd1, bd2, found) =
            find_journal_page_for_line(&conn, "BH", 10, 0, 948).unwrap().unwrap();
        assert_eq!(found, id);
        assert_eq!((bd1, bd2), (11, 0), "returns the stored band, not the citation divs");
        // The band address alone (11, 0, 948) matches nothing: only the
        // citation says where the passage lives in the text.
        assert!(find_journal_page_for_line(&conn, "BH", 11, 0, 948).unwrap().is_none());
    }

    #[test]
    fn scene_band_pages_merge_scene_and_passages_in_time_order() {
        let conn = mem();
        // Scene Q&A first, then two passage Q&As, all in (1, 0) — interleaved
        // with an unrelated scene/passage and a work page that must be excluded.
        save_journal_page(&conn, "BH", 1, 0, "SceneQ?", "SceneA.", "m", "scene", "qa").unwrap();
        save_passage_page(
            &conn, "BH", 1, 0, "BH.1.0.14", "BH.1.0.14",
            "<p>chancery…</p>", "PassQ1?", "PassA1.", "m",
        ).unwrap();
        save_passage_page(
            &conn, "BH", 1, 0, "BH.1.0.18", "BH.1.0.18",
            "<p>fog…</p>", "PassQ2?", "PassA2.", "m",
        ).unwrap();
        // Different scene band — must NOT appear.
        save_journal_page(&conn, "BH", 2, 0, "OtherScene?", "x", "m", "scene", "qa").unwrap();
        save_passage_page(
            &conn, "BH", 2, 0, "BH.2.0.1", "BH.2.0.1", "<p>x</p>", "OtherPass?", "x", "m",
        ).unwrap();
        // Whole-work page — must NOT appear.
        save_journal_page(&conn, "BH", -1, -1, "WorkQ?", "x", "m", "work", "qa").unwrap();

        let pages = find_scene_band_pages(&conn, "BH", 1, 0).unwrap();
        let qs: Vec<&str> = pages.iter().map(|p| p.question.as_str()).collect();
        assert_eq!(qs, vec!["SceneQ?", "PassQ1?", "PassQ2?"]);
        // Passage rows carry their citations; the scene row does not.
        assert!(pages[0].start_citation.is_none());
        assert_eq!(pages[1].start_citation.as_deref(), Some("BH.1.0.14"));
        assert_eq!(pages[2].start_citation.as_deref(), Some("BH.1.0.18"));
    }

    #[test]
    fn passage_columns_migrate_idempotently() {
        let conn = mem();
        ensure_journal_table(&conn).unwrap(); // second call must not error
        for col in ["start_citation", "end_citation", "source_text"] {
            let has: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name=?1").unwrap()
                .exists([col]).unwrap();
            assert!(has, "column {col} should exist after ensure_journal_table");
        }
    }

    #[test]
    fn kind_defaults_to_qa_and_roundtrips() {
        let conn = mem();
        // Old-style insert path (scene) must default kind to 'qa'.
        let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id, id);
        assert_eq!(pages[0].kind, "qa");
    }

    #[test]
    fn move_page_changes_band_scene_to_work_and_back() {
        let conn = mem();
        let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();

        // Move scene -> work.
        move_journal_page(&conn, id, "work", -1, -1).unwrap();
        assert!(find_journal_pages(&conn, "Ham", 1, 2).unwrap().is_empty());
        let work = find_work_pages(&conn, "Ham").unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].id, id);
        assert_eq!(work[0].div1, -1);
        assert_eq!(work[0].div2, -1);

        // Move work -> a different scene.
        move_journal_page(&conn, id, "scene", 3, 1).unwrap();
        assert!(find_work_pages(&conn, "Ham").unwrap().is_empty());
        let scene = find_journal_pages(&conn, "Ham", 3, 1).unwrap();
        assert_eq!(scene.len(), 1);
        assert_eq!(scene[0].id, id);
    }

    #[test]
    fn author_pages_roundtrip_and_exclude_work_scene() {
        let conn = mem();
        let nid = save_author_page(&conn, "Shakespeare", "", "## Cry\n\n**load** it", "m", "note").unwrap();
        save_author_page(&conn, "Shakespeare", "Corpus Q?", "Corpus A.", "m", "qa").unwrap();
        // A scene page for an actual work must NOT appear in author queries.
        save_journal_page(&conn, "Ham", 1, 2, "SQ?", "SA.", "m", "scene", "qa").unwrap();

        let pages = find_author_pages(&conn, "Shakespeare").unwrap();
        assert_eq!(pages.len(), 2);
        let note = pages.iter().find(|p| p.id == nid).unwrap();
        assert_eq!(note.kind, "note");
        assert_eq!(note.question, "");
        assert_eq!(note.answer, "## Cry\n\n**load** it");
        assert_eq!(note.div1, -2);
        assert_eq!(note.div2, -2);
    }

    #[test]
    fn move_to_author_band_sets_scope_and_sentinel() {
        let conn = mem();
        let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();
        move_journal_page(&conn, id, "author", AUTHOR_DIV.0, AUTHOR_DIV.1).unwrap();
        // NOTE: move keeps work_abbrev; author-band lookups key by work_abbrev, so a
        // moved-from-a-work page keys under the WORK abbrev, not the author. That's
        // acceptable: the move picker is out of scope for author here (Task 6 does
        // not add an Author move target). This test documents move_journal_page is
        // scope-agnostic and needs no change.
        let n: i64 = conn
            .query_row("SELECT div1 FROM journal_entries WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, -2);
    }

    #[test]
    fn vocab_word_column_migrates_idempotently() {
        let conn = mem();
        ensure_journal_table(&conn).unwrap(); // second call must not error
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='word'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has, "word column should exist after ensure_journal_table");
    }

    #[test]
    fn term_query_tags_first_then_fts_fallback_across_works() {
        let conn = mem();
        // two works; the "fee simple" phrase lives only in an answer on Rom
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (20, 'Rom', 3, 1, 'q', 'A fee simple, in Elizabethan law, is absolute ownership.', 'scene')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (7, '2H6', 1, 1, 'q', 'Nothing about property law here.', 'scene')",
            [],
        ).unwrap();

        // No tags yet -> FTS fallback finds the Rom entry, and carries its work_abbrev.
        let hits = find_pages_by_term(&conn, "fee simple").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page.id, 20);
        assert_eq!(hits[0].work_abbrev, "Rom");

        // Now tag the 2H6 entry with the term -> tags-first path takes precedence
        // and returns the tagged entry (not the FTS match).
        conn.execute(
            "INSERT INTO journal_tags (entry_id, term, source) VALUES (7, 'fee simple', 'backfill')",
            [],
        ).unwrap();
        let hits2 = find_pages_by_term(&conn, "Fee Simple").unwrap(); // case-insensitive
        assert_eq!(hits2.iter().map(|m| m.page.id).collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn term_query_empty_or_pathological_returns_empty_not_error() {
        let conn = mem();
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (1, 'Rom', 1, 1, 'q', 'a fee simple answer', 'scene')",
            [],
        )
        .unwrap();

        // Empty / whitespace-only terms short-circuit to Ok([]) (no query run).
        assert!(find_pages_by_term(&conn, "").unwrap().is_empty());
        assert!(find_pages_by_term(&conn, "   ").unwrap().is_empty());

        // A term of only quote chars strips to an empty FTS phrase (MATCH '""'),
        // which matches nothing -> Ok([]), NOT an FTS syntax error/panic.
        assert!(find_pages_by_term(&conn, "\"\"").unwrap().is_empty());
    }

    #[test]
    fn terms_for_entry_sorted_and_empty() {
        let conn = mem();
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (9,'Rom',3,1,'q','a','scene')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO journal_tags (entry_id, term) VALUES (9,'quibble'),(9,'fee simple')",
            [],
        )
        .unwrap();
        // sorted ascending, scoped to the entry
        assert_eq!(
            terms_for_entry(&conn, 9).unwrap(),
            vec!["fee simple".to_string(), "quibble".to_string()]
        );
        // untagged entry -> empty
        assert!(terms_for_entry(&conn, 999).unwrap().is_empty());
    }

    #[test]
    fn distinct_terms_sorted() {
        let conn = mem();
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (1,'Rom',1,1,'q','a','scene')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO journal_tags (entry_id, term) VALUES (1,'freehold'),(1,'fee simple')",
            [],
        )
        .unwrap();
        assert_eq!(
            find_distinct_terms(&conn).unwrap(),
            vec!["fee simple".to_string(), "freehold".to_string()]
        );
    }

    #[test]
    fn ensure_journal_tags_is_idempotent() {
        let conn = mem();
        ensure_journal_tags(&conn).unwrap(); // second call must not error
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('journal_tags','journal_fts')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn replace_auto_tags_replaces_auto_preserves_manual() {
        let conn = mem();
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (5, 'Rom', 1, 1, 'q', 'a', 'scene')",
            [],
        ).unwrap();
        // Pre-existing tags: one backfill, one reader-auto (both auto), one manual.
        conn.execute(
            "INSERT INTO journal_tags (entry_id, term, source) VALUES \
             (5,'old-backfill','backfill'),(5,'old-auto','reader-auto'),(5,'keepme','manual')",
            [],
        ).unwrap();

        replace_auto_tags(&conn, 5, &["fee simple".to_string(), "freehold".to_string()]).unwrap();

        let mut got: Vec<(String, String)> = conn
            .prepare("SELECT term, source FROM journal_tags WHERE entry_id=5 ORDER BY term")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        got.sort();
        // manual survives; both auto rows gone; two new reader-auto rows present.
        assert_eq!(
            got,
            vec![
                ("fee simple".to_string(), "reader-auto".to_string()),
                ("freehold".to_string(), "reader-auto".to_string()),
                ("keepme".to_string(), "manual".to_string()),
            ]
        );
    }

    #[test]
    fn replace_auto_tags_empty_clears_auto_only() {
        let conn = mem();
        conn.execute(
            "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
             VALUES (6, 'Rom', 1, 1, 'q', 'a', 'scene')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO journal_tags (entry_id, term, source) VALUES \
             (6,'gone','reader-auto'),(6,'stay','manual')",
            [],
        ).unwrap();

        replace_auto_tags(&conn, 6, &[]).unwrap();

        let terms: Vec<String> = conn
            .prepare("SELECT term FROM journal_tags WHERE entry_id=6 ORDER BY term")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(terms, vec!["stay".to_string()]);
    }

    #[test]
    fn vocab_page_roundtrip_and_reuse_lookup() {
        let conn = mem();
        let id = save_vocab_page(
            &conn, "Cym", 3, 2, "Cym.3.2.77", "Cym.3.2.80",
            "A riding suit no costlier than would fit\nA franklin's huswife.",
            "franklin", "\u{201c}franklin\u{201d} in this segment, and across Shakespeare",
            "Imogen prices her disguise\u{2026}", "claude-opus-4-8",
        ).unwrap();
        assert!(id > 0);

        // Exact reuse hit.
        let page = find_vocab_page(&conn, "Cym", 3, 2, "franklin").unwrap().unwrap();
        assert_eq!(page.id, id);
        assert_eq!(page.kind, "vocab");
        assert_eq!(page.source_text.as_deref().unwrap(), "A riding suit no costlier than would fit\nA franklin's huswife.");

        // Different word, different segment, different work: all miss.
        assert!(find_vocab_page(&conn, "Cym", 3, 2, "huswife").unwrap().is_none());
        assert!(find_vocab_page(&conn, "Cym", 3, 3, "franklin").unwrap().is_none());
        assert!(find_vocab_page(&conn, "Ham", 3, 2, "franklin").unwrap().is_none());

        // Most recent wins (same-second timestamps tie-break on id DESC).
        let id2 = save_vocab_page(
            &conn, "Cym", 3, 2, "Cym.3.2.77", "Cym.3.2.80", "src",
            "franklin", "Q2?", "Second answer.", "m",
        ).unwrap();
        assert_eq!(find_vocab_page(&conn, "Cym", 3, 2, "franklin").unwrap().unwrap().id, id2);

        // Vocab rows ride the passage scope into the scene band render.
        let band = find_scene_band_pages(&conn, "Cym", 3, 2).unwrap();
        assert_eq!(band.len(), 2);
        assert!(band.iter().all(|p| p.kind == "vocab"));

        // A plain passage Q&A (kind='qa') must never satisfy the vocab lookup.
        save_passage_page(&conn, "Cym", 3, 4, "Cym.3.4.1", "Cym.3.4.2", "s", "Q?", "A.", "m").unwrap();
        assert!(find_vocab_page(&conn, "Cym", 3, 4, "franklin").unwrap().is_none());
    }

    /// Insert a journal entry with an explicit scope AND a citation span.
    /// `save_journal_page` takes no citations and `save_passage_page` forces
    /// `scope='passage'`, so neither can build the row this bug is about: a
    /// `scope='scene'` entry that still carries a span (what a litdb
    /// re-import leaves behind — 19 such rows in lit.db).
    fn insert_cited(
        conn: &Connection,
        work: &str,
        div1: i64,
        div2: i64,
        scope: &str,
        start: &str,
        end: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO journal_entries
                (work_abbrev, div1, div2, question, answer, claude_model,
                 scope, start_citation, end_citation, source_text)
             VALUES (?1, ?2, ?3, 'Q?', 'A.', 'm', ?4, ?5, ?6, 'src')",
            rusqlite::params![work, div1, div2, scope, start, end],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// The reported bug (2026-07-27, both reports). A `scope='scene'` entry
    /// whose citation span covers the cursor line must be reachable: the `\`
    /// cycle probes through this function, and BH's entries are ALL
    /// scene-filed, so the journal stop was dead on that work.
    #[test]
    fn scene_scoped_entry_with_a_span_is_found() {
        let conn = mem();
        // Mirrors lit.db id 24: filed under band (2,0), citing BH.2.0.48.
        let id = insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.48", "BH.2.0.48");

        let hit = find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap();
        assert_eq!(
            hit,
            Some((2, 0, id)),
            "a scene-filed entry whose span covers the line must be found"
        );
    }

    /// Guard: the fix must not widen into "any entry in the band". An entry
    /// with no citation carries no location, so it stays unreachable by `\`
    /// (Ctrl+j and the picker still reach it).
    #[test]
    fn entry_without_citations_is_still_not_found() {
        let conn = mem();
        save_journal_page(&conn, "BH", 2, 0, "Q?", "A.", "m", "scene", "qa").unwrap();

        assert_eq!(find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap(), None);
    }

    /// Guard: segment scoping is intact. A span that does not cover the
    /// anchor must not match, whatever its scope — this is the 2026-07-27
    /// rule that stopped `\` opening a Q&A about a different passage.
    #[test]
    fn span_not_covering_the_anchor_is_not_found() {
        let conn = mem();
        insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.10", "BH.2.0.20");

        assert_eq!(find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap(), None);
    }

    /// Both scopes are candidates now, so the existing priority rule must
    /// still pick the NARROWEST enclosing span (largest start <= line).
    #[test]
    fn narrowest_span_wins_across_mixed_scopes() {
        let conn = mem();
        // Inverted on purpose: the WIDE span is passage-scoped and the NARROW
        // one scene-scoped, so the old `scope='passage'` predicate would have
        // returned the WIDE row. Only span-based selection returns `narrow`.
        insert_cited(&conn, "BH", 2, 0, "passage", "BH.2.0.40", "BH.2.0.60");
        let narrow = insert_cited(&conn, "BH", 2, 0, "scene", "BH.2.0.47", "BH.2.0.49");

        let hit = find_journal_page_for_line(&conn, "BH", 2, 0, 48).unwrap();
        assert_eq!(hit, Some((2, 0, narrow)), "nearest start must still win");
    }

    #[test]
    fn list_all_journal_rows_loads_body_text() {
        let conn = crate::db::queries::open_db().unwrap();
        let rows = list_all_journal_rows(&conn).unwrap();
        // Whatever the corpus, every row must carry its answer text (regression
        // guard: the loader must SELECT answer, not just metadata).
        assert!(rows.iter().all(|r| !r.work_abbrev.is_empty()));
        if let Some(r) = rows.first() {
            assert!(!r.answer.is_empty() || !r.question.is_empty());
        }
    }
}
