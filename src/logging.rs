use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

static LOG_PATH: OnceLock<String> = OnceLock::new();

pub fn init(path: &str) {
    LOG_PATH.set(path.to_string()).ok();
}

/// Write a line to the log file.
pub fn log(msg: &str) {
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
