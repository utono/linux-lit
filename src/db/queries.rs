use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::HashMap;

use super::line_types;
use super::models::{Line, MediaItem, TimeRange, Timestamp, Work, WorkSummary};
// The schema-migration fns live in db::migrations (audit #94); the test
// fixtures below still call them unqualified.
#[cfg(test)]
use super::migrations::{
    ensure_bookmarks_table, ensure_characters_table, ensure_gloss_audio_table,
    ensure_gloss_voices_table, ensure_journal_audio_table, ensure_synopsis_audio_table,
    ensure_vocab_highlight_column, ensure_voice_catalog_table,
};
use crate::scansion::{LineScansion, ScanSyllable};

fn db_path() -> String {
    // LIT_DB_PATH lets an isolated run (e.g. the headless nav-fuzz) read its own
    // private copy of the database instead of the shared lit.db, so it doesn't
    // contend with a live `cargo run` session's SQLite file locks.
    if let Ok(p) = std::env::var("LIT_DB_PATH") {
        if !p.is_empty() {
            return p;
        }
    }
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    format!("{}/utono/litdb/data/lit.db", home)
}

/// Panic message for the `open_db().expect(...)` sites that treat a missing
/// lit.db as unrecoverable (startup, pickers, concordance). Named so the
/// message can't drift between the ~14 call sites.
pub const OPEN_DB_PANIC_MSG: &str = "Failed to open lit.db";

pub fn open_db() -> Result<Connection, rusqlite::Error> {
    Connection::open_with_flags(db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn list_works(conn: &Connection) -> Result<Vec<WorkSummary>, rusqlite::Error> {
    // The Ctrl+p library picker lists works to open (read/listen to), so:
    //
    // 1. Only list works with at least one associated media file — a work with
    //    no audio can't be played. `work_media_associations` is the
    //    authoritative "has media" signal (superset of media_files.work_abbrev),
    //    matching the media picker's own join.
    //
    // 2. Hide a BASE work whose media really belongs to its specific editions
    //    ("edition-leak"): if a base with editions (e.g. AWW, editions AWW-Amb/
    //    AWW-BBC) has ONLY media that is (a) not a multi-work bundle AND (b)
    //    shared with one of its own editions, the base is redundant with the
    //    edition — hide it. EXCEPTION: a media file that contains more than one
    //    work (a multi-play bundle, associated with >1 distinct base work — e.g.
    //    Rom's Hamlet+Macbeth+Romeo m4b) keeps the base shown, since that
    //    recording is only reachable through the base. Base = abbrev before the
    //    first '-'. Result on current lit.db: only AWW is hidden by rule 2;
    //    Rom/MND (bundle) and Cym (has a base-only file) remain.
    let mut stmt = conn.prepare(
        "WITH bundle AS ( \
             SELECT media_id FROM ( \
                 SELECT media_id, \
                     CASE WHEN instr(work_abbrev,'-')>0 \
                          THEN substr(work_abbrev,1,instr(work_abbrev,'-')-1) \
                          ELSE work_abbrev END AS base \
                 FROM work_media_associations \
             ) GROUP BY media_id HAVING COUNT(DISTINCT base) > 1 \
         ) \
         SELECT abbrev, title, author, work_type FROM works w \
         WHERE EXISTS ( \
                 SELECT 1 FROM work_media_associations wma WHERE wma.work_abbrev = w.abbrev \
             ) \
             AND NOT ( \
                 w.abbrev NOT LIKE '%-%' \
                 AND EXISTS (SELECT 1 FROM works e WHERE e.abbrev LIKE w.abbrev || '-%') \
                 AND NOT EXISTS ( \
                     SELECT 1 FROM work_media_associations wma \
                     WHERE wma.work_abbrev = w.abbrev AND wma.media_id IN (SELECT media_id FROM bundle) \
                 ) \
                 AND NOT EXISTS ( \
                     SELECT 1 FROM work_media_associations wma \
                     WHERE wma.work_abbrev = w.abbrev \
                       AND NOT EXISTS ( \
                           SELECT 1 FROM work_media_associations wma2 \
                           JOIN works e ON e.abbrev = wma2.work_abbrev \
                           WHERE wma2.media_id = wma.media_id AND e.abbrev LIKE w.abbrev || '-%' \
                       ) \
                 ) \
             ) \
         ORDER BY title",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WorkSummary {
            abbrev: row.get(0)?,
            title: row.get(1)?,
            author: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            work_type: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Load Wright scansion for every scanned line of `abbrev`, keyed by
/// `line_mapping.id`. Lines with no `line_meter` row are absent from the map
/// (rendered plain by the caller). Mirrors `load_work`'s query idiom.
pub fn load_scansion_for_work(
    conn: &Connection,
    abbrev: &str,
) -> Result<HashMap<i64, LineScansion>, rusqlite::Error> {
    // 1. line_meter rows for this work's lines.
    let mut meter_stmt = conn.prepare(
        "SELECT lm.line_id, lm.line_type, lm.caesura_after \
         FROM line_meter lm JOIN line_mapping m ON lm.line_id = m.id \
         WHERE m.work_abbrev = ?1",
    )?;
    let mut map: HashMap<i64, LineScansion> = HashMap::new();
    let meter_rows = meter_stmt.query_map([abbrev], |row| {
        let line_id: i64 = row.get(0)?;
        let line_type: String = row.get(1)?;
        let caesura_after: Option<i32> = row.get(2)?;
        Ok((line_id, line_type, caesura_after))
    })?;
    for r in meter_rows {
        let (line_id, line_type, caesura_after) = r?;
        map.insert(line_id, LineScansion { line_type, caesura_after, syllables: Vec::new() });
    }

    // 2. syllable_scan rows, appended in position order to their line.
    let mut syl_stmt = conn.prepare(
        "SELECT s.line_id, s.surface, s.ictus, s.is_extrametrical \
         FROM syllable_scan s JOIN line_mapping m ON s.line_id = m.id \
         WHERE m.work_abbrev = ?1 ORDER BY s.line_id, s.position",
    )?;
    let syl_rows = syl_stmt.query_map([abbrev], |row| {
        let line_id: i64 = row.get(0)?;
        let surface: Option<String> = row.get(1)?;
        let ictus: i64 = row.get(2)?;
        let is_extra: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
        Ok((line_id, surface.unwrap_or_default(), ictus as i8, is_extra != 0))
    })?;
    for r in syl_rows {
        let (line_id, surface, ictus, is_extrametrical) = r?;
        if let Some(ls) = map.get_mut(&line_id) {
            ls.syllables.push(ScanSyllable { surface, ictus, is_extrametrical });
        }
    }
    Ok(map)
}

pub fn load_work(conn: &Connection, abbrev: &str) -> Result<Work, rusqlite::Error> {
    // 1. Get work metadata
    let (title, author, work_type): (String, String, String) = conn.query_row(
        "SELECT title, COALESCE(author, ''), work_type FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    // text_file column may not exist yet (manual migration) — graceful fallback
    let text_file: Option<String> = conn.query_row(
        "SELECT text_file FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| row.get(0),
    ).unwrap_or(None);

    // vocab_highlight column may be absent on older/other DBs — graceful
    // fallback to OFF. 1 => on; 0/NULL/absent => off.
    let vocab_highlight: bool = conn.query_row(
        "SELECT vocab_highlight FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| row.get::<_, Option<i64>>(0),
    ).unwrap_or(None).unwrap_or(0) == 1;

    let is_prose = line_types::is_prose_work(&work_type);

    // 2. Load all lines
    let mut line_stmt = conn.prepare(
        "SELECT id, canonical_text, normalized_text, speaker, div1, div2, line_in_div, sub_line \
         FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div, sub_line",
    )?;
    let lines: Vec<Line> = line_stmt
        .query_map([abbrev], |row| {
            let text: String = row.get(1)?;
            let normalized: String = row.get(2)?;
            let speaker: Option<String> = row.get(3)?;
            let div1: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let div2: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
            let line_in_div: i64 = row.get(6)?;
            let sub_line: i64 = row.get(7)?;
            let citation = crate::db::models::citation(abbrev, div1, div2, line_in_div);
            Ok(Line {
                id: row.get(0)?,
                citation,
                // A stage direction (sub_line > 0) is never spoken dialogue.
                is_dialogue: sub_line == 0 && line_types::is_dialogue(&text, is_prose),
                text,
                normalized,
                speaker,
                timestamp: None,
                div1,
                div2,
                line_in_div,
                sub_line,
                is_chapter: false,
                is_spoken: None,
            })
        })?
        .collect::<Result<_, _>>()?;

    // 3. Load timestamps
    let mut ts_stmt = conn.prepare(
        "SELECT lt.line_mapping_id, lt.start_time, lt.end_time, lt.media_id, \
         lt.sentence_start_time, lt.source, lt.is_track_mark \
         FROM line_timestamps lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let timestamps: Vec<Timestamp> = ts_stmt
        .query_map([abbrev], |row| {
            let source: String = row.get::<_, Option<String>>(5)?.unwrap_or_default();
            Ok(Timestamp {
                line_id: row.get(0)?,
                start: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                end: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                media_id: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                sentence_start: row.get::<_, Option<f64>>(4)?,
                is_manual: source == "manual",
                is_track_mark: row.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
            })
        })?
        .collect::<Result<_, _>>()?;

    // 4. Load media paths (needed to determine active media_id for timestamp filtering)
    let mut media_stmt = conn.prepare(
        "SELECT mf.id, mf.path FROM media_files mf \
         JOIN work_media_associations wma ON wma.media_id = mf.id \
         WHERE wma.work_abbrev = ?1 \
         ORDER BY wma.priority DESC",
    )?;
    let media_rows: Vec<(i64, String)> = media_stmt
        .query_map([abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let media_id = media_rows.first().map(|(id, _)| *id);
    let media_ids: Vec<i64> = media_rows.iter().map(|(id, _)| *id).collect();
    let media_paths: Vec<String> = media_rows.into_iter().map(|(_, path)| path).collect();

    // 5. Build timestamp lookup: line_id -> TimeRange (filtered by active media_id)
    let mut ts_map: HashMap<i64, TimeRange> = HashMap::new();
    for ts in &timestamps {
        if media_id.map_or(true, |mid| ts.media_id == mid) {
            ts_map.entry(ts.line_id).or_insert(TimeRange {
                start: ts.start,
                end: ts.end,
                sentence_start: ts.sentence_start,
                is_manual: ts.is_manual,
            });
        }
    }

    // 5c. Load spoken status for the active media
    let mut spoken_map: HashMap<i64, bool> = HashMap::new();
    if let Some(mid) = media_id {
        let mut spoken_stmt = conn.prepare(
            "SELECT line_mapping_id, is_spoken FROM line_spoken_status WHERE media_id = ?1",
        )?;
        let rows = spoken_stmt.query_map([mid], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? != 0))
        })?;
        for row in rows {
            let (lm_id, spoken) = row?;
            spoken_map.insert(lm_id, spoken);
        }
    }

    // 6. Attach timestamps and spoken status to lines
    let mut lines: Vec<Line> = lines
        .into_iter()
        .map(|mut line| {
            line.timestamp = ts_map.get(&line.id).copied();
            line.is_spoken = spoken_map.get(&line.id).copied();
            line
        })
        .collect();

    // 6b. Mark structural chapter starts from div1 boundaries (media-independent).
    crate::text_file_map::mark_chapter_starts(&mut lines, is_prose);

    Ok(Work {
        abbrev: abbrev.to_string(),
        canonical_abbrev: canonical_work_abbrev(conn, abbrev),
        title,
        author,
        work_type,
        text_file,
        vocab_highlight,
        lines,
        timestamps,
        media_paths,
        media_ids,
        media_id,
    })
}

/// The directory holding a work's page-scan images (`works.image_dir`), or None
/// if the work has no scans / the column is absent (graceful for older DBs).
pub fn load_image_dir(conn: &Connection, abbrev: &str) -> Option<String> {
    conn.query_row(
        "SELECT image_dir FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

/// Load a work's page images in reading order (`page_order`). Empty when the
/// work has no scans or the `page_images` table is absent (older DBs).
pub fn load_page_images(conn: &Connection, abbrev: &str) -> Vec<crate::db::models::PageImage> {
    let mut stmt = match conn.prepare(
        "SELECT image_path, page_order, start_line_id, end_line_id \
         FROM page_images WHERE work_abbrev = ?1 ORDER BY page_order",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(), // table missing -> no images
    };
    let rows = stmt.query_map([abbrev], |row| {
        Ok(crate::db::models::PageImage {
            image_path: row.get(0)?,
            page_order: row.get(1)?,
            start_line_id: row.get(2)?,
            end_line_id: row.get(3)?,
        })
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Set one page's `start_line_id` during calibration (rw connection). The
/// `end_line_id` columns are derived separately by `recompute_page_image_ends`.
pub fn save_page_image_start(
    conn: &Connection,
    abbrev: &str,
    page_order: i64,
    start_line_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE page_images SET start_line_id = ?3 \
         WHERE work_abbrev = ?1 AND page_order = ?2",
        rusqlite::params![abbrev, page_order, start_line_id],
    )?;
    Ok(())
}

/// Recompute every page's `end_line_id` for `abbrev` from the calibrated
/// `start_line_id` sequence: a page ends at the line-id just before the NEXT
/// calibrated page's start; the last calibrated page ends at `last_line_id`
/// (the work's final line). `ordered_line_ids` is the work's line_mapping ids in
/// reading order (id ascending == reading order, since ids are assigned that
/// way). Pages with a NULL start are left as-is (uncalibrated).
pub fn recompute_page_image_ends(
    conn: &mut Connection,
    abbrev: &str,
    ordered_line_ids: &[i64],
) -> Result<(), rusqlite::Error> {
    // Collect calibrated (page_order, start_line_id) in page order.
    let starts: Vec<(i64, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT page_order, start_line_id FROM page_images \
             WHERE work_abbrev = ?1 AND start_line_id IS NOT NULL \
             ORDER BY page_order",
        )?;
        let rows = stmt.query_map([abbrev], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    if starts.is_empty() {
        return Ok(());
    }
    let last_line_id = *ordered_line_ids.last().unwrap_or(&0);
    let pos = |lid: i64| ordered_line_ids.iter().position(|&x| x == lid);

    let tx = conn.transaction()?;
    for w in 0..starts.len() {
        let (page_order, _start) = starts[w];
        let end_line_id = if w + 1 < starts.len() {
            // Line just before the next calibrated page's start.
            let next_start = starts[w + 1].1;
            match pos(next_start) {
                Some(p) if p > 0 => ordered_line_ids[p - 1],
                _ => next_start, // next start is the first line; degenerate, keep it
            }
        } else {
            last_line_id
        };
        tx.execute(
            "UPDATE page_images SET end_line_id = ?3 \
             WHERE work_abbrev = ?1 AND page_order = ?2",
            rusqlite::params![abbrev, page_order, end_line_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Load translations for a work, keyed by line_mapping.id.
///
/// Production variants (`-Amb` Ambrose, `-BBC` BBC Radio, etc.) share the base
/// edition's translations: translations are stored only against the base
/// edition's line_mapping rows. If the direct query returns nothing and the
/// abbrev is a `<base>-<suffix>` variant, fall back to matching the variant's
/// lines to the base-edition lines by (div1, div2, line_in_div) and key the
/// translations to the variant's line_mapping.id, so the app's existing lookup
/// by line.id works unchanged. E.g. Cym-Amb and Cym-BBC both inherit Cym's
/// translations. This handles any production suffix, not just `-Amb`.
pub fn load_translations(
    conn: &Connection,
    abbrev: &str,
) -> Result<HashMap<i64, String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT lm.id, lt.translation \
         FROM line_translations lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([abbrev], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, translation) = row?;
        map.insert(id, translation);
    }

    // No direct translations: if this is a production variant `<base>-<suffix>`,
    // inherit the base work's translations matched line-for-line.
    if map.is_empty() {
        if let Some((base, _suffix)) = abbrev.rsplit_once('-') {
            let mut stmt = conn.prepare(
                "SELECT a.id, MIN(lt.translation) \
                 FROM line_mapping a \
                 JOIN line_mapping b \
                   ON b.work_abbrev = ?2 \
                  AND b.div1 = a.div1 \
                  AND b.div2 = a.div2 \
                  AND b.line_in_div = a.line_in_div \
                 JOIN line_translations lt ON lt.line_mapping_id = b.id \
                 WHERE a.work_abbrev = ?1 \
                 GROUP BY a.id",
            )?;
            let rows = stmt.query_map([abbrev, base], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, translation) = row?;
                map.insert(id, translation);
            }
        }
    }

    Ok(map)
}

pub fn load_synopses(conn: &Connection, work_abbrev: &str) -> HashMap<(i64, i64), String> {
    let mut map = HashMap::new();
    let mut stmt = match conn.prepare(
        "SELECT div1, div2, synopsis FROM scene_synopses WHERE work_abbrev = ?1"
    ) {
        Ok(s) => s,
        Err(_) => return map,
    };
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    });
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            map.insert((row.0, row.1), row.2);
        }
    }
    map
}

/// Update (or insert) the synopsis text for one scene. Used by the `A` amend
/// flow in the synopsis overlay; the UNIQUE(work_abbrev, div1, div2) constraint
/// makes this an upsert. Requires a read-write connection (open_db_rw).
pub fn save_synopsis(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    synopsis: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO scene_synopses (work_abbrev, div1, div2, synopsis, claude_model) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(work_abbrev, div1, div2) DO UPDATE SET \
             synopsis = excluded.synopsis, claude_model = excluded.claude_model",
        rusqlite::params![work_abbrev, div1, div2, synopsis, claude_model],
    )?;
    Ok(())
}

/// Restore a synopsis's text WITHOUT changing its recorded `claude_model`. Used
/// by the `U` undo path, which reverts to the pre-amend text — that earlier text
/// was authored by whatever model the row already records, so undo must not
/// overwrite the model the way a fresh amend (save_synopsis) does. If the row
/// doesn't exist yet (no prior amend persisted), this is a no-op UPDATE.
pub fn restore_synopsis_text(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    synopsis: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE scene_synopses SET synopsis = ?4 \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3",
        rusqlite::params![work_abbrev, div1, div2, synopsis],
    )?;
    Ok(())
}

/// Look up the `scene_synopses.id` for a scene, keyed by `(work, div1, div2)`.
/// Returns `None` when no synopsis row exists for that scene yet. Used by the
/// synopsis overlay's `c` (copy id) bind, mirroring gloss `c` (gloss_id).
pub fn synopsis_id(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT id FROM scene_synopses \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3",
        rusqlite::params![work_abbrev, div1, div2],
        |row| row.get::<_, i64>(0),
    )
    .optional()
}

/// Load all vocab words + variants for matching against buffer text.
/// Returns a HashSet of lowercase words (base words + variants).
pub fn load_vocab_words(
    conn: &Connection,
    _work_abbrev: &str,
) -> Result<std::collections::HashSet<String>, rusqlite::Error> {
    let mut words = std::collections::HashSet::new();

    let mut stmt = conn.prepare("SELECT LOWER(word) FROM vocab_words")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        words.insert(row?);
    }

    let mut stmt = conn.prepare("SELECT LOWER(v.variant) FROM vocab_word_variants v")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        words.insert(row?);
    }

    Ok(words)
}

/// Load definition and sources for a vocab word.
pub fn load_vocab_definition(
    conn: &Connection,
    word: &str,
) -> Option<(String, Vec<String>)> {
    let result: Result<(String, Option<String>), _> = conn.query_row(
        "SELECT w.definition, GROUP_CONCAT(s.source) \
         FROM vocab_words w \
         LEFT JOIN vocab_word_sources s ON s.word_id = w.id \
         WHERE LOWER(w.word) = ?1 \
         GROUP BY w.id",
        [word.to_lowercase()],
        |row| Ok((
            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            row.get::<_, Option<String>>(1)?,
        )),
    );
    match result {
        Ok((def, sources_str)) => {
            let sources: Vec<String> = sources_str
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default();
            if def.is_empty() { None } else { Some((def, sources)) }
        }
        Err(_) => None,
    }
}

#[allow(dead_code)]
pub struct VocabEtymology {
    pub prefix: Option<String>,
    pub prefix_gloss: Option<String>,
    pub root: Option<String>,
    pub root_gloss: Option<String>,
    pub suffix: Option<String>,
    pub suffix_gloss: Option<String>,
}

/// Load etymology breakdown from vocab_rhetoric.
pub fn load_vocab_etymology(
    conn: &Connection,
    word: &str,
) -> Option<VocabEtymology> {
    conn.query_row(
        "SELECT vr.prefix, vr.prefix_gloss, vr.root, \
         vr.root_gloss, vr.suffix, vr.suffix_gloss \
         FROM vocab_rhetoric vr \
         JOIN vocab_words vw ON vr.word_id = vw.id \
         WHERE LOWER(vw.word) = ?1",
        [word.to_lowercase()],
        |row| Ok(VocabEtymology {
            prefix: row.get::<_, Option<String>>(0)?,
            prefix_gloss: row.get::<_, Option<String>>(1)?,
            root: row.get::<_, Option<String>>(2)?,
            root_gloss: row.get::<_, Option<String>>(3)?,
            suffix: row.get::<_, Option<String>>(4)?,
            suffix_gloss: row.get::<_, Option<String>>(5)?,
        }),
    ).ok()
}

/// Load a vocab-word gloss for a word near a given line.
pub fn load_vocab_gloss(
    conn: &Connection,
    word: &str,
    work_abbrev: &str,
    line_citation: &str,
) -> Option<String> {
    let word_id: i64 = conn.query_row(
        "SELECT id FROM vocab_words WHERE LOWER(word) = ?1",
        [word.to_lowercase()],
        |row| row.get(0),
    ).ok()?;

    conn.query_row(
        "SELECT g.gloss_text FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE g.gloss_type = 'vocab-word' \
         AND g.word_id = ?1 \
         AND p.work_abbrev = ?2 \
         AND p.start_citation <= ?3 \
         AND p.end_citation >= ?3",
        rusqlite::params![word_id, work_abbrev, line_citation],
        |row| row.get::<_, String>(0),
    ).ok()
}

/// List all vocab words found in a work's text, with occurrence counts.
pub fn load_vocab_word_list(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT canonical_text FROM line_mapping \
         ORDER BY div1, div2, line_in_div, sub_line"
    )?;
    let lines: Vec<String> = stmt.query_map([], |row| {
        row.get::<_, String>(0)
    })?.collect::<Result<_, _>>()?;

    let vocab = load_vocab_words(conn, work_abbrev)?;

    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in &lines {
        for token in line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}') {
            let lower = token.to_lowercase();
            if vocab.contains(&lower) {
                *counts.entry(lower).or_insert(0) += 1;
            }
        }
    }

    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Narration start time of the FIRST phrase whose char range extends past
/// char offset `char_off` within a line (`end_char > char_off`, ordered by
/// `start_char`) — i.e. the first phrase that is (wholly or partly) on the
/// NEXT prose page when the page boundary falls at `char_off`. Always the
/// phrase's `start_time`, including for a phrase that STRADDLES the offset:
/// the page must turn the moment that phrase's first word is highlighted, so
/// its continuation is readable on the new page as it is narrated (BH "…sat
/// down behind the door, |where" — waiting for the mid-phrase crossing left
/// the karaoke tint parked on the old page while "where…" ran off-screen).
/// The straddler's on-page head (often one turned-under word) is knowingly
/// cut by the turn. Used to fire a prose page turn when a page boundary falls
/// inside a spoken paragraph. `None` = no phrase_timestamps rows for this
/// (line, media) pair — the caller falls back to whole-line char-fraction
/// interpolation.
pub fn phrase_crossing_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    char_off: usize,
) -> Option<f64> {
    conn.query_row(
        "SELECT start_time FROM phrase_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2 AND end_char > ?3 \
         ORDER BY start_char LIMIT 1",
        rusqlite::params![line_mapping_id, media_id, char_off as i64],
        |row| row.get(0),
    )
    .ok()
}

