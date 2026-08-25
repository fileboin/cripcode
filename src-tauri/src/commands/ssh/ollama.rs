//! Ollama connection and model discovery.
//!
//! Ollama exposes a REST API on port 11434 by default. For local detection,
//! we hit `http://localhost:11434/api/version` and `/api/tags` directly via
//! `reqwest`. For remote Ollama (on a VPS), we SSH exec
//! `curl http://localhost:11434/api/...` on the VPS — the Ollama port is
//! typically bound to localhost only, so we can't reach it without a tunnel,
//! but `curl` from inside the VPS works.
//!
//! Phase 14 MVP: detect + list models. Model selection (Phase 16) and AI
//! provider abstraction (Phase 17) build on this.

use crate::errors::CommandError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default Ollama API port.
const OLLAMA_PORT: u16 = 11434;

/// Timeout for Ollama API requests (local and remote).
const OLLAMA_TIMEOUT_SECS: u64 = 10;

/// Ollama version info from `/api/version`.
#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaVersion {
    pub version: String,
}

/// A single model from Ollama's `/api/tags` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub name: String,
    pub model: String,
    pub size: u64,
    /// Quantization level (e.g. "q4_K_M").
    #[serde(rename = "quantization_level")]
    pub quantization: Option<String>,
    /// Human-readable size string (e.g. "4.7 GB").
    pub details: Option<OllamaModelDetails>,
}

/// Model details from Ollama's `/api/tags` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModelDetails {
    pub family: String,
    pub parameter_size: String,
    pub quantization_level: Option<String>,
}

/// The `/api/tags` response envelope.
#[derive(Debug, Serialize, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

/// Ollama connection status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    /// Whether Ollama is installed (binary found on PATH for local, or
    /// `ollama --version` succeeded for remote).
    pub installed: bool,
    /// Whether the Ollama server is running and reachable.
    pub running: bool,
    /// Ollama version string (if detectable).
    pub version: Option<String>,
    /// Endpoint URL used for the check (for display).
    pub endpoint: String,
    /// Error message if the check failed (for display).
    pub error: Option<String>,
}

/// Check if Ollama is installed locally by looking for the `ollama` binary.
fn is_ollama_installed_local() -> bool {
    crate::commands::claude::find_binary_by_name("ollama").is_some()
}

/// Check if Ollama is running locally by hitting the version endpoint.
async fn check_ollama_local() -> OllamaStatus {
    let endpoint = format!("http://localhost:{OLLAMA_PORT}");

    if !is_ollama_installed_local() {
        return OllamaStatus {
            installed: false,
            running: false,
            version: None,
            endpoint,
            error: Some("Ollama binary not found on PATH".into()),
        };
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(OLLAMA_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return OllamaStatus {
                installed: true,
                running: false,
                version: None,
                endpoint,
                error: Some(format!("Failed to create HTTP client: {e}")),
            };
        }
    };

    let url = format!("{endpoint}/api/version");
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let version: Option<OllamaVersion> = resp.json().await.ok();
            OllamaStatus {
                installed: true,
                running: true,
                version: version.map(|v| v.version),
                endpoint,
                error: None,
            }
        }
        Ok(resp) => OllamaStatus {
            installed: true,
            running: false,
            version: None,
            endpoint,
            error: Some(format!("Ollama responded with HTTP {}", resp.status())),
        },
        Err(e) => OllamaStatus {
            installed: true,
            running: false,
            version: None,
            endpoint,
            error: Some(format!(
                "Connection refused — Ollama may not be running: {e}"
            )),
        },
    }
}

