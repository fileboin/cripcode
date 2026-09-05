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
    let check_installed_args =
        super::build_remote_ssh_args(&server, "which ollama 2>/dev/null && echo __INSTALLED__");
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&check_installed_args);
    super::connection::apply_ssh_auth_env(&mut cmd, &server)?;
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
    let check_running_args = super::build_remote_ssh_args(
        &server,
        &format!(
            "curl -s --connect-timeout 5 http://localhost:{OLLAMA_PORT}/api/version 2>/dev/null"
        ),
    );
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&check_running_args);
    super::connection::apply_ssh_auth_env(&mut cmd, &server)?;
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

            let args = super::build_remote_ssh_args(
                &server,
                &format!(
                    "curl -s --connect-timeout 5 http://localhost:{OLLAMA_PORT}/api/tags 2>/dev/null"
                ),
            );
            let mut cmd = tokio::process::Command::new("ssh");
            cmd.args(&args);
            super::connection::apply_ssh_auth_env(&mut cmd, &server)?;

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

// ============ Model Info (/api/show) ============

/// Detailed info for a single model from Ollama's `/api/show` endpoint.
/// This includes the context window length, which `/api/tags` doesn't provide.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModelInfo {
    /// Model name (e.g. "llama3:latest").
    pub name: String,
    /// Model family (e.g. "llama").
    pub family: String,
    /// Parameter size (e.g. "8B").
    pub parameter_size: String,
    /// Quantization level (e.g. "q4_K_M").
    pub quantization: Option<String>,
    /// Context window length in tokens, if detectable from model_info.
    pub context_length: Option<u64>,
    /// Number of parameters (approximate, from model_info if available).
    pub parameter_count: Option<u64>,
    /// Whether the model is currently loaded in memory.
    pub loaded: bool,
}

/// Raw `/api/show` response from Ollama. We only extract the fields we need;
/// the full response is much larger (includes modelfile, license, etc.).
#[derive(Debug, Deserialize)]
struct OllamaShowResponse {
    /// Free-form model info key-value pairs (e.g. "general.context_length").
    #[serde(default)]
    model_info: std::collections::HashMap<String, String>,
    /// Model details (same structure as `/api/tags` details).
    #[serde(default)]
    details: Option<OllamaModelDetails>,
}

/// Get detailed info for a single model via Ollama's `/api/show` endpoint.
/// This provides the context window length and other details not available
/// from `/api/tags`. When `server_id` is None, queries locally; when set,
/// uses SSH exec + curl on the VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn get_ollama_model_info(
    server_id: Option<String>,
    model_name: String,
) -> Result<OllamaModelInfo, CommandError> {
    match server_id {
        None => {
            let url = format!("http://localhost:{OLLAMA_PORT}/api/show");
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(OLLAMA_TIMEOUT_SECS))
                .build()
                .map_err(|e| CommandError::Io {
                    message: format!("Failed to create HTTP client: {e}"),
                })?;

            let resp = client
                .post(&url)
                .json(&serde_json::json!({ "name": model_name }))
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

            let show: OllamaShowResponse = resp.json().await.map_err(|e| CommandError::Io {
                message: format!("Failed to parse Ollama show response: {e}"),
            })?;

            Ok(parse_show_response(&model_name, &show))
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

            let escaped_name = model_name.replace('\'', "'\\''");
            let remote_cmd = format!(
                "curl -s --connect-timeout 5 -X POST http://localhost:{OLLAMA_PORT}/api/show -d '{{\"name\":\"{}\"}}' 2>/dev/null",
                escaped_name
            );
            let args = super::build_remote_ssh_args(&server, &remote_cmd);
            let mut cmd = tokio::process::Command::new("ssh");
            cmd.args(&args);
            super::connection::apply_ssh_auth_env(&mut cmd, &server)?;

            let output = crate::external_command::run_with_timeout(
                cmd,
                "ssh ollama-show",
                OLLAMA_TIMEOUT_SECS,
            )
            .await?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                return Err(CommandError::Io {
                    message: format!("Ollama returned empty response for model `{model_name}`"),
                });
            }

            let show: OllamaShowResponse =
                serde_json::from_str(&stdout).map_err(|e| CommandError::Io {
                    message: format!("Failed to parse Ollama show response: {e}"),
                })?;

            Ok(parse_show_response(&model_name, &show))
        }
    }
}