/// One phrase's audio window + char range within its line's canonical text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhraseSpan {
    pub start_time: f64,
    pub end_time: f64,
    pub start_char: usize,
    pub end_char: usize,
}

/// All phrase spans for one (line, media), ordered by start_time. Empty vec =
/// no phrase_timestamps rows for the pair — callers cache the negative result
/// so works without phrase data stay inert with no per-tick re-query.
pub fn phrase_spans_for_line(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Vec<PhraseSpan> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT start_time, end_time, start_char, end_char FROM phrase_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2 ORDER BY start_time",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map(rusqlite::params![line_mapping_id, media_id], |row| {
        Ok(PhraseSpan {
            start_time: row.get(0)?,
            end_time: row.get(1)?,
            start_char: row.get::<_, i64>(2)?.max(0) as usize,
            end_char: row.get::<_, i64>(3)?.max(0) as usize,
        })
    });
    match rows {
        Ok(r) => r.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

/// ALL phrase spans for a media, grouped per line (each vec ordered by
/// start_time). One bulk query instead of one per line: the vocab-sentence
/// loop's eager build touched thousands of lines, and the per-call overhead
/// of phrase_spans_for_line made Ctrl+r take ~13s on a full novel (measured;
/// the bulk form is ~0.03s for the same 41k rows).
pub fn phrase_spans_for_media(
    conn: &Connection,
    media_id: i64,
) -> std::collections::HashMap<i64, Vec<PhraseSpan>> {
    let mut out: std::collections::HashMap<i64, Vec<PhraseSpan>> =
        std::collections::HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT line_mapping_id, start_time, end_time, start_char, end_char \
         FROM phrase_timestamps WHERE media_id = ?1 \
         ORDER BY line_mapping_id, start_time",
    ) else {
        return out;
    };
    let rows = stmt.query_map([media_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            PhraseSpan {
                start_time: row.get(1)?,
                end_time: row.get(2)?,
                start_char: row.get::<_, i64>(3)?.max(0) as usize,
                end_char: row.get::<_, i64>(4)?.max(0) as usize,
            },
        ))
    });
    if let Ok(r) = rows {
        for (line_id, span) in r.filter_map(Result::ok) {
            out.entry(line_id).or_default().push(span);
        }
    }
    out
}

/// Whether ANY phrase_timestamps rows exist for this media. Cheap gate so
/// no-phrase-data works skip vocab-sentence resolution entirely (and its
/// per-line span queries) and fall back to the plain vocab jump silently.
pub fn media_has_phrase_data(conn: &Connection, media_id: i64) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM phrase_timestamps WHERE media_id = ?1)",
        [media_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .unwrap_or(false)
}

