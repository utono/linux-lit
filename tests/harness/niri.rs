//! Headless **niri** test harness for linux-lit.
//!
//! Companion to the cage harness in `mod.rs`. Cage remains the default for the
//! existing suite; this exists to verify behavior that depends on the real
//! window manager the user actually runs (`~/utono/niri-mlj`) — window
//! decorations, tiling geometry, and anything where a kiosk compositor's
//! force-fullscreen would paper over a bug.
//!
//! # Why niri is nested inside cage
//!
//! niri is a **Smithay** compositor, not wlroots. It has **no headless
//! backend**: `WLR_BACKENDS=headless` is ignored entirely, and with no
//! `WAYLAND_DISPLAY`/`DISPLAY` in the environment it selects the **TTY**
//! backend and panics ("error initializing the TTY backend ... Failed to open
//! session"). Its only nestable backend is **winit**, chosen when it sees a
//! parent display. So the stack is:
//!
//! ```text
//!   cage (wlroots, headless backend, pixman)   <- provides the parent display
//!     └── niri (winit backend)                 <- the WM under test
//!           └── linux-lit                      <- the app under test
//! ```
//!
//! Consequences that bit during bring-up, all verified by running it:
//!
//! * **The output size comes from the OUTER cage window, not niri's config.**
//!   A `mode "1920x1236"` in the niri config is a request the winit backend
//!   ignores; the winit output inherits cage's size (default 1280x720, which
//!   reports as a 1272x688 usable logical size). [`NiriHarness::set_output_size`]
//!   therefore resizes the OUTER cage output and lets niri follow.
//! * **The niri IPC socket path is subject to `SUN_LEN`.** A deep runtime dir
//!   (e.g. under a session scratchpad) makes every `niri msg` fail with "path
//!   must be shorter than SUN_LEN". The harness mints its runtime dir directly
//!   under `/tmp` with a short prefix for this reason — do NOT point it at a
//!   nested temp path.
//! * **niri tiles; it does not force fullscreen.** cage guarantees the single
//!   client a fullscreen, focused, configured surface, which is what makes the
//!   app's reveal complete. Under niri the harness must ask for that explicitly
//!   via `niri msg action fullscreen-window`.
//!
//! Capture is `grim` and input is `wtype`, exactly as in the cage harness —
//! both verified working through the nested stack (niri implements
//! wlr-screencopy and the virtual-keyboard protocol).

#![allow(dead_code)] // helpers are used by individual test files, not all at once

use std::ffi::OsStr;
use std::fs;
use std::io::{self};
use std::os::unix::process::CommandExt; // process_group
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Where the harness looks for the deterministic niri config, relative to the
/// crate root. See that file's header for why it is not the user's real config.
const TEST_CONFIG: &str = "tests/harness/niri-test.kdl";

/// A running `cage → niri → app` stack.
///
/// Drop kills the whole process group, reaping cage, niri, and the app together.
pub struct NiriHarness {
    /// Short-path runtime dir (`/tmp/lit-niri-*`). Removed on Drop. This is a
    /// plain PathBuf rather than a `TempDir` because the path must stay short
    /// for niri's IPC socket to fit in `SUN_LEN`, and because Drop needs to
    /// outlive the child kill.
    runtime_dir: PathBuf,
    /// The wayland socket NIRI serves (the app's display), not cage's.
    wayland_display: String,
    /// niri's IPC socket, for `niri msg`.
    niri_socket: PathBuf,
    /// The outer cage display, used only to resize the real output.
    cage_display: String,
    cage: Child,
    /// Per-run app log (`LIT_LOG_PATH`), isolated from the live dev session's
    /// log so assertions can never read the wrong process.
    log_path: PathBuf,
}

impl NiriHarness {
    /// Launch the `cage → niri → bin` stack. Blocks until niri's wayland socket
    /// and IPC socket both appear, then until the app maps.
    ///
    /// `extra_env` is applied to the whole stack (inherited by the app), e.g.
    /// `[("LIT_DEV", "1"), ("LIT_HEADLESS_TEST", "1")]`.
    pub fn start_app<I, S>(bin: &Path, args: I, extra_env: &[(&str, &str)]) -> io::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        // Short path on purpose: niri's IPC socket name is
        // `<runtime>/niri.wayland-N.<pid>.sock`, and the whole thing must fit
        // in sockaddr_un. A deep tempdir silently breaks every `niri msg`.
        let runtime_dir = short_runtime_dir()?;
        let log_path = runtime_dir.join("linux-lit-test.log");

