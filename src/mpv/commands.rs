use std::collections::HashMap;

/// Commands sent from the GTK UI thread to the Tokio runtime.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvCommand {
    Seek(f64),
    SeekRelative(f64),
    VolumeAdjust(f64),
    TogglePause,
    Pause,
    /// Resume without seeking (pause=no). Counterpart of `Pause` for the
    /// chat-loop exit path, which must restore playback exactly where the
    /// arm-time `Pause` left it.
    Resume,
    ResumeAndSeek(f64),
    SetSpeed(f64),
    Connect(String),
    SetTimestamps {
        timestamps: Vec<(i64, f64, f64)>,
        line_id_to_index: HashMap<i64, usize>,
    },
    SetAbLoop { a: f64, b: f64 },
    ClearAbLoop,
    /// Recolor a running MPV window's backdrop (letterbox/border matte + idle
    /// background) to the given color. Sent when the reader's theme or root
    /// variant changes so an already-open MPV window follows live. New windows
    /// pick up the color at launch instead (see `set_mpv_background`).
    SetBackground(String),
    LoadFile(String),
    /// Load file and seek+resume after MPV reports it's ready.
    LoadFileAndSeek(String, f64),
    /// Load file and seek but stay paused after MPV reports it's ready.
    LoadFileSeekPaused(String, f64),
    /// Load file, then after MPV reports it ready: seek+resume AND set an
    /// AB-loop (a, b). Used when reloading media that needs a loop, since
    /// loadfile-replace clears ab-loop props set before the file loads.
    LoadFileSeekAndLoop(String, f64, f64),
    Quit,
}

/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
    TimePos(f64),
    ThemeChanged,
}
