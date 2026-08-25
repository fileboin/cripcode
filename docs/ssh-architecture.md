# SSH Architecture — Cripcode Remote Runtime

> **Phase 6 deliverable.** This document designs the SSH integration before any
> implementation. Phase 7 (SSH MVP) implements only the connection layer described
> here. Remote terminal, filesystem, git, etc. are later phases.

## Target Architecture

```
CRIPCODE
  │
  ├── Local Runtime (existing — unchanged)
  │   ├── Local PTY         (pty_session.rs / tauri-plugin-pty)
  │   ├── Local filesystem  (validate_project_path, commands/projects/)
  │   └── Local Git         (git CLI via run_with_timeout)
  │
  └── Remote Runtime (NEW)
      │
      ├── SSH Connection Layer          ← Phase 7 (SSH MVP)
      │   ├── Server config storage     (~/.ship-studio/ssh-servers.json)
      │   ├── Connection state registry  (in-memory, like state.rs)
      │   ├── Connection test            (ssh CLI via run_with_timeout)
      │   └── Credential storage        (keychain, like accounts.rs)
      │
      ├── Remote Terminal                ← Phase 8
      │   └── ssh user@host in PTY       (reuse pty_session.rs)
      │
      ├── Remote Filesystem              ← Phase 9
      │   └── sftp/scp CLI               (reuse run_with_timeout)
      │
      ├── Remote Workspace               ← Phase 10
      │   └── Local/Remote project switch
      │
      └── Remote Git                     ← Phase 11
          └── ssh user@host "git ..."    (reuse run_with_timeout)
```

## Design Principle: Reuse, Don't Rebuild

The existing codebase has three layers the SSH integration builds on:

1. **PTY layer** — `pty_session.rs` and the `tauri-plugin-pty` plugin already
   provide a real pseudo-terminal with resize, ring-buffer replay, and
   backend-owned lifecycle. An interactive SSH shell is just another command
   spawned in this PTY (`ssh user@host`), needing **zero new terminal code**.

2. **Command execution** — `external_command.rs::run_with_timeout()` already
   provides timeout-enforced, structured-error-mapped CLI invocation. SSH exec
   (`ssh user@host "command"`) and SFTP operations can use it directly.

3. **Credential storage** — `accounts.rs` already stores secrets in the
   macOS Keychain (via the `security` CLI) with per-account isolation. SSH key
   passphrases can use the same mechanism.

**No new Rust SSH library is needed for Phase 7.** The system `ssh` CLI
(OpenSSH, pre-installed on macOS/Linux and available on Windows 10+) handles
the entire SSH protocol. We add a library only if a later phase needs
something the CLI can't provide (e.g., progress-tracked SFTP → `russh`).

## Existing Code to Reuse

| Layer | File(s) | What it provides | SSH reuse |
|-------|---------|-------------------|-----------|
| PTY sessions | `commands/pty_session.rs` | Backend-owned PTY with ring buffer, resize, attach/detach | Spawn `ssh` as the PTY command → interactive remote shell |
| PTY plugin | `plugins/tauri-plugin-pty/src/lib.rs` | `portable_pty`-based PTY (spawn, write, read, resize, kill) | Alternative PTY backend for SSH sessions |
| One-shot PTY | `commands/pty/spawn.rs` | `spawn_pty` — command in a thread, stdout/stderr to frontend | SSH one-shot commands |
| Command exec | `external_command.rs` | `run_with_timeout` — timeout, retry, kill_on_drop, structured errors | `ssh user@host "cmd"` for connection test, git, etc. |
| Credential vault | `commands/accounts.rs` | Keychain write/read/delete via `security` CLI | SSH key passphrase storage |
| Settings | `commands/setup/state.rs` + `mod.rs` | `read_app_state` / `write_app_state` — JSON persistence | SSH server list in `AppState` or separate file |
| Path validation | `utils.rs::validate_project_path()` | Constrains paths to `~/ShipStudio` or registered roots | Validate SSH key paths |
| Command builder | `utils.rs::create_command()` | Windows: `CREATE_NO_WINDOW` | SSH CLI invocation without console popup |
| State registry | `state.rs` | In-memory `LazyLock<Mutex<HashMap>>` registries | SSH connection state registry |
| Error types | `errors.rs::CommandError` | Structured error variants (Timeout, Validation, etc.) | SSH command errors |
| Frontend IPC | `src/lib/*.ts` pattern | `invoke()` wrappers per domain | `src/lib/ssh.ts` wrapper |
| Modal UI | `src/components/primitives/ModalFrame.tsx` | Shared modal frame | Add/Edit server modals |
| Button UI | `src/components/primitives/Button.tsx` | Shared button variants | Connect/Disconnect/Test buttons |

