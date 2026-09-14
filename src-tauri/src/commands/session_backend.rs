//! Transport seam for backend-owned terminal sessions.
//!
//! Desktop sessions are a real local PTY process (portable_pty); for remote
//! execution targets that process is the OpenSSH client. Android has no
//! OpenSSH binary, so a later phase will add an in-process SSH channel
//! adapter behind this same trait. Until that adapter exists, remote sessions
//! fail closed with an explicit "unsupported" error instead of spawning a
//! missing `ssh` and surfacing a confusing spawn failure.

use crate::errors::CommandError;
use portable_pty::{Child, ChildKiller, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::Mutex;

/// One live terminal session's byte transport plus lifecycle.
///
/// The session registry (`pty_session.rs`) owns everything above the bytes:
/// ring buffer, attach offsets, liveness, exit code, events. A backend only
/// moves bytes and reports when the session ends.
pub trait SessionBackend: Send + Sync {
    /// Blocking read. `Ok(0)` means EOF — the session has ended.
    fn read(&self, buf: &mut [u8]) -> std::io::Result<usize>;
    /// Write terminal input bytes.
    fn write(&self, data: &[u8]) -> std::io::Result<()>;
    /// Resize the terminal. The error text must preserve the platform's raw
    /// message: the dead-PTY classifier matches on it (`is_resize_on_dead_pty`).
    fn resize(&self, rows: u16, cols: u16) -> Result<(), String>;
    /// Terminate the session. Idempotent.
    fn kill(&self);
    /// Block until the session ends; returns the exit code (0 = clean,
    /// -1 = unknown).
    fn wait(&self) -> i32;
    /// OS process id for process-based backends; 0 for channel transports
    /// (no local process to signal).
    fn pid(&self) -> u32;
}

/// Desktop backend: a child process attached to a local PTY.
pub struct ProcessBackend {
    reader: Mutex<Box<dyn Read + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    pid: u32,
}

impl ProcessBackend {
    pub fn new(
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        killer: Box<dyn ChildKiller + Send + Sync>,
        child: Box<dyn Child + Send + Sync>,
        pid: u32,
    ) -> Self {
        Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
            master: Mutex::new(master),
            killer: Mutex::new(killer),
            child: Mutex::new(child),
            pid,
        }
    }
}

fn poisoned(context: &str, e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(format!("{context}: {e}"))
}

impl SessionBackend for ProcessBackend {
    fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader
            .lock()
            .map_err(|e| poisoned("reader lock poisoned", e))?
            .read(buf)
    }

    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.writer
            .lock()
            .map_err(|e| poisoned("writer lock poisoned", e))?
            .write_all(data)
    }

    fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let master = self
            .master
            .lock()
            .map_err(|e| format!("master lock poisoned: {e}"))?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("{e}"))
    }

    fn kill(&self) {
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }

    fn wait(&self) -> i32 {
        match self.child.lock() {
            Ok(mut child) => match child.wait() {
                Ok(status) => {
                    if status.success() {
                        0
                    } else {
                        // portable_pty exposes exit_code as u32 (0-255).
                        status.exit_code() as i32
                    }
                }
                Err(_) => -1,
            },
            Err(_) => -1,
        }
    }

    fn pid(&self) -> u32 {
        self.pid
    }
}

/// Whether a remote (SSH) PTY session can be opened on `os`.
///
/// Pure so the Android fallback decision is testable on any host. Android
/// reports `"android"` from `std::env::consts::OS`; the in-process SSH
/// adapter is not implemented yet, so remote sessions fail closed there.
pub fn remote_pty_supported_on(os: &str) -> bool {
    os != "android"
}

/// Whether this build can open a remote (SSH) PTY session.
pub fn remote_pty_supported() -> bool {
    remote_pty_supported_on(std::env::consts::OS)
}

/// Explicit fail-closed error for platforms without a remote transport yet.
/// `Expected` keeps an environment gap out of telemetry.
pub fn unsupported_remote_pty_error() -> CommandError {
    CommandError::expected("Remote SSH terminals are not supported on this platform yet.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_is_not_supported() {
        assert!(!remote_pty_supported_on("android"));
    }

    #[test]
    fn desktop_platforms_are_supported() {
        for os in ["windows", "macos", "linux"] {
            assert!(remote_pty_supported_on(os), "{os} must be supported");
        }
    }

    #[test]
    fn current_platform_matches_the_compile_time_decision() {
        assert_eq!(remote_pty_supported(), !cfg!(target_os = "android"));
    }

    #[test]
    fn unsupported_error_is_expected_and_explicit() {
        let err = unsupported_remote_pty_error();
        assert!(matches!(err, CommandError::Expected { .. }), "got {err:?}");
        assert!(
            err.to_string().contains("not supported"),
            "message must say the transport is unsupported: {err}"
        );
    }

    /// Compile-time proof that the desktop backend satisfies the seam. The
    /// Android adapter (F2+) must implement the same trait.
    #[test]
    fn process_backend_implements_the_seam() {
        fn assert_backend<T: SessionBackend>() {}
        assert_backend::<ProcessBackend>();
    }
}
