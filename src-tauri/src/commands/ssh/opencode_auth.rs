//! Manage OpenCode provider credentials in the remote OpenCode auth store.
//!
//! OpenCode (running on a remote server) stores provider credentials in
//! `~/.local/share/opencode/auth.json` as a JSON object keyed by provider id:
//! `{"anthropic": {"type": "api", "key": "..."}}`.
//!
//! These commands let the UI add/update/remove an API key for any provider id
//! OpenCode understands (OpenCode Zen = `opencode`, OpenCode Go =
//! `opencode-go`, or any future provider such as `openai`) without exposing
//! it anywhere observable:
//!
//! - The key travels over IPC once, is merged into the auth JSON in memory,
//!   and is piped to the remote `cat` **via stdin** — never argv, env, a
//!   PTY, logs, tracing spans (the key param is `skip`ped), or errors.
//! - Status checks return provider ids only; values are never returned,
//!   logged, or echoed.
//! - Existing entries (e.g. `anthropic`) are preserved: a merge, never an
//!   overwrite. A corrupt remote file is an error, not a clobber.

use super::config;
use super::files::{get_server, run_ssh_exec_strict, run_ssh_exec_with_stdin_strict};
use crate::errors::CommandError;
use serde_json::{Map, Value};

/// Remote path of the OpenCode auth store (expanded by the remote shell).
const AUTH_PATH: &str = "$HOME/.local/share/opencode/auth.json";

/// Upper bound on a provider id, mirroring OpenCode's own id shape. The value
/// is validated structurally (not against a whitelist) so new providers work
/// without a code change while arbitrary strings can never reach the auth file.
const MAX_PROVIDER_ID_LEN: usize = 100;

/// Remote write command: ensure the directory exists, write stdin to the auth
/// file, and keep the 0600 permission OpenCode expects. The key is piped via
/// stdin and never appears in this command string.
const WRITE_CMD: &str = "mkdir -p \"$HOME/.local/share/opencode\" && umask 077 && cat > \"$HOME/.local/share/opencode/auth.json\" && chmod 600 \"$HOME/.local/share/opencode/auth.json\"";

/// `cat` the remote auth file. Returns `None` when the file does not exist
/// (cat fails with exit != 0 and empty-ish stderr). The content is only ever
/// handled in memory; errors carry no file content. Runs with
/// `StrictHostKeyChecking=yes`: the UI calls the explicit host-key gate before
/// this path, so an unverified host fails closed here.
async fn read_auth_file(server: &crate::types::SshServer) -> Result<Option<String>, CommandError> {
    let cat_cmd = format!("cat \"{AUTH_PATH}\" 2>/dev/null");
    let label = format!("ssh opencode auth read {}", server.name);
    let output = run_ssh_exec_strict(server, &cat_cmd, &label).await?;
    if !output.status.success() {
        // Missing file (or unreadable) — treat as "no store yet".
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

/// Write a serialized auth file to the remote store via SSH stdin. `contents`
/// never appears in the command string, argv, environment, or error messages.
async fn write_auth_file(
    server: &crate::types::SshServer,
    contents: &str,
    action: &str,
) -> Result<(), CommandError> {
    let label = format!("ssh opencode auth {action} {}", server.name);
    let output =
        run_ssh_exec_with_stdin_strict(server, WRITE_CMD, contents.as_bytes(), &label).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            // cat/mkdir/chmod errors never echo stdin content.
            stderr: stderr.trim().to_string(),
        });
    }
    Ok(())
}

/// Parse the remote auth file into a JSON object map. Missing/blank files are
/// an empty map; corrupt files are rejected instead of clobbered.
fn parse_auth_object(existing: Option<&str>) -> Result<Map<String, Value>, CommandError> {
    match existing {
        None => Ok(Map::new()),
        Some(text) if text.trim().is_empty() => Ok(Map::new()),
        Some(text) => {
            let value: Value =
                serde_json::from_str(text).map_err(|_| CommandError::Validation {
                    field: "auth.json".into(),
                    reason: "Remote OpenCode auth.json is not valid JSON; refusing to modify it"
                        .into(),
                })?;
            value.as_object().cloned().ok_or(CommandError::Validation {
                field: "auth.json".into(),
                reason: "Remote OpenCode auth.json is not a JSON object; refusing to modify it"
                    .into(),
            })
        }
    }
}

/// Merge a single provider entry into the auth JSON, preserving every other
/// entry (merge, never overwrite).
fn merge_auth_entry(
    existing: Option<&str>,
    provider_id: &str,
    api_key: &str,
) -> Result<String, CommandError> {
    let mut root = parse_auth_object(existing)?;
    root.insert(
        provider_id.to_string(),
        serde_json::json!({ "type": "api", "key": api_key }),
    );
    Ok(Value::Object(root).to_string())
}

