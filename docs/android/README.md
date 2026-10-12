# CripCode Android — Agent Entry Point

**READ THIS FILE BEFORE MAKING ANY ANDROID CHANGE.**

CripCode Android is the phone build of CripCode. The phone is a *thin client*:
it holds no source code of its own. It selects a **Current Project**, binds it to
a **Remote Project** on a **VPS** over **SSH**, and drives the **Original Agent**
(OpenCode) that runs on that VPS. Everything heavy — the agent, Ollama, Git,
builds, dev servers — executes on the remote machine, not on the phone.

This document is the handover. Everything a future agent needs to continue the
work is reachable from here.

---

## 1. Read in this order

| # | File | Read it when |
|---|------|--------------|
| 1 | [ARCHITECTURE.md](ARCHITECTURE.md) | Before touching any Android code. Explains the `russh` transport, host-key gate, auth, and how the phone reaches the VPS. |
| 2 | [IMPLEMENTATION-HISTORY.md](IMPLEMENTATION-HISTORY.md) | To understand what was already built, in order, and why each piece exists. |
| 3 | [TEST-RESULTS.md](TEST-RESULTS.md) | To know what was **proven** on a real device and what was **not**. Do not assume. |
| 4 | [KNOWN-ISSUES.md](KNOWN-ISSUES.md) | Before "fixing" something that looks broken — several things are known-broken on purpose. |
| 5 | [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md) | Before building an APK or changing versioning. |
| 6 | [NEXT-STEPS.md](NEXT-STEPS.md) | To pick up the work where the previous agent stopped. |

---

## 2. Current status (as of this handover)

**Verified in the repository on the day these docs were written.**

- Android support is **implemented and builds**. See
  [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md) for what was produced.
- The whole Android line of work is **15 commits on the local branch
  `feat/isolate-cripcode`**, plus a further **uncommitted working set**.
