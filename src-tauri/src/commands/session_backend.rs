//! Transport seam for backend-owned terminal sessions.
//!
//! Desktop sessions are a real local PTY process (portable_pty); for remote
//! execution targets that process is the OpenSSH client. Android has no
//! OpenSSH binary, so remote sessions use the in-process SSH channel adapter
//! (`commands::ssh::transport_russh::RusshBackend`) behind this same trait.
//! The session registry above this seam is transport-agnostic.

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof that the desktop backend satisfies the seam. The
    /// Android adapter (`transport_russh::RusshBackend`) implements the same
    /// trait behind `#[cfg(target_os = "android")]`.
    #[test]
    fn process_backend_implements_the_seam() {
        fn assert_backend<T: SessionBackend>() {}
        assert_backend::<ProcessBackend>();
    }
}
