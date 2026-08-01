use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;

/// MPV launch volume (percent) for the `--volume=` arg. Set once at startup
/// from `config.mpv_volume` (see `set_mpv_volume`); defaults to 100 if never
/// set. A process global so `launch_mpv` need not thread config through its
/// three call sites, which live inside spawn_blocking closures.
static MPV_VOLUME: AtomicU32 = AtomicU32::new(100);

/// Record the configured MPV launch volume. Call once after config load.
pub fn set_mpv_volume(percent: u32) {
    MPV_VOLUME.store(percent, Ordering::Relaxed);
}

/// The configured MPV launch volume (percent). Read by the IPC client to
/// re-assert the volume after connect/loadfile (watch_later restore in the
/// user's mpv.conf would otherwise override the `--volume=` launch arg).
pub fn mpv_volume() -> u32 {
    MPV_VOLUME.load(Ordering::Relaxed)
}

/// MPV window background color (the letterbox/border matte around video and the
/// idle backdrop) for the `--background-color=` arg. Set from the active
/// theme's `root_color` so an MPV window opened by linux-lit matches the
/// reader's outer wallpaper color instead of MPV's default. A process global
/// for the same reason as `MPV_VOLUME`: `launch_mpv` runs inside spawn_blocking
/// closures that can't borrow AppState. Empty until the theme is applied, in
/// which case launch_mpv falls back to MPV's own default.
static MPV_BACKGROUND: RwLock<String> = RwLock::new(String::new());

/// Record the MPV window background color. Call at startup once the theme is
/// resolved, and again whenever the theme or root variant changes.
pub fn set_mpv_background(color: &str) {
    if let Ok(mut g) = MPV_BACKGROUND.write() {
        *g = color.to_string();
    }
}

pub fn derive_socket_path(media_path: &str) -> String {
    derive_socket_path_marked(media_path, "")
}