pub fn list_media_for_work(
    conn: &Connection,
    abbrev: &str,
) -> Result<Vec<MediaItem>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT mf.id, mf.path, wma.display_name, wma.priority \
         FROM media_files mf \
         JOIN work_media_associations wma ON wma.media_id = mf.id \
         WHERE wma.work_abbrev = ?1 \
         ORDER BY wma.priority DESC",
    )?;
    let rows = stmt.query_map([abbrev], |row| {
        Ok(MediaItem {
            media_id: row.get(0)?,
            path: row.get(1)?,
            display_name: row.get(2)?,
            priority: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// True when `media_id` is a multi-work bundle — a media file that contains
/// more than one work, i.e. associated (via work_media_associations) with more
/// than one distinct BASE work (base = abbrev before the first '-', so Rom and
/// Rom-BBC are the same base). Used to avoid silently auto-loading a bundle when
/// it's a work's only media (e.g. Rom's Hamlet+Macbeth+Romeo m4b): the media
/// picker is shown instead so the user chooses knowingly.
pub fn is_bundle_media(conn: &Connection, media_id: i64) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 1 FROM ( \
             SELECT DISTINCT CASE WHEN instr(work_abbrev,'-')>0 \
                                  THEN substr(work_abbrev,1,instr(work_abbrev,'-')-1) \
                                  ELSE work_abbrev END AS base \
             FROM work_media_associations WHERE media_id = ?1 \
         )",
        [media_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .unwrap_or(false)
}

pub fn set_media_priority(
    conn: &Connection,
    abbrev: &str,
    media_id: i64,
) -> Result<(), rusqlite::Error> {
    // Find the current max priority for this work
    let max_priority: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(priority), 10) FROM work_media_associations WHERE work_abbrev = ?1",
            [abbrev],
            |row| row.get(0),
        )?;
    // Set all other media for this work to priority 10
    conn.execute(
        "UPDATE work_media_associations SET priority = 10 WHERE work_abbrev = ?1",
        [abbrev],
    )?;
    // Set the selected one to max + 10 (or at least 20)
    let new_priority = (max_priority + 10).max(20);
    conn.execute(
        "UPDATE work_media_associations SET priority = ?1 WHERE work_abbrev = ?2 AND media_id = ?3",
        rusqlite::params![new_priority, abbrev, media_id],
    )?;
    Ok(())
}

pub fn open_db_rw() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

/// Does `table` have a column named `col`? The `pragma_table_info` probe the
/// idempotent `ensure_*` migrations share before an `ALTER TABLE ... ADD COLUMN`
/// (SQLite has no `ADD COLUMN IF NOT EXISTS`). Shared by the `ensure_*` helpers
/// here and by `db::journal::ensure_journal_table` (audit #37/#67). EXCLUDED: the
/// `works.default_voice_id` probe deliberately SWALLOWS its error (the table may
/// not exist on a fresh/test DB) instead of propagating with `?`, so it keeps its
/// own non-`?` form.
pub(crate) fn column_exists(
    conn: &Connection,
    table: &str,
    col: &str,
) -> Result<bool, rusqlite::Error> {
    conn.prepare(&format!(
        "SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{col}'"
    ))?
    .exists([])
}

/// Resolve the abbrev under which a work's shared artifacts (glosses, journal
/// Q&A, scene synopses) are stored and looked up. A variant edition
/// (`Cym-Amb`, `Cym-BBC`, `MND-KPR`) resolves to its base work (`Cym`) so the
/// artifacts are shared across every edition — but ONLY when stripping the
/// last `-suffix` names a real work by the SAME author. That guard keeps
/// non-variant hyphenated abbrevs intact: `Mac-Ep-1` (MacCulloch) must never
/// collapse onto `Mac` (Macbeth), and `Aen-MW`/`Od-F` have no base work at
/// all. Unknown abbrevs (not in `works`) are returned unchanged.
pub fn canonical_work_abbrev(conn: &Connection, abbrev: &str) -> String {
    fn author_of(conn: &Connection, abbrev: &str) -> Option<String> {
        conn.query_row(
            "SELECT COALESCE(author, '') FROM works WHERE abbrev = ?1",
            [abbrev],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }
    let Some(author) = author_of(conn, abbrev) else {
        return abbrev.to_string();
    };
    let mut cur = abbrev.to_string();
    while let Some((base, _)) = cur.rsplit_once('-') {
        match author_of(conn, base) {
            Some(a) if a == author => cur = base.to_string(),
            _ => break,
        }
    }
    cur
}

/// The Arkangel edition's abbrev for `abbrev` if one exists, else `abbrev`
/// unchanged. Every Shakespeare play has a `{base}-Arkangel` sibling `works`
/// row (its own full-audio edition); works without one (non-Shakespeare, or an
/// already-`-Arkangel` abbrev) fall back to the input. Used so surfaces that
/// pick a work by its canonical/base abbrev can open the Arkangel edition —
/// mirroring picking the "(Arkangel)" row directly in the Ctrl+\ library
/// picker. Idempotent on a `-Arkangel` abbrev (no `…-Arkangel-Arkangel` row).
pub fn preferred_arkangel_abbrev(conn: &Connection, abbrev: &str) -> String {
    let candidate = format!("{abbrev}-Arkangel");
    let exists = conn
        .query_row(
            "SELECT 1 FROM works WHERE abbrev = ?1",
            [&candidate],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some();
    if exists {
        candidate
    } else {
        abbrev.to_string()
    }
}

/// The abbrev prefix of a full citation string `{abbrev}.{div1}.{div2}.{line}`
/// (abbrevs contain no dots, so it's everything before the last three).
fn citation_abbrev(citation: &str) -> Option<&str> {
    let mut idx = citation.len();
    for _ in 0..3 {
        idx = citation[..idx].rfind('.')?;
    }
    Some(&citation[..idx])
}

/// Idempotent startup migration: re-key shared artifacts stored under a
/// VARIANT edition's abbrev (`Cym-BBC`) onto the base work (`Cym`) so glosses,
/// journal Q&A, and scene synopses are shared across all editions. Rows land
/// under a variant abbrev only via pre-fix app builds — new writes go through
/// `Work.canonical_abbrev`. Citation strings are re-prefixed too (including
/// journal entries whose `work_abbrev` was already the base but whose
/// citations carried the variant prefix from `GlossContext`).
pub fn ensure_canonical_artifact_abbrevs(conn: &Connection) -> Result<(), rusqlite::Error> {
    for table in ["passages", "journal_entries", "scene_synopses"] {
        let abbrevs: Vec<String> = conn
            .prepare(&format!(
                "SELECT DISTINCT work_abbrev FROM {table} WHERE work_abbrev LIKE '%-%'"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for old in abbrevs {
            let new = canonical_work_abbrev(conn, &old);
            if new == old {
                continue;
            }
            crate::log_fmt!("MIGRATE-ABBREV: {table} {old} -> {new}");
            match table {
                "passages" => migrate_variant_passages(conn, &old, &new)?,
                "scene_synopses" => {
                    // UNIQUE(work_abbrev, div1, div2): keep an existing base
                    // row, drop the variant duplicate it collides with.
                    conn.execute(
                        "UPDATE OR IGNORE scene_synopses SET work_abbrev = ?1 \
                         WHERE work_abbrev = ?2",
                        rusqlite::params![new, old],
                    )?;
                    conn.execute(
                        "DELETE FROM scene_synopses WHERE work_abbrev = ?1",
                        [&old],
                    )?;
                }
                _ => {
                    conn.execute(
                        "UPDATE journal_entries SET work_abbrev = ?1 WHERE work_abbrev = ?2",
                        rusqlite::params![new, old],
                    )?;
                }
            }
        }
    }
    rekey_journal_citations(conn)
}

/// Re-key a variant edition's passages to the base abbrev: `work_abbrev`, both
/// citation prefixes, and the dedup `hash`. The hash is
/// md5("{abbrev}:{start}:{end}:{gloss_type}") — recomputed only when the
/// stored hash verifiably matches that recipe for one of the passage's
/// attached gloss types (so a future gloss on the same lines dedups onto the
/// migrated passage); otherwise the old hash is kept (it stays unique).
fn migrate_variant_passages(
    conn: &Connection,
    old: &str,
    new: &str,
) -> Result<(), rusqlite::Error> {
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = conn
        .prepare(
            "SELECT id, hash, start_citation, end_citation FROM passages \
             WHERE work_abbrev = ?1",
        )?
        .query_map([old], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let old_prefix = format!("{old}.");
    for (id, hash, start, end) in rows {
        let reprefix = |c: &Option<String>| {
            c.as_ref().map(|c| match c.strip_prefix(&old_prefix) {
                Some(rest) => format!("{new}.{rest}"),
                None => c.clone(),
            })
        };
        let new_start = reprefix(&start);
        let new_end = reprefix(&end);
        let mut new_hash = hash.clone();
        if let (Some(os), Some(oe), Some(ns), Some(ne)) =
            (&start, &end, &new_start, &new_end)
        {
            let types: Vec<String> = conn
                .prepare("SELECT DISTINCT gloss_type FROM glosses WHERE passage_id = ?1")?
                .query_map([id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for gt in &types {
                let old_input = format!("{old}:{os}:{oe}:{gt}");
                if format!("{:x}", md5::compute(old_input.as_bytes())) == hash {
                    let cand = format!(
                        "{:x}",
                        md5::compute(format!("{new}:{ns}:{ne}:{gt}").as_bytes())
                    );
                    let taken = conn
                        .prepare("SELECT 1 FROM passages WHERE hash = ?1")?
                        .exists([&cand])?;
                    if !taken {
                        new_hash = cand;
                    }
                    break;
                }
            }
        }
        conn.execute(
            "UPDATE passages SET work_abbrev = ?1, start_citation = ?2, \
             end_citation = ?3, hash = ?4 WHERE id = ?5",
            rusqlite::params![new, new_start, new_end, new_hash, id],
        )?;
    }
    Ok(())
}

/// Journal entries created from a variant edition can carry the VARIANT
/// citation prefix (`Cym-BBC.1.1.1`) even when `work_abbrev` is already the
/// base — the citations came from `GlossContext`. Re-prefix them to the
/// canonical abbrev so the journal→gloss cross-lookup (exact start_citation
/// match) finds the migrated passage.
fn rekey_journal_citations(conn: &Connection) -> Result<(), rusqlite::Error> {
    let rows: Vec<(i64, String, Option<String>)> = conn
        .prepare(
            "SELECT id, start_citation, end_citation FROM journal_entries \
             WHERE start_citation IS NOT NULL AND start_citation != ''",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, start, end) in rows {
        let Some(abbr) = citation_abbrev(&start) else {
            continue;
        };
        let canon = canonical_work_abbrev(conn, abbr);
        if canon == abbr {
            continue;
        }
        crate::log_fmt!("MIGRATE-ABBREV: journal citation {start} -> {canon} prefix");
        let old_prefix = format!("{abbr}.");
        let new_start = format!("{canon}.{}", &start[old_prefix.len()..]);
        let new_end = end.map(|e| match e.strip_prefix(&old_prefix) {
            Some(rest) => format!("{canon}.{rest}"),
            None => e,
        });
        conn.execute(
            "UPDATE journal_entries SET start_citation = ?1, end_citation = ?2 WHERE id = ?3",
            rusqlite::params![new_start, new_end, id],
        )?;
    }
    Ok(())
}

/// Set a work's per-work vocab-highlight flag (`1` on / `0` off), keyed by the
/// exact `abbrev` row. Call on a read-write connection (`open_db_rw`).
pub fn set_vocab_highlight(
    conn: &Connection,
    abbrev: &str,
    on: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE works SET vocab_highlight = ?2 WHERE abbrev = ?1",
        rusqlite::params![abbrev, on as i64],
    )?;
    Ok(())
}

/// Result of `insert_vocab_word`: whether the row was newly written / filled,
/// or already had a good definition and was left untouched.
pub enum VocabInsertOutcome {
    Added,
    AlreadyPresent,
}

/// Insert a vocab word, idempotent on the UNIQUE `word` column. A new word is
/// inserted; an existing word with an EMPTY definition is filled; an existing
/// word with a good definition is left intact. `word` is expected already
/// normalized (trimmed, lowercased) by the caller.
pub fn insert_vocab_word(
    conn: &Connection,
    word: &str,
    definition: &str,
    source: &str,
) -> Result<VocabInsertOutcome, rusqlite::Error> {
    // UNIQUE(word) is case-sensitive but words are matched case-insensitively
    // everywhere else, so probe NOCASE first — a capitalization difference
    // must update the existing row, never create a duplicate. The typed
    // capitalization always wins (proper nouns are stored capitalized; see
    // `normalize_vocab_word`).
    let existing: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, word, IFNULL(definition, '') FROM vocab_words \
             WHERE word = ?1 COLLATE NOCASE",
            [word],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    match existing {
        None => {
            conn.execute(
                "INSERT INTO vocab_words(word, definition, source) VALUES(?1, ?2, ?3)",
                rusqlite::params![word, definition, source],
            )?;
            Ok(VocabInsertOutcome::Added)
        }
        Some((id, stored_word, existing_def)) => {
            if existing_def.is_empty() {
                conn.execute(
                    "UPDATE vocab_words SET word = ?2, definition = ?3, source = ?4 WHERE id = ?1",
                    rusqlite::params![id, word, definition, source],
                )?;
                Ok(VocabInsertOutcome::Added)
            } else {
                if stored_word != word {
                    conn.execute(
                        "UPDATE vocab_words SET word = ?2 WHERE id = ?1",
                        rusqlite::params![id, word],
                    )?;
                }
                Ok(VocabInsertOutcome::AlreadyPresent)
            }
        }
    }
}

/// The voices associated with a gloss, ordered by `position` (cycle order).
pub fn get_gloss_voices(conn: &Connection, gloss_id: i64) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT voice_id, model_id FROM gloss_voices WHERE gloss_id = ?1 ORDER BY position",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![gloss_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}

/// Toggle a voice's membership in a gloss's set. Returns `true` if it was ADDED
/// (appended at the next position), `false` if it was REMOVED.
pub fn toggle_gloss_voice(
    conn: &Connection,
    gloss_id: i64,
    voice_id: &str,
    model_id: &str,
) -> bool {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM gloss_voices WHERE gloss_id = ?1 AND voice_id = ?2",
            rusqlite::params![gloss_id, voice_id],
            |_| Ok(()),
        )
        .is_ok();
    if exists {
        let _ = conn.execute(
            "DELETE FROM gloss_voices WHERE gloss_id = ?1 AND voice_id = ?2",
            rusqlite::params![gloss_id, voice_id],
        );
        false
    } else {
        let next_pos: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM gloss_voices WHERE gloss_id = ?1",
                rusqlite::params![gloss_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "INSERT INTO gloss_voices (gloss_id, voice_id, model_id, position) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![gloss_id, voice_id, model_id, next_pos],
        );
        true
    }
}

/// Return the cached audio path for a gloss block in a SPECIFIC voice, if any.
pub fn find_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM gloss_audio
         WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3 AND voice_id = ?4",
        rusqlite::params![gloss_id, kind, index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Default age used when a character has no curated age (NULL).
const DEFAULT_AGE: i64 = 40;

/// Read (Gender, age) for a speaker. Multi-speaker (comma) / no row → (Unknown,
/// None); a real DB error is logged and also yields (Unknown, None). Generalizes
/// get_character_gender to also pull age.
fn get_character_gender_age(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
) -> (crate::elevenlabs::Gender, Option<i64>) {
    if speaker.contains(',') {
        return (crate::elevenlabs::Gender::Unknown, None);
    }
    let row: Result<(String, Option<i64>), _> = conn.query_row(
        "SELECT gender, age FROM characters WHERE work_abbrev = ?1 AND speaker = ?2",
        rusqlite::params![work_abbrev, speaker],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    match row {
        Ok((g, a)) => (crate::elevenlabs::Gender::from_db(&g), a),
        Err(rusqlite::Error::QueryReturnedNoRows) => (crate::elevenlabs::Gender::Unknown, None),
        Err(e) => {
            crate::log_fmt!(
                "get_character_gender_age: unexpected DB error for {}/{}: {}",
                work_abbrev, speaker, e
            );
            (crate::elevenlabs::Gender::Unknown, None)
        }
    }
}

/// The narrator voice_id for PROSE/gloss of `work_abbrev`:
/// per-work `works.default_voice_id` → per-author `author_default_voice` →
/// global male default (Benedick). Always resolves; a query error logs and
/// falls through (e.g. a fresh DB without a `works` table → global default).
fn resolve_prose_voice(conn: &Connection, work_abbrev: &str) -> String {
    // 1. Per-work override.
    let per_work: Option<String> = conn
        .query_row(
            "SELECT default_voice_id FROM works
             WHERE abbrev = ?1 AND default_voice_id IS NOT NULL",
            rusqlite::params![work_abbrev],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!("resolve_prose_voice: per-work query error for {}: {}", work_abbrev, e);
            None
        });
    if let Some(v) = per_work {
        return v;
    }
    // 2. Per-author default (join works.author -> author_default_voice).
    let per_author: Option<String> = conn
        .query_row(
            "SELECT adv.voice_id FROM works w
             JOIN author_default_voice adv ON adv.author = w.author
             WHERE w.abbrev = ?1",
            rusqlite::params![work_abbrev],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!("resolve_prose_voice: per-author query error for {}: {}", work_abbrev, e);
            None
        });
    if let Some(v) = per_author {
        return v;
    }
    // 3. Global default: the male narrator.
    crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string()
}

/// Pick the default (voice_id, model_id) for a speaker by (gender, age) from the
/// voice_catalog: the narrowest band CONTAINING the age, else the NEAREST
/// same-gender band, else the legacy `voice_for` constants. `is_verse` selects
/// the verse/prose role. Unknown/neutral gender → male; missing age → DEFAULT_AGE.
pub fn resolve_default_voice(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
    is_verse: bool,
) -> (String, String) {
    // Prose (explication) reads in ONE narrator per work, resolved from data:
    // per-work override → per-author default → global male default. Shakespeare
    // is seeded to Eleanor; all other authors fall to the male default. (Verse
    // still picks by (gender, age) below; a per-gloss associated voice still
    // overrides this default at the call site in play_block_tts.)
    if !is_verse {
        return (
            resolve_prose_voice(conn, work_abbrev),
            crate::elevenlabs::OP_MODEL_ID.to_string(),
        );
    }

    let (gender, age_opt) = get_character_gender_age(conn, work_abbrev, speaker);
    // Catalog gender is 'male' | 'female'; everything not Female → male.
    let cat_gender = if gender == crate::elevenlabs::Gender::Female { "female" } else { "male" };
    let age = age_opt.unwrap_or(DEFAULT_AGE);
    let role = if is_verse { "verse" } else { "prose" };

    // 1. Containment: narrowest band that contains `age`.
    let contained: Option<(String, String)> = conn
        .query_row(
            "SELECT voice_id, model_id FROM voice_catalog
             WHERE gender = ?1 AND role = ?2 AND ?3 BETWEEN age_min AND age_max
             ORDER BY (age_max - age_min) ASC LIMIT 1",
            rusqlite::params![cat_gender, role, age],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!(
                "resolve_default_voice: containment query error for {}/{}: {}",
                work_abbrev, speaker, e
            );
            None
        });
    if let Some(hit) = contained {
        return hit;
    }

    // 2. Nearest same-gender/role band: clamped distance from `age` to the band's
    //    [age_min, age_max] interval — below-band uses (age_min - age), above-band
    //    uses (age - age_max), inside-band is 0 (those are already caught by step 1).
    let nearest: Option<(String, String)> = conn
        .query_row(
            "SELECT voice_id, model_id FROM voice_catalog
             WHERE gender = ?1 AND role = ?2
             ORDER BY MAX(0, age_min - ?3) + MAX(0, ?3 - age_max) ASC LIMIT 1",
            rusqlite::params![cat_gender, role, age],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap_or_else(|e| {
            crate::log_fmt!(
                "resolve_default_voice: nearest query error for {}/{}: {}",
                work_abbrev, speaker, e
            );
            None
        });
    if let Some(hit) = nearest {
        return hit;
    }

    // 3. Last resort (catalog empty / no same-gender voice — unreachable given
    //    the seed rows): the legacy gender-only constants.
    let (v, m) = crate::elevenlabs::voice_for(gender, is_verse);
    (v.to_string(), m.to_string())
}

/// Insert or replace the audio path for a gloss block in a specific voice.
pub fn save_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(gloss_id, kind, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![gloss_id, kind, index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}

/// Cached MP3 path for a synopsis paragraph in a specific voice, if any.
pub fn find_synopsis_audio(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    paragraph_index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM synopsis_audio
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3
           AND paragraph_index = ?4 AND voice_id = ?5",
        rusqlite::params![work_abbrev, div1, div2, paragraph_index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Upsert a cached synopsis-paragraph MP3 path.
pub fn save_synopsis_audio(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    paragraph_index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO synopsis_audio
            (work_abbrev, div1, div2, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(work_abbrev, div1, div2, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![work_abbrev, div1, div2, paragraph_index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}

/// Cached MP3 path for a journal-page paragraph in a specific voice, if any.
pub fn find_journal_audio(
    conn: &Connection,
    entry_id: i64,
    paragraph_index: i64,
    voice_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM journal_audio
         WHERE entry_id = ?1 AND paragraph_index = ?2 AND voice_id = ?3",
        rusqlite::params![entry_id, paragraph_index, voice_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Upsert a cached journal-paragraph MP3 path.
pub fn save_journal_audio(
    conn: &Connection,
    entry_id: i64,
    paragraph_index: i64,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_audio
            (entry_id, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(entry_id, paragraph_index, voice_id)
         DO UPDATE SET audio_path = excluded.audio_path,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![entry_id, paragraph_index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}

/// Delete all cached audio rows for a journal entry (call when the entry is
/// removed, since SQLite FK cascade is not enabled app-wide). Returns the
/// `audio_path`s removed so the caller can delete the files, mirroring
/// `delete_gloss_audio_block`.
pub fn delete_journal_audio(conn: &Connection, entry_id: i64) -> Result<Vec<String>, rusqlite::Error> {
    let paths: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT audio_path FROM journal_audio WHERE entry_id = ?1")?;
        let rows = stmt.query_map(rusqlite::params![entry_id], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    conn.execute(
        "DELETE FROM journal_audio WHERE entry_id = ?1",
        rusqlite::params![entry_id],
    )?;
    Ok(paths)
}

/// Delete all cached audio rows for a gloss (call when the gloss is removed,
/// since SQLite FK cascade is not enabled app-wide). Returns the number of rows
/// removed, so a caller can report exactly how many cached takes were purged.
pub fn delete_gloss_audio(conn: &Connection, gloss_id: i64) -> Result<usize, rusqlite::Error> {
    let n = conn.execute(
        "DELETE FROM gloss_audio WHERE gloss_id = ?1",
        rusqlite::params![gloss_id],
    )?;
    Ok(n)
}

/// Delete the cached audio rows for ONE block of a gloss (all voices) and return
/// their `audio_path`s so the caller can remove the files. Scoped, unlike
/// `delete_gloss_audio` which clears a whole gloss. Used by the fix-IPA flow to
/// invalidate just the corrected source block before re-synthesis.
pub fn delete_gloss_audio_block(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
) -> Result<Vec<String>, rusqlite::Error> {
    let paths: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT audio_path FROM gloss_audio
             WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![gloss_id, kind, index], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    conn.execute(
        "DELETE FROM gloss_audio WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        rusqlite::params![gloss_id, kind, index],
    )?;
    Ok(paths)
}

/// Load all bookmarked line_mapping_ids for a work.
pub fn load_bookmarks(conn: &Connection, work_abbrev: &str) -> Result<Vec<i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT line_mapping_id FROM bookmarks WHERE work_abbrev = ?1"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| row.get::<_, i64>(0))?;
    rows.collect()
}

/// Toggle a bookmark on a line. Returns true if added, false if removed.
pub fn toggle_bookmark(
    conn: &Connection,
    work_abbrev: &str,
    line_mapping_id: i64,
) -> Result<bool, rusqlite::Error> {
    let existing: Option<i64> = conn.query_row(
        "SELECT id FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_mapping_id],
        |row| row.get(0),
    ).optional()?;

    if let Some(id) = existing {
        conn.execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO bookmarks (work_abbrev, line_mapping_id) VALUES (?1, ?2)",
            rusqlite::params![work_abbrev, line_mapping_id],
        )?;
        Ok(true)
    }
}

/// Get the line_mapping_id of the most recently created bookmark for a work.
pub fn most_recent_bookmark(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT line_mapping_id FROM bookmarks WHERE work_abbrev = ?1 ORDER BY created_at DESC LIMIT 1",
        [work_abbrev],
        |row| row.get(0),
    ).optional()
}

/// Load bookmarks with line text for the picker, sorted by most recent first.
pub fn load_bookmarks_with_details(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<super::models::BookmarkItem>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT b.line_mapping_id, lm.canonical_text, lm.speaker, \
                lm.div1, lm.div2, lm.line_in_div \
         FROM bookmarks b \
         JOIN line_mapping lm ON b.line_mapping_id = lm.id \
         WHERE b.work_abbrev = ?1 \
         ORDER BY lm.div1, lm.div2, lm.line_in_div, lm.sub_line"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        let div1: i64 = row.get(3)?;
        let div2: i64 = row.get(4)?;
        let line_in_div: i64 = row.get(5)?;
        let citation = crate::db::models::citation(work_abbrev, div1, div2, line_in_div);
        Ok(super::models::BookmarkItem {
            line_mapping_id: row.get(0)?,
            line_text: row.get(1)?,
            speaker: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            citation,
        })
    })?;
    rows.collect()
}

/// Delete a bookmark by work and line_mapping_id.
pub fn delete_bookmark(
    conn: &Connection,
    work_abbrev: &str,
    line_mapping_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_mapping_id],
    )?;
    Ok(())
}

pub fn upsert_start_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source) \
         VALUES (?1, ?2, ?3, ?4, 'manual') \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET start_time = ?4, source = 'manual', updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    Ok(())
}

pub fn upsert_spoken_status(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    is_spoken: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_spoken_status \
         (line_mapping_id, media_id, is_spoken, confidence) \
         VALUES (?1, ?2, ?3, 1.0) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET is_spoken = ?3, confidence = 1.0",
        rusqlite::params![line_mapping_id, media_id, is_spoken as i64],
    )?;
    Ok(())
}

pub fn upsert_chapter(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: f64,
) -> Result<bool, rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source, is_track_mark) \
         VALUES (?1, ?2, ?3, ?4, 'manual', 1) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET is_track_mark = CASE WHEN is_track_mark = 1 THEN 0 ELSE 1 END, source = 'manual', updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    let new_val: bool = conn.query_row(
        "SELECT is_track_mark FROM line_timestamps WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id],
        |row| row.get(0),
    )?;
    Ok(new_val)
}

/// Toggle line_mapping.chapter_start for one paragraph. Returns the new value
/// (true = now marks a chapter start). NULL is treated as 0.
pub fn toggle_chapter_start(conn: &Connection, line_mapping_id: i64) -> Result<bool, rusqlite::Error> {
    conn.execute(
        "UPDATE line_mapping SET chapter_start = 1 - COALESCE(chapter_start, 0) WHERE id = ?1",
        [line_mapping_id],
    )?;
    let v: i64 = conn.query_row(
        "SELECT COALESCE(chapter_start, 0) FROM line_mapping WHERE id = ?1",
        [line_mapping_id],
        |r| r.get(0),
    )?;
    Ok(v == 1)
}

pub fn update_end_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    end_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE line_timestamps SET end_time = ?3, updated_at = CURRENT_TIMESTAMP \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id, end_time],
    )?;
    Ok(())
}

pub fn delete_timestamp(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM line_timestamps WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id],
    )?;
    Ok(())
}

pub fn get_timestamp_snapshot(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Result<Option<crate::input::timestamps::TimestampSnapshot>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT citation, start_time, end_time, is_track_mark \
         FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
    )?;
    let result = stmt.query_row(rusqlite::params![line_mapping_id, media_id], |row| {
        Ok(crate::input::timestamps::TimestampSnapshot {
            citation: row.get(0)?,
            start_time: row.get(1)?,
            end_time: row.get(2)?,
            is_track_mark: row.get::<_, bool>(3).unwrap_or(false),
        })
    });
    match result {
        Ok(snap) => Ok(Some(snap)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn restore_timestamp(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: Option<f64>,
    end_time: Option<f64>,
    is_track_mark: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, end_time, source, is_track_mark) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET start_time = ?4, end_time = ?5, is_track_mark = ?6, updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time, end_time, is_track_mark],
    )?;
    Ok(())
}

