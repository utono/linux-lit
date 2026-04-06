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
    ResumeAndSeek(f64),
    SetSpeed(f64),
    Connect(String),
    SetTimestamps {
        timestamps: Vec<(i64, f64, f64)>,
        line_id_to_index: HashMap<i64, usize>,
    },
    SetAbLoop { a: f64, b: f64 },
    ClearAbLoop,
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
