//! Per-process instance slot assignment.
//!
//! Each linux-lit process auto-assigns itself a slot number (1, 2, 3, ...)
//! at startup by flock-ing a per-slot lock file under $XDG_RUNTIME_DIR.
//! The locked File is held in a OnceLock for the process lifetime, so the
//! OS releases the slot on any exit including crashes — no stale-slot
//! cleanup exists or is needed.
//!
//! Slot 1 must stay byte-identical to the pre-multi-instance app: no
//! suffix in socket paths, log filenames, or the window title.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static SLOT: OnceLock<u32> = OnceLock::new();
static LOCK_FILE: OnceLock<File> = OnceLock::new();

/// Highest slot probed before degrading to slot 1.
const MAX_SLOTS: u32 = 64;

fn slot_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("linux-lit");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    PathBuf::from(format!("/tmp/linux-lit-{}", user))
}

fn try_lock_slot(dir: &Path, n: u32) -> Option<File> {
    let path = dir.join(format!("slot-{}.lock", n));
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        // WouldBlock (held by another instance) or IO error — either way
        // this slot is unavailable.
        Err(_) => None,
    }
}

/// Acquire the lowest free slot. Called once, first thing in `main()`,
/// BEFORE `logging::init` (the log filename depends on the slot), so it
/// cannot log — `main()` logs the outcome right after logging comes up.
pub fn acquire() -> u32 {
    let dir = slot_dir();
    if fs::create_dir_all(&dir).is_err() {
        // Pathological: degrade to slot 1 (today's behavior) rather than
        // refusing to start.
        SLOT.set(1).ok();
        return 1;
    }
    // LIT_INSTANCE pins a slot for deterministic test/debug runs. If the
    // requested slot is already locked, fall through to the normal scan.
    if let Ok(v) = std::env::var("LIT_INSTANCE") {
        if let Ok(n) = v.parse::<u32>() {
            if n >= 1 {
                if let Some(f) = try_lock_slot(&dir, n) {
                    LOCK_FILE.set(f).ok();
                    SLOT.set(n).ok();
                    return n;
                }
            }
        }
    }
    for n in 1..=MAX_SLOTS {
        if let Some(f) = try_lock_slot(&dir, n) {
            LOCK_FILE.set(f).ok();
            SLOT.set(n).ok();
            return n;
        }
    }
    SLOT.set(1).ok();
    1
}

/// The slot this process holds. 1 when `acquire` was never called (unit
/// tests) so every suffix helper defaults to legacy names.
pub fn slot() -> u32 {
    SLOT.get().copied().unwrap_or(1)
}

/// Socket-name infix for `derive_socket_path`: "" for slot 1 (legacy
/// paths unchanged), "i{n}-" for slot n >= 2.
pub fn socket_infix_for(slot: u32) -> String {
    if slot <= 1 {
        String::new()
    } else {
        format!("i{}-", slot)
    }
}

pub fn socket_infix() -> String {
    socket_infix_for(slot())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_one_has_no_infix() {
        assert_eq!(socket_infix_for(1), "");
    }

    #[test]
    fn higher_slots_get_i_n_infix() {
        assert_eq!(socket_infix_for(2), "i2-");
        assert_eq!(socket_infix_for(10), "i10-");
    }

    #[test]
    fn slot_defaults_to_one_without_acquire() {
        // Unit tests never call acquire(); every consumer must see slot 1
        // (legacy behavior) in that state.
        assert_eq!(slot(), 1);
        assert_eq!(socket_infix(), "");
    }
}
