# CripCode Android — Next Steps

Prioritised work for the next agent. Read [README.md](README.md) first, then
[KNOWN-ISSUES.md](KNOWN-ISSUES.md).

Order matters. Steps 0–2 are the ones that block the product being usable from a
phone; everything after is refinement.

**Each step follows the standing procedure:**
`READ-ONLY audit → findings → recommended minimal fix → approval → implementation → tests → report → STOP`

---

## 0. Get the work out of this machine — do this first

**Why:** `origin/feat/isolate-cripcode` sits at `10dc121b`, the commit *before*
the first Android commit. All 15 Android commits and the entire uncommitted
working set exist **only locally**. Until they are pushed, this handover is the
only record, and a lost disk loses everything.

**Before pushing:** the working tree mixes finished Android work with unrelated
Windows console-encoding artefacts. Stage only the intended files.

**Needs explicit approval.** Do not commit or push without it.

---

## 1. Fix APK updating — highest priority

**Why:** an APK that cannot update itself is a dead end for a phone-only
workflow — every fix requires a PC and a cable, which is exactly what this
project exists to eliminate.

Issue: [KNOWN-ISSUES §1](KNOWN-ISSUES.md#1-apk-update-does-not-complete).

1. Read `src-tauri/src/commands/android_update.rs`,
   `src/lib/androidUpdate.ts`, `src/components/AndroidUpdateBanner.tsx`,
   `src-tauri/src/commands/update_diagnostics.rs`.
2. **First read-only pass:** enumerate every way the flow can terminate —
   metadata fetch, versionCode comparison, URL-prefix check, download, SHA-256
   verify, installer Intent, and the IPC bridge loss that motivates the event
   fallback. Confirm which terminal state the device actually reached.
3. Build a signed release APK, publish it with a **strictly greater
   `versionCode`**, and install it on a real phone over mobile internet.
4. When it fails, use the in-app diagnostics view (share the
   `CRIPCODE_UPDATE_DIAG` lines) instead of guessing. Record what you find in
   [TEST-RESULTS.md](TEST-RESULTS.md).
5. Fix the smallest thing that is actually broken. Do not redesign the flow.

Check `src-tauri/Cargo.toml` vs `tauri.conf.json` version skew while here —
[KNOWN-ISSUES §5](KNOWN-ISSUES.md#5-version-skew-between-cargotoml-and-tauriconfjson).

---

## 2. Make the OpenCode session survive a page refresh

**Why:** losing the agent session on every WebView teardown makes the phone
client unusable for real work, regardless of everything else being fixed.

Issue: [KNOWN-ISSUES §2](KNOWN-ISSUES.md#2-opencode-remote-session-disappears-after-page-refresh).

1. Read `src-tauri/src/commands/pty_session.rs` (registry, ring buffer, attach
   offsets, liveness, exit code, events), `src/lib/ptySession.ts`, and the
   window `Destroyed` handling in `src-tauri/src/lib.rs`.
2. **First read-only pass:** establish which side loses the session — the
   backend drops it, or the backend keeps it and the frontend fails to reattach.
   Log the session id on both sides and compare across one refresh. Do not fix
   this until you know which.
3. Then make the smallest change that survives a refresh — most likely
   frontend reattachment to an existing backend session id, **not** a backend
   redesign.

Do **not** paper over this with frontend-only state that outlives a backend
session that does not.

---

## 3. Commit the work already in the tree

Several fixes are complete, tested, and simply uncommitted. Review and land
them individually so each is reviewable:

| Area | Files | Why |
|---|---|---|
| Android password auth | `ssh/secrets.rs`, `ssh/transport_russh.rs`, `ssh/config.rs` | Removes the "not available on this platform" fail-closed blocks; adds the session-only store |
| Plain Terminal login shell | `pty_session.rs`, `ssh/transport_russh.rs` | `open_shell(..., Option<&str>)`; without it the transport exec'd the literal `user@host` |
| Duplicate servers | `AddServerModal.tsx` + test | One form → at most one server; preserves server id and project bindings |
| Release signing config | `gen/android/app/build.gradle.kts` | Without it, release builds are unsigned |
| Conditional frontend build | `scripts/conditional-build.mjs` + `tauri.conf.json` | Cuts a 20–45 min Rust recompile on unchanged Android builds |
| Update diagnostics | `android_update.rs`, `update_diagnostics.rs`, `AndroidUpdate*`, `androidUpdate.ts`, `diagLog.ts`, `androidServerDiag.ts` | The instrumentation step 1 depends on |

Run `pnpm check:all && pnpm test:run && pnpm rust:test` before each. Note that
these gates do **not** compile Android code — also build the APK.

---

## 4. Complete password-authentication end-to-end verification

Issue: [KNOWN-ISSUES §3](KNOWN-ISSUES.md#3-password-authentication-session-only-e2e-unverified).

1. Build and install a signed release APK.
2. Add a password-mode SSH server, confirm the host key, connect, authenticate,
   run a real remote command.
3. Confirm the intended session-only behaviour: **kill and relaunch the app**,
   then attempt to reconnect — it must fail with the "re-enter its password"
   message, and editing the server must restore it.
4. Record the result in [TEST-RESULTS.md](TEST-RESULTS.md).

Never introduce an at-rest plaintext fallback to make this easier.

---

## 5. Prove it works on a real phone, alone, over the internet

This is the acceptance test for the whole product. It must pass **with no PC,
no USB cable, and ideally not the build machine's Wi-Fi**:

- [ ] Install the signed release APK from the distribution endpoint.
- [ ] Phone on mobile data (4G/5G) — prove the VPS is reachable over the public
      internet, not only on a LAN.
- [ ] Add the server, approve the host key, connect.
- [ ] Terminal → login shell.
- [ ] Agent terminal → OpenCode starts, streams output, accepts input.
- [ ] Files → browse, read, write, rename, delete.
- [ ] Git → status, diff, commit, pull, push in the remote workspace.
- [ ] Ollama → status, model list, selected model.
- [ ] Remote build → start, stream logs, correct exit code, stop.
- [ ] Dev server → start, restart, stop.
- [ ] Preview → tunnel up, HTTP probe succeeds, tunnel stopped cleanly.
- [ ] Refresh the page mid-session and background/re-enter the app.
- [ ] Cleanup → nothing left running on the VPS.
- [ ] Security → a changed host key is **blocked**; no credential appears in any
      log, export or diagnostics view.

---

## 6. Give CI the ability to catch Android breakage

Issue: [KNOWN-ISSUES §7](KNOWN-ISSUES.md#7-android-code-is-not-compile-checked-by-ci-or-host-tests).

Today no workflow builds Android, and a host `cargo test` never compiles
`#[cfg(target_os = "android")]` blocks. A one-line Android compile break ships
silently.

**Constraint:** GitHub workflows and release configuration must not change
without explicit approval. Propose the change, get a yes, then implement — an
Android build job that at minimum compiles the four Rust targets and assembles a
debug APK, without touching signing secrets.

---

## 7. Smaller items, in order

| Priority | Item | Ref |
|---|---|---|
| Medium | SSH server-list layout on a phone viewport | [§4](KNOWN-ISSUES.md#4-ssh-server-list-layout-and-duplicate-servers) |
| Medium | Reconcile `Cargo.toml` vs `tauri.conf.json` versions | [§5](KNOWN-ISSUES.md#5-version-skew-between-cargotoml-and-tauriconfjson) |
| Medium | Wire the Cmd+K palette to the Android-relevant actions | `CLAUDE.md` → "New feature → contribute commands" |
| Low | Keystore-backed passphrase support for SSH keys — only if required, never a plaintext fallback | [§6](KNOWN-ISSUES.md#6-passphrase-protected-ssh-keys-are-unsupported) |
| Low | Remove the five stray PTY-kill-test marker files — separate, approved cleanup | [§8](KNOWN-ISSUES.md#8-stray-untracked-files-in-src-tauri) |
| Low | Document diagnostics on-device: log location, how to pull `cripcode.log` | — |

---

## Standing constraints

These apply to every step above.

1. **Do not break web/desktop.** Every Android change belongs behind
   `#[cfg(target_os = "android")]` / `#[cfg(desktop)]` / `isAndroid()`.
2. **Shared-code changes need prior explanation and explicit approval.**
3. **No second agent system, no duplicate project or Git architecture.**
4. **No VPS, deployment, environment, DNS, storage, GitHub-settings or secret
   changes without explicit approval.**
5. **Never expose credentials** in logs, docs, arguments or environment.
6. **No commit, no push without explicit approval.**
7. **Never mark an issue fixed without device evidence.** Update
   [KNOWN-ISSUES.md](KNOWN-ISSUES.md) and [TEST-RESULTS.md](TEST-RESULTS.md)
   when you have it.
8. **Report and stop.** Do not roll on to the next step.

---

## Related documents

- [README.md](README.md)
- [ARCHITECTURE.md](ARCHITECTURE.md)
- [IMPLEMENTATION-HISTORY.md](IMPLEMENTATION-HISTORY.md)
- [TEST-RESULTS.md](TEST-RESULTS.md)
- [KNOWN-ISSUES.md](KNOWN-ISSUES.md)
- [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)