/// Extract a `OllamaModelInfo` from the raw `/api/show` response.
/// The context length is buried in `model_info` under various keys
/// depending on the model family (e.g. "llama.context_length",
/// "general.context_length").
fn parse_show_response(name: &str, show: &OllamaShowResponse) -> OllamaModelInfo {
    let details = show.details.as_ref();

    // Try common context length keys
    let context_length = show
        .model_info
        .get("llama.context_length")
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| {
            show.model_info
                .get("general.context_length")
                .and_then(|v| v.parse::<u64>().ok())
        })
        .or_else(|| {
            // Some models use "<family>.context_length"
            show.model_info
                .iter()
                .find(|(k, _)| k.ends_with(".context_length"))
                .and_then(|(_, v)| v.parse::<u64>().ok())
        });

    // Parameter count — sometimes in model_info as "general.parameter_count"
    let parameter_count = show
        .model_info
        .get("general.parameter_count")
        .and_then(|v| v.parse::<u64>().ok());

    OllamaModelInfo {
        name: name.to_string(),
        family: details
            .map(|d| d.family.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        parameter_size: details
            .map(|d| d.parameter_size.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        quantization: details.and_then(|d| d.quantization_level.clone()),
        context_length,
        parameter_count,
        loaded: false, // /api/show doesn't report loaded state; /api/ps does
    }
}

// ============ Model Selection ============

/// Ollama model selection config, persisted to
/// `~/CripCode/.cripcode/ollama-config.json`.
/// Maps a location key ("local" or an SSH server ID) to a selected model name.
#[derive(Debug, Serialize, Deserialize, Default)]
struct OllamaConfig {
    #[serde(default)]
    selections: std::collections::HashMap<String, String>,
}

/// Get the path to the Ollama config file.
fn ollama_config_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    Ok(home
        .join("CripCode")
        .join(".cripcode")
        .join("ollama-config.json"))
}

/// Load the Ollama config from disk (empty config if file doesn't exist).
fn load_ollama_config() -> Result<OllamaConfig, String> {
    let path = ollama_config_path()?;
    if !path.exists() {
        return Ok(OllamaConfig::default());
    }
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read Ollama config: {e}"))?;
    serde_json::from_str(&contents).map_err(|e| format!("Failed to parse Ollama config: {e}"))
}

/// Save the Ollama config to disk.
fn save_ollama_config(config: &OllamaConfig) -> Result<(), String> {
    let path = ollama_config_path()?;
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .cripcode directory: {e}"))?;
        }
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize Ollama config: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write Ollama config: {e}"))
}

/// Location key: "local" for local Ollama, or the SSH server ID for remote.
fn location_key(server_id: &Option<String>) -> String {
    match server_id {
        None => "local".to_string(),
        Some(id) => id.clone(),
    }
}

/// Get the currently selected Ollama model for a location (local or remote).
#[tauri::command]
#[tracing::instrument]
pub fn get_selected_ollama_model(
    server_id: Option<String>,
) -> Result<Option<String>, CommandError> {
    let config = load_ollama_config().map_err(CommandError::from)?;
    Ok(config.selections.get(&location_key(&server_id)).cloned())
}

