//! Dedicated chat-panel MPV player: a SECOND mpv process on a `chat-`-marked
//! socket, driven write-only (connect → one JSON command → close). It has no
//! event loop and no channel into the app, so it can never feed the main
//! player's cursor-sync engine. Spawned lazily by the transcript's `space`
//! loop; quit whenever focus leaves the transcript (see the design doc
//! docs/superpowers/specs/2026-07-20-chat-panel-space-loop-design.md).
//!
//! Known leak (by design): a WM-level window close has no `close_request`
//! handler, so it can leave a paused chat mpv behind, invisible to discovery.

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
    /// Generation token bumped on every arm/stop/teardown. `spawn_and_arm`
    /// captures the value at arm and refuses to send the arm command if it
    /// has changed by the time the socket is up — otherwise a teardown inside
    /// the ~3s launch window would arm a process nothing tracks (audible
    /// loop, no handle). Arc<AtomicU64> so the detached thread can read it.
    pub arm_gen: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ChatLoopState {
    /// Arm-time capture: remember (sticky-OR) whether WE are the reason the
    /// main player is paused. Returns `(pause_main, generation)` — the u64 is
    /// the token `spawn_and_arm` must still see to actually arm. Sticky
    /// because a re-arm after a nav-stop sees mpv_playing == false (we paused
    /// it at the first arm) and must not forget the restore.
    pub fn on_arm(&mut self, mpv_playing: bool) -> (bool, u64) {
        use std::sync::atomic::Ordering;
        self.main_was_playing = self.main_was_playing || mpv_playing;
        self.armed = true;
        self.paused = false;
        let gen = self.arm_gen.fetch_add(1, Ordering::SeqCst) + 1;
        (mpv_playing, gen)
    }

    /// Nav-stop: disarm only. Main stays paused; the restore flag survives so
    /// a later full teardown still resumes correctly. Bumps the generation so
    /// any in-flight arm is cancelled.
    pub fn on_stop(&mut self) {
        use std::sync::atomic::Ordering;
        self.armed = false;
        self.paused = false;
        self.arm_gen.fetch_add(1, Ordering::SeqCst);
    }

    /// Full teardown. Returns whether the main player must be resumed —
    /// keyed on main_was_playing ALONE (it is set only at arm and cleared
    /// only here, so it precisely encodes "we paused main and haven't
    /// restored it"), never on armed, which a nav-stop already cleared.
    /// Bumps the generation so any in-flight arm is cancelled.
    pub fn on_teardown(&mut self) -> bool {
        use std::sync::atomic::Ordering;
        let resume = self.main_was_playing;
        self.armed = false;
        self.paused = false;
        self.main_was_playing = false;
        self.arm_gen.fetch_add(1, Ordering::SeqCst);
        resume
    }
}

/// The displayed entry's source passage, resolved from the ENTRY's own
/// identity. The line fields are per-division line numbers (`line_in_div`) —
/// NOT `line_mapping.id`s. They are resolved to global ids at space-time via
/// `line_id_for_location` (the echoes precedent); `line_mapping.id` is a
/// global autoincrement, so passing a `line_in_div` where an id is expected
/// can never match.
pub struct LoopSource {
    pub work_abbrev: String,
    pub div1: i64,
    pub div2: i64,
    pub first_line_in_div: i64,
    pub last_line_in_div: i64,
    /// Exact text of the first/last passage lines — the fallback lookup key
    /// when the (div1, div2, line_in_div) location misses because the entry's
    /// numbering came from a different edition than the one being played
    /// (`work_abbrev` is canonical, the numbers are the loaded edition's).
    pub first_text: String,
    pub last_text: String,
}

