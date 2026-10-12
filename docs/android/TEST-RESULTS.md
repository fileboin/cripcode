# CripCode Android — Test Results

What was actually proven, and what was not. Read [README.md](README.md) first.

This document deliberately separates **evidence** from **claims**. Where a result
is only a developer's account of a physical-device session and no artifact
survives in the repository, it says so.

---

## How to read this

| Tier | Meaning |
|---|---|
| **A — Verified in this pass** | Re-run while writing these docs. Command and result recorded. |
| **B — Verified historically** | Committed code and/or build artifacts prove it, but it was not re-run now. |
| **C — Reported, no artifact** | The developer states it passed on a device; nothing in the repo corroborates it. Treat as plausible, not as evidence. |
| **D — Untested / incomplete** | Known not to be finished. |

Nothing below is claimed as passing unless it is in tier A or B.

---

## Tier A — Verified in this pass (2026-10-12)

### Frontend unit tests, Android update + server configuration

```
node ./node_modules/vitest/vitest.mjs run \
  src/lib/androidUpdate.test.ts \
  src/components/AndroidUpdateBanner.test.tsx \
  src/components/ssh/AddServerModal.test.tsx \
  src/lib/updateDiagnostics.test.ts
```

**Result: 4 test files passed, 57 tests passed, 0 failed** (67 s).

Notable cases confirmed passing:

| Test file | Coverage |
|---|---|
| `AndroidUpdateBanner.test.tsx` | prompt appears when remote `versionCode` is higher; "Later" defers that `versionCode` for the session; plain label when the server sends no `Content-Length`; **a failed SHA verification never reaches the installer**; a double tap starts exactly one download; the diagnostics view opens from the banner |
| `AddServerModal.test.tsx` | defaults to key mode (key-path field visible, `authType: key` submitted); password mode shows the password field and passes it transiently |
| `androidUpdate.test.ts` | update check / download helpers |
| `updateDiagnostics.test.ts` | diagnostics parsing helpers |

### No documentation checks exist

`pnpm format:check` only covers `src/**/*.{ts,tsx,css}`; `check:patterns` and
`check:loc` only scan source. There is **no lint, link-check or style check for
`docs/`**. The documentation added here is therefore unverified by tooling.

### Rust tests were not re-run

`cargo test` was attempted and exceeded a 15-minute budget while compiling the
workspace. **No claim is made about the current Rust test state.** Run
`pnpm rust:test` before relying on it.

---

## Tier B — Verified historically (evidence in the repository)

### The app builds

Gradle build outputs exist on disk:

| Artifact | Size | Built |
|---|---|---|
| `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk` | ~205 MB | 2026-09-29 |
| `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` | ~445 MB | 2026-09-24 |
| `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab` | ~24.6 MB | 2026-09-24 |

The release `output-metadata.json` records `applicationId
com.fileboin.cripcode`, `versionCode 18020`, `versionName 0.18.21`, `minSdk 24`.

A release **R8 minified** build (`mapping.txt`, 17 MB) and a signed release
configuration both exist — see [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md).

### Rust test suites covering Android logic exist in source

Counted `#[test]` functions per file (the repository compiles and runs these on
the **desktop host**; note that `#[cfg(target_os = "android")]` code is *not*
compiled by a host `cargo test`):

| File | `#[test]` fns | What it covers |
|---|---|---|
| `ssh/exit_tracker.rs` | 10 | exit-status ordering incl. status-after-EOF, `exit-signal` → `-1`, never-a-fake-zero, lifecycle matrix |
| `ssh/key_import.rs` | 7 | key file naming / traversal stripping / UUID fallback, writes inside `keys/`, overwrite, empty-key fail-closed, **no key material in error strings**, `0600` on Unix |
| `ssh/secrets.rs` | 6 | session store round-trip + idempotent delete, askpass behaviour |
| `ssh/config.rs` | 22 | add/update/delete with a mock store; blank-password edit on an empty session store; server id preserved across edits |
| `commands/android_update.rs` | 13 | versionCode comparison, SHA-256 format, trusted-URL prefix |
| `commands/update_diagnostics.rs` | 6 | marker-line extraction, raw-text fallback for malformed lines, newest-N cap, tail read drops a partial first line |
| `commands/pty_session.rs` | 26 | incl. 3 Android argv-parser tests (login shell vs remote command) |
| `commands/setup/status.rs` | 4 | incl. Android readiness with/without a configured server |
| `ssh/remote_build.rs`, `ssh/remote_dev_server.rs` | 8, 9 | shell construction, path-injection safety, `< /dev/null` detach |
| `ssh/host_key.rs` | 24 | host-key state evaluation |

