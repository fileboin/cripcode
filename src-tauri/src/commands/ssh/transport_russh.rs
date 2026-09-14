//! Android SSH transport — the platform adapter behind the existing
//! `SessionBackend` seam.
//!
//! Android has no OpenSSH binary and cannot exec one from app-private storage
//! (W^X/SELinux), so remote sessions use this in-process `russh` client.
//! Desktop keeps the OpenSSH CLI process transport unchanged; nothing here is
//! compiled on desktop.
//!
//! Security parity with the desktop CLI path:
//! - host keys are verified against an app-private `known_hosts` with the
//!   exact same explicit-gate policy: unknown → the existing approval flow,
//!   changed → BLOCK, accepted → connect. There is no `accept-new`.
//! - fingerprints are the OpenSSH `SHA256:<base64>` form.
//! - password auth is not implemented on Android (the OS keystore has no
//!   backend there); password servers fail closed instead of falling back to
//!   plaintext storage. Key auth only.

use super::host_key::HostKeyStatus;
use crate::commands::session_backend::SessionBackend;
use crate::errors::CommandError;
use crate::types::{AuthType, SshServer};
use russh::client::{self, Config, Handle};
use russh::keys::ssh_key::{HashAlg, PublicKey};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// TCP + handshake budget, mirroring the desktop `ConnectTimeout=10` (with a
/// little headroom for slower mobile networks).
const CONNECT_TIMEOUT_SECS: u64 = 15;
/// Probe budget for the host-key capture connection.
const HOST_KEY_PROBE_TIMEOUT_SECS: u64 = 15;

fn client_config() -> Config {
    Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        // Mirrors the desktop `ServerAliveInterval=30` / `ServerAliveCountMax=3`.
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    }
}

fn address(server: &SshServer) -> (String, u16) {
    (server.host.clone(), server.port.unwrap_or(22))
}

/// App-private `known_hosts` — never the desktop `~/.ssh/known_hosts`.
fn known_hosts_path() -> Result<PathBuf, CommandError> {
    crate::android_paths::app_data_dir()
        .map(|d| d.join("known_hosts"))
        .ok_or_else(|| CommandError::expected("Android app data directory is not available yet."))
}

/// OpenSSH `SHA256:<base64>` fingerprint — identical to `ssh-keygen -lf`.
fn fingerprint(key: &PublicKey) -> Option<String> {
    Some(key.fingerprint(HashAlg::Sha256).to_string())
}

/// The base64 key blob (second token of an OpenSSH public-key line), which is
/// what `known_hosts` stores and compares.
fn public_key_blob(key: &PublicKey) -> Option<String> {
    key.to_openssh()
        .ok()?
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
}

/// Keep only `known_hosts` entries for this host reference, so a blob that
/// belongs to a different host can never satisfy the comparison. Mirrors the
/// desktop `ssh-keygen -F <hostref>` pre-filter.
fn known_hosts_lines_for_hostref(content: &str, hostref: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return false;
            }
            let mut tokens = line.split_whitespace();
            let first = tokens.next().unwrap_or_default();
            let host = if first.starts_with('@') {
                tokens.next().unwrap_or_default()
            } else {
                first
            };
            host == hostref
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the base64 blob from one `known_hosts` line (marker-aware).
fn known_hosts_line_blob(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut tokens = line.split_whitespace();
    let first = tokens.next()?;
    if first.starts_with('@') {
        tokens.next();
    }
    let _key_type = tokens.next()?;
    tokens.next().map(str::to_string)
}

fn allowed_blobs(server: &SshServer) -> Result<Vec<String>, CommandError> {
    let content = std::fs::read_to_string(known_hosts_path()?).unwrap_or_default();
    let hostref = super::host_key::known_hosts_hostref(&server.host, server.port);
    Ok(known_hosts_lines_for_hostref(&content, &hostref)
        .lines()
        .filter_map(known_hosts_line_blob)
        .collect())
}