/// Resolve the source passage for the loop. `gloss_ctx` (the entry's own
/// record, carrying its work) wins; a raw not-yet-glossed pin falls back to
/// `cursor_lines` + `current_abbrev` — safe because a raw pin is same-work
/// by construction (every main-card work switch wipes the panel). NEVER
/// resolve a glossed entry from current_work: the future cross-work `f`
/// finder will pin other works' entries.
///
/// The returned line fields are `line_in_div` values (gloss_ctx.act/scene are
/// the passage's div1/div2, source_line_numbers are line_in_div;
/// SegmentContext carries div1/div2 and cursor_lines with `line_in_div`). The
/// caller resolves them to `line_mapping.id`s with `line_id_for_location`.
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
            div1: ctx.act,
            div2: ctx.scene,
            first_line_in_div: first,
            last_line_in_div: last,
            first_text: ctx.source_text.lines().next().unwrap_or("").to_string(),
            last_text: ctx.source_text.lines().last().unwrap_or("").to_string(),
        });
    }
    let p = pinned?;
    let first = p.cursor_lines.first()?;
    let last = p.cursor_lines.last()?;
    Some(LoopSource {
        work_abbrev: current_abbrev?.to_string(),
        div1: p.div1,
        div2: p.div2,
        first_line_in_div: first.line_in_div,
        last_line_in_div: last.line_in_div,
        first_text: first.text.clone(),
        last_text: last.text.clone(),
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

/// Pick the loop's media AND the edition abbrev whose `line_mapping` rows the
/// passage must be resolved in, from `media_for_base_work`'s
/// (association abbrev, media) rows. Canonical prose abbrevs (`BH`) own no
/// media — only their editions do — and each edition numbers its divisions
/// independently, so media and lookup abbrev must be chosen TOGETHER:
/// 1. rows keyed by `base` itself (the Shakespeare model, where `Cym` owns
///    both mapping and media) — the original `pick_default_media` rule;
/// 2. the loaded edition of the same base (the voice the user is hearing);
/// 3. any edition: Arkangel path first, else the highest-priority row.
pub fn pick_edition_media(
    rows: &[(String, MediaItem)],
    base: &str,
    current_abbrev: Option<&str>,
) -> Option<(String, MediaItem)> {
    let of = |abbrev: &str| -> Vec<MediaItem> {
        rows.iter().filter(|(a, _)| a == abbrev).map(|(_, m)| m.clone()).collect()
    };
    if let Some(m) = pick_default_media(&of(base)) {
        return Some((base.to_string(), m));
    }
    if let Some(cur) = current_abbrev {
        if cur.strip_prefix(base).is_some_and(|rest| rest.starts_with('-')) {
            if let Some(m) = pick_default_media(&of(cur)) {
                return Some((cur.to_string(), m));
            }
        }
    }
    rows.iter()
        .find(|(_, m)| m.path.contains("/aax-Arkangel/"))
        .or_else(|| rows.first())
        .map(|(a, m)| (a.clone(), m.clone()))
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
pub fn spawn_and_arm(
    socket_path: String,
    media_path: String,
    a: f64,
    b: Option<f64>,
    gen: std::sync::Arc<std::sync::atomic::AtomicU64>,
    expected: u64,
) {
    use std::sync::atomic::Ordering;
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
        // A stop/teardown during the launch window bumped the generation; do
        // NOT arm a process nothing tracks — quit it instead.
        if gen.load(Ordering::SeqCst) != expected {
            send_json(&socket_path, r#"{"command":["quit"]}"#);
            crate::logging::log("CHAT-MPV: arm cancelled (stale generation), quit sent");
            return;
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

    fn line_at(line_in_div: i64) -> crate::db::models::Line {
        crate::db::models::Line {
            id: 9_000_000 + line_in_div, // deliberately NOT line_in_div: ids are millions
            citation: String::new(),
            text: String::new(),
            normalized: String::new(),
            speaker: None,
            is_dialogue: true,
            timestamp: None,
            div1: 2,
            div2: 4,
            line_in_div,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    #[test]
    fn loop_source_prefers_gloss_ctx_own_work() {
        let ctx = crate::gloss::GlossContext {
            work_abbrev: "BH-Barrett".into(),
            work_title: String::new(),
            start_citation: String::new(),
            end_citation: String::new(),
            act: 3,
            scene: 5,
            speaker: String::new(),
            source_text: "first line\nmiddle line\nlast line".into(),
            source_line_numbers: vec![41, 42, 43],
            hash: String::new(),
            gloss_type: String::new(),
            work_type: "play".into(),
        };
        // gloss_ctx wins even when current_work says otherwise — the entry's
        // own identity, the cross-work `f` finder's contract.
        let src = loop_source_from(Some(&ctx), None, Some("TGV-Amb")).unwrap();
        assert_eq!(src.work_abbrev, "BH-Barrett");
        // act/scene are the passage's div1/div2; source_line_numbers are
        // line_in_div values — resolved to ids at space-time.
        assert_eq!((src.div1, src.div2), (3, 5));
        assert_eq!((src.first_line_in_div, src.last_line_in_div), (41, 43));
        // The passage's own text rides along as the cross-edition fallback key.
        assert_eq!(src.first_text, "first line");
        assert_eq!(src.last_text, "last line");
        // Empty line list → unresolvable, not a bogus 0..0 range.
        let empty = crate::gloss::GlossContext { source_line_numbers: vec![], ..ctx };
        assert!(loop_source_from(Some(&empty), None, Some("TGV-Amb")).is_none());
        // Nothing pinned at all → None.
        assert!(loop_source_from(None, None, Some("TGV-Amb")).is_none());
    }

    #[test]
    fn pick_edition_media_exact_base_keeps_shakespeare_model() {
        // Rows keyed by the base itself win, with the Arkangel preference.
        let rows = vec![
            ("Cym".to_string(), media("/m/plain.m4b")),
            ("Cym".to_string(), media("/m/aax-Arkangel/cym.m4b")),
            ("Cym-BBC".to_string(), media("/m/bbc.m4b")),
        ];
        let (abbrev, m) = pick_edition_media(&rows, "Cym", Some("Cym-Amb")).unwrap();
        assert_eq!(abbrev, "Cym");
        assert_eq!(m.path, "/m/aax-Arkangel/cym.m4b");
    }

    #[test]
    fn pick_edition_media_prefers_loaded_edition_for_prose() {
        // No base-keyed rows (the prose model): the loaded edition of the
        // same base wins over a higher-priority sibling edition.
        let rows = vec![
            ("BH-Margolyes".to_string(), media("/m/margolyes.m4b")),
            ("BH-Vance".to_string(), media("/m/vance.m4b")),
        ];
        let (abbrev, m) = pick_edition_media(&rows, "BH", Some("BH-Vance")).unwrap();
        assert_eq!(abbrev, "BH-Vance");
        assert_eq!(m.path, "/m/vance.m4b");
        // A DIFFERENT base loaded (the cross-work finder case): fall through
        // to Arkangel-else-first among the editions.
        let (abbrev, m) = pick_edition_media(&rows, "BH", Some("TGV-Amb")).unwrap();
        assert_eq!(abbrev, "BH-Margolyes");
        assert_eq!(m.path, "/m/margolyes.m4b");
        // A base that merely PREFIXES another ("BH" vs "BHX-Foo") never
        // matches as an edition.
        let odd = vec![("BHX-Foo".to_string(), media("/m/x.m4b"))];
        let (abbrev, _) = pick_edition_media(&odd, "BH", Some("BHX-Foo")).unwrap();
        assert_eq!(abbrev, "BHX-Foo"); // via the any-edition fallback only
        assert!(pick_edition_media(&[], "BH", None).is_none());
    }

    #[test]
    fn teardown_resumes_after_nav_stop() {
        // arm(playing) → nav-stop → full teardown must resume: the nav-stop
        // cleared `armed`, but main_was_playing still says we paused main.
        let mut st = ChatLoopState::default();
        assert!(st.on_arm(true).0);
        st.on_stop();
        assert!(st.on_teardown());
    }

    #[test]
    fn teardown_resumes_after_rearm_sticky_capture() {
        // arm(true) → nav-stop → re-arm(false, main already paused by us) →
        // teardown must still resume (sticky main_was_playing capture).
        let mut st = ChatLoopState::default();
        st.on_arm(true);
        st.on_stop();
        assert!(!st.on_arm(false).0); // we don't re-pause; already paused
        assert!(st.on_teardown());
    }

    #[test]
    fn teardown_does_not_resume_when_main_was_not_playing() {
        // We never paused main → never resume it.
        let mut st = ChatLoopState::default();
        assert!(!st.on_arm(false).0);
        assert!(!st.on_teardown());
    }

    #[test]
    fn teardown_clears_flag_for_next_cycle() {
        let mut st = ChatLoopState::default();
        st.on_arm(true);
        assert!(st.on_teardown());
        // Fresh cycle where main was NOT playing → flag actually cleared.
        assert!(!st.on_arm(false).0);
        assert!(!st.on_teardown());
    }

    #[test]
    fn arm_generation_increases_each_arm() {
        let mut st = ChatLoopState::default();
        let g1 = st.on_arm(true).1;
        let g2 = st.on_arm(false).1;
        assert!(g2 > g1);
    }

    #[test]
    fn stop_makes_prior_generation_stale() {
        use std::sync::atomic::Ordering;
        let mut st = ChatLoopState::default();
        let g = st.on_arm(true).1;
        assert_eq!(st.arm_gen.load(Ordering::SeqCst), g); // fresh: matches
        st.on_stop();
        assert_ne!(st.arm_gen.load(Ordering::SeqCst), g); // now stale
    }

    #[test]
    fn teardown_makes_prior_generation_stale() {
        use std::sync::atomic::Ordering;
        let mut st = ChatLoopState::default();
        let g = st.on_arm(true).1;
        st.on_teardown();
        assert_ne!(st.arm_gen.load(Ordering::SeqCst), g); // teardown invalidates
    }

    #[test]
    fn loop_source_pinned_fallback_uses_line_in_div_and_current_work() {
        let pinned = SegmentContext {
            segments: vec![String::new()],
            cursor_index: 0,
            cursor_lines: vec![line_at(41), line_at(42), line_at(43)],
            div1: 2,
            div2: 4,
        };
        // No gloss_ctx: fall back to the raw pin + current_work. The line
        // fields are line_in_div (41/43), NOT the Line.id millions.
        let src = loop_source_from(None, Some(&pinned), Some("TGV-Amb")).unwrap();
        assert_eq!(src.work_abbrev, "TGV-Amb");
        assert_eq!((src.div1, src.div2), (2, 4));
        assert_eq!((src.first_line_in_div, src.last_line_in_div), (41, 43));
    }
}
