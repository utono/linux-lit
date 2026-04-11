use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static LOG_PATH: OnceLock<String> = OnceLock::new();
static DEBUG_MODE: AtomicBool = AtomicBool::new(true);

pub fn init(path: &str) {
    LOG_PATH.set(path.to_string()).ok();
}

/// Enable or disable debug logging at runtime.
pub fn set_debug_mode(enabled: bool) {
    DEBUG_MODE.store(enabled, Ordering::Relaxed);
}

/// Returns whether debug mode is currently on.
pub fn debug_mode() -> bool {
    DEBUG_MODE.load(Ordering::Relaxed)
}

/// Write a line to the log file (only when debug mode is on).
pub fn log(msg: &str) {
    if !DEBUG_MODE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(path) = LOG_PATH.get() {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{}", msg);
        }
    }
}

/// Always write to the log file regardless of debug mode.
/// Use for critical messages like startup, errors, and mode toggles.
pub fn log_always(msg: &str) {
    if let Some(path) = LOG_PATH.get() {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{}", msg);
        }
    }
}

/// Log with format args, like `log_fmt!("x={} y={}", x, y)`.
#[macro_export]
macro_rules! log_fmt {
    ($($arg:tt)*) => {
        $crate::logging::log(&format!($($arg)*))
    };
}
