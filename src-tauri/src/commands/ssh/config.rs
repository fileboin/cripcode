//! SSH server configuration CRUD and JSON persistence.
//!
//! Server configs are stored in `~/CripCode/.cripcode/ssh-servers.json`,
//! following the same pattern as `external-projects.json`. The private key
//! file itself is never read into memory — only its filesystem path is stored.
//! A password-mode server's password is never written to this file at all:
//! it lives in the OS keystore (see [`secrets`]) keyed by server id, and the
//! add/update/delete flows below pass it through transiently.

use super::secrets::{KeyringStore, SecretStore};
use crate::errors::CommandError;
use crate::types::{AuthType, SshServer, SshServersConfig, SSH_SERVERS_CONFIG_SCHEMA_VERSION};
use std::path::{Path, PathBuf};
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
    load_config_from(&config_path)
}

fn load_config_from(config_path: &Path) -> Result<SshServersConfig, String> {
    if !config_path.exists() {
        return Ok(SshServersConfig {
            schema_version: SSH_SERVERS_CONFIG_SCHEMA_VERSION,
            servers: Vec::new(),
        });
    }

    let contents = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read SSH servers config: {e}"))?;

    serde_json::from_str(&contents).map_err(|e| format!("Failed to parse SSH servers config: {e}"))
}

/// Temporary file used while persisting the config, in the same directory so
/// the final `rename` stays on one filesystem (and therefore atomic).
fn temp_config_path(config_path: &std::path::Path) -> PathBuf {
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ssh-servers.json");
    config_path.with_file_name(format!(".{file_name}.tmp"))
}

/// Save the SSH servers config to disk, creating the parent directory if needed.
///
/// Atomic write: write to a temp file in the same directory, then rename over
/// the real file — `rename` is atomic on the same filesystem, so a reader can
/// never observe a half-written config and an interrupted write leaves the
/// previous `ssh-servers.json` intact instead of truncated.
fn save_config(config: &SshServersConfig) -> Result<(), String> {
    let config_path = get_config_path()?;
    save_config_to(&config_path, config)
}

fn save_config_to(config_path: &Path, config: &SshServersConfig) -> Result<(), String> {
    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .cripcode directory: {e}"))?;
        }
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize SSH servers config: {e}"))?;

    let temp_path = temp_config_path(config_path);
    std::fs::write(&temp_path, &json)
        .map_err(|e| format!("Failed to write SSH servers config: {e}"))?;

    std::fs::rename(&temp_path, config_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        format!("Failed to replace SSH servers config: {e}")
    })
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

/// Expand a leading `~` to the user's home directory so the stored key path is
/// absolute everywhere (the ssh CLI's own tilde handling differs per platform).
fn expand_home(path: &str) -> Result<String, CommandError> {
    if path != "~" && !path.starts_with("~/") && !path.starts_with("~\\") {
        return Ok(path.to_string());
    }
    let home = dirs::home_dir().ok_or_else(|| CommandError::Validation {
        field: "keyPath".into(),
        reason: "Could not resolve the home directory".into(),
    })?;
    if path == "~" {
        return Ok(home.to_string_lossy().to_string());
    }
    Ok(home.join(&path[2..]).to_string_lossy().to_string())
}