/// Merge multiple lines into one. Updates the first line's text and deletes the rest.

/// Replace a set of lines with new text lines. Updates the first line,
/// deletes excess old lines, or inserts new lines if output has more.
/// `old_ids`: IDs of original lines (ordered).
/// `new_texts`: replacement texts (ordered).
pub fn replace_lines(
    conn: &Connection,
    work_abbrev: &str,
    old_ids: &[i64],
    new_texts: &[String],
) -> Result<(), rusqlite::Error> {
    if old_ids.is_empty() || new_texts.is_empty() {
        return Ok(());
    }

    // Update existing lines where we have both old and new
    let update_count = old_ids.len().min(new_texts.len());
    for i in 0..update_count {
        conn.execute(
            "UPDATE line_mapping SET canonical_text = ?2, normalized_text = ?2 WHERE id = ?1",
            rusqlite::params![old_ids[i], new_texts[i]],
        )?;
    }

    // Delete excess old lines
    for &id in &old_ids[update_count..] {
        conn.execute("DELETE FROM line_mapping WHERE id = ?1", [id])?;
    }

    // Insert new lines if output has more than old
    if new_texts.len() > old_ids.len() {
        // Get div info from the first old line to use for new inserts
        let (div1, div2, base_line_in_div): (i64, i64, i64) = conn.query_row(
            "SELECT div1, div2, line_in_div FROM line_mapping WHERE id = ?1",
            [old_ids[0]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        for (i, text) in new_texts[old_ids.len()..].iter().enumerate() {
            let new_line_in_div = base_line_in_div + (old_ids.len() + i) as i64;
            conn.execute(
                "INSERT INTO line_mapping (work_abbrev, canonical_text, normalized_text, div1, div2, line_in_div) \
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
                rusqlite::params![work_abbrev, text, div1, div2, new_line_in_div],
            )?;
        }
    }

    Ok(())
}


#[derive(Debug, Clone)]
pub struct SavedGloss {
    pub gloss_id: i64,
    pub passage_id: i64,
    pub gloss_text: String,
    pub timestamp: String,
    pub gloss_type: String,
    pub start_citation: String,
    pub end_citation: String,
}

fn row_to_saved_gloss(row: &rusqlite::Row) -> rusqlite::Result<SavedGloss> {
    Ok(SavedGloss {
        gloss_id: row.get(0)?,
        gloss_text: row.get(1)?,
        timestamp: row.get(2)?,
        passage_id: row.get(3)?,
        gloss_type: row.get(4)?,
        start_citation: row.get(5)?,
        end_citation: row.get(6)?,
    })
}

fn row_to_glossed_passage(row: &rusqlite::Row) -> rusqlite::Result<GlossedPassage> {
    Ok(GlossedPassage {
        passage_id: row.get(0)?,
        work_abbrev: row.get(1)?,
        start_citation: row.get(2)?,
        end_citation: row.get(3)?,
        act: row.get(4)?,
        scene: row.get(5)?,
        speaker: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        source_text: row.get(7)?,
    })
}

pub fn find_existing_gloss(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
    gloss_type: &str,
) -> Result<Option<SavedGloss>, rusqlite::Error> {
    let gt = gloss_type.to_string();
    conn.query_row(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id, p.start_citation, p.end_citation \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND p.end_citation = ?3 \
           AND g.gloss_type = ?4 \
         ORDER BY g.timestamp DESC \
         LIMIT 1",
        rusqlite::params![work_abbrev, start_citation, end_citation, gloss_type],
        |row| {
            Ok(SavedGloss {
                gloss_id: row.get(0)?,
                gloss_text: row.get(1)?,
                timestamp: row.get(2)?,
                passage_id: row.get(3)?,
                gloss_type: gt.clone(),
                start_citation: row.get(4)?,
                end_citation: row.get(5)?,
            })
        },
    )
    .optional()
}

/// The passage a single gloss belongs to, looked up by the gloss's own id — for
/// the Ctrl+f cross-corpus search jump. Returns the passage (work_abbrev +
/// start_citation + source_text + act/scene/speaker) so the caller can rebuild
/// the gloss overlay the same way `open_gloss_at_cursor` does (find_glossed_passages
/// + find_glosses_by_start + open_gloss_overlay). `Ok(None)` if the gloss id no
/// longer exists (deleted between popup-load and Enter).
pub fn find_gloss_passage_by_id(
    conn: &Connection,
    gloss_id: i64,
) -> Result<Option<GlossedPassage>, rusqlite::Error> {
    conn.query_row(
        "SELECT p.id, p.work_abbrev, COALESCE(p.start_citation, ''), \
                COALESCE(p.end_citation, ''), p.div1, p.div2, p.character, p.source_text \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE g.id = ?1",
        [gloss_id],
        row_to_glossed_passage,
    )
    .optional()
}

pub fn find_all_glosses(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
    gloss_types: &[&str],
) -> Result<Vec<SavedGloss>, rusqlite::Error> {
    if gloss_types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..gloss_types.len())
        .map(|i| format!("?{}", i + 4))
        .collect();
    let sql = format!(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id, g.gloss_type, p.start_citation, p.end_citation \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND p.end_citation = ?3 \
           AND g.gloss_type IN ({}) \
         ORDER BY g.timestamp DESC",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(work_abbrev.to_string()));
    params.push(Box::new(start_citation.to_string()));
    params.push(Box::new(end_citation.to_string()));
    for gt in gloss_types {
        params.push(Box::new(gt.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), row_to_saved_gloss)?;
    rows.collect()
}

/// Like `find_all_glosses` but matches on START citation only (any end/span),
/// so glosses anchored to different-length passages that share a first line
/// co-list and cycle together. Reader-gloss rows sort first, then by recency.
pub fn find_glosses_by_start(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    gloss_types: &[&str],
) -> Result<Vec<SavedGloss>, rusqlite::Error> {
    if gloss_types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..gloss_types.len())
        .map(|i| format!("?{}", i + 3))
        .collect();
    let sql = format!(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id, g.gloss_type, p.start_citation, p.end_citation \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND g.gloss_type IN ({}) \
         ORDER BY (g.gloss_type = 'reader-gloss') DESC, g.timestamp DESC, g.id DESC",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(work_abbrev.to_string()));
    params.push(Box::new(start_citation.to_string()));
    for gt in gloss_types {
        params.push(Box::new(gt.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), row_to_saved_gloss)?;
    rows.collect()
}

#[derive(Debug, Clone)]
pub struct GlossedPassage {
    pub passage_id: i64,
    pub work_abbrev: String,
    pub start_citation: String,
    pub end_citation: String,
    pub act: i64,
    pub scene: i64,
    pub speaker: String,
    pub source_text: String,
}

pub fn find_glossed_passages(
    conn: &Connection,
    work_abbrev: &str,
    gloss_types: &[&str],
) -> Result<Vec<GlossedPassage>, rusqlite::Error> {
    if gloss_types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..gloss_types.len())
        .map(|i| format!("?{}", i + 2))
        .collect();
    let sql = format!(
        // Order in true work order: act, then scene, then the line-in-div
        // NUMERICALLY. start_citation is "ABBR.div1.div2.line" text, so sorting
        // it as a string puts line 17 before line 7. Extract the trailing line
        // number by stripping the non-trailing-digit prefix (rtrim removes the
        // trailing digits, replace deletes that prefix, leaving the number).
        "SELECT DISTINCT p.id, p.work_abbrev, p.start_citation, p.end_citation, \
                p.div1, p.div2, p.character, p.source_text \
         FROM passages p \
         JOIN glosses g ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND g.gloss_type IN ({}) \
         ORDER BY p.div1, p.div2, \
                  CAST(replace(p.start_citation, rtrim(p.start_citation, '0123456789'), '') AS INTEGER)",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(work_abbrev.to_string()));
    for gt in gloss_types {
        params.push(Box::new(gt.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), row_to_glossed_passage)?;
    rows.collect()
}

/// Every reader-gloss gloss across all works, with body text + citation +
/// speaker, for the Ctrl+f cross-corpus search popup. Joins passages for the
/// work/citation/speaker that the glosses row lacks.
pub fn list_all_gloss_rows(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Vec<crate::input::corpus_search::GlossRow>> {
    let mut stmt = conn.prepare(
        "SELECT g.id, p.work_abbrev, COALESCE(p.start_citation, ''),
                COALESCE(p.character, ''), g.gloss_text
         FROM glosses g
         JOIN passages p ON p.id = g.passage_id
         WHERE g.gloss_type = 'reader-gloss'
         ORDER BY p.work_abbrev, p.start_citation, g.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(crate::input::corpus_search::GlossRow {
                gloss_id: r.get(0)?,
                work_abbrev: r.get(1)?,
                start_citation: r.get(2)?,
                speaker: r.get(3)?,
                gloss_text: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn save_gloss(
    conn: &Connection,
    hash: &str,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
    act: i64,
    scene: i64,
    character: &str,
    source_text: &str,
    gloss_text: &str,
    gloss_type: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO passages \
         (hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![hash, work_abbrev, start_citation, end_citation, act, scene, character, source_text],
    )?;

    let passage_id: i64 = conn.query_row(
        "SELECT id FROM passages WHERE work_abbrev = ?1 AND start_citation = ?2 AND end_citation = ?3",
        rusqlite::params![work_abbrev, start_citation, end_citation],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO glosses (passage_id, gloss_type, gloss_text, claude_model) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![passage_id, gloss_type, gloss_text, claude_model],
    )?;

    let gloss_id = conn.last_insert_rowid();
    Ok(gloss_id)
}

pub fn update_gloss(
    conn: &Connection,
    gloss_id: i64,
    gloss_text: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE glosses SET gloss_text = ?1, claude_model = ?2, timestamp = CURRENT_TIMESTAMP WHERE id = ?3",
        rusqlite::params![gloss_text, claude_model, gloss_id],
    )?;
    Ok(())
}

pub fn delete_gloss(conn: &Connection, gloss_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM glosses WHERE id = ?1", [gloss_id])?;
    Ok(())
}

/// Load a map of work abbreviation → title for all works.
pub fn load_work_titles(conn: &Connection) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT abbrev, title FROM works")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (abbrev, title) = row?;
        map.insert(abbrev, title);
    }
    Ok(map)
}

/// Open the db and load the abbrev→title map, defaulting to an empty map on any
/// failure (db open or query). The byte-identical
/// `open_db().ok().and_then(|c| load_work_titles(&c).ok()).unwrap_or_default()`
/// chain repeated at every cross-work title lookup (echoes, visual selection).
pub fn load_work_titles_or_default() -> HashMap<String, String> {
    open_db()
        .ok()
        .and_then(|conn| load_work_titles(&conn).ok())
        .unwrap_or_default()
}

/// Resolve a line's line_mapping.id from its location within a work.
pub fn line_id_for_location(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    line_in_div: i64,
) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM line_mapping \
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND line_in_div = ?4 \
         LIMIT 1",
        rusqlite::params![work_abbrev, div1, div2, line_in_div],
        |row| row.get::<_, i64>(0),
    )
    .ok()
}

/// Search every line whose canonical text contains `query` (case-insensitive),
/// across all works. Returns (work_abbrev, div1, div2, line_in_div, text), capped.
pub fn search_lines(conn: &Connection, query: &str, limit: i64)
    -> Result<Vec<(String, i64, i64, i64, String)>, rusqlite::Error>
{
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT work_abbrev, div1, div2, line_in_div, canonical_text \
         FROM line_mapping \
         WHERE canonical_text LIKE ?1 COLLATE NOCASE \
         ORDER BY work_abbrev, div1, div2, line_in_div, sub_line \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    rows.collect()
}

/// Look up a single line's start time for a given media file. Returns None when
/// no timestamp row exists for that (line, media) pair.
pub fn line_start_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64> {
    conn.query_row(
        "SELECT start_time FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_id, media_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}

/// Look up a single line's end time for a given media file. Mirrors
/// `line_start_time`: None when no row exists OR the row's end_time is NULL.
pub fn line_end_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64> {
    conn.query_row(
        "SELECT end_time FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_id, media_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}

/// Earliest start_time on `media_id` strictly after `t` — the chat loop's
/// b-point fallback when a passage's last line has no end_time. Uses times,
/// not line ids, so no assumption about id ordering within a work.
pub fn next_start_after(conn: &Connection, media_id: i64, t: f64) -> Option<f64> {
    conn.query_row(
        "SELECT MIN(start_time) FROM line_timestamps \
         WHERE media_id = ?1 AND start_time > ?2",
        rusqlite::params![media_id, t],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_highlight_migration_and_writer() {
        let conn = Connection::open_in_memory().unwrap();
        // A works table WITHOUT the vocab_highlight column (legacy/fresh).
        conn.execute_batch(
            "CREATE TABLE works (
                abbrev TEXT UNIQUE NOT NULL, title TEXT NOT NULL,
                author TEXT, work_type TEXT NOT NULL);
             INSERT INTO works (abbrev,title,work_type) VALUES ('W1','One','prose');",
        ).unwrap();

        // Migration adds the column (DEFAULT 0 => existing/new rows read off).
        ensure_vocab_highlight_column(&conn).unwrap();
        let v: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, Some(0), "fresh-added column defaults rows to 0 (off)");

        // Writer flips the per-work value.
        set_vocab_highlight(&conn, "W1", true).unwrap();
        let v2: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v2, Some(1), "writer sets the column to 1");

        // Idempotent: a second ensure is a no-op and does NOT reset the value.
        ensure_vocab_highlight_column(&conn).unwrap();
        let v3: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v3, Some(1), "second ensure must not backfill/reset existing values");

        set_vocab_highlight(&conn, "W1", false).unwrap();
        let v4: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v4, Some(0), "writer clears the column to 0");
    }

    #[test]
    fn track_mark_column_roundtrips() {
        // Schema mirrors the MIGRATED lit.db: the column is is_track_mark.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_timestamps (
                id INTEGER PRIMARY KEY, citation TEXT, line_mapping_id INTEGER,
                media_id INTEGER, start_time REAL, end_time REAL, source TEXT,
                is_track_mark INTEGER DEFAULT 0,
                sentence_start_time REAL, sentence_end_time REAL,
                created_at TEXT, updated_at TEXT,
                UNIQUE(line_mapping_id, media_id)
            );",
        ).unwrap();

        // First toggle: inserts with is_track_mark=1 -> returns true.
        let on = upsert_chapter(&conn, 7, 100, "W.1.0.1", 1.5).unwrap();
        assert!(on);
        let v: i64 = conn.query_row(
            "SELECT is_track_mark FROM line_timestamps WHERE line_mapping_id=7 AND media_id=100",
            [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);

        // Second toggle: flips back to 0 -> returns false.
        let off = upsert_chapter(&conn, 7, 100, "W.1.0.1", 1.5).unwrap();
        assert!(!off);
    }

    #[test]
    fn search_lines_matches_substring_case_insensitive_with_limit() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_mapping (
                id INTEGER PRIMARY KEY, work_abbrev TEXT, canonical_text TEXT,
                div1 INTEGER, div2 INTEGER, line_in_div INTEGER, sub_line INTEGER
             );
             INSERT INTO line_mapping (id, work_abbrev, canonical_text, div1, div2, line_in_div, sub_line) VALUES
                (1, 'Ham', 'To be, or not to be', 3, 1, 56, 0),
                (2, 'Mac', 'Tomorrow and tomorrow', 5, 5, 19, 0),
                (3, 'Lr',  'Nothing will come of nothing', 1, 1, 92, 0);",
        ).unwrap();
        let hits = search_lines(&conn, "TOMORROW", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], ("Mac".to_string(), 5, 5, 19, "Tomorrow and tomorrow".to_string()));
        let all = search_lines(&conn, "o", 2).unwrap();
        assert_eq!(all.len(), 2);
        assert!(search_lines(&conn, "zzzz", 10).unwrap().is_empty());
    }

    #[test]
    fn line_start_time_reads_stored_value() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_timestamps (
                line_mapping_id INTEGER, media_id INTEGER, start_time REAL
             );
             INSERT INTO line_timestamps (line_mapping_id, media_id, start_time)
                VALUES (42, 7, 123.5);",
        )
        .unwrap();
        assert_eq!(line_start_time(&conn, 42, 7), Some(123.5));
        // Wrong media or missing line -> None.
        assert_eq!(line_start_time(&conn, 42, 99), None);
        assert_eq!(line_start_time(&conn, 1, 7), None);
    }

    #[test]
    fn upsert_spoken_status_inserts_then_updates() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_spoken_status (
                id INTEGER PRIMARY KEY,
                line_mapping_id INTEGER NOT NULL,
                media_id INTEGER NOT NULL,
                is_spoken INTEGER NOT NULL DEFAULT 1,
                confidence REAL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(line_mapping_id, media_id)
            );",
        )
        .unwrap();

        // Insert: row created with is_spoken=1, confidence=1.0
        upsert_spoken_status(&conn, 42, 7, true).unwrap();
        let (spoken, conf): (i64, f64) = conn
            .query_row(
                "SELECT is_spoken, confidence FROM line_spoken_status \
                 WHERE line_mapping_id = 42 AND media_id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(spoken, 1);
        assert_eq!(conf, 1.0);

        // Pre-existing not-spoken row gets flipped to spoken by upsert.
        conn.execute(
            "INSERT INTO line_spoken_status (line_mapping_id, media_id, is_spoken, confidence) \
             VALUES (99, 7, 0, 0.0)",
            [],
        )
        .unwrap();
        upsert_spoken_status(&conn, 99, 7, true).unwrap();
        let spoken2: i64 = conn
            .query_row(
                "SELECT is_spoken FROM line_spoken_status \
                 WHERE line_mapping_id = 99 AND media_id = 7",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(spoken2, 1);

        // No duplicate rows for the same (line, media).
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM line_spoken_status", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_open_db() {
        let conn = open_db().expect(OPEN_DB_PANIC_MSG);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM works", [], |row| row.get(0))
            .unwrap();
        assert!(count > 0, "works table should have rows");
    }

    #[test]
    fn test_list_works() {
        let conn = open_db().unwrap();
        let works = list_works(&conn).unwrap();
        assert!(works.len() > 100, "Should have 100+ works");
        assert!(works.iter().any(|w| w.abbrev == "Ham"));

        // Every listed work must have at least one associated media file — the
        // picker filters out media-less works. Verify none of the listed works
        // lacks a work_media_associations row, and that a known media-less work
        // (2H6, which has text but no audio) is excluded.
        for w in &works {
            let has_media: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM work_media_associations WHERE work_abbrev = ?1)",
                    [&w.abbrev],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(has_media, "listed work {} has no media association", w.abbrev);
        }
        assert!(
            !works.iter().any(|w| w.abbrev == "2H6"),
            "media-less work 2H6 should be filtered out of the picker"
        );

        // Edition-leak: a base work (AWW) whose only media is a single-play file
        // shared with its own edition (AWW-BBC) is hidden — reach it via the
        // edition. The Hamlet+Macbeth+Romeo bundle m4b is now associated with
        // the -BBCClassic editions (not the bases), so bases Rom/MND are
        // media-less and filtered like 2H6. Cym stays (base-only dedicated
        // file); Ham stays (dedicated media).
        assert!(
            !works.iter().any(|w| w.abbrev == "AWW"),
            "edition-leak base AWW should be filtered out (reach it via an edition)"
        );
        for gone in ["Rom", "MND"] {
            assert!(
                !works.iter().any(|w| w.abbrev == gone),
                "media-less base {gone} should be filtered (bundle moved to -BBCClassic)"
            );
        }
        for keep in ["Cym", "Ham", "Rom-BBCClassic"] {
            assert!(
                works.iter().any(|w| w.abbrev == keep),
                "{keep} should remain listed (has media association)"
            );
        }
    }

    #[test]
    fn test_is_bundle_media() {
        let conn = open_db().unwrap();
        // media_id 80 is the Hamlet+Macbeth+Romeo BBC m4b — a multi-work bundle.
        assert!(
            is_bundle_media(&conn, 80),
            "media 80 spans Ham/Mac/Rom — should be a bundle"
        );
        // A nonexistent media id is not a bundle (no rows -> false, no panic).
        assert!(!is_bundle_media(&conn, -1));
    }

    #[test]
    fn test_load_translations() {
        let conn = open_db().unwrap();
        let translations = load_translations(&conn, "Ham").unwrap();
        // Hamlet may or may not have translations — just verify no crash
        // and that the return type is correct
        assert!(translations.len() >= 0);
    }

    #[test]
    fn test_load_translations_ambrose_fallback() {
        // Use an in-memory DB fixture: the live lit.db no longer has translations
        // for any work that has an -Amb variant, so we construct the scenario
        // directly. The fallback joins -Amb lines to their base-work counterparts
        // by (div1, div2, normalized_text) and maps the translation to the -Amb id.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_mapping (
                id INTEGER PRIMARY KEY,
                work_abbrev TEXT NOT NULL,
                div1 INTEGER,
                div2 INTEGER,
                line_in_div INTEGER NOT NULL,
                sub_line INTEGER NOT NULL DEFAULT 0,
                canonical_text TEXT NOT NULL,
                normalized_text TEXT NOT NULL,
                speaker TEXT
             );
             CREATE TABLE line_translations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                line_mapping_id INTEGER NOT NULL REFERENCES line_mapping(id),
                translation TEXT NOT NULL,
                UNIQUE(line_mapping_id)
             );
             -- Base work (Err) line
             INSERT INTO line_mapping VALUES (1,'Err',1,1,1,0,'What is your will?','what is your will',NULL);
             -- -Amb counterpart: same (div1, div2, normalized_text), different id
             INSERT INTO line_mapping VALUES (2,'Err-Amb',1,1,1,0,'What is your will?','what is your will',NULL);
             -- Translation attached to the base-work line
             INSERT INTO line_translations (line_mapping_id, translation) VALUES (1,'Quid vis?');",
        ).unwrap();

        let translations = load_translations(&conn, "Err-Amb").unwrap();
        assert!(
            !translations.is_empty(),
            "Err-Amb should get translations via -Amb fallback to Err"
        );
        // The key must be the Err-Amb line_mapping.id (2), not Err's (1)
        assert!(
            translations.contains_key(&2),
            "Keys must be Err-Amb line_mapping.id, not Err's"
        );
        assert_eq!(
            translations.get(&2).map(|s| s.as_str()),
            Some("Quid vis?"),
            "Translation text must match what was stored on the base-work row"
        );
        assert!(
            !translations.contains_key(&1),
            "Err's id must not appear as a key"
        );
    }

    #[test]
    fn test_load_translations_production_variant_fallback() {
        // Any `<base>-<suffix>` production variant (not just -Amb) inherits the
        // base work's translations. Verifies the generalized rsplit_once('-')
        // fallback: Cym-BBC and Cym-Amb both fall back to Cym.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_mapping (
                id INTEGER PRIMARY KEY,
                work_abbrev TEXT NOT NULL,
                div1 INTEGER,
                div2 INTEGER,
                line_in_div INTEGER NOT NULL,
                sub_line INTEGER NOT NULL DEFAULT 0,
                canonical_text TEXT NOT NULL,
                normalized_text TEXT NOT NULL,
                speaker TEXT
             );
             CREATE TABLE line_translations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                line_mapping_id INTEGER NOT NULL REFERENCES line_mapping(id),
                translation TEXT NOT NULL,
                UNIQUE(line_mapping_id)
             );
             -- Base work (Cym) line
             INSERT INTO line_mapping VALUES (1,'Cym',1,1,1,0,'You do not meet a man but frowns.','you do not meet a man but frowns',NULL);
             -- BBC and Ambrose variants: same (div1, div2, line_in_div), different ids
             INSERT INTO line_mapping VALUES (2,'Cym-BBC',1,1,1,0,'You do not meet a man but frowns.','you do not meet a man but frowns',NULL);
             INSERT INTO line_mapping VALUES (3,'Cym-Amb',1,1,1,0,'You do not meet a man but frowns.','you do not meet a man but frowns',NULL);
             -- Translation attached only to the base-work line
             INSERT INTO line_translations (line_mapping_id, translation) VALUES (1,'Non incontri un uomo.');",
        ).unwrap();

        // -BBC (the previously-unhandled suffix) inherits Cym's translation.
        let bbc = load_translations(&conn, "Cym-BBC").unwrap();
        assert_eq!(
            bbc.get(&2).map(|s| s.as_str()),
            Some("Non incontri un uomo."),
            "Cym-BBC must inherit Cym's translation, keyed to the Cym-BBC line id"
        );
        assert!(!bbc.contains_key(&1), "Base work's id must not be a key");

        // -Amb still works via the same generalized path.
        let amb = load_translations(&conn, "Cym-Amb").unwrap();
        assert_eq!(
            amb.get(&3).map(|s| s.as_str()),
            Some("Non incontri un uomo."),
            "Cym-Amb must inherit Cym's translation, keyed to the Cym-Amb line id"
        );
    }

    #[test]
    fn test_load_vocab_words() {
        let conn = open_db().unwrap();
        let words = load_vocab_words(&conn, "Ham").unwrap();
        assert!(!words.is_empty(), "Should have vocab words for Hamlet");
    }

    #[test]
    fn test_load_vocab_definition() {
        let conn = open_db().unwrap();
        let words = load_vocab_words(&conn, "Ham").unwrap();
        if let Some(word) = words.iter().next() {
            let def = load_vocab_definition(&conn, word);
            let _ = def;
        }
    }

    #[test]
    fn test_load_vocab_word_list() {
        let conn = open_db().unwrap();
        let list = load_vocab_word_list(&conn, "Ham").unwrap();
        if list.len() > 1 {
            assert!(list[0].0 <= list[1].0, "Should be alphabetically sorted");
        }
    }

    /// `phrase_spans_for_line` returns every span for one (line, media) ordered
    /// by start_time; an unknown pair yields an empty vec (a cacheable negative
    /// result for the phrase-highlight driver).
    #[test]
    fn phrase_spans_for_line_returns_ordered_spans() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE phrase_timestamps (
                 id INTEGER PRIMARY KEY, line_mapping_id INTEGER, media_id INTEGER,
                 start_time REAL, end_time REAL, start_char INTEGER, end_char INTEGER);
             INSERT INTO phrase_timestamps
                 (line_mapping_id, media_id, start_time, end_time, start_char, end_char)
             VALUES (7, 3, 12.0, 13.5, 20, 40),
                    (7, 3, 10.0, 11.8, 0, 20),
                    (7, 3, 15.0, 17.0, 40, 60),
                    (8, 3, 99.0, 99.5, 0, 10),
                    (7, 4, 50.0, 51.0, 0, 20);",
        )
        .unwrap();
        let spans = phrase_spans_for_line(&conn, 7, 3);
        assert_eq!(spans.len(), 3);
        // Ordered by start_time regardless of insert order.
        assert_eq!(
            spans[0],
            PhraseSpan { start_time: 10.0, end_time: 11.8, start_char: 0, end_char: 20 }
        );
        assert_eq!(spans[1].start_char, 20);
        assert_eq!(spans[2].end_char, 60);
        // No rows -> empty vec (valid negative result).
        assert!(phrase_spans_for_line(&conn, 999, 3).is_empty());
    }

    /// `phrase_crossing_time` resolves the FIRST phrase whose char range
    /// extends past the boundary char offset (`end_char > char_off`, ordered by
    /// start_char) — the Task-9 downstream contract for firing a prose page
    /// turn. It ALWAYS yields that phrase's start_time, straddler included:
    /// the page turns the moment the first word of a phrase continuing onto
    /// the next page is highlighted (not at a mid-phrase interpolated
    /// crossing, which parked the tint on the old page while the phrase's
    /// continuation was narrated off-screen).
    #[test]
    fn phrase_crossing_time_picks_first_phrase_past_offset() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE phrase_timestamps (
                id INTEGER PRIMARY KEY, line_mapping_id INTEGER, media_id INTEGER,
                start_time REAL, end_time REAL, start_char INTEGER, end_char INTEGER);
             -- one line (id 7) on media 3, four contiguous phrases 0..80.
             INSERT INTO phrase_timestamps
               (line_mapping_id,media_id,start_time,end_time,start_char,end_char) VALUES
               (7,3, 10.0,12.0,  0,20),
               (7,3, 12.0,15.0, 20,40),
               (7,3, 15.0,18.0, 40,60),
               (7,3, 18.0,21.0, 60,80),
               -- a different media_id must not leak in.
               (7,9, 99.0,99.5, 30,50);",
        ).unwrap();

        // Offset 0: the very first phrase (end_char 20 > 0).
        assert_eq!(phrase_crossing_time(&conn, 7, 3, 0), Some(10.0));
        // Offset 20: phrase 1 ends exactly at 20 (NOT > 20); the crossing phrase
        // is phrase 2 (start_char 20, end_char 40) at t=12.0.
        assert_eq!(phrase_crossing_time(&conn, 7, 3, 20), Some(12.0));
        // Offset 25: STRADDLES phrase 2 (chars 20..40, t 12..15) — still that
        // phrase's start_time, so the turn fires as its first word highlights.
        assert_eq!(phrase_crossing_time(&conn, 7, 3, 25), Some(12.0));
        // Offset 55: straddles phrase 3 (40..60, t 15..18) -> its start, 15.0.
        assert_eq!(phrase_crossing_time(&conn, 7, 3, 55), Some(15.0));
        // Offset 79: straddles the last phrase (60..80, t 18..21) -> 18.0.
        assert_eq!(phrase_crossing_time(&conn, 7, 3, 79), Some(18.0));
        // Offset 80: no phrase extends past 80 -> None (caller interpolates).
        assert_eq!(phrase_crossing_time(&conn, 7, 3, 80), None);
        // Unknown (line, media) pair -> None.
        assert_eq!(phrase_crossing_time(&conn, 7, 1, 0), None);
        assert_eq!(phrase_crossing_time(&conn, 8, 3, 0), None);
    }

    /// Isolated in-memory DB for the bookmark tests: a stub `line_mapping` (only
    /// the columns `load_bookmarks_with_details` JOINs) with one Hamlet line at
    /// id 100, plus the real `bookmarks` schema. Using a fresh connection per
    /// test removes the shared-fixture race that made `test_bookmark_toggle`
    /// flake — the two bookmark tests previously toggled the SAME (Ham, LIMIT-1)
    /// row on the shared real lit.db in parallel, reading each other's writes.
    fn bookmark_fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            // `works` is the FK target of `bookmarks` (work_abbrev REFERENCES
            // works(abbrev)); SQLite resolves the FK table at insert time even
            // with enforcement off, so the stub must include it.
            "CREATE TABLE works (abbrev TEXT PRIMARY KEY);
             INSERT INTO works (abbrev) VALUES ('Ham');
             CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, work_abbrev TEXT,
               div1 INTEGER, div2 INTEGER, line_in_div INTEGER, canonical_text TEXT,
               speaker TEXT, sub_line INTEGER);
             INSERT INTO line_mapping
               VALUES (100,'Ham',1,1,1,'Who''s there?','BARNARDO',0);",
        )
        .unwrap();
        ensure_bookmarks_table(&conn).expect("Failed to create bookmarks table");
        conn
    }

    #[test]
    fn test_bookmark_toggle() {
        let conn = bookmark_fixture();
        let work_abbrev = "Ham";
        let line_id: i64 = 100;

        // Toggle on
        let added = toggle_bookmark(&conn, work_abbrev, line_id).unwrap();
        assert!(added, "First toggle should add bookmark");

        // Should appear in load_bookmarks
        let bookmarks = load_bookmarks(&conn, work_abbrev).unwrap();
        assert!(bookmarks.contains(&line_id));

        // Should be the most recent
        let recent = most_recent_bookmark(&conn, work_abbrev).unwrap();
        assert_eq!(recent, Some(line_id));

        // Toggle off
        let removed = toggle_bookmark(&conn, work_abbrev, line_id).unwrap();
        assert!(!removed, "Second toggle should remove bookmark");

        // Should no longer appear
        let bookmarks = load_bookmarks(&conn, work_abbrev).unwrap();
        assert!(!bookmarks.contains(&line_id));
    }

    #[test]
    fn test_load_bookmarks_with_details() {
        let conn = bookmark_fixture();
        let work_abbrev = "Ham";
        let line_id: i64 = 100;

        // Add a bookmark
        toggle_bookmark(&conn, work_abbrev, line_id).unwrap();

        // Load with details
        let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
        let found = items.iter().find(|i| i.line_mapping_id == line_id);
        assert!(found.is_some(), "Should find the bookmarked line");
        let item = found.unwrap();
        assert!(!item.line_text.is_empty(), "Line text should not be empty");
        assert!(!item.citation.is_empty(), "citation should not be empty");

        // Delete it
        delete_bookmark(&conn, work_abbrev, line_id).unwrap();
        let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
        assert!(
            !items.iter().any(|i| i.line_mapping_id == line_id),
            "Bookmark should be deleted"
        );
    }

    #[test]
    fn gloss_audio_roundtrip_and_upsert() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // gloss_audio references glosses(id); create a minimal glosses table for the FK.
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (4823);",
        )
        .unwrap();
        ensure_gloss_audio_table(&conn).unwrap();

        // Miss before insert.
        assert_eq!(find_gloss_audio(&conn, 4823, "explication", 0, "voiceA").unwrap(), None);

        // Insert, then hit.
        save_gloss_audio(&conn, 4823, "explication", 0, "/tmp/a/0.mp3", "voiceA", "modelA").unwrap();
        assert_eq!(
            find_gloss_audio(&conn, 4823, "explication", 0, "voiceA").unwrap(),
            Some("/tmp/a/0.mp3".to_string())
        );

        // Upsert: same (gloss_id, kind, paragraph_index, voice_id) replaces the path.
        save_gloss_audio(&conn, 4823, "explication", 0, "/tmp/a/0b.mp3", "voiceA", "modelB").unwrap();
        assert_eq!(
            find_gloss_audio(&conn, 4823, "explication", 0, "voiceA").unwrap(),
            Some("/tmp/a/0b.mp3".to_string())
        );

        // Distinct paragraph_index is a separate row.
        save_gloss_audio(&conn, 4823, "explication", 1, "/tmp/a/1.mp3", "voiceA", "modelA").unwrap();
        assert_eq!(
            find_gloss_audio(&conn, 4823, "explication", 1, "voiceA").unwrap(),
            Some("/tmp/a/1.mp3".to_string())
        );
    }

    #[test]
    fn delete_gloss_audio_removes_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (7);",
        )
        .unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        save_gloss_audio(&conn, 7, "explication", 0, "/tmp/7/0.mp3", "v", "m").unwrap();
        save_gloss_audio(&conn, 7, "explication", 1, "/tmp/7/1.mp3", "v", "m").unwrap();
        assert!(find_gloss_audio(&conn, 7, "explication", 0, "v").unwrap().is_some());
        let removed = delete_gloss_audio(&conn, 7).unwrap();
        assert_eq!(removed, 2, "should report both deleted audio rows");
        assert!(find_gloss_audio(&conn, 7, "explication", 0, "v").unwrap().is_none());
        assert!(find_gloss_audio(&conn, 7, "explication", 1, "v").unwrap().is_none());
    }

    #[test]
    fn delete_gloss_audio_block_scopes_to_one_block() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (7);",
        ).unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        let ins = |kind: &str, idx: i64, voice: &str, path: &str| {
            conn.execute(
                "INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
                 VALUES (7, ?1, ?2, ?3, ?4, 'm')",
                rusqlite::params![kind, idx, path, voice],
            ).unwrap();
        };
        ins("source", 0, "vA", "/a0.mp3");
        ins("source", 0, "vB", "/a0b.mp3"); // same block, second voice
        ins("source", 1, "vA", "/a1.mp3");  // different block — must survive
        let paths = delete_gloss_audio_block(&conn, 7, "source", 0).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/a0.mp3".to_string()));
        assert!(paths.contains(&"/a0b.mp3".to_string()));
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM gloss_audio WHERE gloss_id=7", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn gloss_audio_kind_distinct_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (9);",
        )
        .unwrap();
        ensure_gloss_audio_table(&conn).unwrap();

        save_gloss_audio(&conn, 9, "explication", 0, "/e0.mp3", "v", "m").unwrap();
        save_gloss_audio(&conn, 9, "source", 0, "/s0.mp3", "v", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 9, "explication", 0, "v").unwrap(), Some("/e0.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 9, "source", 0, "v").unwrap(), Some("/s0.mp3".to_string()));

        save_gloss_audio(&conn, 9, "source", 0, "/s0b.mp3", "v", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 9, "source", 0, "v").unwrap(), Some("/s0b.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 9, "explication", 0, "v").unwrap(), Some("/e0.mp3".to_string()));
    }

    #[test]
    fn gloss_audio_caches_per_voice() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (1);",
        ).unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        // two voices for the SAME (gloss, kind, index) coexist as separate rows
        save_gloss_audio(&conn, 1, "source", 0, "/a.mp3", "vA", "m1").unwrap();
        save_gloss_audio(&conn, 1, "source", 0, "/b.mp3", "vB", "m2").unwrap();
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vA").unwrap(), Some("/a.mp3".into()));
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vB").unwrap(), Some("/b.mp3".into()));
        // re-saving the same (gloss,kind,index,voice) overwrites just that one
        save_gloss_audio(&conn, 1, "source", 0, "/a2.mp3", "vA", "m1").unwrap();
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vA").unwrap(), Some("/a2.mp3".into()));
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vB").unwrap(), Some("/b.mp3".into()));
        // a voice with no cached row -> None
        assert_eq!(find_gloss_audio(&conn, 1, "source", 0, "vZ").unwrap(), None);
    }

    #[test]
    fn gloss_audio_migrates_unique_key_to_per_voice() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (5);",
        ).unwrap();
        // Pre-per-voice shape: has `kind`, but 3-column UNIQUE (no voice_id).
        conn.execute_batch(
            "CREATE TABLE gloss_audio (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gloss_id INTEGER NOT NULL,
                kind TEXT NOT NULL DEFAULT 'explication',
                paragraph_index INTEGER NOT NULL,
                audio_path TEXT NOT NULL,
                voice_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(gloss_id, kind, paragraph_index)
            );
            INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
                VALUES (5, 'source', 0, '/old.mp3', 'vA', 'm');",
        ).unwrap();

        ensure_gloss_audio_table(&conn).unwrap();
        // Existing row preserved under its voice.
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vA").unwrap(), Some("/old.mp3".to_string()));
        // A second voice now coexists (was impossible under the old UNIQUE).
        save_gloss_audio(&conn, 5, "source", 0, "/new.mp3", "vB", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vA").unwrap(), Some("/old.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vB").unwrap(), Some("/new.mp3".to_string()));

        // Idempotent: a second ensure does not re-migrate or lose data.
        ensure_gloss_audio_table(&conn).unwrap();
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vA").unwrap(), Some("/old.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 5, "source", 0, "vB").unwrap(), Some("/new.mp3".to_string()));
    }

    #[test]
    fn gloss_audio_migrates_legacy_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (3);",
        )
        .unwrap();
        // Legacy table shape (no `kind` column), with one row.
        conn.execute_batch(
            "CREATE TABLE gloss_audio (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gloss_id INTEGER NOT NULL,
                paragraph_index INTEGER NOT NULL,
                audio_path TEXT NOT NULL,
                voice_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(gloss_id, paragraph_index)
            );
            INSERT INTO gloss_audio (gloss_id, paragraph_index, audio_path, voice_id, model_id)
                VALUES (3, 0, '/legacy0.mp3', 'v', 'm');",
        )
        .unwrap();

        ensure_gloss_audio_table(&conn).unwrap();
        assert_eq!(find_gloss_audio(&conn, 3, "explication", 0, "v").unwrap(), Some("/legacy0.mp3".to_string()));
        save_gloss_audio(&conn, 3, "source", 0, "/s.mp3", "v", "m").unwrap();
        assert_eq!(find_gloss_audio(&conn, 3, "source", 0, "v").unwrap(), Some("/s.mp3".to_string()));

        // Idempotent: a second ensure call is a no-op and preserves data.
        ensure_gloss_audio_table(&conn).unwrap();
        assert_eq!(find_gloss_audio(&conn, 3, "explication", 0, "v").unwrap(), Some("/legacy0.mp3".to_string()));
        assert_eq!(find_gloss_audio(&conn, 3, "source", 0, "v").unwrap(), Some("/s.mp3".to_string()));
    }

    #[test]
    fn test_load_work_hamlet() {
        let conn = open_db().unwrap();
        // Ham-Arkangel, not bare Ham: the timestamps assertion needs an
        // edition that owns line_timestamps rows, and after the per-edition
        // split the bare Ham abbrev has none (they live on Ham-Argo,
        // Ham-Arkangel, Ham-BBCClassic, Ham-Naxos).
        let work = load_work(&conn, "Ham-Arkangel").unwrap();
        assert_eq!(work.title, "Hamlet (Arkangel)");
        assert_eq!(work.work_type, "play");
        assert!(work.lines.len() > 4000, "Hamlet should have 4000+ lines");
        // With sub_line ordering, line[0] is now the opening stage direction
        // "[Enter Barnardo and Francisco, two sentinels.]"
        assert!(
            work.lines[0].text.starts_with("[Enter Barnardo"),
            "First line should be the opening stage direction, got: {:?}",
            work.lines[0].text,
        );
        assert!(work.lines[0].sub_line > 0, "Opening stage direction should have sub_line > 0");
        assert!(!work.lines[0].is_dialogue, "Stage direction must not be dialogue");
        // The first spoken dialogue line is "Who's there?"
        let first_dialogue = work.lines.iter().find(|l| l.is_dialogue).unwrap();
        assert_eq!(first_dialogue.text, "Who\u{2019}s there?");
        assert!(!work.timestamps.is_empty(), "Work should have timestamps loaded");
    }

    #[test]
    fn load_work_vocab_highlight_matches_column() {
        let conn = open_db().unwrap();
        // Read the raw column for a work known to exist in lit.db.
        let raw: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev = 'Ham'", [], |r| r.get(0))
            .unwrap();
        let expected = raw.unwrap_or(0) == 1;
        let work = load_work(&conn, "Ham").unwrap();
        assert_eq!(
            work.vocab_highlight, expected,
            "Work.vocab_highlight must mirror the works.vocab_highlight column",
        );
    }

    #[test]
    fn ensure_characters_table_creates_usable_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_characters_table(&conn).unwrap();
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Ham','HAMLET','male')",
            [],
        ).unwrap();
        let g: String = conn
            .query_row(
                "SELECT gender FROM characters WHERE work_abbrev='Ham' AND speaker='HAMLET'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(g, "male");
    }

    #[test]
    fn characters_table_has_age_column() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_characters_table(&conn).unwrap();
        // age column exists and is nullable
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Ham','GHOST','male')",
            [],
        ).unwrap();
        let age: Option<i64> = conn
            .query_row("SELECT age FROM characters WHERE speaker='HAMLET'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(age, Some(30));
        let none: Option<i64> = conn
            .query_row("SELECT age FROM characters WHERE speaker='GHOST'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn characters_table_migrates_legacy_no_age() {
        let conn = Connection::open_in_memory().unwrap();
        // legacy 3-column table (pre-age) with a row
        conn.execute_batch(
            "CREATE TABLE characters (
                work_abbrev TEXT NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL,
                PRIMARY KEY (work_abbrev, speaker));
             -- 3-col positional INSERT is valid here: legacy table, pre-migration
             INSERT INTO characters VALUES ('Ham','HAMLET','male');",
        ).unwrap();
        ensure_characters_table(&conn).unwrap(); // should ALTER ADD age
        // existing row preserved, age NULL
        let (g, a): (String, Option<i64>) = conn
            .query_row("SELECT gender, age FROM characters WHERE speaker='HAMLET'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(g, "male");
        assert_eq!(a, None);
    }

    #[test]
    fn from_db_parses_lowercase_and_defaults_unknown() {
        use crate::elevenlabs::Gender;
        assert_eq!(Gender::from_db("male"), Gender::Male);
        assert_eq!(Gender::from_db("female"), Gender::Female);
        assert_eq!(Gender::from_db("neutral"), Gender::Neutral);
        assert_eq!(Gender::from_db("MALE"), Gender::Unknown);   // case-sensitive by design
        assert_eq!(Gender::from_db("garbage"), Gender::Unknown);
    }

    #[test]
    fn gloss_voices_toggle_add_remove_and_order() {
        let conn = Connection::open_in_memory().unwrap();
        // Parent table for the gloss_id FK (rusqlite enforces foreign keys).
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (1), (2);",
        )
        .unwrap();
        ensure_gloss_voices_table(&conn).unwrap();
        // add two voices -> both present, in insertion order
        assert!(toggle_gloss_voice(&conn, 1, "vA", "m1"));   // true = added
        assert!(toggle_gloss_voice(&conn, 1, "vB", "m2"));
        assert_eq!(
            get_gloss_voices(&conn, 1),
            vec![("vA".to_string(), "m1".to_string()), ("vB".to_string(), "m2".to_string())]
        );
        // toggling vA again removes it
        assert!(!toggle_gloss_voice(&conn, 1, "vA", "m1"));  // false = removed
        assert_eq!(get_gloss_voices(&conn, 1), vec![("vB".to_string(), "m2".to_string())]);
        // a different gloss has its own (empty) set
        assert!(get_gloss_voices(&conn, 2).is_empty());
    }

    #[test]
    fn gloss_voices_readd_goes_to_end() {
        let conn = Connection::open_in_memory().unwrap();
        // Parent table for the gloss_id FK (rusqlite enforces foreign keys).
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (1), (2);",
        )
        .unwrap();
        ensure_gloss_voices_table(&conn).unwrap();
        toggle_gloss_voice(&conn, 1, "vA", "m");  // pos 0
        toggle_gloss_voice(&conn, 1, "vB", "m");  // pos 1
        toggle_gloss_voice(&conn, 1, "vA", "m");  // remove vA
        toggle_gloss_voice(&conn, 1, "vA", "m");  // re-add vA -> pos 2 (after vB)
        assert_eq!(
            get_gloss_voices(&conn, 1),
            vec![("vB".to_string(), "m".to_string()), ("vA".to_string(), "m".to_string())],
            "re-added voice should sort after existing ones (end of cycle order)"
        );
    }

    #[test]
    fn voice_catalog_seeds_four_pairs() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_voice_catalog_table(&conn).unwrap();
        // 8 rows: 4 pairs x verse/prose
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 8);
        // Benedick prose (witty male, the older/default male) is present with its band
        let (vid, lo, hi): (String, i64, i64) = conn
            .query_row(
                "SELECT voice_id, age_min, age_max FROM voice_catalog \
                 WHERE gender='male' AND role='prose' AND age_min=26",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(vid, crate::elevenlabs::DEFAULT_MALE_VOICE_ID);
        assert_eq!((lo, hi), (26, 34));
        // idempotent: a second ensure does not duplicate rows
        ensure_voice_catalog_table(&conn).unwrap();
        let n2: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n2, 8);
    }

    fn seed_catalog_and_chars(conn: &Connection) {
        ensure_voice_catalog_table(conn).unwrap();
        ensure_characters_table(conn).unwrap();
        // works table with authors, for prose narrator resolution.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS works (
                abbrev TEXT UNIQUE NOT NULL,
                author TEXT,
                default_voice_id TEXT
            );
            INSERT INTO works (abbrev, author) VALUES ('Rom', 'Shakespeare');
            INSERT INTO works (abbrev, author) VALUES ('Lr', 'Shakespeare');
            INSERT INTO works (abbrev, author) VALUES ('Ham', 'Shakespeare');
            INSERT INTO works (abbrev, author) VALUES ('BCP1662', 'Book of Common Prayer');
            INSERT INTO works (abbrev, author) VALUES ('BCP1549M', 'Book of Common Prayer');
            INSERT INTO works (abbrev, author) VALUES ('OT', 'Charles Dickens');
            INSERT INTO works (abbrev, author, default_voice_id)
                VALUES ('OVERRIDE', 'Shakespeare', 'OVERRIDE_VOICE_XXXXX');"
        ).unwrap();
        // Re-run migration now that works exists (idempotent): the first call
        // above already created+seeded author_default_voice; this second call is
        // a no-op safety net proving idempotency with the works table present.
        ensure_voice_catalog_table(conn).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Rom','JULIET','female',14)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Lr','LEAR','male',80)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Rom','NURSE','female')", []).unwrap();
    }

    #[test]
    fn ensure_voice_catalog_adds_author_voice_schema() {
        let conn = Connection::open_in_memory().unwrap();
        // works table must exist for the ADD COLUMN to target.
        conn.execute_batch(
            "CREATE TABLE works (abbrev TEXT UNIQUE NOT NULL, author TEXT);"
        ).unwrap();
        ensure_voice_catalog_table(&conn).unwrap();
        // works.default_voice_id column now exists.
        let has_col: bool = conn
            .prepare("SELECT default_voice_id FROM works")
            .is_ok();
        assert!(has_col, "works.default_voice_id column should exist");
        // author_default_voice seeded Shakespeare -> Eleanor.
        let vid: String = conn
            .query_row(
                "SELECT voice_id FROM author_default_voice WHERE author = 'Shakespeare'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(vid, crate::elevenlabs::DEFAULT_FEMALE_VOICE_ID);
    }

    #[test]
    fn resolve_containment_picks_the_band_containing_age() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Juliet 14 female -> Juliet voice (12-19) verse
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", true),
            (crate::elevenlabs::JULIET_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_nearest_band_when_no_containment() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Lear 80 male VERSE: no band contains 80; nearest male band is Benedick
        // (26-34, distance 46) vs Romeo (15-25, distance 55) -> Benedick verse.
        // (Prose resolves separately — see resolve_prose_voice_precedence.)
        assert_eq!(
            resolve_default_voice(&conn, "Lr", "LEAR", true),
            (crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_null_age_uses_default_age_40() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Nurse female, NULL age -> DEFAULT_AGE 40. No female band contains 40
        // (Juliet 12-19, Eleanor 20-30); nearest is Eleanor (dist 10) verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "NURSE", true),
            (crate::elevenlabs::DEFAULT_FEMALE_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_unknown_gender_defaults_male() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // No characters row -> Unknown gender -> male; NULL age -> 40; no male band
        // contains 40, nearest is Benedick (26-34, dist 6) -> Benedick verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "NOBODY", true),
            (crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_neutral_gender_uses_male_voice() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Rom','CHORUS','neutral',40)",
            [],
        ).unwrap();
        // neutral -> male; age 40, no male band contains it, nearest is Benedick
        // (26-34, dist 6) -> Benedick verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "CHORUS", true),
            (crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn synopsis_audio_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_synopsis_audio_table(&conn).unwrap();

        // Miss before save.
        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit, None);

        save_synopsis_audio(
            &conn, "KingJohn", 4, 2, 0, "/tmp/a.mp3", "voice123", "eleven_v3",
        )
        .unwrap();

        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/a.mp3"));

        // Different voice is a separate cache entry → miss.
        let other = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voiceXYZ").unwrap();
        assert_eq!(other, None);

        // Upsert updates the path in place.
        save_synopsis_audio(
            &conn, "KingJohn", 4, 2, 0, "/tmp/b.mp3", "voice123", "eleven_v3",
        )
        .unwrap();
        let hit = find_synopsis_audio(&conn, "KingJohn", 4, 2, 0, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/b.mp3"));
    }

    #[test]
    fn journal_audio_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_journal_audio_table(&conn).unwrap();

        // Miss before save.
        assert_eq!(find_journal_audio(&conn, 42, 1, "voice123").unwrap(), None);

        save_journal_audio(&conn, 42, 1, "/tmp/a.mp3", "voice123", "eleven_v3").unwrap();
        let hit = find_journal_audio(&conn, 42, 1, "voice123").unwrap();
        assert_eq!(hit.as_deref(), Some("/tmp/a.mp3"));

        // Different voice / paragraph are separate cache entries → miss.
        assert_eq!(find_journal_audio(&conn, 42, 1, "voiceXYZ").unwrap(), None);
        assert_eq!(find_journal_audio(&conn, 42, 0, "voice123").unwrap(), None);

        // Upsert updates the path in place.
        save_journal_audio(&conn, 42, 1, "/tmp/b.mp3", "voice123", "eleven_v3").unwrap();
        assert_eq!(
            find_journal_audio(&conn, 42, 1, "voice123").unwrap().as_deref(),
            Some("/tmp/b.mp3")
        );

        // Delete returns the removed paths and clears the entry's rows.
        save_journal_audio(&conn, 42, 2, "/tmp/c.mp3", "voiceXYZ", "eleven_v3").unwrap();
        let mut removed = delete_journal_audio(&conn, 42).unwrap();
        removed.sort();
        assert_eq!(removed, vec!["/tmp/b.mp3".to_string(), "/tmp/c.mp3".to_string()]);
        assert_eq!(find_journal_audio(&conn, 42, 1, "voice123").unwrap(), None);
    }

    #[test]
    fn resolve_prose_voice_precedence() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        let model = crate::elevenlabs::OP_MODEL_ID.to_string();
        let eleanor = crate::elevenlabs::DEFAULT_FEMALE_VOICE_ID.to_string();
        let benedick = crate::elevenlabs::DEFAULT_MALE_VOICE_ID.to_string();

        // Shakespeare prose -> Eleanor (author_default_voice row).
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", false),
            (eleanor.clone(), model.clone())
        );
        // BCP prose -> Benedick (no author row -> global default).
        assert_eq!(
            resolve_default_voice(&conn, "BCP1662", "UNKNOWN", false),
            (benedick.clone(), model.clone())
        );
        // Other author (Dickens) prose -> Benedick (global default).
        assert_eq!(
            resolve_default_voice(&conn, "OT", "NOBODY", false),
            (benedick.clone(), model.clone())
        );
        // Per-work override beats the author default (even for Shakespeare).
        assert_eq!(
            resolve_default_voice(&conn, "OVERRIDE", "ANY", false),
            ("OVERRIDE_VOICE_XXXXX".to_string(), model.clone())
        );
        // Verse path is UNCHANGED: UNKNOWN verse -> male; Juliet verse -> female.
        assert_eq!(
            resolve_default_voice(&conn, "BCP1662", "UNKNOWN", true),
            (benedick.clone(), model.clone())
        );
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", true),
            (crate::elevenlabs::JULIET_VOICE_ID.to_string(), model)
        );
    }

    fn timestamps_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_timestamps (
                 id INTEGER PRIMARY KEY,
                 line_mapping_id INTEGER NOT NULL,
                 media_id INTEGER,
                 start_time REAL,
                 end_time REAL
             );
             INSERT INTO line_timestamps
                 (line_mapping_id, media_id, start_time, end_time) VALUES
                 (10, 1, 100.0, 103.5),
                 (11, 1, 104.0, NULL),
                 (12, 1, 108.0, 111.0),
                 (10, 2, 500.0, 502.0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn line_end_time_reads_the_media_scoped_row() {
        let conn = timestamps_test_conn();
        assert_eq!(line_end_time(&conn, 10, 1), Some(103.5));
        assert_eq!(line_end_time(&conn, 10, 2), Some(502.0));
        // NULL end_time and missing rows are both None, mirroring
        // line_start_time's contract.
        assert_eq!(line_end_time(&conn, 11, 1), None);
        assert_eq!(line_end_time(&conn, 99, 1), None);
    }

    #[test]
    fn next_start_after_is_the_earliest_strictly_later_start() {
        let conn = timestamps_test_conn();
        // After line 11's start (104.0) the next start on media 1 is 108.0.
        assert_eq!(next_start_after(&conn, 1, 104.0), Some(108.0));
        // Strictly after: a row AT t does not count.
        assert_eq!(next_start_after(&conn, 1, 108.0), None);
        // Media-scoped: media 2 has nothing after 502.
        assert_eq!(next_start_after(&conn, 2, 502.0), None);
    }
}

#[cfg(test)]
mod scansion_tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, work_abbrev TEXT,
               div1 INTEGER, div2 INTEGER, line_in_div INTEGER, canonical_text TEXT);
             CREATE TABLE line_meter (line_id INTEGER, syllable_count INTEGER,
               nominal_feet INTEGER, line_type TEXT, caesura_after INTEGER,
               is_rhymed INTEGER, confidence REAL, source_note TEXT);
             CREATE TABLE syllable_scan (line_id INTEGER, position INTEGER,
               foot_index INTEGER, ictus INTEGER, foot_type TEXT, surface TEXT,
               start_char INTEGER, end_char INTEGER, phenomenon TEXT,
               is_extrametrical INTEGER);
             INSERT INTO line_mapping VALUES (10,'TN',1,1,1,'If music');
             INSERT INTO line_meter (line_id,syllable_count,nominal_feet,line_type,caesura_after)
               VALUES (10,2,5,'regular',NULL);
             INSERT INTO syllable_scan (line_id,position,foot_index,ictus,surface,is_extrametrical)
               VALUES (10,1,1,0,'If',0),(10,2,1,1,'mu',0);
             INSERT INTO line_mapping VALUES (11,'TN',1,1,2,'O brave');
             INSERT INTO line_meter (line_id,syllable_count,nominal_feet,line_type,caesura_after)
               VALUES (11,2,5,'feminine_ending',1);
             INSERT INTO syllable_scan (line_id,position,foot_index,ictus,surface,is_extrametrical)
               VALUES (11,1,1,1,'O',0),(11,2,1,0,'brave',1);",
        ).unwrap();
        c
    }

    #[test]
    fn loads_scansion_keyed_by_line_id() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        let ls = map.get(&10).expect("line 10 present");
        assert_eq!(ls.line_type, "regular");
        assert_eq!(ls.caesura_after, None);
        assert_eq!(ls.syllables.len(), 2);
        assert_eq!(ls.syllables[1].ictus, 1);
        assert_eq!(ls.syllables[1].surface, "mu");
    }

    #[test]
    fn loads_caesura_and_extrametrical() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        let ls = map.get(&11).expect("line 11 present");
        assert_eq!(ls.line_type, "feminine_ending");
        assert_eq!(ls.caesura_after, Some(1));          // Option<i32> Some-branch
        assert_eq!(ls.syllables.len(), 2);
        assert!(!ls.syllables[0].is_extrametrical);     // 0 -> false
        assert!(ls.syllables[1].is_extrametrical);      // 1 -> true
        assert_eq!(ls.syllables[0].surface, "O");
    }

    #[test]
    fn unscanned_line_absent_from_map() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        assert!(map.get(&999).is_none());
    }
}