/// Socket path with an extra `marker` segment after the instance infix
/// (e.g. "chat-" for the chat panel's dedicated player). A marked socket is
/// invisible to the main player's discovery, which only ever derives
/// unmarked paths — so probe/connect/stale-clean can't cross the streams.
pub fn derive_socket_path_marked(media_path: &str, marker: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let author = extract_author(media_path, &home);
    let basename = Path::new(media_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    // Per-instance namespace: "" for slot 1 (legacy paths, reattach
    // compatibility), "i{n}-" for slot n >= 2 — so discovery can only ever
    // find/connect/stale-clean THIS instance's players.
    let infix = crate::instance::socket_infix();

    let is_ytdlp = media_path.contains("/yt-dlp-mlj/");
    let socket_path = if is_ytdlp {
        format!("/tmp/mpvsocket-{}{}ytdlp-{}-{}", infix, marker, author, basename)
    } else {
        format!("/tmp/mpvsocket-{}{}{}-{}", infix, marker, author, basename)
    };

    if socket_path.len() > 95 {
        let prefix = &socket_path[..87];
        let mut hasher = Sha256::new();
        hasher.update(socket_path.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        format!("{}-{}", prefix, &hash[..7])
    } else {
        socket_path
    }
}

fn extract_author(media_path: &str, home: &str) -> String {
    let music_prefix = format!("{}/Music/", home);
    if let Some(rest) = media_path.strip_prefix(&music_prefix) {
        if let Some(author) = rest.split('/').next() {
            return author.to_string();
        }
    }
    let rips_prefix = format!("{}/rips/", home);
    if let Some(rest) = media_path.strip_prefix(&rips_prefix) {
        if let Some(author) = rest.split('/').next() {
            return author.to_string();
        }
    }
    let ytdlp_prefix = format!("{}/yt-dlp-mlj/", home);
    if let Some(rest) = media_path.strip_prefix(&ytdlp_prefix) {
        if let Some(author) = rest.split('/').next() {
            return author.to_string();
        }
    }
    "music".to_string()
}

/// Find a live socket that matches one of the work's media paths.
/// Probes each socket to verify MPV is actually running behind it.
/// Removes stale socket files that fail to connect.
/// Returns (socket_path, matched_media_path).
pub fn find_socket_for_work(media_paths: &[String]) -> Option<(PathBuf, String)> {
    // Headless test runs must never touch real sockets: probing would connect
    // to (and stale-cleanup would DELETE) the live session's MPV socket when
    // both run the same work — a fuzz run would then seek the user's player.
    // Exception: LIT_SYNC_TEST (the playback-sync timing test) re-enables
    // discovery — that harness rewrites its private DB copy's media path so
    // the derived socket can only be its own pre-launched mpv, never the
    // live player's.
    if (std::env::var_os("LIT_HEADLESS_TEST").is_some()
        || std::env::var_os("LIT_NO_MPV").is_some())
        && std::env::var_os("LIT_SYNC_TEST").is_none()
    {
        crate::logging::log("MPV discovery: skipped (LIT_HEADLESS_TEST/LIT_NO_MPV)");
        return None;
    }
    for media_path in media_paths {
        let socket_path = derive_socket_path(media_path);
        let path = PathBuf::from(&socket_path);
        if path.exists() {
            if probe_socket(&path) {
                crate::logging::log(&format!(
                    "MPV discovery: live socket {} for media {}",
                    socket_path, media_path
                ));
                return Some((path, media_path.clone()));
            }
            // Stale socket — remove it
            crate::logging::log(&format!(
                "MPV discovery: removing stale socket {}",
                socket_path
            ));
            let _ = std::fs::remove_file(&path);
        }
    }
    crate::logging::log("MPV discovery: no matching socket found");
    None
}

/// Probe a socket to check if MPV is alive behind it.
/// Tries a synchronous connect — if it succeeds, MPV is running.
fn probe_socket(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// Launch MPV for a media file. Sets wayland-app-id to "mpv-lit" so the
/// compositor can single out this MPV: dwl places it on tag 10 (dwl config.h),
/// niri gives it `open-focused false` (window-rule in niri config.kdl) so it
/// never steals focus from the reader. Both consume the same app-id, so it
/// must stay stable across compositor changes.
///
/// Under the headless UI test harness (`LIT_HEADLESS_TEST` set) MPV is NOT
/// launched at all — the UI tests don't exercise audio sync, and a spawned MPV
/// would otherwise map a window that covers the reader in the test compositor
/// AND leak as a detached process across test runs. The socket path is returned
/// as usual; with no MPV listening, connection attempts simply fail (the app
/// already handles "no MPV" gracefully — see the discovery/connect path).
pub fn launch_mpv(media_path: &str) -> String {
    let socket_path = derive_socket_path(media_path);
    launch_mpv_at(&socket_path, media_path);
    socket_path
}

/// Launch mpv listening on an explicit socket path (the chat player passes a
/// `chat-`-marked one). Same args and headless guards as `launch_mpv`.
pub fn launch_mpv_at(socket_path: &str, media_path: &str) {
    // LIT_NO_MPV: diagnostic toggle to launch with no MPV at all (A/B the startup
    // flicker against the MPV window-map). Same skip as the headless test path.
    if std::env::var_os("LIT_HEADLESS_TEST").is_some()
        || std::env::var_os("LIT_NO_MPV").is_some()
    {
        crate::logging::log(&format!(
            "MPV: skipped (LIT_HEADLESS_TEST/LIT_NO_MPV) for {}",
            media_path
        ));
        return;
    }
    // Paint MPV's window backdrop with the reader's root color so the
    // letterbox/border matte around video (and the idle backdrop) matches the
    // reader's wallpaper instead of MPV's default checkerboard/black. `color`
    // mode makes both the idle background (`--background`) and the border matte
    // (`--border-background`) use `--background-color`. Skipped (empty string)
    // until the theme is applied, leaving MPV's own defaults.
    let bg = MPV_BACKGROUND
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();
    let bg_args: Vec<String> = if bg.is_empty() {
        Vec::new()
    } else {
        vec![
            "--background=color".to_string(),
            "--border-background=color".to_string(),
            format!("--background-color={}", bg),
        ]
    };
    match std::process::Command::new("mpv")
        .arg(format!("--input-ipc-server={}", socket_path))
        .arg("--pause")
        .arg("--no-terminal")
        .arg(format!("--volume={}", MPV_VOLUME.load(Ordering::Relaxed)))
        .args(&bg_args)
        // Keep MPV's window: some works are videos, and the audiobook window
        // carries cover art. The compositor keeps it clear of the reader by
        // app-id — dwl routes `mpv-lit` to its own tag (config.h), niri marks
        // it `open-focused false` (config.kdl).
        .arg("--wayland-app-id=mpv-lit")
        .arg(media_path)
        // Detach MPV's stdio from ours. Otherwise the spawned MPV inherits our
        // stdout/stderr; under a `… 2>&1 | tee` launcher (crll) it holds the
        // pipe's write end open, so `tee` never sees EOF and the terminal hangs
        // after the app exits. MPV runs `--no-terminal` and we never read its
        // output, so null is lossless.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {
            crate::logging::log(&format!("MPV: launched for {} (app-id=mpv-lit)", media_path));
            niri_expel_mpv_to_own_column();
        }
        Err(e) => crate::logging::log(&format!("MPV: launch failed: {}", e)),
    }
}

/// Move our just-spawned MPV into its own niri column, to the right of the
/// reader.
///
/// niri opens a new window INTO the focused column (stacked below the reader as
/// a second row) rather than beside it. No window rule can change that: niri
/// 26.04 has no `open-in-new-column`-style property — the full set of
/// open-time placement rules is open-on-output / open-on-workspace /
/// open-maximized / open-fullscreen / open-floating / open-focused, none of
/// which controls column membership. So this has to be an action AFTER the
/// window maps, and linux-lit is the only party that knows which MPV is ours.
///
/// `expel-window-from-column` is the action that splits it out. It operates on
/// a window id, so we poll `niri msg --json windows` for our `mpv-lit` window
/// instead of assuming it has mapped yet (spawn returns long before the Wayland
/// surface exists).
///
/// Expelling is then followed by `maximize-column`, because the expel restores
/// the column but not the height — see the comment at that call for the
/// measurements and for why `set-window-height` cannot do it.
///
/// No-ops off niri: `NIRI_SOCKET` is set only inside a niri session, so under
/// dwl-mlj this returns immediately and the tag-10 rule in config.h keeps
/// handling placement as before.
fn niri_expel_mpv_to_own_column() {
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return;
    }
    // Detached: MPV's surface takes a moment to map, and the caller is on a
    // spawn_blocking thread we should not stall further.
    std::thread::spawn(|| {
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let Ok(out) = std::process::Command::new("niri")
                .args(["msg", "--json", "windows"])
                .output()
            else {
                return;
            };
            let Ok(text) = String::from_utf8(out.stdout) else {
                return;
            };
            // Hand-scan rather than pull in a JSON dependency for one field:
            // find the mpv-lit object and read the "id" that precedes its
            // app_id within the same object.
            let Some(id) = find_mpv_lit_window_id(&text) else {
                continue;
            };
            // `expel-window-from-column` takes NO --id: it acts on the focused
            // column only. So focus MPV, expel it, then hand focus straight
            // back to the reader — otherwise this would undo the whole point
            // of the `open-focused false` window rule.
            let reader_id = find_reader_window_id(&text);
            let _ = std::process::Command::new("niri")
                .args(["msg", "action", "focus-window", "--id", &id.to_string()])
                .status();
            let expelled = std::process::Command::new("niri")
                .args(["msg", "action", "expel-window-from-column"])
                .status();
            // Expelling restores the COLUMN but not the HEIGHT. When piri
            // swallows MPV into the launching terminal's column (which it
            // does, because `crll` runs the reader from kitty and MPV is a
            // process-tree descendant), the window is sized to a share of that
            // column and the `default-window-height { proportion 1.0; }` rule
            // in config.kdl -- applied once at map time -- is already spent.
            // Measured in a nested cage on a 1272x688 output: swallowed MPV
            // maps at 636x362 and stays 636x362 after the expel, while an MPV
            // that was never swallowed gets 1280x723.
            //
            // set-window-height cannot recover it: `100%`, `688`, `+100` are
            // all ACCEPTED and all no-ops, because MPV requests a small
            // surface and niri honors that request for a window alone in its
            // column. maximize-column is the action that overrides it --
            // verified to take the swallowed-then-expelled window from
            // 636x362 to 1272x719, and to stick across focus changes.
            //
            // This is also what the user wants independently: MPV as a
            // full-width column. It does not cover the reader, which is
            // fullscreen on its own column.
            //
            // CRITICAL: maximize-column takes NO --id (unlike focus-window and
            // set-window-height), so it acts on whatever column is focused --
            // and `expel-window-from-column` does NOT leave the expelled window
            // focused. Measured: after expelling MPV, focus lands back on the
            // parent kitty, so maximizing there hits the TERMINAL's column and
            // exits 0 while MPV keeps its swallowed size. That is exactly the
            // bug this looked fixed for and was not: the log said
            // `maximize -> Ok(ExitStatus(0))` while MPV stayed 960x544.
            // So re-focus MPV explicitly here rather than assuming the expel
            // left it focused.
            let _ = std::process::Command::new("niri")
                .args(["msg", "action", "focus-window", "--id", &id.to_string()])
                .status();
            let maximized = std::process::Command::new("niri")
                .args(["msg", "action", "maximize-column"])
                .status();
            if let Some(reader) = reader_id {
                let _ = std::process::Command::new("niri")
                    .args([
                        "msg",
                        "action",
                        "focus-window",
                        "--id",
                        &reader.to_string(),
                    ])
                    .status();
            }
            crate::logging::log(&format!(
                "MPV: niri expel id={} -> {:?}, maximize -> {:?}, focus restored to reader={:?}",
                id, expelled, maximized, reader_id
            ));
            return;
        }
        crate::logging::log("MPV: niri expel skipped — mpv-lit window never appeared");
    });
}