/// Normalize and validate a key path: expand a leading `~`, then require an
/// absolute path inside the user's home directory. The key file itself is
/// never read by Cripcode — only the normalized path is stored and handed to
/// the ssh CLI. Returns the path to persist, or None when no key is configured.
fn normalize_key_path(key_path: &Option<String>) -> Result<Option<String>, CommandError> {
    let Some(path) = key_path else {
        return Ok(None);
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "keyPath".into(),
            reason: "Key path must not be empty when provided".into(),
        });
    }

    let expanded = expand_home(trimmed)?;
    let pb = std::path::Path::new(&expanded);
    if !pb.is_absolute() {
        return Err(CommandError::Validation {
            field: "keyPath".into(),
            reason: "Key path must be an absolute filesystem path".into(),
        });
    }
    if pb
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(CommandError::Validation {
            field: "keyPath".into(),
            reason: "Key path must not contain ..".into(),
        });
    }

    // Keys live under the user's home in every normal setup (any subfolder —
    // no fixed `.ssh` assumption); paths outside it are rejected so a
    // compromised webview can't point `-i` at arbitrary system files.
    let canonical = dunce::canonicalize(pb).unwrap_or_else(|_| pb.to_path_buf());
    let home = dirs::home_dir().ok_or_else(|| CommandError::Validation {
        field: "keyPath".into(),
        reason: "Could not resolve the home directory".into(),
    })?;
    let home_canonical = dunce::canonicalize(&home).unwrap_or(home);
    if !canonical.starts_with(&home_canonical) {
        return Err(CommandError::Validation {
            field: "keyPath".into(),
            reason: "Key path must be inside your home directory".into(),
        });
    }

    Ok(Some(expanded))
}

/// List all saved SSH server configurations.
#[tauri::command]
#[tracing::instrument]
pub fn list_ssh_servers() -> Result<Vec<SshServer>, CommandError> {
    let config = load_config().map_err(CommandError::from)?;
    Ok(config.servers)
}

/// Add a new SSH server configuration.
///
/// For `AuthType::Password` the password is written to the OS keystore keyed
/// by the new server id and never persisted to the JSON file. If the JSON
/// write fails after the keystore write, the stored password is removed
/// (compensation) so no orphaned secret remains.
#[tauri::command]
#[tracing::instrument(skip(password))]
pub fn add_ssh_server(
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    key_path: Option<String>,
    auth_type: Option<AuthType>,
    password: Option<String>,
) -> Result<SshServer, CommandError> {
    let config_path = get_config_path().map_err(CommandError::from)?;
    add_server_with_store(
        &config_path,
        &KeyringStore,
        name,
        host,
        port,
        username,
        key_path,
        auth_type,
        password,
    )
}

pub(crate) fn add_server_with_store(
    config_path: &Path,
    store: &dyn SecretStore,
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    key_path: Option<String>,
    auth_type: Option<AuthType>,
    password: Option<String>,
) -> Result<SshServer, CommandError> {
    validate_name(&name)?;
    validate_host(&host)?;
    validate_username(&username)?;
    let auth_type = auth_type.unwrap_or_default();
    // The key path is only meaningful for key authentication; password mode
    // never passes a key file.
    let key_path = match auth_type {
        AuthType::Key => normalize_key_path(&key_path)?,
        AuthType::Password => None,
    };
    require_password_when_password_mode(auth_type, password.as_deref())?;

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
        key_path,
        auth_type,
        created_at: now,
        last_connected_at: None,
    };

    let mut config = load_config_from(config_path).map_err(CommandError::from)?;
    config.servers.push(server.clone());

    // Keystore write happens BEFORE the JSON write; if the JSON write then
    // fails, the secret is removed so nothing is orphaned.
    if auth_type == AuthType::Password {
        store.set_password(&server.id, password.as_deref().unwrap_or_default())?;
    }
    if let Err(e) = save_config_to(config_path, &config) {
        if auth_type == AuthType::Password {
            let _ = store.delete_password(&server.id);
        }
        return Err(e.into());
    }
    Ok(server)
}

/// Update an existing SSH server configuration.
///
/// Password handling by mode transition: switching to password requires a new
/// password (or an already-stored one), staying in password mode with a blank
/// field keeps the stored password, and switching back to key authentication
/// deletes the stored password entirely.
#[tauri::command]
#[tracing::instrument(skip(password))]
pub fn update_ssh_server(
    id: String,
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    key_path: Option<String>,
    auth_type: Option<AuthType>,
    password: Option<String>,
) -> Result<SshServer, CommandError> {
    let config_path = get_config_path().map_err(CommandError::from)?;
    update_server_with_store(
        &config_path,
        &KeyringStore,
        id,
        name,
        host,
        port,
        username,
        key_path,
        auth_type,
        password,
    )
}

