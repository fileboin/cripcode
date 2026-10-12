# CripCode Android — Architecture

How the Android build works, and why each platform split exists. Read
[README.md](README.md) first.

---

## 1. The shape of the app on Android

CripCode is a Tauri v2 application. The Android build is produced from the same
repository as the desktop build, through the Tauri CLI's Android support, into
`src-tauri/gen/android/`.

The decisive constraint on Android is:

> **There is no OpenSSH binary on Android, and one cannot be exec'd out of
> app-private storage (W^X + SELinux).**

Everything below follows from that single fact. Desktop shells out to the
`ssh` CLI. Android cannot, so Android talks SSH **in-process**, in Rust, with
the [`russh`](https://crates.io/crates/russh) crate.

---

## 2. Transport seams

Android code is never sprinkled through the app. It sits behind two seams, so
the session registry, the file browser, the build runner and the dev-server
controller above them stay transport-agnostic and shared.

### Seam 1 — terminal sessions: `SessionBackend`

`src-tauri/src/commands/session_backend.rs` defines one trait:

```rust
pub trait SessionBackend: Send + Sync {
    fn read(&self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn write(&self, data: &[u8]) -> std::io::Result<()>;
    fn resize(&self, rows: u16, cols: u16) -> Result<(), String>;
    fn kill(&self);
    fn wait(&self) -> i32;   // 0 = clean, -1 = unknown
    fn pid(&self) -> u32;    // 0 for channel transports
}
```

| Implementation | Platform | Backing |
|----------------|----------|---------|
| `ProcessBackend` | desktop | a local PTY process (`portable_pty`); for remote execution targets that process is the OpenSSH client |
| `RusshBackend` | Android | an SSH channel on the VPS, driven by `russh` |

Everything above the trait — the ring buffer, attach offsets, liveness, exit
code, events, the whole session registry in `commands/pty_session.rs` — is
shared and unchanged.

`RusshBackend` bridges an async channel pump to the trait's synchronous
contract with `ChannelReader`, a blocking `Read` adapter fed over an
`std_mpsc::channel`. The sync side only ever pushes into an unbounded queue;
there is no `block_on` anywhere.

### Seam 2 — remote command execution: `exec::run` / `exec::run_with_stdin`

`src-tauri/src/commands/ssh/exec.rs` exposes the same
`std::process::Output` surface on both platforms:

- **Desktop** → spawns the `ssh` CLI, unchanged from before.
- **Android** → `transport_russh::exec(...)`, because there is no binary to
  spawn.

Non-zero remote exit codes are returned as `Ok(Output)` — matching desktop
`run_with_timeout` semantics. Only transport failures and timeouts are `Err`.

Callers migrated to this seam:

| Feature | File |
|---|---|
| Files (list/read/write/mkdir/delete/rename) | `commands/ssh/files.rs` |
| Remote Git | `commands/ssh/remote_git.rs` |
| Remote build | `commands/ssh/remote_build.rs` |
| Remote dev server | `commands/ssh/remote_dev_server.rs` |
| Ollama | `commands/ssh/ollama.rs` |
| OpenCode provider auth | `commands/ssh/opencode_auth.rs` |
| Remote agent presence check | `commands/ssh/remote_agent.rs` |

---

## 3. `transport_russh.rs` — the SSH transport

This is the heart of Android remote work. Everything below is in
`src-tauri/src/commands/ssh/transport_russh.rs`.

### Connection parameters

| Constant | Value | Mirrors |
|---|---|---|
| `CONNECT_TIMEOUT_SECS` | 15 | desktop `ConnectTimeout=10`, plus mobile headroom |
| `HOST_KEY_PROBE_TIMEOUT_SECS` | 15 | probe budget |
| `EXIT_STATUS_GRACE` | 2 s | wait for a late `exit-status` after EOF |
| `inactivity_timeout` | 3600 s | — |
| `keepalive_interval` / `keepalive_max` | 30 s / 3 | desktop `ServerAliveInterval=30`, `ServerAliveCountMax=3` |
| `nodelay` | true | interactive terminal responsiveness |

### Host-key verification

Android reproduces the desktop policy **exactly**. There is no `accept-new`, no
silent TOFU, no trust-on-first-use shortcut.

- The trusted-key store is an **app-private `known_hosts`**, never the desktop
  `~/.ssh/known_hosts`. Path: `<app data dir>/known_hosts`.
- Only entries matching the **exact host reference** (`host` or `[host]:port`)
  are considered — `known_hosts_lines_for_hostref` mirrors the desktop
  `ssh-keygen -F <hostref>` pre-filter, so a key blob belonging to a different
  host can never satisfy the comparison.
- The compared value is the **base64 key blob** (second token of an OpenSSH
  public-key line), not a display string.
- Fingerprints are rendered in OpenSSH form, `SHA256:<base64>` — identical to
  `ssh-keygen -lf`.
- Marker lines (`@revoked`, etc.) are handled by `known_hosts_line_blob`.

Two handlers implement the gate:

| Handler | Behaviour |
|---|---|
| `ProbeHandler` | Captures the offered key during KEX and returns `false`. **Never authenticates. Never writes `known_hosts`.** |
| `StrictHandler` | Accepts **only** a key blob already recorded for that host. Unknown or changed key → fail closed at KEX. |

Policy states (shared vocabulary with desktop, from `host_key.rs`):

- **unknown** → the existing explicit-approval flow runs; only after the user
  confirms does `accept_remote_host_key` append the key.
- **changed** → BLOCK.
- **accepted** → connect.

`accept_remote_host_key` re-runs the capture probe and appends the confirmed key.
Same accept flow as desktop, different implementation.

**Known limitation, carried over from the desktop path:** the fingerprint shown
in the modal comes from the `check` capture, while the stored key comes from a
second capture seconds later. If the server rotated its key in that window, the
stored key can differ from the displayed one. The real connections still verify
the stored key at connect time and refuse a mismatch, so the worst case is a
stored-but-unusable key, not an undetected MITM.

### Authentication

| Mode | Android behaviour |
|---|---|
| **Private key** | Key text is pasted into the UI, validated with the exact parser the transport uses (`russh::keys::decode_secret_key`), then written to `<app data dir>/keys/<sanitized-server-id>` with mode `0600`. Only the absolute path is stored on the server record. |
| **Password** | Read from the platform secret store. On Android that store is **process-lifetime memory only** (see below). |

**Passphrase-protected keys are rejected, fail-closed**, until passphrase storage
exists. Importing validates the key before writing, so a key that imports is a
key that can authenticate — there is no second, weaker validator.

Key file names are derived from the server id after stripping everything that is
not `[A-Za-z0-9_-]`, falling back to a UUID. Caller-supplied text cannot escape
the `keys/` directory. Error messages never contain key material — there is a
unit test asserting a secret marker cannot leak through `Display` or `Debug`.

### Secret storage: `store_for_platform()`

`src-tauri/src/commands/ssh/secrets.rs` selects the store at compile time:

| Platform | Store | Persistence |
|---|---|---|
| Desktop | `KeyringStore` (OS credential store) | durable |
| Android | `SessionSecretStore` | **process memory only** — nothing touches disk, the map dies with the process |

There is deliberately **no at-rest plaintext fallback anywhere**. Consequence:
after an app process restart, a password-mode server has no stored password and
must be edited to re-enter it. The error says exactly that. A blank password
field on a config-only edit (rename, port change) is accepted and writes nothing,
because the store may legitimately be empty.

### Interactive sessions (PTY)

`open_shell(server, cols, rows, remote_command: Option<&str>)`:

- Requests a PTY with **`TERM=xterm-256color` explicitly.** OpenSSH inherits
  `TERM` from the local environment; `russh` has no local environment to inherit
  from. OpenCode's TUI needs a capable terminal.
- `Some(cmd)` → `exec` the quoted shell program (execution targets, agent
  terminals).
- `None` → `request_shell` for the server's login shell. This is the plain
  Terminal's shape: the legacy frontend sends `ssh <options> <destination>` with
  no remote command, and without this branch the transport would try to exec the
  literal string `user@host` as a program.

`android_remote_command` (in `pty_session.rs`) parses the legacy argv shape and
is pure + platform-independent so it is unit-tested on every host. It knows
that `-o`, `-p` and `-i` consume the following argv entry, honours `--`, skips
the destination operand, and returns `None` when nothing follows.

### Exit status

`exit_tracker.rs` makes the ordering rules explicit and unit-testable,
independent of the transport:

- A real `exit-status` is recorded whenever it arrives — **before or after EOF**.
- `exit-signal` records `-1` (no numeric status exists).
- **`Eof` alone does not end the pump** — a status may still follow. This was a
  real defect: servers that send `exit-status` *after* EOF produced a permanent
  `-1` (internal reference F3c.1).
- Only `Close` / a `None` receiver ends the pump. After EOF the pump waits
  `EXIT_STATUS_GRACE` (2 s) for a late status.
- When no status was ever seen the code stays `-1`. **A closed channel is never
  reported as a clean `0`.**

`exit_status_from_code` encodes the remote code into a real
`std::process::ExitStatus` (exit code in the high byte of the Unix wait status;
unknown → SIGTERM), so callers keep the exact `status.success()` / `status.code()`
surface they already have.

### Remote port forwarding (preview tunnel)

`start_tunnel(server, remote_port, local_port)`:

- Reuses the **same** `connect_authenticated` path — one SSH connection, strict
  known-hosts verification, no separate SSH lifecycle and no `accept-new`.
- Binds a local listener on `127.0.0.1:<local_port>` and, per accepted
  connection, opens one `direct-tcpip` channel and copies bytes
  bidirectionally with a half-close — the same shape as desktop `ssh -L`.
- Connection tasks live in a `JoinSet` and are reaped, so the set cannot grow.
- A 1-second ticker detects a dropped SSH connection and lets the tunnel die with
  it; an `AliveGuard` clears the alive flag on **every** exit path, including
  task abort.
- `stop()` aborts the accept task: the listener drops (port released), in-flight
  tasks abort, the SSH connection closes.

`remote_preview.rs` keeps one registry shape for both platforms; only the
`TunnelInfo` payload differs (PID on desktop, `TunnelHandle` on Android).

---

## 4. App-private paths

Android has no home directory. `src-tauri/src/android_paths.rs` holds the
resolved app data directory in a process-global `OnceCell`, populated in
`.setup()` **before** any Android startup initialisation needs it (path helpers
without an `AppHandle` cannot call `app.path()` themselves).

Everything Android persists lives under it:

| File | Contents |
|---|---|
| `ssh-servers.json` | SSH server records (no passwords, only key paths) |
| `known_hosts` | trusted host keys |
| `keys/<name>` | imported SSH private keys, mode `0600` |
| `app-state` / logs | app state and log files |

Desktop keeps its existing locations (`~/CripCode/.cripcode/…`). The split is at
`get_config_path()` and friends.

---

## 5. The remote OpenCode session — how the pieces map

| Concept | Where it lives | Notes |
|---|---|---|
| **Current Project** | selected in CripCode | a selection, not a local clone |
| **Remote Project** | `commands/ssh/remote_projects.rs` | maps a project to `(server, remote path)`; persisted app-privately on Android |
| **Execution Target** | `ExecutionTarget` in the frontend, `ssh_server` in `pty_session_open` | local or remote |
| **SSH** | `commands/ssh/transport_russh.rs` | the channel |
| **Original Agent (OpenCode)** | `check_remote_agent_installed`, the agent terminal's PTY session | runs on the VPS |
| **Ollama** | `commands/ssh/ollama.rs` | model runtime on the VPS |
| **Git** | `commands/ssh/remote_git.rs` | version control in the remote workspace |

`lib.rs` registers `import_ssh_key`, the three `android_update_*` commands and
the two `update_diagnostics` commands under `#[cfg(target_os = "android")]`, so
they do not exist on desktop at all.

---

## 6. In-app update (Android only)

Tauri has no updater for Android, so `commands/android_update.rs` implements the
same user-confirmed flow against Android's own package installer:

1. Fetch a **fixed, trusted** metadata JSON (`android-latest.json`) from the
   project's package registry.
2. Compare the numeric `versionCode` with the running app's versionCode.
   Strictly-greater only — equal or lower never prompts.
3. Download the APK and **verify its SHA-256** before touching it.
4. Hand the file to the platform installer via an `ACTION_VIEW` Intent with
   `application/vnd.android.package-archive` and the existing `FileProvider`
   content URI.

Security properties:

- The APK URL must start with the hardcoded distribution prefix and end in
  `.apk`. The URL is never taken from user input — only from the trusted
  metadata document.
- The local `versionCode` comes from the Android build (forwarded by
  `build.rs` as `ANDROID_VERSION_CODE`), not from the display label.
- A network or metadata failure is **never** an error to the user: the frontend
  treats it as "no update", so an unreachable endpoint can never block startup.
- Silent installation is never attempted; no root and no ADB are needed. Android
  itself refuses any APK not signed with the same release key, and the user
  confirms in the OS installer UI.

Long downloads report progress over events — `android-update-progress`,
`android-update-complete`, `android-update-failed` — because a multi-minute
invoke can lose its response when the Android WebView IPC bridge breaks
mid-flight. The native task is the authoritative source of its own outcome.

`commands/update_diagnostics.rs` reads the local log's `CRIPCODE_UPDATE_DIAG`
lines and opens the Android share sheet. It is **local-only**: nothing is
uploaded; the user picks the destination.

---

## 7. Platform branches — the complete inventory

`#[cfg(target_os = "android")]` appears in the backend at these files:
`lib.rs`, `android_paths.rs` (module), `error_reporting.rs`, `logging.rs`,
`commands/android_update.rs`, `commands/update_diagnostics.rs`,
`commands/pty_session.rs`, `commands/setup/{status,state}.rs`,
`commands/projects/pins.rs`, `commands/ssh/{connection,config,exec,exit_tracker,files,host_key,key_import,mod,ollama,remote_agent,remote_preview,remote_projects,secrets,transport_russh}.rs`,
`commands/session_backend.rs` (doc comment).

On the frontend the discriminator is `isAndroid()` from `src/lib/setup.ts`.

---

## 8. Related documents

- [IMPLEMENTATION-HISTORY.md](IMPLEMENTATION-HISTORY.md)
- [TEST-RESULTS.md](TEST-RESULTS.md)
- [KNOWN-ISSUES.md](KNOWN-ISSUES.md)
- [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)
- [NEXT-STEPS.md](NEXT-STEPS.md)
- Desktop-side SSH design: [`docs/ssh-architecture.md`](../ssh-architecture.md)