/// Check if Ollama is running on a remote VPS via SSH exec.
/// Runs `curl -s http://localhost:11434/api/version` on the VPS.
async fn check_ollama_remote(server_id: &str) -> Result<OllamaStatus, CommandError> {
    let ssh_config = super::config::load_config_pub().map_err(CommandError::from)?;
    let server = ssh_config
        .servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| CommandError::Validation {
            field: "server_id".into(),
            reason: format!("No SSH server found with id `{server_id}`"),
        })?;

    // Check if ollama is installed on the VPS
    let check_installed_args = super::connection::build_ssh_args(&server);
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&check_installed_args)
        .arg("which ollama 2>/dev/null && echo __INSTALLED__");
    let installed_output =
        crate::external_command::run_with_timeout(cmd, "ssh ollama-installed-check", 10).await?;
    let installed = String::from_utf8_lossy(&installed_output.stdout).contains("__INSTALLED__");

    if !installed {
        return Ok(OllamaStatus {
            installed: false,
            running: false,
            version: None,
            endpoint: format!(
                "ssh://{}@{}:{}/ollama",
                server.username,
                server.host,
                server.port.unwrap_or(22)
            ),
            error: Some("Ollama not installed on the remote VPS".into()),
        });
    }

    // Check if Ollama is running on the VPS
    let check_running_args = super::connection::build_ssh_args(&server);
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&check_running_args).arg(format!(
        "curl -s --connect-timeout 5 http://localhost:{OLLAMA_PORT}/api/version 2>/dev/null"
    ));
    let running_output = crate::external_command::run_with_timeout(
        cmd,
        "ssh ollama-running-check",
        OLLAMA_TIMEOUT_SECS,
    )
    .await?;

    let stdout = String::from_utf8_lossy(&running_output.stdout);
    let endpoint = format!(
        "ssh://{}@{}:{}/ollama",
        server.username,
        server.host,
        server.port.unwrap_or(22)
    );

    if stdout.trim().is_empty() {
        return Ok(OllamaStatus {
            installed: true,
            running: false,
            version: None,
            endpoint,
            error: Some("Ollama not running on the VPS (port 11434 not responding)".into()),
        });
    }

    // Try to parse the version
    let version: Option<OllamaVersion> = serde_json::from_str(&stdout).ok();
    Ok(OllamaStatus {
        installed: true,
        running: true,
        version: version.map(|v| v.version),
        endpoint,
        error: None,
    })
}

/// Check Ollama status. When `server_id` is None, checks locally. When set,
/// checks on the remote VPS via SSH exec.
#[tauri::command]
#[tracing::instrument]
pub async fn check_ollama_status(server_id: Option<String>) -> Result<OllamaStatus, CommandError> {
    match server_id {
        None => Ok(check_ollama_local().await),
        Some(id) => check_ollama_remote(&id).await,
    }
}

/// List available Ollama models. When `server_id` is None, queries the local
/// Ollama API directly. When set, uses SSH exec + curl on the VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn list_ollama_models(
    server_id: Option<String>,
) -> Result<Vec<OllamaModel>, CommandError> {
    match server_id {
        None => {
            let url = format!("http://localhost:{OLLAMA_PORT}/api/tags");
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(OLLAMA_TIMEOUT_SECS))
                .build()
                .map_err(|e| CommandError::Io {
                    message: format!("Failed to create HTTP client: {e}"),
                })?;

            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| CommandError::Io {
                    message: format!("Failed to reach Ollama: {e}"),
                })?;

            if !resp.status().is_success() {
                return Err(CommandError::Io {
                    message: format!("Ollama returned HTTP {}", resp.status()),
                });
            }

            let tags: OllamaTagsResponse = resp.json().await.map_err(|e| CommandError::Io {
                message: format!("Failed to parse Ollama response: {e}"),
            })?;

            Ok(tags.models)
        }
        Some(id) => {
            let ssh_config = super::config::load_config_pub().map_err(CommandError::from)?;
            let server = ssh_config
                .servers
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| CommandError::Validation {
                    field: "server_id".into(),
                    reason: format!("No SSH server found with id `{id}`"),
                })?;

            let args = super::connection::build_ssh_args(&server);
            let mut cmd = tokio::process::Command::new("ssh");
            cmd.args(&args).arg(format!(
                "curl -s --connect-timeout 5 http://localhost:{OLLAMA_PORT}/api/tags 2>/dev/null"
            ));

            let output = crate::external_command::run_with_timeout(
                cmd,
                "ssh ollama-models",
                OLLAMA_TIMEOUT_SECS,
            )
            .await?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                return Ok(Vec::new());
            }

            let tags: OllamaTagsResponse =
                serde_json::from_str(&stdout).map_err(|e| CommandError::Io {
                    message: format!("Failed to parse Ollama models response: {e}"),
                })?;

            Ok(tags.models)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_status_serializes_camel_case() {
        let status = OllamaStatus {
            installed: true,
            running: true,
            version: Some("0.1.0".into()),
            endpoint: "http://localhost:11434".into(),
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"installed\":true"));
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"version\":\"0.1.0\""));
    }

    #[test]
    fn ollama_tags_response_parses() {
        let json = r#"{"models":[{"name":"llama3:latest","model":"llama3:latest","size":3825829519,"quantization_level":"q4_K_M","details":{"family":"llama","parameter_size":"8B","quantization_level":"q4_K_M"}}]}"#;
        let resp: OllamaTagsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.models.len(), 1);
        assert_eq!(resp.models[0].name, "llama3:latest");
        assert_eq!(resp.models[0].size, 3825829519);
        assert!(resp.models[0].details.is_some());
    }
}
