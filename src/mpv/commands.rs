use std::collections::HashMap;

/// Commands sent from the GTK UI thread to the Tokio runtime.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvCommand {
    Seek(f64),
    TogglePause,
    Pause,
    ResumeAndSeek(f64),
    SetSpeed(f64),
    LoadFile(String),
    Connect(String),
    Disconnect,
    SetTimestamps {
        timestamps: Vec<(i64, f64, f64)>,
        line_id_to_index: HashMap<i64, usize>,
    },
}

/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
}
