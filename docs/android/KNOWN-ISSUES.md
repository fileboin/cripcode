# CripCode Android — Known Issues

Open, verified problems on Android. Read [README.md](README.md) first.

**Nothing here is marked fixed without observed evidence on a real device.** Some
of these have partial fixes sitting uncommitted — those are called out
explicitly, and an uncommitted fix is still an open issue.

---

## 1. APK update does not complete

**Status: OPEN.** The in-app update flow is implemented
(`commands/android_update.rs`, `AndroidUpdateBanner.tsx`,
`src/lib/androidUpdate.ts`, committed in `1ebcec75`) and its frontend tests pass
(see [TEST-RESULTS.md](TEST-RESULTS.md)), but the end-to-end upgrade on a
physical device does **not** complete.

The work in flight is diagnostic, not corrective: the backend now emits
`CRIPCODE_UPDATE_DIAG` lines and progress/complete/failed events, and
`commands/update_diagnostics.rs` exposes them through an in-app view with copy /
share. That exists so the next attempt produces an answer instead of silence.

Suspects already ruled in or out by the implementation:

- The download is a long-held `invoke`. The Android WebView IPC bridge can break
  mid-flight (`Error invoking postMessage: Java exception`), losing the response.
  The event-based path was added to cover this — **not yet confirmed sufficient.**
- SHA-256 verification failure is covered by a passing test and correctly blocks
  the installer.
- Android itself will refuse an APK not signed with the **same** release key as
  the installed app. A debug-signed APK over a release-signed install fails
  here. See [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md).
- The metadata endpoint and the APK URL prefix must match the constants in
  `commands/android_update.rs`. A `versionCode` that is not strictly greater
  produces no prompt by design.

**Do not** mark this fixed until a real device installs a higher `versionCode`
APK and the app comes up on it. Capture the diagnostics output when it fails.

---

## 2. OpenCode remote session disappears after page refresh

**Status: OPEN.** A remote OpenCode/agent session starts and runs, but it does
not survive a page refresh (or an equivalent WebView teardown/recreate). After
the refresh the session is gone and must be relaunched.

The PTY session lives in a backend registry keyed by the tab's `sessionId`. A
refresh destroys the frontend; whether the backend session is reattached or
orphaned is the open question. Relevant code:

- `src-tauri/src/commands/pty_session.rs` — registry, ring buffer, attach
  offsets, liveness.
- `src-tauri/src/lib.rs` — window `Destroyed` handling.
- `src/components/dashboard/AgentLaunchPanel.tsx` — the launch surface.

**Do not** paper over this with frontend-only state that survives a refresh
while the backend session does not. First establish *which side* loses the
session, then fix that side.

---

## 3. Password authentication: session-only, E2E unverified

**Status: OPEN — implemented, not verified.**

The Android password path was fail-closed ("not available on this platform")
until the uncommitted work in `ssh/secrets.rs`, `ssh/transport_russh.rs` and
`ssh/config.rs` enabled it via `SessionSecretStore`.

Behaviour, by design:

- The password lives in **process memory for the app's lifetime only**. Nothing
  is written to disk, because the Android OS keystore has no usable backend for
  this crate on that platform.
- After an app process restart the store is empty and the server **must be
  edited to re-enter its password**. This is intentional and must not be
  "fixed" by introducing plaintext storage.
- A blank password field on a config-only edit (rename, port change) succeeds
  and writes nothing.
- `AuthType::Key` remains the fully-exercised path.

**Not yet done:** an end-to-end physical test of add → connect → authenticate →
run a command with a password-mode server. Until that is done, treat password
auth as unproven on Android.

---

## 4. SSH server-list layout and duplicate servers

**Status: OPEN — layout open, duplicate-server fix uncommitted.**

Two symptoms were observed on the device:

1. **Layout.** The server list renders poorly on a phone-sized viewport. The
   uncommitted `src/components/ssh/ServerList.tsx` (+14 lines) and
   `src/styles/features/ssh.css` (+7) touch this, but the layout has not been
   confirmed good on a real phone.