/// Extract the window id of the `mpv-lit` window from `niri msg --json windows`
/// output. The array is a flat list of objects; each object carries both "id"
/// and "app_id", so we split on object boundaries and take the id from the one
/// whose app_id is mpv-lit.
fn find_mpv_lit_window_id(json: &str) -> Option<u64> {
    find_window_id_by_app_id(json, "\"mpv-lit\"")
}

/// Window id of this reader (release or dev app-id), so focus can be handed
/// back after the expel. Returns None if the reader isn't in the list, in which
/// case focus is simply left where the expel put it.
fn find_reader_window_id(json: &str) -> Option<u64> {
    find_window_id_by_app_id(json, "\"com.utono.linux-lit.dev\"")
        .or_else(|| find_window_id_by_app_id(json, "\"com.utono.linux-lit\""))
}

fn find_window_id_by_app_id(json: &str, quoted_app_id: &str) -> Option<u64> {
    for chunk in json.split('{') {
        if !chunk.contains(quoted_app_id) {
            continue;
        }
        let idx = chunk.find("\"id\":")?;
        let rest = &chunk[idx + 5..];
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(id) = digits.parse::<u64>() {
            return Some(id);
        }
    }
    None
}

/// Blocking discover-or-launch (audit #65): reuse an existing MPV socket for
/// this media path if one is live, otherwise launch MPV and wait up to ~3s
/// (60 × 50ms) for its IPC socket to appear. Returns the socket path either
/// way — with no MPV listening, connection attempts simply fail gracefully.
/// Run inside `spawn_blocking`; it sleeps on the calling thread.
pub fn discover_or_launch_blocking(media_path: &str) -> String {
    if let Some((sock, _)) = find_socket_for_work(&[media_path.to_string()]) {
        return sock.to_string_lossy().to_string();
    }
    let launched = launch_mpv(media_path);
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if std::path::Path::new(&launched).exists() {
            return launched;
        }
    }
    launched
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::FileTypeExt;

    fn scan_sockets() -> Vec<PathBuf> {
        let mut sockets = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/tmp") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("mpvsocket-") {
                    let path = entry.path();
                    if let Ok(meta) = std::fs::symlink_metadata(&path) {
                        if meta.file_type().is_socket() {
                            sockets.push(path);
                        }
                    }
                }
            }
        }
        sockets.sort();
        sockets
    }

    #[test]
    fn test_derive_socket_path_music() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{}/Music/shakespeare-william/Hamlet.m4b", home);
        let socket = derive_socket_path(&path);
        assert!(socket.starts_with("/tmp/mpvsocket-shakespeare-william-"));
        assert!(socket.contains("Hamlet.m4b"));
        assert!(!socket.contains("ytdlp"));
    }

    #[test]
    fn test_derive_socket_path_ytdlp() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{}/yt-dlp-mlj/some-author/video.mp4", home);
        let socket = derive_socket_path(&path);
        assert!(socket.starts_with("/tmp/mpvsocket-ytdlp-some-author-"));
    }

    #[test]
    fn test_derive_socket_path_truncation() {
        let home = std::env::var("HOME").unwrap();
        let long_name = "a".repeat(100);
        let path = format!("{}/Music/author/{}.m4b", home, long_name);
        let socket = derive_socket_path(&path);
        assert!(socket.len() <= 95);
    }

    #[test]
    fn test_socket_infix_splices_after_prefix() {
        // derive_socket_path always uses the process slot (1 in unit tests →
        // no infix, asserted by the existing tests). The slot-n shape is
        // pinned here via the pure helper so a format drift can't slip in.
        let infix = crate::instance::socket_infix_for(2);
        let path = format!("/tmp/mpvsocket-{}shakespeare-william-Hamlet.m4b", infix);
        assert_eq!(path, "/tmp/mpvsocket-i2-shakespeare-william-Hamlet.m4b");
    }

    #[test]
    fn test_scan_sockets_runs() {
        let _sockets = scan_sockets();
    }

    #[test]
    fn test_derive_socket_path_marked_chat() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{}/Music/shakespeare-william/Hamlet.m4b", home);
        let socket = derive_socket_path_marked(&path, "chat-");
        // Slot 1 in unit tests → no instance infix; the marker sits where the
        // infix would extend: /tmp/mpvsocket-{infix}{marker}{author}-{basename}.
        assert!(socket.starts_with("/tmp/mpvsocket-chat-shakespeare-william-"));
        assert!(socket.contains("Hamlet.m4b"));
        // The unmarked path is a DIFFERENT socket — main-player discovery can
        // never probe/stale-clean the chat player's socket.
        assert_ne!(socket, derive_socket_path(&path));
        // Truncation cap still applies with a marker.
        let long = format!("{}/Music/author/{}.m4b", home, "a".repeat(100));
        assert!(derive_socket_path_marked(&long, "chat-").len() <= 95);
    }
}