        // Resolve the config to an absolute path: niri is spawned by cage with
        // an unspecified cwd, so a relative -c would not resolve.
        let config = fs::canonicalize(TEST_CONFIG).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("niri test config not found at {TEST_CONFIG}: {e}"),
            )
        })?;

        // Build the inner command: niri runs the app as its startup command, so
        // the app is a client of NIRI (not of cage) and lands on niri's socket.
        // `niri -c <cfg> -- <bin> <args>`.
        let mut cmd = Command::new("cage");
        cmd.arg("--")
            .arg("niri")
            .arg("-c")
            .arg(&config)
            .arg("--")
            .arg(bin)
            .args(args);

        cmd.env("XDG_RUNTIME_DIR", &runtime_dir)
            // Outer cage: wlroots headless + software rendering (no GPU/DRM).
            // niri ignores all of these; they are for cage alone.
            .env("WLR_BACKENDS", "headless")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_RENDERER_ALLOW_SOFTWARE", "1")
            .env("WLR_HEADLESS_OUTPUTS", "1")
            // niri's winit backend renders through EGL/GL; force llvmpipe so it
            // works with no GPU. (It logs a benign dmabuf-feedback fallback.)
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            .env("GALLIUM_DRIVER", "llvmpipe")
            // The app is a GTK client: force Wayland + the cairo renderer. The
            // default Vulkan/ngl renderer loses its surface on this software
            // path and the app aborts.
            .env("GDK_BACKEND", "wayland")
            .env("GSK_RENDERER", "cairo")
            .env("LIT_LOG_PATH", &log_path)
            // Never nest inside, or talk to, the user's real session.
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .env_remove("NIRI_SOCKET")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Own process group so Drop can reap cage + niri + app together.
            .process_group(0);

        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        let cage = cmd.spawn()?;

        // cage takes wayland-0; niri then takes wayland-1. Wait for BOTH, and
        // identify niri's by its IPC socket rather than by guessing the name.
        let deadline = Instant::now() + Duration::from_secs(15);
        let cage_display = wait_for_socket_named(&runtime_dir, "wayland-0", deadline)
            .ok_or_else(|| timed_out("cage wayland socket never appeared"))?;
        let niri_socket = wait_for_niri_socket(&runtime_dir, deadline)
            .ok_or_else(|| timed_out("niri IPC socket never appeared (did niri fall back to the TTY backend?)"))?;
        let wayland_display = niri_display_from_socket(&niri_socket).ok_or_else(|| {
            io::Error::other(format!(
                "could not derive niri's WAYLAND_DISPLAY from {}",
                niri_socket.display()
            ))
        })?;
        wait_for_socket_named(&runtime_dir, &wayland_display, deadline)
            .ok_or_else(|| timed_out("niri's wayland socket never appeared"))?;

        let h = Self {
            runtime_dir,
            wayland_display,
            niri_socket,
            cage_display,
            cage,
            log_path,
        };

        // NOTE: deliberately NOT fullscreening here. The window has not mapped
        // yet at this point, so a `fullscreen-window` action would be a silent
        // no-op — and because that action is a TOGGLE, a later call would then
        // flip the window INTO fullscreen when a test expected it tiled. That
        // exact confusion produced a decoration test that passed with
        // decorations enabled. Tests that need fullscreen call
        // [`Self::ensure_fullscreen`] after the app has revealed; tests that
        // need tiled (decoration checks) call [`Self::unfullscreen_window`].
        Ok(h)
    }

    /// Resize the output the app sees.
    ///
    /// **Resizes the OUTER cage output**, because niri's winit output inherits
    /// its size from the parent surface — a `mode` in niri's own config does
    /// nothing here. niri follows cage's new size automatically.
    ///
    /// Two-column play tests MUST widen the output: the 1280x720 default is too
    /// narrow for a two-column card (the layout never settles, so the reveal
    /// that emits `TEST_VIEWPORT_RECT` never fires) and too short to reproduce
    /// tall-viewport pagination bugs. Production geometry is 1920x1236.
    ///
    /// Returns whether wlr-randr reported success; the achieved size is best
    /// confirmed with [`Self::output_size`].
    pub fn set_output_size(&self, w: u32, h: u32) -> io::Result<bool> {
        // Target CAGE's socket, not niri's: it is cage's headless output we are
        // resizing. niri does not implement wlr-output-management, so pointing
        // wlr-randr at niri would find nothing to set.
        let name = self.cage_output_name().unwrap_or_else(|| "HEADLESS-1".into());
        let ok = self
            .cage_cmd("wlr-randr")
            .arg("--output")
            .arg(&name)
            .arg("--custom-mode")
            .arg(format!("{w}x{h}"))
            .status()?
            .success();
        // Give niri a moment to observe the parent resize and re-lay-out.
        // Deliberately does NOT touch fullscreen: the action is a toggle, and
        // flipping it here silently changed the window state out from under
        // decoration tests. Callers choose their own state explicitly.
        sleep(Duration::from_millis(700));
        Ok(ok)
    }

    /// The logical size of niri's output, straight from niri's own IPC — the
    /// authority on what the app actually got. Use this to confirm a resize
    /// landed rather than trusting `set_output_size`'s exit status.
    pub fn output_size(&self) -> io::Result<(u32, u32)> {
        let out = self.niri_msg(&["--json", "outputs"])?;
        // {"winit":{...,"logical":{"x":0,"y":0,"width":1272,"height":688,...}}}
        let width = json_number_after(&out, "\"width\":")
            .ok_or_else(|| io::Error::other("no width in `niri msg outputs` JSON"))?;
        let height = json_number_after(&out, "\"height\":")
            .ok_or_else(|| io::Error::other("no height in `niri msg outputs` JSON"))?;
        Ok((width, height))
    }

    /// Send niri's fullscreen TOGGLE. Prefer [`Self::ensure_fullscreen`] —
    /// this flips whatever state the window is currently in.
    pub fn fullscreen_window(&self) -> io::Result<()> {
        self.niri_msg(&["action", "fullscreen-window"]).map(|_| ())
    }

    /// Make the window fullscreen, the tiling equivalent of cage's kiosk
    /// behavior. Idempotent, unlike the raw toggle: only acts if the window is
    /// not already fullscreen. Call AFTER the app has revealed — the action is
    /// a silent no-op while the window is unmapped.
    pub fn ensure_fullscreen(&self) -> io::Result<()> {
        if !self.window_is_fullscreen()? {
            self.niri_msg(&["action", "fullscreen-window"])?;
            sleep(Duration::from_millis(700));
        }
        Ok(())
    }

    /// Ensure the window is TILED (not fullscreen), by pixel-verifying the
    /// result and toggling at most once.
    ///
    /// Decoration tests MUST call this: a fullscreen window has no decorations
    /// by definition, so a titlebar assertion made while fullscreen passes
    /// vacuously — it stays green even with `.decorated(true)`.
    ///
    /// niri 26.04 offers only a TOGGLE (`fullscreen-window`; there is no
    /// `unset-fullscreen-window`), and the harness cannot query the current
    /// state — two dead ends, both confirmed by measurement:
    ///
    /// * `niri msg --json windows` reports **no fullscreen flag** (fields are
    ///   `id`, `title`, `app_id`, `pid`, `workspace_id`, `is_focused`,
    ///   `is_floating`, `is_urgent`, `layout`, `focus_timestamp`), so a JSON
    ///   check silently answers "not fullscreen" every time.
    /// * **Geometry is identical in both states.** With `gaps 0` and borders
    ///   off a tiled window already fills the output exactly — measured
    ///   1272x688 in a 1272x688 output either way — so comparing window size
    ///   to output size cannot distinguish them either.
    ///
    /// What DOES differ is the rendered top edge, so that is the oracle here.
    pub fn is_tiled(&self) -> io::Result<bool> {
        Ok(!self.window_is_fullscreen()?)
    }

    /// Make the window tiled, toggling only if it currently looks fullscreen.
    pub fn unfullscreen_window(&self) -> io::Result<()> {
        if self.window_is_fullscreen()? {
            self.niri_msg(&["action", "fullscreen-window"])?;
            sleep(Duration::from_millis(700));
        }
        Ok(())
    }

    /// `(tile_size, window_size)` for the focused window, as niri reports them.
    ///
    /// This is the decoration probe. niri's IPC has no explicit "is decorated"
    /// field, but it does report the tile niri allotted and the size the window
    /// actually occupies. With server-side decorations the WM reserves part of
    /// the tile for a titlebar, so `window_size.height < tile_size.height`;
    /// undecorated, the window fills its tile. Only meaningful while TILED —
    /// call [`Self::unfullscreen_window`] first.
    pub fn focused_tile_and_window_size(&self) -> io::Result<((u32, u32), (u32, u32))> {
        let out = self.niri_msg(&["--json", "focused-window"])?;
        let tile = json_pair_after(&out, "\"tile_size\":")
            .ok_or_else(|| io::Error::other("no tile_size in `niri msg focused-window`"))?;
        let win = json_pair_after(&out, "\"window_size\":")
            .ok_or_else(|| io::Error::other("no window_size in `niri msg focused-window`"))?;
        Ok((tile, win))
    }

    /// Raw `niri msg` call against THIS instance's IPC socket, returning stdout.
    /// Errors if niri reports failure (e.g. the socket path exceeded SUN_LEN).
    pub fn niri_msg(&self, args: &[&str]) -> io::Result<String> {
        let out = Command::new("niri")
            .env("NIRI_SOCKET", &self.niri_socket)
            .env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env("WAYLAND_DISPLAY", &self.wayland_display)
            .env_remove("DISPLAY")
            .arg("msg")
            .args(args)
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "`niri msg {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Whether the window appears fullscreen, detected from PIXELS.
    ///
    /// Deliberately NOT read from `niri msg --json windows`: niri 26.04 reports
    /// no fullscreen flag at all, so a JSON-based check returns `false`
    /// unconditionally and silently defeats anything gated on it. Window/output
    /// geometry is no better — with `gaps 0` both states measure the same.
    ///
    /// The oracle is the top edge of the output. Fullscreen, niri hands the app
    /// the whole output and the reading card's cream reaches row 0. Tiled, the
    /// top rows are something else: a white CSD titlebar if decorations are on,
    /// otherwise the app's own dark teal root margin. So "row 0 is a bright,
    /// WARM colour (cream)" means fullscreen.
    pub fn window_is_fullscreen(&self) -> io::Result<bool> {
        let tmp = self.runtime_dir.join("fs-probe.png");
        self.screenshot(&tmp)?;
        let out = Command::new("python3")
            .arg("-c")
            .arg(
                "import sys\n\
                 from PIL import Image\n\
                 im=Image.open(sys.argv[1]).convert('RGB')\n\
                 r,g,b=im.getpixel((im.size[0]//2,0))\n\
                 # cream card: bright and warm (red channel clearly above blue).\n\
                 print(1 if (min(r,g,b)>=200 and (r-b)>=10) else 0)",
            )
            .arg(&tmp)
            .output()?;
        let _ = fs::remove_file(&tmp);
        Ok(String::from_utf8_lossy(&out.stdout).trim() == "1")
    }

    /// The app window's title as niri sees it. This is the WM's own view of the
    /// client, so it is the right probe for decoration/title behavior.
    pub fn window_title(&self) -> io::Result<Option<String>> {
        let out = self.niri_msg(&["--json", "windows"])?;
        Ok(json_string_after(&out, "\"title\":"))
    }

    /// Build a command targeting NIRI's wayland socket (grim/wtype/python).
    /// This is the display the app is on, so it is what tests should capture.
    fn client_cmd(&self, program: impl AsRef<OsStr>) -> Command {
        let mut c = Command::new(program);
        c.env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env("WAYLAND_DISPLAY", &self.wayland_display)
            .env("NIRI_SOCKET", &self.niri_socket)
            .env_remove("DISPLAY");
        c
    }

    /// Build a command targeting the OUTER cage socket. Only output management
    /// needs this; capture and input must go to niri.
    fn cage_cmd(&self, program: impl AsRef<OsStr>) -> Command {
        let mut c = Command::new(program);
        c.env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env("WAYLAND_DISPLAY", &self.cage_display)
            .env_remove("DISPLAY")
            .env_remove("NIRI_SOCKET");
        c
    }

    /// First output name reported by wlr-randr on the CAGE display.
    fn cage_output_name(&self) -> Option<String> {
        let out = self.cage_cmd("wlr-randr").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find(|l| !l.starts_with(char::is_whitespace) && !l.trim().is_empty())
            .and_then(|l| l.split_whitespace().next())
            .map(|s| s.to_string())
    }

    /// Capture niri's output framebuffer to a PNG (wlr-screencopy via grim).
    /// Verified working through the nested stack.
    pub fn screenshot(&self, out: &Path) -> io::Result<()> {
        let ok = self.client_cmd("grim").arg(out).status()?.success();
        if ok {
            Ok(())
        } else {
            Err(io::Error::other("grim failed against the niri display"))
        }
    }

    /// Capture a named UI state to `target/ui/<name>.png`, plus the best-effort
    /// annotated overlay. Mirrors the cage harness so tests can swap harnesses.
    pub fn capture(&self, name: &str) -> io::Result<PathBuf> {
        fs::create_dir_all("target/ui")?;
        let png = PathBuf::from(format!("target/ui/{name}.png"));
        self.screenshot(&png)?;
        // `--app` is REQUIRED by the script; see the cage harness's `capture`.
        let _ = self
            .client_cmd("python3")
            .arg("scripts/annotate_ui.py")
            .arg("--shot")
            .arg(&png)
            .arg("--app")
            .arg(super::ATSPI_APP_NAME)
            .status();
        Ok(png)
    }

    /// Capture `name` and assert the text pane doesn't clip its first/last line
    /// within the pixel `region` (x, y, w, h). Fails closed — a missing dep is a
    /// failure, not a pass. Run under `scripts/e2e-env.sh` so numpy/pillow exist.
    pub fn assert_no_line_clipping(
        &self,
        name: &str,
        region: (i32, i32, i32, i32),
    ) -> io::Result<()> {
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
        Err(io::Error::other(format!(
            "line-clipping check failed for '{name}' (see {}_clip.png):\n{report}",
            png.display().to_string().trim_end_matches(".png")
        )))
    }

    /// Type literal text into the focused surface, `per_key_ms` between keys.
    pub fn type_text(&self, text: &str, per_key_ms: u32) -> io::Result<()> {
        self.client_cmd("wtype")
            .arg("-d")
            .arg(per_key_ms.to_string())
            .arg(text)
            .status()?;
        Ok(())
    }

    /// Press+release one xkb keysym, e.g. "Return", "Escape", "j".
    ///
    /// The test config pins a plain `us` layout, so keysyms here are the
    /// standard ones — NOT the user's Real Programmers Dvorak mapping.
    pub fn key(&self, keysym: &str, settle_ms: u32) -> io::Result<()> {
        self.client_cmd("wtype")
            .arg("-s")
            .arg(settle_ms.to_string())
            .arg("-k")
            .arg(keysym)
            .status()?;
        Ok(())
    }

    /// Send a modifier chord, e.g. `chord(&["shift"], "g")`. Must be a SINGLE
    /// wtype invocation: wtype releases held keys when it exits.
    pub fn chord(&self, mods: &[&str], keysym: &str) -> io::Result<()> {
        let mut c = self.client_cmd("wtype");
        for m in mods {
            c.arg("-M").arg(m);
        }
        c.arg("-k").arg(keysym);
        for m in mods.iter().rev() {
            c.arg("-m").arg(m);
        }
        c.status()?;
        Ok(())
    }

    /// Give the app time to map + paint + finish its two-phase load.
    pub fn settle(&self, dur: Duration) {
        sleep(dur);
    }

    /// This run's app log path (`LIT_LOG_PATH`).
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Block until the app logs its reading-viewport rect, then return
    /// `(x, y, w, h)` for the clipping detector's `--region`. Doubles as a
    /// readiness gate: the rect is logged when the window is revealed.
    pub fn wait_for_viewport_rect(&self, timeout: Duration) -> io::Result<(i32, i32, i32, i32)> {
        self.wait_for_rect("TEST_VIEWPORT_RECT ", timeout)
    }

    /// As above, for the synopsis/gloss OVERLAY viewport.
    pub fn wait_for_overlay_viewport_rect(
        &self,
        timeout: Duration,
    ) -> io::Result<(i32, i32, i32, i32)> {
        self.wait_for_rect("TEST_OVERLAY_VIEWPORT_RECT ", timeout)
    }

    fn wait_for_rect(&self, marker: &str, timeout: Duration) -> io::Result<(i32, i32, i32, i32)> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(&self.log_path) {
                if let Some(rect) = text.lines().rev().find_map(|l| l.split(marker).nth(1)) {
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
        Err(timed_out(&format!(
            "{marker}never appeared in the app log (is LIT_DEV + LIT_HEADLESS_TEST set, did the window reveal, and did fullscreen take?)"
        )))
    }
}

impl Drop for NiriHarness {
    fn drop(&mut self) {
        // Kill the whole group: cage, niri, and the app. `process_group(0)` at
        // spawn made the child a group leader, so -pid reaches all three.
        let pid = self.cage.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.cage.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => sleep(Duration::from_millis(50)),
                _ => {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                    let _ = self.cage.wait();
                    break;
                }
            }
        }
        // Remove the runtime dir by hand (it is not a TempDir — see the field
        // docs on why the path must stay short).
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

/// Mint a private runtime dir directly under `/tmp` with a SHORT name.
///
/// niri's IPC socket lives at `<runtime>/niri.<display>.<pid>.sock`, and the
/// full path must fit in `sockaddr_un.sun_path`. Nesting this under a deep
/// session/scratchpad path makes every `niri msg` fail with "path must be
/// shorter than SUN_LEN", which shows up as a mysteriously unfullscreenable
/// window rather than an obvious error.
fn short_runtime_dir() -> io::Result<PathBuf> {
    for _ in 0..64 {
        // Cheap unique suffix without pulling in a rand dependency.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let dir = PathBuf::from(format!("/tmp/lit-niri-{:x}-{:x}", std::process::id(), nanos));
        match fs::create_dir(&dir) {
            Ok(()) => {
                // XDG_RUNTIME_DIR must be private or wayland refuses it.
                let perms = <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700);
                fs::set_permissions(&dir, perms)?;
                return Ok(dir);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("could not create a unique /tmp runtime dir"))
}

/// Wait for a specific wayland socket name to appear in `dir`.
fn wait_for_socket_named(dir: &Path, name: &str, deadline: Instant) -> Option<String> {
    while Instant::now() < deadline {
        if dir.join(name).exists() {
            return Some(name.to_string());
        }
        sleep(Duration::from_millis(100));
    }
    None
}

/// Wait for niri's IPC socket (`niri.<display>.<pid>.sock`) to appear.
fn wait_for_niri_socket(dir: &Path, deadline: Instant) -> Option<PathBuf> {
    while Instant::now() < deadline {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("niri.") && name.ends_with(".sock") {
                    return Some(e.path());
                }
            }
        }
        sleep(Duration::from_millis(100));
    }
    None
}