## SSH Core Design

### Data Structures (Rust)

```rust
/// SSH server configuration. Stored on disk (JSON), never holds the key content.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SshServer {
    /// UUID generated at creation time.
    pub id: String,
    /// User-friendly label (e.g. "Production VPS", "Staging").
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// SSH port. Defaults to 22 if None.
    pub port: Option<u16>,
    /// Remote username.
    pub username: String,
    /// Absolute filesystem path to the private key (e.g. `~/.ssh/id_ed25519`).
    /// The key file itself is never read into memory — only its path is stored.
    pub key_path: Option<String>,
    /// Unix ms timestamp of creation.
    pub created_at: u64,
    /// Unix ms timestamp of the last successful connection (None = never).
    pub last_connected_at: Option<u64>,
}

/// Live connection state. In-memory only; never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SshConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// In-memory entry for a server's connection state.
struct SshConnectionInfo {
    state: SshConnectionState,
    server_id: String,
    connected_at: Option<u64>,
    error: Option<String>,
}
```

### Server Config Storage

SSH server configurations are persisted to a JSON file, following the same
pattern as `app_state.json`:

```
~/.ship-studio/ssh-servers.json
```

Structure:
```json
{
  "servers": [
    {
      "id": "a1b2c3d4-...",
      "name": "Production VPS",
      "host": "203.0.113.1",
      "port": 22,
      "username": "deploy",
      "keyPath": "/Users/me/.ssh/id_ed25519",
      "createdAt": 1724600000000,
      "lastConnectedAt": 1724700000000
    }
  ]
}
```

Rationale for a **separate file** (not inside `app_state.json`):
- Server configs are a self-contained domain — CRUD doesn't need to read/write
  the entire app state.
- The file can grow independently (many servers) without bloating app_state.
- Follows the pattern of `.shipstudio/project.json` (per-project metadata in
  the project folder) vs `app_state.json` (global settings).

### Connection State Registry

In-memory only, following the `state.rs` pattern:

```rust
use std::sync::{LazyLock, Mutex};
use std::collections::HashMap;

static SSH_CONNECTIONS: LazyLock<Mutex<HashMap<String, SshConnectionInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
```

The registry tracks live connection state. On app restart, all connections
start as `Disconnected` (no persistent connection state).

### Backend Commands (Rust)

New module: `src-tauri/src/commands/ssh/`

```
commands/ssh/
  mod.rs        — module root, re-exports
  config.rs     — server CRUD (list, add, update, delete)
  connection.rs — connection state, test, connect, disconnect
```

Commands (all return `Result<T, CommandError>` per the four rules):

| Command | Purpose |
|---------|---------|
| `list_ssh_servers` | Return all saved server configs |
| `add_ssh_server(name, host, port, username, key_path)` | Create + persist a new server, return it |
| `update_ssh_server(id, ...)` | Update an existing server |
| `delete_ssh_server(id)` | Remove a server config |
| `test_ssh_connection(id)` | Run `ssh -o BatchMode=yes -o ConnectTimeout=10 ... "echo ok"` via `run_with_timeout`. Returns "ok" or a `CommandError`. |
| `connect_ssh(id)` | Run a connection test; if successful, record state as `Connected` + timestamp. Does NOT open an interactive session — that's Phase 8. |
| `disconnect_ssh(id)` | Set state to `Disconnected`. |
| `get_ssh_connection_state(id)` | Return the current `SshConnectionState`. |

