/// Commands sent from the GTK UI thread to the Tokio runtime.
#[derive(Debug)]
pub enum MpvCommand {
    Seek(f64),
    TogglePause,
    LoadFile(String),
    Connect(String),
    Disconnect,
}

/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
}
