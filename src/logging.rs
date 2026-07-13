use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The log file, opened once at `init` and held open for the process lifetime,
/// so each `log`/`log_always` is a single `write`+`flush` rather than an
/// `open`+`write`+`close` per line. `Mutex` because logging is called from the
/// GTK main thread AND async/tokio worker threads (mpv client/discovery); the
/// mutex serializes their writes so lines never interleave. Every write flushes
/// immediately — the log is a live diagnostic and a crash must not swallow the
/// last lines, so buffering across lines is deliberately avoided.
static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();
static APP_START: OnceLock<Instant> = OnceLock::new();
static DEBUG_MODE: AtomicBool = AtomicBool::new(true);

pub fn init(path: &str) {
    // main.rs truncates the file (write "") BEFORE calling init, so open for
    // append here to preserve that fresh-per-launch behavior. If the open
    // fails, LOG_FILE stays unset and logging becomes a silent no-op (same as
    // the old open-failure path).
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(path) {
        LOG_FILE.set(Mutex::new(file)).ok();
    }
    APP_START.set(Instant::now()).ok();
}

/// Milliseconds elapsed since `init` was called. Used to prefix every log
/// line with a relative timestamp so startup races (window allocate vs.
/// scrolled_window allocate vs. resize tick vs. display_work) become
/// visible in the log timeline.
fn elapsed_ms() -> u128 {
    APP_START.get().map_or(0, |t| t.elapsed().as_millis())
}

/// Enable or disable debug logging at runtime.
pub fn set_debug_mode(enabled: bool) {
    DEBUG_MODE.store(enabled, Ordering::Relaxed);
}

/// Returns whether debug mode is currently on.
pub fn debug_mode() -> bool {
    DEBUG_MODE.load(Ordering::Relaxed)
}

/// Write one timestamped line to the held log file and flush it. Shared by
/// `log`/`log_always`. No-op when the file never opened (init failure).
fn write_line(msg: &str) {
    if let Some(lock) = LOG_FILE.get() {
        if let Ok(mut file) = lock.lock() {
            let _ = writeln!(file, "[{:>5}ms] {}", elapsed_ms(), msg);
            let _ = file.flush();
        }
    }
}

/// Write a line to the log file (only when debug mode is on).
pub fn log(msg: &str) {
    if !DEBUG_MODE.load(Ordering::Relaxed) {
        return;
    }
    write_line(msg);
}

/// Always write to the log file regardless of debug mode.
/// Use for critical messages like startup, errors, and mode toggles.
pub fn log_always(msg: &str) {
    write_line(msg);
}

/// Log with format args, like `log_fmt!("x={} y={}", x, y)`.
#[macro_export]
macro_rules! log_fmt {
    ($($arg:tt)*) => {
        $crate::logging::log(&format!($($arg)*))
    };
}