/// Derive niri's wayland display name from its IPC socket filename:
/// `niri.wayland-1.12345.sock` -> `wayland-1`.
fn niri_display_from_socket(sock: &Path) -> Option<String> {
    let name = sock.file_name()?.to_string_lossy();
    name.strip_prefix("niri.")?.split('.').next().map(String::from)
}

/// Pull the first integer following `key` out of a JSON blob. Deliberately
/// dependency-free: the harness needs two numbers, not a JSON crate.
fn json_number_after(json: &str, key: &str) -> Option<u32> {
    let rest = json.split(key).nth(1)?;
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Pull a two-number array following `key` out of a JSON blob, e.g.
/// `"tile_size":[1888.0,1168.0]` -> `(1888, 1168)`. Values may be floats;
/// the fractional part is truncated.
fn json_pair_after(json: &str, key: &str) -> Option<(u32, u32)> {
    let rest = json.split(key).nth(1)?.trim_start();
    let rest = rest.strip_prefix('[')?;
    let end = rest.find(']')?;
    let mut parts = rest[..end].split(',');
    let a: f64 = parts.next()?.trim().parse().ok()?;
    let b: f64 = parts.next()?.trim().parse().ok()?;
    Some((a as u32, b as u32))
}

/// Pull the first quoted string following `key` out of a JSON blob.
fn json_string_after(json: &str, key: &str) -> Option<String> {
    let rest = json.split(key).nth(1)?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn timed_out(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, msg.to_string())
}