/// Probe handler: captures the offered key during KEX and then aborts — the
/// probe NEVER authenticates and never mutates `known_hosts`. This mirrors the
/// desktop unauthenticated capture connection.
struct ProbeHandler {
    captured: Arc<Mutex<Option<PublicKey>>>,
}

impl client::Handler for ProbeHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        if let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key {
            if let Ok(mut slot) = self.captured.lock() {
                *slot = Some(key.clone());
            }
        }
        Ok(false)
    }
}

/// Connect handler: accepts ONLY a key blob already recorded in the
/// app-private `known_hosts` for this host. Unknown or changed keys fail
/// closed at KEX — the explicit approval flow is the only way in.
struct StrictHandler {
    allowed: Vec<String>,
}

impl client::Handler for StrictHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            return Ok(false);
        };
        Ok(public_key_blob(key)
            .map(|blob| self.allowed.iter().any(|allowed| allowed == &blob))
            .unwrap_or(false))
    }
}

/// Capture the host's public key without authenticating. `None` means the
/// probe could not complete (unreachable host, timeout) — callers fail closed.
async fn capture_host_key(server: &SshServer) -> Result<Option<PublicKey>, CommandError> {
    let captured = Arc::new(Mutex::new(None));
    let handler = ProbeHandler {
        captured: captured.clone(),
    };
    let connect = client::connect(Arc::new(client_config()), address(server), handler);
    // The handler rejects the key, so `connect` resolves with an error right
    // after KEX; the captured key is the result. A timeout means no capture.
    let _ = tokio::time::timeout(Duration::from_secs(HOST_KEY_PROBE_TIMEOUT_SECS), connect).await;
    Ok(captured.lock().ok().and_then(|slot| slot.clone()))
}

/// Android replacement for `check_remote_host_key`: same states, same
/// fingerprint format, same fail-closed fallbacks as the desktop CLI probe.
pub async fn check_remote_host_key(server_id: &str) -> Result<HostKeyStatus, CommandError> {
    let server = super::files::get_server(server_id)?;
    let hostref = super::host_key::known_hosts_hostref(&server.host, server.port);
    let known_content = std::fs::read_to_string(known_hosts_path()?).unwrap_or_default();
    let filtered = known_hosts_lines_for_hostref(&known_content, &hostref);

    let Some(key) = capture_host_key(&server).await? else {
        return Ok(HostKeyStatus {
            state: super::host_key::fallback_state_without_probe(&filtered).into(),
            fingerprint: None,
            key_type: None,
        });
    };

    let key_type = key.algorithm().as_str().to_string();
    let blob = public_key_blob(&key).unwrap_or_default();
    let scan = vec![(hostref, key_type.clone(), blob)];
    let (state, evaluated_key_type) = super::host_key::evaluate_host_key_state(&scan, &filtered);

    Ok(HostKeyStatus {
        state: state.into(),
        fingerprint: fingerprint(&key),
        key_type: evaluated_key_type,
    })
}

/// Append the confirmed key to the app-private `known_hosts` — the Android
/// counterpart of the desktop `accept_remote_host_key`.
pub async fn accept_remote_host_key(server_id: &str) -> Result<(), CommandError> {
    let server = super::files::get_server(server_id)?;
    let Some(key) = capture_host_key(&server).await? else {
        return Err(CommandError::expected(format!(
            "Could not retrieve the host key for {} — host unreachable?",
            server.name
        )));
    };

    let hostref = super::host_key::known_hosts_hostref(&server.host, server.port);
    let openssh = key
        .to_openssh()
        .map_err(|e| CommandError::expected(format!("Could not encode the host key: {e}")))?;
    let mut tokens = openssh.split_whitespace();
    let key_type = tokens
        .next()
        .ok_or_else(|| CommandError::expected("Could not encode the host key"))?;
    let blob = tokens
        .next()
        .ok_or_else(|| CommandError::expected("Could not encode the host key"))?;

    let path = known_hosts_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CommandError::Io {
            message: format!("Failed to create the app data directory: {e}"),
        })?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| CommandError::Io {
            message: format!("Failed to open known_hosts: {e}"),
        })?;
    file.write_all(format!("{hostref} {key_type} {blob}\n").as_bytes())
        .map_err(|e| CommandError::Io {
            message: format!("Failed to update known_hosts: {e}"),
        })?;
    Ok(())
}