All commands use:
- `#[tracing::instrument]` (rule 4)
- `validate_project_path()` for key path validation (rule 2)
- `run_with_timeout` for SSH CLI invocation (rule 3)
- `Result<T, CommandError>` (rule 1)

### SSH Command Construction

The SSH CLI is invoked with these options for non-interactive (test/exec)
calls:

```bash
ssh \
  -o BatchMode=yes          \  # never prompt for password (fail instead)
  -o ConnectTimeout=10      \  # 10s connection timeout
  -o StrictHostKeyChecking=accept-new \  # auto-accept first connection, reject changes
  -p <port>                  \
  -i <key_path>              \  # private key (if configured)
  <username>@<host>          \
  "<command>"                   # remote command
```

For interactive terminal sessions (Phase 8), the same options are passed but
without a trailing command, spawned inside a PTY:

```bash
ssh \
  -o ConnectTimeout=10 \
  -o ServerAliveInterval=30 \  # keepalive
  -o ServerAliveCountMax=3  \  # disconnect after 3 missed keepalives
  -p <port> \
  -i <key_path> \
  <username>@<host>
```

### Frontend (TypeScript)

New file: `src/lib/ssh.ts` — follows the existing `src/lib/*.ts` pattern:

```typescript
import { invoke } from '@tauri-apps/api/core';

export interface SshServer { /* mirrors the Rust struct */ }
export type SshConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export function listSshServers(): Promise<SshServer[]> { ... }
export function addSshServer(config: NewSshServer): Promise<SshServer> { ... }
export function updateSshServer(id: string, config: Partial<SshServer>): Promise<SshServer> { ... }
export function deleteSshServer(id: string): Promise<void> { ... }
export function testSshConnection(id: string): Promise<string> { ... }
export function connectSsh(id: string): Promise<void> { ... }
export function disconnectSsh(id: string): Promise<void> { ... }
export function getSshConnectionState(id: string): Promise<SshConnectionState> { ... }
```

UI components (Phase 7):
- `AddServerModal` — uses `<ModalFrame>`, `<Button variant>`, `useAsyncState`
- `ServerList` — reusable list component
- `ServerCard` — per-server card with connect/disconnect/test actions
- Commands registered via `useCommands` in the Cmd+K palette

### Command Registration

Following the existing pattern, SSH commands are registered in `lib.rs`:

```rust
// src-tauri/src/lib.rs — in the invoke_handler macro
commands::ssh::list_ssh_servers,
commands::ssh::add_ssh_server,
commands::ssh::update_ssh_server,
commands::ssh::delete_ssh_server,
commands::ssh::test_ssh_connection,
commands::ssh::connect_ssh,
commands::ssh::disconnect_ssh,
commands::ssh::get_ssh_connection_state,
```

## Security

### Private Key

- **Never read into memory.** Only the filesystem path is stored in
  `ssh-servers.json`. The SSH CLI reads the key directly.
- **Path validation.** `key_path` must be an absolute path within the user's
  home directory (or an explicitly allowed location). This prevents a
  compromised webview from pointing SSH at arbitrary system files.
- **File permissions.** SSH itself enforces key file permissions (refuses
  keys readable by others). We rely on this.

### Key Passphrase

- If the private key has a passphrase, store it in the keychain following the
  `accounts.rs` pattern (`ship-studio-ssh-<server_id>` service name).
- Pass via `SSH_ASKPASS` or the `ssh-askpass` mechanism — **not** as a CLI
  argument (visible via `ps`).
- On Windows, use Windows Credential Manager (the `accounts.rs` keychain
  helpers are macOS-only via `security` CLI; Windows will need a parallel
  implementation — same debt as the existing accounts system).

### known_hosts

