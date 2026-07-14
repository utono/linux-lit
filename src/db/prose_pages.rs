//! Persisted visual-row prose pages, keyed by citation (`line_mapping` ids)
//! + pixel row offsets, and the layout fingerprint they were generated at.
//! See docs/superpowers/specs/2026-07-05-prose-visual-row-pagination-design.md.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct ProsePageRow {
    pub page_no: i64,
    pub start_line_id: i64,
    /// Pixel offset from start line's top; a snapped visual-row top.
    pub start_off: i64,
    pub end_line_id: i64,
    /// Exclusive pixel bottom edge within the end line.
    pub end_off: i64,
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
        "CREATE TABLE IF NOT EXISTS prose_pages (
             work_abbrev        TEXT NOT NULL,
             layout_fingerprint TEXT NOT NULL,
             page_no            INTEGER NOT NULL,
             start_line_id      INTEGER NOT NULL,
             start_row_offset   INTEGER NOT NULL,
             end_line_id        INTEGER NOT NULL,
             end_row_offset     INTEGER NOT NULL,
             PRIMARY KEY (work_abbrev, layout_fingerprint, page_no)
         );
         CREATE TABLE IF NOT EXISTS prose_pages_meta (
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
) -> rusqlite::Result<Option<(PagesMeta, Vec<ProsePageRow>)>> {
    let meta: Option<PagesMeta> = conn
        .query_row(
            "SELECT db_fingerprint, page_count, generated_at, validated
             FROM prose_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
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
        "SELECT page_no, start_line_id, start_row_offset, end_line_id, end_row_offset
         FROM prose_pages
         WHERE work_abbrev = ?1 AND layout_fingerprint = ?2 ORDER BY page_no",
    )?;
    let rows = stmt
        .query_map(params![abbrev, layout_fingerprint], |row| {
            Ok(ProsePageRow {
                page_no: row.get(0)?,
                start_line_id: row.get(1)?,
                start_off: row.get(2)?,
                end_line_id: row.get(3)?,
                end_off: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() as i64 != meta.page_count {
        return Ok(None);
    }
    Ok(Some((meta, rows)))
}

pub fn store_pages(
    conn: &mut Connection,
    abbrev: &str,
    meta: &PagesMeta,
    rows: &[ProsePageRow],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM prose_pages WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![abbrev, meta.layout_fingerprint],
    )?;
    tx.execute(
        "DELETE FROM prose_pages_meta WHERE work_abbrev = ?1 AND layout_fingerprint = ?2",
        params![abbrev, meta.layout_fingerprint],
    )?;
    for r in rows {
        tx.execute(
            "INSERT INTO prose_pages
             (work_abbrev, layout_fingerprint, page_no,
              start_line_id, start_row_offset, end_line_id, end_row_offset)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![abbrev, meta.layout_fingerprint, r.page_no,
                    r.start_line_id, r.start_off, r.end_line_id, r.end_off],
        )?;
    }
    tx.execute(
        "INSERT INTO prose_pages_meta
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
            generated_at: "epoch:1751700000".into(),
            validated: true,
        }
    }

    fn sample_rows() -> Vec<ProsePageRow> {
        vec![
            ProsePageRow { page_no: 1, start_line_id: 100, start_off: 0,
                           end_line_id: 101, end_off: 240 },
            ProsePageRow { page_no: 2, start_line_id: 101, start_off: 240,
                           end_line_id: 105, end_off: 60 },
        ]
    }

    #[test]
    fn roundtrips_pages_and_meta() {
        let mut conn = mem();
        store_pages(&mut conn, "BH", &sample_meta(), &sample_rows()).unwrap();
        let (meta, rows) = load_pages(&conn, "BH", "v1|abc").unwrap().unwrap();
        assert_eq!(meta.db_fingerprint, 42);
        assert_eq!(rows, sample_rows());
    }

    #[test]
    fn load_misses_on_wrong_fingerprint_or_abbrev() {
        let mut conn = mem();
        store_pages(&mut conn, "BH", &sample_meta(), &sample_rows()).unwrap();
        assert!(load_pages(&conn, "BH", "v1|OTHER").unwrap().is_none());
        assert!(load_pages(&conn, "DC", "v1|abc").unwrap().is_none());
    }

    #[test]
    fn unvalidated_meta_loads_as_none() {
        let mut conn = mem();
        let mut meta = sample_meta();
        meta.validated = false;
        store_pages(&mut conn, "BH", &meta, &sample_rows()).unwrap();
        assert!(load_pages(&conn, "BH", "v1|abc").unwrap().is_none());
    }

    #[test]
    fn row_count_mismatch_loads_as_none() {
        let mut conn = mem();
        store_pages(&mut conn, "BH", &sample_meta(), &sample_rows()).unwrap();
        conn.execute("UPDATE prose_pages_meta SET page_count = 3", []).unwrap();
        assert!(load_pages(&conn, "BH", "v1|abc").unwrap().is_none());
    }
}
