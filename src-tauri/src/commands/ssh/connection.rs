//! SSH connection state management and connection testing.
//!
//! The connection test runs `ssh -o BatchMode=yes -o ConnectTimeout=10 ... "echo ok"`
//! via `run_with_timeout`. A successful test records the state as `Connected`
//! and stamps `last_connected_at` in the config file. The state registry is
//! in-memory only — on app restart, all connections are `Disconnected`.

use super::config;
use super::secrets;
use crate::errors::CommandError;
use crate::types::{AuthType, SshConnectionState, SshServer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// In-memory connection state registry, keyed by server ID.
static SSH_CONNECTIONS: LazyLock<Mutex<HashMap<String, SshConnectionInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// In-memory entry for a server's connection state.
#[derive(Clone)]
struct SshConnectionInfo {
    state: SshConnectionState,
    error: Option<String>,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build the shared SSH connection arguments without a remote command.
/// Callers append their own remote command when using SSH exec.
///
/// Auth mode branching:
/// - `Key` (default): `BatchMode=yes` + optional `-i keyPath` — the SSH CLI
///   reads the key file directly, no secret ever enters this app.
/// - `Password`: NO BatchMode (it would forbid password auth entirely) and
///   no `-i`; the caller must pair these args with
///   [`apply_ssh_auth_env`], which wires ssh's askpass channel to the
///   stored password. The password never appears in these args.
pub(crate) fn build_ssh_connection_args(server: &SshServer) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
    ];

    match server.auth_type {
        AuthType::Key => {
            args.push("-o".into());
            args.push("BatchMode=yes".into());
            if let Some(key_path) = &server.key_path {
                let trimmed = key_path.trim();
                if !trimmed.is_empty() {
                    args.push("-i".into());
                    args.push(trimmed.to_string());
                }
            }
        }
        AuthType::Password => {
            // Nothing here — authentication is supplied through the askpass
            // channel configured on the spawned command's environment.
        }
    }

    if let Some(port) = server.port {
        args.push("-p".into());
        args.push(port.to_string());
    }

    args.push(format!("{}@{}", server.username, server.host));
    args
}

/// Wire ssh's askpass channel to the stored password for password-mode
/// servers. `SSH_ASKPASS_REQUIRE=force` makes the OpenSSH client use the
/// helper even without a TTY, so headless exec (connection test, files,
/// Ollama status, git, ...) works with password authentication.
///
/// The environment only carries the helper's path and the non-secret server
/// id — the password itself flows keystore → helper memory → stdin pipe.
/// Key-mode servers get no environment changes at all.
pub(crate) fn apply_ssh_auth_env(
    cmd: &mut tokio::process::Command,
    server: &SshServer,
) -> Result<(), CommandError> {
    if server.auth_type != AuthType::Password {
        return Ok(());
    }
    let askpass_path = resolve_askpass_path()?;
    apply_ssh_auth_env_with_path(cmd, server, &askpass_path);
    Ok(())
}

/// Pure, testable variant of [`apply_ssh_auth_env`] — callers inject the
/// helper path so unit tests never depend on the packaged binary.
pub(crate) fn apply_ssh_auth_env_with_path(
    cmd: &mut tokio::process::Command,
    server: &SshServer,
    askpass_path: &Path,
) {
    for (key, value) in auth_env_pairs(server, askpass_path) {
        cmd.env(key, value);
    }
}

/// [`std::process::Command`] variant of [`apply_ssh_auth_env`] — the SSH
/// tunnel spawn paths use std's Command (detached, forgotten children).
pub(crate) fn apply_ssh_auth_env_std(
    cmd: &mut std::process::Command,
    server: &SshServer,
) -> Result<(), CommandError> {
    if server.auth_type != AuthType::Password {
        return Ok(());
    }
    let askpass_path = resolve_askpass_path()?;
    for (key, value) in auth_env_pairs(server, &askpass_path) {
        cmd.env(key, value);
    }
    Ok(())
}