2. **Duplicates.** Adding the same VPS twice produced two server records with
   different ids, and remote projects stayed bound to the stale id. Root cause:
   the backend connection test needs a persisted server id, so the first Test on
   an unsaved form created a server — and cancelling, or saving after testing,
   created a second one.

The fix is in the uncommitted `AddServerModal.tsx`: one form yields at most one
server. The first Test persists exactly one draft (`testDraftId`); every later
Test/Save **updates that same id**; closing without saving deletes the draft.
Editing always goes through `update_ssh_server(existingId, …)`, so a server id —
and every remote project bound to it — survives re-entry.

**This fix is uncommitted.** It is covered by
`AddServerModal.test.tsx` (12 tests passing), but it has not been observed on a
device. Commit it and verify on hardware before treating it as resolved.

---

## 5. Version skew between `Cargo.toml` and `tauri.conf.json`

**Status: OPEN — minor, but it will confuse version reporting.**

In the working tree:

| File | Version |
|---|---|
| `src-tauri/Cargo.toml` | `0.18.18` |
| `src-tauri/tauri.conf.json` | `0.18.21` |

The installed APK reports `versionName 0.18.21`, `versionCode 18020` (from
`tauri.properties`, which the Tauri CLI generates from `tauri.conf.json`). The
Rust crate version differs. Anything deriving a version from `CARGO_PKG_VERSION`
— including the fallback path in `android_update::local_version_code()` — will
disagree with the Android build. Reconcile them before relying on
version-based behaviour.

---

## 6. Passphrase-protected SSH keys are unsupported

**Status: OPEN by design.**

`import_ssh_key` validates with `russh::keys::decode_secret_key(text, None)` and
rejects passphrase-protected keys, fail-closed. There is no passphrase prompt and
no passphrase storage. Do not add a workaround that stores a passphrase
insecurely — keyring-backed passphrase storage is the correct fix if this ever
becomes a requirement.

---

## 7. Android code is not compile-checked by CI or host tests

**Status: OPEN — structural risk.**

`.github/workflows/` contains only `ci.yml`, `release.yml` and
`release-windows.yml`; none mention Android. Separately, a host `cargo test`
does not compile `#[cfg(target_os = "android")]` blocks at all. The practical
consequence:

> An Android-only compile error — a wrong import, a changed russh API, a broken
> `#[cfg]` boundary — **will not be caught** by `pnpm check:all` or
> `pnpm rust:test`. It only surfaces on the next real Android build.

Until an Android build job exists, treat "CI is green" as meaning nothing about
Android. Budget time for a manual Android build after any Android change.

---

## 8. Stray untracked files in `src-tauri/`

**Status: OPEN — housekeeping.**

Five untracked files matching
`src-tauri/C?\u{200}\u{200}Usersbojan…ss-kill-test-<pid>-survived-marker` exist.
They are Windows console-encoding artefacts from PTY kill tests, unrelated to
Android. They were left in place deliberately — deleting files was out of scope.
Remove them in a separate, explicitly-approved cleanup.

---

## Not issues

Things that look like bugs but are working as designed:

- **Password lost after an app restart.** By design — see issue 3.
- **No update prompt on an equal or lower `versionCode`.** By design — strictly
  greater only.
- **Host key blocked on first use.** By design — trust is only recorded after an
  explicit confirmation. There is no `accept-new`.
- **Desktop still shells out to the `ssh` CLI.** Intended. Only Android uses
  `russh`.
- **SSH server config lives in the app-private data dir on Android.** Intended;
  Android has no home directory.

---

## Related documents

- [README.md](README.md)
- [ARCHITECTURE.md](ARCHITECTURE.md)
- [IMPLEMENTATION-HISTORY.md](IMPLEMENTATION-HISTORY.md)
- [TEST-RESULTS.md](TEST-RESULTS.md)
- [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)
- [NEXT-STEPS.md](NEXT-STEPS.md)