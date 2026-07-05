use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// MPV launch volume (percent) for the `--volume=` arg. Set once at startup
/// from `config.mpv_volume` (see `set_mpv_volume`); defaults to 100 if never
/// set. A process global so `launch_mpv` need not thread config through its
/// three call sites, which live inside spawn_blocking closures.
static MPV_VOLUME: AtomicU32 = AtomicU32::new(100);

/// Record the configured MPV launch volume. Call once after config load.
pub fn set_mpv_volume(percent: u32) {
    MPV_VOLUME.store(percent, Ordering::Relaxed);
}

pub fn derive_socket_path(media_path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let author = extract_author(media_path, &home);
    let basename = Path::new(media_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let is_ytdlp = media_path.contains("/yt-dlp-mlj/");
    let socket_path = if is_ytdlp {
        format!("/tmp/mpvsocket-ytdlp-{}-{}", author, basename)
    } else {
        format!("/tmp/mpvsocket-{}-{}", author, basename)
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
    if std::env::var_os("LIT_HEADLESS_TEST").is_some()
        || std::env::var_os("LIT_NO_MPV").is_some()
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

/// Launch MPV for a media file. Sets wayland-app-id to "mpv-lit" so dwl
/// places the window on tag 10 per the rule in dwl config.h.
///
/// Under the headless UI test harness (`LIT_HEADLESS_TEST` set) MPV is NOT
/// launched at all — the UI tests don't exercise audio sync, and a spawned MPV
/// would otherwise map a window that covers the reader in the test compositor
/// AND leak as a detached process across test runs. The socket path is returned
/// as usual; with no MPV listening, connection attempts simply fail (the app
/// already handles "no MPV" gracefully — see the discovery/connect path).
pub fn launch_mpv(media_path: &str) -> String {
    let socket_path = derive_socket_path(media_path);
    // LIT_NO_MPV: diagnostic toggle to launch with no MPV at all (A/B the startup
    // flicker against the MPV window-map). Same skip as the headless test path.
    if std::env::var_os("LIT_HEADLESS_TEST").is_some()
        || std::env::var_os("LIT_NO_MPV").is_some()
    {
        crate::logging::log(&format!(
            "MPV: skipped (LIT_HEADLESS_TEST/LIT_NO_MPV) for {}",
            media_path
        ));
        return socket_path;
    }
    match std::process::Command::new("mpv")
        .arg(format!("--input-ipc-server={}", socket_path))
        .arg("--pause")
        .arg("--no-terminal")
        .arg(format!("--volume={}", MPV_VOLUME.load(Ordering::Relaxed)))
        // Keep MPV's window: some works are videos, and the audiobook window
        // carries cover art. dwl routes `mpv-lit` to its own tag (config.h), so
        // it doesn't cover the reader.
        .arg("--wayland-app-id=mpv-lit")
        .arg(media_path)
        .spawn()
    {
        Ok(_) => crate::logging::log(&format!("MPV: launched for {} (app-id=mpv-lit)", media_path)),
        Err(e) => crate::logging::log(&format!("MPV: launch failed: {}", e)),
    }
    socket_path
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
    fn test_scan_sockets_runs() {
        let _sockets = scan_sockets();
    }
}
