//! SSH server configuration CRUD and JSON persistence.
//!
//! Server configs are stored in `~/ShipStudio/.cripcode/ssh-servers.json`,
//! following the same pattern as `external-projects.json`. The private key
//! file itself is never read into memory — only its filesystem path is stored.

use crate::errors::CommandError;
use crate::types::{SshServer, SshServersConfig, SSH_SERVERS_CONFIG_SCHEMA_VERSION};
use std::path::PathBuf;
use uuid::Uuid;

/// Get the path to the SSH servers config file.
fn get_config_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    Ok(home
        .join("CripCode")
        .join(".cripcode")
        .join("ssh-servers.json"))
}

/// Load the SSH servers config from disk. Returns an empty config if the file
/// doesn't exist yet (first run).
fn load_config() -> Result<SshServersConfig, String> {
    let config_path = get_config_path()?;

    if !config_path.exists() {
        return Ok(SshServersConfig {
            schema_version: SSH_SERVERS_CONFIG_SCHEMA_VERSION,
            servers: Vec::new(),
        });
    }

    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read SSH servers config: {e}"))?;

    serde_json::from_str(&contents).map_err(|e| format!("Failed to parse SSH servers config: {e}"))
}

/// Save the SSH servers config to disk, creating the parent directory if needed.
fn save_config(config: &SshServersConfig) -> Result<(), String> {
    let config_path = get_config_path()?;

    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .cripcode directory: {e}"))?;
        }
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize SSH servers config: {e}"))?;

    std::fs::write(&config_path, json)
        .map_err(|e| format!("Failed to write SSH servers config: {e}"))
}

/// Validate a server name: non-empty, at most 100 chars.
fn validate_name(name: &str) -> Result<(), CommandError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "name".into(),
            reason: "Server name is required".into(),
        });
    }
    if trimmed.len() > 100 {
        return Err(CommandError::Validation {
            field: "name".into(),
            reason: "Server name must be at most 100 characters".into(),
        });
    }
    Ok(())
}

/// Validate a host: non-empty, no spaces, at most 255 chars (DNS limit).
fn validate_host(host: &str) -> Result<(), CommandError> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "host".into(),
            reason: "Host is required".into(),
        });
    }
    if trimmed.contains(char::is_whitespace) {
        return Err(CommandError::Validation {
            field: "host".into(),
            reason: "Host must not contain spaces".into(),
        });
    }
    if trimmed.len() > 255 {
        return Err(CommandError::Validation {
            field: "host".into(),
            reason: "Host must be at most 255 characters".into(),
        });
    }
    Ok(())
}

/// Validate a username: non-empty, no spaces.
fn validate_username(username: &str) -> Result<(), CommandError> {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "username".into(),
            reason: "Username is required".into(),
        });
    }
    if trimmed.contains(char::is_whitespace) {
        return Err(CommandError::Validation {
            field: "username".into(),
            reason: "Username must not contain spaces".into(),
        });
    }
    Ok(())
}

/// Validate a key path: if provided, must be an absolute filesystem path.
/// The key file itself is never read by Cripcode — only its path is stored.
fn validate_key_path(key_path: &Option<String>) -> Result<(), CommandError> {
    if let Some(path) = key_path {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(CommandError::Validation {
                field: "keyPath".into(),
                reason: "Key path must not be empty when provided".into(),
            });
        }
        let pb = std::path::Path::new(trimmed);
        if !pb.is_absolute() {
            return Err(CommandError::Validation {
                field: "keyPath".into(),
                reason: "Key path must be an absolute filesystem path".into(),
            });
        }
    }
    Ok(())
}

/// List all saved SSH server configurations.
#[tauri::command]
#[tracing::instrument]
pub fn list_ssh_servers() -> Result<Vec<SshServer>, CommandError> {
    let config = load_config().map_err(CommandError::from)?;
    Ok(config.servers)
}

/// Add a new SSH server configuration.
#[tauri::command]
#[tracing::instrument]
pub fn add_ssh_server(
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    key_path: Option<String>,
) -> Result<SshServer, CommandError> {
    validate_name(&name)?;
    validate_host(&host)?;
    validate_username(&username)?;
    validate_key_path(&key_path)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let server = SshServer {
        id: Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        host: host.trim().to_string(),
        port: port.or(Some(22)),
        username: username.trim().to_string(),
        key_path: key_path.and_then(|p| {
            let t = p.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }),
        created_at: now,
        last_connected_at: None,
    };

    let mut config = load_config().map_err(CommandError::from)?;
    config.servers.push(server.clone());
    save_config(&config).map_err(CommandError::from)?;
    Ok(server)
}

/// Update an existing SSH server configuration.
#[tauri::command]
#[tracing::instrument]
pub fn update_ssh_server(
    id: String,
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    key_path: Option<String>,
) -> Result<SshServer, CommandError> {
    validate_name(&name)?;
    validate_host(&host)?;
    validate_username(&username)?;
    validate_key_path(&key_path)?;

    let mut config = load_config().map_err(CommandError::from)?;
    let server = config
        .servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| CommandError::Validation {
            field: "id".into(),
            reason: format!("No SSH server found with id `{id}`"),
        })?;

    server.name = name.trim().to_string();
    server.host = host.trim().to_string();
    server.port = port.or(Some(22));
    server.username = username.trim().to_string();
    server.key_path = key_path.and_then(|p| {
        let t = p.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });

    let updated = server.clone();
    save_config(&config).map_err(CommandError::from)?;
    Ok(updated)
}

/// Delete an SSH server configuration by ID.
#[tauri::command]
#[tracing::instrument]
pub fn delete_ssh_server(id: String) -> Result<(), CommandError> {
    let mut config = load_config().map_err(CommandError::from)?;
    let before = config.servers.len();
    config.servers.retain(|s| s.id != id);
    if config.servers.len() == before {
        return Err(CommandError::Validation {
            field: "id".into(),
            reason: format!("No SSH server found with id `{id}`"),
        });
    }
    save_config(&config).map_err(CommandError::from)?;
    Ok(())
}

/// Public wrapper for `load_config` — used by the connection module to
/// look up server details for connection testing.
pub(crate) fn load_config_pub() -> Result<SshServersConfig, String> {
    load_config()
}

/// Stamp `last_connected_at` on a server after a successful connection.
/// Called by `connection::connect_ssh` on success.
pub(crate) fn record_successful_connection(id: &str) -> Result<(), String> {
    let mut config = load_config()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(server) = config.servers.iter_mut().find(|s| s.id == id) {
        server.last_connected_at = Some(now);
    }
    save_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_rejects_empty() {
        assert!(validate_name("").is_err());
        assert!(validate_name("  ").is_err());
    }

    #[test]
    fn validate_name_rejects_too_long() {
        assert!(validate_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn validate_name_accepts_normal() {
        assert!(validate_name("Production VPS").is_ok());
    }

    #[test]
    fn validate_host_rejects_spaces() {
        assert!(validate_host("has space").is_err());
    }

    #[test]
    fn validate_host_accepts_ip_and_hostname() {
        assert!(validate_host("203.0.113.1").is_ok());
        assert!(validate_host("example.com").is_ok());
    }

    #[test]
    fn validate_key_path_accepts_none() {
        assert!(validate_key_path(&None).is_ok());
    }

    #[test]
    fn validate_key_path_rejects_relative() {
        assert!(validate_key_path(&Some("relative/path".into())).is_err());
    }

    #[test]
    fn validate_key_path_accepts_absolute() {
        assert!(validate_key_path(&Some("/Users/me/.ssh/id_ed25519".into())).is_ok());
    }
}