- Use the system's `~/.ssh/known_hosts` file. SSH CLI handles host key
  verification natively.
- First connection: `StrictHostKeyChecking=accept-new` auto-accepts and
  records the host key. Subsequent connections reject any change (MITM
  protection).
- The `known_hosts` file is never modified directly by Cripcode — SSH manages
  it.

### Connection Timeout

- `ConnectTimeout=10` — 10 seconds for the TCP + SSH handshake.
- `run_with_timeout` wraps the entire call with its own timeout (default 30s).
- No unbounded waits — a hung server can't lock the UI.

### Cancellation

- Test/exec commands: `run_with_timeout` with `kill_on_drop(true)` — dropping
  the future kills the SSH process.
- Interactive sessions (Phase 8): `pty_session_kill` terminates the PTY,
  which sends SIGHUP to the SSH process.

## Dependencies

### Phase 7 (SSH MVP) — NO new crates

| Dependency | Source | Purpose |
|------------|--------|---------|
| `ssh` CLI | System (pre-installed) | SSH protocol, connection, exec |
| `run_with_timeout` | `external_command.rs` | Timeout-enforced invocation |
| `portable_pty` | Existing Cargo dep | (Phase 8) PTY for interactive shell |
| `reqwest` | Existing Cargo dep | (Future) Ollama API calls over SSH tunnel |

### Future phases — conditional

| Crate | When | Why |
|-------|------|-----|
| `russh` | Phase 9 (Remote FS) if CLI SFTP is insufficient | Pure-Rust SFTP with progress callbacks, no system `sftp` dependency |
| `russh-sftp` | Phase 9 | SFTP client (if `russh` is chosen) |

Decision deferred until Phase 9 evaluates whether `sftp`/`scp` CLI suffices.

## Implementation Plan

### Phase 7 — SSH MVP (next phase)

1. **Types**: Add `SshServer` to `types.rs`
2. **Backend**: Create `commands/ssh/` module (config.rs, connection.rs)
3. **Storage**: Implement `ssh-servers.json` read/write
4. **Connection test**: `test_ssh_connection` via `ssh` CLI + `run_with_timeout`
5. **State registry**: `SSH_CONNECTIONS` in `commands/ssh/connection.rs`
6. **Command registration**: Add to `lib.rs` invoke_handler
7. **Frontend**: Create `src/lib/ssh.ts` wrapper
8. **UI**: `AddServerModal`, `ServerList`, `ServerCard` using existing primitives
9. **Commands**: Register SSH actions in Cmd+K palette via `useCommands`
10. **Tests**: Rust unit tests for config CRUD + connection state; frontend tests for the wrapper

### Phase 8 — Remote Terminal

- Spawn `ssh user@host` inside `pty_session_open` (reuse the existing
  backend-owned PTY)
- No new terminal UI — the existing `Terminal` component works unchanged
- Add a "remote" badge to the terminal tab when the session is SSH-based

### Phase 9 — Remote Filesystem

- Evaluate `sftp` CLI vs `russh` SFTP
- Implement a `FileProvider` abstraction: `LocalFileProvider` + `SshFileProvider`
- Editor opens files through the provider — no editor changes needed

### Phase 10 — Local/Remote Workspace

- Project metadata gains a `runtime: "local" | "remote"` field + `ssh_server_id`
- Dashboard shows local and remote projects
- Workspace switch is transparent — the editor/terminal/git all route through
  the active runtime

### Phase 11 — Remote Git

- `git` commands route through SSH when the project is remote
- `ssh user@host "cd /path && git status"` via `run_with_timeout`
- Existing Git UI works unchanged

## Platform Notes

- **macOS**: `ssh` pre-installed (OpenSSH). Keychain via `security` CLI.
- **Linux**: `ssh` pre-installed. Secret storage via `secret-service` (future).
- **Windows**: `ssh` available in Windows 10+ (OpenSSH Client optional feature).
  Keychain via Windows Credential Manager (future — same debt as accounts.rs).