#[cfg(test)]
mod niri_expel_tests {
    use super::find_mpv_lit_window_id;

    // Shape copied from real `niri msg --json windows` output on niri 26.04.
    const SAMPLE: &str = r#"[{"app_id":"kitty","id":24,"title":"x"},{"app_id":"com.utono.linux-lit.dev","id":25,"title":"r"},{"app_id":"mpv-lit","id":26,"title":"m"}]"#;

    #[test]
    fn finds_the_mpv_lit_window() {
        assert_eq!(find_mpv_lit_window_id(SAMPLE), Some(26));
    }

    #[test]
    fn none_when_mpv_absent() {
        let s = r#"[{"app_id":"kitty","id":24},{"app_id":"com.utono.linux-lit","id":25}]"#;
        assert_eq!(find_mpv_lit_window_id(s), None);
    }

    #[test]
    fn id_before_app_id_in_same_object() {
        let s = r#"[{"id":26,"app_id":"mpv-lit"}]"#;
        assert_eq!(find_mpv_lit_window_id(s), Some(26));
    }

    #[test]
    fn finds_the_reader_window() {
        assert_eq!(super::find_reader_window_id(SAMPLE), Some(25));
    }

    #[test]
    fn reader_lookup_matches_release_app_id_too() {
        let s = r#"[{"app_id":"com.utono.linux-lit","id":9}]"#;
        assert_eq!(super::find_reader_window_id(s), Some(9));
    }

    /// The dev app-id contains the release app-id as a prefix, so a substring
    /// match on the release id would also hit a dev window. Dev is checked
    /// first; assert the release lookup can't claim a dev-only window.
    #[test]
    fn reader_lookup_prefers_dev_when_both_present() {
        let s = r#"[{"app_id":"com.utono.linux-lit","id":9},{"app_id":"com.utono.linux-lit.dev","id":25}]"#;
        assert_eq!(super::find_reader_window_id(s), Some(25));
    }
}
