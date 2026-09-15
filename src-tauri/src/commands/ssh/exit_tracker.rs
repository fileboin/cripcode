//! Exit-status accounting for the Android russh channel pump.
//!
//! SSH servers send `exit-status` around channel EOF, but the ordering is not
//! guaranteed for every command shape: the observed Android failure was a
//! status arriving AFTER the pump had already stopped on `Eof`, leaving the
//! session code at `-1`. This tracker makes the ordering rules explicit and
//! unit-testable, independent of the transport:
//!
//! - a real `exit-status` is recorded whenever it arrives (before OR after EOF)
//! - `exit-signal` records `-1` (no numeric status exists)
//! - `Eof` alone does NOT end the pump — a status may still follow
//! - only `Close`/`None` ends the pump; when no status was ever seen the code
//!   stays `-1` (the established "unknown" fallback — never silently `0`)
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

/// Minimal event set the pump feeds into the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PumpEvent {
    /// Remote `exit-status` channel request.
    ExitStatus(i32),
    /// Remote `exit-signal` channel request (terminated by signal).
    ExitSignal,
    /// Channel EOF — the peer finished writing; a status may still follow.
    Eof,
    /// Channel close, or the russh receiver reporting `None`.
    Close,
}

/// Records the remote exit code and decides when the pump is finished.
#[derive(Debug, Default)]
pub(crate) struct ExitTracker {
    exit_code: Option<i32>,
}

impl ExitTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed one event. Returns `true` when the pump should stop.
    pub(crate) fn on_event(&mut self, event: PumpEvent) -> bool {
        match event {
            PumpEvent::ExitStatus(code) => {
                self.exit_code = Some(code);
                false
            }
            // A signal termination has no numeric exit code; `-1` keeps the
            // existing SessionBackend "unknown/terminated" contract.
            PumpEvent::ExitSignal => {
                self.exit_code = Some(-1);
                false
            }
            // EOF does not finish the pump: exit-status may arrive after it.
            PumpEvent::Eof => false,
            PumpEvent::Close => true,
        }
    }

    /// The exit code to report. `-1` when no real status was ever received —
    /// a closed channel is never reported as a clean `0`.
    pub(crate) fn exit_code(&self) -> i32 {
        self.exit_code.unwrap_or(-1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(events: &[PumpEvent]) -> (i32, bool) {
        let mut t = ExitTracker::new();
        let mut done = false;
        for ev in events {
            done = t.on_event(*ev);
            if done {
                break;
            }
        }
        (t.exit_code(), done)
    }

    #[test]
    fn remote_exit_zero_maps_to_zero() {
        let (code, done) = run(&[PumpEvent::ExitStatus(0), PumpEvent::Eof, PumpEvent::Close]);
        assert_eq!(code, 0);
        assert!(done);
    }

    #[test]
    fn remote_nonzero_maps_to_the_same_status() {
        for code in [1, 7, 42, 127, 255] {
            let (out, _) = run(&[
                PumpEvent::ExitStatus(code),
                PumpEvent::Eof,
                PumpEvent::Close,
            ]);
            assert_eq!(out, code);
        }
    }

    #[test]
    fn status_before_eof_is_kept() {
        let (code, _) = run(&[PumpEvent::ExitStatus(3), PumpEvent::Eof, PumpEvent::Close]);
        assert_eq!(code, 3);
    }

    /// The F3c.1 regression: status arriving after EOF must still be captured.
    #[test]
    fn status_after_eof_is_still_captured() {
        let (code, _) = run(&[PumpEvent::Eof, PumpEvent::ExitStatus(0), PumpEvent::Close]);
        assert_eq!(code, 0);
        let (code, _) = run(&[PumpEvent::Eof, PumpEvent::ExitStatus(7), PumpEvent::Close]);
        assert_eq!(code, 7);
    }

    #[test]
    fn eof_without_any_status_falls_back_to_unknown() {
        let (code, done) = run(&[PumpEvent::Eof, PumpEvent::Close]);
        assert_eq!(code, -1, "closed channel must never be reported as exit 0");
        assert!(done);
    }

    #[test]
    fn exit_signal_maps_to_unknown() {
        let (code, _) = run(&[PumpEvent::ExitSignal, PumpEvent::Close]);
        assert_eq!(code, -1);
    }

    #[test]
    fn eof_alone_does_not_finish_the_pump() {
        let mut t = ExitTracker::new();
        assert!(!t.on_event(PumpEvent::Eof));
        // The grace expiry path reads `exit_code()` directly.
        assert_eq!(t.exit_code(), -1);
    }

    #[test]
    fn close_is_terminal_and_idempotent() {
        let mut t = ExitTracker::new();
        assert!(!t.on_event(PumpEvent::ExitStatus(5)));
        assert!(t.on_event(PumpEvent::Close));
        // Extra events after close must not panic or change the code.
        assert!(t.on_event(PumpEvent::Close));
        assert!(!t.on_event(PumpEvent::ExitStatus(9)));
        assert_eq!(t.exit_code(), 9, "last write wins; no panic on late events");
    }

    #[test]
    fn close_without_status_is_safe() {
        let (code, done) = run(&[PumpEvent::Close]);
        assert_eq!(code, -1);
        assert!(done);
    }

    /// Full lifecycle shapes seen from a real server, in one table.
    #[test]
    fn pump_lifecycle_matrix() {
        let cases: &[(&[PumpEvent], i32)] = &[
            (
                &[PumpEvent::ExitStatus(0), PumpEvent::Eof, PumpEvent::Close],
                0,
            ),
            (
                &[PumpEvent::ExitStatus(1), PumpEvent::Eof, PumpEvent::Close],
                1,
            ),
            (
                &[PumpEvent::Eof, PumpEvent::ExitStatus(2), PumpEvent::Close],
                2,
            ),
            (&[PumpEvent::Eof, PumpEvent::Close], -1),
            (&[PumpEvent::Close], -1),
            (
                &[PumpEvent::ExitSignal, PumpEvent::Eof, PumpEvent::Close],
                -1,
            ),
        ];
        for (events, expected) in cases {
            let (code, done) = run(events);
            assert!(done, "pump must finish for {events:?}");
            assert_eq!(code, *expected, "wrong exit code for {events:?}");
        }
    }
}