/// Remove one provider entry, preserving every other entry. Returns `None`
/// when the provider is not present (nothing to write).
fn remove_auth_entry(
    existing: Option<&str>,
    provider_id: &str,
) -> Result<Option<String>, CommandError> {
    let mut root = parse_auth_object(existing)?;
    if root.remove(provider_id).is_none() {
        return Ok(None);
    }
    Ok(Some(Value::Object(root).to_string()))
}

/// Extract provider ids from the auth JSON. Values are never read.
fn provider_ids(existing: Option<&str>) -> Vec<String> {
    let Some(text) = existing else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut ids: Vec<String> = map.keys().cloned().collect();
    ids.sort();
    ids
}

/// Trim and lowercase a user-supplied provider id (OpenCode provider ids are
/// lowercase, so `OpenAI` and `openai` must not become two entries).
fn normalize_provider_id(provider_id: &str) -> String {
    provider_id.trim().to_ascii_lowercase()
}

/// Validate a provider id structurally: non-empty, bounded, and restricted to
/// the charset OpenCode uses. Replaces the old two-provider whitelist so any
/// OpenCode-known provider works without a code change.
fn validate_provider_id(provider_id: &str) -> Result<(), CommandError> {
    let invalid = |reason: &str| CommandError::Validation {
        field: "provider_id".into(),
        reason: reason.into(),
    };
    if provider_id.is_empty() {
        return Err(invalid("Provider id must not be empty"));
    }
    if provider_id.len() > MAX_PROVIDER_ID_LEN {
        return Err(invalid("Provider id is too long"));
    }
    let first = provider_id.chars().next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return Err(invalid("Provider id must start with a letter or digit"));
    }
    if !provider_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(invalid(
            "Provider id may contain only letters, digits, '-', '_' and '.'",
        ));
    }
    Ok(())
}

/// Validate the API key shape: non-empty, bounded, no control characters
/// (control chars would corrupt the JSON or the pipe).
fn validate_api_key(api_key: &str) -> Result<(), CommandError> {
    if api_key.trim().is_empty() {
        return Err(CommandError::Validation {
            field: "api_key".into(),
            reason: "API key must not be empty".into(),
        });
    }
    if api_key.len() > 4096 {
        return Err(CommandError::Validation {
            field: "api_key".into(),
            reason: "API key is too long".into(),
        });
    }
    if api_key.chars().any(|c| c.is_control()) {
        return Err(CommandError::Validation {
            field: "api_key".into(),
            reason: "API key must not contain control characters".into(),
        });
    }
    Ok(())
}

/// Return the provider ids present in the remote OpenCode auth store
/// (`PRESENT` status). Never returns or logs any credential value.
#[tauri::command]
#[tracing::instrument]
pub async fn opencode_auth_status(server_id: String) -> Result<Vec<String>, CommandError> {
    let server = get_server(&server_id)?;
    let existing = read_auth_file(&server).await?;
    Ok(provider_ids(existing.as_deref()))
}

/// Add/update one provider's API key in the remote OpenCode auth store.
/// Merge-only: every other entry (e.g. `anthropic`) is preserved byte-for-
/// byte semantically. The key is written via SSH stdin — never argv, env, a
/// PTY, logs, or error messages.
#[tauri::command]
#[tracing::instrument(skip(api_key))]
pub async fn opencode_auth_set_provider(
    server_id: String,
    provider_id: String,
    api_key: String,
) -> Result<(), CommandError> {
    let provider_id = normalize_provider_id(&provider_id);
    validate_provider_id(&provider_id)?;
    validate_api_key(&api_key)?;
    let server = get_server(&server_id)?;

    let existing = read_auth_file(&server).await?;
    let merged = merge_auth_entry(existing.as_deref(), &provider_id, &api_key)?;

    write_auth_file(&server, &merged, "write").await
}

/// Remove one provider's API key from the remote OpenCode auth store.
/// No secret is involved: only the provider id crosses IPC. Removing an
/// entry that is not present is a no-op.
#[tauri::command]
#[tracing::instrument]
pub async fn opencode_auth_remove_provider(
    server_id: String,
    provider_id: String,
) -> Result<(), CommandError> {
    let provider_id = normalize_provider_id(&provider_id);
    validate_provider_id(&provider_id)?;
    let server = get_server(&server_id)?;

    let existing = read_auth_file(&server).await?;
    let Some(updated) = remove_auth_entry(existing.as_deref(), &provider_id)? else {
        return Ok(());
    };
    write_auth_file(&server, &updated, "remove").await
}

