use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
    if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
        crate::logging::log(&format!(
            "MPV: skipped (LIT_HEADLESS_TEST) for {}",
            media_path
        ));
        return socket_path;
    }
    match std::process::Command::new("mpv")
        .arg(format!("--input-ipc-server={}", socket_path))
        .arg("--pause")
        .arg("--no-terminal")
        // Audio-sync backend only — never maps a window. Without this, MPV opens a
        // cover-art/video window (even for an audio-only .m4b); when that window
        // maps ~1s after launch the compositor re-tiles the output and the reader
        // visibly flickers once. `--no-video` (+ `--force-window=no`) keeps MPV
        // headless so there's no map event and no flicker. linux-lit drives MPV
        // purely over the IPC socket, so the window is never needed.
        .arg("--no-video")
        .arg("--force-window=no")
        .arg("--volume=75")
        .arg("--wayland-app-id=mpv-lit")
        .arg(media_path)
        .spawn()
    {
        Ok(_) => crate::logging::log(&format!("MPV: launched for {} (app-id=mpv-lit)", media_path)),
        Err(e) => crate::logging::log(&format!("MPV: launch failed: {}", e)),
    }
    socket_path
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