/// Connect + authenticate with the stored key. Password-mode servers fail
/// closed: Android has no OS keystore backend, and CripCode never stores a
/// password in plaintext.
async fn connect_authenticated(server: &SshServer) -> Result<Handle<StrictHandler>, CommandError> {
    if server.auth_type != AuthType::Key {
        return Err(CommandError::expected(
            "Password authentication is not available on this platform yet — use an SSH key.",
        ));
    }
    let key_path = server
        .key_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| CommandError::Validation {
            field: "keyPath".into(),
            reason: "An SSH private key path is required on this platform".into(),
        })?;
    let private_key = load_secret_key(key_path, None)
        .map_err(|e| CommandError::expected(format!("Could not load the SSH key: {e}")))?;

    let handler = StrictHandler {
        allowed: allowed_blobs(server)?,
    };
    let connect = client::connect(Arc::new(client_config()), address(server), handler);
    let mut handle = tokio::time::timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS), connect)
        .await
        .map_err(|_| CommandError::Timeout {
            cmd: format!("ssh {}", server.name),
            secs: CONNECT_TIMEOUT_SECS,
        })?
        .map_err(|e| CommandError::expected(format!("SSH connection failed: {e}")))?;

    let auth = handle
        .authenticate_publickey(
            server.username.clone(),
            PrivateKeyWithHashAlg::new(Arc::new(private_key), Some(HashAlg::Sha256)),
        )
        .await
        .map_err(|e| CommandError::expected(format!("SSH authentication failed: {e}")))?;
    if !auth.success() {
        return Err(CommandError::expected(
            "SSH authentication failed: the server rejected the key.",
        ));
    }
    Ok(handle)
}

/// Android replacement for the connection test: `echo __cripcode_ssh_ok__`
/// over a real authenticated session, same marker contract as desktop.
pub async fn test_connection(server: &SshServer) -> Result<String, CommandError> {
    let handle = connect_authenticated(server).await?;
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| CommandError::expected(format!("SSH channel failed: {e}")))?;
    channel
        .exec(true, "echo __cripcode_ssh_ok__")
        .await
        .map_err(|e| CommandError::expected(format!("SSH exec failed: {e}")))?;

    let mut output = Vec::new();
    let mut exit_code = -1;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => output.extend_from_slice(&data),
            ChannelMsg::ExtendedData { data, .. } => output.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => exit_code = exit_status as i32,
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }

    let text = String::from_utf8_lossy(&output);
    if text.contains("__cripcode_ssh_ok__") {
        return Ok("ok".into());
    }
    Err(CommandError::Process {
        cmd: format!("ssh test {}", server.name),
        exit_code,
        stderr: text.trim().to_string(),
    })
}

/// Commands from the sync Tauri surface to the async channel task.
enum BackendCmd {
    Data(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    Close,
}

/// Blocking reader adapter: the async pump feeds chunks over a std channel;
/// the existing reader thread keeps its plain blocking `read()` contract.
struct ChannelReader {
    rx: std_mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    offset: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.pending.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.pending = chunk;
                    self.offset = 0;
                }
                // All senders dropped — the session ended.
                Err(_) => return Ok(0),
            }
        }
        let n = (self.pending.len() - self.offset).min(buf.len());
        buf[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

/// Android PTY backend: an interactive shell running on the VPS over a russh
/// channel. Implements the same seam as the desktop process PTY.
pub struct RusshBackend {
    tx: UnboundedSender<BackendCmd>,
    reader: Mutex<ChannelReader>,
    exit_rx: Mutex<std_mpsc::Receiver<i32>>,
}

impl SessionBackend for RusshBackend {
    fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader
            .lock()
            .map_err(|e| std::io::Error::other(format!("reader lock poisoned: {e}")))?
            .read(buf)
    }

    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.tx.send(BackendCmd::Data(data.to_vec())).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "SSH session has ended")
        })
    }

    fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        // Async delivery: a failure here means the session already ended,
        // which the alive/exit state reports anyway.
        let _ = self.tx.send(BackendCmd::Resize { rows, cols });
        Ok(())
    }

    fn kill(&self) {
        let _ = self.tx.send(BackendCmd::Close);
    }

    fn wait(&self) -> i32 {
        self.exit_rx
            .lock()
            .map(|rx| rx.recv().unwrap_or(-1))
            .unwrap_or(-1)
    }

    fn pid(&self) -> u32 {
        // Channel-backed session: no local process to signal.
        0
    }
}

