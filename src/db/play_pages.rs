//! Persisted page spreads for two-column plays, keyed by citation
//! (`line_mapping` ids) and the layout fingerprint they were generated at.
//! See docs/plans/2026-07-04-pinned-play-pagination-design.md.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct PageRow {
    pub page_no: i64,
    pub left_start_id: i64,
    /// First line of the right column; None = empty right column (watermark).
    pub split_id: Option<i64>,
    /// Last line ON the page, inclusive.
    pub end_id: i64,
}

#[derive(Debug, Clone)]
pub struct PagesMeta {
    pub layout_fingerprint: String,
    pub db_fingerprint: u64,
    pub page_count: i64,
    pub generated_at: String,
    pub validated: bool,
}

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS play_pages (
             work_abbrev        TEXT NOT NULL,
             layout_fingerprint TEXT NOT NULL,
             page_no            INTEGER NOT NULL,
             left_start_id      INTEGER NOT NULL,
             split_id           INTEGER,
             end_id             INTEGER NOT NULL,
             PRIMARY KEY (work_abbrev, layout_fingerprint, page_no)
         );
         CREATE TABLE IF NOT EXISTS play_pages_meta (
             work_abbrev        TEXT NOT NULL,
             layout_fingerprint TEXT NOT NULL,
             db_fingerprint     TEXT NOT NULL,
             page_count         INTEGER NOT NULL,
             generated_at       TEXT NOT NULL,
             validated          INTEGER NOT NULL,
             PRIMARY KEY (work_abbrev, layout_fingerprint)
         );",
    )
}

pub fn load_pages(
    conn: &Connection,
    abbrev: &str,
    layout_fingerprint: &str,
) -> rusqlite::Result<Option<(PagesMeta, Vec<PageRow>)>> {
    let meta: Option<PagesMeta> = conn
        .query_row(
            "SELECT db_fingerprint, page_count, generated_at, validated
             FROM play_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
            params![abbrev, layout_fingerprint],
            |row| {
                let db_fp: String = row.get(0)?;
                Ok(PagesMeta {
                    layout_fingerprint: layout_fingerprint.to_string(),
                    db_fingerprint: db_fp.parse::<u64>().unwrap_or(0),
                    page_count: row.get(1)?,
                    generated_at: row.get(2)?,
                    validated: row.get::<_, i64>(3)? != 0,
                })
            },
        )
        .optional()?;
    let Some(meta) = meta else { return Ok(None) };
    if !meta.validated {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT page_no, left_start_id, split_id, end_id FROM play_pages
         WHERE work_abbrev = ?1 AND layout_fingerprint = ?2 ORDER BY page_no",
    )?;
    let rows = stmt
        .query_map(params![abbrev, layout_fingerprint], |row| {
            Ok(PageRow {
                page_no: row.get(0)?,
                left_start_id: row.get(1)?,
                split_id: row.get(2)?,
                end_id: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() as i64 != meta.page_count {
        return Ok(None); // partial write / manual tampering: treat as absent
    }
    Ok(Some((meta, rows)))
}

pub fn store_pages(
    conn: &mut Connection,
    abbrev: &str,
    meta: &PagesMeta,
    rows: &[PageRow],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM play_pages WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![abbrev, meta.layout_fingerprint],
    )?;
    tx.execute(
        "DELETE FROM play_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![abbrev, meta.layout_fingerprint],
    )?;
    for r in rows {
        tx.execute(
            "INSERT INTO play_pages
             (work_abbrev, layout_fingerprint, page_no, left_start_id, split_id, end_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![abbrev, meta.layout_fingerprint, r.page_no,
                    r.left_start_id, r.split_id, r.end_id],
        )?;
    }
    tx.execute(
        "INSERT INTO play_pages_meta
         (work_abbrev, layout_fingerprint, db_fingerprint, page_count, generated_at, validated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![abbrev, meta.layout_fingerprint,
                meta.db_fingerprint.to_string(), rows.len() as i64,
                meta.generated_at, meta.validated as i64],
    )?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    fn sample_meta() -> PagesMeta {
        PagesMeta {
            layout_fingerprint: "v1|abc".into(),
            db_fingerprint: 42,
            page_count: 2,
            generated_at: "2026-07-04T12:00:00Z".into(),
            validated: true,
        }
    }

    fn sample_rows() -> Vec<PageRow> {
        vec![
            PageRow { page_no: 1, left_start_id: 100, split_id: Some(140), end_id: 180 },
            PageRow { page_no: 2, left_start_id: 181, split_id: None, end_id: 200 },
        ]
    }

    #[test]
    fn roundtrips_pages_and_meta() {
        let mut conn = mem();
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()).unwrap();
        let (meta, rows) = load_pages(&conn, "MND", "v1|abc").unwrap().unwrap();
        assert_eq!(meta.db_fingerprint, 42);
        assert_eq!(meta.page_count, 2);
        assert!(meta.validated);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].split_id, Some(140));
        assert_eq!(rows[1].split_id, None);
        assert_eq!(rows[1].end_id, 200);
    }

    #[test]
    fn load_misses_on_wrong_fingerprint_or_abbrev() {
        let mut conn = mem();
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()).unwrap();
        assert!(load_pages(&conn, "MND", "v1|OTHER").unwrap().is_none());
        assert!(load_pages(&conn, "Ham", "v1|abc").unwrap().is_none());
    }

    #[test]
    fn store_replaces_same_key_only() {
        let mut conn = mem();
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()).unwrap();
        // A second layout's table coexists (the headless-vs-production case).
        let mut meta2 = sample_meta();
        meta2.layout_fingerprint = "v1|headless".into();
        store_pages(&mut conn, "MND", &meta2, &sample_rows()[..1]).unwrap();
        // Re-store the first layout with 1 row: replaces its rows, not layout 2's.
        store_pages(&mut conn, "MND", &sample_meta(), &sample_rows()[..1]).unwrap();
        let (_, rows1) = load_pages(&conn, "MND", "v1|abc").unwrap().unwrap();
        let (_, rows2) = load_pages(&conn, "MND", "v1|headless").unwrap().unwrap();
        assert_eq!(rows1.len(), 1);
        assert_eq!(rows2.len(), 1);
    }

    #[test]
    fn unvalidated_meta_loads_as_none() {
        let mut conn = mem();
        let mut meta = sample_meta();
        meta.validated = false;
        store_pages(&mut conn, "MND", &meta, &sample_rows()).unwrap();
        assert!(load_pages(&conn, "MND", "v1|abc").unwrap().is_none());
    }
}
