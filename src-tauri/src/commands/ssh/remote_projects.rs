//! Remote project registration and management.
//!
//! Remote projects are local metadata entries that point to a project folder
//! on a VPS, accessible via an SSH server. They're stored in
//! `~/ShipStudio/.cripcode/remote-projects.json` so they appear on the
//! dashboard alongside local projects. The actual file/terminal operations
//! go through the SSH commands in `files.rs` and the PTY-based
//! `RemoteTerminal` component.

use crate::errors::CommandError;
use crate::types::{RemoteProject, RemoteProjectsConfig, REMOTE_PROJECTS_CONFIG_SCHEMA_VERSION};
use uuid::Uuid;

/// Get the path to the remote projects config file.
fn get_config_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    Ok(home
        .join("CripCode")
        .join(".cripcode")
        .join("remote-projects.json"))
}

/// Load the remote projects config from disk.
fn load_config() -> Result<RemoteProjectsConfig, String> {
    let config_path = get_config_path()?;
    if !config_path.exists() {
        return Ok(RemoteProjectsConfig {
            schema_version: REMOTE_PROJECTS_CONFIG_SCHEMA_VERSION,
            projects: Vec::new(),
        });
    }
    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read remote projects config: {e}"))?;
    serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse remote projects config: {e}"))
}

/// Save the remote projects config to disk.
fn save_config(config: &RemoteProjectsConfig) -> Result<(), String> {
    let config_path = get_config_path()?;
    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .cripcode directory: {e}"))?;
        }
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize remote projects config: {e}"))?;
    std::fs::write(&config_path, json)
        .map_err(|e| format!("Failed to write remote projects config: {e}"))
}

/// Validate a remote path: must be absolute, no `..` traversal.
fn validate_remote_path(path: &str) -> Result<(), CommandError> {
    if !path.starts_with('/') {
        return Err(CommandError::Validation {
            field: "remotePath".into(),
            reason: "Remote path must be an absolute path (start with /)".into(),
        });
    }
    if path.contains("..") {
        return Err(CommandError::Validation {
            field: "remotePath".into(),
            reason: "Remote path must not contain .. (path traversal not allowed)".into(),
        });
    }
    Ok(())
}

/// List all registered remote projects.
#[tauri::command]
#[tracing::instrument]
pub fn list_remote_projects() -> Result<Vec<RemoteProject>, CommandError> {
    let config = load_config().map_err(CommandError::from)?;
    Ok(config.projects)
}

/// Register a new remote project. The `server_id` must reference an existing
/// SSH server config. The `remote_path` must be an absolute path on the VPS.
#[tauri::command]
#[tracing::instrument]
pub fn add_remote_project(
    name: String,
    server_id: String,
    remote_path: String,
) -> Result<RemoteProject, CommandError> {
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "name".into(),
            reason: "Project name is required".into(),
        });
    }
    if name_trimmed.len() > 100 {
        return Err(CommandError::Validation {
            field: "name".into(),
            reason: "Project name must be at most 100 characters".into(),
        });
    }
    validate_remote_path(&remote_path)?;

    // Verify the server exists
    let ssh_config = super::config::load_config_pub().map_err(CommandError::from)?;
    if !ssh_config.servers.iter().any(|s| s.id == server_id) {
        return Err(CommandError::Validation {
            field: "server_id".into(),
            reason: format!("No SSH server found with id `{server_id}`"),
        });
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let project = RemoteProject {
        id: Uuid::new_v4().to_string(),
        name: name_trimmed.to_string(),
        server_id,
        remote_path: remote_path.trim().to_string(),
        created_at: now,
        last_opened: None,
    };

    let mut config = load_config().map_err(CommandError::from)?;
    config.projects.push(project.clone());
    save_config(&config).map_err(CommandError::from)?;
    Ok(project)
}

/// Remove a remote project registration (does NOT delete anything on the VPS).
#[tauri::command]
#[tracing::instrument]
pub fn remove_remote_project(id: String) -> Result<(), CommandError> {
    let mut config = load_config().map_err(CommandError::from)?;
    let before = config.projects.len();
    config.projects.retain(|p| p.id != id);
    if config.projects.len() == before {
        return Err(CommandError::Validation {
            field: "id".into(),
            reason: format!("No remote project found with id `{id}`"),
        });
    }
    save_config(&config).map_err(CommandError::from)?;
    Ok(())
}

/// Mark a remote project as opened (stamps `last_opened`). Called when the
/// user opens a remote project from the dashboard.
#[tauri::command]
#[tracing::instrument]
pub fn mark_remote_project_opened(id: String) -> Result<(), CommandError> {
    let mut config = load_config().map_err(CommandError::from)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let project = config
        .projects
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| CommandError::Validation {
            field: "id".into(),
            reason: format!("No remote project found with id `{id}`"),
        })?;
    project.last_opened = Some(now);
    save_config(&config).map_err(CommandError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_remote_path_rejects_relative() {
        assert!(validate_remote_path("relative/path").is_err());
    }

    #[test]
    fn validate_remote_path_rejects_traversal() {
        assert!(validate_remote_path("/home/user/../etc").is_err());
    }

    #[test]
    fn validate_remote_path_accepts_absolute() {
        assert!(validate_remote_path("/home/user/myproject").is_ok());
    }
}