#[cfg(test)]
mod chapter_start_tests {
    use super::*;
    use rusqlite::Connection;

    fn mk() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, chapter_start INTEGER DEFAULT 0);
             INSERT INTO line_mapping (id, chapter_start) VALUES (7, 0);",
        ).unwrap();
        c
    }

    #[test]
    fn toggle_sets_then_clears() {
        let c = mk();
        assert_eq!(toggle_chapter_start(&c, 7).unwrap(), true);
        let v: i64 = c.query_row("SELECT chapter_start FROM line_mapping WHERE id=7", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
        assert_eq!(toggle_chapter_start(&c, 7).unwrap(), false);
        let v2: i64 = c.query_row("SELECT chapter_start FROM line_mapping WHERE id=7", [], |r| r.get(0)).unwrap();
        assert_eq!(v2, 0);
    }

    #[test]
    fn toggle_handles_null_as_zero() {
        let c = mk();
        c.execute("UPDATE line_mapping SET chapter_start = NULL WHERE id = 7", []).unwrap();
        assert_eq!(toggle_chapter_start(&c, 7).unwrap(), true);
    }
}

#[cfg(test)]
mod passages_div1_div2_tests {
    use super::*;

    #[test]
    fn glossed_passages_read_div1_div2_columns() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        // Schema mirrors the MIGRATED lit.db: columns are div1/div2 (was act/scene).
        conn.execute_batch(
            "CREATE TABLE passages (
                id INTEGER PRIMARY KEY, hash TEXT, work_abbrev TEXT,
                start_citation TEXT, end_citation TEXT, div1 INTEGER, div2 INTEGER,
                character TEXT, source_text TEXT
             );
             CREATE TABLE glosses (
                id INTEGER PRIMARY KEY, passage_id INTEGER, gloss_type TEXT,
                gloss_text TEXT, status TEXT, word_id INTEGER
             );
             INSERT INTO passages (id, hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text)
                VALUES (1, 'h', 'Err', 'Err.2.2.1', 'Err.2.2.12', 2, 2, 'Antipholus', 'text');
             INSERT INTO glosses (id, passage_id, gloss_type, gloss_text, status, word_id)
                VALUES (1, 1, 'reader-gloss', 'g', 'complete', NULL);",
        ).unwrap();
        let ps = find_glossed_passages(&conn, "Err", &["reader-gloss"]).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].act, 2);   // field name unchanged; value comes from div1 column
        assert_eq!(ps[0].scene, 2); // from div2 column
    }

    #[test]
    fn list_all_gloss_rows_loads_gloss_text() {
        let conn = open_db().unwrap();
        let rows = list_all_gloss_rows(&conn).unwrap();
        // Regression guard for the citation-only gap: gloss_text MUST be loaded.
        if let Some(r) = rows.first() {
            assert!(!r.gloss_text.is_empty());
            assert!(!r.work_abbrev.is_empty());
        }
    }

    /// Seed a minimal `works` table for the canonical-abbrev tests: Cymbeline
    /// with two variant editions, plus the two hyphenated-but-NOT-variant traps
    /// (`Mac-Ep-1` shares the `Mac` prefix with Macbeth but a DIFFERENT author;
    /// `Aen-MW` has no base work at all).
    fn seed_works(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "CREATE TABLE works (abbrev TEXT PRIMARY KEY, author TEXT);
             INSERT INTO works VALUES
                ('Cym', 'Shakespeare'),
                ('Cym-Amb', 'Shakespeare'),
                ('Cym-Arkangel', 'Shakespeare'),
                ('Cym-BBC', 'Shakespeare'),
                ('Mac', 'Shakespeare'),
                ('Mac-Ep-1', 'Diarmaid MacCulloch'),
                ('Aen-MW', 'Virgil (trans. McGill-Wright)');",
        )
        .unwrap();
    }

    #[test]
    fn preferred_arkangel_prefers_arkangel_when_it_exists() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        seed_works(&conn);
        // A base with an Arkangel sibling -> the Arkangel edition.
        assert_eq!(preferred_arkangel_abbrev(&conn, "Cym"), "Cym-Arkangel");
    }

    #[test]
    fn preferred_arkangel_falls_back_to_base_without_arkangel() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        seed_works(&conn);
        // Mac has no Mac-Arkangel row -> unchanged base.
        assert_eq!(preferred_arkangel_abbrev(&conn, "Mac"), "Mac");
        // Non-Shakespeare / unknown -> unchanged.
        assert_eq!(preferred_arkangel_abbrev(&conn, "Aen-MW"), "Aen-MW");
        assert_eq!(preferred_arkangel_abbrev(&conn, "Nope"), "Nope");
    }

    #[test]
    fn preferred_arkangel_is_idempotent_on_an_arkangel_abbrev() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        seed_works(&conn);
        // Already an Arkangel edition: appending again would seek the
        // non-existent Cym-Arkangel-Arkangel, so it stays put.
        assert_eq!(preferred_arkangel_abbrev(&conn, "Cym-Arkangel"), "Cym-Arkangel");
    }

    #[test]
    fn canonical_abbrev_shares_variants_and_keeps_non_variants() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        seed_works(&conn);
        // Variant editions collapse onto the base work.
        assert_eq!(canonical_work_abbrev(&conn, "Cym"), "Cym");
        assert_eq!(canonical_work_abbrev(&conn, "Cym-Amb"), "Cym");
        assert_eq!(canonical_work_abbrev(&conn, "Cym-BBC"), "Cym");
        // A different author's hyphenated abbrev must NOT collapse onto a
        // same-prefix work (Mac-Ep-1 is MacCulloch, Mac is Macbeth).
        assert_eq!(canonical_work_abbrev(&conn, "Mac-Ep-1"), "Mac-Ep-1");
        // No base work at all -> unchanged; unknown abbrev -> unchanged.
        assert_eq!(canonical_work_abbrev(&conn, "Aen-MW"), "Aen-MW");
        assert_eq!(canonical_work_abbrev(&conn, "Nope-X"), "Nope-X");
    }

    /// A gloss created on ANY edition is stored under — and found under — the
    /// canonical base abbrev, so all editions share it. The startup migration
    /// re-keys pre-fix rows stored under a variant abbrev (`Cym-BBC`),
    /// re-prefixes their citations, and recomputes the dedup hash
    /// (md5("{abbrev}:{start}:{end}:{gloss_type}")) so a future gloss on the
    /// same lines still dedups onto the migrated passage.
    #[test]
    fn migration_rekeys_variant_passages_to_base() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        seed_works(&conn);
        let old_hash = format!(
            "{:x}",
            md5::compute("Cym-BBC:Cym-BBC.1.1.1:Cym-BBC.1.1.3:reader-gloss".as_bytes())
        );
        conn.execute_batch(&format!(
            "CREATE TABLE passages (
                id INTEGER PRIMARY KEY, hash TEXT UNIQUE, work_abbrev TEXT,
                start_citation TEXT, end_citation TEXT, div1 INTEGER, div2 INTEGER,
                character TEXT, source_text TEXT
             );
             CREATE TABLE glosses (
                id INTEGER PRIMARY KEY, passage_id INTEGER, gloss_type TEXT,
                gloss_text TEXT, status TEXT, word_id INTEGER
             );
             CREATE TABLE journal_entries (
                id INTEGER PRIMARY KEY, work_abbrev TEXT, div1 INTEGER, div2 INTEGER,
                question TEXT, answer TEXT, scope TEXT,
                start_citation TEXT, end_citation TEXT, source_text TEXT
             );
             CREATE TABLE scene_synopses (
                id INTEGER PRIMARY KEY, work_abbrev TEXT, div1 INTEGER, div2 INTEGER,
                synopsis TEXT, UNIQUE(work_abbrev, div1, div2)
             );
             INSERT INTO passages (id, hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text)
                VALUES (1, '{old_hash}', 'Cym-BBC', 'Cym-BBC.1.1.1', 'Cym-BBC.1.1.3', 1, 1, 'FIRST GENTLEMAN', 'text');
             INSERT INTO glosses (id, passage_id, gloss_type, gloss_text, status, word_id)
                VALUES (1, 1, 'reader-gloss', 'g', 'complete', NULL);
             -- journal row already keyed by the base but carrying VARIANT citations
             INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope, start_citation, end_citation)
                VALUES (1, 'Cym', 1, 1, 'q', 'a', 'passage', 'Cym-BBC.1.1.1', 'Cym-BBC.1.1.3');
             -- variant synopsis colliding with an existing base row: base wins
             INSERT INTO scene_synopses (work_abbrev, div1, div2, synopsis) VALUES
                ('Cym', 1, 1, 'base'), ('Cym-BBC', 1, 1, 'variant'), ('Cym-BBC', 1, 2, 'only-variant');"
        ))
        .unwrap();

        ensure_canonical_artifact_abbrevs(&conn).unwrap();

        // Passage re-keyed + citations re-prefixed + hash recomputed.
        let (abbrev, start, end, hash): (String, String, String, String) = conn
            .query_row(
                "SELECT work_abbrev, start_citation, end_citation, hash FROM passages WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(abbrev, "Cym");
        assert_eq!(start, "Cym.1.1.1");
        assert_eq!(end, "Cym.1.1.3");
        assert_eq!(
            hash,
            format!("{:x}", md5::compute("Cym:Cym.1.1.1:Cym.1.1.3:reader-gloss".as_bytes()))
        );
        // Every edition now finds it; the variant abbrev no longer matches.
        assert_eq!(find_glossed_passages(&conn, "Cym", &["reader-gloss"]).unwrap().len(), 1);
        assert_eq!(find_glossed_passages(&conn, "Cym-BBC", &["reader-gloss"]).unwrap().len(), 0);

        // Journal citations re-prefixed (work_abbrev was already the base).
        let (js, je): (String, String) = conn
            .query_row(
                "SELECT start_citation, end_citation FROM journal_entries WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((js.as_str(), je.as_str()), ("Cym.1.1.1", "Cym.1.1.3"));

        // Synopses: collision keeps the base row, non-colliding variant re-keyed.
        let rows: Vec<(String, i64, i64, String)> = conn
            .prepare("SELECT work_abbrev, div1, div2, synopsis FROM scene_synopses ORDER BY div1, div2")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("Cym".to_string(), 1, 1, "base".to_string()),
                ("Cym".to_string(), 1, 2, "only-variant".to_string()),
            ]
        );
    }
}