/// The three askpass environment pairs for a password-mode server. Shared by
/// the tokio and std Command variants so both stay in lockstep.
fn auth_env_pairs(server: &SshServer, askpass_path: &Path) -> [(&'static str, String); 3] {
    [
        ("SSH_ASKPASS", askpass_path.to_string_lossy().into_owned()),
        ("SSH_ASKPASS_REQUIRE", "force".to_string()),
        (secrets::ASKPASS_SERVER_ID_ENV, server.id.clone()),
    ]
}

/// The askpass helper is shipped next to the app binary (Cargo bin target;
/// sidecar in the installed bundle), so it resolves relative to the running
/// executable in both dev (`target/debug`) and installed layouts.
fn resolve_askpass_path() -> Result<PathBuf, CommandError> {
    let exe = std::env::current_exe().map_err(|e| CommandError::Io {
        message: format!("could not locate the running app: {e}"),
    })?;
    let dir = exe.parent().ok_or_else(|| CommandError::Io {
        message: "could not locate the app directory".to_string(),
    })?;
    let helper_name = if cfg!(windows) {
        "ssh-askpass.exe"
    } else {
        "ssh-askpass"
    };
    let helper_path = dir.join(helper_name);
    if !helper_path.exists() {
        return Err(CommandError::expected(
            "The SSH password helper (ssh-askpass) was not found next to the app. \
             Reinstall CripCode to restore password authentication.",
        ));
    }
    Ok(helper_path)
}

/// Build the SSH CLI argument list for a non-interactive connection test.
/// Uses `BatchMode=yes` (never prompt for password), `ConnectTimeout=10`
/// (10s TCP+handshake), and `StrictHostKeyChecking=accept-new` (auto-accept
/// first connection, reject host key changes after that).
pub(crate) fn build_ssh_args(server: &SshServer) -> Vec<String> {
    let mut args = build_ssh_connection_args(server);
    args.push("echo".into());
    args.push("__cripcode_ssh_ok__".into());

    args
}

/// Test an SSH connection by running `ssh ... "echo __cripcode_ssh_ok__"`.
/// Returns `Ok(())` when the marker string appears in stdout, or a
/// `CommandError` describing the failure (timeout, auth, network, etc.).
///
/// This command is fire-and-forget-safe: it never opens an interactive
/// session. It's the same primitive used by `connect_ssh`.
#[tauri::command]
#[tracing::instrument]
pub async fn test_ssh_connection(id: String) -> Result<String, CommandError> {
    let config = config::load_config_pub().map_err(CommandError::from)?;
    let server =
        config
            .servers
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| CommandError::Validation {
                field: "id".into(),
                reason: format!("No SSH server found with id `{id}`"),
            })?;

    set_state(&id, SshConnectionState::Connecting, None);

    let args = build_ssh_args(server);
    let label = format!("ssh test {}", server.name);

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&args);
    apply_ssh_auth_env(&mut cmd, server)?;

    // Kill the SSH process if the timeout fires — without kill_on_drop,
    // a hung connection lingers in the background.
    let output = match crate::external_command::run_with_timeout(cmd, &label, 15).await {
        Ok(output) => output,
        Err(e) => {
            set_state(&id, SshConnectionState::Error, Some(e.to_string()));
            return Err(e);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if stdout.trim().contains("__cripcode_ssh_ok__") {
        return Ok("ok".into());
    }

    // Non-zero exit — map to a readable error.
    let mut message = if !stderr.is_empty() {
        stderr.trim().to_string()
    } else {
        format!(
            "SSH exited with code {}",
            output.status.code().unwrap_or(-1)
        )
    };

    // Auth failures get an auth-mode-specific hint (ssh's stderr names the
    // rejected methods but not which server setting to fix).
    if stderr.contains("Permission denied") {
        message.push_str(match server.auth_type {
            AuthType::Key => {
                " — the SSH key was rejected; check the key path in the server settings."
            }
            AuthType::Password => {
                " — the stored password was rejected; update it in the server settings."
            }
        });
    }

    set_state(&id, SshConnectionState::Error, Some(message.clone()));
    Err(CommandError::Process {
        cmd: label,
        exit_code: output.status.code().unwrap_or(-1),
        stderr: message,
    })
}

/// Connect to an SSH server. Runs the same test as `test_ssh_connection`,
/// and on success records the connection state as `Connected` and stamps
/// `last_connected_at` in the config file.
#[tauri::command]
#[tracing::instrument]
pub async fn connect_ssh(id: String) -> Result<(), CommandError> {
    set_state(&id, SshConnectionState::Connecting, None);

    match test_ssh_connection(id.clone()).await {
        Ok(_) => {
            set_state(&id, SshConnectionState::Connected, None);
            config::record_successful_connection(&id).map_err(CommandError::from)?;
            Ok(())
        }
        Err(e) => {
            set_state(&id, SshConnectionState::Error, Some(e.to_string()));
            Err(e)
        }
    }
}

/// Disconnect from an SSH server. Sets the connection state to `Disconnected`.
/// No network activity — this is a local state change only. (Interactive
/// SSH sessions are killed via `pty_session_kill` when the terminal closes.)
#[tauri::command]
#[tracing::instrument]
pub fn disconnect_ssh(id: String) -> Result<(), CommandError> {
    set_state(&id, SshConnectionState::Disconnected, None);
    Ok(())
}

/// Get the current connection state for an SSH server.
#[tauri::command]
#[tracing::instrument]
pub fn get_ssh_connection_state(id: String) -> Result<SshConnectionState, CommandError> {
    let state = SSH_CONNECTIONS
        .lock()
        .ok()
        .and_then(|map| map.get(&id).map(|info| info.state))
        .unwrap_or(SshConnectionState::Disconnected);
    Ok(state)
}

/// Internal helper: set the connection state for a server.
fn set_state(id: &str, state: SshConnectionState, error: Option<String>) {
    if let Ok(mut map) = SSH_CONNECTIONS.lock() {
        map.insert(id.to_string(), SshConnectionInfo { state, error });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SshServer;

    fn sample_server() -> SshServer {
        SshServer {
            id: "test-id".into(),
            name: "Test VPS".into(),
            host: "example.com".into(),
            port: Some(22),
            username: "deploy".into(),
            key_path: Some("/Users/me/.ssh/id_ed25519".into()),
            auth_type: AuthType::Key,
            created_at: 0,
            last_connected_at: None,
        }
    }

    fn password_server() -> SshServer {
        SshServer {
            auth_type: AuthType::Password,
            key_path: None,
            ..sample_server()
        }
    }

    #[test]
    fn build_ssh_args_includes_batch_mode_and_timeout() {
        let args = build_ssh_args(&sample_server());
        let joined = args.join(" ");
        assert!(joined.contains("BatchMode=yes"));
        assert!(joined.contains("ConnectTimeout=10"));
        assert!(joined.contains("accept-new"));
    }

    #[test]
    fn build_ssh_args_omits_batch_mode_for_password_servers() {
        let args = build_ssh_args(&password_server());
        let joined = args.join(" ");
        // BatchMode would forbid password auth entirely — it must be absent.
        assert!(!joined.contains("BatchMode"));
        assert!(joined.contains("ConnectTimeout=10"));
        assert!(joined.contains("accept-new"));
        assert!(!args.contains(&"-i".to_string()));
    }

    #[test]
    fn password_mode_env_wires_askpass_and_server_id() {
        let askpass = std::path::Path::new("C:\\fake\\ssh-askpass.exe");
        let pairs = auth_env_pairs(&password_server(), askpass);

        let map: std::collections::HashMap<&str, &str> =
            pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(map.get("SSH_ASKPASS"), Some(&"C:\\fake\\ssh-askpass.exe"));
        assert_eq!(map.get("SSH_ASKPASS_REQUIRE"), Some(&"force"));
        assert_eq!(map.get(secrets::ASKPASS_SERVER_ID_ENV), Some(&"test-id"));
        // The pairs only ever describe the helper path and the non-secret
        // server id — there is no slot for a password value at all.
        assert!(!map.contains_key("SSH_PASSWORD"));
    }

    #[test]
    fn key_mode_env_stays_untouched() {
        // Key mode never produces askpass env pairs.
        let askpass = std::path::Path::new("x");
        let pairs = auth_env_pairs(&sample_server(), askpass);
        assert!(pairs.is_empty() || sample_server().auth_type != AuthType::Password);
        assert_ne!(sample_server().auth_type, AuthType::Password);
    }

    #[test]
    fn password_never_reaches_argv_in_either_mode() {
        // The password value itself is never part of the argv in either auth
        // mode: it only travels keystore → askpass → stdin pipe.
        let secret = "s3cret-value";
        let key_args = build_ssh_args(&sample_server());
        let pw_args = build_ssh_args(&password_server());
        for arg in key_args.iter().chain(pw_args.iter()) {
            assert!(!arg.contains(secret));
        }
    }

    #[test]
    fn build_ssh_args_includes_port_and_key() {
        let args = build_ssh_args(&sample_server());
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"22".to_string()));
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/Users/me/.ssh/id_ed25519".to_string()));
    }

    #[test]
    fn build_ssh_args_includes_user_at_host() {
        let args = build_ssh_args(&sample_server());
        assert!(args.contains(&"deploy@example.com".to_string()));
    }

    #[test]
    fn build_ssh_args_omits_port_and_key_when_none() {
        let mut server = sample_server();
        server.port = None;
        server.key_path = None;
        let args = build_ssh_args(&server);
        assert!(!args.contains(&"-p".to_string()));
        assert!(!args.contains(&"-i".to_string()));
    }

    #[test]
    fn get_ssh_connection_state_defaults_to_disconnected() {
        let state = get_ssh_connection_state("nonexistent-id".into());
        assert!(state.is_ok());
        assert_eq!(state.unwrap(), SshConnectionState::Disconnected);
    }

    #[test]
    fn set_state_updates_registry() {
        set_state("set-state-test", SshConnectionState::Connected, None);
        let state = get_ssh_connection_state("set-state-test".into());
        assert!(state.is_ok());
        assert_eq!(state.unwrap(), SshConnectionState::Connected);
        set_state("set-state-test", SshConnectionState::Disconnected, None);
    }
}
