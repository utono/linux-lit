//! Dedicated chat-panel MPV player: a SECOND mpv process on a `chat-`-marked
//! socket, driven write-only (connect → one JSON command → close). It has no
//! event loop and no channel into the app, so it can never feed the main
//! player's cursor-sync engine. Spawned lazily by the transcript's `space`
//! loop; quit whenever focus leaves the transcript (see the design doc
//! docs/superpowers/specs/2026-07-20-chat-panel-space-loop-design.md).

use std::io::Write;
use std::os::unix::net::UnixStream;

use crate::db::models::MediaItem;
use crate::gloss::GlossContext;
use crate::input::segments::SegmentContext;

/// Handle to the chat panel's mpv process. Lives on `AppState.chat_player`
/// (NOT ChatState, which is Default-reset on panel close/work switch — the
/// process must be quit explicitly, never silently dropped).
pub struct ChatPlayer {
    pub socket_path: String,
    pub media_path: String,
}

/// State machine for the transcript `space` loop. On `AppState` for the same
/// reason as `ChatPlayer`. `main_was_playing` is captured at arm time and
/// preserved across nav-stops so a later full exit still restores correctly.
#[derive(Default)]
pub struct ChatLoopState {
    pub armed: bool,
    pub paused: bool,
    pub main_was_playing: bool,
}

/// The displayed entry's source passage, resolved from the ENTRY's own
/// identity.
pub struct LoopSource {
    pub work_abbrev: String,
    pub first_line_id: i64,
    pub last_line_id: i64,
}

/// Resolve the source passage for the loop. `gloss_ctx` (the entry's own
/// record, carrying its work) wins; a raw not-yet-glossed pin falls back to
/// `cursor_lines` + `current_abbrev` — safe because a raw pin is same-work
/// by construction (every main-card work switch wipes the panel). NEVER
/// resolve a glossed entry from current_work: the future cross-work `f`
/// finder will pin other works' entries.
pub fn loop_source_from(
    gloss_ctx: Option<&GlossContext>,
    pinned: Option<&SegmentContext>,
    current_abbrev: Option<&str>,
) -> Option<LoopSource> {
    if let Some(ctx) = gloss_ctx {
        let first = *ctx.source_line_numbers.first()?;
        let last = *ctx.source_line_numbers.last()?;
        return Some(LoopSource {
            work_abbrev: ctx.work_abbrev.clone(),
            first_line_id: first,
            last_line_id: last,
        });
    }
    let p = pinned?;
    let first = p.cursor_lines.first()?.id;
    let last = p.cursor_lines.last()?.id;
    Some(LoopSource {
        work_abbrev: current_abbrev?.to_string(),
        first_line_id: first,
        last_line_id: last,
    })
}

/// Default media for a work: prefer Arkangel, else the highest-priority row
/// (list_media_for_work orders by priority DESC). The play_selected_echo rule.
pub fn pick_default_media(items: &[MediaItem]) -> Option<MediaItem> {
    items
        .iter()
        .find(|m| m.path.contains("/aax-Arkangel/"))
        .cloned()
        .or_else(|| items.first().cloned())
}

/// The single loadfile that arms the whole loop: per-file options seek to
/// `a`, set the a-b loop, and unpause atomically on load (loadfile-replace
/// would clear ab-loop props set beforehand — see MpvCommand::LoadFileSeekAndLoop).
/// Argument order is mpv >= 0.38 (url, flags, index, options); -1 = "no index".
pub fn arm_command_json(media_path: &str, a: f64, b: Option<f64>) -> String {
    let opts = match b {
        Some(b) => format!("start={:.3},ab-loop-a={:.3},ab-loop-b={:.3},pause=no", a, a, b),
        None => format!("start={:.3},pause=no", a),
    };
    format!(
        r#"{{"command":["loadfile",{},"replace",-1,{}]}}"#,
        serde_json::to_string(media_path).unwrap_or_default(),
        serde_json::to_string(&opts).unwrap_or_default(),
    )
}

/// Fire one command at the socket and hang up. A fresh connection per
/// command means mpv never accumulates an unread event backlog for us.
fn send_json(socket_path: &str, json: &str) {
    match UnixStream::connect(socket_path) {
        Ok(mut stream) => {
            let _ = stream.write_all(json.as_bytes());
            let _ = stream.write_all(b"\n");
        }
        Err(e) => {
            crate::logging::log(&format!("CHAT-MPV: send failed ({}): {}", socket_path, e))
        }
    }
}