- ⚠️ **Those commits are NOT on the remote.** `origin/feat/isolate-cripcode`
  points at `10dc121b`, which is the commit *before* the first Android commit.
  Anyone who clones the repository today gets **none** of the Android work.
  This is the single most important fact on this page. See
  [IMPLEMENTATION-HISTORY.md → "The Android work is unpushed"](IMPLEMENTATION-HISTORY.md#the-android-work-is-unpushed).
- The remote OpenCode/session flow has **open, unresolved defects** — see
  [KNOWN-ISSUES.md](KNOWN-ISSUES.md). Do not describe it as finished.
- Password authentication on Android is **implemented but not verified
  end-to-end on a device**.

---

## 3. Target workflow

```
Android phone
   │  Internet / Wi-Fi / 4G / 5G
   ▼
CripCode APK
   ▼
Current Project            (the project selected in CripCode)
   ▼
Remote Project mapping      (server + path on the VPS)
   ▼
Execution Target = SSH/VPS
   ▼
SSH connection              (in-process russh, no ssh binary)
   ▼
VPS                         (the remote execution environment)
   ▼
Original Agent (OpenCode)   │
   ├── Ollama               │  model runtime, hosted on the VPS
   ├── Git                  │  version control in the remote workspace
   ├── remote build         │
   ├── dev server           │
   └── tunnel + preview     │
```

The phone never needs a PC, a USB cable, or a local clone. It talks to the VPS
directly over the public internet.

Roles of each layer, stated precisely so they are not confused:

| Layer | Role | Is it… |
|-------|------|--------|
| **GitHub** | Hosts the CripCode repository and its history. Source context and version control for *CripCode itself*. | Not the runtime. The phone does not need GitHub to run a remote agent — it needs SSH to the VPS. |
| **Current Project** | The project the user has selected inside CripCode. | The local *selection*, not a local clone. |
| **Remote Project** | The mapping from a Current Project to a concrete `server + remote path` on the VPS. | Where the code actually lives. |
| **Execution Target** | Local or remote. On Android, remote work runs through an SSH execution target. | The choice of *where* commands run. |
| **SSH** | The secure transport and remote execution channel. | **Not** a project, **not** an agent, **not** interchangeable with either. |
| **VPS** | The remote execution environment where commands, builds and agents run. | The machine. |
| **OpenCode** | The Original Agent — the AI coding agent that performs the work. | **Not** the SSH connection and **not** the project. |
| **Ollama** | The model runtime, when hosted on the VPS. | Provides models to the agent. |
| **Git** | Version control inside the relevant project workspace (remote workspace on the VPS). | Distinct from GitHub. |

**SSH, Remote Project and OpenCode are three different things.** SSH is the pipe.
The Remote Project is the mapped location on the far machine. OpenCode is the
program doing the work through that pipe. Never collapse them into one another in
code, in comments, or in user-facing copy.

---

## 4. Rules for every agent working on this codebase

These are binding. If a task seems to require breaking one, **stop and ask**.

1. **Read the Android documentation before making changes.** Start with this
   file, then [ARCHITECTURE.md](ARCHITECTURE.md).
2. **Preserve existing functionality.** Do not silently drop, disable, or
   "simplify" a working path to make a new one work.
3. **Make the smallest necessary change.** One problem, one minimal fix.
4. **Android work must not alter web/desktop behaviour.** Everything Android is
   reached through `#[cfg(target_os = "android")]` / `#[cfg(desktop)]` /
   `isAndroid()` branches. If your change alters a shared code path, it must be
   provably a no-op on desktop and web — and say so in your report.
5. **Prefer Android-specific fixes.** Fix inside the Android branch rather than
   reshaping shared logic.
6. **Shared-code changes require prior explanation and explicit approval.**
   Before touching anything both platforms use, state what changes for desktop
   and web, and wait for a yes.
7. **Do not introduce a second agent system, or duplicate the project / Git
   architecture.** One agent (OpenCode), one project model, one Git model.
8. **No VPS, deployment, environment, DNS, storage, GitHub-settings or secret
   changes without explicit approval.** These are out of scope by default.
9. **No commit and no push without explicit approval.**
10. **Never expose credentials** in logs, documentation, command arguments or
    environment variables. See [BUILD-AND-SIGNING.md → Secrets](BUILD-AND-SIGNING.md#secrets).
11. **Do not mark an issue fixed without evidence.** A code change is not a fix
    until it has been observed working on a real device; record that in
    [TEST-RESULTS.md](TEST-RESULTS.md).

### The standing procedure for any new problem

```
READ-ONLY audit
   → findings
   → recommended minimal fix
   → approval
   → implementation
   → tests
   → report
   → STOP
```

Do not skip the first four steps to get to the edit faster.

---

## 5. Where the Android code lives

| Area | Path |
|------|------|
| Android app module (Gradle) | `src-tauri/gen/android/` |
| App data dir helper | `src-tauri/src/android_paths.rs` |
| **SSH transport (the core)** | `src-tauri/src/commands/ssh/transport_russh.rs` |
| Remote exec seam | `src-tauri/src/commands/ssh/exec.rs` |
| Exit-status accounting | `src-tauri/src/commands/ssh/exit_tracker.rs` |
| Session transport seam | `src-tauri/src/commands/session_backend.rs` |
| Host-key policy (shared) | `src-tauri/src/commands/ssh/host_key.rs` |
| Private-key import | `src-tauri/src/commands/ssh/key_import.rs` |
| Secret store (desktop/Android split) | `src-tauri/src/commands/ssh/secrets.rs` |
| In-app update | `src-tauri/src/commands/android_update.rs` |
| Update diagnostics | `src-tauri/src/commands/update_diagnostics.rs` |
| Update frontend | `src/lib/androidUpdate.ts`, `src/components/AndroidUpdateBanner.tsx` |
| SSH server UI | `src/components/ssh/` |

---

## 6. Verification note

There is **no Android CI workflow** in `.github/workflows/`. Android builds and
device tests are performed manually. Before saying "Android passes CI", remember
that CI never builds an APK — see [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)
for what must be run by hand.