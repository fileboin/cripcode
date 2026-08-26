//! Remote agent detection over SSH.
//!
//! Checks whether an AI agent CLI (Claude Code, Codex, OpenCode) is installed
//! on a remote VPS by running `which <binary>` via SSH. The agent runs ON the
//! VPS — the local machine only needs SSH access, not the agent CLI.

use super::config;
use super::connection::build_ssh_args;
use crate::errors::CommandError;
use serde::Serialize;

/// Timeout for the agent detection check.
const SSH_AGENT_TIMEOUT_SECS: u64 = 10;

/// Look up a server by ID.
fn get_server(server_id: &str) -> Result<crate::types::SshServer, CommandError> {
    let config = config::load_config_pub().map_err(CommandError::from)?;
    config
        .servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| CommandError::Validation {
            field: "server_id".into(),
            reason: format!("No SSH server found with id `{server_id}`"),
        })
}

/// Agent installation status on a remote VPS.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentStatus {
    /// Whether the agent CLI is installed on the VPS.
    pub installed: bool,
    /// The path to the agent binary on the VPS (if found).
    pub path: Option<String>,
    /// The agent binary name that was checked.
    pub binary_name: String,
    /// Error message if the check failed.
    pub error: Option<String>,
}

/// Check if an agent CLI is installed on a remote VPS.
///
/// Runs `which <binary>` on the VPS via SSH. Returns the binary path if found,
/// or `installed: false` if the agent isn't installed.
#[tauri::command]
#[tracing::instrument]
pub async fn check_remote_agent_installed(
    server_id: String,
    binary_name: String,
) -> Result<RemoteAgentStatus, CommandError> {
    let server = get_server(&server_id)?;

    let binary_trimmed = binary_name.trim();
    if binary_trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "binary_name".into(),
            reason: "Binary name must not be empty".into(),
        });
    }

    let args = build_ssh_args(&server);
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&args).arg(format!(
        "which {} 2>/dev/null || echo '__NOT_FOUND__'",
        binary_trimmed
    ));

    let label = format!("ssh check-agent {}", server.name);
    let output =
        crate::external_command::run_with_timeout(cmd, &label, SSH_AGENT_TIMEOUT_SECS).await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed.contains("__NOT_FOUND__") || trimmed.is_empty() {
        return Ok(RemoteAgentStatus {
            installed: false,
            path: None,
            binary_name: binary_trimmed.to_string(),
            error: Some(format!(
                "Agent '{}' not found on {}",
                binary_trimmed, server.name
            )),
        });
    }

    Ok(RemoteAgentStatus {
        installed: true,
        path: Some(trimmed.to_string()),
        binary_name: binary_trimmed.to_string(),
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_agent_status_serializes_camel_case() {
        let status = RemoteAgentStatus {
            installed: true,
            path: Some("/usr/local/bin/claude".into()),
            binary_name: "claude".into(),
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"installed\":true"));
        assert!(json.contains("\"path\":\"/usr/local/bin/claude\""));
        assert!(json.contains("\"binaryName\":\"claude\""));
    }

    #[test]
    fn remote_agent_status_not_installed() {
        let status = RemoteAgentStatus {
            installed: false,
            path: None,
            binary_name: "codex".into(),
            error: Some("Agent 'codex' not found on VPS".into()),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"installed\":false"));
        assert!(json.contains("\"path\":null"));
    }
}
