//! SSH connection state management and connection testing.
//!
//! The connection test runs `ssh -o BatchMode=yes -o ConnectTimeout=10 ... "echo ok"`
//! via `run_with_timeout`. A successful test records the state as `Connected`
//! and stamps `last_connected_at` in the config file. The state registry is
//! in-memory only — on app restart, all connections are `Disconnected`.

use super::config;
use crate::errors::CommandError;
use crate::types::{SshConnectionState, SshServer};
use std::collections::HashMap;
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

/// Build the SSH CLI argument list for a non-interactive connection test.
/// Uses `BatchMode=yes` (never prompt for password), `ConnectTimeout=10`
/// (10s TCP+handshake), and `StrictHostKeyChecking=accept-new` (auto-accept
/// first connection, reject host key changes after that).
pub(crate) fn build_ssh_args(server: &SshServer) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
    ];

    if let Some(port) = server.port {
        args.push("-p".into());
        args.push(port.to_string());
    }

    if let Some(key_path) = &server.key_path {
        let trimmed = key_path.trim();
        if !trimmed.is_empty() {
            args.push("-i".into());
            args.push(trimmed.to_string());
        }
    }

    args.push(format!("{}@{}", server.username, server.host));
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
    let message = if !stderr.is_empty() {
        stderr.trim().to_string()
    } else {
        format!(
            "SSH exited with code {}",
            output.status.code().unwrap_or(-1)
        )
    };

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
            created_at: 0,
            last_connected_at: None,
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
