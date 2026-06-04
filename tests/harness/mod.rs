//! Headless Wayland test harness for linux-lit.
//!
//! Runs the app inside `cage` — a single-client kiosk wlroots compositor — on
//! the headless backend with the pixman software renderer, so it works with no
//! GPU and no monitor (container, SSH, CI). This is the automated counterpart
//! to the manual `cage` flow documented in CLAUDE.md.
//!
//! **Why cage, not bare dwl/sway:** linux-lit's window only lays out and paints
//! once it gets a configured, focused, fullscreen surface. cage gives the single
//! client exactly that; bare dwl/sway on the headless backend leave the window
//! unsized so the app's reveal never completes (it hits its 5s "load may be
//! stuck" fallback and renders blank). cage also keeps the one client
//! fullscreen, so the app's MPV helper window can't cover the reader — though
//! the tests additionally set `LIT_HEADLESS_TEST=1` so MPV launches `--no-video`
//! and maps no surface at all. The trade-off vs sway is no `swaymsg` geometry
//! IPC; the clipping detector + screenshots don't need it.
//!
//! Capture is via `grim` (wlr-screencopy, which cage supports); input via
//! `wtype` (virtual-keyboard protocol). The line-clipping detector
//! (`scripts/check_line_clipping.py`) is driven by an explicit `--region`,
//! because linux-lit's `sourceview5::View` exposes no AT-SPI Text interface for
//! the detector's auto-detect to find.
//!
//! Only deps are `tempfile` and `libc` (both in `[dev-dependencies]`).

use std::ffi::OsStr;
use std::fs;
use std::io::{self};
use std::os::unix::process::CommandExt; // process_group
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// A running headless `cage` session with the app under test inside it.
pub struct Harness {
    _runtime: TempDir, // kept alive so the dir isn't removed mid-test
    runtime_dir: PathBuf,
    wayland_display: String,
    cage: Child,
}

impl Harness {
    /// Launch `bin` (with `args` and `extra_env`) inside a fresh headless cage.
    /// Cage is the compositor AND the app launcher, so this both starts the
    /// compositor and the client. Blocks until cage's wayland socket appears.
    ///
    /// `extra_env` is applied to the whole cage process (inherited by the app):
    /// e.g. `[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")]`.
    pub fn start_app<I, S>(bin: &Path, args: I, extra_env: &[(&str, &str)]) -> io::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let runtime = tempfile::tempdir()?;
        let runtime_dir = runtime.path().to_path_buf();

        let mut cmd = Command::new("cage");
        // `cage -- <app> <args>`: cage opens a headless output, maps the single
        // client fullscreen, and exits when it does.
        cmd.arg("--").arg(bin).args(args);

        cmd.env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("WLR_BACKENDS", "headless")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_RENDERER_ALLOW_SOFTWARE", "1")
            .env("WLR_HEADLESS_OUTPUTS", "1")
            // The app is a GTK client of cage: force Wayland + software GTK
            // rendering. GSK_RENDERER=cairo is MANDATORY — the default Vulkan/ngl
            // renderer loses its surface on the headless backend and the app
            // aborts with a stack overflow.
            .env("GDK_BACKEND", "wayland")
            .env("GSK_RENDERER", "cairo")
            // don't nest inside or talk to the real session:
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // own process group so Drop can reap cage + the app together:
            .process_group(0);

        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        let cage = cmd.spawn()?;

        let wayland_display = wait_for_socket(&runtime_dir, Duration::from_secs(5)).ok_or_else(|| {
            io::Error::new(io::ErrorKind::TimedOut, "cage wayland socket never appeared")
        })?;