#[cfg(test)]
mod tests {
    use super::{
        merge_auth_entry, normalize_provider_id, provider_ids, remove_auth_entry, validate_api_key,
        validate_provider_id,
    };

    #[test]
    fn merge_preserves_existing_entries() {
        let existing = r#"{"anthropic":{"type":"api","key":"sk-ant-existing"}}"#;
        let merged = merge_auth_entry(Some(existing), "opencode", "zen-key").unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["anthropic"]["key"], "sk-ant-existing");
        assert_eq!(value["opencode"]["type"], "api");
        assert_eq!(value["opencode"]["key"], "zen-key");
    }

    #[test]
    fn merge_creates_store_when_missing() {
        let merged = merge_auth_entry(None, "opencode-go", "go-key").unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["opencode-go"]["key"], "go-key");
        assert!(value.get("anthropic").is_none());
    }

    #[test]
    fn merge_overwrites_only_the_target_entry() {
        let existing =
            r#"{"anthropic":{"type":"api","key":"a"},"opencode":{"type":"api","key":"old"}}"#;
        let merged = merge_auth_entry(Some(existing), "opencode", "new").unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["opencode"]["key"], "new");
        assert_eq!(value["anthropic"]["key"], "a");
    }

    #[test]
    fn merge_rejects_corrupt_existing_file() {
        let err = merge_auth_entry(Some("{not json"), "opencode", "k").unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn merge_rejects_non_object_existing_file() {
        let err = merge_auth_entry(Some("[1,2]"), "opencode", "k").unwrap_err();
        assert!(err.to_string().contains("not a JSON object"));
    }

    #[test]
    fn provider_ids_lists_only_names() {
        let ids = provider_ids(Some(
            r#"{"anthropic":{"type":"api"},"opencode":{"type":"api"}}"#,
        ));
        assert_eq!(ids, vec!["anthropic".to_string(), "opencode".to_string()]);
        assert!(provider_ids(None).is_empty());
        assert!(provider_ids(Some("garbage")).is_empty());
    }

    #[test]
    fn provider_id_shape_is_validated() {
        // Existing providers keep working...
        assert!(validate_provider_id("opencode").is_ok());
        assert!(validate_provider_id("opencode-go").is_ok());
        // ...and future providers are accepted without a code change.
        assert!(validate_provider_id("openai").is_ok());
        assert!(validate_provider_id("some.future_provider-2").is_ok());
        // Arbitrary/injection-shaped ids are still rejected.
        assert!(validate_provider_id("").is_err());
        assert!(validate_provider_id("-leading").is_err());
        assert!(validate_provider_id("has space").is_err());
        assert!(validate_provider_id("has/slash").is_err());
        assert!(validate_provider_id("bad\nkey").is_err());
        assert!(validate_provider_id("$(rm -rf /)").is_err());
        assert!(validate_provider_id(&"x".repeat(101)).is_err());
    }

    #[test]
    fn normalize_provider_id_trims_and_lowercases() {
        assert_eq!(normalize_provider_id("  OpenCode-Go "), "opencode-go");
        assert_eq!(normalize_provider_id("openai"), "openai");
    }

    #[test]
    fn remove_auth_entry_preserves_other_entries() {
        let existing =
            r#"{"anthropic":{"type":"api","key":"a"},"opencode":{"type":"api","key":"z"}}"#;
        let removed = remove_auth_entry(Some(existing), "opencode")
            .unwrap()
            .expect("entry was present");
        let value: serde_json::Value = serde_json::from_str(&removed).unwrap();
        assert!(value.get("opencode").is_none());
        assert_eq!(value["anthropic"]["key"], "a");
    }

    #[test]
    fn remove_auth_entry_reports_missing_entry() {
        let existing = r#"{"anthropic":{"type":"api","key":"a"}}"#;
        assert!(remove_auth_entry(Some(existing), "opencode")
            .unwrap()
            .is_none());
        assert!(remove_auth_entry(None, "opencode").unwrap().is_none());
    }

    #[test]
    fn remove_auth_entry_rejects_corrupt_file() {
        assert!(remove_auth_entry(Some("{not json"), "opencode").is_err());
        assert!(remove_auth_entry(Some("[1,2]"), "opencode").is_err());
    }

    #[test]
    fn api_key_shape_is_validated() {
        assert!(validate_api_key("sk-zen-abc123").is_ok());
        assert!(validate_api_key("   ").is_err());
        assert!(validate_api_key("bad\nkey").is_err());
        assert!(validate_api_key(&"x".repeat(5000)).is_err());
    }
}