impl ChatPlayer {
    pub fn toggle_pause(&self) {
        send_json(&self.socket_path, r#"{"command":["cycle","pause"]}"#);
    }

    /// Disarm the a-b loop and pause; the process stays for reuse.
    pub fn stop_loop(&self) {
        send_json(&self.socket_path, r#"{"command":["set_property","ab-loop-a","no"]}"#);
        send_json(&self.socket_path, r#"{"command":["set_property","ab-loop-b","no"]}"#);
        send_json(&self.socket_path, r#"{"command":["set_property","pause",true]}"#);
    }

    pub fn quit(&self) {
        send_json(&self.socket_path, r#"{"command":["quit"]}"#);
    }
}

/// Ensure the chat mpv is running on `socket_path` and arm the loop. Runs on
/// a detached thread: a first launch waits up to ~3s for the IPC socket
/// (mirroring discover_or_launch_blocking), which must never block the GTK
/// thread. State was already updated optimistically by the caller.
pub fn spawn_and_arm(socket_path: String, media_path: String, a: f64, b: Option<f64>) {
    std::thread::spawn(move || {
        if UnixStream::connect(&socket_path).is_err() {
            let _ = std::fs::remove_file(&socket_path); // stale leftover, if any
            crate::mpv::discovery::launch_mpv_at(&socket_path, &media_path);
            for _ in 0..60 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if UnixStream::connect(&socket_path).is_ok() {
                    break;
                }
            }
        }
        send_json(&socket_path, &arm_command_json(&media_path, a, b));
        crate::logging::log(&format!(
            "CHAT-MPV: armed a={:.2} b={:?} media={}",
            a, b, media_path
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::MediaItem;

    fn media(path: &str) -> MediaItem {
        MediaItem { media_id: 1, path: path.to_string(), display_name: None, priority: 0 }
    }

    #[test]
    fn pick_default_media_prefers_arkangel_else_first() {
        let plain = media("/home/x/Music/a/plain.m4b");
        let ark = media("/home/x/Music/a/aax-Arkangel/ham.m4b");
        assert_eq!(
            pick_default_media(&[plain.clone(), ark.clone()]).unwrap().path,
            ark.path
        );
        assert_eq!(pick_default_media(&[plain.clone()]).unwrap().path, plain.path);
        assert!(pick_default_media(&[]).is_none());
    }

    #[test]
    fn arm_command_is_one_loadfile_with_per_file_options() {
        let with_loop = arm_command_json("/m/a.m4b", 100.5, Some(112.25));
        assert_eq!(
            with_loop,
            r#"{"command":["loadfile","/m/a.m4b","replace",-1,"start=100.500,ab-loop-a=100.500,ab-loop-b=112.250,pause=no"]}"#
        );
        // No b-point: play once from a, no ab-loop options at all.
        let once = arm_command_json("/m/a.m4b", 100.5, None);
        assert_eq!(
            once,
            r#"{"command":["loadfile","/m/a.m4b","replace",-1,"start=100.500,pause=no"]}"#
        );
    }

    #[test]
    fn loop_source_prefers_gloss_ctx_own_work() {
        let ctx = crate::gloss::GlossContext {
            work_abbrev: "BH-Barrett".into(),
            work_title: String::new(),
            start_citation: String::new(),
            end_citation: String::new(),
            act: 0,
            scene: 0,
            speaker: String::new(),
            source_text: String::new(),
            source_line_numbers: vec![41, 42, 43],
            hash: String::new(),
            gloss_type: String::new(),
        };
        // gloss_ctx wins even when current_work says otherwise — the entry's
        // own identity, the cross-work `f` finder's contract.
        let src = loop_source_from(Some(&ctx), None, Some("TGV-Amb")).unwrap();
        assert_eq!(src.work_abbrev, "BH-Barrett");
        assert_eq!((src.first_line_id, src.last_line_id), (41, 43));
        // Empty line list → unresolvable, not a bogus 0..0 range.
        let empty = crate::gloss::GlossContext { source_line_numbers: vec![], ..ctx };
        assert!(loop_source_from(Some(&empty), None, Some("TGV-Amb")).is_none());
        // Nothing pinned at all → None.
        assert!(loop_source_from(None, None, Some("TGV-Amb")).is_none());
    }
}