pub(crate) fn update_server_with_store(
    config_path: &Path,
    store: &dyn SecretStore,
    id: String,
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    key_path: Option<String>,
    auth_type: Option<AuthType>,
    password: Option<String>,
) -> Result<SshServer, CommandError> {
    validate_name(&name)?;
    validate_host(&host)?;
    validate_username(&username)?;
    let auth_type = auth_type.unwrap_or_default();
    let key_path = match auth_type {
        AuthType::Key => normalize_key_path(&key_path)?,
        AuthType::Password => None,
    };

    let mut config = load_config_from(config_path).map_err(CommandError::from)?;
    let server = config
        .servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| CommandError::Validation {
            field: "id".into(),
            reason: format!("No SSH server found with id `{id}`"),
        })?;

    let was_password = server.auth_type == AuthType::Password;
    server.name = name.trim().to_string();
    server.host = host.trim().to_string();
    server.port = port.or(Some(22));
    server.username = username.trim().to_string();
    server.key_path = key_path;
    server.auth_type = auth_type;

    // Keystore transitions. `restore` snapshots the previous value so a
    // failed JSON write can roll the keystore back instead of orphaning or
    // clobbering a secret.
    let mut restore: Option<Option<String>> = None;
    match auth_type {
        AuthType::Key => {
            if was_password {
                store.delete_password(&id)?;
            }
        }
        AuthType::Password => match password.as_deref() {
            Some(pw) if !pw.is_empty() => {
                restore = Some(store.get_password(&id)?);
                store.set_password(&id, pw)?;
            }
            _ if was_password => {
                if store.get_password(&id)?.is_none() {
                    return Err(CommandError::expected(
                        "A password is required for password authentication.",
                    ));
                }
            }
            _ => {
                return Err(CommandError::expected(
                    "A password is required for password authentication.",
                ));
            }
        },
    }

    let updated = server.clone();
    if let Err(e) = save_config_to(config_path, &config) {
        if let Some(previous) = restore {
            match previous {
                Some(previous) => {
                    let _ = store.set_password(&id, &previous);
                }
                None => {
                    let _ = store.delete_password(&id);
                }
            }
        }
        return Err(e.into());
    }
    Ok(updated)
}

/// Delete an SSH server configuration by ID. The keystore entry (password
/// mode) is removed best-effort afterwards: the server is already gone from
/// the JSON at that point, so a keystore hiccup must not roll the deletion
/// back or leave the server undeletable.
#[tauri::command]
#[tracing::instrument]
pub fn delete_ssh_server(id: String) -> Result<(), CommandError> {
    let config_path = get_config_path().map_err(CommandError::from)?;
    delete_server_with_store(&config_path, &KeyringStore, id)
}

pub(crate) fn delete_server_with_store(
    config_path: &Path,
    store: &dyn SecretStore,
    id: String,
) -> Result<(), CommandError> {
    let mut config = load_config_from(config_path).map_err(CommandError::from)?;
    let before = config.servers.len();
    config.servers.retain(|s| s.id != id);
    if config.servers.len() == before {
        return Err(CommandError::Validation {
            field: "id".into(),
            reason: format!("No SSH server found with id `{id}`"),
        });
    }
    save_config_to(config_path, &config).map_err(CommandError::from)?;
    let _ = store.delete_password(&id);
    Ok(())
}