/// Open an interactive remote shell (PTY request + exec of `remote_command`)
/// and return it as a `SessionBackend`. `remote_command` is the same quoted
/// shell program the desktop `ssh -t` path runs.
pub async fn open_shell(
    server: &SshServer,
    cols: u16,
    rows: u16,
    remote_command: &str,
) -> Result<Arc<dyn SessionBackend>, CommandError> {
    let handle = connect_authenticated(server).await?;
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| CommandError::expected(format!("SSH channel failed: {e}")))?;

    // TERM must be explicit: OpenSSH inherits it from the local environment,
    // russh has no local environment to inherit. OpenCode needs a capable
    // terminal for its TUI.
    channel
        .request_pty(true, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
        .await
        .map_err(|e| CommandError::expected(format!("SSH PTY request failed: {e}")))?;
    channel
        .exec(true, remote_command.as_bytes().to_vec())
        .await
        .map_err(|e| CommandError::expected(format!("SSH exec failed: {e}")))?;

    let (cmd_tx, cmd_rx) = unbounded_channel();
    let (data_tx, data_rx) = std_mpsc::channel::<Vec<u8>>();
    let (exit_tx, exit_rx) = std_mpsc::channel::<i32>();

    spawn_pump(handle, channel, cmd_rx, data_tx, exit_tx);

    Ok(Arc::new(RusshBackend {
        tx: cmd_tx,
        reader: Mutex::new(ChannelReader {
            rx: data_rx,
            pending: Vec::new(),
            offset: 0,
        }),
        exit_rx: Mutex::new(exit_rx),
    }))
}

/// The async pump: owns the connection + channel, multiplexes terminal input,
/// resize, and channel output. No `block_on` anywhere — the sync side only
/// pushes into an unbounded queue.
fn spawn_pump(
    handle: Handle<StrictHandler>,
    mut channel: russh::Channel<russh::client::Msg>,
    mut cmd_rx: UnboundedReceiver<BackendCmd>,
    data_tx: std_mpsc::Sender<Vec<u8>>,
    exit_tx: std_mpsc::Sender<i32>,
) {
    tauri::async_runtime::spawn(async move {
        let mut exit_code: i32 = -1;
        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => match cmd {
                    Some(BackendCmd::Data(bytes)) => {
                        if channel.data_bytes(bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(BackendCmd::Resize { rows, cols }) => {
                        let _ = channel
                            .window_change(cols as u32, rows as u32, 0, 0)
                            .await;
                    }
                    // Close requested (kill) or all senders dropped.
                    Some(BackendCmd::Close) | None => {
                        let _ = channel.close().await;
                        break;
                    }
                },
                msg = channel.wait() => match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if data_tx.send(data.to_vec()).is_err() {
                            break;
                        }
                    }
                    // Without a PTY stderr arrives as extended data; with the
                    // PTY request it is merged into Data. Forward either way.
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = data_tx.send(data.to_vec());
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(ChannelMsg::ExitSignal { .. }) => {
                        exit_code = -1;
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                    _ => {}
                },
            }
        }
        let _ = handle.disconnect(Disconnect::ByApplication, "", "").await;
        // Dropping the sender is the reader's EOF.
        drop(data_tx);
        let _ = exit_tx.send(exit_code);
    });
}