#[cfg(test)]
mod gloss_ordering_tests {
    use super::*;

    /// Two reader-glosses on ONE passage sharing a timestamp: `CURRENT_TIMESTAMP`
    /// has one-second granularity, so reglossing twice in a second ties. The
    /// newest (highest id) must still win.
    ///
    /// The rows are inserted in REVERSE id order (id 2 first) on purpose: with
    /// only `timestamp DESC` to go on, SQLite's tie order tracks the scan, so
    /// the pre-fix query returns 'older' first and this test genuinely FAILS
    /// red. Inserting in ascending id order could pass by luck and prove
    /// nothing.
    #[test]
    fn same_timestamp_glosses_order_newest_id_first() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE passages (
                id INTEGER PRIMARY KEY, hash TEXT, work_abbrev TEXT,
                start_citation TEXT, end_citation TEXT, div1 INTEGER, div2 INTEGER,
                character TEXT, source_text TEXT
             );
             CREATE TABLE glosses (
                id INTEGER PRIMARY KEY, passage_id INTEGER, gloss_type TEXT,
                gloss_text TEXT, status TEXT, word_id INTEGER,
                claude_model TEXT, timestamp TEXT
             );
             INSERT INTO passages (id, hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text)
                VALUES (1, 'h', 'Err', 'Err.2.2.1', 'Err.2.2.12', 2, 2, 'Antipholus', 'text');
             INSERT INTO glosses (id, passage_id, gloss_type, gloss_text, status, word_id, claude_model, timestamp)
                VALUES (2, 1, 'reader-gloss', 'newer', 'complete', NULL, 'm', '2026-07-16 10:00:00'),
                       (1, 1, 'reader-gloss', 'older', 'complete', NULL, 'm', '2026-07-16 10:00:00');",
        ).unwrap();

        let gs = find_glosses_by_start(&conn, "Err", "Err.2.2.1", &["reader-gloss"]).unwrap();
        assert_eq!(gs.len(), 2);
        assert_eq!(gs[0].gloss_text, "newer");
        assert_eq!(gs[0].gloss_id, 2);
        assert_eq!(gs[1].gloss_text, "older");
    }

    /// The pre-existing ordering rules must survive the new tiebreak:
    /// reader-gloss outranks other types, and a newer timestamp still wins.
    #[test]
    fn reader_gloss_and_timestamp_still_outrank_id() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE passages (
                id INTEGER PRIMARY KEY, hash TEXT, work_abbrev TEXT,
                start_citation TEXT, end_citation TEXT, div1 INTEGER, div2 INTEGER,
                character TEXT, source_text TEXT
             );
             CREATE TABLE glosses (
                id INTEGER PRIMARY KEY, passage_id INTEGER, gloss_type TEXT,
                gloss_text TEXT, status TEXT, word_id INTEGER,
                claude_model TEXT, timestamp TEXT
             );
             INSERT INTO passages (id, hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text)
                VALUES (1, 'h', 'Err', 'Err.2.2.1', 'Err.2.2.12', 2, 2, 'Antipholus', 'text');
             INSERT INTO glosses (id, passage_id, gloss_type, gloss_text, status, word_id, claude_model, timestamp)
                VALUES (9, 1, 'teacher-generic', 'teacher', 'complete', NULL, 'm', '2026-07-16 12:00:00'),
                       (1, 1, 'reader-gloss', 'old-reader', 'complete', NULL, 'm', '2026-07-16 10:00:00'),
                       (2, 1, 'reader-gloss', 'new-reader', 'complete', NULL, 'm', '2026-07-16 11:00:00');",
        ).unwrap();

        let gs = find_glosses_by_start(
            &conn, "Err", "Err.2.2.1", &["teacher-generic", "reader-gloss"],
        ).unwrap();
        assert_eq!(gs.len(), 3);
        // reader-gloss first (despite the teacher gloss having the newest timestamp)
        assert_eq!(gs[0].gloss_text, "new-reader");
        assert_eq!(gs[1].gloss_text, "old-reader");
        assert_eq!(gs[2].gloss_text, "teacher");
    }
}