/// A password-mode server cannot be saved without a password; key mode must
/// not carry one. The check reads the raw value only — passwords are never
/// trimmed (they may legitimately contain spaces) and never embedded in the
/// error message.
fn require_password_when_password_mode(
    auth_type: AuthType,
    password: Option<&str>,
) -> Result<(), CommandError> {
    match auth_type {
        AuthType::Password => match password {
            Some(pw) if !pw.is_empty() => Ok(()),
            _ => Err(CommandError::expected(
                "A password is required for password authentication.",
            )),
        },
        AuthType::Key => Ok(()),
    }
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
    fn normalize_key_path_accepts_none() {
        assert!(normalize_key_path(&None).unwrap().is_none());
    }

    #[test]
    fn normalize_key_path_rejects_empty() {
        assert!(normalize_key_path(&Some("   ".into())).is_err());
    }

    #[test]
    fn normalize_key_path_rejects_relative() {
        assert!(normalize_key_path(&Some("relative/path".into())).is_err());
    }

    #[test]
    fn normalize_key_path_accepts_paths_inside_home() {
        let home = dirs::home_dir().expect("home dir");
        let path = home.join(".ssh").join("id_ed25519");
        let normalized = normalize_key_path(&Some(path.to_string_lossy().to_string()))
            .unwrap()
            .expect("normalized path");
        assert!(normalized.ends_with("id_ed25519"));
    }

    #[test]
    fn normalize_key_path_expands_tilde_into_home() {
        let home = dirs::home_dir().expect("home dir");
        let normalized = normalize_key_path(&Some("~/.ssh/id_ed25519".into()))
            .unwrap()
            .expect("expanded path");
        assert!(normalized.starts_with(&home.to_string_lossy().to_string()));
        assert!(normalized.ends_with("id_ed25519"));
    }

    #[test]
    fn normalize_key_path_rejects_paths_outside_home() {
        let home = dirs::home_dir().expect("home dir");
        let outside = home
            .parent()
            .expect("home has a parent")
            .join("key-outside-home");
        assert!(normalize_key_path(&Some(outside.to_string_lossy().to_string())).is_err());
    }

    #[test]
    fn normalize_key_path_rejects_parent_dir_traversal() {
        let home = dirs::home_dir().expect("home dir");
        let traversal = home.join("..").join("etc").join("id_ed25519");
        assert!(normalize_key_path(&Some(traversal.to_string_lossy().to_string())).is_err());
    }

    #[test]
    fn temp_config_path_is_hidden_tmp_sibling_of_the_real_file() {
        let real = std::path::Path::new("/home/user/.cripcode/ssh-servers.json");
        let temp = temp_config_path(real);
        assert_eq!(temp.parent(), real.parent());
        assert_eq!(temp.file_name().unwrap(), ".ssh-servers.json.tmp");
    }

    // ---- password transit (auth type + keystore) ----

    use crate::commands::ssh::secrets::MockSecretStore;
    use crate::types::AuthType;

    /// Isolated temp config dir per test — the real ssh-servers.json is
    /// never touched by unit tests.
    struct TempConfig {
        dir: std::path::PathBuf,
    }

    impl TempConfig {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("cripcode-ssh-config-test-{}-{tag}", Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("create temp config dir");
            Self { dir }
        }

        fn path(&self) -> std::path::PathBuf {
            self.dir.join("ssh-servers.json")
        }

        fn json(&self) -> String {
            std::fs::read_to_string(self.path()).expect("read config json")
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn add_args() -> (String, String, Option<u16>, String, Option<String>) {
        (
            "Test VPS".into(),
            "example.com".into(),
            Some(22),
            "deploy".into(),
            None,
        )
    }

    #[test]
    fn add_password_server_stores_secret_in_keystore_not_json() {
        let temp = TempConfig::new("add-password");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        let server = add_server_with_store(
            &temp.path(),
            &store,
            name,
            host,
            port,
            username,
            None,
            Some(AuthType::Password),
            Some("s3cret-pw".into()),
        )
        .expect("add password server");

        assert_eq!(server.auth_type, AuthType::Password);
        assert!(store.contains(&server.id));
        // The JSON on disk must be free of the password.
        let json = temp.json();
        assert!(!json.contains("s3cret-pw"));
        assert!(json.contains("\"authType\": \"password\""));
    }

    #[test]
    fn add_key_server_defaults_and_touches_no_keystore_entry() {
        let temp = TempConfig::new("add-key");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        let server = add_server_with_store(
            &temp.path(),
            &store,
            name,
            host,
            port,
            username,
            Some("~/id_ed25519".into()),
            None, // no explicit auth type → defaults to Key
            None,
        )
        .expect("add key server");

        assert_eq!(server.auth_type, AuthType::Key);
        assert!(server.key_path.is_some());
        assert!(store.is_empty());
    }

    #[test]
    fn add_password_server_without_password_is_rejected() {
        let temp = TempConfig::new("add-pw-missing");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        let result = add_server_with_store(
            &temp.path(),
            &store,
            name,
            host,
            port,
            username,
            None,
            Some(AuthType::Password),
            None,
        );
        assert!(result.is_err());
        assert!(!store.contains("nonexistent"));
        // Nothing was persisted.
        assert!(!temp.path().exists());
    }

    #[test]
    fn failed_json_write_compensates_keystore_entry() {
        let temp = TempConfig::new("compensate");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        // config_path's parent is a FILE → create_dir_all fails → JSON save
        // fails after the keystore write → compensation must remove the
        // secret instead of orphaning it.
        let blocker = temp.dir.join("blocker");
        std::fs::write(&blocker, b"not a directory").expect("write blocker");
        let blocked_path = blocker.join("ssh-servers.json");

        let result = add_server_with_store(
            &blocked_path,
            &store,
            name,
            host,
            port,
            username,
            None,
            Some(AuthType::Password),
            Some("s3cret-pw".into()),
        );
        assert!(result.is_err());
        // The keystore write happened before the JSON write and the secret
        // was compensated away — nothing may remain in the store.
        assert!(store.is_empty());
    }

    #[test]
    fn update_changes_keeps_and_clears_password() {
        let temp = TempConfig::new("update-pw");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        let server = add_server_with_store(
            &temp.path(),
            &store,
            name.clone(),
            host.clone(),
            port,
            username.clone(),
            None,
            Some(AuthType::Password),
            Some("old-pw".into()),
        )
        .expect("add");

        // Change: blank field (None) keeps the stored password.
        let updated = update_server_with_store(
            &temp.path(),
            &store,
            server.id.clone(),
            name.clone(),
            host.clone(),
            port,
            username.clone(),
            None,
            Some(AuthType::Password),
            None,
        )
        .expect("update keep");
        assert_eq!(updated.auth_type, AuthType::Password);
        assert!(store.contains(&server.id));

        // Change: a new value overwrites.
        update_server_with_store(
            &temp.path(),
            &store,
            server.id.clone(),
            name.clone(),
            host.clone(),
            port,
            username.clone(),
            None,
            Some(AuthType::Password),
            Some("new-pw".into()),
        )
        .expect("update change");

        // Switch back to key auth → stored password is deleted.
        let switched = update_server_with_store(
            &temp.path(),
            &store,
            server.id.clone(),
            name,
            host,
            port,
            username,
            None,
            Some(AuthType::Key),
            None,
        )
        .expect("switch to key");
        assert_eq!(switched.auth_type, AuthType::Key);
        assert!(!store.contains(&server.id));
    }

    #[test]
    fn switching_to_password_without_a_password_is_rejected() {
        let temp = TempConfig::new("switch-pw-missing");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        let server = add_server_with_store(
            &temp.path(),
            &store,
            name.clone(),
            host.clone(),
            port,
            username.clone(),
            None,
            Some(AuthType::Key),
            None,
        )
        .expect("add key server");

        let result = update_server_with_store(
            &temp.path(),
            &store,
            server.id,
            name,
            host,
            port,
            username,
            None,
            Some(AuthType::Password),
            None, // no stored password, none provided → reject
        );
        assert!(result.is_err());
    }

    #[test]
    fn delete_removes_keystore_entry() {
        let temp = TempConfig::new("delete-pw");
        let store = MockSecretStore::new();
        let (name, host, port, username, _key) = add_args();

        let server = add_server_with_store(
            &temp.path(),
            &store,
            name,
            host,
            port,
            username,
            None,
            Some(AuthType::Password),
            Some("s3cret-pw".into()),
        )
        .expect("add");

        delete_server_with_store(&temp.path(), &store, server.id.clone()).expect("delete");
        assert!(!store.contains(&server.id));
        assert!(!temp.json().contains(&server.id));
    }
}
