use rusqlite::Connection;
use super::models::Chunk;

pub fn load_chunks(
    conn: &Connection,
    work_abbrev: &str,
    media_id: i64,
) -> Result<Vec<Chunk>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, a_line, b_line, a_time, b_time, div1, div2 \
         FROM chunks \
         WHERE work_abbrev = ?1 AND media_id = ?2 \
         ORDER BY div1, div2, a_line",
    )?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, media_id], |row| {
        Ok(Chunk {
            id: row.get(0)?,
            a_line: row.get(1)?,
            b_line: row.get(2)?,
            a_time: row.get(3)?,
            b_time: row.get(4)?,
            div1: row.get(5)?,
            div2: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries::open_db;

    #[test]
    fn test_load_chunks_no_error() {
        let conn = open_db().unwrap();
        // Should not error even if no chunks exist for this work/media combo
        let result = load_chunks(&conn, "Ref", 1);
        assert!(result.is_ok());
    }
}