### Bugs that were found by real device testing

Two commits exist purely because of physical-device findings, and their
regression tests are committed:

1. `9fb70102` — `exit-status` arriving **after** channel EOF produced a
   permanent `-1`. Fixed by `exit_tracker.rs`; regression test
   `status_after_eof_is_still_captured`.
2. `5680ca01` — `nohup bash -c … &` inherited the SSH channel's stdin, so the
   russh exec never saw the channel close and background builds / dev servers
   hung. Fixed with `< /dev/null`; assertion added to the build-command test.

The existence of these fixes is strong evidence that the exec, PTY, build and
dev-server paths were exercised on a device. It is **not** evidence that every
path passes today.

---

## Tier C — Reported on a device, no artifact in the repository

No device log, screenshot or test report is committed anywhere in this
repository (checked: `docs/`, `docs/internal/`, `.shipstudio/`, root). The
following are the developer's account of physical-device sessions. **Treat them
as a working baseline, not as proof.**

| Area | Reported status |
|---|---|
| SSH connection over mobile internet | Working |
| Host-key unknown → approval gate | Working |
| Host-key changed → blocked | Working |
| PTY / terminal session over SSH | Working |
| OpenCode (Original Agent) on the VPS | Worked, but **the session disappears after a page refresh** — see [KNOWN-ISSUES.md](KNOWN-ISSUES.md#2-opencode-remote-session-disappears-after-page-refresh) |
| Ollama (on the VPS) | Working |
| Git (remote workspace) | Working |
| Files (browse / read / write / rename / delete) | Working |
| Remote build | Working |
| Dev server | Working |
| Tunnel + preview | Working |
| Cleanup (stop build / dev server / tunnel) | Working |
| **Password authentication** | **Implemented but NOT verified end-to-end** — see [KNOWN-ISSUES.md](KNOWN-ISSUES.md#3-password-authentication-session-only-e2e-unverified) |
| **APK in-app update** | **Not working end-to-end** — see [KNOWN-ISSUES.md](KNOWN-ISSUES.md#1-apk-update-does-not-complete) |

---

## Tier D — Untested or incomplete

| Area | State |
|---|---|
| Android build in CI | **Does not exist.** `.github/workflows/` has only `ci.yml`, `release.yml`, `release-windows.yml`; none mention Android. Nothing catches an Android compile break automatically. |
| Android code compiled by `cargo test` | **No.** `#[cfg(target_os = "android")]` blocks are invisible to a host build. Only a real Android cargo build (through Gradle) type-checks them. |
| `isNewer`, `is_valid_sha256`, `is_trusted_apk_url` | Unit-tested on the host only. The download/installer path is JNI-heavy and runs nowhere but a device. |
| Passphrase-protected SSH keys | Rejected by design; no test other than the rejection path. |
| Desktop/web regression | Not verified in this pass. Run `pnpm check:all && pnpm test:run && pnpm rust:test` before landing any Android change. |
| Multiple Windows / concurrent projects on one device | Not tested. |
| Update across a genuine versionCode bump on a real device | Not achieved; see the known issue. |

---

## How to reproduce the device checks

There is no automation. A physical pass, in order:

1. Build and install a **signed release** APK (see
   [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)). A debug APK cannot install
   over a signed release build.
2. Put the phone on **mobile internet** (not the build machine's Wi-Fi) — the
   point is that no PC or USB cable is needed.
3. Add the SSH server, confirm the host key, connect.
4. Open the Terminal → check a login shell; open the agent terminal → check
   OpenCode starts.
5. Files → browse, read, write, rename, delete.
6. Git → status, diff, commit, pull, push.
7. Ollama → status, models, selected model.
8. Remote build → start, watch logs, confirm exit code, stop.
9. Dev server → start, restart, stop.
10. Preview → start tunnel, confirm HTTP probe, stop tunnel.
11. Refresh the page / background the app and re-enter — **this is where the
    session-loss defect shows up.**
12. Pull `cripcode.log` (or use the in-app update diagnostics view) and keep it
    as the record.

Record the outcome in this file, and update
[KNOWN-ISSUES.md](KNOWN-ISSUES.md) — an issue only moves to "fixed" when it has
been observed working.

---

## Related documents

- [README.md](README.md)
- [ARCHITECTURE.md](ARCHITECTURE.md)
- [IMPLEMENTATION-HISTORY.md](IMPLEMENTATION-HISTORY.md)
- [KNOWN-ISSUES.md](KNOWN-ISSUES.md)
- [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)
- [NEXT-STEPS.md](NEXT-STEPS.md)