/// Set the selected Ollama model for a location (local or remote).
/// Persists to disk so the selection survives app restarts.
#[tauri::command]
#[tracing::instrument]
pub fn set_selected_ollama_model(
    server_id: Option<String>,
    model_name: String,
) -> Result<(), CommandError> {
    let model_trimmed = model_name.trim();
    if model_trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "model_name".into(),
            reason: "Model name must not be empty".into(),
        });
    }
    let mut config = load_ollama_config().map_err(CommandError::from)?;
    config
        .selections
        .insert(location_key(&server_id), model_trimmed.to_string());
    save_ollama_config(&config).map_err(CommandError::from)?;
    Ok(())
}

/// Clear the selected Ollama model for a location (reset to None / default).
#[tauri::command]
#[tracing::instrument]
pub fn clear_selected_ollama_model(server_id: Option<String>) -> Result<(), CommandError> {
    let mut config = load_ollama_config().map_err(CommandError::from)?;
    config.selections.remove(&location_key(&server_id));
    save_ollama_config(&config).map_err(CommandError::from)?;
    Ok(())
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

    #[test]
    fn ollama_show_response_parses_with_context_length() {
        let json = r#"{
            "model_info": {
                "llama.context_length": "8192",
                "general.architecture": "llama"
            },
            "details": {
                "family": "llama",
                "parameter_size": "8B",
                "quantization_level": "q4_K_M"
            }
        }"#;
        let show: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(show.model_info.get("llama.context_length").unwrap(), "8192");
        let info = parse_show_response("llama3:latest", &show);
        assert_eq!(info.context_length, Some(8192));
        assert_eq!(info.family, "llama");
        assert_eq!(info.parameter_size, "8B");
    }

    #[test]
    fn ollama_show_response_parses_general_context_length() {
        // Some models use "general.context_length" instead of "llama.context_length"
        let json = r#"{
            "model_info": {
                "general.context_length": "4096"
            },
            "details": {
                "family": "gemma",
                "parameter_size": "7B",
                "quantization_level": "q4_K_M"
            }
        }"#;
        let show: OllamaShowResponse = serde_json::from_str(json).unwrap();
        let info = parse_show_response("gemma:latest", &show);
        assert_eq!(info.context_length, Some(4096));
        assert_eq!(info.family, "gemma");
    }

    #[test]
    fn ollama_show_response_handles_missing_context_length() {
        let json = r#"{
            "model_info": {},
            "details": {
                "family": "unknown",
                "parameter_size": "?",
                "quantization_level": null
            }
        }"#;
        let show: OllamaShowResponse = serde_json::from_str(json).unwrap();
        let info = parse_show_response("test:latest", &show);
        assert_eq!(info.context_length, None);
        assert_eq!(info.family, "unknown");
    }

    #[test]
    fn ollama_model_info_serializes_camel_case() {
        let info = OllamaModelInfo {
            name: "llama3:latest".into(),
            family: "llama".into(),
            parameter_size: "8B".into(),
            quantization: Some("q4_K_M".into()),
            context_length: Some(8192),
            parameter_count: Some(8_000_000_000),
            loaded: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"contextLength\":8192"));
        assert!(json.contains("\"parameterSize\":\"8B\""));
        assert!(json.contains("\"parameterCount\":8000000000"));
    }

    #[test]
    fn location_key_uses_local_for_none() {
        assert_eq!(location_key(&None), "local");
    }

    #[test]
    fn location_key_uses_server_id_for_some() {
        assert_eq!(location_key(&Some("server-1".into())), "server-1");
    }

    #[test]
    fn ollama_config_round_trips() {
        let mut config = OllamaConfig::default();
        config
            .selections
            .insert("local".into(), "llama3:latest".into());
        config
            .selections
            .insert("server-1".into(), "gemma:latest".into());
        let json = serde_json::to_string(&config).unwrap();
        let parsed: OllamaConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.selections.get("local").unwrap(), "llama3:latest");
        assert_eq!(parsed.selections.get("server-1").unwrap(), "gemma:latest");
    }

    #[test]
    fn ollama_config_defaults_to_empty() {
        let config = OllamaConfig::default();
        assert!(config.selections.is_empty());
    }
}