        Ok(Self {
            _runtime: runtime,
            runtime_dir,
            wayland_display,
            cage,
        })
    }

    /// Build a command targeting this cage's wayland socket (grim/wtype/python).
    fn client_cmd(&self, program: impl AsRef<OsStr>) -> Command {
        let mut c = Command::new(program);
        c.env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env("WAYLAND_DISPLAY", &self.wayland_display)
            .env_remove("DISPLAY");
        c
    }

    /// Capture cage's output framebuffer to a PNG (wlr-screencopy via grim).
    pub fn screenshot(&self, out: &Path) -> io::Result<()> {
        let ok = self.client_cmd("grim").arg(out).status()?.success();
        if ok {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "grim failed"))
        }
    }

    /// Capture a named UI state to `target/ui/<name>.png` and (best-effort)
    /// produce the annotated overlay + suspects list. Returns the screenshot
    /// path; the Stop hook gates the turn on a review of everything captured.
    pub fn capture(&self, name: &str) -> io::Result<PathBuf> {
        fs::create_dir_all("target/ui")?;
        let png = PathBuf::from(format!("target/ui/{name}.png"));
        self.screenshot(&png)?;
        let _ = self
            .client_cmd("python3")
            .arg("scripts/annotate_ui.py")
            .arg("--shot")
            .arg(&png)
            .status(); // best-effort; review hook still has the raw PNG
        Ok(png)
    }

    /// Capture `name` and assert the text pane doesn't clip its first/last line,
    /// checking the pixel `region` (x, y, w, h) of the reading pane. linux-lit's
    /// TextView exposes no AT-SPI Text interface, so the region is passed
    /// explicitly rather than auto-detected. Fails closed: a missing dep or an
    /// unevaluable check is a failure, not a pass — run under `e2e-env.sh` so
    /// numpy/pillow are present.
    pub fn assert_no_line_clipping(&self, name: &str, region: (i32, i32, i32, i32)) -> io::Result<()> {
        let png = self.capture(name)?;
        let (x, y, w, h) = region;
        let out = self
            .client_cmd("python3")
            .arg("scripts/check_line_clipping.py")
            .arg("--shot")
            .arg(&png)
            .arg("--region")
            .arg(format!("{x},{y},{w},{h}"))
            .output()?;
        if out.status.success() {
            return Ok(());
        }
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "line-clipping check failed for '{name}' (see {}_clip.png):\n{report}",
                png.display().to_string().trim_end_matches(".png")
            ),
        ))
    }

    /// Type literal text into the focused surface, with `per_key_ms` delay
    /// between keystrokes (wtype `-d`). Bursting through a virtual keyboard can
    /// garble input; use a few ms. Pass 0 for no throttling.
    pub fn type_text(&self, text: &str, per_key_ms: u32) -> io::Result<()> {
        self.client_cmd("wtype")
            .arg("-d")
            .arg(per_key_ms.to_string())
            .arg(text)
            .status()?;
        Ok(())
    }

    /// Press+release one xkb keysym, e.g. "Return", "Escape", "j", "x".
    /// `settle_ms` (wtype `-s`) sleeps before the key so a freshly-mapped
    /// surface has time to take focus.
    pub fn key(&self, keysym: &str, settle_ms: u32) -> io::Result<()> {
        self.client_cmd("wtype")
            .arg("-s")
            .arg(settle_ms.to_string())
            .arg("-k")
            .arg(keysym)
            .status()?;
        Ok(())
    }

    /// Send a modifier chord, e.g. `chord(&["shift"], "g")` for Shift+G. Must be
    /// a SINGLE wtype invocation: wtype releases all held keys when it exits, so
    /// a modifier can't span separate `key` calls. Modifiers: shift, ctrl, alt,
    /// logo, etc.
    pub fn chord(&self, mods: &[&str], keysym: &str) -> io::Result<()> {
        let mut c = self.client_cmd("wtype");
        for m in mods {
            c.arg("-M").arg(m); // press modifier
        }
        c.arg("-k").arg(keysym); // press+release the key while held
        for m in mods.iter().rev() {
            c.arg("-m").arg(m); // release modifier
        }
        c.status()?;
        Ok(())
    }

    /// Give the app time to map + paint + finish its two-phase work load. cage
    /// has no queryable IPC, so readiness is time-based (the app reveals ~2s
    /// after launch on this software-rendered path).
    pub fn settle(&self, dur: Duration) {
        sleep(dur);
    }

    /// Block until the app emits its reading-viewport rectangle to the dev log
    /// (it does this once, on first reveal, when `LIT_HEADLESS_TEST` is set),
    /// then return `(x, y, w, h)` in screenshot-pixel coordinates for use as the
    /// clipping detector's `--region`. Doubles as a readiness gate: the rect is
    /// logged at the moment the window is revealed and painted.
    ///
    /// Reads the dev log at `$HOME/utono/linux-lit/linux-lit-dev.log` (where the
    /// app writes under `LIT_DEV`). The harness truncates it before launch so a
    /// stale rect from a prior run isn't picked up.
    pub fn wait_for_viewport_rect(&self, timeout: Duration) -> io::Result<(i32, i32, i32, i32)> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(Self::dev_log_path()) {
                // last matching line wins (most recent reveal)
                if let Some(rect) = text
                    .lines()
                    .rev()
                    .find_map(|l| l.split("TEST_VIEWPORT_RECT ").nth(1))
                {
                    let nums: Vec<i32> = rect
                        .split_whitespace()
                        .take(4)
                        .filter_map(|n| n.parse().ok())
                        .collect();
                    if let [x, y, w, h] = nums[..] {
                        return Ok((x, y, w, h));
                    }
                }
            }
            sleep(Duration::from_millis(100));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "TEST_VIEWPORT_RECT never appeared in the dev log (is LIT_DEV + LIT_HEADLESS_TEST set, and did the window reveal?)",
        ))
    }

    /// Path to the dev log the app writes under `LIT_DEV`.
    fn dev_log_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join("utono/linux-lit/linux-lit-dev.log")
    }

    /// Truncate the dev log so `wait_for_viewport_rect` can't read a stale rect
    /// from a previous run. Call before `start_app`. Best-effort.
    pub fn reset_dev_log() {
        let _ = fs::write(Self::dev_log_path(), b"");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // cage leads its own process group (process_group(0)); SIGTERM the whole
        // group so cage AND the app (and any helper it spawned) die together.
        let pgid = self.cage.id() as i32;
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
        let _ = self.cage.wait();
    }
}

/// Poll a fresh runtime dir for the first `wayland-*` socket (skipping locks).
fn wait_for_socket(runtime_dir: &Path, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(entries) = fs::read_dir(runtime_dir) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("wayland-") && !name.ends_with(".lock") {
                    return Some(name.into_owned());
                }
            }
        }
        sleep(Duration::from_millis(50));
    }
    None
}
