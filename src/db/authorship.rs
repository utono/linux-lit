use rusqlite::Connection;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct AttributionSet {
    pub id: i64,
    pub work_abbrev: String,
    pub name: String,
    pub display_name: String,
    pub primary_author: String,
    pub secondary_author: String,
}

pub fn load_attribution_sets(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<AttributionSet>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, work_abbrev, name, display_name, primary_author, secondary_author
         FROM attribution_sets WHERE work_abbrev = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(AttributionSet {
            id: row.get(0)?,
            work_abbrev: row.get(1)?,
            name: row.get(2)?,
            display_name: row.get(3)?,
            primary_author: row.get(4)?,
            secondary_author: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn load_secondary_line_ids(
    conn: &Connection,
    set_id: i64,
    work_abbrev: &str,
) -> Result<HashSet<i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT lm.id
         FROM line_authorship la
         JOIN line_mapping lm
           ON lm.work_abbrev = ?2
          AND la.citation = lm.work_abbrev || '.' || lm.div1 || '.' || lm.div2 || '.' || lm.line_in_div
         WHERE la.attribution_set_id = ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![set_id, work_abbrev], |row| {
        row.get::<_, i64>(0)
    })?;
    let mut ids = HashSet::new();
    for r in rows {
        ids.insert(r?);
    }
    Ok(ids)
}
