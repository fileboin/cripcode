//! AI Provider Abstraction
//!
//! Unifies the existing CLI-based agents (Claude Code, Codex, OpenCode, Cursor)
//! with API-based providers like Ollama under a single `ProviderInfo` type.
//! The existing agent spawning (PTY/terminal) is NOT changed — this module
//! is a read-only registry that lets the frontend discover and select providers.
//!
//! Provider types:
//! - `Cli`: Terminal-based agents (spawned in a PTY via the existing system)
//! - `Ollama`: API-based provider (accessed via HTTP REST on port 11434)
//! - Future: `OpenAiCompatible`, `Gemini`, `OpenRouter`, etc.

use crate::agent::ALL_AGENTS;
use crate::errors::CommandError;
use serde::Serialize;

/// The type of an AI provider — determines how it's accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    /// Terminal-based agent (Claude Code, Codex, OpenCode, Cursor).
    /// Spawned in a PTY via the existing terminal system.
    Cli,
    /// Ollama local/remote API (HTTP REST on port 11434).
    Ollama,
}

/// Information about an AI provider, returned by `list_ai_providers`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    /// Unique identifier (e.g. "claude-code", "ollama").
    pub id: String,
    /// Human-readable name (e.g. "Claude Code", "Ollama").
    pub name: String,
    /// Provider type — determines how it's accessed.
    pub provider_type: ProviderType,
    /// Whether the provider is available (CLI binary found, or API running).
    pub available: bool,
    /// Short description for UI display.
    pub description: String,
    /// For Ollama: the server ID if remote, None if local. For CLI: always None.
    pub server_id: Option<String>,
}

/// List all available AI providers: existing CLI agents + Ollama (local).
/// Ollama is included only if it's installed (binary found on PATH).
#[tauri::command]
#[tracing::instrument]
pub async fn list_ai_providers() -> Result<Vec<ProviderInfo>, CommandError> {
    let mut providers = Vec::new();

    // Existing CLI agents from the agent registry
    for agent in ALL_AGENTS {
        let available = crate::commands::claude::find_binary_by_name(agent.binary_name).is_some();
        providers.push(ProviderInfo {
            id: agent.id.to_string(),
            name: agent.display_name.to_string(),
            provider_type: ProviderType::Cli,
            available,
            description: format!("{} CLI agent", agent.display_name),
            server_id: None,
        });
    }

    // Ollama (local) — included if the binary is found
    let ollama_installed = crate::commands::claude::find_binary_by_name("ollama").is_some();
    providers.push(ProviderInfo {
        id: "ollama".to_string(),
        name: "Ollama".to_string(),
        provider_type: ProviderType::Ollama,
        available: ollama_installed,
        description: "Local Ollama instance (API on port 11434)".to_string(),
        server_id: None,
    });

    // Remote Ollama providers — one per registered SSH server
    if let Ok(ssh_config) = super::config::load_config_pub() {
        for server in &ssh_config.servers {
            providers.push(ProviderInfo {
                id: format!("ollama-{}", server.id),
                name: format!("Ollama ({})", server.name),
                provider_type: ProviderType::Ollama,
                available: false, // Unknown until probed — the frontend can check
                description: format!("Remote Ollama on {}@{}", server.username, server.host),
                server_id: Some(server.id.clone()),
            });
        }
    }

    Ok(providers)
}

/// Get info for a single provider by ID.
#[tauri::command]
#[tracing::instrument]
pub async fn get_ai_provider_info(provider_id: String) -> Result<ProviderInfo, CommandError> {
    let providers = list_ai_providers().await?;
    providers
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| CommandError::Validation {
            field: "provider_id".into(),
            reason: format!("No AI provider found with id `{provider_id}`"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_type_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&ProviderType::Cli).unwrap(),
            "\"cli\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderType::Ollama).unwrap(),
            "\"ollama\""
        );
    }

    #[test]
    fn provider_info_serializes_camel_case() {
        let info = ProviderInfo {
            id: "ollama".into(),
            name: "Ollama".into(),
            provider_type: ProviderType::Ollama,
            available: true,
            description: "Local Ollama".into(),
            server_id: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"providerType\":\"ollama\""));
        assert!(json.contains("\"serverId\":null"));
    }
}
