use std::cell::RefCell;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// In-process audio player for gloss TTS clips. Holds the rodio output stream
/// alive for the app's lifetime (dropping it stops all audio). A no-op stub
/// under LIT_HEADLESS_TEST, where there is no audio device.
pub struct TtsPlayer {
    inner: Option<Inner>,
    /// Sink gain (1.0 = 100%). Seeded at startup from `config.mpv_volume` so
    /// TTS clips play at the same level as the MPV player; applied to every
    /// sink `play_file` creates (and live to a playing sink on change).
    volume: std::cell::Cell<f32>,
}

struct Inner {
    // The stream must be kept alive; rodio drops audio if it is dropped.
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
    sink: RefCell<Option<rodio::Sink>>,
}

impl TtsPlayer {
    pub fn new() -> Self {
        if std::env::var("LIT_HEADLESS_TEST").is_ok() {
            return TtsPlayer { inner: None, volume: std::cell::Cell::new(1.0) };
        }
        match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => TtsPlayer {
                inner: Some(Inner {
                    _stream: stream,
                    handle,
                    sink: RefCell::new(None),
                }),
                volume: std::cell::Cell::new(1.0),
            },
            Err(e) => {
                crate::log_fmt!("TTS: no audio output device: {}", e);
                TtsPlayer { inner: None, volume: std::cell::Cell::new(1.0) }
            }
        }
    }

    /// Set the player volume as a percent (100 = unity), matching how
    /// `config.mpv_volume` expresses MPV's launch volume. Applies to the
    /// currently playing sink (if any) and to every future clip.
    pub fn set_volume_percent(&self, percent: u32) {
        self.apply_volume(percent as f32 / 100.0);
    }

    /// Nudge the player volume by a percent delta (the Ctrl+Up/Down ±5 step),
    /// clamped to 0..=150 — the same range `config.mpv_volume` accepts — so the
    /// TTS level tracks MPV's relative `add volume` nudges. Applies live to a
    /// playing clip.
    pub fn adjust_volume_percent(&self, delta: f64) {
        self.apply_volume((self.volume.get() + delta as f32 / 100.0).clamp(0.0, 1.5));
    }

    /// Current volume as a percent (100 = unity), for user-facing feedback.
    #[must_use]
    pub fn volume_percent(&self) -> u32 {
        (self.volume.get() * 100.0).round() as u32
    }

    fn apply_volume(&self, v: f32) {
        self.volume.set(v);
        if let Some(inner) = &self.inner {
            if let Some(sink) = inner.sink.borrow().as_ref() {
                sink.set_volume(v);
            }
        }
    }

    /// Stop any current clip and play the MP3 at `path`.
    pub fn play_file(&self, path: &Path) {
        let inner = match &self.inner {
            Some(i) => i,
            None => return,
        };
        self.stop();
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                crate::log_fmt!("TTS: open {} failed: {}", path.display(), e);
                return;
            }
        };
        let decoder = match rodio::Decoder::new(BufReader::new(file)) {
            Ok(d) => d,
            Err(e) => {
                crate::log_fmt!("TTS: decode failed: {}", e);
                return;
            }
        };
        match rodio::Sink::try_new(&inner.handle) {
            Ok(sink) => {
                sink.set_volume(self.volume.get());
                sink.append(decoder);
                *inner.sink.borrow_mut() = Some(sink);
            }
            Err(e) => crate::log_fmt!("TTS: sink failed: {}", e),
        }
    }

    /// Pause the current clip in place (resumable with `resume`). No-op when
    /// nothing is loaded.
    pub fn pause(&self) {
        if let Some(inner) = &self.inner {
            if let Some(sink) = inner.sink.borrow().as_ref() {
                sink.pause();
            }
        }
    }

    /// Resume a clip paused by `pause`. No-op when nothing is loaded.
    pub fn resume(&self) {
        if let Some(inner) = &self.inner {
            if let Some(sink) = inner.sink.borrow().as_ref() {
                sink.play();
            }
        }
    }

    /// True when a clip is loaded and paused mid-play.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        match &self.inner {
            Some(inner) => inner
                .sink
                .borrow()
                .as_ref()
                .map(|s| s.is_paused() && !s.empty())
                .unwrap_or(false),
            None => false,
        }
    }

    /// Stop and drop the current clip. Takes `&self` (not `&mut self`) so it can
    /// be called from `play_file` and from shared `Rc<RefCell<AppState>>` borrows.
    pub fn stop(&self) {
        if let Some(inner) = &self.inner {
            if let Some(sink) = inner.sink.borrow_mut().take() {
                sink.stop();
            }
        }
    }

    /// True while a clip is still playing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        match &self.inner {
            Some(inner) => inner
                .sink
                .borrow()
                .as_ref()
                .map(|s| !s.empty())
                .unwrap_or(false),
            None => false,
        }
    }
}

impl Default for TtsPlayer {
    fn default() -> Self {
        Self::new()
    }
}
