//! Prose chapter-start toggle: flip line_mapping.chapter_start on the cursor's
//! paragraph, re-derive (div1,div2) via the litdb tool, reload the work in
//! place. Prose-only.

use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::cell::RefCell;

use crate::app::AppState;
use crate::db::line_types::is_prose_work;

/// Build the `chapter_divisions.py derive` subprocess for a work. Pure so it is
/// unit-testable; the litdb checkout is assumed at <home>/utono/litdb.
pub(crate) fn litdb_derive_command(home: &Path, abbrev: &str) -> Command {
    let script = home.join("utono/litdb/scripts/chapter_divisions.py");
    let mut cmd = Command::new("python3");
    cmd.arg(script).arg("derive").arg("--work").arg(abbrev);
    cmd
}

/// Toggle whether the cursor's paragraph begins a chapter, then re-derive the
/// work's chapter divisions and reload in place (prose only).
pub fn toggle_chapter_start(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    // --- resolve everything from a short borrow ---
    let resolved = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        if !is_prose_work(&work.work_type) {
            crate::log_fmt!(
                "chapter_start: ignored (work '{}' is not prose)",
                work.abbrev
            );
            return;
        }
        let buffer_line = s.current_line;
        let lm_id = match s.line_mapping_id_for_buffer(buffer_line) {
            Some(id) => id,
            None => {
                crate::log_fmt!(
                    "chapter_start: no line_mapping id for buffer line {}",
                    buffer_line
                );
                return;
            }
        };
        (work.abbrev.clone(), lm_id)
    };
    let (abbrev, lm_id) = resolved;

    let handle = tokio_handle.clone();
    let state_rc = Rc::clone(state);
    glib::spawn_future_local(async move {
        let abbrev_clone = abbrev.clone();
        let derive_result = handle
            .spawn_blocking(move || -> Result<bool, String> {
                // 1. Toggle the column (own connection, dropped before derive).
                let new_state = {
                    let conn = crate::db::queries::open_db_rw()
                        .map_err(|e| format!("open_db_rw: {e}"))?;
                    crate::db::queries::toggle_chapter_start(&conn, lm_id)
                        .map_err(|e| format!("toggle: {e}"))?
                    // conn dropped here — SQLite write committed
                };
                // 2. Re-derive divisions via the litdb tool.
                let home = std::env::var("HOME").map_err(|e| format!("HOME: {e}"))?;
                let out = litdb_derive_command(Path::new(&home), &abbrev_clone)
                    .output()
                    .map_err(|e| format!("spawn derive: {e}"))?;
                if !out.status.success() {
                    return Err(format!(
                        "derive failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                Ok(new_state)
            })
            .await;

        match derive_result {
            Ok(Ok(now_marked)) => {
                crate::log_fmt!(
                    "chapter_start: {} -> {}",
                    abbrev,
                    if now_marked { "set" } else { "cleared" }
                );
                // Reload in place, cursor restored to same line_mapping row.
                // load_work_at spawns its own future; just call it.
                crate::input::actions::pickers::load_work_at(
                    &state_rc,
                    &handle,
                    abbrev,
                    Some(lm_id),
                );
            }
            Ok(Err(e)) => crate::log_fmt!("chapter_start: {}", e),
            Err(e) => crate::log_fmt!("chapter_start: join error: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::litdb_derive_command;
    use std::path::Path;

    #[test]
    fn builds_derive_command() {
        let cmd = litdb_derive_command(Path::new("/home/u"), "Cromwell");
        assert_eq!(cmd.get_program(), "python3");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_str().unwrap().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "/home/u/utono/litdb/scripts/chapter_divisions.py".to_string(),
                "derive".to_string(),
                "--work".to_string(),
                "Cromwell".to_string(),
            ]
        );
    }
}