#[cfg(test)]
mod vocab_insert_tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE vocab_words (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL UNIQUE,
                definition TEXT NOT NULL,
                difficulty_level INTEGER,
                created_at TEXT DEFAULT (datetime('now')),
                source TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn insert_new_word_reports_added() {
        let conn = mem_db();
        let out = insert_vocab_word(&conn, "brave", "courageous", "wordnet").unwrap();
        assert!(matches!(out, VocabInsertOutcome::Added));
        let def: String = conn
            .query_row("SELECT definition FROM vocab_words WHERE word='brave'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(def, "courageous");
    }

    #[test]
    fn reinsert_keeps_good_definition_reports_already_present() {
        let conn = mem_db();
        insert_vocab_word(&conn, "brave", "courageous", "wordnet").unwrap();
        let out = insert_vocab_word(&conn, "brave", "SOMETHING ELSE", "claude").unwrap();
        assert!(matches!(out, VocabInsertOutcome::AlreadyPresent));
        let def: String = conn
            .query_row("SELECT definition FROM vocab_words WHERE word='brave'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(def, "courageous"); // unchanged
    }

    #[test]
    fn reinsert_fills_empty_definition() {
        let conn = mem_db();
        insert_vocab_word(&conn, "brave", "", "wordnet").unwrap();
        let out = insert_vocab_word(&conn, "brave", "courageous", "gcide").unwrap();
        assert!(matches!(out, VocabInsertOutcome::Added));
        let def: String = conn
            .query_row("SELECT definition FROM vocab_words WHERE word='brave'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(def, "courageous");
    }

    #[test]
    fn case_variant_updates_capitalization_not_a_duplicate() {
        // Proper noun stored lowercase historically; re-adding it capitalized
        // must retype the ONE row (typed case wins), never insert a second.
        let conn = mem_db();
        insert_vocab_word(&conn, "michaelmas", "a Christian feast", "claude").unwrap();
        let out = insert_vocab_word(&conn, "Michaelmas", "ignored", "claude").unwrap();
        assert!(matches!(out, VocabInsertOutcome::AlreadyPresent));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM vocab_words", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let (word, def): (String, String) = conn
            .query_row("SELECT word, definition FROM vocab_words", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(word, "Michaelmas"); // capitalization updated in place
        assert_eq!(def, "a Christian feast"); // definition untouched
    }
}
