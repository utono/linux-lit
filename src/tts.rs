use std::cell::RefCell;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// In-process audio player for gloss TTS clips. Holds the rodio output stream
/// alive for the app's lifetime (dropping it stops all audio). A no-op stub
/// under LIT_HEADLESS_TEST, where there is no audio device.
pub struct TtsPlayer {
    inner: Option<Inner>,
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
            return TtsPlayer { inner: None };
        }
        match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => TtsPlayer {
                inner: Some(Inner {
                    _stream: stream,
                    handle,
                    sink: RefCell::new(None),
                }),
            },
            Err(e) => {
                crate::log_fmt!("TTS: no audio output device: {}", e);
                TtsPlayer { inner: None }
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
                sink.append(decoder);
                *inner.sink.borrow_mut() = Some(sink);
            }
            Err(e) => crate::log_fmt!("TTS: sink failed: {}", e),
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
